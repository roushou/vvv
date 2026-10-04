//! Workspace browsing independent of a search, with an outline from the displayed file.
pub(crate) mod screen;

use crate::action::{Action, Effect};
use crate::input::{Caret, Edit, TextInput};
use crate::model::{Cursor, FilePreview};
use crate::modes::search::files::PathMatch;
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
    pub outline: Cursor,
    pub scroll: usize,
    displayed_symbol: usize,
    positions: BTreeMap<RelPath, (usize, usize)>,
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
    pub fn retained_bytes(&self) -> usize {
        self.paths
            .iter()
            .map(|p| p.as_str().len() + 32)
            .sum::<usize>()
            + self.preview.as_ref().map_or(0, FilePreview::retained_bytes)
            + self.filter.len()
            + self
                .positions
                .keys()
                .map(|p| p.as_str().len() + 32)
                .sum::<usize>()
    }
    pub fn install(&mut self, paths: Vec<RelPath>) -> Vec<Effect> {
        self.paths = Arc::new(paths);
        self.inventory_loading = false;
        self.reconcile()
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
            self.positions
                .insert(file.clone(), (self.outline.index, self.scroll));
        }
        self.file = path;
        if self.file.is_none() {
            self.preview = None;
        }
        self.outline.index = self
            .file
            .as_ref()
            .and_then(|p| self.positions.get(p).map(|position| position.0))
            .unwrap_or_default();
        self.loading = self.file.is_some();
        self.preview_effect()
    }
    pub fn moved(&mut self, by: i32, files: bool) -> Vec<Effect> {
        if files {
            let paths = self.visible();
            let mut cursor = Cursor {
                index: paths
                    .iter()
                    .position(|p| Some(*p) == self.file.as_ref())
                    .unwrap_or(0),
            };
            cursor.move_by(by, paths.len());
            let path = paths.get(cursor.index).map(|p| (*p).clone());
            self.preferred = path.clone();
            self.select(path)
        } else {
            self.outline.move_by(by, self.symbols().len());
            self.displayed_symbol = self.outline.index;
            self.scroll = self
                .symbol()
                .and_then(|s| self.preview.as_ref()?.lines_in(s.span))
                .map_or(0, |lines| lines.start.saturating_sub(3));
            Vec::new()
        }
    }
    pub fn symbols(&self) -> &[Symbol] {
        self.preview
            .as_ref()
            .filter(|p| !self.loading && Some(&p.path) == self.file.as_ref())
            .map_or(&[], |p| &p.symbols)
    }
    pub fn symbol(&self) -> Option<&Symbol> {
        self.symbols().get(self.outline.index)
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
        let line = preview
            .symbols
            .get(self.displayed_symbol)
            .and_then(|symbol| preview.lines_in(symbol.name_span))
            .map_or(self.scroll, |lines| lines.start);
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
        if self
            .preview
            .as_ref()
            .is_none_or(|shown| shown.path != preview.path)
        {
            self.scroll = self
                .positions
                .get(&preview.path)
                .map_or(0, |position| position.1);
        }
        self.loading = false;
        self.preview = Some(preview);
        self.outline.index = self
            .outline
            .index
            .min(self.symbols().len().saturating_sub(1));
        self.displayed_symbol = self.outline.index;
        self.scroll = self.scroll.min(
            self.preview
                .as_ref()
                .map_or(0, |p| p.line_count().saturating_sub(1)),
        );
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
    pub fn refresh(&mut self, generation: u64) -> Vec<Effect> {
        self.loading = self.file.is_some();
        self.inventory_loading = true;
        let mut effects = vec![Effect::WorkspaceFiles { generation }];
        effects.extend(self.preview_effect());
        effects
    }
    pub fn update(&mut self, action: Action, files: bool, source: bool) -> Vec<Effect> {
        match action {
            Action::Move(n) | Action::File(n) => self.moved(n, files),
            Action::Scroll(n) if source => {
                self.scrolled(n);
                Vec::new()
            }
            Action::Scroll(n) => self.moved(n, files),
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
