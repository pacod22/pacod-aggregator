//! The public API: what a gateway reads. Ids are opaque (`agm_…`), one path segment each.

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::header::{CONTENT_TYPE, ETAG, IF_NONE_MATCH};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use pacod_agg_catalog::{Catalog, Model, Vendor};
use ring::digest::{SHA256, digest};
use serde::{Deserialize, Serialize};

use super::auth::CatalogReader;
use super::{ApiError, ApiResult, AppState};
use crate::catalog::{self, model_of, vendor_of};
use crate::store::models::{ModelFilter, PriceRow};

#[derive(Debug, Default, Deserialize)]
pub(super) struct CatalogQuery {
    /// Vendor ids, comma separated.
    vendor: Option<String>,
}

fn vendors_named(q: &CatalogQuery) -> Option<Vec<String>> {
    q.vendor
        .as_deref()
        .map(|v| v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
}

/// The tag of a catalogue: of what it says, not of when it was written, so that a consumer that
/// asks again is told nothing changed.
fn etag(catalog: &Catalog) -> Result<String, serde_json::Error> {
    let body = serde_json::to_vec(&Catalog { generated_at: None, ..catalog.clone() })?;
    Ok(format!("\"{}\"", hex::encode(&digest(&SHA256, &body).as_ref()[..16])))
}

/// The whole catalogue in version 1, or the vendors named. Answers 304 to the tag it was last read
/// with, so that a daily reader fetches it only when it changed. The console's export comes from
/// here too, so that a file and this API cannot come to differ.
pub(super) async fn catalogue_answer(st: &AppState, q: &CatalogQuery, headers: &HeaderMap) -> ApiResult<Response> {
    let names = vendors_named(q);
    let catalog = catalog::catalog(&st.store, names.as_deref(), st.config.public_url.clone()).await?;
    let internal = |e: serde_json::Error| ApiError::invalid(e.to_string());
    let tag = etag(&catalog).map_err(internal)?;
    let value = HeaderValue::from_str(&tag).map_err(|e| ApiError::invalid(e.to_string()))?;
    let cached = headers
        .get(IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|given| given.trim() == "*" || given.split(',').any(|t| t.trim().trim_start_matches("W/") == tag));
    if cached {
        return Ok((StatusCode::NOT_MODIFIED, [(ETAG, value)]).into_response());
    }
    let body = serde_json::to_vec(&catalog).map_err(internal)?;
    Ok(([(ETAG, value), (CONTENT_TYPE, HeaderValue::from_static("application/json"))], body).into_response())
}

async fn catalogue(
    State(st): State<AppState>,
    _: CatalogReader,
    headers: HeaderMap,
    Query(q): Query<CatalogQuery>,
) -> ApiResult<Response> {
    catalogue_answer(&st, &q, &headers).await
}

async fn vendors(State(st): State<AppState>, _: CatalogReader) -> ApiResult<Json<Vec<Vendor>>> {
    let list = st.store.vendors().await?;
    Ok(Json(list.iter().filter(|v| v.enabled).map(vendor_of).collect()))
}

#[derive(Debug, Default, Deserialize)]
struct ModelsQuery {
    vendor: Option<String>,
    status: Option<String>,
    cursor: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct ModelPage {
    items: Vec<Model>,
    /// Where the next page starts. Absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

async fn models(
    State(st): State<AppState>,
    _: CatalogReader,
    Query(q): Query<ModelsQuery>,
) -> ApiResult<Json<ModelPage>> {
    let limit = q.limit.unwrap_or(100).clamp(1, 500);
    let filter = ModelFilter {
        vendor_id: q.vendor.as_deref(),
        status: q.status.as_deref(),
        cursor: q.cursor.as_deref(),
        // One more than asked for says whether there is another page.
        limit: limit + 1,
    };
    let mut rows = st.store.published_page(&filter).await?;
    let more = rows.len() as i64 > limit;
    rows.truncate(limit as usize);
    let next_cursor = more.then(|| rows.last().map(|r| r.model.id.clone())).flatten();
    Ok(Json(ModelPage { items: rows.iter().map(model_of).collect(), next_cursor }))
}

async fn model(State(st): State<AppState>, _: CatalogReader, Path(id): Path<String>) -> ApiResult<Json<Model>> {
    let row = st.store.published_one(&id).await?.ok_or_else(|| ApiError::not_found("model"))?;
    Ok(Json(model_of(&row)))
}

async fn prices(
    State(st): State<AppState>,
    _: CatalogReader,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<PriceRow>>> {
    if st.store.published_one(&id).await?.is_none() {
        return Err(ApiError::not_found("model"));
    }
    Ok(Json(st.store.price_history(&id).await?))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/v1/catalog", get(catalogue))
        .route("/v1/vendors", get(vendors))
        .route("/v1/models", get(models))
        .route("/v1/models/{id}", get(model))
        .route("/v1/models/{id}/prices", get(prices))
}
