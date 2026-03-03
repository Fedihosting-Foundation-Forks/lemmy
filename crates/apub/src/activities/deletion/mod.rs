use crate::{
  activities::{
    community::send_activity_in_community,
    send_lemmy_activity,
    verify_is_public,
    verify_mod_action,
    verify_person,
    verify_person_in_community,
  },
  activity_lists::AnnouncableActivities,
  objects::{
    comment::ApubComment,
    community::ApubCommunity,
    person::ApubPerson,
    post::ApubPost,
    private_message::ApubPrivateMessage,
  },
  protocol::{
    activities::deletion::{delete::Delete, undo_delete::UndoDelete},
    InCommunity,
  },
};
use activitypub_federation::{
  config::Data,
  fetch::object_id::ObjectId,
  kinds::public,
  protocol::verification::{verify_domains_match, verify_urls_match},
  traits::{Actor, Object},
};
use lemmy_api_common::{context::LemmyContext, utils::purge_user_account};
use lemmy_db_schema::{
  source::{
    activity::ActivitySendTargets,
    comment::{Comment, CommentUpdateForm},
    community::{Community, CommunityUpdateForm},
    person::Person,
    post::{Post, PostUpdateForm},
    private_message::{PrivateMessage, PrivateMessageUpdateForm},
  },
  traits::Crud,
};
use lemmy_utils::{error::LemmyResult, LemmyErrorType};
use std::ops::Deref;
use url::Url;

pub mod delete;
pub mod undo_delete;

/// Parameter `reason` being set indicates that this is a removal by a mod. If its unset, this
/// action was done by a normal user.
#[tracing::instrument(skip_all)]
pub(crate) async fn send_apub_delete_in_community(
  actor: Person,
  community: Community,
  object: DeletableObjects,
  reason: Option<String>,
  deleted: bool,
  context: &Data<LemmyContext>,
) -> LemmyResult<()> {
  let actor = ApubPerson::from(actor);
  let is_mod_action = reason.is_some();
  let activity = if deleted {
    let delete = Delete::new(&actor, object, public(), Some(&community), reason, context)?;
    AnnouncableActivities::Delete(delete)
  } else {
    let undo = UndoDelete::new(&actor, object, public(), Some(&community), reason, context)?;
    AnnouncableActivities::UndoDelete(undo)
  };
  send_activity_in_community(
    activity,
    &actor,
    &community.into(),
    ActivitySendTargets::empty(),
    is_mod_action,
    context,
  )
  .await
}

#[tracing::instrument(skip_all)]
pub(crate) async fn send_apub_delete_private_message(
  actor: &ApubPerson,
  pm: PrivateMessage,
  deleted: bool,
  context: Data<LemmyContext>,
) -> LemmyResult<()> {
  let recipient_id = pm.recipient_id;
  let recipient: ApubPerson = Person::read(&mut context.pool(), recipient_id)
    .await?
    .ok_or(LemmyErrorType::CouldntFindPerson)?
    .into();

  let deletable = DeletableObjects::PrivateMessage(pm.into());
  let inbox = ActivitySendTargets::to_inbox(recipient.shared_inbox_or_inbox());
  if deleted {
    let delete: Delete = Delete::new(actor, deletable, recipient.id(), None, None, &context)?;
    send_lemmy_activity(&context, delete, actor, inbox, true).await?;
  } else {
    let undo = UndoDelete::new(actor, deletable, recipient.id(), None, None, &context)?;
    send_lemmy_activity(&context, undo, actor, inbox, true).await?;
  };
  Ok(())
}

pub async fn send_apub_delete_user(
  person: Person,
  remove_data: bool,
  context: Data<LemmyContext>,
) -> LemmyResult<()> {
  let person: ApubPerson = person.into();

  let deletable = DeletableObjects::Person(person.clone());
  let mut delete: Delete = Delete::new(&person, deletable, public(), None, None, &context)?;
  delete.remove_data = Some(remove_data);

  let inboxes = ActivitySendTargets::to_all_instances();

  send_lemmy_activity(&context, delete, &person, inboxes, true).await?;
  Ok(())
}

pub enum DeletableObjects {
  Community(ApubCommunity),
  Person(ApubPerson),
  Comment(ApubComment),
  Post(ApubPost),
  PrivateMessage(ApubPrivateMessage),
}

