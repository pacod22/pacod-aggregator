//! What pages say, settled into the catalogue: the test plays the scraper on the internal API, and
//! a fake of the extraction model's endpoint answers. Each of the things a gateway relies on has a
//! test of its own here: `checked_at`, `complete`, `api_id_confirmed`, and that a failed page takes
//! nothing away.

mod common;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use common::{DEV, Server, serve};
use pacod_agg::config::{Config, LlmLink};
use pacod_agg_catalog::{Catalog, Format, parse};
use serde_json::{Value, json};

const KEY: &str = "a-key-of-the-gateway";
const PROVIDER_KEY: &str = "a-key-of-a-provider";

/// Headers by name and value, in order of name.
type Headers = Vec<(String, String)>;

/// The extraction model's endpoint: answers in the order given, and keeps what it was sent.
#[derive(Clone, Default)]
struct Llm {
    replies: Arc<Mutex<Vec<(u16, Value)>>>,
    calls: Arc<Mutex<Vec<(Value, String)>>>,
    /// The headers of each call that name something to a gateway (`x-gw-*`), by name.
    gateway_headers: Arc<Mutex<Vec<Headers>>>,
}

async fn completions(State(llm): State<Llm>, headers: HeaderMap, body: String) -> impl IntoResponse {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    llm.calls.lock().unwrap().push((serde_json::from_str(&body).unwrap(), auth));
    let mut named: Headers = headers
        .iter()
        .filter(|(name, _)| name.as_str().starts_with("x-gw-"))
        .map(|(name, value)| (name.as_str().to_owned(), value.to_str().unwrap_or_default().to_owned()))
        .collect();
    named.sort();
    llm.gateway_headers.lock().unwrap().push(named);
    let mut replies = llm.replies.lock().unwrap();
    let (status, answer) = if replies.is_empty() { (200, json!({ "models": [] })) } else { replies.remove(0) };
    let content = answer.to_string();
    let body = json!({ "choices": [{ "message": { "role": "assistant", "content": content } }],
                       "error": { "message": answer["error"] } });
    (StatusCode::from_u16(status).unwrap(), axum::Json(body))
}

impl Llm {
    fn answer(&self, models: Value) {
        self.replies.lock().unwrap().push((200, json!({ "models": models })));
    }

    fn fail(&self, status: u16) {
        self.replies.lock().unwrap().push((status, json!({ "error": "the model is away" })));
    }

    fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    async fn url(&self) -> String {
        let app = Router::new().route("/v1/chat/completions", post(completions)).with_state(self.clone());
        format!("{}/v1", serve(app).await)
    }
}

/// A server with one vendor, its model set, and the fake endpoint behind it.
async fn server(adjust: impl FnOnce(&mut Config)) -> (Server, Llm) {
    let llm = Llm::default();
    let url = llm.url().await;
    let s = Server::start(|c| {
        c.llm = Some(LlmLink::new(url, Some(KEY.into())));
        adjust(c);
    })
    .await;
    s.only_vendor("openai").await;
    set_model(&s).await;
    (s, llm)
}

async fn set_model(s: &Server) {
    let (_, mut settings) = s.api("GET", "/settings", None).await;
    settings["extraction_model"] = json!("reader");
    let (st, v) = s.api("PUT", "/settings", Some(settings)).await;
    assert_eq!(st, 200, "{v}");
}

/// Takes a check's lease and answers the run it starts.
async fn run(s: &Server) -> String {
    let (st, l) = s.internal("/lease", json!({ "job": "check", "holder": "scraper-1", "ttl_seconds": 120 })).await;
    assert_eq!((st, l["due"].as_bool()), (200, Some(true)), "{l}");
    l["run_id"].as_str().unwrap().to_owned()
}

async fn finish(s: &Server, run_id: &str) {
    let done = json!({ "run_id": run_id, "job": "check", "finished_at": "2026-11-05T12:00:00Z", "ok": true });
    assert_eq!(s.internal(&format!("/{run_id}/result"), done).await.0, 200);
}

/// Asks for a run over every page, as "check now" does.
async fn check_again(s: &Server) -> String {
    assert_eq!(s.api("POST", "/runs/check", None).await.0, 202);
    run(s).await
}

async fn read(s: &Server, run_id: &str, reading: Value) -> Value {
    let (st, v) = s.internal(&format!("/{run_id}/readings"), reading).await;
    assert_eq!(st, 200, "{v}");
    v
}

/// A page that changed, for the extraction model to read.
fn changed(page: &str, hash: &str) -> Value {
    json!({ "page_id": page, "outcome": "changed", "content_hash": hash, "text": "gpt-a | $2.00 | $10.00" })
}

fn unchanged(page: &str) -> Value {
    json!({ "page_id": page, "outcome": "unchanged" })
}

