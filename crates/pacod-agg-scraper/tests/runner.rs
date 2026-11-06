//! The scraper against a server of the test's own: it takes the check's lease, fetches a feed,
//! renders pages (with a renderer of the test's), and reports each page and the run.

use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use pacod_agg_jobs::egress::EgressPolicy;
use pacod_agg_jobs::jobs::{BrowserStatus, JOB_CHECK, JOB_SCAN};
use pacod_agg_scraper::fetch::Fetcher;
use pacod_agg_scraper::render::{BoxFuture, Render, Rendered};
use pacod_agg_scraper::runner::{Runner, Server};
use pacod_agg_scraper::text::fingerprint;
use serde_json::{Value, json};

const TOKEN: &str = "test-internal-token-0123456789";

/// What the fake server was told.
#[derive(Default)]
struct Told {
    leases: Vec<Value>,
    readings: Vec<Value>,
    results: Vec<Value>,
    /// The work of the one run it hands out.
    work: Option<Value>,
    /// How many reports it takes before it answers 500; `None`: all of them.
    takes: Option<usize>,
}

impl Told {
    /// Whether the next report is one too many. The run's own record still gets through, as with a
    /// server whose disk is full: changing a row may fit where a new one does not.
    fn refuses(&mut self) -> bool {
        match &mut self.takes {
            Some(0) => true,
            Some(n) => {
                *n -= 1;
                false
            }
            None => false,
        }
    }
}

const REFUSED: (StatusCode, &str) = (StatusCode::INTERNAL_SERVER_ERROR, "no space left on device");

type Shared = Arc<Mutex<Told>>;

async fn lease(State(told): State<Shared>, headers: HeaderMap, Json(req): Json<Value>) -> Json<Value> {
    assert_eq!(headers["x-agg-internal-token"], TOKEN);
    let mut t = told.lock().unwrap();
    t.leases.push(req.clone());
    match (req["job"].as_str(), t.work.take()) {
        (Some(JOB_CHECK | JOB_SCAN), Some(work)) => {
            Json(json!({ "granted": true, "due": true, "run_id": "run_1", "work": work }))
        }
        _ => Json(json!({ "granted": true, "due": false, "run_id": null, "work": null })),
    }
}

async fn reading(
    State(told): State<Shared>,
    Path(_run): Path<String>,
    Json(r): Json<Value>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    if told.lock().unwrap().refuses() {
        return Err(REFUSED);
    }
    let outcome = match r["outcome"].as_str() {
        Some("changed") => "read",
        Some(other) => other,
        None => "unknown",
    }
    .to_owned();
    told.lock().unwrap().readings.push(r);
    Ok(Json(json!({ "outcome": outcome })))
}

async fn scan_report(
    State(told): State<Shared>,
    Path(_run): Path<String>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    if told.lock().unwrap().refuses() {
        return Err(REFUSED);
    }
    Ok(Json(json!({ "proposed": [] })))
}

async fn result(State(told): State<Shared>, Path(_run): Path<String>, Json(r): Json<Value>) -> Json<Value> {
    told.lock().unwrap().results.push(r);
    Json(json!({}))
}

async fn heartbeat() -> Json<Value> {
    Json(json!({ "keep_going": true }))
}

/// Pages as a browser would render them.
struct Pages;

const PRICING: &str = "Model | Input | Output\ngpt-x | $2.00 / 1M | $10.00 / 1M\n";

impl Render for Pages {
    fn render<'a>(&'a self, url: &'a str) -> BoxFuture<'a, anyhow::Result<Rendered>> {
        Box::pin(async move {
            match url {
                "https://vendor.example/pricing" | "https://vendor.example/models" => Ok(Rendered {
                    status: Some(200),
                    final_url: url.to_owned(),
                    title: "Pricing".into(),
                    text: PRICING.into(),
                    ..Default::default()
                }),
                "https://vendor.example/gone" => {
                    Ok(Rendered { status: Some(404), text: "Not found".into(), ..Default::default() })
                }
                _ => anyhow::bail!("the browser could not open {url}"),
            }
        })
    }

    fn status(&self) -> BoxFuture<'_, BrowserStatus> {
        Box::pin(async { BrowserStatus { ok: true, product: "Chrome/test".into(), message: String::new() } })
    }
}

