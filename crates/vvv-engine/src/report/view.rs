//! Seeing a [`Document`]: a [`View`] turns the report into a [`Presentation`],
//! the rows a screen draws. The report carries the facts; a view carries the
//! look, and each interface holds the one it draws.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::lines;
use super::{Block, Document, Note, Options, Row};
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{Ago, Files, IntentLine, Mark, Plural};
use crate::{Confidence, FileChange, ImportSite, Match, Notice, Occurrence, Reason, Respelling};

/// A report laid out for a screen: the result stream and the note stream.
#[derive(Debug, Clone, Default)]
pub struct Presentation {
    /// What the command found; the interface prints it to the result stream.
    pub body: Vec<Row>,
    /// Summaries, hints and warnings; the interface prints it to the note
    /// stream.
    pub notes: Vec<Row>,
}

/// How a report is shown. One per way of seeing it — the terminal's detailed
/// layout, the picker's compact rows — so an interface holds the one it draws.
///
/// [`View::present`] supplies block composition. An interface may add its own
/// advice at this presentation boundary.
pub trait View {
    /// Lay one block out as rows, in `width` columns (`usize::MAX` when a
    /// stream is clipped by the terminal rather than by the view) and with
    /// the flags the command was asked for.
    fn rows(&self, block: &Block, options: Options, width: usize) -> Vec<Row>;

    /// Lay a whole report out, block by block.
    fn present(&self, report: &Document, options: Options, width: usize) -> Presentation {
        let (body, notes) = report.parts();
        Presentation {
            body: body
                .iter()
                .flat_map(|block| self.rows(block, options, width))
                .collect(),
            notes: notes
                .iter()
                .flat_map(|block| self.rows(block, options, width))
                .collect(),
        }
    }

    /// Lay one occurrence out as the picker's list row. The report's own
    /// verdict block lays them out in groups; this is one row of that list,
    /// ticked or not.
    fn occurrence(
        &self,
        occurrence: &Occurrence,
        ordinal: usize,
        ticked: bool,
        width: usize,
    ) -> Row;

    /// Lay one relation row out: an occurrence of the declaration a search
    /// was narrowed to, with no tick — the hub reads, it does not commit.
    fn relation(&self, occurrence: &Occurrence, ordinal: usize, width: usize) -> Row {
        self.occurrence(occurrence, ordinal, false, width)
    }

    /// Lay one re-spelling out as the picker's list row.
    fn respelling(&self, respelling: &Respelling, width: usize) -> Row;

    /// Lay one notice out as the picker's list row.
    fn notice(&self, notice: &Notice, width: usize) -> Row;

    /// Lay one rewrite match out as the picker's list row.
    fn rewrite(&self, m: &Match, ordinal: usize, ticked: bool, width: usize) -> Row;
}

/// The terminal's view: a block per line, notes prefixed, as a command prints.
#[derive(Debug, Default)]
pub struct Detailed;

