//! A command's output as data: a list of [`Block`]s per stream, which a
//! [`View`] lays out. The vocabulary is deliberately small; the richness
//! lives in the [`Line`]s a block carries.
//!
//! The composition lives on [`Document`] itself — `Document::search`,
//! `Document::rename`, `Document::moved` — so nothing here is a free function.

pub(crate) mod lines;
mod view;

pub use view::{Detailed, Presentation, View};

use vvv_core::RelPath;

use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{Files, IntentLine, Mark, Plural};
use crate::protocol::{
    Answer, Batch, BatchIntent, Consumer, Dep, Explanation, Exposed, Failure, History, ImportSite,
    Importer, Intent, OutlineItem, References, Rewrite, Site, Undo, Unreferenced,
};
use crate::{Confidence, FileChange, HistoryEntry, Match, Notice, Occurrence, Respelling};

use lines as l;

/// The presentation choices a view needs beyond the answer.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Expand what is collapsed by default.
    pub verbose: bool,
    /// Show every file's full patch, not only structural edits.
    pub diff: bool,
}

/// A command's output, one sequence of blocks per stream: results, then
/// notes (summaries, hints, warnings).
#[derive(Debug, Default, Clone)]
pub struct Document {
    body: Vec<Block>,
    notes: Vec<Block>,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append bare lines to the result stream.
    pub fn body(&mut self, lines: impl IntoIterator<Item = Line>) {
        self.body.extend(lines.into_iter().map(Block::Line));
    }

    /// Append bare lines to the note stream.
    pub fn notes(&mut self, lines: impl IntoIterator<Item = Line>) {
        self.notes.extend(lines.into_iter().map(Block::Line));
    }

    /// Append one structured block to the result stream.
    pub fn block_body(&mut self, block: Block) {
        self.body.push(block);
    }

    /// Append one structured block to the note stream.
    pub fn block_note(&mut self, block: Block) {
        self.notes.push(block);
    }

    /// Both streams, for the view.
    pub fn parts(&self) -> (&[Block], &[Block]) {
        (&self.body, &self.notes)
    }

    // ------------------------------------------------------------------ entry

    /// Compose one answer into the blocks its command prints.
    pub fn of(answer: &Answer) -> Self {
        match answer {
            Answer::Search(r) => Self::search(&r.matches, &r.skipped),
            Answer::Outline(r) => Self::outline(r),
            Answer::References(r) => Self::references(r),
            Answer::Where(r) => Self::locations(r),
            Answer::Deps(r) => Self::deps(r),
            Answer::Explain(r) => Self::explain(r),
            Answer::Surface(r) => Self::surface(r),
            Answer::Impact(r) => Self::impact(r),
            Answer::Dead(r) => Self::dead(r),
            Answer::Imports(r) => Self::imports(r),
            // The file answer is the picker's; no CLI command asks for one.
            Answer::File(_) => Self::new(),
            Answer::Rewrite(r) => Self::rewrite(r),
            Answer::Rename(r) => Self::rename(r),
            Answer::Move(r) => Self::move_file(r),
            Answer::MoveSymbol(r) => Self::move_symbol(r),
            Answer::Batch(r) => Self::batch(r),
            Answer::Undo(r) => Self::undo(r),
            Answer::History(r) => Self::history(r),
        }
    }

    /// Compose a failure as `✗ message`; interfaces present its hints.
    pub fn error(failure: &Failure) -> Self {
        let mut report = Self::new();
        report.notes([Line::of(Role::Error, "✗ ").and(Role::Plain, failure.message.clone())]);
        report
    }

    // ---------------------------------------------------------------- helpers

    /// A title line, then a blank.
    pub(crate) fn title(&mut self, text: impl std::fmt::Display) {
        self.block_body(Block::Title(text.to_string()));
    }

    /// `warning: …` on the note stream.
    pub(crate) fn warning(&mut self, line: Line) {
        self.block_note(Block::Note(Note::Warning(line)));
    }

    /// `path`: the file the command opened.
    pub(crate) fn file_header(&mut self, path: &std::path::Path) {
        self.body([Line::of(Role::Path, path.display().to_string())]);
    }

    /// The body of a move preview: counts, the `→` rows, the `!` rows, and a
    /// `±` hunk for every file whose edits are more than re-spelled paths.
    /// Answers how many are structural.
    pub(crate) fn moved(
        &mut self,
        state: crate::MutationState,
        files: &[FileChange],
        respellings: &[Respelling],
        notices: &[Notice],
    ) -> usize {
        let structural = files
            .iter()
            .filter(|f| l::Diff::is_structural(f, respellings))
            .count();
        self.block_body(Block::Moved {
            state,
            files: files.to_vec(),
            respellings: respellings.to_vec(),
            notices: notices.to_vec(),
        });
        structural
    }

    /// `→ 12  ± 2  ! 1   12 files` on the note stream, then the verdict.
    pub(crate) fn moved_summary(&mut self, state: crate::MutationState, counts: MoveCounts) {
        let MoveCounts {
            respellings,
            structural,
            notices,
            files,
        } = counts;
        let mut counts: Vec<Line> = Vec::new();
        if respellings > 0 {
            counts.push(
                Line::mark(Mark::Import)
                    .and(Role::Plain, " ")
                    .and(Role::Plain, respellings.to_string()),
            );
        }
        if structural > 0 {
            counts.push(
                Line::mark(Mark::Structure)
                    .and(Role::Plain, " ")
                    .and(Role::Plain, structural.to_string()),
            );
        }
        if notices > 0 {
            counts.push(
                Line::mark(Mark::ByHand)
                    .and(Role::Plain, " ")
                    .and(Role::Plain, notices.to_string()),
            );
        }
        let counts = Self::join(counts, "  ")
            .and(Role::Plain, "   ")
            .and(Role::Dim, Plural(files, "file").to_string());
        if state.is_applied() {
            self.receipt(state, counts);
            return;
        }
        self.notes([counts]);
    }

