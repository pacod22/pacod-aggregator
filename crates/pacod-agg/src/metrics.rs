//! Prometheus metrics, on their own listener (`AGG_METRICS_LISTEN`, job `pacod-aggregator`). No
//! label ever holds a model id or an address that is not a page's: the series stay bounded.

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

/// Installs the recorder and returns the handle `/metrics` renders.
pub fn install() -> anyhow::Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new().install_recorder()?;
    metrics::describe_gauge!("agg_up", "1 while the server runs");
    metrics::gauge!("agg_up").set(1.0);
    Ok(handle)
}
