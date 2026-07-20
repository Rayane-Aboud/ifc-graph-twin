//! The IFC4 → BOT mapping rules. This is where domain knowledge lives.
//!
//! Two decisions, both grounded in the specs:
//! - **IFC entity → node class.** `IfcSite`/`IfcBuilding`/`IfcBuildingStorey`/
//!   `IfcSpace` become the corresponding `bot:Zone` subclasses; everything else
//!   is a `bot:Element`. The original IFC type is preserved in `properties`
//!   under `ifcType` so no information is lost when many entities collapse to
//!   `bot:Element`.
//! - **IFC relationship → edge.** `IfcRelAggregates` builds the spatial
//!   hierarchy (`hasBuilding`/`hasStorey`/`hasSpace`, chosen by the child's
//!   class); `IfcRelContainedInSpatialStructure` becomes `containsElement`.

use crate::report::IngestReport;
use crate::source::{EntitySource, IfcRel};
use crate::IngestError;
use twin_core::{NodeRef, Properties, PropertyValue, SemanticGraph, SourceId, Term};
use twin_vocab::bot;

/// Map every entity and relationship of `source` into `graph`, returning an
/// [`IngestReport`] of what landed and what was dropped.
#[tracing::instrument(name = "map_source", skip_all)]
pub fn map_source(
    source: &impl EntitySource,
    graph: &mut SemanticGraph,
) -> Result<IngestReport, IngestError> {
    // Pass 1: nodes. Idempotent add keyed on GlobalId lets later passes resolve.
    // `entities()` is a stream of owned items; count as we drain rather than
    // asking the source for a length it may not know.
    let mut entity_count = 0usize;
    for e in source.entities() {
        let class = class_for(&e.ifc_type);
        let mut props = Properties::new();
        props.insert("ifcType".into(), PropertyValue::Text(e.ifc_type.clone()));
        if let Some(name) = &e.name {
            props.insert("name".into(), PropertyValue::Text(name.clone()));
        }
        // Physical quantities keep their unit — a bare Number would lose it.
        for q in &e.quantities {
            props.insert(
                q.name.clone(),
                PropertyValue::Quantity { value: q.value, unit: q.unit.clone() },
            );
        }
        tracing::trace!(global_id = %e.global_id, ifc_type = %e.ifc_type, class = %class, "mapped entity");
        graph.add_node(Some(SourceId::new(&e.global_id)), class, props);
        entity_count += 1;
    }
    tracing::debug!(entities = entity_count, nodes = graph.node_count(), "entities mapped");

    // Pass 2: spatial hierarchy. Relation term is chosen by the child's class.
    // Dropped counts foreshadow problem #3 (a real diagnostics channel).
    let mut agg_dropped = 0usize;
    for rel in source.aggregates() {
        if let Some((parent, child)) = resolve_pair(graph, &rel) {
            let relation = aggregate_relation(graph, child);
            tracing::trace!(relating = %rel.relating, related = %rel.related, relation = %relation, "aggregate edge");
            graph.add_edge(parent, child, relation);
        } else {
            tracing::debug!(relating = %rel.relating, related = %rel.related, "dropped aggregate: endpoint not ingested");
            agg_dropped += 1;
        }
    }

    // Pass 3: element containment.
    let mut cont_dropped = 0usize;
    for rel in source.contained_in_spatial() {
        if let Some((structure, element)) = resolve_pair(graph, &rel) {
            tracing::trace!(relating = %rel.relating, related = %rel.related, "containment edge");
            graph.add_edge(structure, element, bot::CONTAINS_ELEMENT);
        } else {
            tracing::debug!(relating = %rel.relating, related = %rel.related, "dropped containment: endpoint not ingested");
            cont_dropped += 1;
        }
    }

    let dropped = agg_dropped + cont_dropped;
    if dropped > 0 {
        tracing::warn!(dropped, agg_dropped, cont_dropped, "some relationships dropped (endpoints not ingested)");
    }
    tracing::info!(
        nodes = graph.node_count(),
        edges = graph.edge_count(),
        dropped,
        "graph built"
    );

    Ok(IngestReport {
        nodes: graph.node_count(),
        edges: graph.edge_count(),
        dropped_aggregates: agg_dropped,
        dropped_containments: cont_dropped,
    })
}

/// IFC entity type name → BOT node class.
///
/// Case-insensitive: raw STEP keywords are UPPERCASE (`IFCSITE`) while the
/// `MockSource` fixtures use the spec's CamelCase (`IfcSite`). Both map here.
fn class_for(ifc_type: &str) -> Term {
    match ifc_type.to_ascii_uppercase().as_str() {
        "IFCSITE" => bot::SITE,
        "IFCBUILDING" => bot::BUILDING,
        "IFCBUILDINGSTOREY" => bot::STOREY,
        "IFCSPACE" => bot::SPACE,
        _ => bot::ELEMENT,
    }
}

/// Pick the `bot:has*` relation for an aggregation based on the child's class.
/// Falls back to `containsElement` for non-zone decomposition.
fn aggregate_relation(graph: &SemanticGraph, child: NodeRef) -> Term {
    match graph.node(child).map(|n| &n.class) {
        Some(c) if *c == bot::BUILDING => bot::HAS_BUILDING,
        Some(c) if *c == bot::STOREY => bot::HAS_STOREY,
        Some(c) if *c == bot::SPACE => bot::HAS_SPACE,
        _ => bot::CONTAINS_ELEMENT,
    }
}

/// Resolve a `relating -> related` GlobalId pair to node handles, skipping the
/// relation if either endpoint was not ingested.
fn resolve_pair(graph: &SemanticGraph, rel: &IfcRel) -> Option<(NodeRef, NodeRef)> {
    let relating = graph.resolve(&SourceId::new(&rel.relating))?;
    let related = graph.resolve(&SourceId::new(&rel.related))?;
    Some((relating, related))
}