impl View for Detailed {
    fn rows(&self, block: &Block, options: Options, _width: usize) -> Vec<Row> {
        match block {
            Block::Title(text) => vec![
                Row::new(Line::of(Role::Title, text.clone())),
                Row::new(Line::new()),
            ],
            Block::Heading(text) => vec![Row::new(Line::of(Role::Strong, text.clone()))],
            Block::Declarations(declarations) => declarations
                .iter()
                .map(|m| {
                    Row::at(
                        lines::Declaration::new(m).line(),
                        m.path.clone(),
                        m.start.line,
                    )
                })
                .collect(),
            Block::Matches(matches) => lines::Sections::new(matches).lines(),
            Block::Verdicts { occurrences, plan } => {
                let files = plan.as_ref().map(|plan| plan.files.as_slice());
                let mut rows = VerdictRows::new(occurrences, files, options).rows();
                if options.diff
                    && let Some(plan) = plan
                {
                    rows.extend(self.changes(&plan.files, plan.state));
                }
                rows
            }
            Block::Outline { path, items } => lines::OutlineTree::new(items)
                .lines()
                .into_iter()
                .zip(items)
                .map(|(mut line, item)| {
                    if options.verbose
                        && let Some(reach) = &item.reach
                    {
                        line = line
                            .and(Role::Plain, "   ")
                            .and(Role::Dim, reach.to_string());
                    }
                    Row::at(line, path.clone(), item.start.line)
                })
                .collect(),
            Block::Sites(sites) => sites
                .iter()
                .flat_map(|site| lines::SiteLine::new(site).rows())
                .collect(),
            Block::Dead(items) => items
                .iter()
                .map(|item| {
                    let mut line = lines::PlacedLine::new(&item.declaration).line();
                    if item.unsure > 0 {
                        line = line
                            .and(Role::Plain, "   ")
                            .and_line(Line::mark(Mark::Unverified))
                            .and(Role::Plain, " ")
                            .and(Role::Dim, item.unsure.to_string());
                    }
                    Row::at(
                        line,
                        item.declaration.path.clone(),
                        item.declaration.start.line,
                    )
                })
                .collect(),
            Block::Exposed(items) => items
                .iter()
                .flat_map(|item| {
                    let mut rows = vec![Row::at(
                        lines::PlacedLine::new(&item.declaration)
                            .line()
                            .and(Role::Plain, "   ")
                            .and_line(Line::mark(Mark::ImportedBy))
                            .and(Role::Plain, " ")
                            .and(Role::Strong, item.importers.to_string()),
                        item.declaration.path.clone(),
                        item.declaration.start.line,
                    )];
                    rows.extend(item.via.iter().map(|alias| {
                        Row::new(
                            Line::of(Role::Plain, "  ")
                                .and_line(Line::mark(Mark::ReExport))
                                .and(Role::Plain, " ")
                                .and(Role::Address, alias.to_string()),
                        )
                    }));
                    rows
                })
                .collect(),
            Block::Consumers(consumers) => {
                let mut rows: Vec<Row> = Vec::new();
                let mut depth = 0;
                for consumer in consumers {
                    if consumer.depth != depth {
                        depth = consumer.depth;
                        let at_depth = consumers.iter().filter(|c| c.depth == depth).count();
                        rows.push(Row::new(Line::new()));
                        rows.push(Row::new(
                            Line::mark(Mark::ImportedBy)
                                .and(Role::Plain, " ")
                                .and(Role::Strong, at_depth.to_string())
                                .and(Role::Plain, "   ")
                                .and(Role::Dim, format!("depth {depth}")),
                        ));
                    }
                    let mut line = Line::of(Role::Plain, "  ")
                        .and(Role::Path, consumer.path.display().to_string());
                    if consumer.depth > 1 {
                        line = line.and(Role::Dim, format!("   via {}", consumer.through));
                    }
                    rows.push(Row::new(line));
                }
                rows
            }
            Block::History(entries) => {
                let now = Ago::now();
                let last = entries.len().saturating_sub(1);
                Row::wrap(
                    entries
                        .iter()
                        .enumerate()
                        .map(|(i, item)| {
                            lines::HistoryLine::new(item, now).newest(i == last).line()
                        })
                        .collect(),
                )
            }
            Block::Batch(intents) => Row::wrap(
                intents
                    .iter()
                    .enumerate()
                    .map(|(i, intent)| {
                        Line::of(Role::Plain, "  ")
                            .and(Role::Ordinal, (i + 1).to_string())
                            .and(Role::Plain, "  ")
                            .and(Role::Plain, IntentLine(intent).to_string())
                    })
                    .collect(),
            ),
            Block::Undo { moves, restored } => {
                let mut rows = Row::wrap(
                    moves
                        .iter()
                        .map(|(from, to)| {
                            Line::of(Role::Plain, "  ")
                                .and(Role::Dim, to.display().to_string())
                                .and(Role::Plain, " ")
                                .and_line(Line::mark(Mark::Import))
                                .and(Role::Plain, " ")
                                .and(Role::Path, from.display().to_string())
                        })
                        .collect(),
                );
                rows.extend(restored.iter().map(|path| {
                    Row::new(
                        Line::of(Role::Plain, "  ").and(Role::Path, path.display().to_string()),
                    )
                }));
                rows
            }
            Block::DepGroups { path, imports, own } => {
                lines::DepGroups::new(path, imports, own.as_deref()).rows()
            }
            Block::Importers(importers) => lines::ImporterRows::new(importers).rows(),
            Block::Explanation(result) => {
                let mut rows = vec![Row::at(
                    Line::of(
                        Role::Strong,
                        format!("{}:{}", result.path.display(), result.position.display()),
                    ),
                    result.path.clone(),
                    result.position.line,
                )];
                // On an import: what it spells, and where that really comes from.
                if let Some(dep) = &result.import {
                    rows.push(Row::at(
                        Line::mark(Mark::Import)
                            .and(Role::Plain, " ")
                            .and(Role::Strong, dep.import.path.to_string()),
                        result.path.clone(),
                        dep.start.line,
                    ));
                    let file = dep
                        .file
                        .as_ref()
                        .map(|f| Line::of(Role::Path, f.display().to_string()));
                    match (&dep.origin, file) {
                        (Some(origin), file) => {
                            let mut line = Line::of(Role::Plain, "  ")
                                .and_line(Line::mark(Mark::ReExport))
                                .and(Role::Plain, " ")
                                .and(Role::Address, origin.to_string())
                                .and(Role::Plain, "   ");
                            if let Some(file) = file {
                                line = line.and_line(file);
                            }
                            rows.push(Row::new(line));
                        }
                        (None, Some(file)) => {
                            rows.push(Row::new(Line::of(Role::Plain, "  ").and_line(file)));
                        }
                        (None, None) => {}
                    }
                }
                let Some(symbol) = &result.symbol else {
                    if result.import.is_none() {
                        rows.push(Row::new(
                            Line::mark(Mark::Nothing)
                                .and(Role::Plain, " ")
                                .and(Role::Dim, "not inside a declaration"),
                        ));
                    }
                    return rows;
                };
                if let (Some(declared), Some(line)) = (&result.declared, &result.line) {
                    rows.extend(
                        lines::Caret::new(
                            declared.line,
                            line,
                            declared.column as usize,
                            symbol.name.chars().count(),
                        )
                        .lines()
                        .into_iter()
                        .enumerate()
                        .map(|(index, line)| {
                            if index == 0 {
                                Row::at(line, result.path.clone(), declared.line)
                            } else {
                                Row::new(line)
                            }
                        }),
                    );
                    rows.push(Row::new(Line::new()));
                }
                let mut line = Line::mark(Mark::Declaration).and(Role::Plain, " ").and(
                    Role::Declaration,
                    format!("{} {}", symbol.kind, symbol.name),
                );
                if let Some(modifier) = symbol.modifier() {
                    line = line.and(Role::Plain, "   ").and(Role::Symbol, modifier);
                }
                rows.push(Row::new(line));
                if let Some(reach) = &result.reach {
                    rows.push(Row::new(
                        Line::of(Role::Plain, "  ")
                            .and(Role::Dim, "reaches")
                            .and(Role::Plain, "  ")
                            .and(Role::Plain, reach.to_string()),
                    ));
                }
                for alias in &result.via {
                    rows.push(Row::new(
                        Line::of(Role::Plain, "  ")
                            .and_line(Line::mark(Mark::ReExport))
                            .and(Role::Plain, " ")
                            .and(Role::Address, alias.to_string()),
                    ));
                }
                rows.push(Row::new(
                    Line::mark(Mark::ImportedBy)
                        .and(Role::Plain, " ")
                        .and(Role::Strong, result.importers.len().to_string()),
                ));
                rows.extend(result.importers.iter().map(|path| {
                    Row::new(
                        Line::of(Role::Plain, "  ").and(Role::Path, path.display().to_string()),
                    )
                }));
                rows
            }
            Block::Imports {
                unresolved,
                unused,
                redundant,
            } => {
                let sections: [(Mark, &str, &[ImportSite]); 3] = [
                    (Mark::Nothing, "unresolved", unresolved),
                    (Mark::ByHand, "unused", unused),
                    (Mark::ByHand, "redundant", redundant),
                ];
                let mut rows = Vec::new();
                let mut first = true;
                for (mark, word, sites) in sections {
                    if sites.is_empty() {
                        continue;
                    }
                    if !first {
                        rows.push(Row::new(Line::new()));
                    }
                    first = false;
                    rows.push(Row::new(
                        Line::mark(mark)
                            .and(Role::Plain, " ")
                            .and(Role::Strong, word)
                            .and(Role::Plain, " ")
                            .and(Role::Dim, sites.len().to_string()),
                    ));
                    rows.extend(sites.iter().map(|site| {
                        Row::at(
                            lines::ImportSiteLine::new(site).line(),
                            site.path.clone(),
                            site.start.line,
                        )
                    }));
                }
                rows
            }
            Block::Line(line) | Block::Summary(line) => vec![Row::new(line.clone())],
            Block::Note(Note::Hint(text)) => vec![Row::new(
                Line::of(Role::Hint, "hint: ").and(Role::Plain, text.clone()),
            )],
            Block::Note(Note::Warning(line)) => vec![Row::new(
                Line::of(Role::Warning, "warning: ").and_line(line.clone()),
            )],
            Block::Changes { state, files } => self.changes(files, *state),
            Block::Moved {
                state,
                files,
                respellings,
                notices,
            } => {
                let structural: Vec<&FileChange> = files
                    .iter()
                    .filter(|f| lines::Diff::is_structural(f, respellings))
                    .collect();
                let mut rows: Vec<Row> = Vec::new();
                if !respellings.is_empty() {
                    rows.push(Row::new(
                        Line::mark(Mark::Import)
                            .and(Role::Plain, " ")
                            .and(Role::Strong, respellings.len().to_string())
                            .and(Role::Plain, "   ")
                            .and(
                                Role::Dim,
                                Files::among(respellings.iter().map(|r| r.path.as_path()))
                                    .to_string(),
                            ),
                    ));
                }
                if !structural.is_empty() {
                    rows.push(Row::new(
                        Line::mark(Mark::Structure)
                            .and(Role::Plain, " ")
                            .and(Role::Strong, structural.len().to_string())
                            .and(Role::Plain, "   ")
                            .and(Role::Dim, Plural(structural.len(), "file").to_string()),
                    ));
                }
                if !notices.is_empty() {
                    rows.push(Row::new(
                        Line::mark(Mark::ByHand)
                            .and(Role::Plain, " ")
                            .and(Role::Strong, notices.len().to_string()),
                    ));
                }
                if !respellings.is_empty() || !notices.is_empty() {
                    rows.push(Row::new(Line::new()));
                    let width = respellings
                        .iter()
                        .map(|r| lines::Respellings::site(&r.path, r.start.line).len())
                        .chain(
                            notices
                                .iter()
                                .map(|n| lines::Respellings::site(&n.path, n.start.line).len()),
                        )
                        .max()
                        .unwrap_or(0);
                    rows.extend(lines::Respellings::new(respellings, width).rows());
                    rows.extend(notices.iter().map(|n| {
                        Row::at(
                            lines::NoticeRow::new(n, width).line(),
                            n.path.clone(),
                            n.start.line,
                        )
                    }));
                }
                for file in files {
                    let is_structural = lines::Diff::is_structural(file, respellings);
                    if !(options.diff || is_structural) {
                        continue;
                    }
                    rows.push(Row::new(Line::new()));
                    rows.extend(lines::Diff::new(file, is_structural).rows(*state));
                }
                rows
            }
            Block::Blank => vec![Row::new(Line::new())],
        }
    }

