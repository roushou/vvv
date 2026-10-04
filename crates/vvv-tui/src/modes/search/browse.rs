//! Explicit browsing, request identity, and bounded immutable page snapshots.
use super::{Locations, Results, Search, SearchPanel, SourceAnchor, body::Body, query::QueryBar};
use crate::model::FilePreview;
use std::collections::VecDeque;
use vvv_engine::{NavigationQuery, SymbolRef};

#[derive(Debug, Clone, Default)]
pub enum BrowsePage {
    #[default]
    Search,
    Workspace,
    Definition(SymbolRef),
    References,
}

/// Only browsing data is retained; executable mutation plans never enter the trail.
#[derive(Debug, Clone)]
pub struct NavigationEntry {
    workspace: Option<crate::modes::workspace::WorkspaceBrowse>,
    page: BrowsePage,
    query: QueryBar,
    locations: Locations,
    results: Results,
    focus: SearchPanel,
    preview: Option<FilePreview>,
    preview_dirty: bool,
    source_anchor: Option<SourceAnchor>,
    body: Body,
    definition_tab: bool,
    preview_scroll: Option<usize>,
    inspection: super::inspection::Inspection,
    expanded: Option<SearchPanel>,
    bytes: usize,
}

impl NavigationEntry {
    pub fn label(&self) -> String {
        if let Some(workspace) = &self.workspace {
            let path = workspace.file.as_ref().map_or("files", |p| p.as_str());
            return if workspace.filter.is_empty() {
                format!("Workspace: {path}")
            } else {
                format!("Workspace: {path} · files: {}", workspace.filter)
            };
        }
        let context = match self.page {
            BrowsePage::Workspace => format!(
                "Workspace: {}",
                self.workspace
                    .as_ref()
                    .and_then(|w| w.file.as_ref())
                    .map_or("files", |p| p.as_str())
            ),
            BrowsePage::Search => format!("Search: {}", self.query.text().trim()),
            BrowsePage::Definition(_) => format!(
                "Definition: {}",
                self.results
                    .current()
                    .and_then(|m| m.symbol.as_ref())
                    .map_or("", |s| s.name.as_str())
            ),
            BrowsePage::References => format!(
                "{}: {}",
                self.results.relation.label(),
                self.query.text().trim()
            ),
        };
        let location = self
            .results
            .current_site()
            .map(|(p, l)| format!("{p}:{}", l + 1))
            .unwrap_or_else(|| {
                self.locations
                    .selected
                    .as_ref()
                    .map_or("workspace".into(), ToString::to_string)
            });
        let mut label = format!("{context} · {location}");
        if let Some(scope) = &self.locations.selected {
            label.push_str(&format!(" · in: {scope}"));
        }
        if self.results.category != super::Category::All {
            label.push_str(&format!(" · {}", self.results.category.label()));
        }
        if !self.results.files.filter.is_empty() {
            label.push_str(&format!(" · files: {}", self.results.files.filter));
        }
        label
    }
    pub fn capture(search: &Search) -> Self {
        // Charge shared payloads on every entry. This deliberately overcounts
        // shared allocations and bounds retention without an ownership registry.
        let mut bytes = 1024 + search.query.text().len() + search.locations.retained_bytes();
        for preview in [search.preview.as_ref(), search.body.preview.as_ref()]
            .into_iter()
            .flatten()
        {
            bytes += preview.retained_bytes();
        }
        bytes += search.results.retained_bytes();
        bytes += search.inspection.retained_bytes() + search.body.inspection.retained_bytes();
        bytes += search
            .source_anchor
            .as_ref()
            .map_or(0, |a| a.path.as_str().len() + 128);
        bytes += search
            .workspace
            .as_ref()
            .map_or(0, crate::modes::workspace::WorkspaceBrowse::retained_bytes);
        Self {
            workspace: search.workspace.clone(),
            page: search.page.clone(),
            query: search.query.clone(),
            locations: search.locations.clone(),
            results: search.results.clone(),
            focus: search.focus,
            preview: search.preview.clone(),
            preview_dirty: search.preview_dirty,
            source_anchor: search.source_anchor.clone(),
            body: search.body.clone(),
            definition_tab: search.definition_tab,
            preview_scroll: search.preview_scroll,
            inspection: search.inspection.clone(),
            expanded: search.expanded,
            bytes,
        }
    }
    pub fn restore(self, search: &mut Search) {
        let ticket = search.body.next_ticket();
        let viewport = search.body.viewport;
        search.workspace = self.workspace;
        search.page = self.page;
        search.query = self.query;
        search.locations = self.locations;
        search.results = self.results;
        search.focus = self.focus;
        search.preview = self.preview;
        search.preview_dirty = self.preview_dirty;
        search.source_anchor = self.source_anchor;
        search.body = self.body;
        search.definition_tab = self.definition_tab;
        search.body.reticket(ticket);
        search.body.viewport = viewport;
        search.preview_scroll = self.preview_scroll;
        search.inspection = self.inspection;
        search.expanded = self.expanded;
        search.stale = true;
    }
}

