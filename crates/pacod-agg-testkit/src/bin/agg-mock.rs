//! A vendor's site and an extraction model, for the development stack and the smoke test. Listens
//! on `AGG_MOCK_LISTEN` (default `0.0.0.0:9100`).

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    pacod_agg_logging::init("agg-mock", "info", "AGG_LOG_FORMAT");
    let listen = std::env::var("AGG_MOCK_LISTEN").unwrap_or_else(|_| "0.0.0.0:9100".to_owned());
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    tracing::info!(listen = %listen, "agg-mock listening");
    axum::serve(listener, pacod_agg_testkit::mock::router()).await?;
    Ok(())
}
