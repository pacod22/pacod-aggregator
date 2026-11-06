//! The public catalogue, as a gateway reads it: who may, what a tag saves, and what is left out.

mod common;

use common::{DEV, Server};
use pacod_agg_catalog::{Format, parse};
use serde_json::{Value, json};

/// A vendor with `models` settled from a list, straight through the internal API.
async fn settled(s: &Server, vendor: &str, models: Value) {
    let (st, page) = s
        .api(
            "POST",
            "/pages",
            Some(json!({ "vendor_id": vendor, "url": format!("https://lists.example/{vendor}.json"), "kind": "feed",
                         "format": "litellm" })),
        )
        .await;
    assert_eq!(st, 201, "{page}");
    let (st, l) = s.internal("/lease", json!({ "job": "check", "holder": "s", "ttl_seconds": 60 })).await;
    assert_eq!((st, l["due"].as_bool()), (200, Some(true)), "{l}");
    let run = l["run_id"].as_str().unwrap();
    let reading = json!({ "page_id": page["id"], "outcome": "changed", "content_hash": "h",
                          "extraction": { "models": models } });
    assert_eq!(s.internal(&format!("/{run}/readings"), reading).await.0, 200);
    let end = json!({ "run_id": run, "job": "check", "finished_at": "2026-11-05T12:00:00Z", "ok": true });
    assert_eq!(s.internal(&format!("/{run}/result"), end).await.0, 200);
}

fn listed(id: &str) -> Value {
    json!({ "model": id, "price": { "rates": { "input": 2.0, "output": 10.0 } },
            "properties": { "context_window": 128000 } })
}

async fn fresh(s: &Server) {
    sqlx::query("DELETE FROM pages").execute(s.store.pool()).await.unwrap();
}

#[tokio::test]
async fn the_catalogue_wants_a_token_unless_it_is_open() {
    let s = Server::start(|_| {}).await;
    let url = format!("{}/v1/catalog", s.public);
    assert_eq!(s.get(url.clone(), None).await.0, 401);
    assert_eq!(s.get(url.clone(), Some("wrong")).await.0, 401);
    assert_eq!(s.get(url.clone(), Some(DEV)).await.0, 200);

    let open = Server::start(|c| c.catalog_public = true).await;
    for path in ["/v1/catalog", "/v1/vendors", "/v1/models"] {
        assert_eq!(open.get(format!("{}{path}", open.public), None).await.0, 200, "{path}");
    }
    // Only the catalogue: the console's API still asks who calls.
    assert_eq!(open.get(format!("{}/api/v1/vendors", open.admin), None).await.0, 401);
}

#[tokio::test]
async fn a_catalogue_that_did_not_change_is_not_sent_again() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    settled(&s, "openai", json!([listed("gpt-a")])).await;
    let url = format!("{}/v1/catalog", s.public);
    let get = |tag: Option<String>| {
        let mut r = s.http.get(&url).bearer_auth(DEV);
        if let Some(t) = tag {
            r = r.header("if-none-match", t);
        }
        r.send()
    };
    let first = get(None).await.unwrap();
    assert_eq!(first.status(), 200);
    let tag = first.headers()["etag"].to_str().unwrap().to_owned();
    assert!(tag.starts_with('"'), "{tag}");
    assert_eq!(first.headers()["content-type"], "application/json");
    let body = first.bytes().await.unwrap();
    let catalog = parse(&body, None).unwrap();
    assert_eq!(catalog.models.len(), 1);

    // The same tag, though generated_at moved on: nothing to fetch.
    let again = get(Some(tag.clone())).await.unwrap();
    assert_eq!(again.status(), 304);
    assert_eq!(again.headers()["etag"], tag.as_str());
    assert!(again.bytes().await.unwrap().is_empty());
    assert_eq!(get(Some(format!("W/{tag}"))).await.unwrap().status(), 304, "a proxy may weaken it");
    assert_eq!(get(Some("\"other\"".into())).await.unwrap().status(), 200);

    // A change gives another tag.
    sqlx::query("DELETE FROM pages").execute(s.store.pool()).await.unwrap();
    settled(&s, "openai", json!([listed("gpt-a"), listed("gpt-b")])).await;
    assert_eq!(get(Some(tag)).await.unwrap().status(), 200);
}

