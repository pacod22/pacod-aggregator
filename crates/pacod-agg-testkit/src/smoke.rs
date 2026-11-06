//! The whole path through a running stack: the scan finds a vendor's pages on the mock's site, an
//! administrator confirms them, a check renders them in the browser, the model reads them, the
//! vendor settles, and a consumer reads the catalogue with the crate a gateway reads it with. Then
//! the page changes, and the catalogue follows. It signs in through pacod-id as a person does, so
//! it also covers the server's manifest, the roles it follows and the tokens it takes.

use std::time::{Duration, Instant};

use anyhow::{Context, bail, ensure};
use pacod_agg_catalog::{Catalog, Format, Found, parse};
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::oidc::{self, IdClient, Session};

/// Where the stack is. The defaults are the development stack's.
pub struct Options {
    /// The console's API.
    pub admin: String,
    /// The public catalogue.
    pub public: String,
    /// The mock's site as the scraper's browser reaches it.
    pub mock: String,
    /// The mock as this program reaches it.
    pub mock_control: String,
    /// The metrics, as this program reaches them.
    pub metrics: String,
    /// The console, as a browser reaches it.
    pub console: String,
    /// A token to call with (`AGG_AUTH=dev`). Without one, the test signs in at `issuer`.
    pub token: Option<String>,
    pub issuer: String,
    pub email: String,
    pub password: String,
}

impl Options {
    pub fn from_env() -> Self {
        let var = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_owned());
        Self {
            admin: var("SMOKE_ADMIN_URL", "http://127.0.0.1:18151"),
            public: var("SMOKE_PUBLIC_URL", "http://127.0.0.1:18150"),
            mock: var("SMOKE_MOCK_URL", "http://agg-mock:9100"),
            mock_control: var("SMOKE_MOCK_CONTROL_URL", "http://127.0.0.1:19100"),
            metrics: var("SMOKE_METRICS_URL", "http://127.0.0.1:19150/metrics"),
            console: var("SMOKE_CONSOLE_URL", "http://localhost:13150"),
            token: std::env::var("SMOKE_TOKEN").ok().filter(|t| !t.is_empty()),
            issuer: var("SMOKE_ID_ISSUER", "http://127.0.0.1:18372"),
            email: var("SMOKE_EMAIL", "admin@pacod-aggregator.test"),
            password: var("SMOKE_PASSWORD", "pacod-aggregator-dev"),
        }
    }
}

/// Who the test calls as.
enum Caller {
    Token(String),
    Person(Mutex<Session>),
}

struct Smoke {
    o: Options,
    http: reqwest::Client,
    caller: Caller,
}

fn step(name: &str, detail: &str) {
    println!("ok    {name:<52} {detail}");
}

impl Smoke {
    async fn token(&self) -> anyhow::Result<String> {
        match &self.caller {
            Caller::Token(token) => Ok(token.clone()),
            Caller::Person(session) => session.lock().await.access_token().await,
        }
    }

    async fn api(&self, method: &str, path: &str, body: Option<Value>) -> anyhow::Result<(u16, Value)> {
        let mut request = self
            .http
            .request(method.parse()?, format!("{}/api/v1{path}", self.o.admin))
            .bearer_auth(self.token().await?);
        if let Some(b) = body {
            request = request.json(&b);
        }
        let r = request.send().await.with_context(|| format!("{method} {path}"))?;
        let status = r.status().as_u16();
        Ok((status, r.json().await.unwrap_or(Value::Null)))
    }