fn failed(page: &str) -> Value {
    json!({ "page_id": page, "outcome": "failed", "message": "the page answered 503" })
}

/// A list's reading, as the scraper reads a list itself.
fn listed(page: &str, hash: &str, models: Value) -> Value {
    json!({ "page_id": page, "outcome": "changed", "content_hash": hash, "extraction": { "models": models } })
}

fn gpt_a() -> Value {
    json!([{ "model": "gpt-a", "display_name": "GPT A", "api_id_stated": true, "currency": "usd", "input": 2.0,
             "output": 10.0, "cache_read": 0.2, "context_window": 400000, "max_output_tokens": 16000, "vision": true,
             "evidence": "gpt-a costs $2.00 per million input tokens" }])
}

async fn catalog(s: &Server) -> Catalog {
    let r = s.http.get(format!("{}/v1/catalog", s.public)).bearer_auth(DEV).send().await.unwrap();
    assert_eq!(r.status(), 200);
    // As a gateway reads it.
    parse(&r.bytes().await.unwrap(), Some(Format::Catalog)).unwrap()
}

fn model<'a>(c: &'a Catalog, api_id: &str) -> &'a pacod_agg_catalog::Model {
    c.models.iter().find(|m| m.api_id == api_id).unwrap_or_else(|| panic!("{api_id} is not in {:?}", c.models))
}

fn page_setup(s: &Server, vendor: &'static str, url: &'static str) -> impl Future<Output = String> {
    s.add_page(vendor, url)
}

#[tokio::test]
async fn a_page_is_read_by_the_model_and_its_models_enter_the_catalogue() {
    let (s, llm) = server(|_| {}).await;
    let page = page_setup(&s, "openai", "https://openai.example/pricing").await;
    llm.answer(json!([
        { "model": "gpt-a", "display_name": "GPT A", "api_id_stated": true, "currency": "usd", "input": 2.0,
          "output": 10.0, "cache_read": 0.2, "variants": { "batch": { "input": 1.0, "output": 5.0 } },
          "tiers": [{ "above_input_tokens": 200000, "input": 4.0, "output": 15.0 }],
          "context_window": 400000, "max_output_tokens": 16000, "input_modalities": ["text", "image"],
          "output_modalities": ["text"], "vision": true, "tools": true, "structured_output": true, "reasons": true,
          "evidence": "gpt-a costs $2.00" },
        { "model": "The Big Model", "api_id_stated": false, "input": 5.0, "output": 20.0 }
    ]));
    let run_id = run(&s).await;
    let r = read(&s, &run_id, changed(&page, "h1")).await;
    assert_eq!(r["outcome"], "read", "{r}");
    assert_eq!(r["settled"].as_array().unwrap().len(), 2, "the vendor settled: {r}");
    finish(&s, &run_id).await;

    // What the extraction model was sent, on the address the operator named, with its key.
    {
        let calls = llm.calls.lock().unwrap();
        let (body, auth) = &calls[0];
        assert_eq!(auth.as_str(), format!("Bearer {KEY}"));
        assert_eq!(body["model"], "reader");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(body["response_format"]["json_schema"]["name"], "price_page");
        assert_eq!(body["response_format"]["json_schema"]["strict"], true);
        let prompt = body["messages"][0]["content"].as_str().unwrap();
        assert!(prompt.contains("\"OpenAI\"") && prompt.contains("gpt-a | $2.00"), "{prompt}");
    }

    let c = catalog(&s).await;
    assert_eq!(c.schema, "pacod.catalog/v1");
    let a = model(&c, "gpt-a");
    assert!(a.id.as_deref().is_some_and(|id| id.starts_with("agm_")), "{a:?}");
    assert_eq!((a.vendor.as_deref(), a.display_name.as_deref()), (Some("openai"), Some("GPT A")));
    assert!(a.api_id_confirmed, "the vendor's page states the id");
    assert_eq!(a.kind, Some(pacod_agg_catalog::ModelKind::Chat));
    assert_eq!(a.status, Some(pacod_agg_catalog::ModelStatus::Active));
    assert_eq!((a.properties.context_window, a.properties.vision), (Some(400_000), Some(true)));
    assert_eq!(a.properties.reasoning, Some(true));
    let price = a.price.as_ref().unwrap();
    assert_eq!((price.currency.as_str(), price.rates.input, price.rates.cache_read), ("USD", Some(2.0), Some(0.2)));
    assert_eq!(price.variants["batch"].input, Some(1.0));
    assert_eq!(price.tiers[0].above_input_tokens, 200_000);
    assert!(price.checked_at.is_some() && price.effective_from.is_some(), "{price:?}");
    let evidence = price.evidence.as_ref().unwrap();
    assert_eq!(evidence.url.as_deref(), Some("https://openai.example/pricing"));
    assert_eq!(evidence.text.as_deref(), Some("gpt-a costs $2.00"));
    assert_eq!(a.provenance["price"].method.as_deref(), Some("rendered_page"));
    assert_eq!(a.provenance["context_window"].url.as_deref(), Some("https://openai.example/pricing"));

    let big = model(&c, "The Big Model");
    assert!(!big.api_id_confirmed, "a name is not an id to call: {big:?}");

    // A gateway finds it, the way it finds any model.
    let index = c.index();
    assert!(index.find("openai", "gpt-a").one().is_some());
    assert!(index.find("openai", "gpt-a-2026-08-07").one().is_some(), "a dated snapshot finds its model");
}