async fn serve(app: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn task(page: &str, url: &str, kind: &str, previous_hash: &str, force: bool) -> Value {
    json!({ "page_id": page, "vendor_id": "openai", "vendor": "OpenAI", "url": url, "kind": kind,
            "format": "pacod", "previous_hash": previous_hash, "force": force, "known_models": ["gpt-x"] })
}

#[tokio::test]
async fn a_check_reports_every_page_and_the_run() {
    let feed = serve(Router::new().route(
        "/prices.json",
        get(|| async {
            r#"{"currency":"USD","models":{"openai/gpt-x":{"input":2.0,"output":10.0},"xai/other":{"input":1}}}"#
        }),
    ))
    .await;
    let told: Shared = Arc::default();
    let known = fingerprint(PRICING, &["gpt-x".to_owned()]);
    told.lock().unwrap().work = Some(json!({ "extraction": true, "pages": [
        task("ppg_feed", &format!("{feed}/prices.json"), "feed", "", false),
        task("ppg_same", "https://vendor.example/pricing", "page", &known, false),
        task("ppg_forced", "https://vendor.example/models", "page", &known, true),
        task("ppg_gone", "https://vendor.example/gone", "page", "", false),
        task("ppg_broken", "https://vendor.example/broken", "page", "", false),
    ] }));
    let server = serve(
        Router::new()
            .route("/internal/v1/jobs/lease", post(lease))
            .route("/internal/v1/jobs/{run}/readings", post(reading))
            .route("/internal/v1/jobs/{run}/heartbeat", post(heartbeat))
            .route("/internal/v1/jobs/{run}/result", post(result))
            .with_state(told.clone()),
    )
    .await;
    let policy = Arc::new(EgressPolicy::parse(&["127.0.0.1".to_owned()]).unwrap());
    let runner = Arc::new(Runner::new(
        Server::new(&server, TOKEN).unwrap(),
        "agg-scraper-test".to_owned(),
        Fetcher::new(policy, "test").unwrap(),
        Arc::new(Pages),
    ));

    assert!(runner.tick(JOB_CHECK).await.unwrap(), "a run happened");
    assert!(!runner.tick(JOB_SCAN).await.unwrap(), "nothing to scan");
    let t = told.lock().unwrap();
    // The scraper says who it is and that its browser answers.
    assert_eq!(t.leases[0]["agent"]["service"], "agg-scraper");
    assert_eq!(t.leases[0]["agent"]["browser"]["ok"], true);
    assert_eq!(t.leases[0]["holder"], runner.holder.as_str());

    let by = |page: &str| t.readings.iter().find(|r| r["page_id"] == page).cloned().unwrap();
    let f = by("ppg_feed");
    assert_eq!((f["outcome"].as_str(), f["http_status"].as_u64()), (Some("changed"), Some(200)), "{f}");
    let models = f["extraction"]["models"].as_array().unwrap();
    assert_eq!(models.len(), 1, "only the vendor's models: {f}");
    assert_eq!(models[0]["price"]["rates"]["input"], 2.0);
    assert_eq!(models[0]["api_id_stated"], false, "a list states no vendor's id: {f}");
    assert!(f["text"].is_null(), "a feed sends its models, not its text");
    // An unchanged page is not sent to be read; one to read anyway is, with its text.
    let same = by("ppg_same");
    assert_eq!((same["outcome"].as_str(), same["text"].is_null()), (Some("unchanged"), true), "{same}");
    let forced = by("ppg_forced");
    assert_eq!((forced["outcome"].as_str(), forced["text"].as_str()), (Some("changed"), Some(PRICING)));
    assert_eq!(forced["content_hash"], known.as_str());
    // A page that answers an error, and one the browser cannot open, fail with the reason.
    let gone = by("ppg_gone");
    assert_eq!((gone["outcome"].as_str(), gone["message"].as_str()), (Some("failed"), Some("the page answered 404")));
    assert!(by("ppg_broken")["message"].as_str().unwrap().contains("could not open"));
    // The run ends with what came of each page.
    assert_eq!(t.results.len(), 1);
    let r = &t.results[0];
    assert_eq!(
        (r["run_id"].as_str(), r["job"].as_str(), r["ok"].as_bool()),
        (Some("run_1"), Some(JOB_CHECK), Some(true))
    );
    assert_eq!(r["result"]["scraper"]["outcomes"], json!({ "failed": 2, "read": 2, "unchanged": 1 }));
}

#[tokio::test]
async fn a_server_that_cannot_be_reached_is_an_error_not_a_silent_round() {
    let runner = Runner::new(
        Server::new("http://127.0.0.1:9", TOKEN).unwrap(),
        "agg-scraper-test".to_owned(),
        Fetcher::new(Arc::new(EgressPolicy::default()), "test").unwrap(),
        Arc::new(Pages),
    );
    assert!(Arc::new(runner).tick(JOB_CHECK).await.is_err());
}

/// Runs a job against a server that takes `takes` of its reports, and returns the run's record.
async fn run_refused(job: &str, work: Value, takes: usize) -> Value {
    let told: Shared = Arc::default();
    {
        let mut t = told.lock().unwrap();
        t.work = Some(work);
        t.takes = Some(takes);
    }
    let server = serve(
        Router::new()
            .route("/internal/v1/jobs/lease", post(lease))
            .route("/internal/v1/jobs/{run}/readings", post(reading))
            .route("/internal/v1/jobs/{run}/scan", post(scan_report))
            .route("/internal/v1/jobs/{run}/heartbeat", post(heartbeat))
            .route("/internal/v1/jobs/{run}/result", post(result))
            .with_state(told.clone()),
    )
    .await;
    let runner = Arc::new(Runner::new(
        Server::new(&server, TOKEN).unwrap(),
        "agg-scraper-test".to_owned(),
        Fetcher::new(Arc::new(EgressPolicy::default()), "test").unwrap(),
        Arc::new(Pages),
    ));
    assert!(runner.tick(job).await.unwrap());
    let results = &told.lock().unwrap().results;
    assert_eq!(results.len(), 1, "the run ends once");
    results[0].clone()
}

#[tokio::test]
async fn a_check_whose_readings_did_not_reach_the_server_failed() {
    let known = fingerprint(PRICING, &["gpt-x".to_owned()]);
    let work = json!({ "extraction": true, "pages": [
        task("ppg_a", "https://vendor.example/pricing", "page", &known, false),
        task("ppg_b", "https://vendor.example/pricing", "page", &known, false),
    ] });

    let run = run_refused(JOB_CHECK, work.clone(), 0).await;
    assert_eq!(run["ok"], false, "a run that told the server nothing did not succeed");
    let message = run["message"].as_str().unwrap();
    assert!(message.starts_with("2 of 2 pages did not reach the server"), "{message}");
    assert!(message.contains("no space left on device"), "the record says why: {message}");

    // One is enough: a vendor is not published until the server heard of all its pages.
    let run = run_refused(JOB_CHECK, work.clone(), 1).await;
    assert_eq!(run["ok"], false);
    let message = run["message"].as_str().unwrap();
    assert!(message.starts_with("1 of 2 pages did not reach the server"), "{message}");

    assert_eq!(run_refused(JOB_CHECK, work, 2).await["ok"], true, "every reading arrived");
}

#[tokio::test]
async fn a_scan_whose_reports_did_not_reach_the_server_failed() {
    let vendor = |id: &str| json!({ "vendor_id": id, "name": id, "roots": [], "known_urls": [], "known_models": [] });
    let work = json!({ "vendors": [vendor("openai"), vendor("xai")], "max_pages_per_vendor": 1,
                       "min_score": 1.0, "keywords": [] });

    let run = run_refused(JOB_SCAN, work.clone(), 1).await;
    assert_eq!(run["ok"], false);
    let message = run["message"].as_str().unwrap();
    assert!(message.starts_with("1 of 2 vendors did not reach the server"), "{message}");

    assert_eq!(run_refused(JOB_SCAN, work, 2).await["ok"], true, "every report arrived");
}

/// A browser stuck on a page that never finishes.
struct Stuck;

impl Render for Stuck {
    fn render<'a>(&'a self, _url: &'a str) -> BoxFuture<'a, anyhow::Result<Rendered>> {
        Box::pin(std::future::pending())
    }

    fn status(&self) -> BoxFuture<'_, BrowserStatus> {
        Box::pin(async { BrowserStatus { ok: true, product: "Chrome/test".into(), message: String::new() } })
    }
}

