//! `POST /scan`, `GET /scan/status`, `POST /scan/cancel` — Verwalter only.

use axum::extract::State;
use axum::http::StatusCode;
use axum::{Json, Router, routing::get, routing::post};

use crate::auth::ApiAdmin;
use crate::error::{ApiError, ApiResult};
use crate::scanner::run::run_scan;
use crate::scanner::state::ScanState;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/scan", post(start))
        .route("/scan/status", get(status))
        .route("/scan/cancel", post(cancel))
}

pub const ALREADY_RUNNING: &str = "Es läuft bereits ein Scan";
pub const NOT_RUNNING: &str = "Es läuft gerade kein Scan";

/// Claims the scanner and runs the scan in the background (legacy `BackgroundTasks`): the
/// response does not wait, `/scan/status` shows the progress. Shared with the web Scan page.
pub fn start_scan(state: &AppState) -> Result<ScanState, ApiError> {
    if !state.scan.try_start() {
        return Err(ApiError::conflict(ALREADY_RUNNING));
    }
    let snapshot = state.scan.snapshot();
    tokio::spawn(run_scan(state.clone()));
    Ok(snapshot)
}

pub fn cancel_scan(state: &AppState) -> Result<ScanState, ApiError> {
    if !state.scan.request_cancel() {
        return Err(ApiError::conflict(NOT_RUNNING));
    }
    Ok(state.scan.snapshot())
}

async fn start(
    State(state): State<AppState>,
    _admin: ApiAdmin,
) -> ApiResult<(StatusCode, Json<ScanState>)> {
    Ok((StatusCode::ACCEPTED, Json(start_scan(&state)?)))
}

async fn status(State(state): State<AppState>, _admin: ApiAdmin) -> Json<ScanState> {
    Json(state.scan.snapshot())
}

async fn cancel(
    State(state): State<AppState>,
    _admin: ApiAdmin,
) -> ApiResult<(StatusCode, Json<ScanState>)> {
    Ok((StatusCode::ACCEPTED, Json(cancel_scan(&state)?)))
}
