//! Renders one page as the check would, and prints its text and fingerprint: to see what the
//! extraction model is given for a page. The browser is `AGG_TEST_BROWSER_URL` (`make
//! test-services` starts one); the proxy runs here, under the default egress policy.
//!
//! `cargo run -p pacod-agg-scraper --example render -- <url> [model id …]`

use std::sync::Arc;
use std::time::Duration;

use pacod_agg_jobs::egress::EgressPolicy;
use pacod_agg_scraper::browser::Browser;
use pacod_agg_scraper::render::Render;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let url = args.next().ok_or_else(|| anyhow::anyhow!("usage: render <url> [model id …]"))?;
    let known: Vec<String> = args.collect();
    let browser_url = std::env::var("AGG_TEST_BROWSER_URL").unwrap_or_else(|_| "http://127.0.0.1:29222".into());
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(pacod_agg_scraper::proxy::serve(listener, Arc::new(EgressPolicy::default())));
    let browser =
        Browser::new(&browser_url, &format!("http://host.docker.internal:{port}"), None, Duration::from_secs(45));
    let page = browser.render(&url).await?;
    println!("status {:?}, {} bytes, ended at {}", page.status, page.bytes, page.final_url);
    println!("title: {}", page.title);
    println!("fingerprint: {}", pacod_agg_scraper::text::fingerprint(&page.text, &known));
    println!("--- {} lines, {} links ---\n{}", page.text.lines().count(), page.links.len(), page.text);
    Ok(())
}
