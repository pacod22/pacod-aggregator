//! The scraper's jobs from the server's side: the leases of the daily check and the weekly scan,
//! the work each run is handed, the heartbeat that keeps a run's lease, runs left open by a
//! scraper that stopped, and what the console is told about the scraper. The test plays the
//! scraper on the internal API.
//!
//! Moved from pacod-gw (commit 50fe4b7, `crates/pacod-gw-core/tests/price_scraper.rs`).

mod common;

use common::{DEV, Server};
use serde_json::{Value, json};

fn agent(browser_ok: bool) -> Value {
    json!({ "service": "agg-scraper", "version": "9.9.9",
            "browser": { "ok": browser_ok, "product": "Chrome/157.0.8103.27",
                         "message": if browser_ok { "" } else { "agg-browser:9222 refused the connection" } } })
}

async fn lease(s: &Server, job: &str, holder: &str) -> Value {
    let (st, v) =
        s.internal("/lease", json!({ "job": job, "holder": holder, "ttl_seconds": 120, "agent": agent(true) })).await;
    assert_eq!(st, 200, "{v}");
    v
}

async fn finish(s: &Server, job: &str, run_id: &str) -> (u16, Value) {
    s.internal(
        &format!("/{run_id}/result"),
        json!({ "run_id": run_id, "job": job, "finished_at": "2026-11-05T12:00:00Z", "ok": true, "message": "",
                "result": { "pages_read": 1 } }),
    )
    .await
}

fn urls(work: &Value) -> Vec<String> {
    work["pages"].as_array().unwrap().iter().map(|p| p["url"].as_str().unwrap().to_owned()).collect()
}

#[tokio::test]
async fn the_check_reads_the_confirmed_pages_on_its_own_schedule() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;

    // Before any scraper asked, the console is told that none runs.
    let (_, v) = s.api("GET", "/status", None).await;
    assert_eq!(v["scraper"]["present"], false, "{v}");
    let problems = v["problems"].to_string();
    assert!(problems.contains("agg-scraper") && problems.contains("No page is confirmed"), "{problems}");

    // Nothing to check yet: the run is skipped, and the schedule moves on.
    let l = lease(&s, "check", "scraper-1").await;
    assert_eq!((l["granted"].as_bool(), l["due"].as_bool()), (Some(true), Some(false)), "{l}");
    let (_, v) = s.api("GET", "/status", None).await;
    assert!(v["check"]["last_started_at"].is_string() && v["check"]["next_run_at"].is_string(), "{v}");
    assert_eq!((v["scraper"]["present"].as_bool(), v["scraper"]["version"].as_str()), (Some(true), Some("9.9.9")));
    assert!(!v["problems"].to_string().contains("agg-scraper"), "{v}");

    // A page is added: it is checked at the next run, asked for alone.
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    let gone = s.add_page("openai", "https://openai.example/old").await;
    assert_eq!(s.api("POST", &format!("/pages/{gone}/dismiss"), None).await.0, 200);
    sqlx::query("INSERT INTO models (id, vendor_id, api_id, key) VALUES ('agm_1', 'openai', 'gpt-x', 'gpt-x')")
        .execute(s.store.pool())
        .await
        .unwrap();
    let l = lease(&s, "check", "scraper-1").await;
    assert_eq!(l["due"], true, "{l}");
    let run_id = l["run_id"].as_str().unwrap().to_owned();
    assert_eq!(urls(&l["work"]), ["https://openai.example/pricing"], "a dismissed page is not read: {l}");
    let task = &l["work"]["pages"][0];
    assert_eq!(
        (task["page_id"].as_str(), task["kind"].as_str(), task["force"].as_bool(), task["vendor"].as_str()),
        (Some(page.as_str()), Some("page"), Some(true), Some("OpenAI"))
    );
    assert_eq!(task["known_models"], json!(["gpt-x"]), "{task}");
    assert_eq!(l["work"]["extraction"], false);
    assert!(s.api("GET", "/status", None).await.1["problems"].to_string().contains("No extraction model"));
    // Another scraper gets nothing while the run is on.
    assert_eq!(lease(&s, "check", "scraper-2").await["granted"], false);
    // The run ends with what the scraper reports.
    let (st, v) = finish(&s, "check", &run_id).await;
    assert_eq!(st, 200, "{v}");
    let (_, runs) = s.api("GET", "/runs?job=check", None).await;
    assert_eq!(
        (
            runs[0]["id"].as_str(),
            runs[0]["trigger"].as_str(),
            runs[0]["covers"].clone(),
            runs[0]["result"]["pages_read"].as_u64()
        ),
        (Some(run_id.as_str()), Some("page"), json!([page]), Some(1))
    );
    assert_eq!(s.api("GET", "/runs?job=nonsense", None).await.0, 422);

    // "Check now" checks every page, by hand.
    assert_eq!(s.api("POST", "/runs/check", None).await.0, 202);
    let l = lease(&s, "check", "scraper-1").await;
    let run_id = l["run_id"].as_str().unwrap().to_owned();
    finish(&s, "check", &run_id).await;
    let (_, runs) = s.api("GET", "/runs?job=check", None).await;
    assert_eq!(runs[0]["trigger"], "manual");
    // Done until the schedule says so.
    assert_eq!(lease(&s, "check", "scraper-1").await["due"], false);
}

