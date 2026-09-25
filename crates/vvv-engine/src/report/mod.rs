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

use crate::SymbolKind;
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{Files, IntentLine, Mark, Plural};
use crate::protocol::{
    Answer, Batch, BatchIntent, Consumer, Dead, Dep, Deps, Explanation, Exposed, Failure, History,
    Impact, ImportSite, Importer, ImportsReport, Intent, Locations, Move, MoveIntent, MoveSymbol,
    Outline, OutlineItem, References, Rename, Rewrite, Site, Skipped, Surface, Undo, Unreferenced,
};
use crate::{Address, Confidence, FileChange, HistoryEntry, Match, Notice, Occurrence, Respelling};

use lines as l;

/// The flags a view needs beyond the answer.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Expand what is collapsed by default (`-v`).
    pub verbose: bool,
    /// Print every file's full patch, not only structural edits (`--diff`).
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
    pub fn of(answer: &Answer, options: Options) -> Self {
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
            Answer::Rename(r) => Self::rename(r, options),
            Answer::Move(r) => Self::move_file(r, options),
            Answer::MoveSymbol(r) => Self::move_symbol(r, options),
            Answer::Batch(r) => Self::batch(r, options),
            Answer::Undo(r) => Self::undo(r),
            Answer::History(r) => Self::history(r),
        }
    }

    /// Compose a failure: `✗ message`, then each hint line.
    pub fn error(failure: &Failure) -> Self {
        let mut report = Self::new();
        report.notes([Line::of(Role::Error, "✗ ").and(Role::Plain, failure.message.clone())]);
        for line in failure.hint.iter().flat_map(|h| h.lines()) {
            report.hint(line);
        }
        report
    }

    // ---------------------------------------------------------------- helpers

    /// A title line, then a blank.
    fn title(&mut self, text: impl std::fmt::Display) {
        self.block_body(Block::Title(text.to_string()));
    }

    /// `hint: …` on the note stream.
    fn hint(&mut self, text: impl std::fmt::Display) {
        self.block_note(Block::Note(Note::Hint(text.to_string())));
    }

    /// `warning: …` on the note stream.
    fn warning(&mut self, line: Line) {
        self.block_note(Block::Note(Note::Warning(line)));
    }

    /// `◆ module   path`, or the path alone when the language has no addresses.
    fn module_header(&mut self, module: Option<&Address>, path: &std::path::Path) {
        match module.filter(|m| !m.package().as_str().is_empty()) {
            Some(module) => self.body([Line::mark(Mark::Address)
                .and(Role::Plain, " ")
                .and(Role::Address, module.to_string())
                .and(Role::Plain, "   ")
                .and(Role::Path, path.display().to_string())]),
            None => self.body([Line::of(Role::Path, path.display().to_string())]),
        }
    }

    /// The body of a move preview: counts, the `→` rows, the `!` rows, and a
    /// `±` hunk for every file whose edits are more than re-spelled paths (or
    /// every file, with `--diff`). Answers how many are structural.
    fn moved(
        &mut self,
        files: &[FileChange],
        respellings: &[Respelling],
        notices: &[Notice],
    ) -> usize {
        let structural = files
            .iter()
            .filter(|f| l::Diff::is_structural(f, respellings))
            .count();
        self.block_body(Block::Moved {
            files: files.to_vec(),
            respellings: respellings.to_vec(),
            notices: notices.to_vec(),
        });
        structural
    }

    /// `→ 12  ± 2  ! 1   12 files` on the note stream, then the verdict.
    fn moved_summary(
        &mut self,
        applied: bool,
        history_id: Option<u64>,
        counts: MoveCounts,
        diff: bool,
    ) {
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
        if applied {
            self.receipt(true, history_id, counts);
            return;
        }
        self.notes([counts]);
        let mut flags = vec!["--apply to write"];
        if !diff {
            flags.push("--diff for the full patch");
        }
        self.hint(flags.join(" · "));
    }

    /// The last line of a mutating command: `counts` then, applied, the
    /// history entry it made (`✓ #3`); otherwise the flag to go on with.
    fn receipt(&mut self, applied: bool, history_id: Option<u64>, counts: Line) {
        match (applied, history_id) {
            (true, Some(id)) => self.notes([Line::mark(Mark::Safe)
                .and(Role::Plain, " ")
                .and(Role::Strong, format!("#{id}"))
                .and(Role::Plain, "   ")
                .and_line(counts)]),
            (true, None) => self.notes([Line::mark(Mark::Safe)
                .and(Role::Plain, "   ")
                .and_line(counts)]),
            (false, _) => {
                self.notes([counts]);
                self.hint("--apply to write");
            }
        }
    }

    /// `◆ old → new`, skipped for a nameless package.
    fn addresses(&mut self, from: &Address, to: &Address) {
        if from.package().as_str().is_empty() {
            return;
        }
        self.body([Line::mark(Mark::Address)
            .and(Role::Plain, " ")
            .and(Role::Address, from.to_string())
            .and(Role::Plain, " ")
            .and_line(Line::mark(Mark::Import))
            .and(Role::Plain, " ")
            .and(Role::Address, to.to_string())]);
    }

    /// The `●` lines a rename or references answer opens with.
    fn declarations(&mut self, declarations: &[Match]) {
        self.body(declarations.iter().map(|m| l::Declaration::new(m).line()));
        self.block_body(Block::Blank);
    }

    /// `✓ 26  ? 7  ✗ 0   4 files`, for a caller to end or continue.
    fn verdict_counts(occurrences: &[Occurrence]) -> Line {
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

    fn edits_in(files: &[FileChange]) -> usize {
        files.iter().map(|f| f.edits.len()).sum()
    }

    /// Lines joined by `separator`.
    fn join(parts: Vec<Line>, separator: &str) -> Line {
        let mut line = Line::new();
        for (i, part) in parts.into_iter().enumerate() {
            if i > 0 {
                line = line.and(Role::Plain, separator.to_owned());
            }
            line = line.and_line(part);
        }
        line
    }

    // ---------------------------------------------------------------- search

    pub fn search(matches: &[Match], skipped: &[Skipped]) -> Self {
        let mut report = Self::new();
        report.block_body(Block::Matches(matches.to_vec()));
        if matches.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no matches"),
            ));
        } else {
            report.block_note(Block::Summary(Self::search_summary(matches)));
        }
        for skipped in skipped {
            report.warning(l::SkippedLine::new(skipped).line());
        }
        if !skipped.is_empty() {
            report.hint(
                "a pattern is written in one language; pass --lang to search that one only (the matches above are complete for the others)",
            );
        }
        report
    }

    /// `● 1  ○ 59   13 files`: what the search found.
    fn search_summary(matches: &[Match]) -> Line {
        let (declarations, uses) = l::Sections::split(matches);
        let mut counts: Vec<Line> = Vec::new();
        if !declarations.is_empty() {
            counts.push(
                Line::mark(Mark::Declaration)
                    .and(Role::Plain, " ")
                    .and(Role::Plain, declarations.len().to_string()),
            );
        }
        if !uses.is_empty() {
            counts.push(
                Line::of(Role::Dim, "○")
                    .and(Role::Plain, " ")
                    .and(Role::Plain, uses.len().to_string()),
            );
        }
        Self::join(counts, "  ").and(Role::Plain, "  ").and(
            Role::Dim,
            Files::among(matches.iter().map(|m| m.path.as_path())).to_string(),
        )
    }

    // --------------------------------------------------------------- outline

    fn outline(result: &Outline) -> Self {
        let mut report = Self::new();
        report.module_header(result.module.as_ref(), &result.path);
        if result.items.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no declarations"),
            ));
            return report;
        }
        report.block_body(Block::Blank);
        report.block_body(Block::Outline(result.items.clone()));
        report.block_note(Block::Summary(Self::outline_summary(result)));
        report
    }

    /// `● 14   pub 9  pub(crate) 1  · 4`: how many of each modifier.
    fn outline_summary(result: &Outline) -> Line {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for item in &result.items {
            if item.symbol.kind == SymbolKind::Impl {
                continue;
            }
            let key = item.symbol.modifier().unwrap_or("·").to_owned();
            match counts.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += 1,
                None => counts.push((key, 1)),
            }
        }
        counts.sort_by_key(|(k, _)| k == "·");
        let counts: Vec<Line> = counts
            .iter()
            .map(|(k, n)| {
                Line::of(Role::Dim, k.clone())
                    .and(Role::Plain, " ")
                    .and(Role::Plain, n.to_string())
            })
            .collect();
        Line::mark(Mark::Declaration)
            .and(Role::Plain, " ")
            .and(Role::Plain, result.items.len().to_string())
            .and(Role::Plain, "   ")
            .and_line(Self::join(counts, "  "))
    }

    // ------------------------------------------------------------ references

    fn references(result: &References) -> Self {
        let mut report = Self::new();
        report.declarations(&result.declarations);
        report.block_body(Block::Verdicts {
            occurrences: result.occurrences.clone(),
            files: None,
        });
        report.block_note(Block::Summary(Self::verdict_counts(&result.occurrences)));
        report
    }

    // ------------------------------------------------------------- locations

    fn locations(result: &Locations) -> Self {
        let mut report = Self::new();
        report.block_body(Block::Sites(result.sites.clone()));
        report.block_note(Block::Summary(
            Line::mark(Mark::Declaration)
                .and(Role::Plain, " ")
                .and(Role::Plain, result.sites.len().to_string()),
        ));
        if result.sites.iter().all(|s| s.import.is_none())
            && result.sites.iter().any(|s| s.address.is_some())
        {
            report.hint("--from <file> for the import to write there");
        }
        report
    }

    // ------------------------------------------------------------------ deps

    fn deps(result: &Deps) -> Self {
        let mut report = Self::new();
        report.module_header(result.module.as_ref(), &result.path);
        report.block_body(Block::Blank);
        let outgoing = l::DepGroups::statements(&result.imports);
        report.body([Line::mark(Mark::Import)
            .and(Role::Plain, " ")
            .and(Role::Strong, outgoing.to_string())]);
        if result.imports.is_empty() {
            report.body([Line::of(Role::Plain, "  ").and_line(Line::mark(Mark::Nothing))]);
        }
        let own = result
            .module
            .as_ref()
            .map(|m| m.package().as_str().to_owned());
        report.block_body(Block::DepGroups {
            imports: result.imports.clone(),
            own,
        });
        report.block_body(Block::Blank);
        let incoming = l::ImporterRows::sites(&result.importers).len();
        report.body([Line::mark(Mark::ImportedBy)
            .and(Role::Plain, " ")
            .and(Role::Strong, incoming.to_string())
            .and(Role::Plain, "   ")
            .and(
                Role::Dim,
                Files::among(result.importers.iter().map(|i| i.path.as_path())).to_string(),
            )]);
        if result.importers.is_empty() {
            report.body([Line::of(Role::Plain, "  ").and_line(Line::mark(Mark::Nothing))]);
        }
        report.block_body(Block::Importers(result.importers.clone()));
        report.block_note(Block::Summary(Self::deps_summary(outgoing, incoming)));
        for skipped in &result.skipped {
            report.warning(l::SkippedLine::new(skipped).line());
        }
        report
    }

    fn deps_summary(outgoing: usize, incoming: usize) -> Line {
        Line::mark(Mark::Import)
            .and(Role::Plain, format!(" {outgoing}  "))
            .and_line(Line::mark(Mark::ImportedBy))
            .and(Role::Plain, format!(" {incoming}"))
    }

    // --------------------------------------------------------------- explain

    fn explain(result: &Explanation) -> Self {
        let mut report = Self::new();
        report.block_body(Block::Explanation(Box::new(result.clone())));
        report
    }

    // --------------------------------------------------------------- surface

    fn surface(result: &Surface) -> Self {
        let mut report = Self::new();
        let scope = result
            .package
            .as_ref()
            .map_or_else(|| "every package".to_owned(), ToString::to_string);
        report.title(format!("surface {scope}"));
        if result.items.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " nothing exposed"),
            ));
            return report;
        }
        report.block_body(Block::Exposed(result.items.clone()));
        report.block_note(Block::Summary(Self::surface_summary(result)));
        report
    }

    fn surface_summary(result: &Surface) -> Line {
        let importers: usize = result.items.iter().map(|i| i.importers).sum();
        Line::mark(Mark::Declaration)
            .and(Role::Plain, " ")
            .and(Role::Strong, result.items.len().to_string())
            .and(Role::Plain, "   ")
            .and_line(Line::mark(Mark::ImportedBy))
            .and(Role::Plain, " ")
            .and(Role::Dim, format!("{importers} imports"))
    }

    // ---------------------------------------------------------------- impact

    fn impact(result: &Impact) -> Self {
        let mut report = Self::new();
        report.body([Line::of(Role::Title, format!("impact {}", result.name))
            .and(Role::Plain, "   ")
            .and(Role::Address, format!("◆ {}", result.address))]);
        if result.consumers.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no module imports it"),
            ));
            return report;
        }
        report.block_body(Block::Consumers(result.consumers.clone()));
        report.block_note(Block::Summary(
            Line::mark(Mark::ImportedBy)
                .and(Role::Plain, " ")
                .and(Role::Strong, result.consumers.len().to_string())
                .and(Role::Plain, "   ")
                .and(
                    Role::Dim,
                    Files::among(result.consumers.iter().map(|c| c.path.as_path())).to_string(),
                ),
        ));
        report
    }

    // ------------------------------------------------------------------ dead

    fn dead(result: &Dead) -> Self {
        let mut report = Self::new();
        report.title("dead");
        if result.items.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " everything is referred to"),
            ));
            return report;
        }
        report.block_body(Block::Dead(result.items.clone()));
        report.block_note(Block::Summary(Self::dead_summary(result)));
        report
    }

    fn dead_summary(result: &Dead) -> Line {
        let unsure = result.items.iter().filter(|i| i.unsure > 0).count();
        Line::mark(Mark::Declaration)
            .and(Role::Plain, " ")
            .and(Role::Strong, result.items.len().to_string())
            .and(Role::Plain, "   ")
            .and_line(Line::mark(Mark::Unverified))
            .and(Role::Plain, " ")
            .and(
                Role::Dim,
                format!("{unsure} with tokens that might be uses"),
            )
    }

    // --------------------------------------------------------------- imports

    fn imports(result: &ImportsReport) -> Self {
        let mut report = Self::new();
        let scope = result.path.as_ref().map_or_else(
            || "every file".to_owned(),
            |path| path.display().to_string(),
        );
        report.title(format!("imports {scope}"));
        let sections: [(Mark, &str, &[ImportSite]); 3] = [
            (Mark::Nothing, "unresolved", &result.unresolved),
            (Mark::ByHand, "unused", &result.unused),
            (Mark::ByHand, "redundant", &result.redundant),
        ];
        let total: usize = sections.iter().map(|(_, _, s)| s.len()).sum();
        if total == 0 {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " nothing to look at"),
            ));
            return report;
        }
        report.block_body(Block::Imports {
            unresolved: result.unresolved.clone(),
            unused: result.unused.clone(),
            redundant: result.redundant.clone(),
        });
        report.block_note(Block::Summary(Self::imports_summary(
            &sections,
            result.unplaced.len(),
        )));
        report
    }

    fn imports_summary(sections: &[(Mark, &str, &[ImportSite])], unplaced: usize) -> Line {
        let mut counts: Vec<Line> = sections
            .iter()
            .filter(|(_, _, s)| !s.is_empty())
            .map(|(mark, word, s)| {
                Line::mark(*mark)
                    .and(Role::Plain, format!(" {word} "))
                    .and(Role::Plain, s.len().to_string())
            })
            .collect();
        if unplaced > 0 {
            counts.push(
                Line::mark(Mark::Nothing)
                    .and(Role::Plain, " ")
                    .and(Role::Dim, "not placed")
                    .and(Role::Plain, " ")
                    .and(Role::Dim, unplaced.to_string()),
            );
        }
        Self::join(counts, "   ")
    }

    // --------------------------------------------------------------- rewrite

    fn rewrite(result: &Rewrite) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Rewrite(result.intent.clone())));
        report.block_body(Block::Changes(result.files.clone()));
        let edits = Self::edits_in(&result.files);
        if edits == 0 {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no matches"),
            ));
            return report;
        }
        report.receipt(
            result.applied,
            result.history_id,
            Line::mark(Mark::Rewrite)
                .and(Role::Plain, format!(" {edits}   "))
                .and(Role::Dim, Plural(result.files.len(), "file").to_string()),
        );
        report
    }

    // ---------------------------------------------------------------- rename

    fn rename(result: &Rename, options: Options) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Rename(result.intent.clone())));
        report.declarations(&result.declarations);
        report.block_body(Block::Verdicts {
            occurrences: result.occurrences.clone(),
            files: Some(result.files.clone()),
        });
        // The plan's patch, when asked for: a rename's rows are the verdicts,
        // so unlike a rewrite its diff is not the default view.
        if options.diff {
            report.block_body(Block::Changes(result.files.clone()));
        }
        let strip = Self::verdict_counts(&result.occurrences);
        let edits = Self::edits_in(&result.files);
        let plan = Line::of(
            Role::Dim,
            format!(
                "→ {} in {}",
                Plural(edits, "occurrence"),
                Plural(result.files.len(), "file")
            ),
        );
        if result.applied {
            report.block_note(Block::Summary(strip));
            report.receipt(true, result.history_id, plan);
            return report;
        }
        report.block_note(Block::Summary(strip.and(Role::Plain, "   ").and_line(plan)));
        let unsure = l::Verdicts::selection(&result.occurrences, Confidence::Unresolved);
        let mut flags = vec!["--apply to write".to_owned()];
        if !unsure.is_empty() {
            flags.push(format!("--select {unsure} for the ? rows alone"));
        }
        if !options.verbose
            && result
                .occurrences
                .iter()
                .any(|o| o.confidence == Confidence::Resolved)
        {
            flags.push("-v to list the ✓ rows".to_owned());
        }
        report.hint(flags.join(" · "));
        report
    }

    // ----------------------------------------------------------------- moves

    fn move_file(result: &Move, options: Options) -> Self {
        let mut report = Self::new();
        // Normalised paths, not the ones typed: what history will show.
        report.title(IntentLine(&Intent::Move(MoveIntent::new(
            &result.from,
            &result.to,
        ))));
        if let (Some(from), Some(to)) = (&result.from_address, &result.to_address) {
            report.addresses(from, to);
        }
        report.block_body(Block::Blank);
        let structural = report.moved(&result.files, &result.respellings, &result.notices);
        report.moved_summary(
            result.applied,
            result.history_id,
            MoveCounts {
                respellings: result.respellings.len(),
                structural,
                notices: result.notices.len(),
                files: result.files.len(),
            },
            options.diff,
        );
        report
    }

    fn move_symbol(result: &MoveSymbol, options: Options) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::MoveSymbol(result.intent.clone())));
        report.addresses(&result.from, &result.to);
        report.block_body(Block::Blank);
        let structural = report.moved(&result.files, &result.respellings, &result.notices);
        report.moved_summary(
            result.applied,
            result.history_id,
            MoveCounts {
                respellings: result.respellings.len(),
                structural,
                notices: result.notices.len(),
                files: result.files.len(),
            },
            options.diff,
        );
        report
    }

    fn batch(result: &Batch, options: Options) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Batch(BatchIntent {
            intents: result.intents.clone(),
        })));
        report.block_body(Block::Batch(result.intents.clone()));
        report.block_body(Block::Blank);
        // Steps compose, so no edit is one re-spelled path: every file is a hunk.
        let structural = report.moved(&result.files, &[], &result.notices);
        report.moved_summary(
            result.applied,
            result.history_id,
            MoveCounts {
                respellings: 0,
                structural,
                notices: result.notices.len(),
                files: result.files.len(),
            },
            options.diff,
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
            report
                .hint("--apply writes a plan and records it here; `vvv undo` reverses the newest");
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
    /// Found rows — hits — that a view numbers and groups.
    Matches(Vec<Match>),
    /// Occurrences a view judges: a reference, a rename.
    Verdicts {
        occurrences: Vec<Occurrence>,
        /// The files a plan would change, when there is one, so a view can
        /// mark the rows an edit touches (`±`).
        files: Option<Vec<FileChange>>,
    },
    /// Import sites worth a look: unresolved, then unused, then redundant.
    Imports {
        unresolved: Vec<ImportSite>,
        unused: Vec<ImportSite>,
        redundant: Vec<ImportSite>,
    },
    /// What a file imports, grouped by package; `own` is its own, to mark.
    DepGroups {
        imports: Vec<Dep>,
        own: Option<String>,
    },
    /// Who imports a file.
    Importers(Vec<Importer>),
    /// What is at a position, and how it is reached.
    Explanation(Box<Explanation>),
    /// A file's declarations as a tree, a view aligning the name column.
    Outline(Vec<OutlineItem>),
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
    Changes(Vec<FileChange>),
    /// A move preview: the changes, and what a plan re-spelled or left by hand.
    Moved {
        files: Vec<FileChange>,
        respellings: Vec<Respelling>,
        notices: Vec<Notice>,
    },
    /// A vertical gap.
    Blank,
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
struct MoveCounts {
    respellings: usize,
    structural: usize,
    notices: usize,
    files: usize,
}
