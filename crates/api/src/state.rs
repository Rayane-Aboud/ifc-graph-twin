//! Shared, cheaply-cloneable application state.
//!
//! This is the **read model** in a CQRS-shaped split: `ingest` is the write side
//! (a graph persisted to Neo4j), `serve` is the read side (the same graph held
//! in memory for queries). The CLI builds the graph through one `load_graph`
//! path and hands that exact instance here, so the persisted model and the
//! queried model cannot diverge within a run — see the CLI's `persist`/`serve`.
//!
//! The graph is read-only behind an `Arc`, so every handler clones a pointer,
//! not the graph. When live updates land (the "twin data plane"), swap the
//! `Arc<SemanticGraph>` for an `Arc<RwLock<SemanticGraph>>` or a versioned
//! snapshot handle — the handler signatures stay the same.

use std::sync::Arc;
use twin_core::SemanticGraph;

#[derive(Clone)]
pub struct AppState {
    pub graph: Arc<SemanticGraph>,
}

impl AppState {
    pub fn new(graph: SemanticGraph) -> Self {
        Self {
            graph: Arc::new(graph),
        }
    }
}
