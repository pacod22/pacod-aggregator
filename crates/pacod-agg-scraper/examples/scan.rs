//! Scans one vendor's site as the weekly scan would, from the pages given, and prints what it
//! found with the scores: to see what the scan proposes before an administrator confirms it.
//! The browser is `AGG_TEST_BROWSER_URL`; the proxy runs here, under the default egress policy.
//!
//! `cargo run -p pacod-agg-scraper --example scan -- <page url>… -- [model id …]`

use std::sync::Arc;
use std::time::Duration;

use pacod_agg_jobs::egress::EgressPolicy;
use pacod_agg_jobs::jobs::{ScanRoot, ScanVendor, ScanWork};
use pacod_agg_scraper::browser::Browser;
use pacod_agg_scraper::fetch::Fetcher;
use pacod_agg_scraper::runner::{Runner, Server};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").unwrap_or(args.len());
    let (roots, models) = (&args[..split], args.get(split + 1..).unwrap_or_default());
    anyhow::ensure!(!roots.is_empty(), "usage: scan <page url>… -- [model id …]");
    let browser_url = std::env::var("AGG_TEST_BROWSER_URL").unwrap_or_else(|_| "http://127.0.0.1:29222".into());
    let policy = Arc::new(EgressPolicy::default());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(pacod_agg_scraper::proxy::serve(listener, policy.clone()));
    let browser =
        Browser::new(&browser_url, &format!("http://host.docker.internal:{port}"), None, Duration::from_secs(45));
    let runner = Runner::new(
        Server::new("http://127.0.0.1:9", "unused-token-0123456789")?,
        "agg-scraper-example".to_owned(),
        Fetcher::new(policy, &pacod_agg_scraper::default_user_agent())?,
        Arc::new(browser),
    );
    let vendor = ScanVendor {
        vendor_id: "prv".into(),
        name: "vendor".into(),
        roots: roots.iter().map(|u| ScanRoot { url: u.clone(), page_id: None, seed: true }).collect(),
        known_urls: Vec::new(),
        known_models: models.to_vec(),
    };
    let keywords = ["pricing", "price", "models", "rates", "billing"].map(String::from).to_vec();
    let work = ScanWork { vendors: Vec::new(), max_pages_per_vendor: 12, min_score: 10.0, keywords };
    let report = pacod_agg_scraper::scan::scan_vendor(&runner, &vendor, &work).await;
    for r in &report.roots {
        println!("start {} → {} ({:?}, score {:?}) {}", r.url, r.outcome, r.http_status, r.score, r.message);
    }
    println!("considered {}, rendered {}", report.considered, report.rendered);
    for f in &report.found {
        println!("{:>5.1}  {}  [{}]  prices {} models {:?}", f.score, f.url, f.title, f.price_hits, f.model_hits);
    }
    Ok(())
}
