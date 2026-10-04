//! File navigation and fuzzy path matching over retained search occurrences.

use super::SearchPanel;
use std::collections::{BTreeMap, BTreeSet};
use vvv_engine::{Match, MatchId, RelPath};

#[derive(Debug, Clone, Default)]
pub struct FileNavigator {
    pub filter: String,
    pub caret: crate::input::Caret,
    pub edit: Option<FileEdit>,
    pub active: Option<RelPath>,
    /// Selection hidden by restrictions, until the user deliberately moves.
    pub preferred: Option<MatchId>,
    remembered: BTreeMap<RelPath, MatchId>,
    pub viewport: ListViewport,
    pub match_viewports: BTreeMap<RelPath, ListViewport>,
}

#[derive(Debug, Clone)]
pub struct FileEdit {
    filter: String,
    pub selected: Option<MatchId>,
    return_focus: SearchPanel,
    active: Option<RelPath>,
    preferred: Option<MatchId>,
    remembered: BTreeMap<RelPath, MatchId>,
    pub viewport: ListViewport,
    pub match_viewports: BTreeMap<RelPath, ListViewport>,
}

pub struct FileGroup<'a> {
    pub path: &'a RelPath,
    pub matches: Vec<&'a Match>,
    pub rank: PathMatch,
}

impl FileNavigator {
    pub fn remember(&mut self, path: RelPath, id: MatchId) {
        self.active = Some(path.clone());
        self.remembered.insert(path, id);
    }

    pub fn selected<'a>(&self, file: &FileGroup<'a>) -> Option<&'a Match> {
        self.remembered
            .get(file.path)
            .and_then(|id| file.matches.iter().find(|m| m.id == *id).copied())
            .or_else(|| file.matches.first().copied())
    }

    pub fn retain(&mut self, matches: &[Match]) {
        let valid: BTreeSet<_> = matches.iter().map(|m| (&m.path, &m.id)).collect();
        self.remembered
            .retain(|path, id| valid.contains(&(path, &*id)));
        self.match_viewports
            .retain(|path, _| matches.iter().any(|m| &m.path == path));
    }

    pub fn begin(&mut self, selected: Option<MatchId>, return_focus: SearchPanel) {
        if self.edit.is_none() {
            self.caret.reset();
            self.edit = Some(FileEdit {
                filter: self.filter.clone(),
                selected,
                return_focus,
                active: self.active.clone(),
                preferred: self.preferred.clone(),
                remembered: self.remembered.clone(),
                viewport: self.viewport,
                match_viewports: self.match_viewports.clone(),
            });
        }
    }

    pub fn cancel(&mut self) -> Option<(Option<MatchId>, SearchPanel)> {
        let edit = self.edit.take()?;
        self.filter = edit.filter;
        self.remembered = edit.remembered;
        self.active = edit.active;
        self.preferred = edit.preferred;
        self.viewport = edit.viewport;
        self.match_viewports = edit.match_viewports;
        Some((edit.selected, edit.return_focus))
    }

    pub fn reveal(&mut self) {
        self.viewport.reveal = true;
        if let Some(path) = &self.active {
            self.match_viewports.entry(path.clone()).or_default().reveal = true;
        }
    }

    pub fn retained_bytes(&self) -> usize {
        let memory = |entries: &BTreeMap<RelPath, MatchId>| {
            entries
                .keys()
                .map(|p| p.as_str().len() + 128)
                .sum::<usize>()
        };
        self.match_viewports.len() * 128
            + usize::from(self.preferred.is_some()) * 64
            + self.filter.len()
            + self.active.as_ref().map_or(0, |p| p.as_str().len() + 32)
            + memory(&self.remembered)
            + self.edit.as_ref().map_or(0, |e| {
                e.filter.len()
                    + memory(&e.remembered)
                    + e.match_viewports
                        .keys()
                        .map(|path| path.as_str().len() + 64)
                        .sum::<usize>()
                    + 128
                    + usize::from(e.preferred.is_some()) * 64
                    + e.active.as_ref().map_or(0, |p| p.as_str().len() + 32)
            })
    }
}

/// A list viewport independent of its selected row. Pointer scrolling leaves
/// selection alone; keyboard selection asks to reveal it again.
#[derive(Debug, Clone, Copy)]
pub struct ListViewport {
    pub offset: usize,
    pub reveal: bool,
}

