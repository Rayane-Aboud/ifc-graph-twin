//! # twin-core
//!
//! The neutral semantic-graph model at the center of the project. Every input
//! notation (`ingest-*`) produces a [`SemanticGraph`]; every graph backend
//! (`store-*`) consumes one. Nothing in here knows about IFC, CityGML, Neo4j,
//! RDF, or any specific ontology.
//!
//! ## Extension points
//! - **New ontology term** — construct a [`Term`] with any prefix. Named
//!   constants for known vocabularies live in the separate `twin-vocab` crate,
//!   so core stays ontology-free.
//! - **New notation / backend** — a new crate at the edge; it only ever touches
//!   the types re-exported below.

pub mod error;
pub mod identity;
pub mod model;
pub mod properties;
pub mod term;

pub use error::CoreError;
pub use identity::{NodeRef, SourceId};
pub use model::{Edge, Node, SemanticGraph};
pub use properties::{Properties, PropertyValue};
pub use term::Term;
