//! Workspace browsing independent of a search, with an outline from the displayed file.
pub(crate) mod screen;

use crate::action::{Action, Effect};
use crate::input::{Caret, Edit, TextInput};
use crate::model::{Cursor, FilePreview};
use crate::modes::search::SearchPanel;
use crate::modes::search::files::{ListViewport, PathMatch, Pointer, PointerIntent};
use crate::modes::search::inspection::{Inspection, InspectionKind};
use std::collections::BTreeMap;
use std::sync::Arc;
use vvv_engine::{NavigationQuery, RelPath, SourceAnchor, Span, Symbol};

#[derive(Debug, Clone, Default)]
pub struct WorkspaceBrowse {
    pub paths: Arc<Vec<RelPath>>,
    pub filter: String,
    pub caret: Caret,
    pub file: Option<RelPath>,
    pub preferred: Option<RelPath>,
    pub preview: Option<FilePreview>,
    pub loading: bool,
    pub inventory_loading: bool,
    /// Index in the complete outline, independent of the visible filter.
    pub outline: Cursor,
    pub scroll: usize,
    pub inspection: Inspection,
    pub expanded: bool,
    pub outline_filter: String,
    pub outline_caret: Caret,
    pub outline_edit: Option<OutlineEdit>,
    pub files_viewport: ListViewport,
    pub outline_viewport: ListViewport,
    pub revision: u64,
    preferred_symbol: Option<usize>,
    displayed_symbol: usize,
    positions: BTreeMap<RelPath, WorkspacePosition>,
}

#[derive(Debug, Clone)]
pub struct OutlineEdit {
    filter: String,
    outline: usize,
    preferred: Option<usize>,
    displayed: usize,
    scroll: usize,
    inspection: Inspection,
    viewport: ListViewport,
}

#[derive(Debug, Clone)]
struct WorkspacePosition {
    outline: usize,
    scroll: usize,
    inspection: Inspection,
    viewport: ListViewport,
}

