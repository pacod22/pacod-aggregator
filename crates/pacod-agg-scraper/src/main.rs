use pacod_agg_scraper::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // RUST_LOG sets the level, AGG_LOG_FORMAT the format (json, the default, or text),
    // SERVICE_NAME the service key (pacod-demo/docs/LOGGING.md).
    pacod_agg_logging::init("agg-scraper", "info", "AGG_LOG_FORMAT");
    let config = Config::from_env()?;
    pacod_agg_scraper::run(config).await
}
