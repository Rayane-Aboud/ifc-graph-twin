//! IFC4 → [`SemanticGraph`] ingestion.
//!
//! This crate is deliberately **concrete** — plain functions, no `Parser` trait.
//! There is exactly one input notation today, so an abstraction would be a guess.
//! When a second notation arrives (`ingest-citygml`, `ingest-ifc2x3`), a trait
//! can be pulled out of [`ingest`] mechanically, because the output type
//! ([`SemanticGraph`]) is already shared.
//!
//! File ingestion needs a real parser: enable the `real-parser` feature and
//! [`ingest`] uses [`ifclite::IfcLiteSource`], backed by `ifc-lite-core`. Without
//! it, [`ingest`] returns [`IngestError::NoParser`] rather than fabricating data.
//! The zero-dependency pipeline demo lives at [`ingest_sample`], which builds the
//! [`source::MockSource`] fixture and is honestly labeled as sample data — the
//! mock is never wired into file ingestion. Both sources implement
//! [`source::EntitySource`], so the mapping logic in [`mapping`] never changes —
//! only the source of the entity records does.
//!
//! This crate normalizes IFC into the project's canonical vocabulary (BOT, from
//! `twin-vocab`). Mapping to other ontologies is a downstream transform over the
//! finished graph, not another ingest path — see [`mapping`].

pub mod mapping;
pub mod report;
pub mod source;
pub mod steputil;
pub mod vocab;

#[cfg(feature = "real-parser")]
pub mod ifclite;

use std::path::{Path, PathBuf};
use thiserror::Error;
use twin_core::SemanticGraph;

pub use report::IngestReport;
use source::EntitySource;

/// Property key stamped on every node to record which input model it came from.
/// For a federated ingest this is the discipline label (architecture, structural,
/// hvac, ...); it is provenance, so cross-model queries can filter by source.
pub const MODEL_PROP: &str = "model";

#[derive(Debug, Error)]
pub enum IngestError {
    #[error("could not open IFC file {path}: {source}")]
    Open {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("IFC parse error: {0}")]
    Parse(String),

    #[error("this build has no IFC parser; rebuild with `--features real-parser` (or run `twin demo` for the sample graph)")]
    NoParser,
}

/// Parse an IFC4 file into a neutral [`SemanticGraph`], plus an [`IngestReport`]
/// of what landed and what was dropped.
///
/// This is the `real-parser` variant: real `ifc-lite-core` parsing via
/// [`ifclite::IfcLiteSource`]. Without the feature, the sibling below returns
/// [`IngestError::NoParser`] instead of fabricating data.
#[cfg(feature = "real-parser")]
#[tracing::instrument(name = "ingest", skip_all, fields(file = %path.display(), parser = "ifc-lite"))]
pub fn ingest(path: &Path) -> Result<(SemanticGraph, IngestReport), IngestError> {
    let source = ifclite::IfcLiteSource::open(path)?;
    ingest_from(&source)
}

/// The default build has no parser. Rather than fabricate data from a real file
/// path (the old `MockSource` trap), it fails loudly. Enable `real-parser` for
/// the variant above, or use [`ingest_sample`] / `twin demo` for the fixture.
#[cfg(not(feature = "real-parser"))]
#[tracing::instrument(name = "ingest", skip_all, fields(file = %path.display(), parser = "none"))]
pub fn ingest(path: &Path) -> Result<(SemanticGraph, IngestReport), IngestError> {
    tracing::error!("ingest called without a parser; enable the `real-parser` feature");
    Err(IngestError::NoParser)
}

