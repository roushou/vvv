//! The engine: a graph of the tree built from a [`Workspace`] and a language
//! registry, and one entry point that runs a [`Command`] against it — a
//! [`protocol`] intent or query, answering with the protocol type of the
//! same name; a mutation answers with a [`Planned`] result that [`Apply`]
//! writes.
//!
//! Interfaces (CLI, TUI, JSON) talk only to [`Engine::run`]; they never touch
//! a language or the file system directly. Each command lives with its
//! components in its own module — `rewrite`, `capabilities::rename`, `capabilities::moves`,
//! `answers`, `understanding`, `batch`, `history` — as `impl Command for
//! <request>`.

mod answers;
mod batch;
mod capabilities;
mod change;
mod command;
mod engine;
mod error;
mod graph;
mod history;
mod plan;
pub mod protocol;
pub mod report;
mod request;
mod rewrite;
mod understanding;
mod vfs;
mod workspace;

pub use command::{Command, Context};
pub use engine::Engine;
pub use error::{EngineError, RecoveryError};
pub use graph::Retention;
pub use history::{Applied, Apply, HistoryError};
pub use plan::{ApplyError, FilePreview, Planned};
pub use protocol::{
    Answer, Batch, BatchIntent, Call, Confidence, Consumer, Dead, DeadQuery, Dep, Deps, DepsQuery,
    ErrorCode, ExplainQuery, Explanation, Exposed, Failure, File, FileChange, FileQuery, History,
    HistoryEntry, HistoryQuery, Impact, ImpactQuery, ImportSite, Importer, ImportsQuery,
    ImportsReport, Intent, Locations, Match, MatchId, Move, MoveIntent, MoveSymbol,
    MoveSymbolIntent, Mutation, MutationAnswer, MutationState, Notice, NoticeKind, Occurrence,
    Outline, OutlineItem, OutlineQuery, Placed, Reach, Reason, Recovery, RecoveryEffect,
    RecoveryIssue, RecoveryOperation, RecoveryState, RecoveryUnverified, References,
    ReferencesQuery, Rename, RenameIntent, Reply, Request, Respelling, Rewrite, RewriteIntent,
    RewriteOf, Search, Selection, SelectionError, Site, Skipped, Surface, SurfaceQuery, Template,
    TemplateError, Undo, UndoLast, Unreferenced, WhereQuery,
};
pub use vfs::{
    DiskVfs, EntryKind, MemoryVfs, MoveError, MoveState, ParentCreation, Stamp, Vfs, VfsError,
};
pub use workspace::Workspace;

/// The nouns of the plugin contract that appear in the engine's answers, so
/// an interface needs nothing but this crate.
pub use vvv_core::{
    RelPath,
    {
        Address, CaptureValue, Edit, Highlight, HighlightKind, ImportGroup, ImportRef, LanguageId,
        LanguageRegistry as Languages, Modifier, ModulePath, Name, Oracle, PackageId, PathHead,
        PathSyntax, Position, Query, QueryBuilder, QueryError, ReachKind, Referent, Role, Span,
        Symbol, SymbolKind,
    },
};

pub(crate) use graph::Candidate;
pub(crate) use plan::Receipt;
pub(crate) use vfs::Overlay;
pub(crate) use workspace::SourceFile;
