//! Structured facts retained for every view.
use crate::protocol::display::Line;
use crate::{
    Consumer, Dep, Explanation, Exposed, FileChange, HistoryEntry, ImportSite, Importer, Intent,
    Match, Notice, Occurrence, OutlineItem, Respelling, Site, Unreferenced,
};
use vvv_core::RelPath;

/// One piece of a report, named for what it is so a view can place it.
#[derive(Debug, Clone)]
pub enum Block {
    /// The command and its intent: its own line, then a blank.
    Title(String),
    /// A section label.
    Heading(String),
    /// The declarations a reference or rename report opens with.
    Declarations(Vec<Match>),
    /// Found rows — hits — that a view numbers and groups.
    Matches(Vec<Match>),
    /// Occurrences a view judges: a reference, a rename.
    Verdicts {
        occurrences: Vec<Occurrence>,
        /// The files a plan would change, when there is one, so a view can
        /// mark the rows an edit touches (`±`).
        plan: Option<ReferencePlan>,
    },
    /// Import sites worth a look: unresolved, then unused, then redundant.
    Imports {
        unresolved: Vec<ImportSite>,
        unused: Vec<ImportSite>,
        redundant: Vec<ImportSite>,
    },
    /// What a file imports, grouped by package; `own` is its own, to mark.
    DepGroups {
        path: RelPath,
        imports: Vec<Dep>,
        own: Option<String>,
    },
    /// Who imports a file.
    Importers(Vec<Importer>),
    /// What is at a position, and how it is reached.
    Explanation(Box<Explanation>),
    /// A file's declarations as a tree, a view aligning the name column.
    Outline {
        path: RelPath,
        items: Vec<OutlineItem>,
    },
    /// Where a name is declared: the sites `where` found.
    Sites(Vec<Site>),
    SymbolMoveCandidates(Vec<crate::SymbolMoveCandidate>),
    /// Compact resolved or candidate declaration locations.
    Locations(Vec<crate::DefinitionLocation>),
    Relationships(Vec<crate::Relationship>),
    /// Declarations nothing refers to, and the unsure-token count of each.
    Dead(Vec<Unreferenced>),
    /// A package's exposed names, and how many import each.
    Exposed(Vec<Exposed>),
    /// The modules that import a declaration, nearest first.
    Consumers(Vec<Consumer>),
    /// The ledger, oldest first.
    History(Vec<HistoryEntry>),
    /// A batch's steps, in order.
    Batch(Vec<Intent>),
    /// What `undo` reversed: moves, then restored files.
    Undo {
        moves: Vec<(RelPath, RelPath)>,
        restored: Vec<RelPath>,
    },
    /// A bare line.
    Line(Line),
    /// The closing summary: what happened, and what to do next.
    Summary(Line),
    /// A hint or a warning.
    Note(Note),
    /// The change a plan would make: one file's edits, laid out as a diff.
    Changes {
        state: crate::MutationState,
        files: Vec<FileChange>,
    },
    /// A move preview: the changes, and what a plan re-spelled or left by hand.
    Moved {
        state: crate::MutationState,
        files: Vec<FileChange>,
        respellings: Vec<Respelling>,
        notices: Vec<Notice>,
    },
    /// A vertical gap.
    Blank,
}

/// The mutation attached to reference verdicts, retained even when its patch
/// is hidden by a view.
#[derive(Debug, Clone)]
pub struct ReferencePlan {
    pub state: crate::MutationState,
    pub files: Vec<FileChange>,
}

/// A hint or a warning: a view prefixes each.
#[derive(Debug, Clone)]
pub enum Note {
    Hint(String),
    Warning(Line),
}

/// The counts a move's summary line reports.
pub(crate) struct MoveCounts {
    pub(crate) respellings: usize,
    pub(crate) structural: usize,
    pub(crate) notices: usize,
    pub(crate) files: usize,
}
