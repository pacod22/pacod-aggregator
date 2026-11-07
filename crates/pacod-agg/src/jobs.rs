//! The scraper's jobs from the server's side: the leases of the daily check and the weekly scan,
//! the work each run is handed, the heartbeat that keeps a long run's lease, and what the console
//! is told about the scraper. The scraper renders pages and looks for them; the server decides
//! what is read and what happens with what is found.
//!
//! Moved from pacod-gw (commit 50fe4b7, `crates/pacod-gw-core/src/mgmt/scraper.rs`).

use std::collections::{HashMap, HashSet};

use axum::Json;
use chrono::{DateTime, Utc};
use pacod_agg_jobs::jobs::{
    CheckWork, HeartbeatReply, HeartbeatRequest, JOB_CHECK, JOB_SCAN, JobResult, LeaseReply, LeaseRequest, PageTask,
    ReleaseRequest, ScanReport, ScanRoot, ScanVendor, ScanWork,
};
use pacod_agg_jobs::pages::normalise_url;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, AppState};
use crate::schedule::{due_by, next_after, parse_cron};
use crate::settings::Settings;
use crate::store::jobs::JobRun;
use crate::store::new_id;
use crate::store::pages::{NewPage, Page};
use crate::store::vendors::Vendor;

/// What a run left open is closed with.
const LEFT_OPEN: &str = "the scraper that ran it stopped before it finished";

/// How long after its lease ran out a run nobody took over is closed.
const LAPSED_MINUTES: i64 = 10;

/// How recently a scraper must have asked for work to count as running.
const PRESENT_SECONDS: i64 = 300;

// --- what a run covers ------------------------------------------------------------------------

/// The active pages of the vendors that are switched on.
fn active<'a>(pages: &'a [Page], vendors: &'a [Vendor]) -> impl Iterator<Item = &'a Page> {
    let on: HashSet<&str> = vendors.iter().filter(|v| v.enabled).map(|v| v.id.as_str()).collect();
    pages.iter().filter(move |p| p.status == "active" && on.contains(p.vendor_id.as_str()))
}

/// The pages a check covers: every active page, or the pages asked for alone.
async fn check_pages(st: &AppState, scope: Option<&[String]>) -> ApiResult<Vec<String>> {
    let (pages, vendors) = (st.store.pages(None, None).await?, st.store.vendors().await?);
    let ids = active(&pages, &vendors);
    if scope.is_none() {
        return Ok(ids.map(|p| p.id.clone()).collect());
    }
    let states = st.store.page_states().await?;
    Ok(ids.filter(|p| states.get(&p.id).is_some_and(|s| s.check_requested)).map(|p| p.id.clone()).collect())
}

/// The work of a check run over `pages`. A page removed or dismissed since the run started is
/// left out.
async fn check_work(st: &AppState, settings: &Settings, pages: &[String]) -> ApiResult<CheckWork> {
    let (all, vendors) = (st.store.pages(None, None).await?, st.store.vendors().await?);
    let states = st.store.page_states().await?;
    let names: HashMap<&str, &str> = vendors.iter().map(|v| (v.id.as_str(), v.display_name.as_str())).collect();
    let mut known: HashMap<String, Vec<String>> = HashMap::new();
    let mut tasks = Vec::new();
    for page in pages.iter().filter_map(|id| active(&all, &vendors).find(|p| &p.id == id)) {
        if !known.contains_key(&page.vendor_id) {
            known.insert(page.vendor_id.clone(), st.store.known_model_ids(&page.vendor_id).await?);
        }
        let state = states.get(&page.id);
        tasks.push(PageTask {
            page_id: page.id.clone(),
            vendor_id: page.vendor_id.clone(),
            vendor: names.get(page.vendor_id.as_str()).map(|n| (*n).to_owned()).unwrap_or_default(),
            url: page.url.clone(),
            kind: page.kind.clone(),
            format: page.format.clone(),
            previous_hash: state.map(|s| s.content_hash.clone()).unwrap_or_default(),
            // A page never read is read whatever its fingerprint.
            force: state.is_none_or(|s| s.force_read || s.last_read_at.is_none()),
            known_models: known[&page.vendor_id].clone(),
        });
    }
    Ok(CheckWork { pages: tasks, extraction: st.config.can_read(settings) })
}

