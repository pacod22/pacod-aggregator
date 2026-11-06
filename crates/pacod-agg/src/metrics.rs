//! Prometheus metrics, on their own listener (`AGG_METRICS_LISTEN`, job `pacod-aggregator`). No
//! label ever holds a model id: the longest label is a page's address, and an administrator
//! confirms a handful of those per vendor.

use std::time::Duration;

use chrono::{DateTime, Utc};
use metrics::{describe_gauge, gauge};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use metrics_util::MetricKindMask;
use pacod_agg_jobs::jobs::{JOB_CHECK, JOB_SCAN};

use crate::api::{ApiResult, AppState};

/// A gauge that nothing set for this long leaves the output. `publish` sets every gauge once a
/// minute, so this only ends the series of a page or a vendor that is gone: without it, a deleted
/// page would keep its alert until the server restarts.
const GONE_AFTER: Duration = Duration::from_secs(300);

/// Installs the recorder and returns the handle `/metrics` renders.
pub fn install() -> anyhow::Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new().idle_timeout(MetricKindMask::GAUGE, Some(GONE_AFTER)).install_recorder()?;
    describe_gauge!("agg_jobs_enabled", "1 while the check and the scan run on their schedules");
    describe_gauge!("agg_scraper_seen_age_seconds", "Seconds since a scraper last asked for work or kept a run");
    describe_gauge!("agg_page_failures", "Checks of a confirmed page that failed in a row, by vendor and address");
    describe_gauge!("agg_pages_proposed", "Pages waiting for an administrator's decision");
    describe_gauge!("agg_models", "Models the catalogue publishes, by vendor");
    Ok(handle)
}

/// Sets the gauges from the stored state. `started` stands for a scraper's last visit until one
/// comes, so that a scraper that never comes raises the alert too.
pub async fn publish(st: &AppState, started: DateTime<Utc>) -> ApiResult<()> {
    let store = &st.store;
    let (check, scan) = (store.job_state(JOB_CHECK).await?, store.job_state(JOB_SCAN).await?);
    let seen = check.last_polled_at.max(scan.last_polled_at).unwrap_or(started);
    gauge!("agg_scraper_seen_age_seconds").set((Utc::now() - seen).num_seconds() as f64);
    let (settings, _) = st.settings().await?;
    gauge!("agg_jobs_enabled").set(if settings.enabled { 1.0 } else { 0.0 });

    let states = store.page_states().await?;
    let pages = store.pages(None, None).await?;
    gauge!("agg_pages_proposed").set(pages.iter().filter(|p| p.status == "proposed").count() as f64);
    for page in pages.iter().filter(|p| p.status == "active") {
        let failures = states.get(&page.id).map_or(0, |s| s.consecutive_failures);
        gauge!("agg_page_failures", "vendor" => page.vendor_id.clone(), "url" => page.url.clone())
            .set(f64::from(failures));
    }

    let published = store.published(None).await?;
    for vendor in store.vendors().await? {
        let models = published.iter().filter(|m| m.model.vendor_id == vendor.id).count();
        gauge!("agg_models", "vendor" => vendor.id).set(models as f64);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// An alert on a series that nothing exports never fires, and nothing says so.
    #[test]
    fn the_alert_rules_use_series_the_server_exports() {
        let rules = include_str!("../../../deploy/prometheus/rules/agg.rules.yml");
        let source = include_str!("metrics.rs");
        let used: Vec<&str> = rules
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|word| word.starts_with("agg_"))
            .collect();
        assert!(!used.is_empty());
        for series in used {
            assert!(source.contains(&format!("gauge!(\"{series}\"")), "no gauge {series}, which a rule uses");
        }
    }
}
