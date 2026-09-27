//! The engine: typed capabilities over a workspace and a language registry.
//! [`Engine::run`] dispatches a [`Request`] and returns an in-process [`Execution`].
//! Queries answer with concrete data; mutations retain [`Planned`] results until
//! [`Apply`] writes and commits their history entry. [`Intent`] is the
//! mutation description shared by history and batch.
//!
//! Typed clients call capability-owned `execute`, `plan`, or `apply` methods,
//! and [`Ledger`] owns history and undo. The dispatcher orchestrates those bodies;
//! interfaces consume [`Execution::into_answer`] only at a reporting boundary.
//!
//! Mutation capability types have one canonical public path, at the crate root:
//!
//! ```
//! use vvv_engine::{Rename, RenameIntent, Move, MoveIntent, MoveSymbol, MoveSymbolIntent};
//! let _: Option<(Rename, RenameIntent, Move, MoveIntent, MoveSymbol, MoveSymbolIntent)> = None;
//! ```
//!
//! They are not aliases in `protocol`:
//!
//! ```compile_fail,E0432
//! use vvv_engine::protocol::{Rename, RenameIntent, Move, MoveIntent, MoveSymbol, MoveSymbolIntent};
//! ```
//!
//! Their owning capability modules are private:
//!
//! ```compile_fail,E0603
//! use vvv_engine::capabilities::rename::Rename;
//! ```

#[cfg(test)]
extern crate self as vvv_engine;

mod batch;
mod capabilities;
mod change;
mod engine;
mod error;
mod graph;
mod history;
mod plan;
pub mod protocol;
pub mod report;
mod rewrite;
mod vfs;
mod workspace;

pub use capabilities::moves::{ExtractionError, Move, MoveIntent, MoveSymbol, MoveSymbolIntent};
pub use capabilities::rename::{Rename, RenameIntent};
pub use engine::{Engine, Execution, ExecutionKind};
pub use error::{EngineError, RecoveryError};
pub use graph::Retention;
pub use history::{Applied, Apply, HistoryError, Ledger};
pub use plan::{ApplyError, FilePreview, Planned};
pub use protocol::{
    Answer, Batch, BatchIntent, Call, Confidence, Consumer, Dead, DeadQuery, Dep, Deps, DepsQuery,
    ErrorCode, ExplainQuery, Explanation, Exposed, Failure, File, FileChange, FileQuery, History,
    HistoryEntry, Impact, ImpactQuery, ImportSite, Importer, ImportsQuery, ImportsReport, Intent,
    Locations, Match, MatchId, Mutation, MutationAnswer, MutationState, Notice, NoticeKind,
    Occurrence, Outline, OutlineItem, OutlineQuery, Placed, Reach, Reason, Recovery,
    RecoveryEffect, RecoveryIssue, RecoveryOperation, RecoveryState, RecoveryUnverified,
    References, ReferencesQuery, Reply, Request, Respelling, Rewrite, RewriteIntent, RewriteOf,
    Search, SearchQuery, Selection, SelectionError, Site, Skipped, Surface, SurfaceQuery, Template,
    TemplateError, Undo, Unreferenced, WhereQuery,
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
