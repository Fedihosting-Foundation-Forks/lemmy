use activitypub_federation::config::Data;
use actix_web::web::Json;
use bcrypt::verify;
use lemmy_api_common::{
  context::LemmyContext,
  person::DeleteAccount,
  send_activity::{ActivityChannel, SendActivityData},
  utils::purge_user_account,
  SuccessResponse,
};
use lemmy_db_schema::source::{login_token::LoginToken, person::Person};
use lemmy_db_views::structs::LocalUserView;
use lemmy_utils::error::{LemmyErrorType, LemmyResult};

#[tracing::instrument(skip(context))]
pub async fn delete_account(
  data: Json<DeleteAccount>,
  context: Data<LemmyContext>,
  local_user_view: LocalUserView,
) -> LemmyResult<Json<SuccessResponse>> {
  // Verify the password
  let valid: bool = verify(
    &data.password,
    &local_user_view.local_user.password_encrypted,
  )
  .unwrap_or(false);
  if !valid {
    Err(LemmyErrorType::IncorrectLogin)?
  }

  if data.delete_content {
    purge_user_account(local_user_view.person.id, &context).await?;
  } else {
    Person::delete_account(&mut context.pool(), local_user_view.person.id).await?;
  }

  LoginToken::invalidate_all(&mut context.pool(), local_user_view.local_user.id).await?;

  ActivityChannel::submit_activity(
    SendActivityData::DeleteUser(local_user_view.person.clone(), data.delete_content),
    &context,
  )
  .await?;

  // FHF anti-spam measure
  if let Some(account_age_threshold) = context
    .settings()
    .fhf_automod_config
    .ban_deleted_persons_created_within_days
  {
    if !data.delete_content
      && local_user_view.person.published
        > lemmy_db_schema::utils::naive_now() - chrono::Days::new(account_age_threshold)
    {
      tracing::info!(
        "[FHF AutoMod][BanLocalDeletedUser] Issuing ban for deletion of recently created user {}",
        local_user_view.person.name
      );

      if let Some(automod_username) = &context.settings().fhf_automod_config.actor_username {
        // keep these here to minimize risk of merge conflicts with upstream
        use lemmy_api_common::utils::remove_user_data;
        use lemmy_db_schema::{
          source::{
            moderator::{ModBan, ModBanForm},
            person::PersonUpdateForm,
          },
          traits::{ApubActor, Crud},
        };
        use lemmy_utils::error::LemmyErrorExt;

        // todo: this might be better to just log and not return an error
        let automod_person =
          Person::read_from_name(&mut context.pool(), automod_username.as_str(), false)
            .await?
            .ok_or(LemmyErrorType::CouldntFindPerson)?;

        // this is more or less the same logic as lemmy_api::local_user::ban_person(), but we can't
        // use it here due to circular dependencies.

        let person = Person::update(
          &mut context.pool(),
          local_user_view.person.id,
          &PersonUpdateForm {
            banned: Some(true),
            ban_expires: Some(None),
            ..Default::default()
          },
        )
        .await
        .with_lemmy_type(LemmyErrorType::CouldntUpdateUser)?;

        remove_user_data(person.id, &context).await?;

        let reason = Some("automod".to_string());

        let form = ModBanForm {
          mod_person_id: automod_person.id,
          other_person_id: local_user_view.person.id,
          reason: reason.clone(),
          banned: Some(true),
          expires: None,
        };

        ModBan::create(&mut context.pool(), &form).await?;

        ActivityChannel::submit_activity(
          SendActivityData::BanFromSite {
            moderator: automod_person,
            banned_user: local_user_view.person.clone(),
            reason,
            remove_data: Some(true),
            ban: true,
            expires: None,
          },
          &context,
        )
        .await?;
      } else {
        tracing::error!("[FHF AutoMod][BanLocalDeletedUser] Unable to ban user: no automod user defined in configuration");
      }
    }
  }

  Ok(Json(SuccessResponse::default()))
}
