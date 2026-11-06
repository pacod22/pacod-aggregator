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

    /// Records that a scraper asked for the job, and what it said about itself. A poll that is
    /// not granted the lease leaves no other trace.
    pub async fn job_polled(&self, name: &str, status: &Value) -> Result<()> {
        sqlx::query(
            "INSERT INTO jobs (name, last_polled_at, poll_status) VALUES ($1, now(), $2)
             ON CONFLICT (name) DO UPDATE SET last_polled_at = now(), poll_status = $2",
        )
        .bind(name)
        .bind(status)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Takes or renews the lease for `holder`. Returns false when another holder has it.
    pub async fn job_lease(&self, name: &str, holder: &str, ttl_seconds: u32) -> Result<bool> {
        let taken = sqlx::query(
            "INSERT INTO jobs (name, holder, lease_until) VALUES ($1, $2, now() + make_interval(secs => $3))
             ON CONFLICT (name) DO UPDATE SET holder = $2, lease_until = now() + make_interval(secs => $3)
             WHERE jobs.holder = $2 OR jobs.holder = '' OR jobs.lease_until IS NULL OR jobs.lease_until < now()",
        )
        .bind(name)
        .bind(holder)
        .bind(f64::from(ttl_seconds))
        .execute(self.pool())
        .await?
        .rows_affected();
        Ok(taken == 1)
    }

    /// Lets a holder's lease run out now. The holder stays named, and a run it held stays open,
    /// so that the same scraper takes its run up again, and another may take over at once.
    pub async fn job_release(&self, name: &str, holder: &str) -> Result<()> {
        sqlx::query("UPDATE jobs SET lease_until = now() WHERE name = $1 AND holder = $2")
            .bind(name)
            .bind(holder)
            .execute(self.pool())
            .await?;
        Ok(())
    }

    /// Answers a request for a run that finds nothing to do: the request is cleared, and a
    /// `full` run counts for the schedule as if it had run.
    pub async fn job_skip(&self, name: &str, full: bool) -> Result<()> {
        sqlx::query(
            "UPDATE jobs SET run_requested = false, run_scope = NULL,
               last_started_at = CASE WHEN $2 THEN now() ELSE last_started_at END
             WHERE name = $1",
        )
        .bind(name)
        .bind(full)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Records the start of a run that covers `covers`, fixed at the start so that the run covers
    /// the same when it is handed out again. A `full` run counts for the schedule.
    pub async fn job_start(
        &self,
        name: &str,
        run_id: &str,
        trigger: &str,
        full: bool,
        covers: &[String],
    ) -> Result<()> {
        let mut tx = self.pool().begin().await?;
        sqlx::query("INSERT INTO job_runs (id, job, trigger, started_at, covers) VALUES ($1, $2, $3, now(), $4)")
            .bind(run_id)
            .bind(name)
            .bind(trigger)
            .bind(covers)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE jobs SET last_started_at = CASE WHEN $3 THEN now() ELSE last_started_at END,
               run_requested = false, run_scope = NULL, current_run = $2
             WHERE name = $1",
        )
        .bind(name)
        .bind(run_id)
        .bind(full)
        .execute(&mut *tx)
        .await?;
        tx.commit().await
    }

    /// Adds `n` to the counter `key` of a run's result and answers the new total.
    pub async fn job_run_count(&self, run_id: &str, key: &str, n: i64) -> Result<i64> {
        let total: Option<i64> = sqlx::query_scalar(
            "UPDATE job_runs SET result = jsonb_set(result, ARRAY[$2], to_jsonb(coalesce((result ->> $2)::bigint, 0) + $3))
             WHERE id = $1 RETURNING (result ->> $2)::bigint",
        )
        .bind(run_id)
        .bind(key)
        .bind(n)
        .fetch_optional(self.pool())
        .await?;
        Ok(total.unwrap_or(n))
    }

    /// Adds `items` to the list `key` of a run's result, as the run goes on.
    pub async fn job_run_append(&self, run_id: &str, key: &str, items: &Value) -> Result<()> {
        sqlx::query(
            "UPDATE job_runs SET result = jsonb_set(result, ARRAY[$2], coalesce(result -> $2, '[]'::jsonb) || $3)
             WHERE id = $1 AND finished_at IS NULL",
        )
        .bind(run_id)
        .bind(key)
        .bind(items)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    /// Records the end of a run. Returns false when the run is unknown or already finished.
    pub async fn job_finish(&self, run_id: &str, ok: bool, message: &str, result: &Value) -> Result<bool> {
        let mut tx = self.pool().begin().await?;
        let done = sqlx::query(
            "UPDATE job_runs SET finished_at = now(), ok = $2, message = $3, result = $4
             WHERE id = $1 AND finished_at IS NULL",
        )
        .bind(run_id)
        .bind(ok)
        .bind(message)
        .bind(result)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        sqlx::query("UPDATE jobs SET current_run = NULL, holder = '', lease_until = NULL WHERE current_run = $1")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(done == 1)
    }

    pub async fn job_run(&self, run_id: &str) -> Result<Option<JobRun>> {
        sqlx::query_as("SELECT * FROM job_runs WHERE id = $1").bind(run_id).fetch_optional(self.pool()).await
    }

    /// Runs, newest first.
    pub async fn job_runs(&self, job: Option<&str>, limit: i64) -> Result<Vec<JobRun>> {
        sqlx::query_as("SELECT * FROM job_runs WHERE ($1::text IS NULL OR job = $1) ORDER BY started_at DESC LIMIT $2")
            .bind(job)
            .bind(limit)
            .fetch_all(self.pool())
            .await
    }

    /// The last finished run of a job.
    pub async fn job_last_finished(&self, name: &str) -> Result<Option<JobRun>> {
        sqlx::query_as(
            "SELECT * FROM job_runs WHERE job = $1 AND finished_at IS NOT NULL ORDER BY started_at DESC LIMIT 1",
        )
        .bind(name)
        .fetch_optional(self.pool())
        .await
    }
}