#[tokio::test]
async fn the_scan_starts_from_the_pages_and_the_vendors_seeds() {
    let s = Server::start(|_| {}).await;
    sqlx::query("DELETE FROM vendors WHERE id NOT IN ('deepseek', 'mistral')").execute(s.store.pool()).await.unwrap();
    let models = s.add_page("mistral", "https://mistral.example/models").await;

    // Never scanned: due at once. A vendor with nowhere to start is left out, a list is no place
    // to look for pages.
    let l = lease(&s, "scan", "scraper-1").await;
    assert_eq!(l["due"], true, "{l}");
    let vendors = l["work"]["vendors"].as_array().unwrap();
    let of = |id: &str| vendors.iter().find(|p| p["vendor_id"] == id).cloned().unwrap();
    assert_eq!(
        of("deepseek")["roots"],
        json!([{ "url": "https://api-docs.deepseek.com/quick_start/pricing", "page_id": null, "seed": true }])
    );
    assert_eq!(
        of("mistral")["roots"],
        json!([{ "url": "https://mistral.example/models", "page_id": models, "seed": false }])
    );
    assert_eq!(of("mistral")["known_urls"].as_array().unwrap().len(), 3, "its lists too");
    assert_eq!(
        (l["work"]["max_pages_per_vendor"].as_u64(), l["work"]["keywords"][0].as_str()),
        (Some(12), Some("pricing"))
    );
    let run_id = l["run_id"].as_str().unwrap().to_owned();
    finish(&s, "scan", &run_id).await;
    let (_, runs) = s.api("GET", "/runs?job=scan", None).await;
    assert_eq!((runs[0]["trigger"].as_str(), runs[0]["covers"].as_array().map(Vec::len)), (Some("schedule"), Some(2)));

    // Asked for by hand, the scan runs again; the check is a job of its own.
    assert_eq!(s.api("POST", "/runs/scan", None).await.0, 202);
    let (_, v) = s.api("GET", "/status", None).await;
    assert_eq!(v["scan"]["run_requested"], true, "{v}");
    assert_eq!(v["scan"]["schedule"], "41 4 * * 1");
    assert_eq!(lease(&s, "scan", "scraper-1").await["due"], true);
}

