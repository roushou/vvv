//! Exact declaration headers, including their attached comments and attributes.
use crate::Span;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeclarationSignature {
    pub name_span: Span,
    /// Contiguous source range, excluding the implementation or member body.
    pub span: Span,
}

/// A declaration whose full extent is a signature, or whose body can be excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignatureRule {
    pub node: &'static str,
    pub body: Option<&'static str>,
    /// Only exclude this body kind (tuple struct fields, for example, stay).
    pub body_kind: Option<&'static str>,
}
impl SignatureRule {
    pub const fn whole(node: &'static str) -> Self {
        Self {
            node,
            body: None,
            body_kind: None,
        }
    }
    pub const fn header(node: &'static str, body: &'static str) -> Self {
        Self {
            node,
            body: Some(body),
            body_kind: None,
        }
    }
    pub const fn body_kind(mut self, kind: &'static str) -> Self {
        self.body_kind = Some(kind);
        self
    }
}
