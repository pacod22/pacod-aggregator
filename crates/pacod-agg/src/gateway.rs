//! What the gateway of gateway mode offers the aggregator: the models and templates its key may put
//! into `model`, for the console to choose the extraction model from. They come from pacod-gw's
//! `GET /v1/models`, asked with `AGG_GW_KEY` at `AGG_GW_URL` and nowhere else, as a page reading
//! is, and as the agent `AGG_GW_AGENT` names where it is set. The answer holds names only, never the
//! address or the key, because everyone who may read the console sees it.

use std::time::Duration;

use axum::http::StatusCode;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::api::{ApiError, ApiResult, AppState};
use crate::config::LlmLink;

/// How long the list may take: the settings page waits for it, and a list answers at once.
const LIST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OfferKind {
    /// A template of the gateway's: called as `@name`, it chooses among its models itself.
    Template,
    /// A model granted to the key's workspace.
    Model,
}

/// One thing the gateway offers: a value for `gateway_model`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
pub struct Offer {
    /// A model's id, or a template's name with its `@`, as `gateway_model` takes it.
    pub id: String,
    pub kind: OfferKind,
    /// For a template: the model it uses now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses: Option<String>,
    /// Whether its answers can be held to a JSON schema, which reading a page needs: for a
    /// template, whether every one of its models can. Null where the gateway does not say.
    pub structured_output: Option<bool>,
    /// The gateway marks it as deprecated.
    pub deprecated: bool,
}

/// What the gateway offers, templates first.
#[derive(Debug, Serialize, JsonSchema)]
pub struct GatewayOffers {
    pub models: Vec<Offer>,
}

/// Asks the gateway what it offers. Every way this fails is the gateway's, and answers 502 with the
/// reason, so that the console can say it and still show the page.
pub async fn offers(st: &AppState) -> ApiResult<GatewayOffers> {
    let link = st
        .config
        .gateway
        .as_ref()
        .ok_or_else(|| ApiError::conflict("AGG_GW_URL is not set, so there is no gateway to ask"))?;
    let failed = |message: String| {
        tracing::warn!(message = %message, "the gateway's models could not be listed");
        ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", message)
    };
    let mut request = st.http.get(format!("{}/models", link.url)).timeout(LIST_TIMEOUT);
    if let Some(key) = &link.key {
        request = request.bearer_auth(key);
    }
    let response = as_agent(request, link)
        .send()
        .await
        .map_err(|e| failed(format!("the gateway could not be reached: {}", words(e))))?;
    let status = response.status();
    let answer: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let said = answer["error"]["message"].as_str().unwrap_or("no reason given");
        return Err(failed(format!("the gateway answered {status}: {said}")));
    }
    let list =
        answer["data"].as_array().ok_or_else(|| failed("the gateway's answer holds no list of models".into()))?;
    let mut models: Vec<Offer> = list.iter().filter_map(offer).collect();
    models.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    models.dedup_by(|a, b| a.id == b.id);
    Ok(GatewayOffers { models })
}

/// A call to the gateway as the agent its key speaks for, where one is set. The list of models is
/// asked as that agent too: a gateway narrows a key to what its agent may call.
pub fn as_agent(request: reqwest::RequestBuilder, link: &LlmLink) -> reqwest::RequestBuilder {
    match &link.agent {
        Some(agent) => request.header("x-gw-agent-id", agent),
        None => request,
    }
}

/// One entry of an OpenAI-shaped model list. pacod-gw adds `gateway`, with the kind and the
/// capabilities; another endpoint's entries are models of which nothing more is known.
fn offer(entry: &Value) -> Option<Offer> {
    let id = entry["id"].as_str().map(str::trim).filter(|id| !id.is_empty())?;
    let gateway = &entry["gateway"];
    let kind =
        if gateway["kind"] == "template" || id.starts_with('@') { OfferKind::Template } else { OfferKind::Model };
    Some(Offer {
        id: id.to_owned(),
        kind,
        uses: gateway["model"].as_str().filter(|_| kind == OfferKind::Template).map(str::to_owned),
        structured_output: gateway["capabilities"]["structured_output"].as_bool(),
        deprecated: gateway["deprecated"].as_bool().unwrap_or(false),
    })
}

/// A failed request in words, with its causes, and without its address: the console shows it.
fn words(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut out = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_gateways_entries_say_what_they_are() {
        let template = json!({ "id": "@reader", "object": "model", "owned_by": "template",
            "gateway": { "kind": "template", "model": "openai/gpt-x", "deprecated": false,
                         "capabilities": { "structured_output": true } } });
        assert_eq!(
            offer(&template),
            Some(Offer {
                id: "@reader".into(),
                kind: OfferKind::Template,
                uses: Some("openai/gpt-x".into()),
                structured_output: Some(true),
                deprecated: false,
            })
        );
        let embedder = json!({ "id": "openai/embed", "gateway": { "kind": "model", "model": null, "deprecated": true,
                               "capabilities": { "structured_output": false } } });
        let e = offer(&embedder).unwrap();
        assert_eq!((e.kind, e.uses, e.structured_output, e.deprecated), (OfferKind::Model, None, Some(false), true));
    }

    #[test]
    fn another_endpoints_entries_are_models_of_unknown_abilities() {
        let plain = offer(&json!({ "id": "gpt-x", "object": "model", "owned_by": "openai" })).unwrap();
        assert_eq!((plain.kind, plain.structured_output, plain.deprecated), (OfferKind::Model, None, false));
        assert_eq!(offer(&json!({ "id": " " })), None);
        assert_eq!(offer(&json!({ "object": "model" })), None);
    }
}
