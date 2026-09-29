//! Host-supplied semantic navigation. All positions are absolute UTF-8 byte spans.
use crate::{ContentId, RelPath, SourceAnchor, SymbolRef};
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ProviderVersion {
    pub provider: String,
    pub revision: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SourceVersion {
    pub path: RelPath,
    pub content: ContentId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticRequest {
    pub version: ProviderVersion,
    pub origin: SourceAnchor,
    /// The exact source against which a protocol adapter converts positions.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticTarget {
    Workspace {
        symbol: SymbolRef,
    },
    /// External locations stay explicit; the engine does not read provider URIs.
    External {
        uri: String,
        content: ContentId,
        span: crate::Span,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticReply {
    pub version: ProviderVersion,
    pub origin: SourceAnchor,
    /// Every consulted workspace source/configuration, beyond origin and targets.
    pub dependencies: Vec<SourceVersion>,
    /// The complete candidate set, never a preferred candidate.
    pub targets: Vec<SemanticTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFailure {
    Unavailable,
    Incomplete,
    Cancelled,
}

/// A provider revision must change whenever any semantic input changes, including
/// build options and external dependencies. Providers must be bounded, cooperate
/// with cancellation, and must not re-enter this engine while answering.
pub trait NavigationProvider: Send + Sync {
    fn version(&self) -> ProviderVersion;
    fn navigate(
        &self,
        request: &SemanticRequest,
        cancellation: &NavigationCancellation,
    ) -> Result<SemanticReply, SemanticFailure>;
}

#[derive(Debug, Clone, Default)]
pub struct NavigationCancellation(Arc<AtomicBool>);
impl NavigationCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub(crate) fn check(&self) -> Result<(), crate::EngineError> {
        if self.is_cancelled() {
            Err(crate::EngineError::NavigationCancelled)
        } else {
            Ok(())
        }
    }
}

pub(crate) struct SemanticNavigation<'a> {
    pub provider: &'a dyn NavigationProvider,
    pub cancellation: &'a NavigationCancellation,
}
