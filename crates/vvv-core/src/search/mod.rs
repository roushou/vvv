//! Search results as plain data.
//!
//! A [`Language`](crate::lang::Language) produces [`RawMatch`]es, which know
//! nothing about files; the engine ties them to a path, a position and a
//! stable id. Only what a language produces lives here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::symbol::Symbol;
use crate::text::Span;

#[derive(Debug, Clone, thiserror::Error)]
pub enum SearchError {
    #[error("invalid pattern: {0}")]
    Pattern(String),
    #[error("invalid node kind: {0}")]
    Kind(String),
    #[error("{0}")]
    Other(String),
}

/// What position a match occupies in its file: the declaration itself, a
/// token inside an import statement, or any other use. Views lead with
/// declarations and dim imports; the engine orders results by it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Declaration,
    Import,
    #[default]
    Use,
}

/// One meta-variable binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capture {
    pub span: Span,
    pub text: String,
}

/// `$A` binds one node; `$$$A` binds a sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CaptureValue {
    Single(Capture),
    Multiple(Vec<Capture>),
}

/// A match within a single text, before it is tied to a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawMatch {
    pub span: Span,
    /// Tree-sitter node kind of the matched node.
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub captures: BTreeMap<String, CaptureValue>,
    /// Set when the matched node is a declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Symbol>,
    #[serde(default)]
    pub role: Role,
}

impl RawMatch {
    /// A match with no captures and no symbol, from a span and its text.
    pub fn plain(span: Span, kind: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            span,
            kind: kind.into(),
            text: text.into(),
            captures: BTreeMap::new(),
            symbol: None,
            role: Role::Use,
        }
    }
}
