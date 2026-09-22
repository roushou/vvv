//! The pure line builders: answer data to [`display::Line`]s, no colours.
//!
//! Each shape is a type that turns protocol data into the runs a row is made
//! of. Styling happens later, when an interface maps a [`Role`] to a colour;
//! this module never sees a palette, a writer, or a terminal.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::Row;
use crate::Role as MatchRole;
use crate::protocol::display::{self, Line, Role};
use crate::protocol::vocabulary::{Ago, Files, IntentLine, Mark, Plural};
use crate::protocol::{
    Dep, FileChange, HistoryEntry, ImportSite, Importer, Notice, NoticeKind, OutlineItem, Placed,
    Respelling, Site, Skipped,
};
use crate::{Confidence, Match, Occurrence, Reach, ReachKind, Reason, Span, Symbol, SymbolKind};

/// A declaration as `● kind name   ◆ address   path:line`.
pub struct Declaration<'a> {
    m: &'a Match,
}

impl<'a> Declaration<'a> {
    pub fn new(m: &'a Match) -> Self {
        Self { m }
    }

    pub fn line(&self) -> Line {
        let m = self.m;
        let mut line = Line::mark(Mark::Declaration);
        if let Some(s) = &m.symbol {
            line = line
                .and(Role::Plain, " ")
                .and(Role::Declaration, format!("{} {}", s.kind, s.name));
        }
        if let Some(address) = &m.address {
            line = line.and(Role::Address, format!("   ◆ {address}"));
        }
        line.and(Role::Plain, "   ")
            .and(Role::Path, m.path.display().to_string())
            .and(Role::Plain, ":")
            .and(Role::LineNumber, m.start.display().to_string())
    }
}

/// `● kind name   ◆ address   path:line` for a declaration the graph placed.
pub struct PlacedLine<'a> {
    placed: &'a Placed,
}

impl<'a> PlacedLine<'a> {
    pub fn new(placed: &'a Placed) -> Self {
        Self { placed }
    }

    pub fn line(&self) -> Line {
        let d = self.placed;
        Line::mark(Mark::Declaration)
            .and(Role::Plain, " ")
            .and(
                Role::Declaration,
                format!("{} {}", d.symbol.kind, d.symbol.name),
            )
            .and(Role::Address, format!("   ◆ {}", d.address))
            .and(Role::Plain, "   ")
            .and(Role::Path, d.path.display().to_string())
            .and(Role::Plain, ":")
            .and(Role::LineNumber, d.start.display().to_string())
    }
}

/// A `where` site: the declaration line, then the import to write, indented.
pub struct SiteLine<'a> {
    site: &'a Site,
}

impl<'a> SiteLine<'a> {
    pub fn new(site: &'a Site) -> Self {
        Self { site }
    }

    pub fn lines(&self) -> Vec<Line> {
        let s = self.site;
        let mut declaration = s.declaration.clone();
        if declaration.address.is_none() {
            declaration.address = s.address.clone();
        }
        let mut lines = vec![Declaration::new(&declaration).line()];
        if let Some(import) = &s.import {
            lines.push(
                Line::of(Role::Plain, "  ")
                    .and_line(Line::mark(Mark::Import))
                    .and(Role::Plain, " ")
                    .and(Role::Strong, import.clone()),
            );
        }
        lines
    }
}

/// `path:line   import` for an import statement worth a look.
pub struct ImportSiteLine<'a> {
    site: &'a ImportSite,
}

impl<'a> ImportSiteLine<'a> {
    pub fn new(site: &'a ImportSite) -> Self {
        Self { site }
    }

    pub fn line(&self) -> Line {
        let s = self.site;
        Line::of(Role::Plain, "  ")
            .and(Role::Path, s.path.display().to_string())
            .and(Role::Plain, ":")
            .and(Role::LineNumber, s.start.display().to_string())
            .and(Role::Plain, "   ")
            .and(Role::Import, s.import.path.to_string())
    }
}