/// Where the scan starts for a vendor: its active pages and the pages it is seeded with, except
/// those it has a page at already that is not active (proposed, or dismissed by an administrator,
/// whose word stands).
fn scan_roots(vendor: &Vendor, pages: &[&Page]) -> Vec<ScanRoot> {
    let mut roots: Vec<ScanRoot> = pages
        .iter()
        .filter(|p| p.status == "active" && p.kind == "page")
        .map(|p| ScanRoot { url: p.url.clone(), page_id: Some(p.id.clone()), seed: false })
        .collect();
    for seed in &vendor.sites {
        let Ok(url) = normalise_url(seed) else { continue };
        if !pages.iter().any(|p| p.url == url) {
            roots.push(ScanRoot { url, page_id: None, seed: true });
        }
    }
    roots
}

/// The vendors a scan covers: those switched on with somewhere to start, or those asked for.
async fn scan_vendors(st: &AppState, asked: Option<&[String]>) -> ApiResult<Vec<String>> {
    let pages = st.store.pages(None, None).await?;
    Ok(st
        .store
        .vendors()
        .await?
        .iter()
        .filter(|v| v.enabled && asked.is_none_or(|a| a.is_empty() || a.contains(&v.id)))
        .filter(|v| {
            let of: Vec<&Page> = pages.iter().filter(|p| p.vendor_id == v.id).collect();
            !scan_roots(v, &of).is_empty()
        })
        .map(|v| v.id.clone())
        .collect())
}

async fn scan_work(st: &AppState, settings: &Settings, ids: &[String]) -> ApiResult<ScanWork> {
    let pages = st.store.pages(None, None).await?;
    let mut work = Vec::new();
    for v in st.store.vendors().await?.iter().filter(|v| ids.contains(&v.id)) {
        let of: Vec<&Page> = pages.iter().filter(|p| p.vendor_id == v.id).collect();
        work.push(ScanVendor {
            vendor_id: v.id.clone(),
            name: v.display_name.clone(),
            roots: scan_roots(v, &of),
            known_urls: of.iter().map(|p| p.url.clone()).collect(),
            known_models: st.store.known_model_ids(&v.id).await?,
        });
    }
    let s = &settings.scan;
    Ok(ScanWork {
        vendors: work,
        max_pages_per_vendor: s.max_pages_per_vendor,
        min_score: s.min_score,
        keywords: s.keywords.clone(),
    })
}

// --- leases and runs --------------------------------------------------------------------------

async fn close_left_open(st: &AppState, job: &str, run_id: &str) -> ApiResult<()> {
    if st.store.job_finish(run_id, false, LEFT_OPEN, &json!({})).await? {
        tracing::warn!(job, run_id, "a run was left open by a scraper that stopped; closed");
    }
    Ok(())
}

/// Gives out the lease of the check or the scan and, when one is due, the work of a run.
pub async fn lease(st: &AppState, req: &LeaseRequest) -> ApiResult<Json<LeaseReply>> {
    if req.job != JOB_CHECK && req.job != JOB_SCAN {
        return Err(ApiError::invalid(format!("there is no job {}", req.job)));
    }
    let nothing = |granted| Json(LeaseReply { granted, run_id: None, due: false, work: Value::Null });
    let ttl = req.ttl_seconds.clamp(30, 3600);
    st.store.job_polled(&req.job, &json!({ "holder": req.holder, "agent": req.agent })).await?;
    let before = st.store.job_state(&req.job).await?;
    if !st.store.job_lease(&req.job, &req.holder, ttl).await? {
        return Ok(nothing(false));
    }
    let (settings, _) = st.settings().await?;
    let check = req.job == JOB_CHECK;
    let state = st.store.job_state(&req.job).await?;
    let start = async |trigger: &str, full: bool, covers: &[String]| -> ApiResult<String> {
        let id = new_id("run");
        st.store.job_start(&req.job, &id, trigger, full, covers).await?;
        Ok(id)
    };
    let (run_id, covers) = match state.current_run {
        // The holder's own run, asked for again: a scraper that restarted under its name.
        Some(open) if before.holder == req.holder => {
            let covers = st.store.job_run(&open).await?.map(|r| r.covers).unwrap_or_default();
            (open, covers)
        }
        // Another holder's run, whose lease ran out: it stopped. The run is closed as failed and
        // run once more, unless it was the second try already. Pages read before have their
        // fingerprint stored, so the second try does not have them read again.
        Some(open) => {
            let left = st.store.job_run(&open).await?;
            close_left_open(st, &req.job, &open).await?;
            // Closing a run gives up its lease; this holder takes it again.
            if !st.store.job_lease(&req.job, &req.holder, ttl).await? {
                return Ok(nothing(false));
            }
            let covers = match left {
                Some(r) if r.trigger != "retry" => r.covers,
                _ => return Ok(nothing(true)),
            };
            if covers.is_empty() {
                return Ok(nothing(true));
            }
            (start("retry", false, &covers).await?, covers)
        }
        None => {
            let schedule = if check { &settings.schedule } else { &settings.scan_schedule };
            let due = settings.enabled && due_by(schedule, &settings.time_zone, state.last_started_at, Utc::now());
            if !due && !state.run_requested {
                return Ok(nothing(true));
            }
            // A scheduled run covers everything, also what was asked for alone.
            let scope = if due { None } else { state.run_scope.clone() };
            let covers = if check {
                check_pages(st, scope.as_deref()).await?
            } else {
                scan_vendors(st, scope.as_deref()).await?
            };
            if covers.is_empty() {
                st.store.job_skip(&req.job, scope.is_none()).await?;
                return Ok(nothing(true));
            }
            let trigger = match (due, &scope) {
                (true, _) => "schedule",
                (false, None) => "manual",
                (false, Some(_)) if check => "page",
                (false, Some(_)) => "manual",
            };
            (start(trigger, scope.is_none(), &covers).await?, covers)
        }
    };
    let work = if check {
        serde_json::to_value(check_work(st, &settings, &covers).await?)
    } else {
        serde_json::to_value(scan_work(st, &settings, &covers).await?)
    }
    .map_err(|e| ApiError::invalid(e.to_string()))?;
    Ok(Json(LeaseReply { granted: true, run_id: Some(run_id), due: true, work }))
}