#[tokio::test]
async fn the_catalogue_can_be_asked_for_some_vendors() {
    let s = Server::start(|_| {}).await;
    fresh(&s).await;
    settled(&s, "openai", json!([listed("gpt-a")])).await;
    let (_, v) = s.get(format!("{}/v1/catalog?vendor=openai,xai", s.public), Some(DEV)).await;
    let vendors: Vec<&str> = v["vendors"].as_array().unwrap().iter().map(|x| x["id"].as_str().unwrap()).collect();
    assert_eq!(vendors, ["openai", "xai"]);
    assert_eq!(v["models"].as_array().unwrap().len(), 1);
    let (_, v) = s.get(format!("{}/v1/catalog?vendor=xai", s.public), Some(DEV)).await;
    assert!(v["models"].as_array().unwrap().is_empty(), "{v}");
}

#[tokio::test]
async fn a_vendor_switched_off_is_not_published() {
    let s = Server::start(|_| {}).await;
    fresh(&s).await;
    settled(&s, "openai", json!([listed("gpt-a")])).await;
    let (_, mut v) = s.api("GET", "/vendors/openai", None).await;
    v["enabled"] = json!(false);
    assert_eq!(s.api("PUT", "/vendors/openai", Some(v)).await.0, 200);
    let (_, c) = s.get(format!("{}/v1/catalog", s.public), Some(DEV)).await;
    assert!(c["models"].as_array().unwrap().is_empty(), "{c}");
    assert!(c["vendors"].as_array().unwrap().iter().all(|v| v["id"] != "openai"), "{c}");
    let (_, vendors) = s.get(format!("{}/v1/vendors", s.public), Some(DEV)).await;
    assert!(vendors.as_array().unwrap().iter().all(|v| v["id"] != "openai"), "{vendors}");
}

#[tokio::test]
async fn models_come_in_pages_with_opaque_ids() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    settled(&s, "openai", json!([listed("gpt-a"), listed("gpt-b"), listed("gpt-c")])).await;
    let mut seen = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let after = cursor.as_deref().map_or(String::new(), |c| format!("&cursor={c}"));
        let (st, page) = s.get(format!("{}/v1/models?vendor=openai&limit=2{after}", s.public), Some(DEV)).await;
        assert_eq!(st, 200, "{page}");
        for m in page["items"].as_array().unwrap() {
            seen.push(m["api_id"].as_str().unwrap().to_owned());
            assert!(m["id"].as_str().unwrap().starts_with("agm_"), "{m}");
        }
        match page["next_cursor"].as_str() {
            Some(c) => cursor = Some(c.to_owned()),
            None => break,
        }
    }
    seen.sort();
    assert_eq!(seen, ["gpt-a", "gpt-b", "gpt-c"]);

    let (_, list) = s.get(format!("{}/v1/models?limit=1", s.public), Some(DEV)).await;
    let id = list["items"][0]["id"].as_str().unwrap();
    let (st, one) = s.get(format!("{}/v1/models/{id}", s.public), Some(DEV)).await;
    assert_eq!((st, one["vendor"].as_str()), (200, Some("openai")), "{one}");
    let (st, prices) = s.get(format!("{}/v1/models/{id}/prices", s.public), Some(DEV)).await;
    assert_eq!((st, prices.as_array().map(Vec::len)), (200, Some(1)), "{prices}");
    assert_eq!(s.get(format!("{}/v1/models/agm_none", s.public), Some(DEV)).await.0, 404);
    assert_eq!(s.get(format!("{}/v1/models/agm_none/prices", s.public), Some(DEV)).await.0, 404);
}

