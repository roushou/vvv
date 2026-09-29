//! Versioned source locations shared by search and navigation.
use serde::{Deserialize, Serialize};
use vvv_core::{LanguageId, RelPath, Span, SymbolKind};

/// Digest of complete file contents, independent of their path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(transparent)]
pub struct ContentId(String);

impl ContentId {
    pub(crate) fn from_fingerprint(value: &crate::plan::Fingerprint) -> Self {
        Self(value.as_str().to_owned())
    }

    pub(crate) fn of_bytes(bytes: &[u8]) -> Self {
        Self(blake3::hash(bytes).to_hex().to_string())
    }

    pub fn of(text: &str) -> Self {
        Self(crate::plan::Fingerprint::of(text).as_str().to_owned())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SourceAnchor {
    pub path: RelPath,
    pub content: ContentId,
    pub span: Span,
}

/// A declaration in one source version, not an identity across edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolRef {
    pub language: LanguageId,
    pub declaration: SourceAnchor,
    pub name_span: Span,
    pub kind: SymbolKind,
}
