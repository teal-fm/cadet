use crate::ctx::Context;
use axum::{Extension, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use types::fm::teal::alpha::actor::defs::ProfileViewData;

#[derive(Serialize)]
pub struct GetProfileResponse {
    profile: ProfileViewData,
}
#[derive(Deserialize)]
pub struct GetProfileQuery {
    pub identity: Option<String>,
}

pub async fn get_actor(
    Extension(ctx): Extension<Context>,
    axum::extract::Query(query): axum::extract::Query<GetProfileQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let repo = &ctx.db; // assuming ctx.db is Box<dyn ActorProfileRepo + Send + Sync>
    let identity = &query.identity;

    if identity.is_none() {
        return Err((StatusCode::BAD_REQUEST, "identity is required".to_string()));
    }

    match repo
        .get_actor_profile(identity.as_ref().expect("identity is not none").as_str())
        .await
    {
        Ok(Some(profile)) => Ok(axum::Json(GetProfileResponse { profile })),
        Ok(None) => Err((StatusCode::NOT_FOUND, "Profile not found".to_string())),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
