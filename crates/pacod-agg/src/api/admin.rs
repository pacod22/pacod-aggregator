//! The admin API, for the console.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use pacod_agg_jobs::jobs::{JOB_CHECK, JOB_SCAN};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::auth::{Manager, Principal, Reader};
use super::{ApiError, ApiResult, AppState};
use crate::jobs::{self, StatusView};
use crate::store::jobs::JobRun;

/// Who is calling, and what they may do.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Session {
    pub id: String,
    pub display: String,
    /// `user` or `service`.
    pub kind: String,
    pub permissions: Vec<String>,
}

async fn session(p: Principal) -> Json<Session> {
    let permissions = p.permissions().into_iter().map(str::to_owned).collect();
    Json(Session { id: p.id, display: p.display, kind: p.kind.to_owned(), permissions })
}

async fn status(State(st): State<AppState>, _: Reader) -> ApiResult<Json<StatusView>> {
    Ok(Json(jobs::status(&st).await?))
}

#[derive(Debug, Default, Deserialize)]
struct RunsQuery {
    job: Option<String>,
    limit: Option<i64>,
}

/// Runs of the check and the scan, newest first.
async fn runs(State(st): State<AppState>, _: Reader, Query(q): Query<RunsQuery>) -> ApiResult<Json<Vec<JobRun>>> {
    if let Some(job) = q.job.as_deref().filter(|j| *j != JOB_CHECK && *j != JOB_SCAN) {
        return Err(ApiError::invalid(format!("job \"{job}\": check or scan")));
    }
    Ok(Json(st.store.job_runs(q.job.as_deref(), q.limit.unwrap_or(30).clamp(1, 200)).await?))
}

/// Asks for a check of every page. It starts when the scraper next asks for work, within a minute.
async fn run_check(State(st): State<AppState>, _: Manager) -> ApiResult<StatusCode> {
    st.store.job_request_run(JOB_CHECK, None).await?;
    Ok(StatusCode::ACCEPTED)
}

/// Asks for a scan of every vendor's site.
async fn run_scan(State(st): State<AppState>, _: Manager) -> ApiResult<StatusCode> {
    st.store.job_request_run(JOB_SCAN, None).await?;
    Ok(StatusCode::ACCEPTED)
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/session", get(session))
        .route("/status", get(status))
        .route("/runs", get(runs))
        .route("/runs/check", post(run_check))
        .route("/runs/scan", post(run_scan))
        .merge(super::resources::routes())
}
