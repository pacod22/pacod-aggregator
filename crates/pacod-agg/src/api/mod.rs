//! The server's APIs: the public catalogue, and the admin and internal APIs on a listener of their
//! own, which only servers reach.

pub mod admin;
pub mod auth;
pub mod public;
pub mod resources;

use std::sync::Arc;

use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use reqwest::StatusCode;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};

pub use auth::AuthMode;

use crate::config::Config;
use crate::settings::Settings;
use crate::store::Store;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub store: Store,
    pub auth: Arc<AuthMode>,
    pub http: reqwest::Client,
    pub egress: Arc<pacod_agg_jobs::egress::EgressPolicy>,
}

impl AppState {
    /// The settings as they are stored, and their version.
    pub async fn settings(&self) -> ApiResult<(Settings, i32)> {
        let (doc, version) = self.store.settings().await?;
        let settings = serde_json::from_value(doc).map_err(|e| {
            tracing::error!(error = %e, "the stored settings cannot be read");
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "the stored settings cannot be read")
        })?;
        Ok((settings, version))
    }
}

/// An answer that is not a success: `{"error": {"type", "message", "details"}}`, as pacod-gw
/// answers.
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub kind: &'static str,
    pub message: String,
    pub details: Option<Value>,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, kind: &'static str, message: impl Into<String>) -> ApiError {
        ApiError { status, kind, message: message.into(), details: None }
    }

    pub fn invalid(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", message)
    }

    pub fn not_found(what: &str) -> ApiError {
        ApiError::new(StatusCode::NOT_FOUND, "not_found", format!("no such {what}"))
    }

    pub fn conflict(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::CONFLICT, "conflict", message)
    }

    pub fn unavailable(message: impl Into<String>) -> ApiError {
        ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "unavailable", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = json!({ "type": self.kind, "message": self.message });
        if let Some(details) = self.details {
            body["details"] = details;
        }
        (self.status, Json(json!({ "error": body }))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        tracing::error!(error = %e, "a database call failed");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", "the aggregator's database failed")
    }
}

/// The error body of the OpenAPI document.
#[derive(Serialize, JsonSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Serialize, JsonSchema)]
pub struct ErrorDetail {
    pub r#type: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

async fn healthz() -> &'static str {
    "ok"
}

/// Ready while the database answers and, with pacod-id, while the copy of the bindings is
/// current. The catalogue is served from the database alone, so a pacod-id that is gone makes
/// the server unready only for those who need it.
async fn readyz(axum::extract::State(state): axum::extract::State<AppState>) -> impl IntoResponse {
    let database = state.store.ping().await;
    let identity = match state.auth.as_ref() {
        AuthMode::Oidc(id) => state.config.catalog_public || id.fresh(),
        AuthMode::Dev { .. } => true,
    };
    let ok = database && identity;
    let status = if ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(json!({ "ok": ok, "database": database, "identity": identity })))
}

fn probes() -> Router<AppState> {
    Router::new().route("/healthz", get(healthz)).route("/readyz", get(readyz))
}

/// The public listener: the catalogue, with Caddy in front of it.
pub fn public_router(state: AppState) -> Router {
    probes().merge(public::routes()).with_state(state)
}

/// The listener on `pacod-admin-net`: the console's API, and the scraper's.
pub fn admin_router(state: AppState) -> Router {
    probes().nest("/api/v1", admin::routes()).with_state(state)
}