#[tokio::test]
async fn a_run_left_open_is_closed_and_run_once_more() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    s.add_page("openai", "https://openai.example/pricing").await;
    let first = lease(&s, "check", "scraper-1").await;
    let run_id = first["run_id"].as_str().unwrap().to_owned();

    // The heartbeat keeps the run its holder's; another holder is told to stop.
    let beat = |holder: &str| json!({ "holder": holder, "ttl_seconds": 120 });
    let (st, v) = s.internal(&format!("/{run_id}/heartbeat"), beat("scraper-1")).await;
    assert_eq!((st, v["keep_going"].as_bool()), (200, Some(true)), "{v}");
    assert_eq!(s.internal(&format!("/{run_id}/heartbeat"), beat("scraper-2")).await.1["keep_going"], false);

    // The scraper stops; another one takes over the lease. The run is closed as failed and run
    // once more, over the same pages.
    s.lapse("check").await;
    let second = lease(&s, "check", "scraper-2").await;
    assert_eq!(second["due"], true, "{second}");
    let retry = second["run_id"].as_str().unwrap().to_owned();
    assert_ne!(retry, run_id);
    assert_eq!(urls(&second["work"]), urls(&first["work"]));
    let (_, runs) = s.api("GET", "/runs?job=check", None).await;
    assert_eq!((runs[0]["trigger"].as_str(), runs[0]["ok"].clone()), (Some("retry"), Value::Null));
    assert_eq!(runs[1]["ok"], false);
    assert!(runs[1]["message"].as_str().unwrap().contains("stopped"), "{runs}");
    // The first scraper, back, is told that the run is no longer its own.
    assert_eq!(s.internal(&format!("/{run_id}/heartbeat"), beat("scraper-1")).await.1["keep_going"], false);
    assert_eq!(finish(&s, "check", &run_id).await.0, 409);

    // The second try is left open too: closed, and not tried a third time.
    s.lapse("check").await;
    let third = lease(&s, "check", "scraper-3").await;
    assert_eq!((third["granted"].as_bool(), third["due"].as_bool()), (Some(true), Some(false)), "{third}");
    let (_, v) = s.api("GET", "/status", None).await;
    assert_eq!(v["check"]["running"], false);
    assert_eq!(v["check"]["last_run"]["trigger"], "retry");
}

#[tokio::test]
async fn a_scraper_that_restarts_takes_up_its_own_run_without_waiting() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    s.add_page("openai", "https://openai.example/pricing").await;
    let first = lease(&s, "check", "agg-scraper-1").await;
    let run_id = first["run_id"].as_str().unwrap();

    // Killed without a word, it comes back under its name, long before its lease runs out: the
    // run is its own, and goes on.
    let again = lease(&s, "check", "agg-scraper-1").await;
    assert_eq!((again["run_id"].as_str(), again["due"].as_bool()), (Some(run_id), Some(true)), "{again}");
    assert_eq!(urls(&again["work"]), urls(&first["work"]));
    // While the lease stands, nobody else gets in.
    assert_eq!(lease(&s, "check", "agg-scraper-2").await["granted"], false);
}

#[tokio::test]
async fn a_scraper_that_stops_gives_up_its_lease_at_once() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    s.add_page("openai", "https://openai.example/pricing").await;
    let first = lease(&s, "check", "agg-scraper-1").await;
    let run_id = first["run_id"].as_str().unwrap().to_owned();

    // Someone else's word changes nothing.
    let release = |holder: &str| json!({ "job": "check", "holder": holder });
    assert_eq!(s.internal("/release", release("agg-scraper-2")).await.0, 204);
    assert_eq!(lease(&s, "check", "agg-scraper-2").await["granted"], false);

    // Its own does. Its run stays open, and it takes it up again when it comes back...
    assert_eq!(s.internal("/release", release("agg-scraper-1")).await.0, 204);
    let back = lease(&s, "check", "agg-scraper-1").await;
    assert_eq!(back["run_id"].as_str(), Some(run_id.as_str()), "{back}");

    // ...or another scraper takes over at once, with no lease to wait for.
    s.internal("/release", release("agg-scraper-1")).await;
    let other = lease(&s, "check", "agg-scraper-2").await;
    assert_eq!(other["due"], true, "{other}");
    assert_ne!(other["run_id"].as_str(), Some(run_id.as_str()));
    let (_, runs) = s.api("GET", "/runs?job=check", None).await;
    assert_eq!(runs[0]["trigger"], "retry");
}

