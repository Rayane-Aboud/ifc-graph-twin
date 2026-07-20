//! Thin HTTP layer over a [`SemanticGraph`]. One construction path
//! ([`build_router`]) shared by the CLI's `serve` command and by tests.

pub mod routes;
pub mod state;

use axum::{routing::get, Router};

pub use state::AppState;

/// Assemble the router. Kept separate from binding/serving so tests can call
/// handlers without opening a socket.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health))
        .route("/stats", get(routes::stats))
        .route("/nodes", get(routes::list_nodes))
        .route("/nodes/{source_id}", get(routes::get_node))
        .route("/nodes/{source_id}/neighbors", get(routes::neighbors))
        .with_state(state)
}
