//! pacod-aggregator's server: the public catalogue, the admin and internal APIs, schedules and
//! leases, extraction, and settling what pages say into the catalogue. `main.rs` is the binary
//! around it.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use axum::routing::get;
use tokio::net::TcpListener;
use tokio::sync::watch;

pub mod api;
pub mod catalog;
pub mod config;
pub mod identity;
pub mod jobs;
pub mod metrics;
pub mod openapi;
pub mod readings;
pub mod schedule;
pub mod seeds;
pub mod settings;
pub mod settle;
pub mod store;

use api::{AppState, AuthMode};
use config::{Auth, Config};
use store::Store;

/// The database, with a few tries: it may start at the same time.
pub async fn connect(url: &str) -> anyhow::Result<Store> {
    let mut wait = Duration::from_millis(500);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match Store::connect(url).await {
            Ok(s) => return Ok(s),
            Err(e) if attempt < 12 => {
                tracing::warn!(error = %e, attempt, "the database does not answer yet");
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(5));
            }
            Err(e) => return Err(e).context("connecting to the database (DATABASE_URL)"),
        }
    }
}

/// The service, put together, but not listening yet.
pub struct App {
    pub public: Router,
    pub admin: Router,
    pub state: AppState,
}

/// Migrates the database and starts following pacod-id.
pub async fn app(config: Arc<Config>, stop: watch::Receiver<bool>) -> anyhow::Result<App> {
    let store = connect(&config.database_url).await?;
    store.migrate().await.context("migrating the database")?;
    seeds::ensure(&store).await.context("adding the vendors a new installation starts from")?;
    let egress = pacod_agg_jobs::egress::EgressPolicy::parse(&config.egress_allow_private)
        .map_err(|why| anyhow::anyhow!("AGG_EGRESS_ALLOW_PRIVATE: {why}"))?;
    let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).build()?;
    let auth = match &config.auth {
        Auth::Oidc(s) => AuthMode::Oidc(identity::Identity::start(s.clone(), stop)),
        Auth::Dev { token } => {
            tracing::warn!("AGG_AUTH=dev: one static token stands for an admin; never outside development");
            AuthMode::Dev { token: token.clone() }
        }
    };
    let state = AppState { config, store, auth: Arc::new(auth), http, egress: Arc::new(egress) };
    Ok(App { public: api::public_router(state.clone()), admin: api::admin_router(state.clone()), state })
}

/// Binds an address, or a host name with a port (an alias on a Docker network), resolved now.
async fn bind(what: &str, addr: &str) -> anyhow::Result<TcpListener> {
    let addrs: Vec<SocketAddr> =
        tokio::net::lookup_host(addr).await.with_context(|| format!("{what} {addr}"))?.collect();
    let mut last = None;
    for a in addrs {
        match TcpListener::bind(a).await {
            Ok(l) => return Ok(l),
            Err(e) => last = Some(e),
        }
    }
    Err(anyhow::anyhow!("{what} {addr}: {}", last.map_or("no address".into(), |e| e.to_string())))
}

/// Housekeeping, once a minute and at the start: closes runs a scraper left open and sets the
/// gauges.
async fn upkeep(state: AppState, mut stop: watch::Receiver<bool>) {
    let started = chrono::Utc::now();
    let mut minute = tokio::time::interval(Duration::from_secs(60));
    loop {
        tokio::select! {
            _ = minute.tick() => {}
            _ = stop.changed() => return,
        }
        if let Err(e) = jobs::close_lapsed(&state).await {
            tracing::warn!(error = %e.message, "closing the runs left open failed");
        }
        if state.config.metrics_listen.is_some()
            && let Err(e) = metrics::publish(&state, started).await
        {
            tracing::warn!(error = %e.message, "setting the gauges failed");
        }
    }
}

/// `pacod-agg serve`: listens until `shutdown` resolves.
pub async fn serve(config: Config, shutdown: impl Future<Output = ()> + Send + 'static) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let (stop_tx, stop) = watch::channel(false);
    let app = app(config.clone(), stop.clone()).await?;
    let wait = |mut stop: watch::Receiver<bool>| async move {
        let _ = stop.wait_for(|s| *s).await;
    };
    let mut servers = Vec::new();
    if let Some(addr) = &config.metrics_listen {
        let handle = metrics::install()?;
        let router = Router::new().route("/metrics", get(move || std::future::ready(handle.render())));
        let listener = bind("AGG_METRICS_LISTEN", addr).await?;
        servers
            .push(tokio::spawn(axum::serve(listener, router).with_graceful_shutdown(wait(stop.clone())).into_future()));
    }
    tokio::spawn(upkeep(app.state.clone(), stop.clone()));
    let listener = bind("AGG_ADMIN_LISTEN", &config.admin_listen).await?;
    servers
        .push(tokio::spawn(axum::serve(listener, app.admin).with_graceful_shutdown(wait(stop.clone())).into_future()));
    let listener = bind("AGG_LISTEN", &config.listen).await?;
    servers.push(tokio::spawn(axum::serve(listener, app.public).with_graceful_shutdown(wait(stop)).into_future()));
    tracing::info!(listen = %config.listen, admin = %config.admin_listen, "pacod-aggregator serving");
    shutdown.await;
    let _ = stop_tx.send(true);
    for s in servers {
        let _ = tokio::time::timeout(Duration::from_secs(10), s).await;
    }
    tracing::info!("pacod-aggregator stopped");
    Ok(())
}
