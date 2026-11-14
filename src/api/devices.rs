//! Remote control (`/me/devices…`): the event stream to a device, what it plays, commands and
//! handing playback over. The rules live in [`crate::devices`]; this file parses and answers.
//! The contract for client authors is in the README ("Remote control").

use axum::body::Body;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

use super::validate::{ApiJson, ApiQuery, QueryMap};
use crate::auth::ApiUser;
use crate::auth::tokens::token_digest;
use crate::devices::hub::{DevicesOut, SendError, TransferError};
use crate::devices::{self, Command, Kind, clean_device_name, valid_device_id};
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

const DEVICES_OFF: &str = "Die Fernsteuerung ist auf diesem Server abgeschaltet";
const INVALID_ID: &str = "Ungültige Gerätekennung";
const NOT_CONNECTED: &str = "Das Gerät ist nicht verbunden";
const TOO_MANY_COMMANDS: &str = "Zu viele Befehle an dieses Gerät";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/me/devices", get(list))
        .route("/me/devices/events", get(events))
        .route("/me/devices/{device_id}", delete(sign_off))
        .route("/me/devices/{device_id}/state", put(report))
        .route("/me/devices/{device_id}/commands", post(command))
        .route("/me/devices/{device_id}/transfer", post(transfer))
}

/// The caller of a remote-control route: 404 on a server with the feature switched off — before
/// the login is looked at, so such a server answers as if the routes did not exist — else an
/// authenticated user.
struct DevicesUser(ApiUser);

impl FromRequestParts<AppState> for DevicesUser {
    type Rejection = ApiError;
    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        if !state.settings.get().devices {
            return Err(ApiError::not_found(DEVICES_OFF));
        }
        ApiUser::from_request_parts(parts, state)
            .await
            .map(DevicesUser)
    }
}

fn checked_id(id: &str) -> ApiResult<&str> {
    if valid_device_id(id) {
        Ok(id)
    } else {
        Err(ApiError::unprocessable(INVALID_ID))
    }
}

async fn list(State(state): State<AppState>, DevicesUser(user): DevicesUser) -> Json<DevicesOut> {
    Json(state.devices.list(user.id))
}

/// `GET /me/devices/events?device_id=…&name=…&kind=…`: server-sent events for as long as the
/// device is there. One long GET is what every platform can do without a library and what gets
/// through a reverse proxy; everything the device says goes over ordinary requests.
async fn events(
    State(state): State<AppState>,
    DevicesUser(user): DevicesUser,
    ApiQuery(query): ApiQuery<QueryMap>,
) -> ApiResult<Response> {
    let id = checked_id(query.get("device_id").map_or("", String::as_str))?;
    let name = query
        .get("name")
        .and_then(|raw| clean_device_name(raw))
        .ok_or_else(|| ApiError::unprocessable("name muss 1 bis 60 Zeichen lang sein"))?;
    let kind = query
        .get("kind")
        .and_then(|raw| Kind::parse(raw))
        .ok_or_else(|| {
            ApiError::unprocessable("kind muss desktop, phone, tablet, web oder car sein")
        })?;
    let digest = token_digest(&user.token);
    let connection = state
        .devices
        .connect(user.id, id, name, kind, digest.clone());
    watch_token(
        &state,
        user.id,
        id.to_owned(),
        connection.generation,
        digest,
    );

    let mut response = Response::new(Body::from_stream(connection.events));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    // nginx buffers a proxied response unless told otherwise; the events would arrive in lumps
    // or not at all.
    headers.insert("x-accel-buffering", HeaderValue::from_static("no"));
    Ok(response)
}

/// A stream outlives the request that authenticated it, so its token is looked up again every
/// now and then: a password change, a deleted account or an expired token end the stream within
/// `Timing::recheck`. (A logout ends it at once, see `api::auth` and `web::login`.)
fn watch_token(state: &AppState, user: i32, id: String, generation: u64, digest: String) {
    let (db, hub) = (state.db.clone(), state.devices.clone());
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(hub.timing().recheck).await;
            if !hub.is_current(user, &id, generation) {
                return;
            }
            let known: Result<Option<i32>, _> =
                sqlx::query_scalar("SELECT user_id FROM auth_tokens WHERE value = $1")
                    .bind(&digest)
                    .fetch_optional(&db)
                    .await;
            // A database that does not answer is no verdict on the token.
            if matches!(known, Ok(None)) {
                hub.close(user, &id, generation);
                return;
            }
        }
    });
}

async fn report(
    State(state): State<AppState>,
    DevicesUser(user): DevicesUser,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<devices::State>,
) -> ApiResult<StatusCode> {
    let id = checked_id(&id)?;
    body.validate().map_err(ApiError::unprocessable)?;
    if !state.devices.report(user.id, id, body) {
        // Name and kind come with the stream; a state without them could not be listed.
        return Err(ApiError::conflict(NOT_CONNECTED));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct CommandIn {
    command: String,
    #[serde(default)]
    value: Value,
    from: String,
}

fn throttled() -> ApiError {
    ApiError::new(StatusCode::TOO_MANY_REQUESTS, TOO_MANY_COMMANDS).with_retry_after(1)
}

async fn command(
    State(state): State<AppState>,
    DevicesUser(user): DevicesUser,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<CommandIn>,
) -> ApiResult<StatusCode> {
    let id = checked_id(&id)?;
    let command =
        Command::parse(&body.command, &body.value, &body.from).map_err(ApiError::unprocessable)?;
    // The target is looked up among the caller's own devices only: another account's device is
    // as unknown as one that does not exist.
    match state.devices.command(user.id, id, &command) {
        Ok(()) => Ok(StatusCode::ACCEPTED),
        Err(SendError::Offline) => Err(ApiError::conflict(NOT_CONNECTED)),
        Err(SendError::Throttled) => Err(throttled()),
    }
}

fn play_by_default() -> bool {
    true
}

#[derive(Deserialize)]
struct TransferIn {
    /// `null` (or left out): from whatever device is the active one.
    from: Option<String>,
    #[serde(default = "play_by_default")]
    play: bool,
}

async fn transfer(
    State(state): State<AppState>,
    DevicesUser(user): DevicesUser,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<TransferIn>,
) -> ApiResult<StatusCode> {
    let id = checked_id(&id)?;
    let from = body.from.as_deref();
    if from.is_some_and(|from| !valid_device_id(from)) {
        return Err(ApiError::unprocessable(
            "from ist keine gültige Gerätekennung",
        ));
    }
    match state.devices.transfer(user.id, id, from, body.play) {
        Ok(()) => Ok(StatusCode::ACCEPTED),
        Err(TransferError::TargetOffline) => Err(ApiError::conflict(NOT_CONNECTED)),
        Err(TransferError::NothingToTake) => Err(ApiError::conflict(
            "Es läuft nichts, das übernommen werden könnte",
        )),
        Err(TransferError::AlreadyThere) => Err(ApiError::conflict(
            "Die Wiedergabe läuft bereits auf diesem Gerät",
        )),
        Err(TransferError::Throttled) => Err(throttled()),
    }
}

async fn sign_off(
    State(state): State<AppState>,
    DevicesUser(user): DevicesUser,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    state.devices.remove(user.id, checked_id(&id)?);
    Ok(StatusCode::NO_CONTENT)
}
