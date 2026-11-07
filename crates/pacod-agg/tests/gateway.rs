//! The gateway's models and templates, as the settings page offers them: asked of the gateway with
//! its own key, and nothing of its address in what the console is told.

mod common;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use common::{Server, serve};
use pacod_agg::config::LlmLink;
use serde_json::{Value, json};

const GATEWAY_KEY: &str = "a-key-of-the-gateway";
const PROVIDER_KEY: &str = "a-key-of-a-provider";

/// A gateway's `GET /v1/models`: answers as told, and keeps the credential of each call.
#[derive(Clone)]
struct Gateway {
    answer: Arc<(u16, Value)>,
    seen: Arc<Mutex<Vec<String>>>,
}

async fn models(State(g): State<Gateway>, headers: HeaderMap) -> (StatusCode, axum::Json<Value>) {
    let auth = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned();
    g.seen.lock().unwrap().push(auth);
    (StatusCode::from_u16(g.answer.0).unwrap(), axum::Json(g.answer.1.clone()))
}

impl Gateway {
    async fn start(status: u16, answer: Value) -> (Gateway, String) {
        let g = Gateway { answer: Arc::new((status, answer)), seen: Arc::default() };
        let app = Router::new().route("/v1/models", get(models)).with_state(g.clone());
        (g, format!("{}/v1", serve(app).await))
    }
}

fn listed() -> Value {
    let caps = |s: bool| json!({ "structured_output": s });
    json!({ "object": "list", "data": [
        { "id": "openai/gpt-x", "object": "model", "owned_by": "openai",
          "gateway": { "kind": "model", "model": null, "deprecated": false, "capabilities": caps(true),
                       "parameters_schema": { "type": "object" } } },
        { "id": "openai/embed", "object": "model", "owned_by": "openai",
          "gateway": { "kind": "model", "model": null, "deprecated": true, "capabilities": caps(false) } },
        { "id": "@reader", "object": "model", "owned_by": "template",
          "gateway": { "kind": "template", "model": "openai/gpt-x", "deprecated": false, "capabilities": caps(true) } }
    ] })
}

async fn with_gateway(url: String) -> Server {
    Server::start(|c| {
        c.gateway = Some(LlmLink::new(url, Some(GATEWAY_KEY.into())));
        c.llm = Some(LlmLink::new("https://provider.example/v1".into(), Some(PROVIDER_KEY.into())));
    })
    .await
}

#[tokio::test]
async fn the_gateways_models_and_templates_are_listed_templates_first() {
    let (gateway, url) = Gateway::start(200, listed()).await;
    let s = with_gateway(url).await;
    let (st, v) = s.api("GET", "/gateway/models", None).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        v,
        json!({ "models": [
            { "id": "@reader", "kind": "template", "uses": "openai/gpt-x", "structured_output": true, "deprecated": false },
            { "id": "openai/embed", "kind": "model", "structured_output": false, "deprecated": true },
            { "id": "openai/gpt-x", "kind": "model", "structured_output": true, "deprecated": false }
        ] })
    );
    // The gateway's own key, and only that: the provider's never goes to the gateway.
    assert_eq!(*gateway.seen.lock().unwrap(), [format!("Bearer {GATEWAY_KEY}")]);
}

#[tokio::test]
async fn without_a_gateway_there_is_nothing_to_ask() {
    let s = Server::start(|_| {}).await;
    let (st, v) = s.api("GET", "/gateway/models", None).await;
    assert_eq!(st, 409, "{v}");
    assert!(v["error"]["message"].as_str().unwrap().contains("AGG_GW_URL"), "{v}");
}

#[tokio::test]
async fn a_gateway_that_refuses_says_why_and_not_where_it_is() {
    let (_, url) = Gateway::start(401, json!({ "error": { "message": "the key is not valid" } })).await;
    let address = url.trim_start_matches("http://").trim_end_matches("/v1").to_owned();
    let s = with_gateway(url).await;
    let (st, v) = s.api("GET", "/gateway/models", None).await;
    assert_eq!(st, 502, "{v}");
    let message = v["error"]["message"].as_str().unwrap();
    assert!(message.contains("401") && message.contains("the key is not valid"), "{message}");
    assert!(!v.to_string().contains(&address), "{v}");
}

#[tokio::test]
async fn a_gateway_that_does_not_answer_says_so_without_its_address() {
    // A port that was free a moment ago: nothing listens there any more.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let s = with_gateway(format!("http://{address}/v1")).await;
    let (st, v) = s.api("GET", "/gateway/models", None).await;
    assert_eq!(st, 502, "{v}");
    let message = v["error"]["message"].as_str().unwrap();
    assert!(message.starts_with("the gateway could not be reached"), "{message}");
    assert!(!message.contains(&address) && !message.contains("127.0.0.1"), "{message}");
}

#[tokio::test]
async fn an_answer_without_a_list_is_the_gateways_fault() {
    let (_, url) = Gateway::start(200, json!({ "models": "none" })).await;
    let s = with_gateway(url).await;
    let (st, v) = s.api("GET", "/gateway/models", None).await;
    assert_eq!((st, v["error"]["type"].as_str()), (502, Some("bad_gateway")), "{v}");
}
