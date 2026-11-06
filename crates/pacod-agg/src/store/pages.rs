//! Pages, and what the daily check finds out about them.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Result, Store, new_id};

/// A page of a vendor's, or a machine-readable list.
#[derive(Debug, Clone, PartialEq, Serialize, sqlx::FromRow, JsonSchema)]
pub struct Page {
    pub id: String,
    pub vendor_id: String,
    /// Normalised: no fragment, no tracking parameters, no slash at the end.
    pub url: String,
    /// `page` (rendered in a browser and read by a model) or `feed` (a list, read without either).
    pub kind: String,
    /// For feeds: `pacod`, `litellm` or `openrouter`.
    pub format: String,
    /// `proposed`, `active` or `dismissed`.
    pub status: String,
    /// `admin`, `scan` or `seed`.
    pub found_by: String,
    /// Where two pages of a vendor say different things about a model, the lower number wins.
    pub priority: i32,
    pub notes: String,
    /// The page's title, as the scan saw it.
    pub title: String,
    /// How strongly the scan judged the page to list models and prices.
    pub score: Option<f64>,
    /// The page this one replaces, where the scan found that a page moved here.
    pub moved_from: Option<String>,
    pub decided_by: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A page as an administrator adds or changes it. Its status changes through confirm and dismiss;
/// what the scan saw about it stays as the scan saw it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PageInput {
    pub vendor_id: String,
    pub url: String,
    #[serde(default = "page")]
    pub kind: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub notes: String,
}

fn page() -> String {
    "page".to_owned()
}

/// What the scan or a seed adds.
#[derive(Debug, Clone, Default)]
pub struct NewPage {
    pub vendor_id: String,
    pub url: String,
    pub kind: String,
    pub format: String,
    pub status: String,
    pub found_by: String,
    pub priority: i32,
    pub title: String,
    pub score: Option<f64>,
    pub moved_from: Option<String>,
}

/// What the last check found out about a page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, sqlx::FromRow, JsonSchema)]
pub struct PageState {
    #[serde(skip)]
    pub page_id: String,
    pub last_checked_at: Option<DateTime<Utc>>,
    /// When the page last answered and read as it should.
    pub last_ok_at: Option<DateTime<Utc>>,
    /// When the page last read differently from the time before.
    pub last_changed_at: Option<DateTime<Utc>>,
    /// When the extraction model last read it.
    pub last_read_at: Option<DateTime<Utc>>,
    #[serde(skip)]
    pub content_hash: String,
    pub http_status: Option<i32>,
    /// Where the page ended up after redirects, when that is elsewhere.
    pub final_url: String,
    /// `read`, `unchanged`, `not_read` or `failed`; empty before the first check.
    pub outcome: String,
    pub message: String,
    pub consecutive_failures: i32,
    /// `gone` or `moved`, when the scan found so.
    pub flag: Option<String>,
    pub moved_to: Option<String>,
    /// A check was asked for and has not happened yet.
    pub check_requested: bool,
    /// The page is to be read at its next check even when it did not change.
    #[serde(skip)]
    pub force_read: bool,
    /// What the last successful reading found.
    #[serde(skip)]
    pub extraction: Value,
}

/// What one check of a page found, as it is recorded.
#[derive(Debug, Clone, Default)]
pub struct Recording {
    /// `read`, `unchanged`, `not_read` or `failed`.
    pub outcome: String,
    pub message: String,
    pub http_status: Option<i32>,
    pub final_url: String,
    /// Stored only after a successful reading, so that a page that could not be read is read
    /// again at its next check.
    pub content_hash: Option<String>,
    pub text_hash: Option<String>,
    pub extraction: Option<Value>,
}

/// What can go wrong saving a page.
pub enum Saved {
    Done(Box<Page>),
    /// The vendor already has a page at the address.
    Taken,
    NoVendor,
}

