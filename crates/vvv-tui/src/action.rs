//! What the user can do ([`Action`]), what the model wants done ([`Effect`]),
//! and what comes back from the engine ([`Event`]). All plain data.

use vvv_engine::RelPath;

use vvv_engine::HistoryEntry;
use vvv_engine::protocol::FileChange;
use vvv_engine::report::Document;
use vvv_engine::{
    Answer, Highlight, Intent, Match, Notice, Occurrence, Query, Request, Respelling, Skipped,
};

use super::model::MenuTarget;

/// A user intention, already mapped from a key by the current mode and the
/// focused panel. Generic where it can be: `Enter` does what the status bar
/// says, `Input` goes to whichever input the mode has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// First tick.
    Start,
    Quit,
    /// Focus the next / previous panel, or the n-th (1-based).
    FocusNext,
    FocusPrev,
    FocusNth(u8),
    /// Move the cursor of the focused list by `n` rows (negative = up).
    Move(i32),
    /// Jump to the previous / next file in the search results.
    File(i32),
    /// Edit the local fuzzy filter on result file paths.
    FilterFiles,
    ClearFileFilter,
    /// Switch the source / definition preview on a single-preview layout.
    PreviewTab,
    InspectFind,
    InspectLine,
    InspectNext(i32),
    InspectHorizontal(i32),
    InspectStart,
    ExpandPreview,
    Page(i32),
    Top,
    Bottom,
    /// Scroll the focused text panel (context, detail) by `n` lines.
    Scroll(i32),
    /// Grow (positive) or shrink the left column, in percent.
    Resize(i16),
    Toggle,
    ToggleAll,
    /// Edit the mode's input: the query, a new name, a destination, a template.
    Input(char),
    InputEdit(crate::input::EditCommand),
    Backspace,
    Clear,
    /// The one thing the status bar names for `⏎`.
    Enter,
    /// Back one step: close an overlay, leave a mode for search.
    Back,
    /// Enter a mode from the search row under the cursor.
    Rename,
    MoveFile,
    MoveSymbol,
    Rewrite,
    History,
    OpenMenu(MenuTarget),
    MenuChoose,
    /// Remove the restriction edited by the open picker.
    MenuClear,
    Undo,
    Help,
    /// Show the full diff for the cursor's file in the detail panel.
    Diff,
    /// Switch between the compact and detailed row layouts.
    View,
    /// Open `$EDITOR` at the cursor's line.
    Edit,
    /// Follow a result, or pick an identifier in the focused source pane.
    Follow,
    Workspace,
    Refresh,
    Recover,
    BrowseBack,
    BrowseForward,
    Places,
    PlacesTab,
    ForgetSearch,
    ResetLayout,
}

/// Work for the engine, run off the UI thread — except `Edit`, which the
/// event loop runs itself since it owns the terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    WorkspaceFiles {
        generation: u64,
    },
    Search {
        generation: u64,
        query: Query,
        scope: vvv_engine::SearchScope,
    },
    /// Ask the engine a read-only question: the search's subject answered
    /// with its references, impact, definition or deps. A mutation is
    /// planned through `Plan`/`Commit`, never here.
    Query {
        generation: u64,
        request: Request,
    },
    Preview {
        path: RelPath,
    },
    /// Resolve an exact occurrence and capture its complete definition preview.
    Definition {
        ticket: u64,
        query: vvv_engine::NavigationQuery,
    },
    /// Explicit navigation is never coalesced with row previews.
    Follow {
        ticket: u64,
        query: vvv_engine::NavigationQuery,
    },
    /// Plan an intent and answer with everything a mode shows about it.
    /// `debounce` when typing drives it (the newest wins after a pause);
    /// the plan that opens a mode goes at once.
    Plan {
        generation: u64,
        intent: Intent,
        debounce: bool,
    },
    /// Plan and write in one step, so the fingerprint check runs against
    /// the tree the user just looked at.
    Commit {
        intent: Intent,
    },
    History,
    Undo,
    Edit {
        path: RelPath,
        line: u32,
    },
    /// The tree changed behind the engine's back (the editor ran): its next
    /// command must look, however recently it walked.
    Touched,
}

/// The engine's answer to an [`Effect`].
#[derive(Debug, Clone)]
pub enum Event {
    WorkspaceFiles {
        generation: u64,
        paths: Vec<RelPath>,
    },
    Paste(String),
    Pointer(crate::modes::search::files::Pointer),
    Viewport {
        width: u16,
        height: u16,
    },
    DefinitionResolved {
        ticket: u64,
        query: vvv_engine::NavigationQuery,
        reply: Result<vvv_engine::NavigationReply, vvv_engine::Failure>,
    },
    Followed {
        ticket: u64,
        query: vvv_engine::NavigationQuery,
        reply: Result<vvv_engine::NavigationReply, vvv_engine::Failure>,
    },
    SourcesChanged,
    Searched {
        generation: u64,
        matches: Vec<Match>,
        /// Languages whose grammar could not run the query.
        skipped: Vec<Skipped>,
    },
    Answered {
        generation: u64,
        answer: Box<Answer>,
    },
    Previewed {
        path: RelPath,
        text: String,
        highlights: Vec<Highlight>,
        symbols: Vec<vvv_engine::Symbol>,
        identifiers: Vec<vvv_engine::SourceAnchor>,
    },
    Planned {
        generation: u64,
        planned: Planned,
    },
    /// A plan request that could not be met: a destination that is not
    /// addressable, a template that does not expand. Shown where the input
    /// is, not as a failure of the session.
    PlanFailed {
        generation: u64,
        problem: Box<crate::problem::Problem>,
    },
    /// Written as history entry `id`.
    Applied {
        id: u64,
        intent: Intent,
        /// What the apply produced, as the report the picker can show.
        report: Document,
    },
    History(Vec<HistoryEntry>),
    Undone(HistoryEntry),
    Failed {
        generation: Option<u64>,
        problem: Box<crate::problem::Problem>,
    },
}

/// A planned intent with what its mode shows about it.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // one per plan; never stored in bulk
pub enum Planned {
    Rename {
        declarations: Vec<Match>,
        occurrences: Vec<Occurrence>,
        /// What the engine's default selection edits; the ticks start there.
        files: Vec<FileChange>,
    },
    Move {
        intent: Intent,
        respellings: Vec<Respelling>,
        notices: Vec<Notice>,
        files: Vec<FileChange>,
    },
    Rewrite {
        /// The plan's files, each with its edits and diff.
        files: Vec<FileChange>,
    },
}