impl WorkspaceBrowse {
    pub fn visible(&self) -> Vec<&RelPath> {
        let mut paths: Vec<_> = self
            .paths
            .iter()
            .filter_map(|path| {
                PathMatch::find(path.as_str(), &self.filter).map(|hit| (path, hit.score))
            })
            .collect();
        paths.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        paths.into_iter().map(|(path, _)| path).collect()
    }
    pub fn visible_symbols(&self) -> Vec<(usize, &Symbol)> {
        self.symbols()
            .iter()
            .enumerate()
            .filter(|(_, symbol)| PathMatch::find(&symbol.name, &self.outline_filter).is_some())
            .collect()
    }
    pub fn retained_bytes(&self) -> usize {
        self.paths
            .iter()
            .map(|p| p.as_str().len() + 32)
            .sum::<usize>()
            + self.preview.as_ref().map_or(0, FilePreview::retained_bytes)
            + self.filter.len()
            + self.outline_filter.len()
            + self.inspection.retained_bytes()
            + self
                .positions
                .iter()
                .map(|(p, position)| p.as_str().len() + 64 + position.inspection.retained_bytes())
                .sum::<usize>()
            + self.outline_edit.as_ref().map_or(0, |edit| {
                edit.filter.len() + edit.inspection.retained_bytes() + 128
            })
    }
    pub fn install(&mut self, paths: Vec<RelPath>) -> Vec<Effect> {
        self.paths = Arc::new(paths);
        self.inventory_loading = false;
        self.revision = self.revision.wrapping_add(1);
        self.files_viewport.reveal = true;
        self.reconcile()
    }
    pub fn input_focused(&self, focus: SearchPanel) -> bool {
        focus == SearchPanel::Query
            || (focus == SearchPanel::Results && self.outline_edit.is_some())
            || (focus == SearchPanel::Context && self.inspection.edit.is_some())
    }
    pub fn focus_changed(&mut self, focus: SearchPanel) {
        if focus != SearchPanel::Results {
            self.outline_edit = None;
        }
        if focus != SearchPanel::Context {
            self.inspection.edit = None;
            self.expanded = false;
        }
    }
    pub fn edit_input(&mut self, edit: Edit<'_>, focus: SearchPanel) -> Vec<Effect> {
        if focus == SearchPanel::Context {
            if let Some(preview) = &self.preview {
                let range = Span::new(0, preview.text().len());
                if let Some(line) = self.inspection.edit_input(edit, preview, range) {
                    self.scroll = line;
                }
            }
            return Vec::new();
        }
        if focus == SearchPanel::Results && self.outline_edit.is_some() {
            if self.preferred_symbol.is_none() {
                self.preferred_symbol = Some(self.outline.index);
            }
            if TextInput::new(&mut self.outline_filter, &mut self.outline_caret).apply(edit) {
                self.reconcile_outline();
                self.revision = self.revision.wrapping_add(1);
            }
            return Vec::new();
        }
        self.edit(edit)
    }
    pub fn edit(&mut self, edit: Edit<'_>) -> Vec<Effect> {
        if edit.motion() {
            TextInput::new(&mut self.filter, &mut self.caret).apply(edit);
            return Vec::new();
        }
        if self.preferred.is_none() {
            self.preferred = self.file.clone();
        }
        if !TextInput::new(&mut self.filter, &mut self.caret).apply(edit) {
            return Vec::new();
        }
        self.revision = self.revision.wrapping_add(1);
        self.files_viewport.reveal = true;
        self.reconcile()
    }
    fn reconcile(&mut self) -> Vec<Effect> {
        let paths = self.visible();
        let path = self
            .preferred
            .as_ref()
            .filter(|p| paths.contains(p))
            .or_else(|| self.file.as_ref().filter(|p| paths.contains(p)))
            .or_else(|| paths.first().copied())
            .cloned();
        self.select(path)
    }
    fn select(&mut self, path: Option<RelPath>) -> Vec<Effect> {
        if self.file == path {
            return Vec::new();
        }
        if let Some(file) = &self.file
            && !self.loading
            && self
                .preview
                .as_ref()
                .is_some_and(|preview| preview.path == *file)
        {
            self.positions.insert(
                file.clone(),
                WorkspacePosition {
                    outline: self.preferred_symbol.unwrap_or(self.outline.index),
                    scroll: self.scroll,
                    inspection: self.inspection.clone(),
                    viewport: self.outline_viewport,
                },
            );
        }
        self.outline_edit = None;
        self.file = path;
        if self.file.is_none() {
            self.preview = None;
            self.inspection = Inspection::default();
        }
        self.outline.index = self
            .file
            .as_ref()
            .and_then(|p| self.positions.get(p).map(|position| position.outline))
            .unwrap_or_default();
        self.preferred_symbol = None;
        self.loading = self.file.is_some();
        self.files_viewport.reveal = true;
        self.revision = self.revision.wrapping_add(1);
        self.preview_effect()
    }
    pub fn moved(&mut self, by: i32, files: bool) -> Vec<Effect> {
        if files {
            let paths = self.visible();
            if paths.is_empty() {
                return Vec::new();
            }
            let mut cursor = Cursor {
                index: paths
                    .iter()
                    .position(|p| Some(*p) == self.file.as_ref())
                    .unwrap_or(0),
            };
            cursor.move_by(by, paths.len());
            let path = paths.get(cursor.index).map(|p| (*p).clone());
            self.preferred = path.clone();
            self.files_viewport.reveal = true;
            self.select(path)
        } else {
            let symbols = self.visible_symbols();
            let mut cursor = Cursor {
                index: symbols
                    .iter()
                    .position(|(i, _)| *i == self.outline.index)
                    .unwrap_or(0),
            };
            cursor.move_by(by, symbols.len());
            let index = symbols.get(cursor.index).map(|(i, _)| *i);
            if let Some(index) = index {
                self.preferred_symbol = Some(index);
                self.select_symbol(index);
            }
            self.outline_viewport.reveal = true;
            Vec::new()
        }
    }
    fn select_symbol(&mut self, index: usize) {
        self.outline.index = index;
        self.displayed_symbol = index;
        self.inspection.line = None;
        self.scroll = self
            .symbol()
            .and_then(|s| self.preview.as_ref()?.lines_in(s.span))
            .map_or(self.scroll, |lines| lines.start.saturating_sub(3));
    }
    fn reconcile_outline(&mut self) {
        let symbols = self.visible_symbols();
        let index = self
            .preferred_symbol
            .filter(|i| symbols.iter().any(|(index, _)| index == i))
            .or_else(|| {
                symbols
                    .iter()
                    .find(|(i, _)| *i == self.outline.index)
                    .map(|(i, _)| *i)
            })
            .or_else(|| symbols.first().map(|(i, _)| *i));
        if let Some(index) = index {
            if index != self.outline.index {
                self.select_symbol(index);
            }
            self.displayed_symbol = index;
        } else {
            self.displayed_symbol = usize::MAX;
        }
        self.outline_viewport.reveal = true;
    }
    pub fn begin_outline_filter(&mut self) {
        if self.outline_edit.is_none() {
            self.outline_caret.reset();
            self.outline_edit = Some(OutlineEdit {
                filter: self.outline_filter.clone(),
                outline: self.outline.index,
                preferred: self.preferred_symbol,
                displayed: self.displayed_symbol,
                scroll: self.scroll,
                inspection: self.inspection.clone(),
                viewport: self.outline_viewport,
            });
        }
    }
    pub fn cancel_edit(&mut self, focus: SearchPanel) -> bool {
        if focus == SearchPanel::Results
            && let Some(edit) = self.outline_edit.take()
        {
            self.outline_filter = edit.filter;
            self.outline.index = edit.outline;
            self.preferred_symbol = edit.preferred;
            self.displayed_symbol = edit.displayed;
            self.scroll = edit.scroll;
            self.inspection = edit.inspection;
            self.outline_viewport = edit.viewport;
            self.revision = self.revision.wrapping_add(1);
            return true;
        }
        if focus == SearchPanel::Context && self.inspection.edit.is_some() {
            self.inspect(Action::Back);
            return true;
        }
        if self.expanded {
            self.expanded = false;
            return true;
        }
        false
    }
    pub fn symbols(&self) -> &[Symbol] {
        self.preview
            .as_ref()
            .filter(|p| !self.loading && Some(&p.path) == self.file.as_ref())
            .map_or(&[], |p| &p.symbols)
    }
    pub fn symbol(&self) -> Option<&Symbol> {
        self.symbols()
            .get(self.outline.index)
            .filter(|symbol| PathMatch::find(&symbol.name, &self.outline_filter).is_some())
    }
    pub fn site(&self) -> Option<(RelPath, u32)> {
        let line = self
            .symbol()
            .and_then(|s| self.preview.as_ref()?.lines_in(s.span))
            .map_or(0, |r| r.start as u32);
        self.file.clone().map(|path| (path, line))
    }
    pub fn displayed_site(&self) -> Option<(RelPath, u32)> {
        let preview = self.preview.as_ref()?;
        let line = self.inspection.line.unwrap_or_else(|| {
            preview
                .symbols
                .get(self.displayed_symbol)
                .and_then(|symbol| preview.lines_in(symbol.name_span))
                .map_or(self.scroll, |lines| lines.start)
        });
        Some((preview.path.clone(), line as u32))
    }
    pub fn query(&self) -> Option<NavigationQuery> {
        let preview = self.preview.as_ref().filter(|_| !self.loading)?;
        let span = self.symbol()?.name_span;
        Some(NavigationQuery::occurrence(SourceAnchor {
            path: preview.path.clone(),
            content: preview.content_id().clone(),
            span,
        }))
    }
    pub fn preview_effect(&self) -> Vec<Effect> {
        if self.loading {
            self.file
                .clone()
                .map(|path| Effect::Preview { path })
                .into_iter()
                .collect()
        } else {
            Vec::new()
        }
    }
    pub fn previewed(&mut self, preview: FilePreview) {
        if Some(&preview.path) != self.file.as_ref() {
            return;
        }
        let changed_file = self
            .preview
            .as_ref()
            .is_none_or(|shown| shown.path != preview.path);
        if changed_file {
            if let Some(position) = self.positions.get(&preview.path) {
                self.scroll = position.scroll;
                self.inspection = position.inspection.clone();
                self.outline_viewport = position.viewport;
            } else {
                self.scroll = 0;
                self.inspection = Inspection::default();
                self.outline_viewport = ListViewport::default();
            }
        }
        self.inspection
            .sync(&preview, Span::new(0, preview.text().len()));
        self.loading = false;
        self.preview = Some(preview);
        self.outline.index = self
            .outline
            .index
            .min(self.symbols().len().saturating_sub(1));
        self.preferred_symbol = Some(self.outline.index);
        self.displayed_symbol = self.outline.index;
        self.reconcile_outline();
        self.scroll = self.scroll.min(
            self.preview
                .as_ref()
                .map_or(0, |p| p.line_count().saturating_sub(1)),
        );
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn scrolled(&mut self, by: i32) {
        let max = self
            .preview
            .as_ref()
            .map_or(0, |p| p.line_count().saturating_sub(1));
        self.scroll = self.scroll.saturating_add_signed(by as isize).min(max);
    }
    pub fn marked(&self) -> Option<Span> {
        self.preview
            .as_ref()?
            .symbols
            .get(self.displayed_symbol)
            .map(|s| s.name_span)
    }
    pub fn inspect(&mut self, action: Action) {
        if action == Action::ExpandPreview {
            self.expanded = !self.expanded;
            return;
        }
        let Some(preview) = &self.preview else {
            return;
        };
        let range = Span::new(0, preview.text().len());
        self.inspection.sync(preview, range);
        let next = match action {
            Action::InspectFind | Action::InspectLine => {
                self.inspection.begin(
                    if action == Action::InspectFind {
                        InspectionKind::Find
                    } else {
                        InspectionKind::Line
                    },
                    self.scroll,
                );
                None
            }
            Action::Enter => self.inspection.accept(0..preview.line_count()),
            Action::Back => self.inspection.cancel(preview, range),
            Action::InspectNext(by) => self.inspection.step(by, preview, self.scroll),
            Action::InspectHorizontal(by) => {
                self.inspection.horizontal_by(by);
                None
            }
            Action::InspectStart => {
                self.inspection.horizontal = 0;
                None
            }
            _ => None,
        };
        if let Some(line) = next {
            self.scroll = line;
        }
    }
    pub fn pointer(&mut self, pointer: Pointer, focus: &mut SearchPanel) -> Vec<Effect> {
        if pointer.revision != self.revision {
            return Vec::new();
        }
        match pointer.intent {
            PointerIntent::File(path) => {
                if !self.visible().contains(&&path) {
                    return Vec::new();
                }
                *focus = SearchPanel::Files;
                self.focus_changed(*focus);
                self.preferred = Some(path.clone());
                return self.select(Some(path));
            }
            PointerIntent::Outline {
                path,
                content,
                span,
            } => {
                let Some(preview) = &self.preview else {
                    return Vec::new();
                };
                if preview.path != path || *preview.content_id() != content {
                    return Vec::new();
                }
                let Some(index) = self
                    .visible_symbols()
                    .iter()
                    .find(|(_, s)| s.name_span == span)
                    .map(|(i, _)| *i)
                else {
                    return Vec::new();
                };
                *focus = SearchPanel::Results;
                self.focus_changed(*focus);
                self.preferred_symbol = Some(index);
                self.select_symbol(index);
                self.outline_viewport.reveal = true;
            }
            PointerIntent::Filter => {
                *focus = SearchPanel::Results;
                self.begin_outline_filter();
            }
            PointerIntent::Focus(panel) => {
                if panel == SearchPanel::Body {
                    return Vec::new();
                }
                *focus = panel;
                self.focus_changed(panel);
            }
            PointerIntent::Scroll {
                panel,
                offset,
                rows,
                height,
                by,
            } => match panel {
                SearchPanel::Files => {
                    self.files_viewport.offset = offset;
                    self.files_viewport.scroll(by, rows, height);
                }
                SearchPanel::Results => {
                    self.outline_viewport.offset = offset;
                    self.outline_viewport.scroll(by, rows, height);
                }
                SearchPanel::Context => self.scrolled(by),
                _ => {}
            },
            PointerIntent::Match(_) => {}
        }
        Vec::new()
    }
    pub fn refresh(&mut self, generation: u64) -> Vec<Effect> {
        self.loading = self.file.is_some();
        self.inventory_loading = true;
        let mut effects = vec![Effect::WorkspaceFiles { generation }];
        effects.extend(self.preview_effect());
        effects
    }
    pub fn update(&mut self, action: Action, files: bool, source: bool) -> Vec<Effect> {
        match action {
            Action::FilterOutline => {
                self.begin_outline_filter();
                Vec::new()
            }
            Action::Clear if !files && !source => {
                self.outline_filter.clear();
                self.preferred_symbol.get_or_insert(self.outline.index);
                self.reconcile_outline();
                self.revision = self.revision.wrapping_add(1);
                Vec::new()
            }
            Action::ClearFileFilter => self.edit(Edit::Clear),
            Action::Enter if self.outline_edit.is_some() => {
                self.outline_edit = None;
                Vec::new()
            }
            Action::Enter if source && self.inspection.edit.is_some() => {
                self.inspect(action);
                Vec::new()
            }
            Action::InspectFind
            | Action::InspectLine
            | Action::InspectNext(_)
            | Action::InspectHorizontal(_)
            | Action::InspectStart
            | Action::ExpandPreview
                if source =>
            {
                self.inspect(action);
                Vec::new()
            }
            Action::Move(n) | Action::Scroll(n) if source => {
                self.scrolled(n);
                Vec::new()
            }
            Action::File(n) => self.moved(n, true),
            Action::Move(n) | Action::Scroll(n) => self.moved(n, files),
            Action::Top if source => {
                self.scroll = 0;
                Vec::new()
            }
            Action::Bottom if source => {
                self.scrolled(i32::MAX);
                Vec::new()
            }
            Action::Top => self.moved(i32::MIN / 2, files),
            Action::Bottom => self.moved(i32::MAX / 2, files),
            _ => Vec::new(),
        }
    }
}
