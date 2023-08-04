use actix_web::web::{Data, Json, Query};
use lemmy_api_common::{
  claims::Claims,
  context::LemmyContext,
  sensitive::Sensitive,
  site::{GetSite, GetSiteResponse, MyUserInfo},
  utils::{check_user_valid},
};
use lemmy_db_schema::{
  source::{
    actor_language::{LocalUserLanguage, SiteLanguage},
    language::Language,
    tagline::Tagline,
  },
};
use lemmy_db_views::structs::{CustomEmojiView, LocalUserView, SiteView};
use lemmy_db_views_actor::structs::{
  CommunityBlockView,
  CommunityFollowerView,
  CommunityModeratorView,
  PersonBlockView,
  PersonView,
};
use lemmy_utils::{
  error::{LemmyError, LemmyErrorExt, LemmyErrorType},
  version,
};

#[tracing::instrument(skip(context))]
pub async fn get_site(
  data: Query<GetSite>,
  context: Data<LemmyContext>,
) -> Result<Json<GetSiteResponse>, LemmyError> {
  let site_view = SiteView::read_local(&mut context.pool()).await?;

  let admins = PersonView::admins(&mut context.pool()).await?;

  // Build the local user
  let my_user = if let Some(local_user_view) =
    local_user_settings_view_from_jwt_opt(data.auth.as_ref(), &context).await
  {
    let person_id = local_user_view.person.id;
    let local_user_id = local_user_view.local_user.id;

    let follows = CommunityFollowerView::for_person(&mut context.pool(), person_id)
      .await
      .with_lemmy_type(LemmyErrorType::SystemErrLogin)?;

    let person_id = local_user_view.person.id;
    let community_blocks = CommunityBlockView::for_person(&mut context.pool(), person_id)
      .await
      .with_lemmy_type(LemmyErrorType::SystemErrLogin)?;

    let person_id = local_user_view.person.id;
    let person_blocks = PersonBlockView::for_person(&mut context.pool(), person_id)
      .await
      .with_lemmy_type(LemmyErrorType::SystemErrLogin)?;

    let moderates = CommunityModeratorView::for_person(&mut context.pool(), person_id)
      .await
      .with_lemmy_type(LemmyErrorType::SystemErrLogin)?;

    let discussion_languages = LocalUserLanguage::read(&mut context.pool(), local_user_id)
      .await
      .with_lemmy_type(LemmyErrorType::SystemErrLogin)?;

    Some(MyUserInfo {
      local_user_view,
      follows,
      moderates,
      community_blocks,
      person_blocks,
      discussion_languages,
    })
  } else {
    None
  };

  let all_languages = Language::read_all(&mut context.pool()).await?;
  let discussion_languages = SiteLanguage::read_local_raw(&mut context.pool()).await?;
  let taglines = Tagline::get_all(&mut context.pool(), site_view.local_site.id).await?;
  let custom_emojis =
    CustomEmojiView::get_all(&mut context.pool(), site_view.local_site.id).await?;

  Ok(Json(GetSiteResponse {
    site_view,
    admins,
    version: version::VERSION.to_string(),
    my_user,
    all_languages,
    discussion_languages,
    taglines,
    custom_emojis,
  }))
}

#[tracing::instrument(skip_all)]
async fn local_user_settings_view_from_jwt_opt(
  jwt: Option<&Sensitive<String>>,
  context: &LemmyContext,
) -> Option<LocalUserView> {
  match jwt {
    Some(jwt) => {
      let local_user_id = Claims::validate(jwt.as_ref(), context).await.ok()?;
      let local_user_view = LocalUserView::read(&mut context.pool(), local_user_id)
        .await
        .ok()?;
      check_user_valid(
        local_user_view.person.banned,
        local_user_view.person.ban_expires,
        local_user_view.person.deleted,
      )
      .ok()?;

      Some(local_user_view)
    }
    None => None,
  }
}