#[tokio::test]
async fn the_console_sees_what_the_catalogue_says() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    settled(&s, "openai", json!([listed("gpt-a")])).await;
    let (st, list) = s.api("GET", "/models?vendor=openai", None).await;
    assert_eq!(st, 200, "{list}");
    let m = &list["items"][0];
    assert_eq!(
        (m["api_id"].as_str(), m["input"].as_f64(), m["context_window"].as_u64(), m["currency"].as_str()),
        (Some("gpt-a"), Some(2.0), Some(128_000), Some("USD")),
        "{m}"
    );
    let (st, detail) = s.api("GET", &format!("/models/{}", m["id"].as_str().unwrap()), None).await;
    assert_eq!(st, 200, "{detail}");
    assert_eq!(detail["entry"]["api_id"], "gpt-a");
    assert_eq!(detail["prices"].as_array().unwrap().len(), 1);
    assert_eq!(detail["changes"][0]["field"], "created");
    let (_, changes) = s.api("GET", "/changes", None).await;
    assert_eq!(changes[0]["field"], "created");
}

#[tokio::test]
async fn what_the_catalogue_writes_is_what_a_reader_reads() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    settled(&s, "openai", json!([listed("gpt-a")])).await;
    let r = s.http.get(format!("{}/v1/catalog", s.public)).bearer_auth(DEV).send().await.unwrap();
    let body = r.bytes().await.unwrap();
    let auto = parse(&body, None).unwrap();
    let named = parse(&body, Some(Format::Catalog)).unwrap();
    assert_eq!(auto, named, "a reader tells the format by itself");
    assert_eq!(auto.source.kind, pacod_agg_catalog::SourceKind::PacodAggregator);
    assert_eq!(auto.source.version.as_deref(), Some(env!("CARGO_PKG_VERSION")));
    let again = serde_json::to_vec(&auto).unwrap();
    assert_eq!(parse(&again, None).unwrap(), auto, "it survives being written again");
}

/// What varies from run to run, written the same every time.
fn normalised(v: &mut Value) {
    match v {
        Value::Object(map) => {
            for (key, value) in map.iter_mut() {
                let varies = key == "id" && value.as_str().is_some_and(|s| s.starts_with("agm_"));
                if varies || key.ends_with("_at") || key == "effective_from" {
                    if !value.is_null() {
                        *value = json!(format!("<{key}>"));
                    }
                } else if key == "version" {
                    *value = json!("<version>");
                } else {
                    normalised(value);
                }
            }
        }
        Value::Array(list) => list.iter_mut().for_each(normalised),
        _ => {}
    }
}

/// The catalogue is a contract: what it writes for a model is pinned in a file, so that a change of
/// its shape shows in a review. `UPDATE_GOLDEN=1` writes the file anew.
#[tokio::test]
async fn the_catalogue_writes_what_the_golden_file_says() {
    let s = Server::start(|_| {}).await;
    s.only_vendor("openai").await;
    settled(
        &s,
        "openai",
        json!([{ "model": "gpt-a", "display_name": "GPT A", "aliases": ["gpt-a-2026-08-07"], "kind": "chat",
                 "price": { "currency": "USD", "rates": { "input": 2.0, "output": 10.0, "cache_read": 0.2 },
                            "variants": { "batch": { "input": 1.0, "output": 5.0 } },
                            "tiers": [{ "above_input_tokens": 200000, "input": 4.0, "output": 15.0 }],
                            "evidence": { "url": "https://lists.example/openai.json", "method": "feed" } },
                 "properties": { "context_window": 400000, "max_output_tokens": 16000, "vision": true,
                                 "modalities": { "input": ["text", "image"], "output": ["text"] } },
                 "parameters": { "temperature": { "support": "accepted", "min": 0.0, "max": 2.0 } } }]),
    )
    .await;
    let (_, mut catalog) = s.get(format!("{}/v1/catalog", s.public), Some(DEV)).await;
    normalised(&mut catalog);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog-golden.json");
    let now = serde_json::to_string_pretty(&catalog).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &now).unwrap();
    }
    let golden = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(golden == now, "the catalogue changed; UPDATE_GOLDEN=1 writes the file:\n{now}");
}