async fn release(State(released): State<Arc<Mutex<Vec<Value>>>>, Json(r): Json<Value>) -> axum::http::StatusCode {
    released.lock().unwrap().push(r);
    axum::http::StatusCode::NO_CONTENT
}

#[tokio::test]
async fn a_scraper_that_stops_gives_up_its_leases_also_in_the_middle_of_a_page() {
    let told: Shared = Arc::default();
    told.lock().unwrap().work =
        Some(json!({ "pages": [task("ppg_stuck", "https://vendor.example/slow", "page", "", false)] }));
    let released: Arc<Mutex<Vec<Value>>> = Arc::default();
    let server = serve(
        Router::new()
            .route("/internal/v1/jobs/lease", post(lease))
            .route("/internal/v1/jobs/{run}/readings", post(reading))
            .route("/internal/v1/jobs/{run}/heartbeat", post(heartbeat))
            .route("/internal/v1/jobs/{run}/result", post(result))
            .with_state(told.clone())
            .merge(Router::new().route("/internal/v1/jobs/release", post(release)).with_state(released.clone())),
    )
    .await;
    let policy = Arc::new(EgressPolicy::parse(&["127.0.0.1".to_owned()]).unwrap());
    let runner = Arc::new(Runner::new(
        Server::new(&server, TOKEN).unwrap(),
        "agg-scraper-1".to_owned(),
        Fetcher::new(policy, "test").unwrap(),
        Arc::new(Stuck),
    ));
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let work = tokio::spawn(runner.run_forever(stopped));
    // It took the run, and sits on the page.
    while told.lock().unwrap().leases.is_empty() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    stop.send(true).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), work).await.expect("it stops at once").unwrap();

    let jobs: Vec<(String, String)> = released
        .lock()
        .unwrap()
        .iter()
        .map(|r| (r["job"].as_str().unwrap().to_owned(), r["holder"].as_str().unwrap().to_owned()))
        .collect();
    let holder = "agg-scraper-1".to_owned();
    assert_eq!(jobs, [(JOB_CHECK.to_owned(), holder.clone()), (JOB_SCAN.to_owned(), holder)]);
    // The run stays open: the same scraper, or the next, takes it up.
    assert!(told.lock().unwrap().results.is_empty(), "no result of a run that was not finished");
}