/// Federated ingest: merge several IFC files into **one** graph, tagging each
/// file's nodes with its `model` label so cross-model queries can tell the
/// disciplines apart. Each `(label, path)` is parsed and mapped into the shared
/// graph in turn; the returned [`IngestReport`] carries the graph totals and the
/// summed dropped counts across all files.
///
/// This is the federation seam. Because [`mapping::map_source`] already appends
/// into an existing graph and dedupes by GlobalId, merging is just calling it
/// once per file — no special-casing. Nodes a file adds are the range
/// `[before, after)` of the node list, which is exactly what gets the label.
#[cfg(feature = "real-parser")]
#[tracing::instrument(name = "ingest_many", skip_all, fields(files = inputs.len()))]
pub fn ingest_many(
    inputs: &[(String, PathBuf)],
) -> Result<(SemanticGraph, IngestReport), IngestError> {
    use twin_core::PropertyValue;

    let mut graph = SemanticGraph::new();
    let mut dropped_aggregates = 0usize;
    let mut dropped_containments = 0usize;

    for (label, path) in inputs {
        let source = ifclite::IfcLiteSource::open(path)?;
        let start = graph.node_count();
        let report = mapping::map_source(&source, &mut graph)?;
        // Stamp provenance on just this file's newly-added nodes.
        for node in &mut graph.nodes_mut()[start..] {
            node.properties
                .insert(MODEL_PROP.into(), PropertyValue::Text(label.clone()));
        }
        dropped_aggregates += report.dropped_aggregates;
        dropped_containments += report.dropped_containments;
        tracing::info!(
            model = %label,
            file = %path.display(),
            added = graph.node_count() - start,
            "merged model"
        );
    }

    let report = IngestReport {
        nodes: graph.node_count(),
        edges: graph.edge_count(),
        dropped_aggregates,
        dropped_containments,
    };
    Ok((graph, report))
}

/// Without a parser, federated ingest can't read files any more than [`ingest`]
/// can — fail loudly rather than fabricate.
#[cfg(not(feature = "real-parser"))]
#[tracing::instrument(name = "ingest_many", skip_all, fields(files = inputs.len()))]
pub fn ingest_many(
    inputs: &[(String, PathBuf)],
) -> Result<(SemanticGraph, IngestReport), IngestError> {
    let _ = inputs;
    tracing::error!("ingest_many called without a parser; enable the `real-parser` feature");
    Err(IngestError::NoParser)
}

/// Build the sample graph from the [`source::MockSource`] fixture — no file, no
/// external parser, clearly sample data. Powers `twin demo` so the pipeline runs
/// end to end with zero dependencies, without any real path masquerading as
/// parsed. Available in every build.
pub fn ingest_sample() -> Result<(SemanticGraph, IngestReport), IngestError> {
    ingest_from(&source::MockSource::sample())
}

/// Map any [`EntitySource`] into a graph. Separated from [`ingest`] so tests and
/// a future real parser share exactly one mapping path. Returns the graph and
/// the [`IngestReport`] describing it.
pub fn ingest_from(
    source: &impl EntitySource,
) -> Result<(SemanticGraph, IngestReport), IngestError> {
    let mut graph = SemanticGraph::new();
    let report = mapping::map_source(source, &mut graph)?;
    Ok((graph, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::MockSource;
    use twin_vocab::bot;

    #[test]
    fn mock_source_produces_connected_graph() {
        let source = MockSource::sample();
        let (graph, report) = ingest_from(&source).unwrap();
        // sample has 1 building, 1 storey, 1 space, 1 wall
        assert_eq!(graph.node_count(), 4);
        // hasStorey + hasSpace + containsElement
        assert_eq!(graph.edge_count(), 3);
        assert!(graph
            .nodes()
            .iter()
            .any(|n| n.class == bot::SPACE));
        // report mirrors the graph, and the sample loses nothing.
        assert_eq!(report.nodes, 4);
        assert_eq!(report.edges, 3);
        assert_eq!(report.dropped(), 0);
        // the sample space carries a unit-bearing quantity, preserved as such.
        let space = graph
            .nodes()
            .iter()
            .find(|n| n.class == bot::SPACE)
            .unwrap();
        assert!(matches!(
            space.properties.get("NetFloorArea"),
            Some(twin_core::PropertyValue::Quantity { .. })
        ));
    }
}