#[tokio::test]
async fn a_scraper_without_its_browser_is_named_on_the_console() {
    let s = Server::start(|_| {}).await;
    let (st, _) =
        s.internal("/lease", json!({ "job": "scan", "holder": "s", "ttl_seconds": 60, "agent": agent(false) })).await;
    assert_eq!(st, 200);
    let (_, v) = s.api("GET", "/status", None).await;
    assert_eq!(v["scraper"]["browser"]["ok"], false, "{v}");
    assert!(v["problems"].to_string().contains("agg-browser:9222 refused the connection"), "{v}");
    // A stored schedule that cannot be read is named too.
    sqlx::query("UPDATE settings SET doc = '{\"scan_schedule\": \"0 3 1 * 1\"}'")
        .execute(s.store.pool())
        .await
        .unwrap();
    let (_, v) = s.api("GET", "/status", None).await;
    assert!(v["problems"].to_string().contains("scan_schedule"), "{v}");
}

#[tokio::test]
async fn what_the_scan_finds_is_proposed_and_what_moved_is_flagged() {
    let s = Server::start(|_| {}).await;
    sqlx::query("DELETE FROM vendors WHERE id NOT IN ('deepseek', 'mistral')").execute(s.store.pool()).await.unwrap();
    let models = s.add_page("mistral", "https://mistral.example/models").await;
    let old = s.add_page("mistral", "https://mistral.example/old-prices").await;
    assert_eq!(s.api("POST", &format!("/pages/{old}/dismiss"), None).await.0, 200);
    let l = lease(&s, "scan", "scraper-1").await;
    let run_id = l["run_id"].as_str().unwrap().to_owned();
    let path = format!("/{run_id}/scan");

    // DeepSeek: the seed answers, and the scan found its models page; a page that shows no prices
    // scores below the bar.
    let (st, v) = s
        .internal(
            &path,
            json!({
                "vendor_id": "deepseek", "considered": 40, "rendered": 6,
                "roots": [{ "url": "https://api-docs.deepseek.com/quick_start/pricing", "outcome": "ok", "score": 30.0 }],
                "found": [
                    { "url": "https://api-docs.deepseek.com/quick_start/pricing/", "score": 40.0, "title": "Models & Pricing" },
                    { "url": "https://api-docs.deepseek.com/news", "score": 2.0, "title": "News" },
                    { "url": "https://api-docs.deepseek.com/api/list-models#top", "score": 25.0, "title": "Lists Models" }
                ]
            }),
        )
        .await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        v["proposed"],
        json!(["https://api-docs.deepseek.com/quick_start/pricing", "https://api-docs.deepseek.com/api/list-models"])
    );
    // Mistral: its models page moved; a page an administrator dismissed is not proposed again.
    let (_, v) = s
        .internal(
            &path,
            json!({
                "vendor_id": "mistral",
                "roots": [{ "url": "https://mistral.example/models", "page_id": models, "outcome": "moved",
                            "final_url": "https://mistral.example/v2/models", "http_status": 200 }],
                "found": [{ "url": "https://mistral.example/old-prices", "score": 50.0 }]
            }),
        )
        .await;
    assert_eq!(v["proposed"], json!(["https://mistral.example/v2/models"]), "{v}");
    assert_eq!(v["flagged"][0]["flag"], "moved");
    // A vendor the run does not scan is refused.
    assert_eq!(s.internal(&path, json!({ "vendor_id": "openai" })).await.0, 409);

    let (_, proposed) = s.api("GET", "/pages?status=proposed", None).await;
    let found: Vec<(String, String)> = proposed
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["url"].as_str().unwrap().to_owned(), p["found_by"].as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(found.len(), 3, "{proposed}");
    assert!(found.contains(&("https://api-docs.deepseek.com/quick_start/pricing".into(), "seed".into())));
    assert!(found.contains(&("https://api-docs.deepseek.com/api/list-models".into(), "scan".into())));
    let moved = proposed.as_array().unwrap().iter().find(|p| p["url"] == "https://mistral.example/v2/models").unwrap();
    assert_eq!(moved["moved_from"], "https://mistral.example/models");
    let (_, mine) = s.api("GET", &format!("/pages/{models}"), None).await;
    assert_eq!(
        (mine["state"]["flag"].as_str(), mine["state"]["moved_to"].as_str()),
        (Some("moved"), Some("https://mistral.example/v2/models"))
    );
    finish(&s, "scan", &run_id).await;
    let (_, runs) = s.api("GET", "/runs?job=scan", None).await;
    assert_eq!(runs[0]["result"]["vendors"][0]["considered"], 40, "{runs}");
    assert_eq!(s.api("GET", "/status", None).await.1["proposed_pages"], 3);

    // The next scan finds the same: nothing is proposed twice. Proposed pages are no place to
    // start from, and the seed is known now.
    assert_eq!(s.api("POST", "/runs/scan", None).await.0, 202);
    let l = lease(&s, "scan", "scraper-1").await;
    let run_id = l["run_id"].as_str().unwrap().to_owned();
    let vendors = l["work"]["vendors"].as_array().unwrap();
    assert!(vendors.iter().all(|p| p["vendor_id"] != "deepseek"), "nowhere to start for DeepSeek: {l}");
    let (_, v) = s
        .internal(
            &format!("/{run_id}/scan"),
            json!({ "vendor_id": "mistral", "found": [{ "url": "https://mistral.example/v2/models", "score": 50.0 }] }),
        )
        .await;
    assert_eq!(v["proposed"], json!([]), "{v}");
}

