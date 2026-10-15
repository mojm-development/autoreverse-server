//! `GET /server/info`.

use axum::extract::State;
use axum::{Json, Router, routing::get};
use serde::Serialize;

use crate::auth::ApiUser;
use crate::error::ApiResult;
use crate::library::queries::{LibraryCounts, library_counts};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/server/info", get(info))
}

#[derive(Serialize)]
struct InfoOut {
    #[serde(flatten)]
    counts: LibraryCounts,
    /// Only admins learn how many accounts exist; `null` for everyone else.
    user_count: Option<i64>,
}

async fn info(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<InfoOut>> {
    let counts = library_counts(&state.db).await?;
    let (is_admin, user_count): (Option<bool>, i64) = sqlx::query_as(
        "SELECT (SELECT is_admin FROM users WHERE id = $1), (SELECT count(*) FROM users)",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;
    let user_count = is_admin.unwrap_or(false).then_some(user_count);
    Ok(Json(InfoOut { counts, user_count }))
}
