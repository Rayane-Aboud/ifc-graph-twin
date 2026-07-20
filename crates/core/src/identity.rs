//! Two distinct notions of identity, deliberately kept separate.
//!
//! - [`SourceId`] — the stable identifier that comes *from the source file*.
//!   For IFC this is `IfcRoot.GlobalId` (ISO 16739, a 22-char compressed GUID);
//!   for CityGML it would be `gml:id`. It is notation-agnostic on purpose so the
//!   core never assumes where it came from.
//! - [`NodeRef`] — a small internal handle into a [`crate::SemanticGraph`].
//!   It is an implementation detail (an index), not a meaningful identifier, and
//!   is only valid within the graph that produced it.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable identifier assigned by the source model (IFC `GlobalId`, `gml:id`, ...).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceId(String);

impl SourceId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SourceId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

impl From<String> for SourceId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque internal handle to a node. Cheap to copy; only valid within the
/// [`crate::SemanticGraph`] that issued it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NodeRef(u32);

impl NodeRef {
    pub(crate) fn from_index(i: usize) -> Self {
        NodeRef(i as u32)
    }

    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for NodeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}
