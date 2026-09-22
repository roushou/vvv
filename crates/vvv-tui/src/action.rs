//! What the user can do ([`Action`]), what the model wants done ([`Effect`]),
//! and what comes back from the engine ([`Event`]). All plain data.

use vvv_engine::RelPath;

use vvv_engine::HistoryEntry;
use vvv_engine::protocol::FileChange;
use vvv_engine::report::Document;
use vvv_engine::{
    Address, Highlight, Intent, Match, Notice, Occurrence, Query, Respelling, Skipped,
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
    Undo,
    Help,
    /// Show the full diff for the cursor's file in the detail panel.
    Diff,
    /// Switch between the compact and detailed row layouts.
    View,
    /// Open `$EDITOR` at the cursor's line.
    Edit,
}

/// Work for the engine, run off the UI thread — except `Edit`, which the
/// event loop runs itself since it owns the terminal.
#[derive(Debug, Clone)]
pub enum Effect {
    Search {
        generation: u64,
        query: Query,
    },
    Preview {
        path: RelPath,
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
    Searched {
        generation: u64,
        matches: Vec<Match>,
        /// Languages whose grammar could not run the query.
        skipped: Vec<Skipped>,
    },
    Previewed {
        path: RelPath,
        text: String,
        highlights: Vec<Highlight>,
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
        message: String,
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
    Failed(String),
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
        addresses: Option<(Address, Address)>,
        respellings: Vec<Respelling>,
        notices: Vec<Notice>,
        files: Vec<FileChange>,
    },
    Rewrite {
        /// One edit per match; its replacement is the `after` text.
        files: Vec<FileChange>,
    },
}
