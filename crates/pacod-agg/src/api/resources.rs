//! Vendors, pages and the settings: what an administrator changes in the console.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use pacod_agg_catalog::Format;
use pacod_agg_jobs::jobs::JOB_CHECK;
use pacod_agg_jobs::pages::normalise_url;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::auth::{Manager, Reader};
use super::{ApiError, ApiResult, AppState};
use crate::settings::Settings;
use crate::store::pages::{Page, PageInput, PageState, Saved};
use crate::store::vendors::{Vendor, VendorInput};

// --- vendors ------------------------------------------------------------------------------------

async fn vendors(State(st): State<AppState>, _: Reader) -> ApiResult<Json<Vec<Vendor>>> {
    Ok(Json(st.store.vendors().await?))
}

async fn vendor(State(st): State<AppState>, _: Reader, Path(id): Path<String>) -> ApiResult<Json<Vendor>> {
    st.store.vendor(&id).await?.map(Json).ok_or_else(|| ApiError::not_found("vendor"))
}

/// A vendor's id is the catalogue's name for it, and a gateway's: lower case, as it will stay.
fn check_vendor(v: &mut VendorInput, st: &AppState) -> ApiResult<()> {
    v.id = v.id.trim().to_owned();
    v.display_name = v.display_name.trim().to_owned();
    if v.id.is_empty() || !v.id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return Err(ApiError::invalid("id: lower-case letters, digits and hyphens"));
    }
    if v.display_name.is_empty() {
        return Err(ApiError::invalid("display_name is empty"));
    }
    if v.currency.len() != 3 || !v.currency.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(ApiError::invalid("currency is a three-letter code in capitals"));
    }
    for site in &mut v.sites {
        *site = checked_url(site, st)?;
    }
    Ok(())
}

async fn vendor_create(
    State(st): State<AppState>,
    _: Manager,
    Json(mut v): Json<VendorInput>,
) -> ApiResult<(StatusCode, Json<Vendor>)> {
    check_vendor(&mut v, &st)?;
    let created = st.store.vendor_create(&v).await?;
    let vendor = created.ok_or_else(|| ApiError::conflict(format!("there is a vendor {} already", v.id)))?;
    Ok((StatusCode::CREATED, Json(vendor)))
}

async fn vendor_update(
    State(st): State<AppState>,
    _: Manager,
    Path(id): Path<String>,
    Json(mut v): Json<VendorInput>,
) -> ApiResult<Json<Vendor>> {
    v.id.clone_from(&id);
    check_vendor(&mut v, &st)?;
    st.store.vendor_update(&id, &v).await?.map(Json).ok_or_else(|| ApiError::not_found("vendor"))
}