#[tokio::test]
async fn an_unchanged_page_moves_checked_at_on_and_writes_nothing_else() {
    let (s, llm) = server(|_| {}).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    llm.answer(gpt_a());
    let id = run(&s).await;
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;
    let before = catalog(&s).await;
    let first = model(&before, "gpt-a").price.clone().unwrap();

    let id = check_again(&s).await;
    let r = read(&s, &id, unchanged(&page)).await;
    assert_eq!(r["settled"][0]["outcome"], "unchanged", "{r}");
    finish(&s, &id).await;
    assert_eq!(llm.calls(), 1, "an unchanged page is not read again");

    // A gateway counts a price as stale from checked_at: an unchanged page confirms it, or every
    // model whose page stands still would go stale.
    let after = catalog(&s).await;
    let again = model(&after, "gpt-a").price.clone().unwrap();
    assert!(again.checked_at > first.checked_at, "{:?} is not after {:?}", again.checked_at, first.checked_at);
    assert_eq!(again.effective_from, first.effective_from, "the same price, not a new one");
    let (_, changes) =
        s.get(format!("{}/v1/models/{}/prices", s.public, model(&after, "gpt-a").id.clone().unwrap()), Some(DEV)).await;
    assert_eq!(changes.as_array().unwrap().len(), 1, "one version: {changes}");
}

#[tokio::test]
async fn a_failed_page_takes_nothing_away_and_confirms_nothing() {
    let (s, llm) = server(|_| {}).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    llm.answer(gpt_a());
    let id = run(&s).await;
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;
    let before = catalog(&s).await;
    assert!(before.vendor("openai").unwrap().complete, "every page was read");

    let id = check_again(&s).await;
    let r = read(&s, &id, failed(&page)).await;
    assert_eq!(r["outcome"], "failed", "{r}");
    finish(&s, &id).await;

    let after = catalog(&s).await;
    let (was, now) = (model(&before, "gpt-a"), model(&after, "gpt-a"));
    // The model stays: a page that could not be read says nothing about what it lists. A gateway
    // would otherwise take the vendor's list as shrunk.
    assert_eq!(now.id, was.id);
    // The price does not count as confirmed by a check that saw nothing.
    assert_eq!(now.price.as_ref().unwrap().checked_at, was.price.as_ref().unwrap().checked_at);
    // And the vendor is not complete: a gateway would resolve its proposals by it.
    assert!(!after.vendor("openai").unwrap().complete, "a page failed: {:?}", after.vendors);
    let (_, p) = s.api("GET", &format!("/pages/{page}"), None).await;
    assert_eq!(p["state"]["consecutive_failures"], 1, "{p}");

    // Read again, all is well.
    let id = check_again(&s).await;
    read(&s, &id, unchanged(&page)).await;
    finish(&s, &id).await;
    assert!(catalog(&s).await.vendor("openai").unwrap().complete);
}

#[tokio::test]
async fn a_vendor_is_complete_only_while_every_page_is_read_and_an_administrator_says_so() {
    let (s, llm) = server(|_| {}).await;
    let (first, second) = (
        s.add_page("openai", "https://openai.example/pricing").await,
        s.add_page("openai", "https://openai.example/models").await,
    );
    // Before anything was read, it is not.
    assert!(!catalog(&s).await.vendor("openai").unwrap().complete);
    llm.answer(gpt_a());
    llm.answer(json!([]));
    let id = run(&s).await;
    let r = read(&s, &id, changed(&first, "h1")).await;
    assert!(r["settled"].is_null(), "one page is still to come: {r}");
    assert!(!catalog(&s).await.vendor("openai").unwrap().complete, "not settled yet");
    let r = read(&s, &id, changed(&second, "h2")).await;
    assert!(r["settled"].is_array(), "{r}");
    finish(&s, &id).await;
    assert!(catalog(&s).await.vendor("openai").unwrap().complete);

    // An administrator withdraws the claim.
    let (_, mut v) = s.api("GET", "/vendors/openai", None).await;
    v["complete"] = json!(false);
    assert_eq!(s.api("PUT", "/vendors/openai", Some(v)).await.0, 200);
    assert!(!catalog(&s).await.vendor("openai").unwrap().complete);
}

