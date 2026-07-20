//! Read-only query handlers over the [`SemanticGraph`].
//!
//! Enough to explore a federated model: liveness, filtered node listing, fetch
//! one node, its neighbors (graph traversal), and a `stats` summary that proves
//! several source models merged into one graph.

use std::collections::BTreeMap;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use twin_core::{Node, PropertyValue, SourceId};

use crate::state::AppState;

/// Node property key carrying which source model a node came from. This mirrors
/// `twin_ingest_ifc4::MODEL_PROP` by convention, but the API deliberately does
/// **not** depend on the ingest crate — the graph is a neutral interchange and a
/// property key is just data. The only contract is the string itself.
const MODEL_PROP: &str = "model";

/// Read a node's `model` provenance tag (set by a federated ingest), if present.
fn node_model(node: &Node) -> Option<&str> {
    match node.properties.get(MODEL_PROP) {
        Some(PropertyValue::Text(s)) => Some(s.as_str()),
        _ => None,
    }
}

#[tracing::instrument(name = "GET /health")]
pub async fn health() -> &'static str {
    tracing::debug!("health check");
    "ok"
}

#[derive(Debug, Deserialize)]
pub struct ListParams {
    /// Optional cap on how many nodes to return.
    pub limit: Option<usize>,
    /// Filter by BOT class — match the local name or full CURIE, case-insensitive
    /// (`Space`, `bot:Space`, `element`). This is how you ask for "all spaces".
    pub class: Option<String>,
    /// Filter by source model / discipline (`architecture`, `structural`, ...).
    /// This is the federated query: "all structural elements" is
    /// `?class=Element&model=structural`.
    pub model: Option<String>,
}

/// True if `node` matches every filter that was supplied.
fn matches(node: &Node, class: &Option<String>, model: &Option<String>) -> bool {
    let class_ok = class.as_ref().map_or(true, |c| {
        let c = c.to_ascii_lowercase();
        node.class.name().to_ascii_lowercase() == c
            || node.class.to_curie().to_ascii_lowercase() == c
    });
    let model_ok = model.as_ref().map_or(true, |m| {
        node_model(node).is_some_and(|nm| nm.eq_ignore_ascii_case(m))
    });
    class_ok && model_ok
}

/// `GET /nodes?class=Space&model=structural&limit=N` — list nodes, optionally
/// filtered by class and/or source model, then capped.
#[tracing::instrument(name = "GET /nodes", skip(state), fields(limit = ?params.limit, class = ?params.class, model = ?params.model))]
pub async fn list_nodes(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Json<Vec<Node>> {
    let total = state.graph.node_count();
    let limit = params.limit.unwrap_or(usize::MAX);
    let out: Vec<Node> = state
        .graph
        .nodes()
        .iter()
        .filter(|n| matches(n, &params.class, &params.model))
        .take(limit)
        .cloned()
        .collect();
    tracing::info!(returned = out.len(), total, "listed nodes");
    Json(out)
}

/// `GET /nodes/:source_id` — fetch a single node by its source identifier.
#[tracing::instrument(name = "GET /nodes/:id", skip(state), fields(source_id = %source_id))]
pub async fn get_node(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<Node>, StatusCode> {
    let sid = SourceId::new(source_id);
    let node = state
        .graph
        .resolve(&sid)
        .and_then(|r| state.graph.node(r))
        .ok_or_else(|| {
            tracing::info!("node not found");
            StatusCode::NOT_FOUND
        })?;
    tracing::info!("node found");
    Ok(Json(node.clone()))
}

/// One outgoing relationship from a node: the relation type and the node it
/// points to. This is the graph traversal a viewer struggles with — "what does
/// this storey contain", "what spaces are in this building".
#[derive(Debug, Serialize)]
pub struct Neighbor {
    /// Relation CURIE, e.g. `bot:containsElement`, `bot:hasSpace`.
    pub relation: String,
    pub node: Node,
}

/// `GET /nodes/:source_id/neighbors` — the nodes directly reachable from this
/// one, each with the relation that links them. 404 if the id is unknown.
#[tracing::instrument(name = "GET /nodes/:id/neighbors", skip(state), fields(source_id = %source_id))]
pub async fn neighbors(
    State(state): State<AppState>,
    Path(source_id): Path<String>,
) -> Result<Json<Vec<Neighbor>>, StatusCode> {
    let g = &state.graph;
    let from = g.resolve(&SourceId::new(source_id)).ok_or(StatusCode::NOT_FOUND)?;
    let out: Vec<Neighbor> = g
        .edges()
        .iter()
        .filter(|e| e.from == from)
        .filter_map(|e| {
            g.node(e.to).map(|n| Neighbor {
                relation: e.relation.to_curie(),
                node: n.clone(),
            })
        })
        .collect();
    tracing::info!(neighbors = out.len(), "listed neighbors");
    Ok(Json(out))
}

/// A count of the whole graph, broken down the two ways that matter for a
/// federated model: by semantic class, and by source model / discipline.
#[derive(Debug, Serialize)]
pub struct Stats {
    pub nodes: usize,
    pub edges: usize,
    /// `bot:Space` → count, `bot:Element` → count, ...
    pub by_class: BTreeMap<String, usize>,
    /// `architecture` → count, `structural` → count, `unknown` → count.
    pub by_model: BTreeMap<String, usize>,
}

/// `GET /stats` — the federated summary. `by_model` shows how many nodes each
/// discipline contributed to the merged graph: the proof that N files became
/// one queryable graph.
#[tracing::instrument(name = "GET /stats", skip(state))]
pub async fn stats(State(state): State<AppState>) -> Json<Stats> {
    let g = &state.graph;
    let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_model: BTreeMap<String, usize> = BTreeMap::new();
    for node in g.nodes() {
        *by_class.entry(node.class.to_curie()).or_default() += 1;
        *by_model
            .entry(node_model(node).unwrap_or("unknown").to_owned())
            .or_default() += 1;
    }
    tracing::info!(nodes = g.node_count(), classes = by_class.len(), models = by_model.len(), "stats");
    Json(Stats {
        nodes: g.node_count(),
        edges: g.edge_count(),
        by_class,
        by_model,
    })
}