impl Default for ListViewport {
    fn default() -> Self {
        Self {
            offset: 0,
            reveal: true,
        }
    }
}

impl ListViewport {
    pub fn offset(
        self,
        rows: usize,
        height: usize,
        selection: Option<std::ops::Range<usize>>,
    ) -> usize {
        let maximum = rows.saturating_sub(height.max(1));
        let mut offset = self.offset.min(maximum);
        if self.reveal
            && height > 0
            && let Some(selected) = selection
        {
            if selected.start < offset {
                offset = selected.start;
            }
            if selected.end > offset + height {
                offset = if selected.len() > height {
                    selected.start
                } else {
                    selected.end - height
                };
            }
        }
        offset.min(maximum)
    }

    pub fn scroll(&mut self, by: i32, rows: usize, height: usize) {
        self.offset = (self.offset as i64 + i64::from(by))
            .clamp(0, rows.saturating_sub(height.max(1)) as i64) as usize;
        self.reveal = false;
    }
}

#[derive(Debug, Clone)]
pub enum PointerIntent {
    Focus(SearchPanel),
    File(RelPath),
    Match(MatchId),
    Outline {
        path: RelPath,
        content: vvv_engine::ContentId,
        span: vvv_engine::Span,
    },
    Filter,
    Scroll {
        panel: SearchPanel,
        offset: usize,
        rows: usize,
        height: usize,
        by: i32,
    },
}

#[derive(Debug, Clone)]
pub struct Pointer {
    pub revision: u64,
    pub intent: PointerIntent,
}

pub struct ListGeometry {
    pub panel: SearchPanel,
    pub area: ratatui::layout::Rect,
    pub content: ratatui::layout::Rect,
    pub rows: Vec<PointerIntent>,
    pub offset: usize,
}

impl ListGeometry {
    pub fn indicator(&self) -> &'static str {
        match (
            self.offset > 0,
            self.offset + (self.content.height as usize) < self.rows.len(),
        ) {
            (true, true) => " ↑↓",
            (true, false) => " ↑",
            (false, true) => " ↓",
            _ => "",
        }
    }
}

/// Geometry and stable identities from the last frame actually presented.
#[derive(Default)]
pub struct SearchFrame {
    pub revision: u64,
    pub panels: Vec<(SearchPanel, ratatui::layout::Rect)>,
    pub lists: Vec<ListGeometry>,
}

impl SearchFrame {
    pub fn pointer(&self, event: ratatui::crossterm::event::MouseEvent) -> Option<Pointer> {
        use ratatui::crossterm::event::{MouseButton, MouseEventKind};
        let point = ratatui::layout::Position::new(event.column, event.row);
        let (panel, _) = self
            .panels
            .iter()
            .rev()
            .find(|(_, area)| area.contains(point))?;
        let list = self.lists.iter().find(|list| list.panel == *panel);
        let intent = match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(list) = list {
                    if list.content.contains(point) {
                        let row = list.offset + usize::from(event.row - list.content.y);
                        list.rows
                            .get(row)
                            .cloned()
                            .unwrap_or(PointerIntent::Focus(*panel))
                    } else if matches!(panel, SearchPanel::Files | SearchPanel::Results)
                        && event.row > list.area.y
                        && event.row < list.content.y
                    {
                        PointerIntent::Filter
                    } else {
                        PointerIntent::Focus(*panel)
                    }
                } else {
                    PointerIntent::Focus(*panel)
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let by = if event.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                };
                if let Some(list) = list {
                    PointerIntent::Scroll {
                        panel: *panel,
                        offset: list.offset,
                        rows: list.rows.len(),
                        height: list.content.height as usize,
                        by,
                    }
                } else if matches!(panel, SearchPanel::Context | SearchPanel::Body) {
                    PointerIntent::Scroll {
                        panel: *panel,
                        offset: 0,
                        rows: 0,
                        height: 0,
                        by,
                    }
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        Some(Pointer {
            revision: self.revision,
            intent,
        })
    }
}

/// Case-insensitive subsequences, scored at path boundaries and in the filename.
/// Every whitespace-separated term must match. Positions index original characters.
#[derive(Debug, Clone, Default)]
pub struct PathMatch {
    pub score: i64,
    pub positions: Vec<usize>,
}

impl PathMatch {
    pub fn find(path: &str, query: &str) -> Option<Self> {
        let mut result = Self::default();
        for word in query.split_whitespace() {
            let matched = Self::word(path, word)?;
            result.score += matched.score;
            result.positions.extend(matched.positions);
        }
        result.positions.sort_unstable();
        result.positions.dedup();
        Some(result)
    }

