//! `GET /tracks/{id}`. Streaming joins with playback (slice 4), `PATCH` with metadata editing
//! (slice 8).

use axum::extract::{Path, State};
use axum::{Json, Router, routing::get};

use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::library::queries;
use crate::serialize::TrackDetail;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/tracks/{id}", get(detail))
}

async fn detail(
    State(state): State<AppState>,
    _user: ApiUser,
    Path(raw_id): Path<String>,
) -> ApiResult<Json<TrackDetail>> {
    let not_found = || ApiError::not_found("Unbekannter Track");
    let id: i32 = raw_id.parse().map_err(|_| not_found())?;
    let row = queries::track(&state.db, id).await?.ok_or_else(not_found)?;
    Ok(Json(TrackDetail::from(&row)))
}
