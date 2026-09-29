//! Shared document construction and delegation to capability-owned composition.
use super::lines as l;
use super::{Block, MoveCounts, Note};
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{Files, Mark, Plural};
use crate::{Answer, Confidence, Failure, FileChange, Match, Notice, Occurrence, Respelling};

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
            Answer::DiscardPlan(r) => Self::plan_review(r),
            Answer::ApplyPlan(r) => Self::plan_receipt(r),
            Answer::ValidatePlan(r) => Self::validation(r),
            Answer::InspectPlan(r) => Self::plan_reply(r),
            Answer::PrepareRename(r) | Answer::PrepareRewrite(r) | Answer::PrepareMove(r) => {
                Self::plan_reply(r)
            }
            Answer::ReviewPlan(r) => Self::plan_page(r),
            #[cfg(feature = "schema")]
            Answer::Schema(r) => Self::schema(r),
            Answer::SearchPage(r) => Self::search_page(r),
            Answer::ContextPage(r) => Self::context_page(r),
            Answer::Continue(r) => Self::page(r),
            Answer::Expand(r) => Self::expansion(r),
            Answer::Discover(r) => Self::discovery(r),
            Answer::Context(r) => Self::context(r),
            Answer::Relationships(r) => Self::relationships(r),
            Answer::Resolve(r) => Self::resolution(r),
            Answer::Navigate(r) => Self::navigation(r),
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
}