impl DeletableObjects {
  #[tracing::instrument(skip_all)]
  pub(crate) async fn read_from_db(
    ap_id: &Url,
    context: &Data<LemmyContext>,
  ) -> LemmyResult<DeletableObjects> {
    if let Some(c) = ApubCommunity::read_from_id(ap_id.clone(), context).await? {
      return Ok(DeletableObjects::Community(c));
    }
    if let Some(p) = ApubPerson::read_from_id(ap_id.clone(), context).await? {
      return Ok(DeletableObjects::Person(p));
    }
    if let Some(p) = ApubPost::read_from_id(ap_id.clone(), context).await? {
      return Ok(DeletableObjects::Post(p));
    }
    if let Some(c) = ApubComment::read_from_id(ap_id.clone(), context).await? {
      return Ok(DeletableObjects::Comment(c));
    }
    if let Some(p) = ApubPrivateMessage::read_from_id(ap_id.clone(), context).await? {
      return Ok(DeletableObjects::PrivateMessage(p));
    }
    Err(diesel::NotFound.into())
  }

  pub(crate) fn id(&self) -> Url {
    match self {
      DeletableObjects::Community(c) => c.id(),
      DeletableObjects::Person(p) => p.id(),
      DeletableObjects::Comment(c) => c.ap_id.clone().into(),
      DeletableObjects::Post(p) => p.ap_id.clone().into(),
      DeletableObjects::PrivateMessage(p) => p.ap_id.clone().into(),
    }
  }
}

#[tracing::instrument(skip_all)]
pub(in crate::activities) async fn verify_delete_activity(
  activity: &Delete,
  is_mod_action: bool,
  context: &Data<LemmyContext>,
) -> LemmyResult<()> {
  let object = DeletableObjects::read_from_db(activity.object.id(), context).await?;
  match object {
    DeletableObjects::Community(community) => {
      verify_is_public(&activity.to, &[])?;
      if community.local {
        // can only do this check for local community, in remote case it would try to fetch the
        // deleted community (which fails)
        verify_person_in_community(&activity.actor, &community, context).await?;
      }
      // community deletion is always a mod (or admin) action
      verify_mod_action(&activity.actor, &community, context).await?;
    }
    DeletableObjects::Person(person) => {
      verify_is_public(&activity.to, &[])?;
      verify_person(&activity.actor, context).await?;
      verify_urls_match(person.actor_id.inner(), activity.object.id())?;
    }
    DeletableObjects::Post(p) => {
      verify_is_public(&activity.to, &[])?;
      verify_delete_post_or_comment(
        &activity.actor,
        &p.ap_id.clone().into(),
        &activity.community(context).await?,
        is_mod_action,
        context,
      )
      .await?;
    }
    DeletableObjects::Comment(c) => {
      verify_is_public(&activity.to, &[])?;
      verify_delete_post_or_comment(
        &activity.actor,
        &c.ap_id.clone().into(),
        &activity.community(context).await?,
        is_mod_action,
        context,
      )
      .await?;
    }
    DeletableObjects::PrivateMessage(_) => {
      verify_person(&activity.actor, context).await?;
      verify_domains_match(activity.actor.inner(), activity.object.id())?;
    }
  }
  Ok(())
}

#[tracing::instrument(skip_all)]
async fn verify_delete_post_or_comment(
  actor: &ObjectId<ApubPerson>,
  object_id: &Url,
  community: &ApubCommunity,
  is_mod_action: bool,
  context: &Data<LemmyContext>,
) -> LemmyResult<()> {
  verify_person_in_community(actor, community, context).await?;
  if is_mod_action {
    verify_mod_action(actor, community, context).await?;
  } else {
    // domain of post ap_id and post.creator ap_id are identical, so we just check the former
    verify_domains_match(actor.inner(), object_id)?;
  }
  Ok(())
}