async fn vendor_delete(State(st): State<AppState>, _: Manager, Path(id): Path<String>) -> ApiResult<StatusCode> {
    if !st.store.vendor_delete(&id).await? {
        return Err(ApiError::not_found("vendor"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// --- pages --------------------------------------------------------------------------------------

/// The address in the one form pages are stored in. The scraper fetches it from inside the
/// network, so the egress policy applies here, and again when it connects.
fn checked_url(url: &str, st: &AppState) -> ApiResult<String> {
    let url = normalise_url(url).map_err(ApiError::invalid)?;
    st.egress.check_url(&url).map_err(|e| ApiError::invalid(format!("url: {e}")))?;
    Ok(url)
}

fn check_page(p: &PageInput, st: &AppState) -> ApiResult<String> {
    let url = checked_url(&p.url, st)?;
    match p.kind.as_str() {
        "page" if p.format.is_empty() => {}
        "page" => return Err(ApiError::invalid("format is for feeds; a page is read by a model")),
        "feed" if Format::from_name(&p.format).is_some_and(|f| f != Format::Catalog) => {}
        "feed" => return Err(ApiError::invalid("format: pacod, litellm or openrouter")),
        _ => return Err(ApiError::invalid("kind: page or feed")),
    }
    if !(0..=1000).contains(&p.priority) || p.notes.chars().count() > 2000 {
        return Err(ApiError::invalid("priority is 0 to 1000, notes at most 2000 characters"));
    }
    Ok(url)
}

/// A page with its vendor's name and what the last check found.
#[derive(Debug, Serialize, JsonSchema)]
pub struct PageView {
    #[serde(flatten)]
    pub page: Page,
    /// The vendor's name.
    pub vendor: String,
    /// What the last check found. Absent before the page was first checked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<PageState>,
}

async fn page_view(st: &AppState, page: Page) -> ApiResult<PageView> {
    let vendor = st.store.vendor(&page.vendor_id).await?.map(|v| v.display_name).unwrap_or_default();
    let state = st.store.page_states().await?.remove(&page.id);
    Ok(PageView { page, vendor, state })
}

#[derive(Debug, Default, Deserialize)]
struct PagesQuery {
    vendor_id: Option<String>,
    status: Option<String>,
}

async fn pages(State(st): State<AppState>, _: Reader, Query(q): Query<PagesQuery>) -> ApiResult<Json<Vec<PageView>>> {
    let names: HashMap<String, String> =
        st.store.vendors().await?.into_iter().map(|v| (v.id, v.display_name)).collect();
    let mut states = st.store.page_states().await?;
    let list = st.store.pages(q.vendor_id.as_deref(), q.status.as_deref()).await?;
    Ok(Json(
        list.into_iter()
            .map(|page| PageView {
                vendor: names.get(&page.vendor_id).cloned().unwrap_or_default(),
                state: states.remove(&page.id),
                page,
            })
            .collect(),
    ))
}

async fn page(State(st): State<AppState>, _: Reader, Path(id): Path<String>) -> ApiResult<Json<PageView>> {
    let page = st.store.page(&id).await?.ok_or_else(|| ApiError::not_found("page"))?;
    Ok(Json(page_view(&st, page).await?))
}

/// Asks for one page to be checked and read at the next run.
async fn request_check(st: &AppState, page: &Page) -> ApiResult<()> {
    st.store.page_request_check(&page.id, true).await?;
    st.store.job_request_run(JOB_CHECK, Some(&[])).await?;
    Ok(())
}

fn done(saved: Saved) -> ApiResult<Page> {
    match saved {
        Saved::Done(p) => Ok(*p),
        Saved::Taken => Err(ApiError::conflict("the vendor has a page at this address")),
        Saved::NoVendor => Err(ApiError::invalid("vendor_id names no vendor")),
    }
}

async fn page_create(
    State(st): State<AppState>,
    Manager(p): Manager,
    Json(input): Json<PageInput>,
) -> ApiResult<(StatusCode, Json<PageView>)> {
    let url = check_page(&input, &st)?;
    let page = done(st.store.page_create(&input, &url, &p.id).await?)?;
    // A new page is checked at the next run, not only at the next scheduled one.
    request_check(&st, &page).await?;
    Ok((StatusCode::CREATED, Json(page_view(&st, page).await?)))
}

async fn page_update(
    State(st): State<AppState>,
    _: Manager,
    Path(id): Path<String>,
    Json(input): Json<PageInput>,
) -> ApiResult<Json<PageView>> {
    let url = check_page(&input, &st)?;
    let before = st.store.page(&id).await?.ok_or_else(|| ApiError::not_found("page"))?;
    let page = done(st.store.page_update(&id, &input, &url).await?)?;
    if page.status == "active" && page.url != before.url {
        request_check(&st, &page).await?;
    }
    Ok(Json(page_view(&st, page).await?))
}

async fn page_delete(State(st): State<AppState>, _: Manager, Path(id): Path<String>) -> ApiResult<StatusCode> {
    if !st.store.page_delete(&id).await? {
        return Err(ApiError::not_found("page"));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Confirms or dismisses a page. A confirmed page is read at the next run; a dismissed one is not
/// read, and the scan does not propose it again.
async fn decide(st: &AppState, by: &str, id: &str, to: &str) -> ApiResult<Json<PageView>> {
    let page = st.store.page(id).await?.ok_or_else(|| ApiError::not_found("page"))?;
    if page.status == to {
        return Err(ApiError::conflict(format!("the page is already {to}")));
    }
    let page = st.store.page_decide(id, to, by).await?.ok_or_else(|| ApiError::not_found("page"))?;
    if to == "active" {
        request_check(st, &page).await?;
    }
    Ok(Json(page_view(st, page).await?))
}

async fn confirm(State(st): State<AppState>, Manager(p): Manager, Path(id): Path<String>) -> ApiResult<Json<PageView>> {
    decide(&st, &p.id, &id, "active").await
}

async fn dismiss(State(st): State<AppState>, Manager(p): Manager, Path(id): Path<String>) -> ApiResult<Json<PageView>> {
    decide(&st, &p.id, &id, "dismissed").await
}

/// Asks for a page to be checked, and read even when it did not change, at the next run.
async fn check(State(st): State<AppState>, _: Manager, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let page = st.store.page(&id).await?.ok_or_else(|| ApiError::not_found("page"))?;
    if page.status != "active" {
        return Err(ApiError::conflict("only a confirmed page is checked; confirm it first"));
    }
    request_check(&st, &page).await?;
    Ok(StatusCode::ACCEPTED)
}

// --- settings -----------------------------------------------------------------------------------

/// The settings, and the version to send back with a change.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SettingsView {
    #[serde(flatten)]
    pub settings: Settings,
    pub version: i32,
}

/// A change of the settings.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SettingsInput {
    #[serde(flatten)]
    pub settings: Settings,
    /// The version the change was made from: the change is refused when another came first.
    pub version: i32,
}

async fn settings(State(st): State<AppState>, _: Reader) -> ApiResult<Json<SettingsView>> {
    let (settings, version) = st.settings().await?;
    Ok(Json(SettingsView { settings, version }))
}

async fn settings_save(
    State(st): State<AppState>,
    _: Manager,
    Json(input): Json<SettingsInput>,
) -> ApiResult<Json<SettingsView>> {
    let mut settings = input.settings;
    settings.check().map_err(ApiError::invalid)?;
    settings.extraction_model = settings.extraction_model.map(|m| m.trim().to_owned()).filter(|m| !m.is_empty());
    let doc = serde_json::to_value(&settings).map_err(|e| ApiError::invalid(e.to_string()))?;
    let version = st
        .store
        .settings_save(&doc, input.version)
        .await?
        .ok_or_else(|| ApiError::conflict("the settings changed in the meantime; load them again"))?;
    Ok(Json(SettingsView { settings, version }))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/vendors", get(vendors).post(vendor_create))
        .route("/vendors/{id}", get(vendor).put(vendor_update).delete(vendor_delete))
        .route("/pages", get(pages).post(page_create))
        .route("/pages/{id}", get(page).put(page_update).delete(page_delete))
        .route("/pages/{id}/confirm", post(confirm))
        .route("/pages/{id}/dismiss", post(dismiss))
        .route("/pages/{id}/check", post(check))
        .route("/settings", get(settings).put(settings_save))
}
