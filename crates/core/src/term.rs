//! An open, namespaced semantic type — the ontology-scalability seam.
//!
//! Node classes and edge relations are *not* a closed Rust enum. If they were,
//! supporting a second vocabulary (Brick, SAREF, QUDT, ...) would mean editing
//! the core every time. Instead a [`Term`] is a CURIE-style `prefix:name` pair,
//! exactly how the linked-data world scales across ontologies. `bot:Space` and
//! `brick:Temperature_Sensor` are equally first-class; BOT is just the default
//! vocabulary the IFC4 mapper happens to emit.
//!
//! Known terms are provided as constants in the `twin-vocab` crate; arbitrary
//! terms can be built at runtime with [`Term::owned`].

use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;

/// A namespaced semantic type, e.g. `bot:Space` or `ifc:IfcWall`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Term {
    /// Vocabulary prefix, e.g. `bot`, `ifc`, `brick`.
    pub prefix: Cow<'static, str>,
    /// Local name within the vocabulary, e.g. `Space`, `hasStorey`.
    pub name: Cow<'static, str>,
}

impl Term {
    /// Construct a term from `'static` strings — usable in `const` context, which
    /// is how `twin-vocab` defines its constants.
    pub const fn new(prefix: &'static str, name: &'static str) -> Self {
        Self {
            prefix: Cow::Borrowed(prefix),
            name: Cow::Borrowed(name),
        }
    }

    /// Construct a term from owned strings — for vocabularies discovered at
    /// runtime (e.g. an unrecognized IFC entity mapped through to an `ifc:` term).
    pub fn owned(prefix: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            prefix: Cow::Owned(prefix.into()),
            name: Cow::Owned(name.into()),
        }
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// `prefix:name` compact IRI form.
    pub fn to_curie(&self) -> String {
        format!("{}:{}", self.prefix, self.name)
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.prefix, self.name)
    }
}
