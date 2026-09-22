use crate::{ApplyError, ErrorCode, Failure, SelectionError, TemplateError, VfsError};

use vvv_core::RelPath;
use vvv_core::{
    EditConflict, LanguageId, Position, QueryError, ResolveError, SearchError, SymbolKind,
};

use crate::history::HistoryError;

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("{}: {source}", path.display())]
    Open {
        path: RelPath,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Vfs(#[from] VfsError),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error("search failed in {}", path.display())]
    Search {
        path: RelPath,
        #[source]
        source: SearchError,
    },
    #[error(transparent)]
    Selection(#[from] SelectionError),
    #[error("{}:{}", path.display(), line + 1)]
    Template {
        path: RelPath,
        line: u32,
        #[source]
        source: TemplateError,
    },
    #[error(transparent)]
    Conflict(#[from] EditConflict),
    #[error("`{name}` is declared in several places ({}); pick one with --in <file>", declarations.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))]
    AmbiguousSymbol {
        name: String,
        declarations: Vec<crate::graph::DeclarationSite>,
    },
    #[error("no {} named `{name}`", kind.map_or("symbol", SymbolKind::as_str))]
    NoSuchSymbol {
        name: String,
        kind: Option<SymbolKind>,
    },
    #[error("{} already exists", .0.display())]
    Exists(RelPath),
    #[error("no registered language claims {}", .0.display())]
    NoLanguage(RelPath),
    #[error("{0} has no layout yet, so its paths cannot be followed or its files moved")]
    NoLayout(LanguageId),
    #[error("{}:{} is past the end of the file", path.display(), position.display())]
    NoSuchPosition { path: RelPath, position: Position },
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error(transparent)]
    Apply(#[from] ApplyError),
    #[error(transparent)]
    History(#[from] HistoryError),
}

impl EngineError {
    /// The stable code a client branches on.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Open { .. } => ErrorCode::Io,
            Self::Vfs(e)
            | Self::History(HistoryError::Vfs(e))
            | Self::Apply(ApplyError::Vfs(e)) => match e {
                VfsError::NotFound(_) => ErrorCode::NotFound,
                _ => ErrorCode::Io,
            },
            Self::Query(_) => ErrorCode::BadQuery,
            Self::Search { .. } => ErrorCode::BadPattern,
            Self::Selection(_) => ErrorCode::BadSelection,
            Self::Template { .. } => ErrorCode::BadTemplate,
            Self::Conflict(_) => ErrorCode::Conflict,
            Self::AmbiguousSymbol { .. } => ErrorCode::AmbiguousSymbol,
            Self::NoSuchSymbol { .. } => ErrorCode::NoSuchSymbol,
            Self::Exists(_) => ErrorCode::Exists,
            Self::NoLanguage(_) => ErrorCode::NoLanguage,
            Self::NoLayout(_) => ErrorCode::NoLayout,
            Self::NoSuchPosition { .. } => ErrorCode::NoSuchPosition,
            Self::Resolve(e) => match e {
                ResolveError::Missing(_) | ResolveError::NoDeclaringFile { .. } => {
                    ErrorCode::NotFound
                }
                _ => ErrorCode::Unmovable,
            },
            Self::Apply(e) => match e {
                ApplyError::Stale { .. } | ApplyError::Modified { .. } => ErrorCode::Stale,
                _ => ErrorCode::Io,
            },
            Self::History(_) => ErrorCode::NoHistory,
        }
    }

    /// What to try instead, when the error suggests something.
    pub fn hint(&self) -> Option<String> {
        match self {
            Self::NoSuchSymbol { name, .. } => Some(format!(
                "declarations are matched by exact name; `vvv search --name {name}` shows what exists"
            )),
            Self::AmbiguousSymbol { declarations, .. } => Some(format!(
                "for example: --in {}",
                declarations
                    .first()
                    .map(|d| d.path.display().to_string())
                    .unwrap_or_default()
            )),
            Self::Query(_) => Some(
                "search by pattern:  vvv search 'fn $NAME($$$) { $$$ }'\nby declaration:     vvv search --symbol trait   |   vvv search --name Foo"
                    .to_owned(),
            ),
            _ => None,
        }
    }
}

impl From<&EngineError> for Failure {
    fn from(error: &EngineError) -> Self {
        let failure = Failure::new(error.code(), format!("{error:#}"));
        match error.hint() {
            Some(hint) => failure.with_hint(hint),
            None => failure,
        }
    }
}