#[tokio::test]
async fn a_vendor_known_from_a_list_alone_is_never_complete() {
    let (s, _llm) = server(|_| {}).await;
    let (st, page) = s
        .api(
            "POST",
            "/pages",
            Some(json!({ "vendor_id": "openai", "url": "https://lists.example/models.json", "kind": "feed",
                         "format": "litellm" })),
        )
        .await;
    assert_eq!(st, 201, "{page}");
    let id = run(&s).await;
    read(
        &s,
        &id,
        listed(
            page["id"].as_str().unwrap(),
            "h1",
            json!([{ "model": "gpt-a", "price": { "rates": { "input": 2.0 } } }]),
        ),
    )
    .await;
    finish(&s, &id).await;
    let c = catalog(&s).await;
    assert!(!c.vendor("openai").unwrap().complete, "{:?}", c.vendors);
    assert!(!model(&c, "gpt-a").api_id_confirmed);
}

#[tokio::test]
async fn only_a_vendors_own_page_confirms_an_id() {
    let (s, llm) = server(|_| {}).await;
    let feed = json!({ "vendor_id": "openai", "url": "https://lists.example/models.json", "kind": "feed",
                       "format": "litellm", "priority": 100 });
    let (_, feed) = s.api("POST", "/pages", Some(feed)).await;
    let feed = feed["id"].as_str().unwrap().to_owned();
    let page = s.add_page("openai", "https://openai.example/pricing").await;

    // A list says nothing of the kind, whatever its reading claims.
    let from_list = json!([{ "model": "gpt-a", "api_id_stated": true, "price": { "rates": { "input": 2.0 } } }]);
    llm.answer(json!([]));
    let id = run(&s).await;
    read(&s, &id, listed(&feed, "f1", from_list.clone())).await;
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;
    assert!(!model(&catalog(&s).await, "gpt-a").api_id_confirmed, "known from a list alone");

    // The vendor's page states it.
    llm.answer(gpt_a());
    let id = check_again(&s).await;
    read(&s, &id, listed(&feed, "f1", from_list)).await;
    read(&s, &id, changed(&page, "h2")).await;
    finish(&s, &id).await;
    assert!(model(&catalog(&s).await, "gpt-a").api_id_confirmed);
}

#[tokio::test]
async fn pages_go_by_priority_and_a_list_fills_the_gaps() {
    let (s, llm) = server(|_| {}).await;
    let feed = json!({ "vendor_id": "openai", "url": "https://lists.example/models.json", "kind": "feed",
                       "format": "litellm", "priority": 100 });
    let (_, feed) = s.api("POST", "/pages", Some(feed)).await;
    let feed = feed["id"].as_str().unwrap().to_owned();
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    llm.answer(json!([{ "model": "gpt-a", "api_id_stated": true, "input": 2.0, "output": 10.0 }]));
    let id = run(&s).await;
    read(
        &s,
        &id,
        listed(
            &feed,
            "f1",
            json!([{ "model": "GPT-A", "price": { "rates": { "input": 9.0, "output": 9.0 } },
                     "properties": { "context_window": 400000 } },
                   { "model": "gpt-list-only", "price": { "rates": { "input": 1.0, "output": 2.0 } } }]),
        ),
    )
    .await;
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;
    let c = catalog(&s).await;
    let a = model(&c, "gpt-a");
    assert_eq!(a.price.as_ref().unwrap().rates.input, Some(2.0), "the vendor's page goes first");
    assert_eq!(a.provenance["price"].method.as_deref(), Some("rendered_page"));
    assert_eq!(a.properties.context_window, Some(400_000), "what the page does not state, the list may");
    assert_eq!(a.provenance["context_window"].method.as_deref(), Some("feed"));
    assert!(c.models.iter().any(|m| m.api_id == "gpt-list-only"), "a list fills gaps on the first day");
}

#[tokio::test]
async fn openrouters_prices_apply_only_for_the_vendors_allowed() {
    let (s, _llm) = server(|_| {}).await;
    let feed = json!({ "vendor_id": "openai", "url": "https://openrouter.example/models", "kind": "feed",
                       "format": "openrouter" });
    let (_, feed) = s.api("POST", "/pages", Some(feed)).await;
    let feed = feed["id"].as_str().unwrap().to_owned();
    let (_, mut settings) = s.api("GET", "/settings", None).await;
    settings["openrouter_price_vendors"] = json!(["anthropic"]);
    assert_eq!(s.api("PUT", "/settings", Some(settings.clone())).await.0, 200);
    let list = json!([{ "model": "gpt-a", "price": { "rates": { "input": 2.0 } },
                        "properties": { "context_window": 400000 } }]);
    let id = run(&s).await;
    read(&s, &id, listed(&feed, "f1", list.clone())).await;
    finish(&s, &id).await;
    let c = catalog(&s).await;
    assert!(model(&c, "gpt-a").price.is_none(), "for this vendor its price is a third party's");
    assert_eq!(model(&c, "gpt-a").properties.context_window, Some(400_000), "its properties apply for every vendor");

    let (_, mut settings) = s.api("GET", "/settings", None).await;
    settings["openrouter_price_vendors"] = json!(["openai"]);
    assert_eq!(s.api("PUT", "/settings", Some(settings)).await.0, 200);
    let id = check_again(&s).await;
    read(
        &s,
        &id,
        json!({ "page_id": feed, "outcome": "changed", "content_hash": "f1", "extraction": { "models": list } }),
    )
    .await;
    finish(&s, &id).await;
    assert_eq!(model(&catalog(&s).await, "gpt-a").price.as_ref().unwrap().rates.input, Some(2.0));
}