/// `#id  when  intent  N files`, with `↩` on the entry `undo` reverses.
pub struct HistoryLine<'a> {
    item: &'a HistoryEntry,
    now: u64,
    newest: bool,
}

impl<'a> HistoryLine<'a> {
    pub fn new(item: &'a HistoryEntry, now: u64) -> Self {
        Self {
            item,
            now,
            newest: false,
        }
    }

    pub fn newest(mut self, newest: bool) -> Self {
        self.newest = newest;
        self
    }

    pub fn line(&self) -> Line {
        let mut line = Line::of(Role::Strong, format!("#{:<3}", self.item.id))
            .and(Role::Plain, "  ")
            .and(
                Role::Dim,
                format!("{:>12}", Ago::between(self.item.at, self.now)),
            )
            .and(Role::Plain, "  ")
            .and(Role::Plain, IntentLine(&self.item.intent).to_string())
            .and(Role::Plain, "  ")
            .and(Role::Dim, Plural(self.item.files, "file").to_string());
        if self.newest {
            line = line.and(Role::Plain, "  ").and_line(Line::mark(Mark::Undo));
        }
        line
    }
}

/// `typescript skipped: <reason>`.
pub struct SkippedLine<'a> {
    skipped: &'a Skipped,
}

impl<'a> SkippedLine<'a> {
    pub fn new(skipped: &'a Skipped) -> Self {
        Self { skipped }
    }

    pub fn line(&self) -> Line {
        Line::of(
            Role::Plain,
            format!("{} skipped: {}", self.skipped.language, self.skipped.reason),
        )
    }
}

/// A reason as a tag: its mark's glyph and word.
pub struct Tag {
    reason: Reason,
}

impl Tag {
    pub fn new(reason: Reason) -> Self {
        Self { reason }
    }

    pub fn line(&self) -> Line {
        let mark = Mark::from(self.reason);
        Line::mark(mark)
            .and(Role::Plain, " ")
            .and(Role::Dim, mark.word())
    }
}

/// The verdict glyphs: `✓` will, `?` unsure, `✗` will not.
pub struct Verdict {
    confidence: Confidence,
}

impl Verdict {
    pub fn new(confidence: Confidence) -> Self {
        Self { confidence }
    }

    pub fn line(&self) -> Line {
        Line::mark(Mark::from(self.confidence))
    }
}

/// One numbered match: `N ● line:col  kind name  source  +N  ◆ address`.
pub(crate) struct MatchRow<'a> {
    ordinal: usize,
    width: usize,
    name_width: usize,
    m: &'a Match,
}

impl MatchRow<'_> {
    /// One match as a numbered row, for a view that lists matches one at a
    /// time (the picker).
    pub fn numbered(m: &Match, ordinal: usize) -> Line {
        MatchRow {
            ordinal,
            width: ordinal.to_string().len(),
            name_width: 0,
            m,
        }
        .line()
    }

    /// The glyph is the role (`●` declaration, `→` import, blank use); `kind
    /// name` is padded to the section's widest so names line up.
    fn line(&self) -> Line {
        let m = self.m;
        let mut line = Line::of(Role::Plain, "  ")
            .and(
                Role::Ordinal,
                format!("{:>w$}", self.ordinal, w = self.width),
            )
            .and(Role::Plain, " ");
        line = match m.role {
            MatchRole::Declaration => line.and_line(Line::mark(Mark::Declaration)),
            MatchRole::Import => line.and_line(Line::mark(Mark::Import)),
            MatchRole::Use => line.and(Role::Plain, " "),
        };
        line = line
            .and(Role::Plain, " ")
            .and(
                Role::LineNumber,
                format!("{:>4}:{:<3}", m.start.line + 1, m.start.column + 1),
            )
            .and(Role::Plain, "  ");
        if self.name_width > 0 {
            let name = m
                .symbol
                .as_ref()
                .map(|s| format!("{} {}", s.kind, s.name))
                .unwrap_or_default();
            line = line
                .and(
                    Role::Declaration,
                    format!("{name:<w$}", w = self.name_width),
                )
                .and(Role::Plain, "  ");
        }
        let rest = if m.role == MatchRole::Import {
            Role::Import
        } else {
            Role::Plain
        };
        line = line.and_line(Line::hit(m, rest));
        if m.line_count() > 1 {
            line = line.and(Role::Dim, format!("  +{}", m.line_count() - 1));
        }
        if let Some(address) = &m.address {
            line = line.and(Role::Address, format!("  ◆ {address}"));
        }
        line
    }
}

