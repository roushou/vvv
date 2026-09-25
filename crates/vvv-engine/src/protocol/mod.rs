//! The JSON contract. Anything printed by `vvv --json` is one of these types,
//! so a client can rely on the shape independently of the CLI's human output.
//!
//! Data only: nothing here reads a file or takes a `Workspace`. A client that
//! only speaks JSON depends on this crate alone.

mod answer;
mod diff;
pub mod display;
mod failure;
mod intent;
mod notice;
mod question;
mod reach;
mod references;
mod request;
mod respelling;
mod result;
mod search;
mod selection;
mod template;
pub mod vocabulary;

use serde::{Deserialize, Serialize};

pub use answer::{
    Consumer, Dead, Dep, Deps, Explanation, Exposed, File, Impact, ImportSite, Importer,
    ImportsReport, Locations, Outline, OutlineItem, Placed, References, Site, Surface,
    Unreferenced,
};
pub use diff::{Diff, DiffKind, DiffLine, Hunk, LineRange};
pub use failure::{ErrorCode, Failure};
pub use intent::{
    BatchIntent, Intent, MoveIntent, MoveSymbolIntent, RenameIntent, RewriteIntent, RewriteOf,
};
pub use notice::{Notice, NoticeKind};
pub use question::{
    DeadQuery, DepsQuery, ExplainQuery, FileQuery, HistoryQuery, ImpactQuery, ImportsQuery,
    OutlineQuery, SurfaceQuery, UndoLast, WhereQuery,
};
pub use reach::Reach;
pub use references::ReferencesQuery;
pub use request::{Answer, Call, Reply, Request};
pub use respelling::Respelling;
pub use result::{
    Batch, FileChange, History, HistoryEntry, Move, MoveSymbol, Mutation, Rename, Rewrite, Search,
    Undo,
};
pub use search::{Confidence, Match, MatchId, Occurrence, Reason, Skipped};
pub use selection::{Selection, SelectionError};
pub use template::{Template, TemplateError};

/// The shape of this contract. Bumped when a field changes meaning or goes
/// away; adding fields does not bump it.
pub const SCHEMA: u32 = 1;

/// Top-level envelope of every `--json` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum Response<T> {
    Ok {
        schema: u32,
        result: T,
    },
    Error {
        schema: u32,
        #[serde(flatten)]
        failure: Failure,
    },
}

impl<T> Response<T> {
    pub fn ok(result: T) -> Self {
        Self::Ok {
            schema: SCHEMA,
            result,
        }
    }

    pub fn error(failure: Failure) -> Self {
        Self::Error {
            schema: SCHEMA,
            failure,
        }
    }
}