#[tokio::test]
async fn a_price_that_changes_is_a_new_version_and_the_change_is_on_record() {
    let (s, llm) = server(|_| {}).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    llm.answer(gpt_a());
    let id = run(&s).await;
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;

    let mut new = gpt_a();
    new[0]["input"] = json!(3.0);
    new[0]["context_window"] = json!(500000);
    llm.answer(new);
    let id = check_again(&s).await;
    let r = read(&s, &id, changed(&page, "h2")).await;
    assert_eq!(r["settled"][0]["outcome"], "changed", "{r}");
    finish(&s, &id).await;

    let c = catalog(&s).await;
    let a = model(&c, "gpt-a");
    assert_eq!((a.price.as_ref().unwrap().rates.input, a.properties.context_window), (Some(3.0), Some(500_000)));
    let model_id = a.id.clone().unwrap();
    let (_, versions) = s.get(format!("{}/v1/models/{model_id}/prices", s.public), Some(DEV)).await;
    let list = versions.as_array().unwrap();
    assert_eq!(list.len(), 2, "{versions}");
    assert!(list[0]["superseded_at"].is_null() && list[1]["superseded_at"].is_string(), "{versions}");
    let (_, changes) = s.api("GET", &format!("/models/{model_id}"), None).await;
    let fields: Vec<&str> =
        changes["changes"].as_array().unwrap().iter().map(|c| c["field"].as_str().unwrap()).collect();
    assert!(
        fields.contains(&"price") && fields.contains(&"context_window") && fields.contains(&"created"),
        "{fields:?}"
    );
}

#[tokio::test]
async fn a_misread_price_is_not_published_and_the_rest_of_the_model_is() {
    let (s, llm) = server(|_| {}).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    llm.answer(json!([{ "model": "gpt-a", "api_id_stated": true, "input": 50000.0, "output": 2.0,
                        "context_window": 15, "max_output_tokens": 16000, "vision": true }]));
    let id = run(&s).await;
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;
    let c = catalog(&s).await;
    let a = model(&c, "gpt-a");
    assert!(a.price.is_none(), "a price per thousand read as one per million: {a:?}");
    assert_eq!(a.properties.context_window, None, "a price read as a context window");
    assert_eq!(a.properties.vision, Some(true));
}

#[tokio::test]
async fn a_model_no_page_names_goes_only_when_every_page_was_read_and_may_come_back() {
    let (s, llm) = server(|_| {}).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    let two = json!([{ "model": "gpt-a", "api_id_stated": true, "input": 2.0, "output": 4.0 },
                     { "model": "gpt-b", "api_id_stated": true, "input": 1.0, "output": 2.0 }]);
    llm.answer(two.clone());
    let id = run(&s).await;
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;
    let id_of_b = model(&catalog(&s).await, "gpt-b").id.clone().unwrap();

    // The page no longer lists it.
    llm.answer(json!([{ "model": "gpt-a", "api_id_stated": true, "input": 2.0, "output": 4.0 }]));
    let id = check_again(&s).await;
    let r = read(&s, &id, changed(&page, "h2")).await;
    assert!(r["settled"].as_array().unwrap().iter().any(|m| m["model"] == "gpt-b" && m["outcome"] == "gone"), "{r}");
    finish(&s, &id).await;
    let c = catalog(&s).await;
    assert!(c.models.iter().all(|m| m.api_id != "gpt-b"), "{:?}", c.models);
    assert_eq!(s.get(format!("{}/v1/models/{id_of_b}", s.public), Some(DEV)).await.0, 404);

    // It comes back, as the same model.
    llm.answer(two);
    let id = check_again(&s).await;
    read(&s, &id, changed(&page, "h3")).await;
    finish(&s, &id).await;
    assert_eq!(model(&catalog(&s).await, "gpt-b").id.as_deref(), Some(id_of_b.as_str()));
}