/// Search results as two sections — `●` declarations, then everything else
/// with `→` on imports — each grouped by file, rows numbered in the order
/// printed (the numbers `--select` takes). Headers appear only when both
/// sections are present; a single section is the whole answer.
pub struct Sections<'a> {
    matches: &'a [Match],
}

impl<'a> Sections<'a> {
    pub fn new(matches: &'a [Match]) -> Self {
        Self { matches }
    }

    /// The `●` rows and the rest, in the engine's order.
    pub fn split(matches: &[Match]) -> (&[Match], &[Match]) {
        let n = matches
            .iter()
            .take_while(|m| m.role == MatchRole::Declaration)
            .count();
        matches.split_at(n)
    }

    pub fn lines(&self) -> Vec<Row> {
        let (declarations, uses) = Self::split(self.matches);
        let both = !declarations.is_empty() && !uses.is_empty();
        let width = self.matches.len().to_string().len();
        let mut rows = Vec::new();
        if !declarations.is_empty() {
            if both {
                rows.push(Row::new(Self::header(
                    Some(Mark::Declaration),
                    declarations,
                )));
            }
            Self::rows(&mut rows, declarations, 1, width);
        }
        if !uses.is_empty() {
            if both {
                rows.push(Row::new(Line::new()));
                rows.push(Row::new(Self::header(None, uses)));
            }
            Self::rows(&mut rows, uses, declarations.len() + 1, width);
        }
        rows
    }

    /// `● 1` / `○ 59  13 files`: a glyph (`○`, uses, is not a mark of its own),
    /// a count, the files.
    fn header(mark: Option<Mark>, matches: &[Match]) -> Line {
        let line = match mark {
            Some(mark) => Line::mark(mark),
            None => Line::of(Role::Dim, "○".to_string()),
        };
        line.and(Role::Plain, " ")
            .and(Role::Strong, matches.len().to_string())
            .and(Role::Plain, "  ")
            .and(
                Role::Dim,
                Files::among(matches.iter().map(|m| m.path.as_path())).to_string(),
            )
    }

    fn rows(out: &mut Vec<Row>, matches: &[Match], first: usize, width: usize) {
        let name_width = matches
            .iter()
            .filter_map(|m| m.symbol.as_ref())
            .map(|s| s.kind.to_string().len() + 1 + s.name.len())
            .max()
            .unwrap_or(0);
        let mut current: Option<&Path> = None;
        for (i, m) in matches.iter().enumerate() {
            if current != Some(m.path.as_path()) {
                if current.is_some() {
                    out.push(Row::new(Line::new()));
                }
                out.push(Row::new(Line::of(Role::Path, m.path.display().to_string())));
                current = Some(&m.path);
            }
            out.push(Row::at(
                MatchRow {
                    ordinal: first + i,
                    width,
                    name_width,
                    m,
                }
                .line(),
                m.path.clone(),
                m.start.line,
            ));
        }
    }
}

/// Occurrences as three sections — `✓`, `?`, `✗` — each a count, its files
/// and its reason as a tag. `✓` collapses to per-file counts unless expanded;
/// `?` and `✗` list every row, numbered as `--select` counts them.
pub struct Verdicts<'a> {
    occurrences: &'a [Occurrence],
    expanded: bool,
    planned: Option<&'a [FileChange]>,
}