    fn occurrence(
        &self,
        occurrence: &Occurrence,
        ordinal: usize,
        _ticked: bool,
        _width: usize,
    ) -> Row {
        Row::at(
            lines::MatchRow::numbered(&occurrence.m, ordinal),
            occurrence.m.path.clone(),
            occurrence.m.start.line,
        )
    }

    fn respelling(&self, respelling: &Respelling, _width: usize) -> Row {
        let site = lines::Respellings::site(&respelling.path, respelling.start.line).len();
        Row::at(
            lines::Respellings::row(respelling, site),
            respelling.path.clone(),
            respelling.start.line,
        )
    }

    fn notice(&self, notice: &Notice, _width: usize) -> Row {
        let site = lines::Respellings::site(&notice.path, notice.start.line).len();
        Row::at(
            lines::NoticeRow::new(notice, site).line(),
            notice.path.clone(),
            notice.start.line,
        )
    }

    fn rewrite(&self, m: &Match, ordinal: usize, _ticked: bool, _width: usize) -> Row {
        Row::at(
            lines::MatchRow::numbered(m, ordinal),
            m.path.clone(),
            m.start.line,
        )
    }
}

impl Detailed {
    fn changes(&self, files: &[FileChange], state: crate::MutationState) -> Vec<Row> {
        let mut rows = Vec::new();
        for (i, file) in files.iter().enumerate() {
            if i > 0 {
                rows.push(Row::new(Line::new()));
            }
            rows.extend(lines::Diff::new(file, false).rows(state));
        }
        rows
    }
}