#[tokio::test]
async fn an_extraction_that_fails_is_read_again() {
    let (s, llm) = server(|_| {}).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    llm.fail(503);
    let id = run(&s).await;
    let r = read(&s, &id, changed(&page, "h1")).await;
    assert_eq!(r["outcome"], "failed", "{r}");
    assert!(r["message"].as_str().unwrap().contains("503"), "{r}");
    finish(&s, &id).await;

    // Its fingerprint was not kept: the next check reads it, though it did not change.
    assert_eq!(s.api("POST", "/runs/check", None).await.0, 202);
    let (_, l) = s.internal("/lease", json!({ "job": "check", "holder": "scraper-1", "ttl_seconds": 120 })).await;
    assert_eq!(l["work"]["pages"][0]["previous_hash"], "", "{l}");
    assert_eq!(l["work"]["pages"][0]["force"], true, "{l}");
    let id = l["run_id"].as_str().unwrap().to_owned();
    llm.answer(gpt_a());
    assert_eq!(read(&s, &id, changed(&page, "h1")).await["outcome"], "read");
    finish(&s, &id).await;
    assert!(catalog(&s).await.models.iter().any(|m| m.api_id == "gpt-a"));
}

#[tokio::test]
async fn a_long_page_is_read_in_parts_within_the_runs_budget() {
    let (s, llm) = server(|_| {}).await;
    let (_, mut settings) = s.api("GET", "/settings", None).await;
    // A small window: the page is read in parts of 24,000 characters.
    settings["extraction_context_window"] = json!(32000);
    assert_eq!(s.api("PUT", "/settings", Some(settings)).await.0, 200);
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    let mut text = String::from("Model | Input | Output\n");
    for i in 0..1800 {
        text.push_str(&format!("other-model-{i:04} | $1.00 / 1M | $2.00 / 1M\n"));
    }
    text.push_str("far | $3.00 / 1M | $6.00 / 1M\n");
    let parts = text.len().div_ceil(24_000);
    assert!(parts >= 3, "{parts}");
    for _ in 1..parts {
        llm.answer(json!([]));
    }
    llm.answer(json!([{ "model": "far", "api_id_stated": true, "input": 3.0, "output": 6.0 }]));
    let id = run(&s).await;
    let reading = json!({ "page_id": page, "outcome": "changed", "content_hash": "h", "text": text });
    assert_eq!(read(&s, &id, reading.clone()).await["outcome"], "read");
    finish(&s, &id).await;
    assert_eq!(llm.calls(), parts, "one call per part");
    assert!(catalog(&s).await.models.iter().any(|m| m.api_id == "far"), "the model in the last part");
    {
        let calls = llm.calls.lock().unwrap();
        let last = calls.last().unwrap().0["messages"][0]["content"].as_str().unwrap();
        assert!(last.contains("Model | Input | Output") && last.contains(&format!("part {parts} of {parts}")));
    }

    // A run may call the extraction model only so often: a page beyond its budget waits.
    let (_, mut settings) = s.api("GET", "/settings", None).await;
    settings["max_extractions_per_run"] = json!(2);
    assert_eq!(s.api("PUT", "/settings", Some(settings)).await.0, 200);
    assert_eq!(s.api("POST", &format!("/pages/{page}/check"), None).await.0, 202);
    let id = run(&s).await;
    let r = read(&s, &id, reading).await;
    assert_eq!(r["outcome"], "not_read", "{r}");
    assert!(r["message"].as_str().unwrap().contains("budget"), "{r}");
    assert_eq!(llm.calls(), parts, "no call was made");
}

#[tokio::test]
async fn without_a_model_or_an_address_pages_wait() {
    let llm = Llm::default();
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    let id = run(&s).await;
    let r = read(&s, &id, changed(&page, "h1")).await;
    assert_eq!((r["outcome"].as_str(), r["message"].as_str()), (Some("not_read"), Some("no extraction model is set")));
    finish(&s, &id).await;
    set_model(&s).await;
    let id = check_again(&s).await;
    let r = read(&s, &id, changed(&page, "h1")).await;
    assert_eq!((r["outcome"].as_str(), r["message"].as_str()), (Some("not_read"), Some("AGG_LLM_URL is not set")));
    assert_eq!(llm.calls(), 0);
    assert!(s.api("GET", "/status", None).await.1["problems"].to_string().contains("AGG_LLM_URL"));
}

#[tokio::test]
async fn the_environment_names_the_model_until_the_console_does() {
    let llm = Llm::default();
    let url = llm.url().await;
    let s = Server::start(|c| {
        c.llm = Some(LlmLink::new(url, Some(KEY.into())));
        c.llm_model = Some("from-env".into());
    })
    .await;
    s.only_vendor("openai").await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    assert_eq!(s.api("GET", "/status", None).await.1["extraction"], true, "no console setting is needed");

    let id = run(&s).await;
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;
    set_model(&s).await;
    let id = check_again(&s).await;
    read(&s, &id, changed(&page, "h2")).await;
    finish(&s, &id).await;
    let (_, mut settings) = s.api("GET", "/settings", None).await;
    settings["extraction_model"] = Value::Null;
    assert_eq!(s.api("PUT", "/settings", Some(settings)).await.0, 200);
    let id = check_again(&s).await;
    read(&s, &id, changed(&page, "h3")).await;

    let calls = llm.calls.lock().unwrap();
    let models: Vec<&str> = calls.iter().map(|(body, _)| body["model"].as_str().unwrap()).collect();
    assert_eq!(models, ["from-env", "reader", "from-env"]);
}

