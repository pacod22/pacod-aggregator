//! Leases and runs of the scraper's jobs.

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use super::{Result, Store};

/// What the server knows about a job.
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct JobState {
    pub holder: String,
    pub lease_until: Option<DateTime<Utc>>,
    pub last_started_at: Option<DateTime<Utc>>,
    pub run_requested: bool,
    /// What a pending request names; `None` asks for everything.
    pub run_scope: Option<Vec<String>>,
    pub current_run: Option<String>,
    /// When a scraper last asked for this job's lease, granted or not, and what it said.
    pub last_polled_at: Option<DateTime<Utc>>,
    pub poll_status: Option<Value>,
}

/// One run of a job.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, JsonSchema)]
pub struct JobRun {
    pub id: String,
    pub job: String,
    /// `schedule`, `manual`, `page` (a page was confirmed or asked to be checked) or `retry` (the
    /// run before was left open).
    pub trigger: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    /// Absent while the run is in progress.
    pub ok: Option<bool>,
    pub message: String,
    pub result: Value,
    /// What the run covers, by id: the pages of a check, the vendors of a scan.
    pub covers: Vec<String>,
}

impl Store {
    pub async fn job_state(&self, name: &str) -> Result<JobState> {
        Ok(sqlx::query_as(
            "SELECT holder, lease_until, last_started_at, run_requested, run_scope, current_run, last_polled_at,
                    poll_status
             FROM jobs WHERE name = $1",
        )
        .bind(name)
        .fetch_optional(self.pool())
        .await?
        .unwrap_or_default())
    }

    /// Asks for a run as soon as a scraper next takes the lease. `scope` names what it covers.
    pub async fn job_request_run(&self, name: &str, scope: Option<&[String]>) -> Result<()> {
        sqlx::query(
            "INSERT INTO jobs (name, run_requested, run_scope) VALUES ($1, true, $2)
             ON CONFLICT (name) DO UPDATE SET run_requested = true,
               run_scope = CASE WHEN jobs.run_requested AND jobs.run_scope IS NULL THEN NULL ELSE $2 END",
        )
        .bind(name)
        .bind(scope)
        .execute(self.pool())
        .await?;
        Ok(())
    }
}
