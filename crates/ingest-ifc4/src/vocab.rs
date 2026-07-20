//! IFC's own namespace prefix — the *source* vocabulary of this notation.
//!
//! Lives in the notation crate, not in `twin-core` (which is ontology-free) and
//! not in `twin-vocab` (which holds the canonical *target* ontologies like BOT).
//! Each `ingest-*` crate owns the prefix for the format it reads.
//!
//! Used to preserve the original IFC entity type on a node when the BOT class
//! alone would lose information (e.g. distinguishing an `IfcWall` from an
//! `IfcColumn`, both `bot:Element`).

use twin_core::Term;

pub const PREFIX: &str = "ifc";

/// Build an `ifc:<EntityName>` term for any IFC entity, e.g. `ifc:IfcWall`.
pub fn entity(name: impl Into<String>) -> Term {
    Term::owned(PREFIX, name)
}
