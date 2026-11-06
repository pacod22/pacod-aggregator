//! The admin API, for the console.

use axum::routing::get;
use axum::{Json, Router};
use schemars::JsonSchema;
use serde::Serialize;

use super::AppState;
use super::auth::Principal;

/// Who is calling, and what they may do.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Session {
    pub id: String,
    pub display: String,
    /// `user` or `service`.
    pub kind: String,
    pub permissions: Vec<String>,
}

async fn session(p: Principal) -> Json<Session> {
    let permissions = p.permissions().into_iter().map(str::to_owned).collect();
    Json(Session { id: p.id, display: p.display, kind: p.kind.to_owned(), permissions })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/session", get(session))
}