impl<'a> Verdicts<'a> {
    pub fn new(occurrences: &'a [Occurrence]) -> Self {
        Self {
            occurrences,
            expanded: false,
            planned: None,
        }
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn planned(mut self, planned: Option<&'a [FileChange]>) -> Self {
        self.planned = planned;
        self
    }

    /// The row numbers of one verdict, as `--select` would take them: `27-33`
    /// when contiguous, else a list.
    pub fn selection(occurrences: &[Occurrence], confidence: Confidence) -> String {
        let numbers: Vec<usize> = occurrences
            .iter()
            .enumerate()
            .filter(|(_, o)| o.confidence == confidence)
            .map(|(i, _)| i + 1)
            .collect();
        match (numbers.first(), numbers.last()) {
            (Some(first), Some(last)) if last - first + 1 == numbers.len() && first != last => {
                format!("{first}-{last}")
            }
            _ => numbers
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
        }
    }

    pub fn lines(&self) -> Vec<Row> {
        let width = self.occurrences.len().to_string().len();
        let mut rows = Vec::new();
        self.section(&mut rows, Confidence::Resolved, !self.expanded, width);
        rows.push(Row::new(Line::new()));
        self.section(&mut rows, Confidence::Unresolved, false, width);
        rows.push(Row::new(Line::new()));
        self.section(&mut rows, Confidence::Other, false, width);
        rows
    }

    /// One occurrence as a numbered row, for a view that lists them one at a
    /// time (the picker).
    pub fn row(occurrence: &Occurrence, ordinal: usize) -> Line {
        MatchRow::numbered(&occurrence.m, ordinal)
    }

    fn section(&self, lines: &mut Vec<Row>, confidence: Confidence, collapsed: bool, width: usize) {
        let rows: Vec<(usize, &Occurrence)> = self
            .occurrences
            .iter()
            .enumerate()
            .filter(|(_, o)| o.confidence == confidence)
            .collect();
        let mut head = Verdict::new(confidence)
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
                .and_line(Tag::new(reason).line());
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
                    Tag::new(o.reason)
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
                MatchRow {
                    ordinal: i + 1,
                    width,
                    name_width: 0,
                    m: &o.m,
                }
                .line(),
                o.m.path.clone(),
                o.m.start.line,
            ));
        }
    }
}

/// A file's declarations as a tree: nesting by span containment, the name
/// column aligned, the modifier after it. With `reach`, who may name each
/// item follows.
pub struct OutlineTree<'a> {
    items: &'a [OutlineItem],
    reach: bool,
}

impl<'a> OutlineTree<'a> {
    pub fn new(items: &'a [OutlineItem]) -> Self {
        Self {
            items,
            reach: false,
        }
    }

    pub fn with_reach(mut self, reach: bool) -> Self {
        self.reach = reach;
        self
    }

    pub fn lines(&self) -> Vec<Line> {
        let depths = Self::depths(self.items);
        let labels: Vec<String> = self
            .items
            .iter()
            .zip(&depths)
            .map(|(item, depth)| format!("{}{}", "  ".repeat(*depth), Self::label(&item.symbol)))
            .collect();
        let width = labels.iter().map(String::len).max().unwrap_or(0);
        self.items
            .iter()
            .zip(&labels)
            .map(|(item, label)| {
                let mut line = Line::of(Role::LineNumber, format!("{:>4}", item.start.line + 1))
                    .and(Role::Plain, "  ");
                if item.symbol.kind == SymbolKind::Impl {
                    line = line.and(Role::Declaration, label.clone());
                } else {
                    let (role, text) = Self::modifier(item);
                    line = line
                        .and(Role::Declaration, format!("{label:<width$}"))
                        .and(Role::Plain, "   ")
                        .and(role, text);
                }
                if self.reach
                    && let Some(reach) = &item.reach
                {
                    line = line
                        .and(Role::Plain, "   ")
                        .and(Role::Dim, reach.to_string());
                }
                line
            })
            .collect()
    }