/// Gives a lease up: the scraper is stopping. Nobody waits for the lease to run out.
pub async fn release(st: &AppState, req: &ReleaseRequest) -> ApiResult<()> {
    Ok(st.store.job_release(&req.job, &req.holder).await?)
}

/// Keeps a run's lease while the scraper works on it. Answers `keep_going: false` when the run is
/// over or another holder has it, so that the scraper stops.
pub async fn heartbeat(st: &AppState, run_id: &str, req: &HeartbeatRequest) -> ApiResult<Json<HeartbeatReply>> {
    let run = st.store.job_run(run_id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    st.store.job_seen(&run.job).await?;
    let state = st.store.job_state(&run.job).await?;
    let mine = run.finished_at.is_none() && state.current_run.as_deref() == Some(run_id) && state.holder == req.holder;
    let keep_going = mine && st.store.job_lease(&run.job, &req.holder, req.ttl_seconds.clamp(30, 3600)).await?;
    Ok(Json(HeartbeatReply { keep_going }))
}

/// Takes what the scan found for one vendor. Pages that list models and prices are proposed, as
/// are the pages the vendor is seeded with; the vendor's pages that are gone are flagged, and a
/// page that moved is flagged and its new address proposed. An address the vendor has a page at,
/// in any status, is never proposed again: a dismissed page stays dismissed.
pub async fn scan_report(st: &AppState, run_id: &str, report: &ScanReport) -> ApiResult<Json<Value>> {
    let run = st.store.job_run(run_id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    if run.finished_at.is_some() {
        return Err(ApiError::conflict("this run is already finished"));
    }
    if run.job != JOB_SCAN || !run.covers.contains(&report.vendor_id) {
        return Err(ApiError::conflict("the vendor is not one this run scans"));
    }
    let (settings, _) = st.settings().await?;
    let pages = st.store.pages(Some(&report.vendor_id), None).await?;
    let mut known: HashSet<String> = pages.iter().map(|p| p.url.clone()).collect();
    let mut proposed: Vec<NewPage> = Vec::new();
    let mut propose = |url: &str, found_by: &str, title: &str, score: Option<f64>, moved_from: Option<&str>| {
        let Ok(url) = normalise_url(url) else { return };
        if st.egress.check_url(&url).is_err() || !known.insert(url.clone()) {
            return;
        }
        proposed.push(NewPage {
            vendor_id: report.vendor_id.clone(),
            url,
            kind: "page".into(),
            format: String::new(),
            status: "proposed".into(),
            found_by: found_by.into(),
            priority: 0,
            title: title.chars().take(300).collect(),
            score,
            moved_from: moved_from.map(str::to_owned),
        });
    };
    let mut flagged = Vec::new();
    for root in &report.roots {
        let target = if root.final_url.is_empty() { root.url.as_str() } else { root.final_url.as_str() };
        match (root.page_id.as_deref(), root.outcome.as_str()) {
            (Some(id), "gone") => flagged.push((id.to_owned(), Some("gone"), None)),
            (Some(id), "moved") => {
                flagged.push((id.to_owned(), Some("moved"), Some(target.to_owned())));
                propose(target, "scan", "", root.score, Some(&root.url));
            }
            (Some(id), "ok") => flagged.push((id.to_owned(), None, None)),
            (None, "ok" | "moved") => propose(target, "seed", "", root.score, None),
            _ => {}
        }
    }
    let min = settings.scan.min_score;
    let mut found: Vec<_> = report.found.iter().filter(|f| f.score.is_finite() && f.score >= min).collect();
    found.sort_by(|a, b| b.score.total_cmp(&a.score));
    for f in found {
        let target = if f.final_url.is_empty() { f.url.as_str() } else { f.final_url.as_str() };
        propose(target, "scan", &f.title, Some(f.score), None);
    }
    proposed.truncate(settings.scan.max_pages_per_vendor as usize);
    for page in &proposed {
        st.store.page_add(page).await?;
    }
    for (page_id, flag, moved_to) in &flagged {
        st.store.page_flag(page_id, *flag, moved_to.as_deref()).await?;
    }
    let urls: Vec<&str> = proposed.iter().map(|p| p.url.as_str()).collect();
    let flags: Vec<Value> = flagged
        .iter()
        .filter(|(_, f, _)| f.is_some())
        .map(|(id, f, to)| json!({ "page_id": id, "flag": f, "moved_to": to }))
        .collect();
    let entry = json!({ "vendor_id": report.vendor_id, "proposed": urls, "flagged": flags,
                        "considered": report.considered, "rendered": report.rendered, "message": report.message });
    st.store.job_run_append(run_id, "vendors", &json!([entry])).await?;
    Ok(Json(json!({ "proposed": urls, "flagged": flags })))
}

/// Closes a run: what the scraper reports is laid over what the server recorded while the run
/// went on.
pub async fn finish(st: &AppState, run_id: &str, res: &JobResult) -> ApiResult<Json<Value>> {
    let run = st.store.job_run(run_id).await?.ok_or_else(|| ApiError::not_found("run"))?;
    if run.finished_at.is_some() {
        return Err(ApiError::conflict("this run is already finished"));
    }
    let mut result = if run.result.is_object() { run.result } else { json!({}) };
    if let (Some(into), Some(from)) = (result.as_object_mut(), res.result.as_object()) {
        into.extend(from.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    st.store.job_finish(&run.id, res.ok, &res.message, &result).await?;
    Ok(Json(json!({ "run_id": run.id })))
}

/// Closes the runs nobody took over long after their lease ran out. A run left open keeps the job
/// from starting another; the next lease would close it too, but only when a scraper asks.
pub async fn close_lapsed(st: &AppState) -> ApiResult<()> {
    let lapsed = Utc::now() - chrono::Duration::minutes(LAPSED_MINUTES);
    for job in [JOB_CHECK, JOB_SCAN] {
        let state = st.store.job_state(job).await?;
        if let (Some(open), Some(until)) = (state.current_run, state.lease_until)
            && until < lapsed
        {
            close_left_open(st, job, &open).await?;
        }
    }
    Ok(())
}

// --- what the console is told -----------------------------------------------------------------

/// One of the scraper's jobs as the console shows it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct JobView {
    pub schedule: String,
    pub last_started_at: Option<DateTime<Utc>>,
    /// The next scheduled start. Absent while the jobs are switched off.
    pub next_run_at: Option<DateTime<Utc>>,
    /// A run was asked for and has not started yet.
    pub run_requested: bool,
    pub running: bool,
    /// The last run that finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<JobRun>,
}

/// The browser the scraper renders pages in, as the scraper last reported it.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct BrowserView {
    pub ok: bool,
    pub product: String,
    pub message: String,
}

/// Whether a scraper runs, as far as the server can tell: it asks for work every half minute.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ScraperView {
    /// When a scraper last asked for work.
    pub last_seen_at: Option<DateTime<Utc>>,
    /// Whether one asked in the last five minutes.
    pub present: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserView>,
}

/// What the Overview shows.
#[derive(Debug, Serialize, JsonSchema)]
pub struct StatusView {
    pub check: JobView,
    pub scan: JobView,
    pub scraper: ScraperView,
    /// Whether pages can be read: the operator set the model's address, and a model is named.
    pub extraction: bool,
    /// Pages waiting to be confirmed.
    pub proposed_pages: usize,
    /// What keeps the aggregator from doing its work, in sentences.
    pub problems: Vec<String>,
}

async fn job_view(
    st: &AppState,
    settings: &Settings,
    name: &str,
) -> ApiResult<(JobView, crate::store::jobs::JobState)> {
    let state = st.store.job_state(name).await?;
    let schedule = if name == JOB_CHECK { &settings.schedule } else { &settings.scan_schedule };
    let next_run_at = settings
        .enabled
        .then(|| next_after(schedule, &settings.time_zone, state.last_started_at.unwrap_or_else(Utc::now)))
        .flatten();
    let view = JobView {
        schedule: schedule.clone(),
        last_started_at: state.last_started_at,
        next_run_at,
        run_requested: state.run_requested,
        running: state.current_run.is_some(),
        last_run: st.store.job_last_finished(name).await?,
    };
    Ok((view, state))
}

/// What keeps the aggregator from doing its work, in sentences for the console.
fn problems(st: &AppState, s: &Settings, scraper: &ScraperView, pages: &[Page], vendors: &[Vendor]) -> Vec<String> {
    let mut out = Vec::new();
    for (field, schedule) in [("schedule", &s.schedule), ("scan_schedule", &s.scan_schedule)] {
        if let Err(e) = parse_cron(schedule) {
            out.push(format!("The {field} \"{schedule}\" cannot be read, so it never runs: {e}."));
        }
    }
    if !s.enabled {
        return out;
    }
    if st.config.internal_token.is_none() {
        out.push("AGG_INTERNAL_TOKEN is not set, so the scraper cannot ask for work.".into());
    }
    if !scraper.present {
        out.push(
            "No scraper has asked for work in the last five minutes: is agg-scraper running, with \
             AGG_SERVER_URL and AGG_INTERNAL_TOKEN set?"
                .into(),
        );
    } else if let Some(b) = scraper.browser.as_ref().filter(|b| !b.ok) {
        out.push(format!("The scraper cannot reach its browser: {}.", b.message.trim_end_matches('.')));
    }
    let reads_pages = active(pages, vendors).any(|p| p.kind == "page");
    let mode = st.config.mode(s);
    if reads_pages && st.config.extraction_model(s).is_none() {
        out.push(format!(
            "No extraction model is set for {} mode, in the settings or as {}: pages are checked, but nothing \
             reads them.",
            mode.name(),
            mode.model_variable(),
        ));
    }
    if reads_pages && st.config.link(s).is_none() {
        out.push(format!(
            "{} is not set, so {} mode has no endpoint: pages are checked, but no model can be called.",
            mode.url_variable(),
            mode.name(),
        ));
    }
    if active(pages, vendors).next().is_none() {
        out.push("No page is confirmed for any vendor: confirm the pages the scan proposes, or add some.".into());
    }
    out
}

pub async fn status(st: &AppState) -> ApiResult<StatusView> {
    let (settings, _) = st.settings().await?;
    let (check, a) = job_view(st, &settings, JOB_CHECK).await?;
    let (scan, b) = job_view(st, &settings, JOB_SCAN).await?;
    let latest = [a, b].into_iter().filter(|s| s.last_polled_at.is_some()).max_by_key(|s| s.last_polled_at);
    let last_seen_at = latest.as_ref().and_then(|s| s.last_polled_at);
    let agent = latest.and_then(|s| s.poll_status).map(|p| p["agent"].clone()).unwrap_or(Value::Null);
    let browser = agent.get("browser").filter(|b| b.is_object()).map(|b| BrowserView {
        ok: b["ok"].as_bool().unwrap_or(false),
        product: b["product"].as_str().unwrap_or_default().to_owned(),
        message: b["message"].as_str().unwrap_or_default().to_owned(),
    });
    let scraper = ScraperView {
        last_seen_at,
        present: last_seen_at.is_some_and(|t| (Utc::now() - t).num_seconds() <= PRESENT_SECONDS),
        version: agent["version"].as_str().map(str::to_owned),
        browser,
    };
    let (pages, vendors) = (st.store.pages(None, None).await?, st.store.vendors().await?);
    let problems = problems(st, &settings, &scraper, &pages, &vendors);
    Ok(StatusView {
        check,
        scan,
        extraction: st.config.can_read(&settings),
        proposed_pages: pages.iter().filter(|p| p.status == "proposed").count(),
        scraper,
        problems,
    })
}