/// Changes one setting, as saving the settings page does.
async fn save_setting(s: &Server, name: &str, value: Value) {
    let (_, mut settings) = s.api("GET", "/settings", None).await;
    settings[name] = value;
    let (st, v) = s.api("PUT", "/settings", Some(settings)).await;
    assert_eq!(st, 200, "{v}");
}

/// What an endpoint was sent: the model and the credential of each call.
fn seen(llm: &Llm) -> Vec<(String, String)> {
    let calls = llm.calls.lock().unwrap();
    calls.iter().map(|(body, auth)| (body["model"].as_str().unwrap().to_owned(), auth.clone())).collect()
}

#[tokio::test]
async fn a_switch_applies_at_the_next_reading_and_each_key_stays_with_its_address() {
    let (direct, gateway) = (Llm::default(), Llm::default());
    let (direct_url, gateway_url) = (direct.url().await, gateway.url().await);
    let s = Server::start(|c| {
        c.llm = Some(LlmLink::new(direct_url, Some(PROVIDER_KEY.into())));
        c.gateway = Some(LlmLink::new(gateway_url, Some(KEY.into())));
    })
    .await;
    s.only_vendor("openai").await;
    save_setting(&s, "extraction_model", json!("reader")).await;
    save_setting(&s, "gateway_model", json!("vendor/reader")).await;
    let first = s.add_page("openai", "https://openai.example/pricing").await;
    let second = s.add_page("openai", "https://openai.example/models").await;

    // Both are set and none is chosen: an endpoint of its own wins, as the full stack had it.
    let id = run(&s).await;
    read(&s, &id, changed(&first, "h1")).await;
    // Within the run, without a restart, and with no change of the model: that is the switch.
    save_setting(&s, "mode", json!("gateway")).await;
    read(&s, &id, changed(&second, "h2")).await;
    finish(&s, &id).await;

    save_setting(&s, "mode", json!("standalone")).await;
    let id = check_again(&s).await;
    read(&s, &id, changed(&first, "h3")).await;

    let (reader, routed) = (String::from("reader"), String::from("vendor/reader"));
    let (provider, its_gateway) = (format!("Bearer {PROVIDER_KEY}"), format!("Bearer {KEY}"));
    assert_eq!(seen(&direct), [(reader.clone(), provider.clone()), (reader, provider)]);
    assert_eq!(seen(&gateway), [(routed, its_gateway)]);
}

/// The `x-gw-*` headers of each call an endpoint was sent.
fn gateway_headers(llm: &Llm) -> Vec<Headers> {
    llm.gateway_headers.lock().unwrap().clone()
}

fn header(name: &str, value: &str) -> (String, String) {
    (name.to_owned(), value.to_owned())
}

#[tokio::test]
async fn a_gateway_is_told_the_task_the_run_and_the_agent_and_a_provider_none_of_them() {
    let (direct, gateway) = (Llm::default(), Llm::default());
    let (direct_url, gateway_url) = (direct.url().await, gateway.url().await);
    let s = Server::start(|c| {
        c.llm = Some(LlmLink::new(direct_url, Some(PROVIDER_KEY.into())));
        c.gateway =
            Some(LlmLink { agent: Some("pacod-aggregator".into()), ..LlmLink::new(gateway_url, Some(KEY.into())) });
    })
    .await;
    s.only_vendor("openai").await;
    save_setting(&s, "extraction_model", json!("reader")).await;
    save_setting(&s, "gateway_model", json!("vendor/reader")).await;
    let first = s.add_page("openai", "https://openai.example/pricing").await;
    let second = s.add_page("openai", "https://openai.example/models").await;

    let id = run(&s).await;
    read(&s, &id, changed(&first, "h1")).await;
    save_setting(&s, "mode", json!("gateway")).await;
    read(&s, &id, changed(&second, "h2")).await;

    assert_eq!(gateway_headers(&direct), [vec![]], "a provider's endpoint is told nothing of the gateway's");
    assert_eq!(
        gateway_headers(&gateway),
        [vec![
            header("x-gw-agent-id", "pacod-aggregator"),
            header("x-gw-task", "price-extraction"),
            header("x-gw-trace", &id),
        ]]
    );
}