    fn label(symbol: &Symbol) -> String {
        match symbol.kind {
            SymbolKind::Function | SymbolKind::Method => format!("{}()", symbol.name),
            SymbolKind::Field | SymbolKind::Variant => symbol.name.clone(),
            kind => format!("{kind} {}", symbol.name),
        }
    }

    /// Each item's depth: how many earlier items' spans enclose it.
    fn depths(items: &[OutlineItem]) -> Vec<usize> {
        let mut stack: Vec<Span> = Vec::new();
        items
            .iter()
            .map(|item| {
                let span = item.symbol.span;
                while stack
                    .last()
                    .is_some_and(|open| !(span.start >= open.start && span.end <= open.end))
                {
                    stack.pop();
                }
                let depth = stack.len();
                stack.push(span);
                depth
            })
            .collect()
    }

    /// The modifier as written, by how far it reaches; `·` when there is none.
    fn modifier(item: &OutlineItem) -> (Role, String) {
        let text = item.symbol.modifier().unwrap_or("·").to_owned();
        let role = match (&item.reach, item.symbol.modifier()) {
            (Some(Reach::Everyone), _) => Role::Added,
            (Some(Reach::Package(_)), _) => Role::Warning,
            (Some(Reach::Within(_)), Some(_)) => Role::Warning,
            (None, Some(_)) => Role::Symbol,
            _ => Role::Dim,
        };
        (role, text)
    }
}

/// A file's imports grouped by the package they lead into, one row per
/// statement: `line  path{a, b}   → file`. The file's own package comes
/// first, unresolved (`?`) last.
pub struct DepGroups<'a> {
    deps: &'a [Dep],
    own: Option<&'a str>,
}

impl<'a> DepGroups<'a> {
    pub fn new(deps: &'a [Dep], own: Option<&'a str>) -> Self {
        Self { deps, own }
    }

    /// How many import statements the entries come from.
    pub fn statements(deps: &[Dep]) -> usize {
        deps.iter()
            .map(|d| Self::statement_of(deps, d))
            .collect::<BTreeSet<Span>>()
            .len()
    }

    pub fn lines(&self) -> Vec<Line> {
        let deps = self.deps;
        let mut lines = Vec::new();
        // Statements in source order, each its entries.
        let mut statements: Vec<(Span, Vec<&Dep>)> = Vec::new();
        for dep in deps {
            let key = Self::statement_of(deps, dep);
            match statements.iter_mut().find(|(k, _)| *k == key) {
                Some((_, entries)) => entries.push(dep),
                None => statements.push((key, vec![dep])),
            }
        }
        // Grouped by package: own, then the others by name, `?` last.
        let package = |entries: &[&Dep]| {
            entries
                .iter()
                .find_map(|d| d.address.as_ref())
                .map(|a| a.package().as_str().to_owned())
        };
        let mut packages: Vec<Option<String>> = statements
            .iter()
            .map(|(_, entries)| package(entries))
            .collect();
        packages.sort_by_key(|k| (k.is_none(), k.as_deref() != self.own, k.clone()));
        packages.dedup();
        // One very long statement should not push every arrow off-screen.
        let width = statements
            .iter()
            .map(|(_, e)| Self::spell(e).len())
            .filter(|w| *w <= 60)
            .max()
            .unwrap_or(0);
        for key in &packages {
            let head = key.as_deref().unwrap_or("?");
            if !head.is_empty() {
                lines.push(Line::of(Role::Plain, "  ").and(Role::Strong, head.to_owned()));
            }
            for (_, entries) in statements.iter().filter(|(_, e)| package(e) == *key) {
                // A group's prefix is not an import of its own.
                let grouped = entries.iter().any(|d| d.import.group.is_some());
                let mut files: Vec<&Path> = entries
                    .iter()
                    .filter(|d| !grouped || d.import.group.is_some())
                    .filter_map(|d| d.file.as_deref())
                    .collect();
                files.sort();
                files.dedup();
                let mut line = Line::of(Role::Plain, "  ")
                    .and(
                        Role::LineNumber,
                        format!("{:>4}", entries[0].start.line + 1),
                    )
                    .and(Role::Plain, "  ");
                if files.is_empty() {
                    line = line.and(Role::Plain, Self::spell(entries));
                } else {
                    line = line
                        .and(Role::Dim, format!("{:<width$}", Self::spell(entries)))
                        .and(Role::Plain, "   ")
                        .and_line(Line::mark(Mark::Import))
                        .and(Role::Plain, " ")
                        .and(
                            Role::Path,
                            files
                                .iter()
                                .map(|f| f.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", "),
                        );
                }
                lines.push(line);
            }
        }
        lines
    }

