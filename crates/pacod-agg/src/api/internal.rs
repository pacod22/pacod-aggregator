//! The internal job API, for the scraper. Its token opens nothing else.

use axum::extract::{FromRequestParts, Path, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::routing::post;
use axum::{Json, Router};
use pacod_agg_jobs::extraction::PageReading;
use pacod_agg_jobs::jobs::{
    HeartbeatReply, HeartbeatRequest, INTERNAL_TOKEN_HEADER, JobResult, LeaseReply, LeaseRequest, ReleaseRequest,
    ScanReport,
};
use serde_json::Value;
use subtle::ConstantTimeEq;

use super::{ApiError, ApiResult, AppState};
use crate::{jobs, readings};

/// A caller that presented the internal token.
struct Scraper;

impl FromRequestParts<AppState> for Scraper {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let given = parts.headers.get(INTERNAL_TOKEN_HEADER).and_then(|v| v.to_str().ok()).unwrap_or_default();
        match &state.config.internal_token {
            Some(token) if bool::from(given.as_bytes().ct_eq(token.as_bytes())) => Ok(Scraper),
            _ => Err(ApiError::new(StatusCode::UNAUTHORIZED, "unauthorized", "this endpoint is for the scraper")),
        }
    }
}

async fn lease(State(st): State<AppState>, _: Scraper, Json(req): Json<LeaseRequest>) -> ApiResult<Json<LeaseReply>> {
    jobs::lease(&st, &req).await
}

async fn release(State(st): State<AppState>, _: Scraper, Json(req): Json<ReleaseRequest>) -> ApiResult<StatusCode> {
    jobs::release(&st, &req).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn heartbeat(
    State(st): State<AppState>,
    _: Scraper,
    Path(run_id): Path<String>,
    Json(req): Json<HeartbeatRequest>,
) -> ApiResult<Json<HeartbeatReply>> {
    jobs::heartbeat(&st, &run_id, &req).await
}

async fn scan(
    State(st): State<AppState>,
    _: Scraper,
    Path(run_id): Path<String>,
    Json(report): Json<ScanReport>,
) -> ApiResult<Json<Value>> {
    jobs::scan_report(&st, &run_id, &report).await
}

async fn reading(
    State(st): State<AppState>,
    _: Scraper,
    Path(run_id): Path<String>,
    Json(reading): Json<PageReading>,
) -> ApiResult<Json<Value>> {
    readings::reading(&st, &run_id, &reading).await
}

async fn result(
    State(st): State<AppState>,
    _: Scraper,
    Path(run_id): Path<String>,
    Json(res): Json<JobResult>,
) -> ApiResult<Json<Value>> {
    jobs::finish(&st, &run_id, &res).await
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/jobs/lease", post(lease))
        .route("/jobs/release", post(release))
        .route("/jobs/{run}/heartbeat", post(heartbeat))
        .route("/jobs/{run}/readings", post(reading))
        .route("/jobs/{run}/scan", post(scan))
        .route("/jobs/{run}/result", post(result))
}