#[derive(Debug)]
pub struct PendingFollow {
    pub ticket: u64,
    pub query: NavigationQuery,
    pub restoring: bool,
}

#[derive(Debug)]
pub struct NavigationTrail {
    back: VecDeque<NavigationEntry>,
    forward: VecDeque<NavigationEntry>,
    serial: u64,
    pub pending: Option<PendingFollow>,
    max_entries: usize,
    max_bytes: usize,
}
impl Default for NavigationTrail {
    fn default() -> Self {
        Self {
            back: VecDeque::new(),
            forward: VecDeque::new(),
            serial: 0,
            pending: None,
            max_entries: 64,
            max_bytes: 16 * 1024 * 1024,
        }
    }
}
impl NavigationTrail {
    pub fn position(&self) -> (usize, usize) {
        (
            self.back.len() + 1,
            self.back.len() + self.forward.len() + 1,
        )
    }
    pub fn locations(&self, search: &Search) -> Vec<(i32, String)> {
        self.back
            .iter()
            .enumerate()
            .map(|(i, e)| (i as i32 - self.back.len() as i32, e.label()))
            .chain(std::iter::once((
                0,
                NavigationEntry::capture(search).label(),
            )))
            .chain(
                self.forward
                    .iter()
                    .rev()
                    .enumerate()
                    .map(|(i, e)| (i as i32 + 1, e.label())),
            )
            .collect()
    }
    pub fn can_travel(&self, forward: bool) -> bool {
        if forward {
            !self.forward.is_empty()
        } else {
            !self.back.is_empty()
        }
    }
    pub fn cancel(&mut self) {
        self.pending = None;
    }
    pub fn request(&mut self, query: NavigationQuery, restoring: bool) -> crate::action::Effect {
        self.serial = self.serial.wrapping_add(1);
        self.pending = Some(PendingFollow {
            ticket: self.serial,
            query: query.clone(),
            restoring,
        });
        crate::action::Effect::Follow {
            ticket: self.serial,
            query,
        }
    }
    pub fn accept(&mut self, ticket: u64, query: &NavigationQuery) -> Option<PendingFollow> {
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.ticket == ticket && &p.query == query)
        {
            self.pending.take()
        } else {
            None
        }
    }
    pub fn commit(&mut self, entry: NavigationEntry) {
        self.forward.clear();
        self.back.push_back(entry);
        self.trim();
    }
    pub fn travel(&mut self, mut current: NavigationEntry, steps: i32) -> Option<NavigationEntry> {
        let forward = steps > 0;
        let available = if forward {
            self.forward.len()
        } else {
            self.back.len()
        };
        if steps == 0 || steps.unsigned_abs() as usize > available {
            return None;
        }
        self.cancel();
        // Move the whole route before trimming: a large departing page must
        // not evict the selected destination halfway through a direct jump.
        for _ in 0..steps.unsigned_abs() {
            current = if forward {
                let entry = self.forward.pop_back()?;
                self.back.push_back(current);
                entry
            } else {
                let entry = self.back.pop_back()?;
                self.forward.push_back(current);
                entry
            };
        }
        self.trim();
        Some(current)
    }
    fn trim(&mut self) {
        while self.back.len() + self.forward.len() > self.max_entries
            || self
                .back
                .iter()
                .chain(&self.forward)
                .map(|e| e.bytes)
                .sum::<usize>()
                > self.max_bytes
        {
            if self.back.pop_front().is_none() {
                self.forward.pop_front();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entry_and_byte_limits_evict_oldest_pages_even_when_payloads_are_shared() {
        let mut trail = NavigationTrail {
            max_entries: 2,
            ..NavigationTrail::default()
        };
        let mut search = Search::default();
        for c in ['a', 'b', 'c'] {
            search.query.push(c);
            trail.commit(NavigationEntry::capture(&search));
        }
        assert_eq!(trail.back.len(), 2);
        assert_eq!(trail.back.front().unwrap().query.text(), "ab");
        let one = trail.back.back().unwrap().bytes;
        trail.max_bytes = one;
        trail.trim();
        assert_eq!(trail.back.len(), 1);
        trail.max_bytes = 1;
        trail.trim();
        assert!(trail.back.is_empty());
        // A page larger than the entire retention budget remains usable as
        // the live page but is not retained when it is left.
        trail.commit(NavigationEntry::capture(&search));
        assert!(trail.back.is_empty());
    }
    #[test]
    fn oversized_source_is_not_retained_and_eviction_releases_shared_results() {
        let search = Search {
            preview: Some(FilePreview::new(vvv_engine::File {
                path: "large.rs".into(),
                text: "x".repeat(4096),
                highlights: vec![],
                symbols: vec![],
                identifiers: vec![],
            })),
            ..Search::default()
        };
        let mut trail = NavigationTrail {
            max_bytes: 1024,
            ..NavigationTrail::default()
        };
        trail.commit(NavigationEntry::capture(&search));
        assert!(!trail.can_travel(false));
        assert_eq!(std::sync::Arc::strong_count(&search.results.matches), 1);
        trail.max_bytes = 1024 * 1024;
        trail.max_entries = 1;
        trail.commit(NavigationEntry::capture(&search));
        trail.commit(NavigationEntry::capture(&search));
        assert_eq!(std::sync::Arc::strong_count(&search.results.matches), 2);
    }
    #[test]
    fn direct_jump_selects_its_destination_before_a_large_departing_page_trims_history() {
        let mut search = Search::default();
        let mut trail = NavigationTrail {
            max_bytes: 8192,
            ..NavigationTrail::default()
        };
        for name in ["First", "Second", "Third"] {
            search.query = QueryBar::from(name.to_owned());
            trail.commit(NavigationEntry::capture(&search));
        }
        search.preview = Some(FilePreview::new(vvv_engine::File {
            path: "large.rs".into(),
            text: "x".repeat(16000),
            highlights: vec![],
            symbols: vec![],
            identifiers: vec![],
        }));
        let target = trail.travel(NavigationEntry::capture(&search), -3).unwrap();
        assert_eq!(target.query.text(), "First");
        assert!(
            trail
                .back
                .iter()
                .chain(&trail.forward)
                .map(|e| e.bytes)
                .sum::<usize>()
                <= trail.max_bytes
        );
    }
    #[test]
    fn request_identity_rejects_duplicate_and_superseded_successes_and_failures() {
        let mut trail = NavigationTrail::default();
        let query = NavigationQuery::at("a.rs", vvv_engine::Position::new(0, 0));
        trail.request(query.clone(), false);
        let old = trail.pending.as_ref().unwrap().ticket;
        trail.request(query.clone(), false);
        let current = trail.pending.as_ref().unwrap().ticket;
        assert!(trail.accept(old, &query).is_none());
        assert!(trail.accept(current, &query).is_some());
        assert!(trail.accept(current, &query).is_none());
    }
}