#[tokio::test]
async fn a_gateway_without_an_agent_is_named_none() {
    let llm = Llm::default();
    let url = llm.url().await;
    let s = Server::start(|c| {
        c.gateway = Some(LlmLink::new(url, Some(KEY.into())));
        c.gateway_model = Some("vendor/from-env".into());
    })
    .await;
    s.only_vendor("openai").await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    let id = run(&s).await;
    read(&s, &id, changed(&page, "h1")).await;
    // A key bound to one agent speaks for it unnamed; an unbound key names none.
    assert_eq!(gateway_headers(&llm), [vec![header("x-gw-task", "price-extraction"), header("x-gw-trace", &id)]]);
}

#[tokio::test]
async fn a_mode_without_its_endpoint_waits_and_the_other_is_not_called() {
    let (s, llm) = server(|_| {}).await;
    save_setting(&s, "gateway_model", json!("vendor/reader")).await;
    save_setting(&s, "mode", json!("gateway")).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;

    let id = run(&s).await;
    let r = read(&s, &id, changed(&page, "h1")).await;
    assert_eq!((r["outcome"].as_str(), r["message"].as_str()), (Some("not_read"), Some("AGG_GW_URL is not set")));
    assert_eq!(llm.calls(), 0, "standalone's endpoint is set, and it is not the one chosen");

    let status = s.api("GET", "/status", None).await.1;
    assert_eq!(status["extraction"], false, "{status}");
    let problems = status["problems"].to_string();
    assert!(problems.contains("AGG_GW_URL") && problems.contains("gateway mode"), "{problems}");
}

#[tokio::test]
async fn an_installation_with_only_a_gateway_reads_without_anyone_choosing() {
    let llm = Llm::default();
    let url = llm.url().await;
    let s = Server::start(|c| {
        c.gateway = Some(LlmLink::new(url, Some(KEY.into())));
        c.gateway_model = Some("vendor/from-env".into());
    })
    .await;
    s.only_vendor("openai").await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    assert_eq!(s.api("GET", "/status", None).await.1["extraction"], true);

    let id = run(&s).await;
    read(&s, &id, changed(&page, "h1")).await;
    assert_eq!(seen(&llm), [(String::from("vendor/from-env"), format!("Bearer {KEY}"))]);
    assert_eq!(llm.calls.lock().unwrap()[0].0["max_tokens"], 16_000, "the gateway translates the limit");
}

#[tokio::test]
async fn a_mode_without_a_model_says_which_variable_would_name_one() {
    let (s, llm) = server(|c| c.gateway = c.llm.clone()).await;
    save_setting(&s, "mode", json!("gateway")).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;

    let id = run(&s).await;
    let r = read(&s, &id, changed(&page, "h1")).await;
    // The standalone model is named, and it is not the gateway's to use.
    assert_eq!((r["outcome"].as_str(), r["message"].as_str()), (Some("not_read"), Some("no extraction model is set")));
    assert_eq!(llm.calls(), 0);
    let problems = s.api("GET", "/status", None).await.1["problems"].to_string();
    assert!(problems.contains("AGG_GW_MODEL"), "{problems}");
}

#[tokio::test]
async fn the_limit_on_an_answer_goes_under_the_name_the_endpoint_takes() {
    for (field, other) in [("max_tokens", "max_completion_tokens"), ("max_completion_tokens", "max_tokens")] {
        let (s, llm) = server(|c| c.llm.as_mut().unwrap().limit_field = field).await;
        let page = s.add_page("openai", "https://openai.example/pricing").await;
        let id = run(&s).await;
        read(&s, &id, changed(&page, "h1")).await;
        let calls = llm.calls.lock().unwrap();
        let body = &calls[0].0;
        assert_eq!(body[field], 16_000, "{body}");
        assert!(body.get(other).is_none(), "{body}");
    }
}

#[tokio::test]
async fn a_reading_outside_its_run_is_refused() {
    let (s, llm) = server(|_| {}).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    llm.answer(gpt_a());
    let id = run(&s).await;
    let (st, v) = s.internal(&format!("/{id}/readings"), changed("pag_other", "h")).await;
    assert_eq!(st, 409, "a page the run does not check: {v}");
    read(&s, &id, changed(&page, "h1")).await;
    finish(&s, &id).await;
    let (st, v) = s.internal(&format!("/{id}/readings"), changed(&page, "h1")).await;
    assert_eq!(st, 409, "the run is over: {v}");
}

#[tokio::test]
async fn a_page_dismissed_during_a_run_is_not_read() {
    let (s, llm) = server(|_| {}).await;
    let page = s.add_page("openai", "https://openai.example/pricing").await;
    let id = run(&s).await;
    assert_eq!(s.api("POST", &format!("/pages/{page}/dismiss"), None).await.0, 200);
    let r = read(&s, &id, changed(&page, "h1")).await;
    assert_eq!(r["outcome"], "not_read", "{r}");
    assert_eq!(llm.calls(), 0);
}