    /// The last line of a mutating command: `counts` then, applied, the
    /// history entry it made (`✓ #3`).
    pub(crate) fn receipt(&mut self, state: crate::MutationState, counts: Line) {
        match state {
            crate::MutationState::Applied { history_id } => self.notes([Line::mark(Mark::Safe)
                .and(Role::Plain, " ")
                .and(Role::Strong, format!("#{history_id}"))
                .and(Role::Plain, "   ")
                .and_line(counts)]),
            crate::MutationState::Preview => self.notes([counts]),
        }
    }

    /// The `●` lines a rename or references answer opens with.
    pub(crate) fn declarations(&mut self, declarations: &[Match]) {
        self.block_body(Block::Declarations(declarations.to_vec()));
        self.block_body(Block::Blank);
    }

    /// `✓ 26  ? 7  ✗ 0   4 files`, for a caller to end or continue.
    pub(crate) fn verdict_counts(occurrences: &[Occurrence]) -> Line {
        let count = |c: Confidence| occurrences.iter().filter(|o| o.confidence == c).count();
        Line::counts(&[
            (
                Mark::from(Confidence::Resolved),
                count(Confidence::Resolved),
            ),
            (
                Mark::from(Confidence::Unresolved),
                count(Confidence::Unresolved),
            ),
            (Mark::from(Confidence::Other), count(Confidence::Other)),
        ])
        .and(
            Role::Dim,
            format!(
                "   {}",
                Files::among(occurrences.iter().map(|o| o.m.path.as_path()))
            ),
        )
    }

    pub(crate) fn edits_in(files: &[FileChange]) -> usize {
        files.iter().map(|f| f.edits.len()).sum()
    }

    /// Lines joined by `separator`.
    pub(crate) fn join(parts: Vec<Line>, separator: &str) -> Line {
        let mut line = Line::new();
        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                line = line.and(Role::Plain, separator.to_owned());
            }
            line = line.and_line(part);
        }
        line
    }

    // ------------------------------------------------------------ references

    fn references(result: &References) -> Self {
        let mut report = Self::new();
        report.declarations(&result.declarations);
        report.block_body(Block::Verdicts {
            occurrences: result.occurrences.clone(),
            plan: None,
        });
        report.block_note(Block::Summary(Self::verdict_counts(&result.occurrences)));
        report
    }

    // --------------------------------------------------------------- rewrite

    fn rewrite(result: &Rewrite) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Rewrite(result.intent.clone())));
        report.block_body(Block::Changes {
            state: result.state,
            files: result.files.clone(),
        });
        let edits = Self::edits_in(&result.files);
        if edits == 0 {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no matches"),
            ));
            return report;
        }
        report.receipt(
            result.state,
            Line::mark(Mark::Rewrite)
                .and(Role::Plain, format!(" {edits}   "))
                .and(Role::Dim, Plural(result.files.len(), "file").to_string()),
        );
        report
    }

    fn batch(result: &Batch) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Batch(BatchIntent {
            intents: result.intents.clone(),
        })));
        report.block_body(Block::Batch(result.intents.clone()));
        report.block_body(Block::Blank);
        // Steps compose, so no edit is one re-spelled path: every file is a hunk.
        let structural = report.moved(result.state, &result.files, &[], &result.notices);
        report.moved_summary(
            result.state,
            MoveCounts {
                respellings: 0,
                structural,
                notices: result.notices.len(),
                files: result.files.len(),
            },
        );
        report
    }

    // ------------------------------------------------------------------ undo

    fn undo(result: &Undo) -> Self {
        let mut report = Self::new();
        report.title(format!(
            "{} #{}  {}",
            Mark::Undo.glyph(),
            result.undone.id,
            IntentLine(&result.undone.intent)
        ));
        report.block_body(Block::Undo {
            moves: result.moves_reverted.clone(),
            restored: result.restored.clone(),
        });
        report.block_note(Block::Summary(
            Line::mark(Mark::Undo)
                .and(Role::Plain, " ")
                .and(Role::Plain, format!("#{}   ", result.undone.id))
                .and(Role::Dim, Plural(result.restored.len(), "file").to_string()),
        ));
        report
    }

    // --------------------------------------------------------------- history

    fn history(result: &History) -> Self {
        let mut report = Self::new();
        if result.entries.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no history"),
            ));
            return report;
        }
        let last = result.entries.len() - 1;
        report.block_body(Block::History(result.entries.clone()));
        report.block_note(Block::Summary(
            Line::of(
                Role::Plain,
                Plural(result.entries.len(), "entry").to_string(),
            )
            .and(Role::Plain, "   ")
            .and_line(Line::mark(Mark::Undo))
            .and(Role::Plain, format!(" #{}", result.entries[last].id)),
        ));
        report
    }
}

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

/// A row: the line to draw, and the source it stands for.
#[derive(Debug, Clone)]
pub struct Row {
    pub line: Line,
    /// Where the row is, when an interface can act on it.
    pub source: Option<Source>,
}

/// The source a row stands for: the file and the line in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub path: RelPath,
    pub line: u32,
}

impl Row {
    /// A row no renderer can act on.
    pub fn new(line: Line) -> Self {
        Self { line, source: None }
    }

    /// A row that stands for a source.
    pub fn at(line: Line, path: RelPath, at: u32) -> Self {
        Self {
            line,
            source: Some(Source { path, line: at }),
        }
    }

    /// Wrap plain lines as rows no renderer can act on.
    pub fn wrap(lines: Vec<Line>) -> Vec<Self> {
        lines.into_iter().map(Self::new).collect()
    }
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