/// Write deletion or restoring of an object to the database, and send websocket message.
#[tracing::instrument(skip_all)]
async fn receive_delete_action(
  object: &Url,
  actor: &ObjectId<ApubPerson>,
  deleted: bool,
  do_purge_user_account: Option<bool>,
  context: &Data<LemmyContext>,
) -> LemmyResult<()> {
  match DeletableObjects::read_from_db(object, context).await? {
    DeletableObjects::Community(community) => {
      if community.local {
        let mod_: Person = actor.dereference(context).await?.deref().clone();
        let object = DeletableObjects::Community(community.clone());
        let c: Community = community.deref().clone();
        send_apub_delete_in_community(mod_, c, object, None, true, context).await?;
      }

      Community::update(
        &mut context.pool(),
        community.id,
        &CommunityUpdateForm {
          deleted: Some(deleted),
          ..Default::default()
        },
      )
      .await?;
    }
    DeletableObjects::Person(person) => {
      if do_purge_user_account.unwrap_or(false) {
        purge_user_account(person.id, context).await?;
      } else {
        Person::delete_account(&mut context.pool(), person.id).await?;

        // FHF anti-spam measure
        if let Some(account_age_threshold) = context
          .settings()
          .fhf_automod_config
          .ban_deleted_persons_created_within_days
        {
          if person.published
            > lemmy_db_schema::utils::naive_now() - chrono::Days::new(account_age_threshold)
          {
            tracing::info!(
        "[FHF AutoMod][BanFederatedDeletedUser] Issuing ban for deletion of recently created user {}",
        person.actor_id
      );

            if let Some(automod_username) = &context.settings().fhf_automod_config.actor_username {
              // keep these here to minimize risk of merge conflicts with upstream
              use lemmy_api_common::{
                community::BanFromCommunity,
                send_activity::{ActivityChannel, SendActivityData},
                utils::remove_user_data,
              };
              use lemmy_db_schema::{
                source::{
                  community::{
                    CommunityFollower,
                    CommunityFollowerForm,
                    CommunityPersonBan,
                    CommunityPersonBanForm,
                  },
                  moderator::{ModBan, ModBanForm, ModBanFromCommunity, ModBanFromCommunityForm},
                  person::PersonUpdateForm,
                },
                traits::{Bannable, Followable},
              };
              use lemmy_db_views::structs::LocalUserView;
              use lemmy_utils::error::LemmyErrorExt;

              // todo: this might be better to just log and not return an error
              let automod_local_user_view =
                LocalUserView::read_from_name(&mut context.pool(), automod_username.as_str())
                  .await?
                  .ok_or(LemmyErrorType::CouldntFindPerson)?;

              let person = Person::update(
                &mut context.pool(),
                person.id,
                &PersonUpdateForm {
                  banned: Some(true),
                  ban_expires: Some(None),
                  ..Default::default()
                },
              )
              .await
              .with_lemmy_type(LemmyErrorType::CouldntUpdateUser)?;

              remove_user_data(person.id, context).await?;

              let reason = Some("automod".to_string());

              let form = ModBanForm {
                mod_person_id: automod_local_user_view.person.id,
                other_person_id: person.id,
                reason: reason.clone(),
                banned: Some(true),
                expires: None,
              };

              ModBan::create(&mut context.pool(), &form).await?;

              // this is basically lemmy_api::ban_nonlocal_user_from_local_communities()
              let ids = Person::list_local_community_ids(&mut context.pool(), person.id).await?;

              for community_id in ids {
                // Ban them from our local communities
                let community_user_ban_form = CommunityPersonBanForm {
                  community_id,
                  person_id: person.id,
                  expires: None,
                };

                // Ignore all errors for these
                CommunityPersonBan::ban(&mut context.pool(), &community_user_ban_form)
                  .await
                  .ok();

                // Also unsubscribe them from the community, if they are subscribed
                let community_follower_form = CommunityFollowerForm {
                  community_id,
                  person_id: person.id,
                  pending: false,
                };

                CommunityFollower::unfollow(&mut context.pool(), &community_follower_form)
                  .await
                  .ok();

                // Mod tables
                let form = ModBanFromCommunityForm {
                  mod_person_id: automod_local_user_view.person.id,
                  other_person_id: person.id,
                  community_id,
                  reason: reason.clone(),
                  banned: Some(true),
                  expires: None,
                };

                ModBanFromCommunity::create(&mut context.pool(), &form).await?;

                // Federate the ban from community
                let ban_from_community = BanFromCommunity {
                  community_id,
                  person_id: person.id,
                  ban: true,
                  reason: reason.clone(),
                  remove_data: Some(true),
                  expires: None,
                };

                ActivityChannel::submit_activity(
                  SendActivityData::BanFromCommunity {
                    moderator: automod_local_user_view.person.clone(),
                    community_id,
                    target: person.clone(),
                    data: ban_from_community,
                  },
                  context,
                )
                .await?;
              }
            } else {
              tracing::error!("[FHF AutoMod][BanFederatedDeletedUser] Unable to ban user: no automod user defined in configuration");
            }
          }
        }
      }
    }
    DeletableObjects::Post(post) => {
      if deleted != post.deleted {
        Post::update(
          &mut context.pool(),
          post.id,
          &PostUpdateForm {
            deleted: Some(deleted),
            ..Default::default()
          },
        )
        .await?;
      }
    }
    DeletableObjects::Comment(comment) => {
      if deleted != comment.deleted {
        Comment::update(
          &mut context.pool(),
          comment.id,
          &CommentUpdateForm {
            deleted: Some(deleted),
            ..Default::default()
          },
        )
        .await?;
      }
    }
    DeletableObjects::PrivateMessage(pm) => {
      PrivateMessage::update(
        &mut context.pool(),
        pm.id,
        &PrivateMessageUpdateForm {
          deleted: Some(deleted),
          ..Default::default()
        },
      )
      .await?;
    }
  }
  Ok(())
}
