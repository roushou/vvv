//! Navigation from a versioned occurrence, with source and metadata captured together.
use crate::graph::Graph;
use crate::protocol::display::{Line, Role};
use crate::report::Document;
use crate::{ContentId, Engine, EngineError, File, Match, SourceAnchor, SymbolRef};
use serde::{Deserialize, Serialize};
use vvv_core::{Address, Position, RelPath, Span};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NavigationOrigin {
    Position {
        path: RelPath,
        position: Position,
        #[serde(default)]
        expected_content: Option<ContentId>,
    },
    Occurrence {
        anchor: SourceAnchor,
    },
    Symbol {
        symbol: SymbolRef,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationQuery {
    pub origin: NavigationOrigin,
    #[serde(default, skip_serializing_if = "crate::Selection::is_all")]
    pub selection: crate::Selection,
}

/// Opaque identity of captured source and project inputs; not a disk transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SnapshotId(pub(crate) ContentId);

impl From<ContentId> for SnapshotId {
    fn from(content: ContentId) -> Self {
        Self(content)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationReply {
    pub snapshot: SnapshotId,
    #[serde(flatten)]
    pub outcome: NavigationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum NavigationOutcome {
    Resolved {
        target: SymbolRef,
        evidence: ResolutionEvidence,
        preview: Box<DefinitionPreview>,
    },
    Ambiguous {
        candidates: Vec<DefinitionCandidate>,
    },
    Unavailable {
        reason: UnavailableReason,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionCandidate {
    pub target: SymbolRef,
    pub declaration: Match,
    pub evidence: ResolutionEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<crate::ProviderVersion>,
    /// The binding's address followed by each re-export address, in order.
    pub addresses: Vec<Address>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionPreview {
    pub container: SymbolRef,
    pub declaration: Match,
    /// Complete captured file. All spans remain absolute UTF-8 byte ranges.
    pub source: File,
    pub selection: Span,
    pub identifiers: Vec<SourceAnchor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    NoIdentifier,
    Unresolved,
    UnsupportedContext,
    ExternalSourceUnavailable,
    CyclicImports,
}

impl UnavailableReason {
    pub fn message(self) -> &'static str {
        match self {
            Self::NoIdentifier => "Select an identifier",
            Self::Unresolved => "Could not resolve this name",
            Self::UnsupportedContext => "Cannot follow this reference yet",
            Self::ExternalSourceUnavailable => "Source is outside this workspace",
            Self::CyclicImports => "Could not resolve cyclic imports",
        }
    }
}

impl NavigationQuery {
    pub fn at(path: impl Into<RelPath>, position: Position) -> Self {
        Self {
            selection: crate::Selection::All,
            origin: NavigationOrigin::Position {
                path: path.into(),
                position,
                expected_content: None,
            },
        }
    }

    pub fn occurrence(anchor: SourceAnchor) -> Self {
        Self {
            selection: crate::Selection::All,
            origin: NavigationOrigin::Occurrence { anchor },
        }
    }

    pub fn select(mut self, selection: crate::Selection) -> Self {
        self.selection = selection;
        self
    }

    pub fn execute(self, engine: &Engine) -> Result<NavigationReply, EngineError> {
        let _operation = engine.operation();
        self.execute_in(&mut *engine.graph()?)
    }

    /// Use a versioned host provider only where syntax cannot establish a target.
    /// The provider runs under operation exclusion and must not re-enter the engine.
    pub fn execute_with(
        self,
        engine: &Engine,
        provider: &dyn crate::NavigationProvider,
        cancellation: &crate::NavigationCancellation,
    ) -> Result<NavigationReply, EngineError> {
        cancellation.check()?;
        let _operation = engine.operation();
        cancellation.check()?;
        engine.graph()?.navigate_with(
            self,
            Some(&super::semantic::SemanticNavigation {
                provider,
                cancellation,
            }),
        )
    }

    pub(crate) fn execute_in(self, graph: &mut Graph) -> Result<NavigationReply, EngineError> {
        graph.navigate(self)
    }
}

impl Document {
    pub(crate) fn navigation(result: &NavigationReply) -> Self {
        let mut report = Self::new();
        match &result.outcome {
            NavigationOutcome::Resolved { preview, .. } => {
                report.declarations(std::slice::from_ref(&preview.declaration));
                let span = preview.container.declaration.span;
                if let Some(text) = preview.source.text.get(span.start..span.end) {
                    report.body(text.lines().map(|line| Line::of(Role::Plain, line)));
                }
            }
            NavigationOutcome::Ambiguous { candidates } => {
                report.block_body(crate::report::Block::Matches(
                    candidates
                        .iter()
                        .map(|c| c.declaration.clone())
                        .collect::<Vec<_>>(),
                ));
                report.notes([Line::of(
                    Role::Dim,
                    "Several definitions match; use --select with a row number or match id",
                )]);
            }
            NavigationOutcome::Unavailable { reason } => {
                report.notes([Line::of(Role::Dim, reason.message())])
            }
        }
        report
    }
}
