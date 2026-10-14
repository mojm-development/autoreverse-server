//! `GET|POST /users`, `PATCH|DELETE /users/{id}` — admins only (legacy `routes/users/**`).
//!
//! Legacy knew only the role in `PATCH` and no `DELETE`; setting another user's password and
//! deleting users are new, with the same last-admin protection as demoting.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::get, routing::patch};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};

use super::validate::ApiJson;
use crate::auth::ApiAdmin;
use crate::auth::directory::{
    DirectoryError, add_user, delete_user, find_user, list_users, update_user, valid_name,
    valid_password,
};
use crate::error::{ApiError, ApiResult};
use crate::serialize::{iso, iso_opt};
use crate::state::AppState;

pub const UNKNOWN_USER: &str = "Unbekannter Nutzer";
pub const NAME_INVALID: &str = "name muss 1–100 Zeichen haben";
pub const PASSWORD_INVALID: &str = "password muss 8–200 Zeichen haben";
pub const NAME_TAKEN: &str = "Nutzername ist bereits vergeben";
pub const LAST_ADMIN_DEMOTE: &str = "Der letzte Verwalter kann nicht herabgestuft werden";
pub const LAST_ADMIN_DELETE: &str = "Der letzte Verwalter kann nicht gelöscht werden";
pub const SELF_DELETE: &str = "Das eigene Konto kann nicht gelöscht werden";
pub const IS_ADMIN_INVALID: &str = "is_admin muss true oder false sein";
pub const NOTHING_TO_CHANGE: &str = "is_admin oder password angeben";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users", get(list).post(create))
        .route("/users/{id}", patch(update).delete(remove))
}

#[derive(Serialize)]
struct UserOut {
    id: i32,
    name: String,
    is_admin: bool,
    #[serde(serialize_with = "iso")]
    created_at: DateTime<Utc>,
    #[serde(serialize_with = "iso_opt")]
    last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct UsersOut {
    users: Vec<UserOut>,
}

/// What `POST` and `PATCH` answer (legacy: `{id, name, is_admin}`).
#[derive(Serialize)]
struct UserRefOut {
    id: i32,
    name: String,
    is_admin: bool,
}

async fn list(State(state): State<AppState>, _admin: ApiAdmin) -> ApiResult<Json<UsersOut>> {
    let users = list_users(&state.db)
        .await?
        .into_iter()
        .map(|u| UserOut {
            id: u.id,
            name: u.name,
            is_admin: u.is_admin,
            created_at: u.created_at,
            last_seen_at: u.last_seen_at,
        })
        .collect();
    Ok(Json(UsersOut { users }))
}

/// An optional boolean field: absent/`null` is `None`, anything but a boolean is a 422.
fn optional_bool(body: &Map<String, Value>, key: &str) -> ApiResult<Option<bool>> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(ApiError::unprocessable(IS_ADMIN_INVALID)),
    }
}

fn map_directory_error(e: DirectoryError, last_admin: &'static str) -> ApiError {
    match e {
        DirectoryError::NotFound => ApiError::not_found(UNKNOWN_USER),
        DirectoryError::LastAdmin => ApiError::conflict(last_admin),
        DirectoryError::SelfDelete => ApiError::conflict(SELF_DELETE),
        DirectoryError::NameTaken => ApiError::conflict(NAME_TAKEN),
        DirectoryError::Db(e) => e.into(),
    }
}

/// `{name, password, is_admin?}` → 201 `{id, name, is_admin}`. The first user ever is admin
/// whatever `is_admin` says.
async fn create(
    State(state): State<AppState>,
    _admin: ApiAdmin,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Response> {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| valid_name(n))
        .ok_or_else(|| ApiError::unprocessable(NAME_INVALID))?;
    let password = body
        .get("password")
        .and_then(Value::as_str)
        .filter(|p| valid_password(p))
        .ok_or_else(|| ApiError::unprocessable(PASSWORD_INVALID))?;
    let is_admin = optional_bool(&body, "is_admin")?.unwrap_or(false);
    let id = add_user(&state.db, name, password, is_admin)
        .await
        .map_err(|e| map_directory_error(e, LAST_ADMIN_DEMOTE))?;
    let row = find_user(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(UNKNOWN_USER))?;
    Ok((
        StatusCode::CREATED,
        Json(UserRefOut {
            id,
            name: row.name,
            is_admin: row.is_admin,
        }),
    )
        .into_response())
}

fn path_id(raw: &str) -> ApiResult<i32> {
    raw.parse().map_err(|_| ApiError::not_found(UNKNOWN_USER))
}

/// `{is_admin?, password?}` → `{id, name, is_admin}`. Demoting the last admin is 409; a new
/// password ends all of the user's sessions except the caller's own.
async fn update(
    State(state): State<AppState>,
    admin: ApiAdmin,
    Path(raw_id): Path<String>,
    ApiJson(body): ApiJson<Map<String, Value>>,
) -> ApiResult<Json<UserRefOut>> {
    let id = path_id(&raw_id)?;
    let is_admin = optional_bool(&body, "is_admin")?;
    let password = match body.get("password") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_str()
                .filter(|p| valid_password(p))
                .ok_or_else(|| ApiError::unprocessable(PASSWORD_INVALID))?,
        ),
    };
    if is_admin.is_none() && password.is_none() {
        return Err(ApiError::unprocessable(NOTHING_TO_CHANGE));
    }
    let keep = (id == admin.id).then_some(admin.token.as_str());
    update_user(&state.db, id, is_admin, password, keep)
        .await
        .map_err(|e| map_directory_error(e, LAST_ADMIN_DEMOTE))?;
    let row = find_user(&state.db, id)
        .await?
        .ok_or_else(|| ApiError::not_found(UNKNOWN_USER))?;
    Ok(Json(UserRefOut {
        id,
        name: row.name,
        is_admin: row.is_admin,
    }))
}

/// 204. Not oneself, not the last admin.
async fn remove(
    State(state): State<AppState>,
    admin: ApiAdmin,
    Path(raw_id): Path<String>,
) -> ApiResult<StatusCode> {
    let id = path_id(&raw_id)?;
    delete_user(&state.db, Some(admin.id), id)
        .await
        .map_err(|e| map_directory_error(e, LAST_ADMIN_DELETE))?;
    Ok(StatusCode::NO_CONTENT)
}
