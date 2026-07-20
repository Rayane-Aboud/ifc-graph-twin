//! # twin-vocab
//!
//! Named constants for the ontology vocabularies the project targets. Kept in
//! its own crate — **not** in `twin-core` — so the neutral model stays free of
//! any specific ontology. `twin-core` provides the mechanism ([`twin_core::Term`],
//! a CURIE `prefix:name`); this crate provides the *known terms*.
//!
//! ## Scaling to new ontologies
//! Adding Brick, SAREF, QUDT, etc. is purely additive: a new module here. No
//! other crate changes, and `twin-core` never learns about it.
//!
//! ## Why BOT is special
//! BOT is the project's **canonical** target: every `ingest-*` notation
//! normalizes into BOT terms, so the graph has one consistent vocabulary
//! regardless of source format. Other ontologies are best expressed as
//! *downstream projections* over that canonical graph rather than as parallel
//! ingest targets — that keeps notations × ontologies at N×1, not N×M. Build
//! those projections when a second ontology actually lands, not before.

use twin_core::Term;

/// W3C Linked Building Data — Building Topology Ontology.
/// <https://w3c-lbd-cg.github.io/bot/>
pub mod bot {
    use super::Term;

    pub const PREFIX: &str = "bot";

    // Classes: bot:Zone hierarchy + bot:Element.
    pub const SITE: Term = Term::new(PREFIX, "Site");
    pub const BUILDING: Term = Term::new(PREFIX, "Building");
    pub const STOREY: Term = Term::new(PREFIX, "Storey");
    pub const SPACE: Term = Term::new(PREFIX, "Space");
    pub const ELEMENT: Term = Term::new(PREFIX, "Element");

    // Object properties (relations).
    pub const HAS_BUILDING: Term = Term::new(PREFIX, "hasBuilding");
    pub const HAS_STOREY: Term = Term::new(PREFIX, "hasStorey");
    pub const HAS_SPACE: Term = Term::new(PREFIX, "hasSpace");
    pub const CONTAINS_ELEMENT: Term = Term::new(PREFIX, "containsElement");
    pub const ADJACENT_ELEMENT: Term = Term::new(PREFIX, "adjacentElement");
    pub const INTERSECTING_ELEMENT: Term = Term::new(PREFIX, "intersectingElement");
}