    /// The statement each entry belongs to: its group's, or, for an entry
    /// without a group of its own, the statement whose span holds it.
    fn statement_of(deps: &[Dep], dep: &Dep) -> Span {
        dep.import.group.as_ref().map_or_else(
            || {
                deps.iter()
                    .filter_map(|d| d.import.group.as_ref())
                    .map(|g| g.statement)
                    .find(|st| st.contains(&dep.import.span))
                    .unwrap_or(dep.import.span)
            },
            |g| g.statement,
        )
    }

    /// The entries of one statement as one path: the shared group prefix with
    /// the tails braced, or the single path.
    fn spell(entries: &[&Dep]) -> String {
        let [only] = entries else {
            let Some(prefix) = entries
                .iter()
                .filter_map(|d| d.import.group.as_ref())
                .map(|g| &g.prefix)
                .next()
            else {
                return entries
                    .iter()
                    .map(|d| d.import.path.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
            };
            let same_prefix = entries
                .iter()
                .all(|d| d.import.group.as_ref().is_none_or(|g| &g.prefix == prefix));
            if !same_prefix {
                return entries
                    .iter()
                    .map(|d| d.import.path.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
            }
            let list = entries
                .iter()
                .filter_map(|d| d.import.group.as_ref())
                .map(|g| g.list)
                .next();
            let tails: Vec<String> = entries
                .iter()
                .filter_map(|d| match d.import.path.strip_prefix(prefix) {
                    Some(tail) if !tail.is_empty() => Some(d.import.path.spell_segments(tail)),
                    _ if list.is_some_and(|l| l.contains(&d.import.span)) => {
                        Some("self".to_owned())
                    }
                    _ if d.import.group.is_some() => Some(d.import.path.to_string()),
                    _ => None,
                })
                .collect();
            let separator = prefix.syntax().separator();
            return format!("{prefix}{separator}{{{}}}", tails.join(", "));
        };
        only.import.path.to_string()
    }
}

/// `path:line   Name Name` for each file that imports the one asked about.
pub struct ImporterRows<'a> {
    importers: &'a [Importer],
}

impl<'a> ImporterRows<'a> {
    pub fn new(importers: &'a [Importer]) -> Self {
        Self { importers }
    }

    /// Distinct statements: one per `(path, line)`.
    pub fn sites(importers: &[Importer]) -> Vec<(&Path, u32, Vec<&str>)> {
        let mut rows: Vec<(&Path, u32, Vec<&str>)> = Vec::new();
        // A group's prefix is listed as an entry too; it names nothing.
        let is_prefix = |i: &Importer| {
            importers.iter().any(|o| {
                o.path == i.path
                    && o.start.line == i.start.line
                    && o.import.path.continues(&i.import.path)
            })
        };
        for i in importers.iter().filter(|i| !is_prefix(i)) {
            let name = i.import.path.last().map_or("", |n| n.as_str());
            match rows
                .iter_mut()
                .find(|(p, l, _)| *p == i.path.as_path() && *l == i.start.line)
            {
                Some((_, _, names)) => names.push(name),
                None => rows.push((i.path.as_path(), i.start.line, vec![name])),
            }
        }
        rows
    }

