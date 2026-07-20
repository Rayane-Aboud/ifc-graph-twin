//! Node attribute storage.
//!
//! Kept a flat `name -> value` map for now. IFC property sets (Psets) are
//! *grouped*, so this will likely grow a `Pset` layer later — the field is named
//! `properties` (not `attrs`) specifically so that growth needs no rename.
//! A `BTreeMap` is used so serialization order is deterministic (stable diffs,
//! reproducible Cypher).

use crate::term::Term;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Ordered set of properties attached to a node.
pub type Properties = BTreeMap<String, PropertyValue>;

/// A single property value. Intentionally small; extend as real IFC data demands.
///
/// Serialized `#[serde(untagged)]`, so each variant is distinguished purely by
/// its JSON shape: string, number, bool, or object. [`Quantity`](Self::Quantity)
/// is the only object-shaped variant, so there is no ambiguity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropertyValue {
    Text(String),
    Number(f64),
    Bool(bool),
    /// A magnitude with a unit — the one thing a bare [`Number`](Self::Number)
    /// silently loses. `unit` is an open [`Term`] (e.g. `unit:squareMetre`, or
    /// QUDT `qudt:M2`), consistent with how classes and relations stay
    /// ontology-open rather than a closed enum. Use it for genuine physical
    /// quantities; a plain count or ratio stays a `Number`.
    Quantity { value: f64, unit: Term },
}

impl PropertyValue {
    /// Construct a unit-bearing [`Quantity`](Self::Quantity).
    pub fn quantity(value: f64, unit: Term) -> Self {
        PropertyValue::Quantity { value, unit }
    }
}

impl From<&str> for PropertyValue {
    fn from(s: &str) -> Self {
        PropertyValue::Text(s.to_owned())
    }
}

impl From<String> for PropertyValue {
    fn from(s: String) -> Self {
        PropertyValue::Text(s)
    }
}

impl From<f64> for PropertyValue {
    fn from(n: f64) -> Self {
        PropertyValue::Number(n)
    }
}

impl From<bool> for PropertyValue {
    fn from(b: bool) -> Self {
        PropertyValue::Bool(b)
    }
}