    /// Asks `done` until it answers, or the time is up.
    async fn wait_for<T>(
        &self,
        what: &str,
        within: Duration,
        mut done: impl AsyncFnMut() -> anyhow::Result<Option<T>>,
    ) -> anyhow::Result<T> {
        let started = Instant::now();
        loop {
            if let Some(found) = done().await? {
                return Ok(found);
            }
            ensure!(started.elapsed() < within, "{what}: nothing after {} s", within.as_secs());
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn catalog(&self, vendor: &str) -> anyhow::Result<(Catalog, String)> {
        let r = self
            .http
            .get(format!("{}/v1/catalog?vendor={vendor}", self.o.public))
            .bearer_auth(self.token().await?)
            .send()
            .await?
            .error_for_status()?;
        let tag = r.headers().get("etag").context("no ETag")?.to_str()?.to_owned();
        Ok((parse(&r.bytes().await?, Some(Format::Catalog)).context("reading the catalogue")?, tag))
    }

    async fn variant(&self, variant: u8) -> anyhow::Result<()> {
        let url = format!("{}/__mock/pricing", self.o.mock_control.trim_end_matches('/'));
        self.http.post(url).json(&json!({ "variant": variant })).send().await?.error_for_status()?;
        Ok(())
    }

    /// Waits until a page was read after `since`.
    async fn read_after(&self, page: &str, since: &Value) -> anyhow::Result<()> {
        self.wait_for("the page to be read", Duration::from_secs(120), async || {
            let (_, p) = self.api("GET", &format!("/pages/{page}"), None).await?;
            let outcome = p["state"]["outcome"].as_str().unwrap_or_default();
            if outcome == "failed" {
                bail!("the page failed: {}", p["state"]["message"]);
            }
            Ok((outcome == "read" && p["state"]["last_read_at"] != *since).then_some(()))
        })
        .await
    }
}

pub async fn run(o: Options) -> anyhow::Result<()> {
    let caller = match &o.token {
        Some(token) => Caller::Token(token.clone()),
        None => {
            let client = IdClient {
                issuer: o.issuer.clone(),
                client_id: "agg-smoke".into(),
                redirect_uri: "http://127.0.0.1:13999/callback".into(),
            };
            let session = Session::sign_in(client, &o.email, &o.password).await?;
            step("a person signs in through pacod-id", &o.email);
            Caller::Person(Mutex::new(session))
        }
    };
    let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?;
    let s = Smoke { o, http, caller };
    let vendor = format!("smoke-{}", &uuid::Uuid::now_v7().simple().to_string()[24..]);
    let (st, settings) = s
        .wait_for("the server to know the signed-in person", Duration::from_secs(60), async || {
            // The role comes through pacod-id's feed, a moment after the manifest arrives.
            let (st, v) = s.api("GET", "/settings", None).await?;
            Ok((st != 401 && st != 403).then_some((st, v)))
        })
        .await?;
    ensure!(st == 200, "the settings answered {st}: {settings}");
    let result = walk(&s, &vendor, settings.clone()).await;
    // Whatever came of it: the next run starts from the first page, no vendor stays behind, and
    // the scan of the real vendors keeps its own threshold.
    let _ = s.variant(1).await;
    let _ = s.api("DELETE", &format!("/vendors/{vendor}"), None).await;
    let mut before = settings;
    before["version"] = s.api("GET", "/settings", None).await?.1["version"].clone();
    let (st, v) = s.api("PUT", "/settings", Some(before)).await?;
    ensure!(st == 200 || result.is_err(), "putting the settings back answered {st}: {v}");
    result?;
    // A person signs in to the console; a token stands for nobody who could.
    if matches!(s.caller, Caller::Person(_)) {
        console(&s).await?;
    }
    println!("all steps passed");
    Ok(())
}

/// The value of a redirect's `location` header.
fn location(r: &reqwest::Response) -> anyhow::Result<String> {
    let to = r.headers().get("location").with_context(|| format!("{} carries no redirect", r.status()))?;
    Ok(to.to_str()?.to_owned())
}

/// The console as a browser uses it: the sign-in with its own client, every page against the real
/// server, and forms that reach it. A page that type-checks can still fail on what the server
/// really sends, and only a posted form shows that the fields map onto the API.
async fn console(s: &Smoke) -> anyhow::Result<()> {
    let base = s.o.console.trim_end_matches('/');
    let issuer = s.o.issuer.trim_end_matches('/');
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?;

    let r = http.get(format!("{base}/catalogue")).send().await?;
    let to = location(&r)?;
    ensure!(r.status() == 303 && to.starts_with("/auth/login"), "without a session the console sent {to}");
    let r = http.get(format!("{base}{to}")).send().await?;
    let started = oidc::cookies(&r);
    let authorize = location(&r)?;
    ensure!(authorize.starts_with(&format!("{issuer}/oidc/authorize")), "the sign-in went to {authorize}");
    ensure!(authorize.contains("client_id=pacod-agg-ui"), "the sign-in names another client: {authorize}");
    let back = oidc::walk(&http, issuer, &authorize, &s.o.email, &s.o.password).await?;
    ensure!(back.starts_with(&format!("{base}/auth/callback")), "pacod-id sent the browser to {back}");
    let r = http.get(&back).header("cookie", started).send().await?;
    ensure!(r.status().is_redirection(), "the callback answered {}", r.status());
    let session = oidc::cookies(&r);
    ensure!(session.contains("agg_session"), "the callback set no session");
    step("a person signs in to the console", "");

    let (_, models) = s.api("GET", "/models?limit=1", None).await?;
    let model = models["items"][0]["id"].as_str().context("the catalogue is empty")?;
    let (_, vendors) = s.api("GET", "/vendors", None).await?;
    let vendor = vendors[0]["id"].as_str().context("there is no vendor")?;
    let pages = [
        ("/".to_owned(), "Scraper"),
        ("/catalogue".to_owned(), "Catalogue"),
        (format!("/catalogue/{model}"), "Properties"),
        ("/vendors".to_owned(), "Vendors"),
        (format!("/vendors/{vendor}"), "its models"),
        ("/pages".to_owned(), "Pages"),
        ("/runs".to_owned(), "Latest changes"),
        ("/settings".to_owned(), "Schedules"),
    ];
    for (path, says) in &pages {
        let r = http.get(format!("{base}{path}")).header("cookie", &session).send().await?;
        let status = r.status();
        let body = r.text().await?;
        ensure!(status == 200 && body.contains(says), "{path} answered {status} without {says:?}");
    }
    step("every page of the console shows", &format!("{} pages", pages.len()));

    let post = |action: &str, origin: &str| {
        let url = format!("{base}{action}");
        http.post(url).header("cookie", &session).header("origin", origin.to_owned())
    };
    let r = post("/runs?/check", "https://elsewhere.example").form(&[("x", "y")]).send().await?;
    ensure!(r.status() == 403, "a form from elsewhere answered {}", r.status());
    let r = post("/runs?/check", base).form(&[("x", "y")]).send().await?;
    let status = r.status();
    let body = r.text().await?;
    ensure!(status == 200 && body.contains("A check is asked for"), "the console's own form answered {status}");
    step("a form works, and one from elsewhere is refused", "");

    // The settings form holds every setting: saved as it stands, nothing may change.
    let (_, before) = s.api("GET", "/settings", None).await?;
    let joined = |v: &Value| {
        v.as_array().map_or(String::new(), |list| list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
    };
    let shown = |v: &Value| v.as_str().map_or_else(|| v.to_string(), str::to_owned);
    let mut fields = vec![
        ("version".to_owned(), shown(&before["version"])),
        ("schedule".to_owned(), shown(&before["schedule"])),
        ("scan_schedule".to_owned(), shown(&before["scan_schedule"])),
        ("time_zone".to_owned(), shown(&before["time_zone"])),
        ("extraction_context_window".to_owned(), shown(&before["extraction_context_window"])),
        ("max_extractions_per_run".to_owned(), shown(&before["max_extractions_per_run"])),
        ("max_pages_per_vendor".to_owned(), shown(&before["scan"]["max_pages_per_vendor"])),
        ("min_score".to_owned(), shown(&before["scan"]["min_score"])),
        ("keywords".to_owned(), joined(&before["scan"]["keywords"])),
        ("openrouter_price_vendors".to_owned(), joined(&before["openrouter_price_vendors"])),
    ];
    if before["enabled"] == true {
        fields.push(("enabled".to_owned(), "on".to_owned()));
    }
    if let Some(model) = before["extraction_model"].as_str() {
        fields.push(("extraction_model".to_owned(), model.to_owned()));
    }
    let r = post("/settings?/save", base).form(&fields).send().await?;
    let status = r.status();
    let body = r.text().await?;
    ensure!(status == 200 && body.contains("The settings are saved"), "saving the form answered {status}");
    let (_, mut after) = s.api("GET", "/settings", None).await?;
    let version = after["version"].as_i64().unwrap_or_default();
    ensure!(version == before["version"].as_i64().unwrap_or_default() + 1, "the form changed no version: {after}");
    after["version"] = before["version"].clone();
    ensure!(after == before, "the form changed the settings:\n{before}\n{after}");
    step("the settings form says what the server holds", "");
    Ok(())
}

async fn walk(s: &Smoke, vendor: &str, mut settings: Value) -> anyhow::Result<()> {
    let mock = s.o.mock.trim_end_matches('/').to_owned();
    s.variant(1).await?;

    let status = s
        .wait_for("a scraper and its browser", Duration::from_secs(120), async || {
            let (st, v) = s.api("GET", "/status", None).await?;
            ensure!(st == 200, "the status answered {st}: {v}");
            Ok((v["scraper"]["browser"]["ok"] == true).then_some(v))
        })
        .await?;
    step("a scraper runs and reaches its browser", status["scraper"]["browser"]["product"].as_str().unwrap_or(""));

    settings["extraction_model"] = json!("mock-reader");
    // The mock's pages are short, and the new vendor has no models yet that would add to their
    // score: the threshold is for the real vendors' pages.
    settings["scan"]["min_score"] = json!(4.0);
    let (st, v) = s.api("PUT", "/settings", Some(settings)).await?;
    ensure!(st == 200, "saving the settings answered {st}: {v}");

    let body = json!({ "id": vendor, "display_name": "Smoke vendor", "complete": true,
                       "sites": [format!("{mock}/__mock/site/about")] });
    let (st, v) = s.api("POST", "/vendors", Some(body)).await?;
    ensure!(st == 201, "adding the vendor answered {st}: {v}");

    // The scan starts from the seed, follows the sitemap and keeps out of what robots.txt forbids.
    // It waits its turn: on a new stack the first scan of the real vendors takes minutes.
    let (st, _) = s.api("POST", "/runs/scan", None).await?;
    ensure!(st == 202, "asking for a scan answered {st}");
    let proposed: Vec<Value> = s
        .wait_for("the scan's proposals", Duration::from_secs(420), async || {
            let (_, pages) = s.api("GET", &format!("/pages?vendor_id={vendor}&status=proposed"), None).await?;
            let list = pages.as_array().cloned().unwrap_or_default();
            let found = |wanted: &str| list.iter().any(|p| p["url"].as_str().is_some_and(|u| u.ends_with(wanted)));
            Ok((found("/__mock/site/pricing") && found("/__mock/site/models")).then_some(list))
        })
        .await?;
    let urls: Vec<&str> = proposed.iter().filter_map(|p| p["url"].as_str()).collect();
    ensure!(!urls.iter().any(|u| u.contains("/private/") || u.ends_with("/news")), "proposed too much: {urls:?}");
    step("the scan proposes the vendor's pages", &format!("{} pages", proposed.len()));

    // Only the pages with prices and models; the seed itself tells nothing.
    let mut pricing = String::new();
    let mut ids = Vec::new();
    for p in &proposed {
        let (id, url) = (p["id"].as_str().unwrap_or_default(), p["url"].as_str().unwrap_or_default());
        let keep = url.ends_with("/pricing") || url.ends_with("/models");
        let action = if keep { "confirm" } else { "dismiss" };
        let (st, v) = s.api("POST", &format!("/pages/{id}/{action}"), None).await?;
        ensure!(st == 200, "{action} answered {st}: {v}");
        if url.ends_with("/pricing") {
            pricing = id.to_owned();
        }
        if keep {
            ids.push(id.to_owned());
        }
    }
    for id in &ids {
        s.read_after(id, &Value::Null).await?;
    }
    step("a check renders the pages and the model reads them", "");

    let (catalog, tag) = s
        .wait_for("the vendor to settle", Duration::from_secs(60), async || {
            let (c, tag) = s.catalog(vendor).await?;
            Ok((c.models.iter().any(|m| m.price.is_some() && m.properties.context_window.is_some())
                && c.vendor(vendor).is_some_and(|v| v.complete))
            .then_some((c, tag)))
        })
        .await?;
    let index = catalog.index();
    let Found::One(steady) = index.find(vendor, "tracked-steady") else { bail!("no tracked-steady in {catalog:?}") };
    let price = steady.price.as_ref().context("tracked-steady has no price")?;
    ensure!(price.rates.input == Some(2.2), "tracked-steady's price: {price:?}");
    ensure!(price.rates.cache_read == Some(0.22), "tracked-steady's cached rate: {price:?}");
    ensure!(steady.properties.context_window == Some(128_000), "its window: {:?}", steady.properties);
    ensure!(steady.properties.vision == Some(true), "its vision: {:?}", steady.properties);
    ensure!(steady.api_id_confirmed, "the vendor's page states its id");
    ensure!(price.checked_at.is_some(), "no checked_at: {price:?}");
    step("a consumer reads the catalogue as a gateway does", &format!("{} models", catalog.models.len()));

    let unchanged = s
        .http
        .get(format!("{}/v1/catalog?vendor={vendor}", s.o.public))
        .bearer_auth(s.token().await?)
        .header("if-none-match", &tag)
        .send()
        .await?;
    ensure!(unchanged.status() == 304, "asked with its tag, the catalogue answered {}", unchanged.status());
    step("a catalogue that did not change is not sent again", "");

    let (_, before) = s.api("GET", &format!("/pages/{pricing}"), None).await?;
    let since = before["state"]["last_read_at"].clone();
    s.variant(2).await?;
    let (st, _) = s.api("POST", &format!("/pages/{pricing}/check"), None).await?;
    ensure!(st == 202, "checking the page answered {st}");
    s.read_after(&pricing, &since).await?;
    let catalog = s
        .wait_for("the change to settle", Duration::from_secs(60), async || {
            let (c, _) = s.catalog(vendor).await?;
            Ok(c.models.iter().any(|m| m.api_id == "tracked-new").then_some(c))
        })
        .await?;
    let index = catalog.index();
    let steady = index.find(vendor, "tracked-steady").one().context("tracked-steady is gone")?;
    let input = steady.price.as_ref().and_then(|p| p.rates.input);
    ensure!(input == Some(2.4), "the new price did not apply: {input:?}");
    let new = index.find(vendor, "tracked-new").one().context("tracked-new is missing")?;
    ensure!(new.api_id_confirmed, "the page states the new model's id");
    step("a changed page changes the catalogue", "a new price, a new model");

    let metrics = s.http.get(&s.o.metrics).send().await?.error_for_status()?.text().await?;
    for series in ["agg_jobs_enabled", "agg_scraper_seen_age_seconds", "agg_models"] {
        ensure!(metrics.lines().any(|l| l.starts_with(series)), "the metrics have no {series}");
    }
    step("the series the alerts read are there", "");
    Ok(())
}