    pub fn lines(&self) -> Vec<Line> {
        let rows = Self::sites(self.importers);
        let width = rows
            .iter()
            .map(|(path, line, _)| Respellings::site(path, *line).len())
            .max()
            .unwrap_or(0);
        rows.into_iter()
            .map(|(path, line, names)| {
                Line::of(Role::Plain, "  ")
                    .and(
                        Role::Path,
                        format!("{:<width$}", Respellings::site(path, line)),
                    )
                    .and(Role::Plain, "   ")
                    .and(Role::Plain, names.join(" "))
            })
            .collect()
    }
}

/// `→ path:line   from → to` for each reference a move re-spelled.
pub struct Respellings<'a> {
    rows: &'a [Respelling],
    width: usize,
}

impl<'a> Respellings<'a> {
    pub fn new(rows: &'a [Respelling], width: usize) -> Self {
        Self { rows, width }
    }

    /// `path:line`, as the row prints it — for sizing the column.
    pub fn site(path: &Path, line: u32) -> String {
        format!("{}:{}", path.display(), line + 1)
    }

    pub fn lines(&self) -> Vec<Line> {
        self.rows.iter().map(|r| Self::row(r, self.width)).collect()
    }

    /// One re-spelling as a row: `→ path:line   from → to`.
    pub fn row(r: &Respelling, width: usize) -> Line {
        Line::mark(Mark::Import)
            .and(Role::Plain, " ")
            .and(
                Role::Path,
                format!("{:<width$}", Self::site(&r.path, r.start.line)),
            )
            .and(Role::Plain, "   ")
            .and(Role::Dim, r.from.clone())
            .and(Role::Plain, " ")
            .and_line(Line::mark(Mark::Import))
            .and(Role::Plain, " ")
            .and_line(Self::changed(&r.from, &r.to))
    }

    /// `to` with the segment that differs from `from` in bold.
    fn changed(from: &str, to: &str) -> Line {
        let from: Vec<char> = from.chars().collect();
        let to: Vec<char> = to.chars().collect();
        let prefix = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
        let suffix = from
            .iter()
            .rev()
            .zip(to.iter().rev())
            .take_while(|(a, b)| a == b)
            .count()
            .min(to.len() - prefix)
            .min(from.len() - prefix);
        let head: String = to[..prefix].iter().collect();
        let mid: String = to[prefix..to.len() - suffix].iter().collect();
        let tail: String = to[to.len() - suffix..].iter().collect();
        Line::of(Role::Plain, head)
            .and(Role::Strong, mid)
            .and(Role::Plain, tail)
    }
}

/// A notice as a `!` row: `! path:line   what, and what to do`.
pub struct NoticeRow<'a> {
    notice: &'a Notice,
    width: usize,
}

impl<'a> NoticeRow<'a> {
    pub fn new(notice: &'a Notice, width: usize) -> Self {
        Self { notice, width }
    }

    pub fn line(&self) -> Line {
        let (n, width) = (self.notice, self.width);
        let line = Line::mark(Mark::ByHand)
            .and(Role::Plain, " ")
            .and(
                Role::Path,
                format!("{:<width$}", Respellings::site(&n.path, n.start.line)),
            )
            .and(Role::Plain, "   ");
        match &n.kind {
            NoticeKind::UnrewritableImport {
                import,
                replacement,
            } => line
                .and(Role::Dim, import.clone())
                .and(Role::Plain, " ")
                .and_line(Line::mark(Mark::Import))
                .and(Role::Plain, " ")
                .and(Role::Plain, replacement.clone())
                .and(Role::Plain, "   ")
                .and(Role::Dim, "grouped import, by hand"),
            NoticeKind::RedundantImport { import } => line
                .and(Role::Dim, import.clone())
                .and(Role::Plain, "   ")
                .and(Role::Dim, "now declared here, remove by hand"),
            NoticeKind::Unreachable { item, from, needs } => line
                .and(Role::Plain, item.clone())
                .and(Role::Plain, "   ")
                .and(
                    Role::Dim,
                    match needs {
                        ReachKind::Everyone => {
                            format!("used from {from}, another package; `pub` is your call")
                        }
                        needs => format!("used from {from}, needs {needs:?}; by hand"),
                    },
                ),
        }
    }
}

