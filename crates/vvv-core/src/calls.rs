//! Syntactic call sites and declarative extraction rules; no target inference.
use crate::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CallKind {
    Direct,
    Member,
    Indirect,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CallSite {
    pub span: Span,
    pub callee: Span,
    pub kind: CallKind,
    /// Name span of the nearest named callable, absent inside anonymous callables.
    pub owner: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallRule {
    pub node: &'static str,
    pub callee: &'static str,
}

/// Select the relevant child of a callee. None unwraps without classifying it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalleeRule {
    pub node: &'static str,
    pub field: &'static str,
    pub kind: Option<CallKind>,
}