/// Occurrences as three sections — `✓`, `?`, `✗` — each a count, its files
/// and its reason as a tag. `✓` collapses to per-file counts unless expanded;
/// `?` and `✗` list every row, numbered as `--select` counts them.
struct VerdictRows<'a> {
    occurrences: &'a [Occurrence],
    options: Options,
    planned: Option<&'a [FileChange]>,
}

impl<'a> VerdictRows<'a> {
    fn new(
        occurrences: &'a [Occurrence],
        planned: Option<&'a [FileChange]>,
        options: Options,
    ) -> Self {
        Self {
            occurrences,
            planned,
            options,
        }
    }

    fn rows(&self) -> Vec<Row> {
        let width = self.occurrences.len().to_string().len();
        let mut rows = Vec::new();
        self.section(
            &mut rows,
            Confidence::Resolved,
            !self.options.verbose,
            width,
        );
        rows.push(Row::new(Line::new()));
        self.section(&mut rows, Confidence::Unresolved, false, width);
        rows.push(Row::new(Line::new()));
        self.section(&mut rows, Confidence::Other, false, width);
        rows
    }

    fn section(&self, lines: &mut Vec<Row>, confidence: Confidence, collapsed: bool, width: usize) {
        let rows: Vec<(usize, &Occurrence)> = self
            .occurrences
            .iter()
            .enumerate()
            .filter(|(_, o)| o.confidence == confidence)
            .collect();
        let mut head = lines::Verdict::new(confidence)
            .line()
            .and(Role::Plain, " ")
            .and(Role::Strong, rows.len().to_string());
        if rows.is_empty() {
            lines.push(Row::new(head));
            return;
        }
        head = head.and(Role::Plain, "  ").and(
            Role::Dim,
            Files::among(rows.iter().map(|(_, o)| o.m.path.as_path())).to_string(),
        );
        let reasons: BTreeSet<Reason> = rows.iter().map(|(_, o)| o.reason).collect();
        let shared = (reasons.len() == 1 && confidence != Confidence::Resolved)
            .then(|| *reasons.iter().next().expect("one"));
        if let Some(reason) = shared {
            head = head
                .and(Role::Plain, "   ")
                .and_line(lines::Tag::new(reason).line());
        }
        if confidence == Confidence::Unresolved
            && let Some(files) = self.planned
        {
            let planned = rows.iter().any(|(_, o)| {
                files
                    .iter()
                    .any(|c| c.path == o.m.path && c.edits.iter().any(|e| e.span == o.m.span))
            });
            let word = if planned { "in the plan" } else { "left out" };
            head = head.and(Role::Plain, "   ").and(Role::Dim, word);
        }
        lines.push(Row::new(head));

        if collapsed {
            // Rows are grouped by reason, so a file may recur; one line per file.
            let mut per_file: BTreeMap<&Path, usize> = BTreeMap::new();
            for (_, o) in &rows {
                *per_file.entry(o.m.path.as_path()).or_default() += 1;
            }
            for (path, count) in per_file {
                lines.push(Row::new(
                    Line::of(Role::Plain, "  ")
                        .and(Role::Ordinal, format!("{count:>width$}"))
                        .and(Role::Plain, "  ")
                        .and(Role::Path, path.display().to_string()),
                ));
            }
            return;
        }

        // Rows come grouped by reason; when a section has several, each run
        // gets a tag line of its own instead of a tag per row.
        let mut current: Option<&Path> = None;
        let mut reason: Option<Reason> = None;
        for (i, o) in &rows {
            if shared.is_none() && reason != Some(o.reason) {
                let run = rows.iter().filter(|(_, r)| r.reason == o.reason).count();
                lines.push(Row::new(
                    lines::Tag::new(o.reason)
                        .line()
                        .and(Role::Plain, "  ")
                        .and(Role::Dim, run.to_string()),
                ));
                reason = Some(o.reason);
                current = None;
            }
            if current != Some(o.m.path.as_path()) {
                lines.push(Row::new(Line::of(
                    Role::Path,
                    o.m.path.display().to_string(),
                )));
                current = Some(&o.m.path);
            }
            lines.push(Row::at(
                lines::MatchRow::new(&o.m, i + 1, width).line(),
                o.m.path.clone(),
                o.m.start.line,
            ));
        }
    }
}