    fn word(path: &str, query: &str) -> Option<Self> {
        let text: Vec<char> = path.chars().collect();
        let needle: Vec<String> = query.chars().map(|c| c.to_lowercase().collect()).collect();
        if needle.len() > text.len() || needle.is_empty() {
            return None;
        }
        let lower: Vec<String> = text.iter().map(|c| c.to_lowercase().collect()).collect();
        let filename = text.iter().rposition(|c| *c == '/').map_or(0, |i| i + 1);
        let mut previous: Vec<Option<i64>> = vec![None; text.len()];
        let mut parents = Vec::new();
        for (row, expected) in needle.iter().enumerate() {
            let mut scores = vec![None; text.len()];
            let mut parent = vec![None; text.len()];
            let mut best: Option<(i64, usize)> = None;
            for (i, actual) in lower.iter().enumerate() {
                if i > 0
                    && let Some(score) = previous[i - 1]
                {
                    let candidate = (score + 2 * (i - 1) as i64, i - 1);
                    if best.is_none_or(|b| candidate.0 > b.0) {
                        best = Some(candidate);
                    }
                }
                if actual != expected {
                    continue;
                }
                let boundary = i == 0
                    || !text[i - 1].is_alphanumeric()
                    || (text[i - 1].is_lowercase() && text[i].is_uppercase());
                let reward =
                    10 + if boundary { 12 } else { 0 } + if i >= filename { 12 } else { 0 };
                if row == 0 {
                    let offset = if i >= filename { i - filename } else { i };
                    scores[i] = Some(reward - offset as i64);
                } else if let Some((score, index)) = best {
                    let mut choice = (score - 2 * i.saturating_sub(1) as i64, index);
                    if i > 0
                        && let Some(score) = previous[i - 1]
                        && score + 16 > choice.0
                    {
                        choice = (score + 16, i - 1);
                    }
                    scores[i] = Some(choice.0 + reward);
                    parent[i] = Some(choice.1);
                }
            }
            previous = scores;
            parents.push(parent);
        }
        let (mut index, score) = previous
            .into_iter()
            .enumerate()
            .filter_map(|(i, score)| score.map(|score| (i, score)))
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))?;
        let mut positions = vec![index];
        for row in (1..needle.len()).rev() {
            index = parents[row][index]?;
            positions.push(index);
        }
        positions.reverse();
        Some(Self { score, positions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_paths_support_abbreviations_terms_case_and_original_unicode_positions() {
        let path = "crates/vvv/src/cli/commands/serve.rs";
        for query in ["srv", "CLI srv", "crvv srv", "", "  "] {
            assert!(PathMatch::find(path, query).is_some(), "{query}");
        }
        assert!(PathMatch::find(path, "srv worker").is_none());
        assert!(PathMatch::find(path, "zzzz").is_none());
        let matched = PathMatch::find("crates/语言/Éngine.rs", "éNG").unwrap();
        let text: Vec<_> = "crates/语言/Éngine.rs".chars().collect();
        let highlighted: String = matched.positions.iter().map(|&i| text[i]).collect();
        assert_eq!(highlighted, "Éng");
    }

    #[test]
    fn ranking_prefers_filename_and_contiguous_boundary_matches() {
        assert!(
            PathMatch::find("crates/vvv/src/context.rs", "ctx")
                .unwrap()
                .score
                > PathMatch::find("crates/context/src/other.rs", "ctx")
                    .unwrap()
                    .score
        );
        assert!(
            PathMatch::find("crates/a/very/long/path/to/context.rs", "ctx")
                .unwrap()
                .score
                > PathMatch::find("crates/context/src/other.rs", "ctx")
                    .unwrap()
                    .score
        );
        assert!(
            PathMatch::find("src/engine.rs", "eng").unwrap().score
                > PathMatch::find("src/end_of_group.rs", "eng").unwrap().score
        );
    }
}
