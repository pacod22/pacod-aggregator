//! The admin API, for the console.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use pacod_agg_jobs::jobs::{JOB_CHECK, JOB_SCAN};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::auth::{Manager, Principal, Reader};
use super::{ApiError, ApiResult, AppState};
use crate::catalog::model_of;
use crate::jobs::{self, StatusView};
use crate::store::jobs::JobRun;
use crate::store::models::{Change, ModelFilter, PriceRow, Published};

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

/// A model in a list: what the console's table shows.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelSummary {
    pub id: String,
    pub vendor: String,
    pub api_id: String,
    /// Whether a vendor's own page states the id.
    pub api_id_confirmed: bool,
    pub display_name: Option<String>,
    pub kind: Option<String>,
    pub status: String,
    pub context_window: Option<u64>,
    pub currency: Option<String>,
    /// Per million tokens.
    pub input: Option<f64>,
    /// Per million tokens.
    pub output: Option<f64>,
    /// When its source last stated the price.
    pub checked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_seen_at: chrono::DateTime<chrono::Utc>,
}

fn summary(p: &Published) -> ModelSummary {
    let m = &p.model;
    let rate = |name: &str| p.price.as_ref().and_then(|v| v["rates"][name].as_f64());
    ModelSummary {
        id: m.id.clone(),
        vendor: m.vendor_id.clone(),
        api_id: m.api_id.clone(),
        api_id_confirmed: m.api_id_confirmed,
        display_name: m.display_name.clone(),
        kind: m.kind.clone(),
        status: m.status.clone(),
        context_window: m.properties["context_window"].as_u64(),
        currency: p.price.as_ref().and_then(|v| v["currency"].as_str().map(str::to_owned)),
        input: rate("input"),
        output: rate("output"),
        checked_at: p.checked_at,
        last_seen_at: m.last_seen_at,
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelList {
    pub items: Vec<ModelSummary>,
    /// Where the next page starts. Absent on the last page.
    pub next_cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ModelsQuery {
    vendor: Option<String>,
    status: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

async fn models(State(st): State<AppState>, _: Reader, Query(q): Query<ModelsQuery>) -> ApiResult<Json<ModelList>> {
    let limit = q.limit.unwrap_or(200).clamp(1, 1000);
    let filter = ModelFilter {
        vendor_id: q.vendor.as_deref(),
        status: q.status.as_deref(),
        cursor: q.cursor.as_deref(),
        limit: limit + 1,
    };
    let mut rows = st.store.published_page(&filter).await?;
    let more = rows.len() as i64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = more.then(|| rows.last().map(|r| r.model.id.clone())).flatten();
    Ok(Json(ModelList { items: rows.iter().map(summary).collect(), next_cursor }))
}

/// A model with everything known about it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ModelDetail {
    #[serde(flatten)]
    pub summary: ModelSummary,
    /// The model as the catalogue publishes it (`pacod.catalog/v1`).
    pub entry: Value,
    /// Every version of its price, newest first.
    pub prices: Vec<PriceRow>,
    /// What changed, newest first.
    pub changes: Vec<Change>,
}

async fn model(State(st): State<AppState>, _: Reader, Path(id): Path<String>) -> ApiResult<Json<ModelDetail>> {
    let row = st.store.published_one(&id).await?.ok_or_else(|| ApiError::not_found("model"))?;
    let entry = serde_json::to_value(model_of(&row)).map_err(|e| ApiError::invalid(e.to_string()))?;
    Ok(Json(ModelDetail {
        summary: summary(&row),
        entry,
        prices: st.store.price_history(&id).await?,
        changes: st.store.changes(Some(&id), 200).await?,
    }))
}

#[derive(Debug, Default, Deserialize)]
struct ChangesQuery {
    limit: Option<i64>,
}

/// What changed in the catalogue, newest first.
async fn changes(State(st): State<AppState>, _: Reader, Query(q): Query<ChangesQuery>) -> ApiResult<Json<Vec<Change>>> {
    Ok(Json(st.store.changes(None, q.limit.unwrap_or(100).clamp(1, 500)).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/session", get(session))
        .route("/models", get(models))
        .route("/models/{id}", get(model))
        .route("/changes", get(changes))
        .route("/status", get(status))
        .route("/runs", get(runs))
        .route("/runs/check", post(run_check))
        .route("/runs/scan", post(run_scan))
        .merge(super::resources::routes())
}
