//! `GET /me`.

use axum::extract::State;
use axum::{Json, Router, routing::get};
use serde::Serialize;

use crate::auth::ApiUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/me", get(me))
}

#[derive(Serialize, sqlx::FromRow)]
struct MeOut {
    id: i32,
    name: String,
    is_admin: bool,
}

async fn me(State(state): State<AppState>, user: ApiUser) -> ApiResult<Json<MeOut>> {
    let row: Option<MeOut> = sqlx::query_as("SELECT id, name, is_admin FROM users WHERE id = $1")
        .bind(user.id)
        .fetch_optional(&state.db)
        .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("Unbekannter Nutzer"))
}