/// A source line with a caret under a span.
pub struct Caret {
    line: u32,
    text: String,
    column: usize,
    len: usize,
}

impl Caret {
    pub fn new(line: u32, text: &str, column: usize, len: usize) -> Self {
        Self {
            line,
            text: text.to_owned(),
            column,
            len,
        }
    }

    pub fn lines(&self) -> Vec<Line> {
        let indent = self.text.len() - self.text.trim_start().len();
        let column = self.column.saturating_sub(indent);
        vec![
            Line::of(Role::Plain, "  ")
                .and(Role::LineNumber, format!("{:>4}", self.line + 1))
                .and(Role::Plain, " ")
                .and(Role::Dim, "│".to_string())
                .and(Role::Plain, " ")
                .and(Role::Plain, self.text.trim_start().to_string()),
            Line::of(Role::Plain, format!("  {:>4} ", ""))
                .and(Role::Dim, "│".to_string())
                .and(Role::Plain, " ")
                .and(Role::Plain, " ".repeat(column))
                .and(Role::Hit, "^".repeat(self.len.max(1))),
        ]
    }
}

/// One file's change: a path header (`from → to` for moves) and its unified
/// diff, the `±` on the header when the hunks are structural edits.
pub struct Diff<'a> {
    file: &'a FileChange,
    structural: bool,
}

impl<'a> Diff<'a> {
    pub fn new(file: &'a FileChange, structural: bool) -> Self {
        Self { file, structural }
    }

    /// Whether a file's edits are more than re-spelled paths: what a move calls
    /// structural, so a view can mark it (`±`) and the composition can count it.
    pub fn is_structural(file: &FileChange, respellings: &[Respelling]) -> bool {
        !(file.moved_to.is_none()
            && !file.edits.is_empty()
            && file.edits.iter().all(|e| {
                respellings
                    .iter()
                    .any(|r| r.path == file.path && r.span == e.span)
            }))
    }

    pub fn lines(&self) -> Vec<Line> {
        let file = self.file;
        let header = match &file.moved_to {
            Some(to) => format!("{} → {}", file.path.display(), to.display()),
            None => file.path.display().to_string(),
        };
        let mut head = if self.structural {
            Line::mark(Mark::Structure).and(Role::Plain, " ")
        } else {
            Line::new()
        };
        head = head.and(Role::Path, header);
        let mut lines = vec![head];
        lines.extend(display::diff(file));
        lines
    }
}

#[cfg(test)]
mod snapshots {
    use std::path::Path;

    use crate::protocol::UnifiedDiff;
    use crate::{Edit, Span};
    use vvv_core::RelPath;

    use super::*;

    #[test]
    fn diff_drops_file_headers_and_shows_moves() {
        let moved = FileChange {
            path: RelPath::from("a.rs"),
            moved_to: Some(RelPath::from("b/a.rs")),
            edits: vec![Edit::replace(Span::new(0, 1), "x")],
            diff: UnifiedDiff::between(Path::new("a.rs"), Path::new("b/a.rs"), "x\n", "y\n"),
        };
        let rendered: String = Diff::new(&moved, false)
            .lines()
            .iter()
            .map(|line| {
                let text: String = line.pieces().iter().map(|p| p.text.as_str()).collect();
                format!("{text}\n")
            })
            .collect();
        insta::assert_snapshot!(rendered);
    }
}
