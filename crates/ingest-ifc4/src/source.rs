//! The integration seam with a real IFC parser.
//!
//! [`EntitySource`] is the *narrow* shape the mapper needs: the list of IFC
//! entities plus the two spatial relationship tables. It is an internal trait
//! (not the cross-notation abstraction) whose second implementation is the real
//! parser — so it is justified today by exactly the mock/real pair.
//!
//! Named `EntitySource` (not `EntityStore`) to avoid colliding with the output
//! side of the system: `store-*` crates are graph *backends* the pipeline writes
//! to; an `EntitySource` is where entities are *read from*.
//!
//! The real implementation is [`crate::ifclite::IfcLiteSource`] (feature
//! `real-parser`); this module keeps [`MockSource`], a pure **test/demo
//! fixture**. It never parses a file — it hands back a hand-built sample graph
//! — so it is deliberately *not* wired into file ingestion. See
//! [`crate::ingest_sample`] for the one honest, clearly-labeled entry point.

use twin_core::Term;

/// One IFC entity, reduced to what the mapper needs.
#[derive(Clone, Debug)]
pub struct IfcEntity {
    /// `IfcRoot.GlobalId` (ISO 16739): the 22-char compressed GUID.
    pub global_id: String,
    /// IFC entity type name, e.g. `IfcWall`, `IfcSpace`.
    pub ifc_type: String,
    /// `IfcRoot.Name`, if present.
    pub name: Option<String>,
    /// Named physical quantities carried by the entity (e.g. `NetFloorArea`).
    /// Empty for most entities; a source that extracts IFC quantities populates
    /// it, and the mapper turns each into a unit-bearing `PropertyValue`.
    pub quantities: Vec<IfcQuantity>,
}

/// A named IFC quantity: a magnitude plus the unit it is expressed in.
///
/// The source picks the unit vocabulary (here, a `unit:` [`Term`]); mapping a
/// real `IfcSIUnit`/`IfcNamedUnit` to a canonical unit term is a future step for
/// the real parser. Kept here (not in `mapping`) so the value never reaches the
/// graph stripped of its unit.
#[derive(Clone, Debug)]
pub struct IfcQuantity {
    /// Quantity name, e.g. `NetFloorArea`, `Height`.
    pub name: String,
    pub value: f64,
    /// Unit as an open term, e.g. `unit:squareMetre`.
    pub unit: Term,
}

/// A directed IFC relationship, `relating -> related` by GlobalId.
#[derive(Clone, Debug)]
pub struct IfcRel {
    pub relating: String,
    pub related: String,
}

/// The parsed IFC model, as far as the mapper cares.
///
/// Methods yield *owned* items through iterators, not borrowed slices. That is
/// the streaming seam: `MockSource` and `IfcLiteSource` happen to iterate a
/// `Vec` they already hold, but a future lazy parser can yield entities pulled
/// straight off the byte stream with no backing collection. The mapper never
/// learns which it got. Relationships still need every node resolved first (IFC
/// GlobalId references are forward-declarable), so the mapper drains entities
/// before relationships — but nothing here forces the source to materialize.
pub trait EntitySource {
    fn entities(&self) -> impl Iterator<Item = IfcEntity> + '_;
    /// `IfcRelAggregates`: `relating` (parent) decomposes into `related` (child).
    /// Drives the spatial hierarchy (`hasBuilding`/`hasStorey`/`hasSpace`).
    fn aggregates(&self) -> impl Iterator<Item = IfcRel> + '_;
    /// `IfcRelContainedInSpatialStructure`: `relating` (spatial structure)
    /// contains `related` (element). Drives `containsElement`.
    fn contained_in_spatial(&self) -> impl Iterator<Item = IfcRel> + '_;
}

/// A hand-built sample model — enough real structure to exercise the whole
/// pipeline and keep the mapper honest, with none of the STEP-parsing work done.
///
/// This is a **fixture, not a parser**: it has no `open(path)` and never reads a
/// file, precisely so it can't masquerade as real ingestion. Tests build it with
/// [`MockSource::sample`]; the `demo` command reaches it via
/// [`crate::ingest_sample`].
pub struct MockSource {
    entities: Vec<IfcEntity>,
    aggregates: Vec<IfcRel>,
    contained: Vec<IfcRel>,
}

impl MockSource {
    /// A tiny hand-built model: Building → Storey → Space, plus a Wall contained
    /// in the storey. Mirrors the shape of a real IFC spatial tree.
    pub fn sample() -> Self {
        let entities = vec![
            IfcEntity { global_id: "B0000000000000000001".into(), ifc_type: "IfcBuilding".into(), name: Some("HQ".into()), quantities: vec![] },
            IfcEntity { global_id: "S0000000000000000001".into(), ifc_type: "IfcBuildingStorey".into(), name: Some("Level 1".into()), quantities: vec![] },
            // The space carries a real quantity so the unit-bearing path is
            // exercised end to end, not just modelled.
            IfcEntity {
                global_id: "P0000000000000000001".into(),
                ifc_type: "IfcSpace".into(),
                name: Some("Office 101".into()),
                quantities: vec![IfcQuantity {
                    name: "NetFloorArea".into(),
                    value: 23.5,
                    unit: Term::owned("unit", "squareMetre"),
                }],
            },
            IfcEntity { global_id: "W0000000000000000001".into(), ifc_type: "IfcWall".into(), name: Some("Wall-A".into()), quantities: vec![] },
        ];
        let aggregates = vec![
            IfcRel { relating: "B0000000000000000001".into(), related: "S0000000000000000001".into() },
            IfcRel { relating: "S0000000000000000001".into(), related: "P0000000000000000001".into() },
        ];
        let contained = vec![
            IfcRel { relating: "S0000000000000000001".into(), related: "W0000000000000000001".into() },
        ];
        Self { entities, aggregates, contained }
    }
}

impl EntitySource for MockSource {
    fn entities(&self) -> impl Iterator<Item = IfcEntity> + '_ {
        self.entities.iter().cloned()
    }
    fn aggregates(&self) -> impl Iterator<Item = IfcRel> + '_ {
        self.aggregates.iter().cloned()
    }
    fn contained_in_spatial(&self) -> impl Iterator<Item = IfcRel> + '_ {
        self.contained.iter().cloned()
    }
}
