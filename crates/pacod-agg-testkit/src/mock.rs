//! The mock: a vendor's site for the scraper to find and render, and an OpenAI-compatible endpoint
//! that reads a page's tables as a capable model would.
//!
//! The site is copied from pacod-gw (commit 50fe4b7, `crates/pacod-gw-testkit/src/mock/server.rs`).

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use axum::extract::{Path, State};
use axum::http::header::{CONTENT_TYPE, HOST};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use crate::extract::price_page_reply;

/// Which version of its pricing page the site serves: 1, or 2 after `POST /__mock/pricing`.
#[derive(Clone, Default)]
struct Mock {
    variant: Arc<AtomicU8>,
}

fn html(body: String) -> Response {
    ([(CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

/// The pages of the mock's site. Variant 2 of the pricing page changes `tracked-steady`'s price
/// and lists `tracked-new`, which no catalogue has.
pub fn site_page(page: &str, variant: u8) -> Option<String> {
    let html = |title: &str, body: &str| {
        format!("<html><head><title>{title}</title></head><body><h1>{title}</h1>{body}</body></html>")
    };
    Some(match page {
        "about" => html(
            "About the mock",
            "<p>The mock vendor serves models. See <a href=\"/__mock/site/pricing\">pricing</a> and \
             <a href=\"/__mock/site/models\">models</a>.</p>",
        ),
        "pricing" => {
            let (steady, new) = match variant {
                2 => (
                    "$2.40",
                    "<tr><td>tracked-new</td><td>$1.00 / 1M tokens</td><td>$3.00 / 1M tokens</td><td>$0.10 / 1M tokens</td></tr>",
                ),
                _ => ("$2.20", ""),
            };
            html(
                "Pricing",
                &format!(
                    "<p>Prices per one million tokens.</p><table><tr><th>Model</th><th>Input</th><th>Output</th>\
                     <th>Cached input</th></tr><tr><td>tracked-steady</td><td>{steady} / 1M tokens</td>\
                     <td>$10.00 / 1M tokens</td><td>$0.22 / 1M tokens</td></tr><tr><td>tracked-jump</td>\
                     <td>$4.00 / 1M tokens</td><td>$20.00 / 1M tokens</td><td>$0.40 / 1M tokens</td></tr>{new}</table>"
                ),
            )
        }
        "models" => html(
            "Models",
            "<table><tr><th>Model</th><th>Context</th><th>Max output</th><th>Vision</th></tr>\
             <tr><td>tracked-steady</td><td>128K</td><td>16384</td><td>yes</td></tr>\
             <tr><td>tracked-jump</td><td>200K</td><td>32768</td><td>no</td></tr></table>",
        ),
        "news" => html("News", "<p>The mock was released.</p>"),
        "private/pricing" => html("Private pricing", "<p>tracked-steady $0.01 / 1M tokens</p>"),
        _ => return None,
    })
}

async fn site(State(m): State<Mock>, Path(page): Path<String>) -> Response {
    match site_page(&page, m.variant.load(Ordering::Relaxed)) {
        Some(body) => html(body),
        None => (StatusCode::NOT_FOUND, format!("no page {page}")).into_response(),
    }
}

async fn robots() -> Response {
    ([(CONTENT_TYPE, "text/plain")], "User-agent: *\nDisallow: /__mock/site/private/\n").into_response()
}

/// The site's sitemap, naming its pages by the address it is reached at.
async fn sitemap(headers: HeaderMap) -> Response {
    let host = headers.get(HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost");
    let urls: String = ["about", "pricing", "models", "news", "private/pricing"]
        .iter()
        .map(|p| format!("<url><loc>http://{host}/__mock/site/{p}</loc></url>"))
        .collect();
    ([(CONTENT_TYPE, "application/xml")], format!("<?xml version=\"1.0\"?><urlset>{urls}</urlset>")).into_response()
}

async fn set_variant(State(m): State<Mock>, Json(body): Json<Value>) -> Json<Value> {
    let variant = u8::from(body["variant"].as_u64() == Some(2)) + 1;
    m.variant.store(variant, Ordering::Relaxed);
    Json(json!({ "variant": variant }))
}

/// An OpenAI-compatible chat completion. It knows one question: what a page says, in the schema
/// named `price_page`.
async fn completions(Json(body): Json<Value>) -> Response {
    if body.pointer("/response_format/json_schema/name").and_then(Value::as_str) != Some("price_page") {
        let error = json!({ "error": { "message": "the mock answers the schema price_page and nothing else" } });
        return (StatusCode::BAD_REQUEST, Json(error)).into_response();
    }
    let prompt = body.pointer("/messages/0/content").and_then(Value::as_str).unwrap_or_default();
    let content = price_page_reply(prompt).to_string();
    let answer = json!({
        "id": "chatcmpl-mock", "object": "chat.completion", "model": body["model"],
        "choices": [{ "index": 0, "finish_reason": "stop", "message": { "role": "assistant", "content": content } }],
    });
    Json(answer).into_response()
}

pub fn router() -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/robots.txt", get(robots))
        .route("/sitemap.xml", get(sitemap))
        .route("/__mock/site/{*page}", get(site))
        .route("/__mock/pricing", post(set_variant))
        .route("/v1/chat/completions", post(completions))
        .with_state(Mock { variant: Arc::new(AtomicU8::new(1)) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_second_pricing_page_changes_one_price_and_adds_a_model() {
        let (one, two) = (site_page("pricing", 1).unwrap(), site_page("pricing", 2).unwrap());
        assert!(one.contains("$2.20") && !one.contains("tracked-new"));
        assert!(two.contains("$2.40") && two.contains("tracked-new"));
        assert!(site_page("nothing", 1).is_none());
    }
}
