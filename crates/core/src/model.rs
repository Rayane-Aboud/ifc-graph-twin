//! The neutral semantic graph: nodes, edges, and the builder that assigns
//! internal handles and de-duplicates by [`SourceId`].
//!
//! This is the single type every `ingest-*` crate produces and every `store-*`
//! crate consumes. It is intentionally a plain in-memory structure
//! (`Vec` of nodes + `Vec` of edges + a source-id index), not a `petgraph`
//! graph, so the core stays decoupled from any traversal library. If graph
//! algorithms are needed later, a `petgraph` view can be built *from* this
//! without leaking its types into the public model.

use crate::identity::{NodeRef, SourceId};
use crate::properties::Properties;
use crate::term::Term;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single entity in the graph (a `bot:Space`, a `bot:Element`, ...).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    /// Internal handle to this node within its graph.
    pub id: NodeRef,
    /// Stable identifier from the source model, if any.
    pub source_id: Option<SourceId>,
    /// Semantic type, e.g. `bot:Space`. Open vocabulary — see [`Term`].
    pub class: Term,
    /// Attributes / property-set values.
    pub properties: Properties,
}

/// A directed, typed relationship between two nodes (a `bot:containsElement`, ...).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeRef,
    pub to: NodeRef,
    /// Relation type, e.g. `bot:containsElement`. Open vocabulary — see [`Term`].
    pub relation: Term,
}

/// The neutral graph. Build it with [`SemanticGraph::add_node`] /
/// [`SemanticGraph::add_edge`]; read it back with the accessors.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SemanticGraph {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    #[serde(skip)]
    by_source: HashMap<SourceId, NodeRef>,
}

impl SemanticGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a node and return its handle. If `source_id` is `Some` and already
    /// present, the existing node is returned unchanged (idempotent ingestion).
    pub fn add_node(
        &mut self,
        source_id: Option<SourceId>,
        class: Term,
        properties: Properties,
    ) -> NodeRef {
        if let Some(sid) = &source_id {
            if let Some(existing) = self.by_source.get(sid) {
                return *existing;
            }
        }

        let id = NodeRef::from_index(self.nodes.len());
        if let Some(sid) = &source_id {
            self.by_source.insert(sid.clone(), id);
        }
        self.nodes.push(Node {
            id,
            source_id,
            class,
            properties,
        });
        id
    }

    /// Add a typed edge between two existing nodes.
    pub fn add_edge(&mut self, from: NodeRef, to: NodeRef, relation: Term) {
        self.edges.push(Edge { from, to, relation });
    }

    /// Resolve a source identifier to its internal handle, if ingested.
    pub fn resolve(&self, source_id: &SourceId) -> Option<NodeRef> {
        self.by_source.get(source_id).copied()
    }

    pub fn node(&self, r: NodeRef) -> Option<&Node> {
        self.nodes.get(r.index())
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// Mutable view of the nodes, symmetric with [`nodes`](Self::nodes). Intended
    /// for editing node *attributes* (e.g. tagging provenance after a merge) — not
    /// for changing a node's `source_id`, which would desync the source-id index
    /// (call [`reindex`](Self::reindex) if you must). Handy for federating several
    /// ingests into one graph and labeling each batch.
    pub fn nodes_mut(&mut self) -> &mut [Node] {
        &mut self.nodes
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Rebuild the source-id index after deserialization (the index is not
    /// serialized, since it is derivable from the nodes).
    pub fn reindex(&mut self) {
        self.by_source.clear();
        for node in &self.nodes {
            if let Some(sid) = &node.source_id {
                self.by_source.insert(sid.clone(), node.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // core is ontology-free, so tests build terms directly rather than pulling
    // in `twin-vocab` (which would be a dependency cycle).
    fn term(name: &'static str) -> Term {
        Term::new("bot", name)
    }

    #[test]
    fn add_node_is_idempotent_on_source_id() {
        let mut g = SemanticGraph::new();
        let a = g.add_node(Some("guid-1".into()), term("Space"), Properties::new());
        let b = g.add_node(Some("guid-1".into()), term("Space"), Properties::new());
        assert_eq!(a, b);
        assert_eq!(g.node_count(), 1);
    }

    #[test]
    fn edges_connect_resolved_nodes() {
        let mut g = SemanticGraph::new();
        let storey = g.add_node(Some("s".into()), term("Storey"), Properties::new());
        let space = g.add_node(Some("sp".into()), term("Space"), Properties::new());
        g.add_edge(storey, space, term("hasSpace"));
        assert_eq!(g.edge_count(), 1);
        assert_eq!(g.resolve(&"sp".into()), Some(space));
    }
}
