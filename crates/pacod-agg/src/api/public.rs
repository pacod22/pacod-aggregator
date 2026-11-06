//! The public API: what a gateway reads.

use axum::Router;

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
}