fn saved(result: Result<Page>) -> Result<Saved> {
    match result {
        Ok(p) => Ok(Saved::Done(Box::new(p))),
        Err(e) => match e.as_database_error() {
            Some(d) if d.is_unique_violation() => Ok(Saved::Taken),
            Some(d) if d.is_foreign_key_violation() => Ok(Saved::NoVendor),
            _ => Err(e),
        },
    }
}

impl Store {
    /// Pages, by vendor and priority. Ids sort by creation time, so pages of one priority keep
    /// the order they were added in.
    pub async fn pages(&self, vendor_id: Option<&str>, status: Option<&str>) -> Result<Vec<Page>> {
        sqlx::query_as(
            "SELECT * FROM pages WHERE ($1::text IS NULL OR vendor_id = $1) AND ($2::text IS NULL OR status = $2)
             ORDER BY vendor_id, priority, id",
        )
        .bind(vendor_id)
        .bind(status)
        .fetch_all(self.pool())
        .await
    }

    pub async fn page(&self, id: &str) -> Result<Option<Page>> {
        sqlx::query_as("SELECT * FROM pages WHERE id = $1").bind(id).fetch_optional(self.pool()).await
    }

    /// A page an administrator adds is read from the next check on.
    pub async fn page_create(&self, p: &PageInput, url: &str, by: &str) -> Result<Saved> {
        saved(
            sqlx::query_as(
                "INSERT INTO pages (id, vendor_id, url, kind, format, status, found_by, priority, notes, decided_by,
                     decided_at)
                 VALUES ($1, $2, $3, $4, $5, 'active', 'admin', $6, $7, $8, now()) RETURNING *",
            )
            .bind(new_id("pag"))
            .bind(&p.vendor_id)
            .bind(url)
            .bind(&p.kind)
            .bind(&p.format)
            .bind(p.priority)
            .bind(&p.notes)
            .bind(by)
            .fetch_one(self.pool())
            .await,
        )
    }

    pub async fn page_update(&self, id: &str, p: &PageInput, url: &str) -> Result<Saved> {
        saved(
            sqlx::query_as(
                "UPDATE pages SET vendor_id = $2, url = $3, kind = $4, format = $5, priority = $6, notes = $7,
                     updated_at = now()
                 WHERE id = $1 RETURNING *",
            )
            .bind(id)
            .bind(&p.vendor_id)
            .bind(url)
            .bind(&p.kind)
            .bind(&p.format)
            .bind(p.priority)
            .bind(&p.notes)
            .fetch_one(self.pool())
            .await,
        )
    }

    pub async fn page_delete(&self, id: &str) -> Result<bool> {
        Ok(sqlx::query("DELETE FROM pages WHERE id = $1").bind(id).execute(self.pool()).await?.rows_affected() > 0)
    }

    /// Adds a page the scan found or a seed names, unless the vendor has a page at the address,
    /// in any status: a dismissed page stays dismissed. Answers whether it was added.
    pub async fn page_add(&self, p: &NewPage) -> Result<bool> {
        Ok(sqlx::query(
            "INSERT INTO pages (id, vendor_id, url, kind, format, status, found_by, priority, title, score, moved_from)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT (vendor_id, url) DO NOTHING",
        )
        .bind(new_id("pag"))
        .bind(&p.vendor_id)
        .bind(&p.url)
        .bind(&p.kind)
        .bind(&p.format)
        .bind(&p.status)
        .bind(&p.found_by)
        .bind(p.priority)
        .bind(&p.title)
        .bind(p.score)
        .bind(&p.moved_from)
        .execute(self.pool())
        .await?
        .rows_affected()
            > 0)
    }

    /// Confirms or dismisses a page.
    pub async fn page_decide(&self, id: &str, status: &str, by: &str) -> Result<Option<Page>> {
        sqlx::query_as(
            "UPDATE pages SET status = $2, decided_by = $3, decided_at = now(), updated_at = now()
             WHERE id = $1 RETURNING *",
        )
        .bind(id)
        .bind(status)
        .bind(by)
        .fetch_optional(self.pool())
        .await
    }

    /// The state of every page that has one, by page id.
    pub async fn page_states(&self) -> Result<HashMap<String, PageState>> {
        let states: Vec<PageState> = sqlx::query_as(
            "SELECT page_id, last_checked_at, last_ok_at, last_changed_at, last_read_at, content_hash, http_status,
                    final_url, outcome, message, consecutive_failures, flag, moved_to, check_requested, force_read,
                    extraction
             FROM page_state",
        )
        .fetch_all(self.pool())
        .await?;
        Ok(states.into_iter().map(|s| (s.page_id.clone(), s)).collect())
    }

    /// Asks for a page to be checked at the next run. `force` has it read by the extraction model
    /// even when it did not change.
    pub async fn page_request_check(&self, page_id: &str, force: bool) -> Result<()> {
        sqlx::query(
            "INSERT INTO page_state (page_id, check_requested, force_read) VALUES ($1, true, $2)
             ON CONFLICT (page_id) DO UPDATE SET check_requested = true,
               force_read = page_state.force_read OR $2, updated_at = now()",
        )
        .bind(page_id)
        .bind(force)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Records one check of a page. A reading or an unchanged page answers the requests for a
    /// check, and counts as the page being fine; a page that failed or was not read keeps being
    /// asked to be read.
    pub async fn page_record(&self, page_id: &str, r: &Recording) -> Result<()> {
        let failed = r.outcome == "failed";
        let done = matches!(r.outcome.as_str(), "read" | "unchanged");
        sqlx::query(
            "INSERT INTO page_state (page_id, last_checked_at, last_ok_at, outcome, message, http_status, final_url,
                 consecutive_failures, content_hash, text_hash, last_changed_at, last_read_at, extraction)
             VALUES ($1, now(), CASE WHEN $9 THEN now() END, $2, $3, $4, $5, CASE WHEN $6 THEN 1 ELSE 0 END,
                 coalesce($7, ''), coalesce($8, ''), CASE WHEN $7 IS NOT NULL THEN now() END,
                 CASE WHEN $10::jsonb IS NOT NULL THEN now() END, coalesce($10, '{}'::jsonb))
             ON CONFLICT (page_id) DO UPDATE SET
               last_checked_at = now(),
               last_ok_at = CASE WHEN $9 THEN now() ELSE page_state.last_ok_at END,
               outcome = $2, message = $3, http_status = $4, final_url = $5,
               consecutive_failures = CASE WHEN $6 THEN page_state.consecutive_failures + 1 ELSE 0 END,
               last_changed_at = CASE WHEN $7 IS NOT NULL AND $7 <> page_state.content_hash THEN now()
                                      ELSE page_state.last_changed_at END,
               content_hash = coalesce($7, page_state.content_hash),
               text_hash = coalesce($8, page_state.text_hash),
               last_read_at = CASE WHEN $10::jsonb IS NOT NULL THEN now() ELSE page_state.last_read_at END,
               extraction = coalesce($10, page_state.extraction),
               check_requested = CASE WHEN $9 THEN false ELSE page_state.check_requested END,
               force_read = CASE WHEN $9 THEN false ELSE page_state.force_read END,
               updated_at = now()",
        )
        .bind(page_id)
        .bind(&r.outcome)
        .bind(&r.message)
        .bind(r.http_status)
        .bind(&r.final_url)
        .bind(failed)
        .bind(&r.content_hash)
        .bind(&r.text_hash)
        .bind(done)
        .bind(&r.extraction)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Records what the scan found of a page: `gone`, `moved` (to `moved_to`), or nothing wrong.
    pub async fn page_flag(&self, page_id: &str, flag: Option<&str>, moved_to: Option<&str>) -> Result<()> {
        sqlx::query(
            "INSERT INTO page_state (page_id, flag, moved_to) VALUES ($1, $2, $3)
             ON CONFLICT (page_id) DO UPDATE SET flag = $2, moved_to = $3, updated_at = now()",
        )
        .bind(page_id)
        .bind(flag)
        .bind(moved_to)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
