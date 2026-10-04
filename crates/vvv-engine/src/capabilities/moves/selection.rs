//! Declaration identity and supported piece ownership for symbol moves.
use crate::graph::{Candidate, Graph, Namespace};
use crate::{ContentId, Engine, EngineError, Match, RelPath, Selection, SourceFile};
use serde::{Deserialize, Serialize};
use vvv_core::{CompanionOwnership, RawMatch, Role, Symbol};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SymbolMoveCandidatesQuery {
    pub name: String,
    pub from: RelPath,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolMoveCandidates {
    pub content: ContentId,
    pub candidates: Vec<SymbolMoveCandidate>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolMoveCandidate {
    pub declaration: Match,
    pub pieces: Vec<SymbolMovePiece>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported: Option<SymbolMoveUnsupported>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SymbolMoveUnsupported {
    MissingEvidence,
    SameFile,
    DestinationBindingConflict,
    NestedDeclaration,
    UnsupportedDeclarationKind,
    CompetingBinding,
    AmbiguousCompanion,
    UnsupportedCompanionTarget,
}
impl SymbolMoveUnsupported {
    pub fn message(self) -> &'static str {
        match self {
            Self::MissingEvidence => "language does not provide declaration ownership evidence",
            Self::SameFile => "source and destination are the same file",
            Self::DestinationBindingConflict => "destination already declares this name",
            Self::UnsupportedDeclarationKind => {
                "this declaration form cannot be moved independently"
            }
            Self::NestedDeclaration => "nested declarations cannot be moved independently",
            Self::CompetingBinding => "several declarations share this module and name",
            Self::AmbiguousCompanion => "companion has several possible owners",
            Self::UnsupportedCompanionTarget => {
                "companion target cannot be assigned to this declaration"
            }
        }
    }
}
impl std::fmt::Display for SymbolMoveUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SymbolMovePiece {
    pub symbol: Symbol,
    pub ownership: SymbolMoveOwnership,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum SymbolMoveOwnership {
    Declaration,
    SameScopeTarget,
}
impl SymbolMovePiece {
    fn declaration(source: &SourceFile, symbol: &Symbol) -> Result<Self, EngineError> {
        let text = source.text();
        for span in [symbol.span, symbol.name_span, symbol.extent] {
            if span.start > span.end
                || span.end > text.len()
                || !text.is_char_boundary(span.start)
                || !text.is_char_boundary(span.end)
            {
                return Err(crate::ExtractionError::InvalidSpan {
                    path: source.path().into(),
                    span,
                }
                .into());
            }
        }
        Ok(Self {
            symbol: symbol.clone(),
            ownership: SymbolMoveOwnership::Declaration,
        })
    }
}
impl SymbolMoveCandidate {
    fn of(source: &Candidate, ns: &Namespace, symbol: &Symbol) -> Result<Self, EngineError> {
        let declaration_piece = SymbolMovePiece::declaration(source.file(), symbol)?;
        let mut raw = RawMatch::plain(
            symbol.span,
            symbol.kind.as_str(),
            &source.text()[symbol.span.start..symbol.span.end],
        );
        raw.symbol = Some(symbol.clone());
        raw.role = Role::Declaration;
        let declaration = source.locate(raw);
        let facts = source.facts()?;
        let evidence = facts
            .declaration_pieces
            .iter()
            .find(|pieces| pieces.declaration == symbol.span);
        let mut unsupported = match evidence {
            None => Some(SymbolMoveUnsupported::MissingEvidence),
            Some(pieces) if !pieces.supported => {
                Some(SymbolMoveUnsupported::UnsupportedDeclarationKind)
            }
            Some(pieces) if !pieces.top_level => Some(SymbolMoveUnsupported::NestedDeclaration),
            _ => None,
        };
        if unsupported.is_none() && symbol.kind == crate::SymbolKind::Module {
            unsupported = Some(SymbolMoveUnsupported::UnsupportedDeclarationKind);
        }
        // Module/name addresses cannot distinguish conditional/overloaded peers.
        if unsupported.is_none()
            && facts
                .symbols
                .iter()
                .filter(|peer| {
                    peer.name == symbol.name
                        && ns.is_addressable(peer.kind)
                        && facts
                            .declaration_pieces
                            .iter()
                            .any(|pieces| pieces.declaration == peer.span && pieces.top_level)
                })
                .count()
                != 1
        {
            unsupported = Some(SymbolMoveUnsupported::CompetingBinding);
        }
        let mut pieces = vec![declaration_piece];
        if let Some(evidence) = evidence {
            for companion in &evidence.companions {
                match companion.ownership {
                    CompanionOwnership::AmbiguousTarget => {
                        unsupported.get_or_insert(SymbolMoveUnsupported::AmbiguousCompanion);
                    }
                    CompanionOwnership::UnsupportedTarget => {
                        unsupported
                            .get_or_insert(SymbolMoveUnsupported::UnsupportedCompanionTarget);
                    }
                    CompanionOwnership::SameScopeTarget => {
                        let symbol = facts
                            .symbols
                            .iter()
                            .find(|symbol| symbol.span == companion.span)
                            .ok_or(EngineError::InvalidSymbolMoveEvidence)?;
                        let mut piece = SymbolMovePiece::declaration(source.file(), symbol)?;
                        piece.ownership = SymbolMoveOwnership::SameScopeTarget;
                        pieces.push(piece);
                    }
                }
            }
        }
        pieces.sort_by_key(|piece| piece.symbol.extent);
        pieces.dedup_by_key(|piece| piece.symbol.extent);
        Ok(Self {
            declaration,
            pieces,
            unsupported,
        })
    }
}
impl SymbolMoveCandidatesQuery {
    pub fn execute(self, engine: &Engine) -> Result<SymbolMoveCandidates, EngineError> {
        let _operation = engine.operation();
        self.execute_in(&mut *engine.graph()?, engine.workspace())
    }
    pub(crate) fn execute_in(
        self,
        graph: &mut Graph,
        workspace: &crate::Workspace,
    ) -> Result<SymbolMoveCandidates, EngineError> {
        if self.from.as_os_str().is_empty()
            || self.from.is_absolute()
            || self.from.components().any(|part| {
                !matches!(
                    part,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            })
            || !self
                .from
                .components()
                .any(|part| matches!(part, std::path::Component::Normal(_)))
        {
            return Err(EngineError::InvalidMovePath { path: self.from });
        }
        let path = workspace.normalize(&self.from);
        let source = graph.file(&path)?;
        let ns = graph.namespace_of(&path)?;
        SymbolMoveCandidates::of(&source, &ns, &self.name)
    }
}
impl SymbolMoveCandidates {
    pub(crate) fn of(source: &Candidate, ns: &Namespace, name: &str) -> Result<Self, EngineError> {
        let mut candidates = source
            .facts()?
            .symbols
            .iter()
            .filter(|symbol| symbol.name == name && ns.is_addressable(symbol.kind))
            .map(|symbol| SymbolMoveCandidate::of(source, ns, symbol))
            .collect::<Result<Vec<_>, _>>()?;
        candidates.sort_by_key(|candidate| candidate.declaration.span);
        Ok(Self {
            content: source.file().content_id(),
            candidates,
        })
    }
    pub(crate) fn select(
        self,
        name: &str,
        selection: &Selection,
    ) -> Result<SymbolMoveCandidate, EngineError> {
        if self.candidates.is_empty() && selection.is_all() {
            return Err(EngineError::NoSuchSymbol {
                name: name.into(),
                kind: None,
            });
        }
        let matches = self
            .candidates
            .iter()
            .map(|candidate| candidate.declaration.clone())
            .collect();
        let selected = selection.narrow(matches)?;
        if selected.len() != 1 {
            return Err(EngineError::SymbolMoveSelection {
                candidates: self.candidates,
            });
        }
        let candidate = self
            .candidates
            .into_iter()
            .find(|candidate| candidate.declaration.id == selected[0].id)
            .ok_or(EngineError::InvalidSymbolMoveEvidence)?;
        if let Some(reason) = candidate.unsupported {
            return Err(EngineError::UnsupportedSymbolMove {
                declaration: Box::new(candidate.declaration),
                reason,
            });
        }
        Ok(candidate)
    }
}
impl crate::report::Document {
    pub(crate) fn symbol_move_candidates(result: &SymbolMoveCandidates) -> Self {
        let mut document = Self::new();
        document.block_body(crate::report::Block::SymbolMoveCandidates(
            result.candidates.clone(),
        ));
        document
    }
}
