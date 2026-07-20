//! Core error type. Edge crates (`ingest-*`, `store-*`) define their own error
//! types and convert into or wrap this where a shared vocabulary helps.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    /// A relation referenced a node handle that does not exist in this graph.
    #[error("dangling node reference: {0}")]
    DanglingRef(String),

    /// A term was malformed (e.g. empty prefix or name).
    #[error("invalid term: {0}")]
    InvalidTerm(String),
}
