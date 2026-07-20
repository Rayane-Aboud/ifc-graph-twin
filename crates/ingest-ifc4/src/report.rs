//! What an ingest produced — and what it had to drop.
//!
//! Dropped relationships were previously only *logged* (a WARN inside
//! [`crate::mapping::map_source`]); the loss never left the function as data.
//! Returning an [`IngestReport`] makes it a value the caller can inspect, print,
//! or — later — fail on, instead of a silent side effect (problem #3).
//!
//! Counts only, by design: the per-drop detail (which endpoints, why) can be
//! promoted to a records channel if a real file ever needs it. Until then, a
//! non-zero `dropped()` is the signal to look closer.

/// Summary of one ingest: what landed in the graph, and what was discarded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IngestReport {
    /// Nodes in the resulting graph.
    pub nodes: usize,
    /// Edges in the resulting graph.
    pub edges: usize,
    /// `IfcRelAggregates` edges dropped because an endpoint wasn't ingested.
    pub dropped_aggregates: usize,
    /// `IfcRelContainedInSpatialStructure` edges dropped for the same reason.
    pub dropped_containments: usize,
}

impl IngestReport {
    /// Total relationships dropped for want of an ingested endpoint.
    pub fn dropped(&self) -> usize {
        self.dropped_aggregates + self.dropped_containments
    }
}

impl std::fmt::Display for IngestReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} nodes, {} edges, {} dropped ({} aggregate, {} containment)",
            self.nodes,
            self.edges,
            self.dropped(),
            self.dropped_aggregates,
            self.dropped_containments,
        )
    }
}
