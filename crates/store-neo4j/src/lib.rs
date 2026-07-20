//! [`SemanticGraph`] → Neo4j.
//!
//! Concrete on purpose — one backend, no `Store` trait yet (see the workspace
//! README for the rule-of-three rationale). When a second backend appears
//! (`store-rdf`, ...), a trait can be extracted from [`write_graph`] because the
//! input type is already the shared [`SemanticGraph`].
//!
//! The Cypher produced here is real and idempotent: re-running an ingest
//! `MERGE`s rather than duplicating. Two ways to consume it:
//!
//! - [`write_graph`] — always available, offline, no network. Streams each
//!   statement to a sink (print, count, buffer). This is the default path.
//! - [`execute_graph`] — behind the `bolt` feature. Opens a real Bolt session
//!   and runs the same statements in one transaction. The statement/param split
//!   in [`cypher`] is shared, so the two paths never drift.

pub mod cypher;

use thiserror::Error;
use twin_core::SemanticGraph;

pub use cypher::{CypherStatement, ParamValue};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("neo4j connection error: {0}")]
    Connection(String),

    #[error("cypher execution error: {0}")]
    Execution(String),
}

/// Connection settings for a Neo4j instance.
#[derive(Clone, Debug)]
pub struct Neo4jConfig {
    pub uri: String,
    pub user: String,
    pub password: String,
    pub database: Option<String>,
}

impl Default for Neo4jConfig {
    fn default() -> Self {
        Self {
            uri: "bolt://127.0.0.1:7687".into(),
            user: "neo4j".into(),
            password: "neo4j".into(),
            database: None,
        }
    }
}

/// Emit the ordered Cypher statements that persist `graph` (all nodes first,
/// then relationships), handing each to `sink` as it is produced.
///
/// The sink is the write-side streaming seam. Statements are pushed one at a
/// time instead of collected into a `Vec`, so a caller can print them, count
/// them, or feed them elsewhere without this function ever holding the whole
/// batch in memory. A caller that still wants a `Vec` passes a closure that
/// pushes into one. To actually run the statements against a live database, use
/// [`execute_graph`] (feature `bolt`), which shares the same builders.
#[tracing::instrument(name = "write_graph", skip_all, fields(uri = %_cfg.uri))]
pub fn write_graph(
    graph: &SemanticGraph,
    _cfg: &Neo4jConfig,
    mut sink: impl FnMut(CypherStatement) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    let mut emitted = 0usize;
    for node in graph.nodes() {
        let stmt = cypher::merge_node(node);
        tracing::trace!(query = %stmt.query, "node statement");
        sink(stmt)?;
        emitted += 1;
    }
    for edge in graph.edges() {
        let stmt = cypher::merge_edge(edge);
        tracing::trace!(query = %stmt.query, "edge statement");
        sink(stmt)?;
        emitted += 1;
    }
    tracing::info!(
        statements = emitted,
        nodes = graph.node_count(),
        edges = graph.edge_count(),
        "emitted cypher"
    );
    Ok(())
}

/// Run the graph's persistence statements against a live Neo4j over Bolt, in a
/// single transaction. Returns the number of statements executed.
///
/// This reuses [`write_graph`] to produce the exact same ordered statements the
/// offline path prints — nodes first, then edges — so the two consumers can
/// never drift. Each [`CypherStatement`] is rebound to a driver query and run;
/// the whole batch commits atomically or rolls back.
///
/// `cfg.database` is currently ignored (the driver's default database is used);
/// multi-db routing waits for a caller that needs it.
///
/// TODO(verify-api): pinned to `neo4rs` 0.8. The `Graph::new` / `start_txn` /
/// `Query::param` / `commit` surface is the moving part — bump deliberately and
/// re-check these calls when the version changes (mirrors the ifc-lite pin).
#[cfg(feature = "bolt")]
#[tracing::instrument(name = "execute_graph", skip_all, fields(uri = %cfg.uri))]
pub async fn execute_graph(
    graph: &SemanticGraph,
    cfg: &Neo4jConfig,
) -> Result<usize, StoreError> {
    // One source of statement ordering: collect what the offline sink would emit.
    let mut statements: Vec<CypherStatement> = Vec::new();
    write_graph(graph, cfg, |stmt| {
        statements.push(stmt);
        Ok(())
    })?;

    let driver = neo4rs::Graph::new(&cfg.uri, &cfg.user, &cfg.password)
        .await
        .map_err(|e| StoreError::Connection(e.to_string()))?;

    let mut txn = driver
        .start_txn()
        .await
        .map_err(|e| StoreError::Connection(e.to_string()))?;

    let total = statements.len();
    for stmt in statements {
        txn.run(bind(stmt))
            .await
            .map_err(|e| StoreError::Execution(e.to_string()))?;
    }

    txn.commit()
        .await
        .map_err(|e| StoreError::Execution(e.to_string()))?;

    tracing::info!(statements = total, "committed cypher over bolt");
    Ok(total)
}

/// Turn a driver-neutral [`CypherStatement`] into a bound `neo4rs::Query`.
#[cfg(feature = "bolt")]
fn bind(stmt: CypherStatement) -> neo4rs::Query {
    let mut q = neo4rs::query(&stmt.query);
    for (key, value) in stmt.params {
        q = match value {
            ParamValue::Text(s) => q.param(&key, s),
            ParamValue::Number(n) => q.param(&key, n),
            ParamValue::Bool(b) => q.param(&key, b),
            ParamValue::Int(i) => q.param(&key, i),
            ParamValue::Null => q.param(&key, None::<String>),
        };
    }
    q
}