#[tokio::test]
async fn the_internal_token_opens_the_job_api_and_nothing_else() {
    let s = Server::start(|_| {}).await;
    let body = json!({ "job": "check", "holder": "x", "ttl_seconds": 60 });
    let post = |token: &'static str| {
        s.http
            .post(format!("{}/internal/v1/jobs/lease", s.admin))
            .header("x-agg-internal-token", token)
            .json(&body)
            .send()
    };
    assert_eq!(post("wrong-token").await.unwrap().status(), 401);
    assert_eq!(
        s.http.post(format!("{}/internal/v1/jobs/lease", s.admin)).json(&body).send().await.unwrap().status(),
        401
    );
    // The development token opens the console's API, not the scraper's...
    assert_eq!(post(DEV).await.unwrap().status(), 401);
    // ...and the scraper's opens nothing of the console's.
    assert_eq!(s.get(format!("{}/api/v1/vendors", s.admin), Some(common::INTERNAL)).await.0, 401);
    // It is not on the public listener.
    let public = s.http.post(format!("{}/internal/v1/jobs/lease", s.public)).json(&body).send().await.unwrap();
    assert_eq!(public.status(), 404);
    // A job that does not exist is refused.
    let (st, _) = s.internal("/lease", json!({ "job": "nothing", "holder": "x", "ttl_seconds": 60 })).await;
    assert_eq!(st, 422);
}

#[tokio::test]
async fn without_an_internal_token_the_job_api_answers_no_one() {
    let s = Server::start(|c| c.internal_token = None).await;
    assert_eq!(s.internal("/lease", json!({ "job": "check", "holder": "x", "ttl_seconds": 60 })).await.0, 401);
    assert!(s.api("GET", "/status", None).await.1["problems"].to_string().contains("AGG_INTERNAL_TOKEN"));
}
