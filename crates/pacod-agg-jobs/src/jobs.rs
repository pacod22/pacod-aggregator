//! Messages of the internal job API the scraper uses on the server (`/internal/v1/jobs/...`). The
//! server hands out a lease so that one scraper runs a job, and it decides what a run covers and
//! what happens with what is found.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The header the internal token travels in.
pub const INTERNAL_TOKEN_HEADER: &str = "x-agg-internal-token";

/// Reads the pages the administrators confirmed, daily by default.
pub const JOB_CHECK: &str = "check";
/// Looks through the vendors' sites for pages that list models and prices, weekly by default.
pub const JOB_SCAN: &str = "scan";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseRequest {
    pub job: String,
    /// Names the scraper. Stable across its restarts and unique among scrapers, so that a scraper
    /// that restarts takes up its own run, and two scrapers never pass for one.
    pub holder: String,
    /// How long the lease is held, in seconds, unless renewed.
    pub ttl_seconds: u32,
    /// What the scraper says about itself, so that the console can show whether it runs and
    /// reaches its browser.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentStatus>,
}

/// Gives a lease up at once: a scraper that stops says so, and nobody waits for the lease to run
/// out. A run it held stays open for the same holder, or the next, to take up.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseRequest {
    pub job: String,
    pub holder: String,
}

/// A scraper's account of itself.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentStatus {
    /// The service, for example `agg-scraper`.
    pub service: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<BrowserStatus>,
}

/// Whether the scraper reaches the browser it renders pages in.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserStatus {
    pub ok: bool,
    /// The browser's product and version, for example `Chrome/157.0.8103.27`.
    pub product: String,
    /// Why it is not reached.
    pub message: String,
}

/// Keeps the lease of a run while the run lasts: a long run outlasts any fixed lease.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub holder: String,
    pub ttl_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatReply {
    /// False when the run is no longer this holder's, or is over: stop working on it.
    pub keep_going: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaseReply {
    pub granted: bool,
    /// The run to report results against. Present when the lease was granted and the job is due.
    pub run_id: Option<String>,
    /// True when the job should run now (schedule reached or "run now" requested).
    pub due: bool,
    /// [`CheckWork`] or [`ScanWork`].
    #[serde(default)]
    pub work: serde_json::Value,
}

/// The work of a run of the daily check: the pages to look at.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckWork {
    pub pages: Vec<PageTask>,
    /// Whether the server has a model to read pages with. Without one, pages are fetched and
    /// fingerprinted, and their readings wait.
    pub extraction: bool,
}

/// One page of a check run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageTask {
    pub page_id: String,
    pub vendor_id: String,
    /// The vendor's name.
    pub vendor: String,
    pub url: String,
    /// `page` (rendered in the browser and read by a model) or `feed` (a machine-readable list).
    pub kind: String,
    /// For feeds: `pacod`, `litellm` or `openrouter`.
    pub format: String,
    /// The fingerprint of the page at its last reading; empty before the first.
    pub previous_hash: String,
    /// Read the page even when its fingerprint did not change: an administrator asked.
    pub force: bool,
    /// The model ids the catalogue knows for this vendor, and their aliases.
    pub known_models: Vec<String>,
}

/// The work of a run of the weekly scan: where to look for pages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanWork {
    pub vendors: Vec<ScanVendor>,
    /// How many candidates per vendor are opened in the browser and scored.
    pub max_pages_per_vendor: u32,
    /// The score a page needs to be reported.
    pub min_score: f64,
    /// Words in an address or title that make a page worth opening.
    pub keywords: Vec<String>,
}

/// One vendor of a scan run.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanVendor {
    pub vendor_id: String,
    pub name: String,
    /// Where the scan starts: the vendor's active pages, and the pages it is seeded with.
    pub roots: Vec<ScanRoot>,
    /// Every address the vendor has a page at, whatever its status: not reported again.
    pub known_urls: Vec<String>,
    /// The model ids the catalogue knows for this vendor.
    pub known_models: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanRoot {
    pub url: String,
    /// The page at this address, when the vendor has one.
    pub page_id: Option<String>,
    /// A page the vendor is seeded with that it has no page at yet.
    pub seed: bool,
}

/// What the scan found for one vendor.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanReport {
    pub vendor_id: String,
    /// Pages that list models and prices, best first.
    pub found: Vec<FoundPage>,
    /// What became of each place the scan started from.
    pub roots: Vec<RootStatus>,
    /// How many addresses the scan considered, and how many it opened in the browser.
    pub considered: u32,
    pub rendered: u32,
    /// What went wrong for the vendor as a whole, in a sentence.
    pub message: String,
}

/// A page the scan judged to list models and prices.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FoundPage {
    pub url: String,
    /// Where the page ended up after redirects.
    pub final_url: String,
    pub title: String,
    pub score: f64,
    /// How many prices per million tokens (or per unit) the page shows.
    pub price_hits: u32,
    /// The catalogue's model ids the page names.
    pub model_hits: Vec<String>,
    /// A few of the lines that made the score, for the administrator who confirms the page.
    pub sample: Vec<String>,
}

/// What became of a place the scan started from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RootStatus {
    pub url: String,
    pub page_id: Option<String>,
    pub http_status: Option<u16>,
    pub final_url: String,
    /// `ok`, `gone` (404 or 410), `moved` (it redirects to another page) or `failed`.
    pub outcome: String,
    pub score: Option<f64>,
    pub message: String,
}

/// The end of a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    pub run_id: String,
    pub job: String,
    pub finished_at: DateTime<Utc>,
    pub ok: bool,
    #[serde(default)]
    pub message: String,
    /// Laid over what the server recorded while the run went on.
    #[serde(default)]
    pub result: serde_json::Value,
}
