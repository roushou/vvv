//! The retained query hub and its read-only relations.
mod body;
pub(crate) mod files;
mod locations;
pub(crate) use locations::Locations;
pub(crate) mod browse;
pub(crate) mod filters;
pub(crate) mod inspection;
use browse::{BrowsePage, NavigationTrail};
pub(crate) mod recall;
use std::sync::Arc;
pub(crate) mod query;
pub(crate) mod screen;
use crate::action::{Action, Effect};
use crate::model::{Cursor, FilePreview, Panels};
use crate::modes::context::ModeContext;
use crate::modes::rename::RenameTarget;
use crate::overlays::MenuTarget;
use body::Body;
use query::Filter;
use query::QueryBar;
use vvv_engine::{Answer, DepsQuery, ExplainQuery, ImpactQuery, Request, Skipped};
use vvv_engine::{
    Confidence, Consumer, Deps, Explanation, Impact, Match, Occurrence, Query, References,
    ReferencesQuery, RelPath, Role,
};
// ---------------------------------------------------------------- search

/// The hub: a query, its results, and context for the cursor row.
#[derive(Debug, Default)]
pub struct Search {
    pub workspace: Option<crate::modes::workspace::WorkspaceBrowse>,
    pub problem: Option<crate::problem::Problem>,
    pub recent: recall::RecentSearches,
    pub trail: NavigationTrail,
    pub page: BrowsePage,
    pub stale: bool,
    pub query: QueryBar,
    pub locations: Locations,
    pub results: Results,
    pub focus: SearchPanel,
    pub preview: Option<FilePreview>,
    pub preview_dirty: bool,
    pub source_anchor: Option<SourceAnchor>,
    pub body: Body,
    /// The preview retained when only one pane fits; changed by preview focus.
    pub definition_tab: bool,
    /// A manual context scroll position; `None` follows the cursor.
    pub preview_scroll: Option<usize>,
    pub inspection: inspection::Inspection,
    pub expanded: Option<SearchPanel>,
}

impl Search {
    pub fn workspace(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if self.workspace.is_some() {
            return self.travel(false, context);
        }
        self.trail.cancel();
        self.trail.commit(browse::NavigationEntry::capture(self));
        self.workspace = Some(crate::modes::workspace::WorkspaceBrowse::default());
        self.page = BrowsePage::Workspace;
        self.focus = SearchPanel::Query;
        self.problem = None;
        self.stale = false;
        self.expanded = None;
        self.body.clear();
        context.status.clear();
        context.status.busy = true;
        self.workspace
            .as_mut()
            .unwrap()
            .refresh(context.next_generation())
    }

    pub fn remember_page(&mut self, busy: bool) {
        if busy || self.stale || self.query.is_empty() {
            return;
        }
        if let Some(recipe) = recall::SearchRecipe::capture(self) {
            self.recent.remember(recipe);
        }
        self.trail.commit(browse::NavigationEntry::capture(self));
    }
    pub fn reopen(
        &mut self,
        recipe: &recall::SearchRecipe,
        context: &mut ModeContext<'_>,
    ) -> Vec<Effect> {
        if !recipe.valid() {
            return Vec::new();
        }
        self.remember_page(context.status.busy);
        recipe.restore(self);
        let effects = self.search(context);
        self.results
            .set_category(Category::from_key(&recipe.category).expect("validated category"));
        effects
    }
    pub fn inspection_editing(&self) -> bool {
        match self.focus {
            SearchPanel::Context => self.inspection.edit.is_some(),
            SearchPanel::Body => self.body.inspection.edit.is_some(),
            _ => false,
        }
    }

    fn inspection_action(&mut self, action: Action) {
        use inspection::InspectionKind;
        if action == Action::ExpandPreview {
            self.expanded = if self.expanded == Some(self.focus) {
                None
            } else {
                Some(self.focus)
            };
            return;
        }
        let definition = self.focus == SearchPanel::Body;
        let source = if definition {
            self.body
                .declaration()
                .and_then(|d| self.body.symbol(d))
                .and_then(|s| self.body.preview.clone().map(|p| (p, s.span)))
        } else {
            self.displayed_source()
                .and_then(|_| self.preview.clone())
                .map(|p| {
                    let end = p.text().len();
                    (p, vvv_engine::Span::new(0, end))
                })
        };
        let Some((preview, range)) = source else {
            return;
        };
        let lines = if definition {
            preview.lines_in(range).unwrap_or(0..0)
        } else {
            0..preview.line_count()
        };
        let scroll = if definition {
            lines.start + self.body.scroll
        } else {
            self.preview_scroll.unwrap_or_else(|| self.preview_anchor())
        };
        let inspection = if definition {
            &mut self.body.inspection
        } else {
            &mut self.inspection
        };
        inspection.sync(&preview, range);
        let next = match action {
            Action::InspectFind | Action::InspectLine => {
                inspection.begin(
                    if action == Action::InspectFind {
                        InspectionKind::Find
                    } else {
                        InspectionKind::Line
                    },
                    scroll,
                );
                None
            }
            Action::Input(c) => inspection.input(Some(c), false, &preview, range),
            Action::Backspace => inspection.input(None, false, &preview, range),
            Action::Clear => inspection.input(None, true, &preview, range),
            Action::Enter => inspection.accept(lines.clone()),
            Action::Back => inspection.cancel(&preview, range),
            Action::InspectNext(by) => inspection.step(by, &preview, scroll),
            Action::InspectHorizontal(by) => {
                inspection.horizontal_by(by);
                None
            }
            Action::InspectStart => {
                inspection.horizontal = 0;
                None
            }
            _ => None,
        };
        if let Some(line) = next {
            if definition {
                self.body.scroll = line.saturating_sub(lines.start);
            } else {
                self.preview_scroll = Some(line);
            }
        }
    }

    pub fn input_focused(&self) -> bool {
        if let Some(workspace) = &self.workspace {
            return workspace.input_focused(self.focus);
        }
        self.focus == SearchPanel::Query
            || self.inspection_editing()
            || (self.focus == SearchPanel::Files && self.results.files.edit.is_some())
    }
    pub fn screen(&self) -> &'static crate::screen::Screen {
        if let Some(workspace) = &self.workspace {
            if self.focus == SearchPanel::Results && workspace.outline_edit.is_some() {
                return &crate::modes::workspace::screen::FILTER_OUTLINE;
            }
            if self.focus == SearchPanel::Context && workspace.inspection.edit.is_some() {
                return &crate::modes::workspace::screen::INSPECT_SOURCE;
            }
            return &crate::modes::workspace::screen::WORKSPACE;
        }
        if self.inspection_editing() {
            if self.focus == SearchPanel::Body {
                &screen::INSPECT_BODY
            } else {
                &screen::INSPECT_SOURCE
            }
        } else if self.results.files.edit.is_some() && self.focus == SearchPanel::Files {
            &screen::FILTER_SEARCH
        } else {
            &screen::SEARCH
        }
    }
    pub fn follow(
        &mut self,
        context: &mut ModeContext<'_>,
    ) -> (
        Option<crate::overlays::navigation::NavigationPicker>,
        Vec<Effect>,
    ) {
        use crate::overlays::navigation::NavigationPicker;
        if let Some(workspace) = &self.workspace {
            if workspace.loading {
                return (None, context.fail("Wait for the source to finish loading"));
            }
            if self.focus == SearchPanel::Context {
                if let Some(preview) = &workspace.preview {
                    let picker = NavigationPicker::identifiers(
                        preview,
                        preview.identifiers.iter().cloned(),
                        workspace.scroll,
                    );
                    if !picker.items.is_empty() {
                        return (Some(picker), Vec::new());
                    }
                }
                return (None, context.fail("No identifiers in this source"));
            }
            let Some(query) = workspace.query() else {
                return (None, context.fail("Select a declaration in the outline"));
            };
            return (None, self.follow_query(query, context));
        }

        if self.stale {
            context
                .status
                .info("Saved source needs validation or refresh (ctrl+r)");
            return (None, Vec::new());
        }
        if context.status.busy {
            return (None, context.fail("Wait for the current search to finish"));
        }
        match self.focus {
            SearchPanel::Body | SearchPanel::Context => {
                let preview = if self.focus == SearchPanel::Body {
                    self.body.preview.as_ref()
                } else {
                    self.preview.as_ref()
                };
                let Some(preview) = preview else {
                    return (None, context.fail("Source is not available yet"));
                };
                let range = if self.focus == SearchPanel::Body {
                    self.body
                        .declaration()
                        .and_then(|d| self.body.symbol(d))
                        .map(|s| s.extent)
                } else {
                    None
                };
                let anchors = preview
                    .identifiers
                    .iter()
                    .filter(|a| range.is_none_or(|r| r.contains(&a.span)))
                    .cloned();
                let line = if self.focus == SearchPanel::Body {
                    self.body
                        .declaration()
                        .and_then(|d| self.body.lines(d))
                        .map_or(0, |r| r.start)
                        + self.body.scroll
                } else {
                    self.preview_scroll.unwrap_or_else(|| self.preview_anchor())
                };
                let picker = NavigationPicker::identifiers(preview, anchors, line);
                if picker.items.is_empty() {
                    return (None, context.fail("No identifiers in this source"));
                }
                (Some(picker), Vec::new())
            }
            SearchPanel::Files | SearchPanel::Results => {
                let query = match &self.page {
                    BrowsePage::Definition(symbol) => Some(vvv_engine::NavigationQuery {
                        origin: vvv_engine::NavigationOrigin::Symbol {
                            symbol: symbol.clone(),
                        },
                        selection: vvv_engine::Selection::All,
                    }),
                    _ => self.body.query(),
                };
                let Some(query) = query else {
                    return (None, context.fail("Select a source row"));
                };
                (None, self.follow_query(query, context))
            }
            SearchPanel::Query => (None, Vec::new()),
        }
    }

    pub fn follow_query(
        &mut self,
        query: vvv_engine::NavigationQuery,
        context: &mut ModeContext<'_>,
    ) -> Vec<Effect> {
        context.next_generation();
        context.status.info("Following reference…");
        vec![self.trail.request(query, false)]
    }

    pub fn followed(
        &mut self,
        ticket: u64,
        query: vvv_engine::NavigationQuery,
        reply: Result<vvv_engine::NavigationReply, vvv_engine::Failure>,
        context: &mut ModeContext<'_>,
    ) -> Option<crate::overlays::navigation::NavigationPicker> {
        use vvv_engine::{NavigationOutcome, NavigationReply};
        let pending = self.trail.accept(ticket, &query)?;
        match reply {
            Ok(
                reply @ NavigationReply {
                    outcome: NavigationOutcome::Resolved { .. },
                    ..
                },
            ) => {
                let NavigationOutcome::Resolved {
                    target, preview, ..
                } = &reply.outcome
                else {
                    unreachable!()
                };
                if pending.restoring {
                    // Validation must confirm the displayed target as well as its origin.
                    if self.body.pending().is_none()
                        && self.body.target.as_ref().is_some_and(|old| old != target)
                    {
                        self.stale = true;
                        context
                            .status
                            .info("Saved definition changed; refresh with ctrl+r");
                        return None;
                    }
                    let scroll = self.body.scroll;
                    self.body.install(query, reply);
                    self.body.scroll = scroll;
                } else {
                    self.trail.commit(browse::NavigationEntry::capture(self));
                    self.workspace = None;
                    self.problem = None;
                    self.page = BrowsePage::Definition(target.clone());
                    self.results.category = Category::Declarations;
                    self.results.location = None;
                    self.results.files = files::FileNavigator::default();
                    self.results.replace(vec![preview.declaration.clone()]);
                    self.results.cursor = Cursor::default();
                    self.preview = None;
                    self.preview_scroll = None;
                    self.body.install(query, reply);
                    self.preview = self.body.preview.clone();
                    self.preview_dirty = false;
                    self.sync_source();
                    self.focus = SearchPanel::Body;
                    self.definition_tab = true;
                }
                self.stale = false;
                context.status.clear();
            }
            Ok(NavigationReply {
                outcome: NavigationOutcome::Ambiguous { candidates },
                ..
            }) if !pending.restoring => {
                context.status.clear();
                return Some(crate::overlays::navigation::NavigationPicker::candidates(
                    query, candidates,
                ));
            }
            Ok(NavigationReply {
                outcome: NavigationOutcome::Unavailable { reason },
                ..
            }) => {
                if pending.restoring && self.body.target.is_none() {
                    self.stale = false;
                }
                context.status.info(reason.message());
            }
            Ok(_) if pending.restoring && self.body.target.is_none() => {
                // An ambiguous origin is a valid saved page too. Validation
                // must not open a picker until the user explicitly follows it.
                self.stale = false;
                context.status.clear();
            }
            Ok(_) => {
                context
                    .status
                    .info("Saved definition changed; refresh with ctrl+r");
            }
            Err(failure) => {
                if failure.code == vvv_engine::ErrorCode::Stale {
                    self.stale = true;
                }
                let retry = Effect::Follow {
                    ticket,
                    query: query.clone(),
                };
                self.problem = Some(crate::problem::Problem::new(failure, Some(retry)));
                context.status.clear();
                self.focus = SearchPanel::Context;
                self.definition_tab = false;
            }
        }
        None
    }

    pub fn travel(&mut self, forward: bool, context: &mut ModeContext<'_>) -> Vec<Effect> {
        self.travel_steps(if forward { 1 } else { -1 }, context)
    }
    pub fn travel_steps(&mut self, steps: i32, context: &mut ModeContext<'_>) -> Vec<Effect> {
        let forward = steps > 0;
        self.trail.cancel();
        if steps == 0 || !self.trail.can_travel(forward) {
            return Vec::new();
        }
        let current = browse::NavigationEntry::capture(self);
        let Some(entry) = self.trail.travel(current, steps) else {
            return Vec::new();
        };
        context.next_generation();
        entry.restore(self);
        self.problem = None;
        if let Some(workspace) = &mut self.workspace {
            self.stale = false;
            context.status.clear();
            return workspace.refresh(*context.generation);
        }
        context.status.busy = false;
        context.status.info("Checking saved source…");
        if let Some(query) = self.body.query() {
            vec![self.trail.request(query, true)]
        } else {
            context.status.info("Refresh saved results with ctrl+r");
            Vec::new()
        }
    }

    pub fn search(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        self.trail.cancel();
        self.workspace = None;
        self.problem = None;
        self.page = BrowsePage::Search;
        self.body.problem = None;
        self.body.reticket(self.body.next_ticket());
        let generation = context.next_generation();
        match self.query.parse() {
            Ok(query) => {
                self.stale = true;
                if query.symbol().is_some() {
                    self.results.set_category(Category::Declarations);
                }
                context.status.busy = true;
                context.status.clear();
                self.results.query = Some(query.clone());
                vec![Effect::Search {
                    generation,
                    query,
                    scope: self.locations.scope(),
                }]
            }
            Err(e) => {
                self.stale = false;
                context.status.busy = false;
                self.results.replace(Vec::new());
                self.results.query = None;
                self.selection_changed();
                if self.query.is_empty() {
                    context.status.clear();
                } else {
                    context.status.error(e.to_string());
                }
                Vec::new()
            }
        }
    }
    /// Enter the current declaration without blanking the retained results.
    pub fn enter_subject(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        let Some(m) = self.results.current().cloned() else {
            return context.fail("put the cursor on a declaration to enter its scope");
        };
        let declaration = if m.role == Role::Declaration {
            Some(&m)
        } else if self.body.pending().is_none() {
            self.body.declaration()
        } else {
            None
        };
        let Some(query) = declaration.and_then(|d| self.results.subject_at(d)) else {
            return context.fail("put the cursor on a declaration to enter its scope");
        };
        self.remember_page(context.status.busy);
        context.status.busy = true;
        let generation = context.next_generation();
        vec![Effect::Query {
            generation,
            request: Request::References(query),
        }]
    }
    pub fn choose_menu(
        &mut self,
        (target, value): (MenuTarget, Option<String>),
        context: &mut ModeContext<'_>,
    ) -> Navigation {
        self.remember_page(context.status.busy);
        if target != MenuTarget::Relation {
            self.results.prefer_selection();
        }
        match target {
            MenuTarget::Filters => {
                let restrictions: Vec<_> = match value.as_deref() {
                    Some("all") => filters::Restriction::ALL.to_vec(),
                    Some(key) => filters::Restriction::from_key(key).into_iter().collect(),
                    None => Vec::new(),
                };
                let mut rerun = false;
                for restriction in restrictions {
                    if restriction.value(self).is_some() {
                        rerun |= restriction.clear(self);
                    }
                }
                if rerun {
                    Navigation::Effects(self.search(context))
                } else {
                    Navigation::Selection
                }
            }
            MenuTarget::Symbol => {
                self.query.set_filter(Filter::Symbol, value.as_deref());
                self.results.set_category(if value.is_some() {
                    Category::Declarations
                } else {
                    Category::All
                });
                Navigation::Effects(self.search(context))
            }
            MenuTarget::Language => {
                self.query.set_filter(Filter::Lang, value.as_deref());
                Navigation::Effects(self.search(context))
            }
            MenuTarget::Relation => {
                let relation = value
                    .as_deref()
                    .and_then(Relation::from_key)
                    .unwrap_or_default();
                self.choose_relation(relation, context)
            }
            MenuTarget::Category => {
                let anchored = self.results.is_anchored();
                let category = value
                    .as_deref()
                    .and_then(Category::from_key)
                    .unwrap_or_default();
                self.results.leave();
                self.results.set_category(category);
                let had_kind = category != Category::Declarations
                    && self.query.filter(Filter::Symbol).is_some();
                if had_kind {
                    self.query.set_filter(Filter::Symbol, None);
                }
                if anchored || had_kind {
                    Navigation::Effects(self.search(context))
                } else {
                    Navigation::Selection
                }
            }
            MenuTarget::Location => match self.locations.select(value.as_deref()) {
                Ok(()) => {
                    self.results.set_location(self.locations.selected.clone());
                    if self.results.is_anchored() && self.results.relation.is_references() {
                        Navigation::Selection
                    } else {
                        Navigation::Effects(self.search(context))
                    }
                }
                Err(error) => Navigation::Effects(context.fail(&error.to_string())),
            },
        }
    }
    pub fn choose_relation(
        &mut self,
        relation: Relation,
        context: &mut ModeContext<'_>,
    ) -> Navigation {
        match relation {
            Relation::Impact | Relation::Definition | Relation::Deps => {
                self.ask_relation(relation, context)
            }
            _ => {
                self.results.set_relation(relation);
                Navigation::Selection
            }
        }
    }
    pub fn ask_relation(
        &mut self,
        relation: Relation,
        context: &mut ModeContext<'_>,
    ) -> Navigation {
        let Some(subject) = self.results.subject.clone() else {
            return Navigation::Effects(Vec::new());
        };
        let Some(declaration) = self.results.subject_declaration().cloned() else {
            return Navigation::Effects(Vec::new());
        };
        let cached = match relation {
            Relation::Impact => self.results.impact.is_some(),
            Relation::Definition => self.results.definition.is_some(),
            Relation::Deps => self.results.deps.is_some(),
            _ => true,
        };
        if cached {
            self.results.set_relation(relation);
            return Navigation::Selection;
        }
        let request = match relation {
            Relation::Impact => Request::Impact(ImpactQuery {
                name: subject.name.clone(),
                declared_in: subject.declared_in.clone(),
            }),
            Relation::Definition => Request::Explain(ExplainQuery {
                path: declaration.path.clone(),
                position: declaration.start,
            }),
            Relation::Deps => Request::Deps(DepsQuery {
                path: declaration.path.clone(),
            }),
            _ => return Navigation::Effects(Vec::new()),
        };
        context.status.busy = true;
        let generation = context.next_generation();
        Navigation::Effects(vec![Effect::Query {
            generation,
            request,
        }])
    }
    pub fn focus_by(&mut self, by: i32) {
        self.results.files.edit = None;
        self.focus = self.focus.step(by);
        while !self.panel_available(self.focus) {
            self.focus = self.focus.step(by);
        }
        self.sync_preview_tab();
    }
    pub fn focus_nth(&mut self, n: u8) {
        self.results.files.edit = None;
        if let Some(p) = SearchPanel::nth(n)
            && self.panel_available(p)
        {
            self.focus = p;
            self.sync_preview_tab();
        }
    }
    fn panel_available(&self, panel: SearchPanel) -> bool {
        if self.workspace.is_some() {
            return panel != SearchPanel::Body;
        }
        match panel {
            SearchPanel::Files => self.results.has_file_list(),
            SearchPanel::Body => self.results.has_body(),
            _ => true,
        }
    }
    fn sync_preview_tab(&mut self) {
        if let Some(workspace) = &mut self.workspace {
            workspace.focus_changed(self.focus);
        }
        self.inspection.edit = None;
        self.body.inspection.edit = None;
        if self.expanded.is_some() {
            self.expanded = self.scroll_focused().then_some(self.focus);
        }
        match self.focus {
            SearchPanel::Body => self.definition_tab = true,
            SearchPanel::Context => self.definition_tab = false,
            _ => {}
        }
    }
    pub fn moved(&mut self, by: i32) {
        self.results.files.preferred = None;
        if self.focus == SearchPanel::Files {
            self.file_by(by);
            return;
        }
        self.results.remember_current();
        self.results.files.reveal();
        let len = self.results.len();
        let previous = self.results.cursor.index;
        if self.focus == SearchPanel::Results && self.results.has_file_list() {
            let groups = self.results.file_groups();
            let mut start = 0;
            if let Some(file) = groups.iter().find(|file| {
                let contains = previous >= start && previous < start + file.matches.len();
                if !contains {
                    start += file.matches.len();
                }
                contains
            }) {
                let next = (previous as i64 + i64::from(by))
                    .clamp(start as i64, (start + file.matches.len() - 1) as i64);
                self.results.cursor.index = next as usize;
            }
        } else {
            self.results.cursor.move_by(by, len);
        }
        if previous != self.results.cursor.index {
            self.selection_changed();
        }
    }
    pub fn file_by(&mut self, by: i32) {
        self.results.files.reveal();
        if self.results.move_file(by) {
            self.selection_changed();
        }
    }
    fn clear_preview_problem(&mut self) {
        if let Some(Effect::Preview { path }) = self.problem.as_ref().and_then(|p| p.retry.as_ref())
        {
            let selected = if let Some(workspace) = &self.workspace {
                workspace.file.clone()
            } else {
                self.results.current_site().map(|(path, _)| path)
            };
            if selected.as_ref() != Some(path) {
                self.problem = None;
            }
        }
    }
    pub fn selection_changed(&mut self) {
        self.clear_preview_problem();
        self.results.remember_current();
        self.results.files.reveal();
        self.trail.cancel();
        self.sync_source();
        self.body
            .select(self.results.current(), self.results.revision);
        if !self.panel_available(self.focus) {
            self.focus = SearchPanel::Results;
            self.sync_preview_tab();
        }
    }
    pub fn site(&self) -> Option<(RelPath, u32)> {
        if let Some(workspace) = &self.workspace {
            return if self.focus == SearchPanel::Context {
                workspace.displayed_site().or_else(|| workspace.site())
            } else {
                workspace.site()
            };
        }
        if self.focus == SearchPanel::Body {
            self.body.declaration().map(|d| {
                (
                    d.path.clone(),
                    self.body.inspection.line.map_or(d.start.line, |n| n as u32),
                )
            })
        } else if self.focus == SearchPanel::Context {
            self.displayed_source()
                .map(|anchor| {
                    (
                        anchor.path,
                        self.inspection.line.map_or(anchor.line, |n| n as u32),
                    )
                })
                .or_else(|| self.results.current_site())
        } else {
            self.results.current_site()
        }
    }
    /// Keep the displayed source coherent until another file's preview arrives.
    pub fn displayed_source(&self) -> Option<SourceAnchor> {
        let selected = self.results.source_anchor()?;
        let preview = self.preview.as_ref()?;
        if selected.path == preview.path && !self.preview_dirty {
            Some(selected)
        } else {
            self.source_anchor
                .as_ref()
                .filter(|anchor| anchor.path == preview.path)
                .cloned()
        }
    }
    fn sync_source(&mut self) {
        if let Some(anchor) = self.results.source_anchor()
            && self.preview.as_ref().is_some_and(|p| p.path == anchor.path)
            && !self.preview_dirty
        {
            if self.source_anchor.as_ref() != Some(&anchor) {
                self.preview_scroll = None;
                self.inspection.cursor = None;
                self.inspection.line = None;
            }
            if let Some(preview) = &self.preview {
                self.inspection
                    .sync(preview, vvv_engine::Span::new(0, preview.text().len()));
            }
            self.source_anchor = Some(anchor);
        }
    }
    pub fn scrolled(&mut self, by: i32) {
        if self.focus == SearchPanel::Body {
            self.body.scroll_by(by);
            return;
        }
        let max = self
            .preview
            .as_ref()
            .map_or(0, |p| p.line_count().saturating_sub(1));
        let current = self.preview_scroll.unwrap_or_else(|| self.preview_anchor());
        self.preview_scroll = Some((current as i32 + by).clamp(0, max as i32) as usize);
    }
    /// The line the context centres on when following the cursor.
    pub fn preview_anchor(&self) -> usize {
        self.displayed_source()
            .map_or(0, |anchor| (anchor.line as usize).saturating_sub(5))
    }
    pub fn preview_effect(&self) -> Vec<Effect> {
        if let Some(workspace) = &self.workspace {
            return workspace.preview_effect();
        }
        let mut effects = Vec::new();
        if let Some((ticket, query)) = self.body.pending() {
            effects.push(Effect::Definition { ticket, query });
        }
        let context = self.results.current_site().map(|(path, _)| path);
        if let Some(path) = &context
            && (self.preview_dirty || self.preview.as_ref().map(|p| &p.path) != Some(path))
        {
            effects.push(Effect::Preview { path: path.clone() });
        }
        effects
    }
    pub fn preview_target(&self) -> Option<RelPath> {
        if let Some(workspace) = &self.workspace {
            workspace.file.clone()
        } else {
            self.results.current_site().map(|(path, _)| path)
        }
    }
    pub fn scroll_focused(&self) -> bool {
        matches!(self.focus, SearchPanel::Context | SearchPanel::Body)
    }
    pub fn jump(&mut self, top: bool) -> Vec<Effect> {
        if self.focus == SearchPanel::Body {
            self.scrolled(if top { i32::MIN } else { i32::MAX });
            Vec::new()
        } else if self.scroll_focused() {
            self.preview_scroll = Some(if top {
                0
            } else {
                self.preview.as_ref().map_or(0, |p| p.line_count())
            });
            Vec::new()
        } else {
            self.moved(if top { i32::MIN / 2 } else { i32::MAX / 2 });
            self.preview_effect()
        }
    }
    pub fn back(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if let Some(workspace) = &mut self.workspace {
            if workspace.cancel_edit(self.focus) {
                return Vec::new();
            }
            return self.travel(false, context);
        }
        if self.inspection_editing() {
            self.inspection_action(Action::Back);
            return Vec::new();
        }
        if self.expanded.is_some() {
            self.expanded = None;
            return Vec::new();
        }
        if self.results.files.edit.is_some() {
            let (selected, focus) = self
                .results
                .files
                .cancel()
                .expect("editing the file filter");
            let viewport = self.results.files.viewport;
            let match_viewports = self.results.files.match_viewports.clone();
            self.results.restore_selection(selected);
            self.selection_changed();
            self.results.files.viewport = viewport;
            self.results.files.match_viewports = match_viewports;
            self.focus = focus;
            self.sync_preview_tab();
            return self.preview_effect();
        }
        match self.focus {
            SearchPanel::Files => {
                self.focus = SearchPanel::Query;
                return Vec::new();
            }
            SearchPanel::Results if self.results.has_file_list() => {
                self.focus = SearchPanel::Files;
                return Vec::new();
            }
            SearchPanel::Context | SearchPanel::Body => {
                self.focus = SearchPanel::Results;
                return Vec::new();
            }
            _ => {}
        }
        if self.results.is_anchored() {
            let changed_location = self.results.location != self.results.searched_location;
            self.results.leave();
            if changed_location {
                return self.search(context);
            }
            self.selection_changed();
            context.status.clear();
            self.preview_effect()
        } else {
            self.focus = SearchPanel::Query;
            Vec::new()
        }
    }
    pub fn pointer(
        &mut self,
        pointer: files::Pointer,
        context: &mut ModeContext<'_>,
    ) -> Vec<Effect> {
        use files::PointerIntent;
        if let Some(workspace) = &mut self.workspace {
            if pointer.revision != workspace.revision {
                return Vec::new();
            }
            if let PointerIntent::Scroll {
                panel: SearchPanel::Context,
                by,
                ..
            } = pointer.intent
                && let Some(problem) = &mut self.problem
            {
                problem.navigate(Action::Scroll(by));
                return Vec::new();
            }
            let effects = workspace.pointer(pointer, &mut self.focus);
            self.clear_preview_problem();
            return effects;
        }
        if pointer.revision != self.results.revision {
            return Vec::new();
        }
        match pointer.intent {
            PointerIntent::Scroll {
                panel,
                offset,
                rows,
                height,
                by,
            } => {
                match panel {
                    SearchPanel::Files => {
                        self.results.files.viewport.offset = offset;
                        self.results.files.viewport.scroll(by, rows, height);
                    }
                    SearchPanel::Results => {
                        if let Some(path) = self.results.current().map(|m| m.path.clone()) {
                            let viewport =
                                self.results.files.match_viewports.entry(path).or_default();
                            viewport.offset = offset;
                            viewport.scroll(by, rows, height);
                        }
                    }
                    SearchPanel::Context | SearchPanel::Body => {
                        let focus = self.focus;
                        self.focus = panel;
                        self.scrolled(by);
                        self.focus = focus;
                    }
                    _ => {}
                }
                return Vec::new();
            }
            PointerIntent::Outline { .. } => return Vec::new(),
            PointerIntent::Filter => return self.update(Action::FilterFiles, context),
            PointerIntent::File(path) => {
                self.results.files.preferred = None;
                let groups = self.results.file_groups();
                let Some(index) = groups.iter().position(|group| *group.path == path) else {
                    return Vec::new();
                };
                let id = self
                    .results
                    .files
                    .selected(&groups[index])
                    .map(|m| m.id.clone());
                self.results.restore_selection(id);
                self.focus = SearchPanel::Files;
                self.selection_changed();
            }
            PointerIntent::Match(id) => {
                self.results.files.preferred = None;
                if !self.results.listed().iter().any(|m| m.id == id) {
                    return Vec::new();
                }
                self.results.files.edit = None;
                self.results.restore_selection(Some(id));
                self.focus = SearchPanel::Results;
                self.selection_changed();
            }
            PointerIntent::Focus(panel) => {
                if !self.panel_available(panel) {
                    return Vec::new();
                }
                if panel != SearchPanel::Files {
                    self.results.files.edit = None;
                }
                self.focus = panel;
                self.sync_preview_tab();
            }
        }
        self.preview_effect()
    }

    pub fn edit_input(
        &mut self,
        edit: crate::input::Edit<'_>,
        context: &mut ModeContext<'_>,
    ) -> Vec<Effect> {
        if let Some(workspace) = &mut self.workspace {
            let effects = workspace.edit_input(edit, self.focus);
            self.clear_preview_problem();
            return effects;
        }
        if self.inspection_editing() {
            self.edit_inspection(edit);
            return Vec::new();
        }
        if self.focus == SearchPanel::Files && self.results.files.edit.is_some() {
            if edit.motion() {
                crate::input::TextInput::new(
                    &mut self.results.files.filter,
                    &mut self.results.files.caret,
                )
                .apply(edit);
                return Vec::new();
            }
            self.results.prefer_selection();
            let selected = self.results.current().map(|m| m.id.clone()).or_else(|| {
                self.results
                    .files
                    .edit
                    .as_ref()
                    .and_then(|e| e.selected.clone())
            });
            if !crate::input::TextInput::new(
                &mut self.results.files.filter,
                &mut self.results.files.caret,
            )
            .apply(edit)
            {
                return Vec::new();
            }
            self.results.restore_selection(selected);
            self.selection_changed();
            return self.preview_effect();
        }
        if self.focus != SearchPanel::Query {
            return Vec::new();
        }
        let mut query = self.query.clone();
        let changed = query.edit(edit);
        if changed {
            self.remember_page(context.status.busy);
            self.results.files.preferred = None;
        }
        self.query = query;
        if changed {
            self.search(context)
        } else {
            Vec::new()
        }
    }
    fn edit_inspection(&mut self, edit: crate::input::Edit<'_>) {
        let definition = self.focus == SearchPanel::Body;
        let source = if definition {
            self.body
                .declaration()
                .and_then(|d| self.body.symbol(d))
                .and_then(|s| self.body.preview.clone().map(|p| (p, s.span)))
        } else {
            self.displayed_source()
                .and_then(|_| self.preview.clone())
                .map(|p| {
                    let end = p.text().len();
                    (p, vvv_engine::Span::new(0, end))
                })
        };
        let Some((preview, range)) = source else {
            return;
        };
        let inspection = if definition {
            &mut self.body.inspection
        } else {
            &mut self.inspection
        };
        if let Some(line) = inspection.edit_input(edit, &preview, range) {
            if definition {
                self.body.scroll =
                    line.saturating_sub(preview.lines_in(range).map_or(0, |lines| lines.start));
            } else {
                self.preview_scroll = Some(line);
            }
        }
    }
    pub fn update(&mut self, action: Action, context: &mut ModeContext<'_>) -> Vec<Effect> {
        if self.workspace.is_some() {
            match action {
                Action::Back => return self.back(context),
                Action::BrowseBack => return self.travel(false, context),
                Action::BrowseForward => return self.travel(true, context),
                Action::FocusNext => self.focus_by(1),
                Action::FocusPrev => self.focus_by(-1),
                Action::FocusNth(n) => self.focus_nth(n),
                Action::Refresh => {
                    self.problem = None;
                    context.status.clear();
                    context.status.busy = true;
                    let generation = context.next_generation();
                    return self.workspace.as_mut().unwrap().refresh(generation);
                }
                _ => {
                    let effects = self.workspace.as_mut().unwrap().update(
                        action,
                        self.focus == SearchPanel::Query || self.focus == SearchPanel::Files,
                        self.focus == SearchPanel::Context,
                    );
                    self.clear_preview_problem();
                    return effects;
                }
            }
            return Vec::new();
        }

        if self.scroll_focused()
            && ((self.inspection_editing()
                && matches!(
                    action,
                    Action::Input(_)
                        | Action::Backspace
                        | Action::Clear
                        | Action::Enter
                        | Action::Back
                ))
                || matches!(
                    action,
                    Action::InspectFind
                        | Action::InspectLine
                        | Action::InspectNext(_)
                        | Action::InspectHorizontal(_)
                        | Action::InspectStart
                        | Action::ExpandPreview
                ))
        {
            self.inspection_action(action);
            return Vec::new();
        }
        if self.results.files.edit.is_some() && self.focus == SearchPanel::Files {
            match action {
                Action::Input(_) | Action::Backspace | Action::Clear => {
                    self.results.prefer_selection();
                    let selected = self.results.current().map(|m| m.id.clone()).or_else(|| {
                        self.results
                            .files
                            .edit
                            .as_ref()
                            .and_then(|e| e.selected.clone())
                    });
                    match action {
                        Action::Input(c) => self.results.files.filter.push(c),
                        Action::Backspace => {
                            self.results.files.filter.pop();
                        }
                        Action::Clear => self.results.files.filter.clear(),
                        _ => unreachable!(),
                    }
                    self.results.restore_selection(selected);
                    self.selection_changed();
                    return self.preview_effect();
                }
                Action::Enter => {
                    self.results.files.edit = None;
                    self.focus = SearchPanel::Results;
                    return Vec::new();
                }
                _ => {}
            }
        }
        match action {
            Action::FilterFiles if self.results.has_file_list() => {
                self.trail.cancel();
                self.results.remember_current();
                let selected = self.results.current().map(|m| m.id.clone());
                self.results.files.begin(selected, self.focus);
                self.focus = SearchPanel::Files;
            }
            Action::ClearFileFilter => {
                let selected = self.results.current().map(|m| m.id.clone());
                self.results.files.filter.clear();
                self.results.restore_selection(selected);
                self.selection_changed();
            }
            Action::Refresh => {
                self.preview_dirty = true;
                return self.search(context);
            }
            Action::BrowseBack => return self.travel(false, context),
            Action::BrowseForward => return self.travel(true, context),
            Action::FocusNext => self.focus_by(1),
            Action::FocusPrev => self.focus_by(-1),
            Action::FocusNth(n) => self.focus_nth(n),
            Action::Move(n) => self.moved(n),
            Action::File(n) => self.file_by(n),
            Action::PreviewTab => {
                if !self.results.has_body() {
                    return Vec::new();
                }
                self.definition_tab = !self.definition_tab;
                self.focus = if self.definition_tab {
                    SearchPanel::Body
                } else {
                    SearchPanel::Context
                };
                self.sync_preview_tab();
            }
            Action::Top => return self.jump(true),
            Action::Bottom => return self.jump(false),
            Action::Scroll(n) if self.scroll_focused() => {
                self.scrolled(n);
                return Vec::new();
            }
            Action::Scroll(n) => self.moved(n),
            Action::Input(c) => {
                self.remember_page(context.status.busy);
                self.results.files.preferred = None;
                self.query.push(c);
                return self.search(context);
            }
            Action::Backspace => {
                self.remember_page(context.status.busy);
                self.results.files.preferred = None;
                self.query.pop();
                return self.search(context);
            }
            Action::Clear => {
                self.remember_page(context.status.busy);
                self.results.files.preferred = None;
                self.query.clear();
                return self.search(context);
            }
            Action::Enter => {
                return match self.focus {
                    SearchPanel::Query => {
                        self.focus = SearchPanel::Results;
                        Vec::new()
                    }
                    SearchPanel::Files => {
                        self.focus = SearchPanel::Results;
                        Vec::new()
                    }
                    SearchPanel::Results => self.enter_subject(context),
                    SearchPanel::Context | SearchPanel::Body => Vec::new(),
                };
            }
            _ => return Vec::new(),
        }
        self.preview_effect()
    }
    pub fn previewed(&mut self, preview: FilePreview) {
        if self.problem.as_ref().is_some_and(
            |p| matches!(&p.retry, Some(Effect::Preview { path }) if *path == preview.path),
        ) {
            self.problem = None;
        }
        if let Some(workspace) = &mut self.workspace {
            workspace.previewed(preview);
            return;
        }
        if self
            .results
            .current_site()
            .is_some_and(|(path, _)| path == preview.path)
        {
            self.preview = Some(preview);
            if let Some(preview) = &self.preview {
                self.inspection
                    .sync(preview, vvv_engine::Span::new(0, preview.text().len()));
            }
            self.preview_dirty = false;
            self.sync_source();
        }
    }
    pub fn searched(
        &mut self,
        matches: Vec<Match>,
        skipped: Vec<Skipped>,
        context: &mut ModeContext<'_>,
    ) {
        context.status.busy = false;
        self.stale = false;
        self.locations.observe(&matches);
        self.results.searched_location = self.locations.selected.clone();
        self.results.replace(matches);
        if let (Some(selected), Some(preview)) = (self.results.current(), &self.preview)
            && selected.path == preview.path
            && selected
                .content
                .as_ref()
                .is_some_and(|content| content != preview.content_id())
        {
            self.preview_dirty = true;
        }
        self.selection_changed();
        if let Some(recipe) = recall::SearchRecipe::capture(self) {
            self.recent.searched(recipe);
        }
        if skipped.is_empty() {
            context.status.clear()
        } else {
            let names: Vec<String> = skipped.iter().map(|s| s.language.to_string()).collect();
            context.status.info(format!(
                "{} skipped: the pattern does not parse there",
                names.join(", ")
            ));
        }
    }
    pub fn answered(&mut self, answer: Answer, context: &mut ModeContext<'_>) -> bool {
        context.status.busy = false;
        match answer {
            Answer::References(references) => {
                self.page = BrowsePage::References;
                self.results.entered(references);
            }
            Answer::Impact(impact) => self.results.show_impact(impact),
            Answer::Explain(definition) => self.results.show_definition(definition),
            Answer::Deps(deps) => self.results.show_deps(deps),
            _ => return false,
        }
        self.selection_changed();
        true
    }
}

/// The site and hit belonging to the source text currently displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAnchor {
    pub path: RelPath,
    pub line: u32,
    pub hit: Option<vvv_engine::Span>,
    pub lines: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchPanel {
    #[default]
    Query,
    Files,
    Results,
    Context,
    Body,
}

impl Panels for SearchPanel {
    const ALL: &'static [Self] = &[
        Self::Query,
        Self::Files,
        Self::Results,
        Self::Context,
        Self::Body,
    ];
}
impl SearchPanel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Query => "Query",
            Self::Files => "Files",
            Self::Results => "Matches",
            Self::Context => "Source",
            Self::Body => "Definition",
        }
    }
}

/// What the hub shows about the declaration it was narrowed to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Relation {
    /// Every judged occurrence, grouped by verdict.
    #[default]
    References,
    /// Only the occurrences that resolve to the declaration.
    Resolved,
    /// Only the ones syntax could not place.
    Unresolved,
    /// Only the ones belonging to another declaration.
    Other,
    /// The modules that import it, then those, outward.
    Impact,
    /// Where the declaration is: its address, reach and importers.
    Definition,
    /// The declaring file's imports, and who imports it.
    Deps,
}

impl Relation {
    pub const ALL: &'static [Self] = &[
        Self::References,
        Self::Resolved,
        Self::Unresolved,
        Self::Other,
        Self::Impact,
        Self::Definition,
        Self::Deps,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::References => "references",
            Self::Resolved => "✓ safe",
            Self::Unresolved => "? unverified",
            Self::Other => "✗ another declaration's",
            Self::Impact => "impact (importers)",
            Self::Definition => "definition",
            Self::Deps => "deps (imports)",
        }
    }

    /// The stable key a menu item carries, and reads back.
    pub fn key(self) -> &'static str {
        match self {
            Self::References => "references",
            Self::Resolved => "resolved",
            Self::Unresolved => "unresolved",
            Self::Other => "other",
            Self::Impact => "impact",
            Self::Definition => "definition",
            Self::Deps => "deps",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|r| r.key() == key)
    }

    /// The verdict a references view keeps, when it is narrowed to one.
    pub fn confidence(self) -> Option<Confidence> {
        match self {
            Self::Resolved => Some(Confidence::Resolved),
            Self::Unresolved => Some(Confidence::Unresolved),
            Self::Other => Some(Confidence::Other),
            Self::References | Self::Impact | Self::Definition | Self::Deps => None,
        }
    }

    pub fn is_impact(self) -> bool {
        matches!(self, Self::Impact)
    }

    /// Whether the relation lists the subject's occurrences.
    pub fn is_references(self) -> bool {
        matches!(
            self,
            Self::References | Self::Resolved | Self::Unresolved | Self::Other
        )
    }
}

/// Retained search results with a cursor in fuzzy-ranked file order. A row
/// can be *entered* as the search's subject: the rows then
/// become that declaration's judged occurrences instead of its spellings.
#[derive(Debug, Clone, Default)]
pub struct Results {
    navigation: std::cell::RefCell<Option<Arc<ResultNavigation>>>,
    /// Reference answers belong to this result set, including its source ranges.
    revision: u64,
    pub query: Option<Query>,
    pub matches: Arc<Vec<Match>>,
    pub files: files::FileNavigator,
    pub category: Category,
    pub location: Option<RelPath>,
    pub searched_location: Option<RelPath>,
    /// The declaration the rows were narrowed to, when a row was entered:
    /// name, kind and file, exactly what `references` disambiguates by.
    pub subject: Option<ReferencesQuery>,
    /// What is shown about the subject.
    pub relation: Relation,
    /// The subject's occurrences once the engine answered; `None` while the
    /// request is out.
    pub references: Option<Arc<References>>,
    /// The subject's consumer modules, once `impact` was asked for.
    pub impact: Option<Arc<Impact>>,
    /// The declaration's definition, once `definition` was asked for.
    pub definition: Option<Arc<Explanation>>,
    /// The declaring file's imports and importers, once `deps` was asked for.
    pub deps: Option<Arc<Deps>>,
    pub cursor: Cursor,
}

impl Results {
    pub fn retained_bytes(&self) -> usize {
        let mut bytes = crate::model::RetainedBytes::default();
        if serde_json::to_writer(
            &mut bytes,
            &(
                &self.query,
                &self.subject,
                self.matches.as_ref(),
                self.references.as_deref(),
                self.impact.as_deref(),
                self.definition.as_deref(),
                self.deps.as_deref(),
            ),
        )
        .is_err()
        {
            return usize::MAX / 128;
        }
        bytes.estimate()
            + self.files.retained_bytes()
            + self
                .navigation
                .borrow()
                .as_ref()
                .map_or(0, |n| n.retained_bytes())
    }

    pub fn replace(&mut self, matches: Vec<Match>) {
        let selected = self.current().map(|m| m.id.clone());
        self.revision += 1;
        self.matches = Arc::new(matches);
        self.files.retain(&self.matches);
        self.subject = None;
        self.relation = Relation::References;
        self.references = None;
        self.clear_lenses();
        self.restore_selection(selected);
    }

    pub fn set_category(&mut self, category: Category) {
        self.prefer_selection();
        let selected = self.current().map(|m| m.id.clone());
        self.category = category;
        self.revision += 1;
        self.restore_selection(selected);
    }

    fn restore_selection(&mut self, selected: Option<vvv_engine::MatchId>) {
        let groups = self.file_groups();
        let fallback = groups
            .iter()
            .find(|g| {
                self.files
                    .active
                    .as_ref()
                    .is_some_and(|path| path == g.path)
            })
            .or_else(|| groups.first())
            .and_then(|f| self.files.selected(f))
            .map(|m| m.id.clone());
        let matches: Vec<_> = groups
            .iter()
            .flat_map(|f| f.matches.iter().copied())
            .collect();
        let index = self
            .files
            .preferred
            .as_ref()
            .and_then(|id| matches.iter().position(|m| &m.id == id))
            .or_else(|| selected.and_then(|id| matches.iter().position(|m| m.id == id)))
            .or_else(|| fallback.and_then(|id| matches.iter().position(|m| m.id == id)))
            .unwrap_or(0);
        let len = matches.len();
        self.cursor.index = index;
        self.cursor.clamp(len);
    }

    pub fn set_location(&mut self, location: Option<RelPath>) {
        self.prefer_selection();
        let selected = self.current().map(|m| m.id.clone());
        self.location = location;
        self.revision += 1;
        self.restore_selection(selected);
    }

    /// File order in the navigator; the underlying engine matches retain their IDs.
    pub fn listed(&self) -> Vec<&Match> {
        self.navigation()
            .listed
            .iter()
            .map(|&slot| self.match_at(slot))
            .collect()
    }

    /// Occurrences eligible for navigation, before the local file filter.
    pub fn eligible(&self) -> Vec<&Match> {
        self.navigation()
            .base
            .eligible
            .iter()
            .map(|&slot| self.match_at(slot))
            .collect()
    }

    pub fn has_file_list(&self) -> bool {
        !self.is_anchored() || self.relation.is_references()
    }

    pub fn file_groups(&self) -> Vec<files::FileGroup<'_>> {
        let navigation = self.navigation();
        navigation
            .visible
            .iter()
            .map(|(index, rank)| {
                let group = &navigation.base.groups[*index];
                files::FileGroup {
                    path: &self.match_at(group[0]).path,
                    matches: group.iter().map(|&slot| self.match_at(slot)).collect(),
                    rank: rank.clone(),
                }
            })
            .collect()
    }

    pub fn unfiltered_file_groups(&self) -> Vec<files::FileGroup<'_>> {
        self.navigation()
            .base
            .groups
            .iter()
            .map(|group| files::FileGroup {
                path: &self.match_at(group[0]).path,
                matches: group.iter().map(|&slot| self.match_at(slot)).collect(),
                rank: files::PathMatch::default(),
            })
            .collect()
    }

    pub fn eligible_file_count(&self) -> usize {
        self.navigation().base.groups.len()
    }

    pub fn eligible_count(&self) -> usize {
        self.navigation().base.eligible.len()
    }

    fn match_at(&self, slot: MatchSlot) -> &Match {
        match slot {
            MatchSlot::Search(index) => &self.matches[index],
            MatchSlot::Reference(index) => &self.references.as_ref().unwrap().occurrences[index].m,
        }
    }

    fn navigation(&self) -> Arc<ResultNavigation> {
        let mut cached = self.navigation.borrow_mut();
        if cached.as_ref().is_none_or(|n| !n.matches(self)) {
            *cached = Some(Arc::new(ResultNavigation::new(self, cached.as_deref())));
        }
        Arc::clone(cached.as_ref().unwrap())
    }

    pub fn remember_current(&mut self) {
        if let Some((path, id)) = self.current().map(|m| (m.path.clone(), m.id.clone())) {
            self.files.remember(path, id);
        }
    }

    fn prefer_selection(&mut self) {
        if self.files.preferred.is_none() {
            self.files.preferred = self.current().map(|m| m.id.clone());
        }
    }

    pub fn move_file(&mut self, by: i32) -> bool {
        self.files.preferred = None;
        self.remember_current();
        let path = self.current().map(|m| &m.path);
        let groups = self.file_groups();
        if groups.is_empty() {
            return false;
        }
        let current = groups
            .iter()
            .position(|g| Some(g.path) == path)
            .unwrap_or(0);
        let index = (current as i64 + i64::from(by)).clamp(0, groups.len() as i64 - 1) as usize;
        let file = &groups[index];
        let selected = self.files.selected(file).unwrap();
        let next = groups[..index]
            .iter()
            .map(|g| g.matches.len())
            .sum::<usize>()
            + file
                .matches
                .iter()
                .position(|m| m.id == selected.id)
                .unwrap();
        if self.cursor.index == next {
            return false;
        }
        self.cursor.index = next;
        true
    }

    /// The engine's answer for a declaration entered: the subject is read
    /// off the answer and the rows become its references. Nothing changes
    /// on screen until this arrives, so the pane never blanks mid-request.
    pub fn entered(&mut self, references: References) {
        let selected = self.current().map(|m| m.id.clone());
        self.revision += 1;
        self.subject = Self::subject_of(&references);
        self.relation = Relation::References;
        self.references = Some(Arc::new(references));
        self.clear_lenses();
        self.restore_selection(selected);
    }

    /// The subject's consumer modules.
    pub fn show_impact(&mut self, impact: Impact) {
        self.revision += 1;
        self.relation = Relation::Impact;
        self.impact = Some(Arc::new(impact));
        self.cursor = Cursor::default();
    }

    /// Where the subject is declared: its address, reach and importers.
    pub fn show_definition(&mut self, definition: Explanation) {
        self.revision += 1;
        self.relation = Relation::Definition;
        self.definition = Some(Arc::new(definition));
        self.cursor = Cursor::default();
    }

    /// The declaring file's imports and importers.
    pub fn show_deps(&mut self, deps: Deps) {
        self.revision += 1;
        self.relation = Relation::Deps;
        self.deps = Some(Arc::new(deps));
        self.cursor = Cursor::default();
    }

    /// Forget every lens but the references, called when entering anew.
    fn clear_lenses(&mut self) {
        self.impact = None;
        self.definition = None;
        self.deps = None;
    }

    /// The subject a `references` answer is about: the declaration it
    /// opened with, named and placed.
    fn subject_of(references: &References) -> Option<ReferencesQuery> {
        let declaration = references.declarations.first()?;
        let symbol = declaration.symbol.as_ref()?;
        let mut query = ReferencesQuery::new(references.name.as_str())
            .of_symbol(symbol.kind)
            .declared_in(declaration.path.clone());
        query.language = Some(declaration.language.clone());
        Some(query)
    }

    /// Show another relation, retaining the selected occurrence or file when visible.
    pub fn set_relation(&mut self, relation: Relation) {
        let selected = self.current().map(|m| m.id.clone());
        self.revision += 1;
        self.relation = relation;
        self.restore_selection(selected);
    }

    /// Leave the subject: the rows are the search's spellings again.
    pub fn leave(&mut self) {
        let selected = self.current().map(|m| m.id.clone());
        self.revision += 1;
        self.subject = None;
        self.relation = Relation::References;
        self.references = None;
        self.clear_lenses();
        self.restore_selection(selected);
    }

    pub fn is_anchored(&self) -> bool {
        self.subject.is_some()
    }

    /// The subject's declaration, once references have answered: the row a
    /// `definition`, `deps` or jump is about.
    pub fn subject_declaration(&self) -> Option<&Match> {
        self.references.as_ref()?.declarations.first()
    }

    /// The occurrences the current relation keeps, in engine order.
    fn reference_matches(&self) -> Vec<(usize, &Occurrence)> {
        if !self.relation.is_references() {
            return Vec::new();
        }
        let Some(r) = &self.references else {
            return Vec::new();
        };
        let confidence = self.relation.confidence();
        let mut shown: Vec<_> = r
            .occurrences
            .iter()
            .enumerate()
            .filter(|(_, o)| confidence.is_none_or(|c| c == o.confidence))
            .filter(|(_, o)| {
                self.location
                    .as_ref()
                    .is_none_or(|p| o.m.path.starts_with(p))
            })
            .collect();
        let rank = |c| match c {
            Confidence::Resolved => 0,
            Confidence::Unresolved => 1,
            Confidence::Other => 2,
        };
        shown.sort_by(|(_, a), (_, b)| {
            rank(a.confidence)
                .cmp(&rank(b.confidence))
                .then(a.m.path.cmp(&b.m.path))
                .then(a.m.start.cmp(&b.m.start))
        });
        shown
    }

    /// How many rows the cursor walks: the search's matches, or the
    /// subject's rows once anchored.
    pub fn len(&self) -> usize {
        if self.has_file_list() {
            return self.navigation().listed.len();
        }
        if self.relation.is_impact() {
            self.impact.as_ref().map_or(0, |i| i.consumers.len())
        } else {
            0
        }
    }

    pub fn current(&self) -> Option<&Match> {
        let i = self.cursor.index;
        if self.has_file_list() {
            return self
                .navigation()
                .listed
                .get(i)
                .map(|&slot| self.match_at(slot));
        }
        None
    }

    /// Confidence metadata in the same indexed order as the match navigator.
    pub fn occurrence_at(&self, index: usize) -> Option<&Occurrence> {
        match *self.navigation().listed.get(index)? {
            MatchSlot::Reference(index) => self.references.as_ref()?.occurrences.get(index),
            MatchSlot::Search(_) => None,
        }
    }

    /// The module under the cursor, in the impact view.
    pub fn current_consumer(&self) -> Option<&Consumer> {
        if !self.relation.is_impact() {
            return None;
        }
        self.impact
            .as_ref()
            .and_then(|i| i.consumers.get(self.cursor.index))
    }

    /// Where the cursor stands: a match's line, a consumer's or the
    /// definition's file. What the context panel and `$EDITOR` open.
    pub fn current_site(&self) -> Option<(RelPath, u32)> {
        if let Some(consumer) = self.current_consumer() {
            return Some((consumer.path.clone(), 0));
        }
        match self.relation {
            Relation::Definition => self
                .definition
                .as_ref()
                .map(|e| (e.path.clone(), e.declared.map_or(0, |p| p.line))),
            Relation::Deps => self.deps.as_ref().map(|d| (d.path.clone(), 0)),
            _ => self.current().map(|m| (m.path.clone(), m.start.line)),
        }
    }

    pub fn source_anchor(&self) -> Option<SourceAnchor> {
        let (path, line) = self.current_site()?;
        let current = self.current().filter(|m| m.path == path);
        Some(SourceAnchor {
            path,
            line,
            hit: current.map(|m| m.span),
            lines: current.map(|m| (m.start.line as usize, m.end.line as usize)),
        })
    }

    /// Whether definition focus is available, independent of cursor and loading.
    pub fn has_body(&self) -> bool {
        !self.matches.is_empty() || !self.anchored_declarations().is_empty()
    }

    /// The subject's declarations, once anchored.
    pub fn anchored_declarations(&self) -> &[Match] {
        self.references
            .as_ref()
            .map_or(&[], |r| r.declarations.as_slice())
    }

    pub fn declarations(&self) -> impl Iterator<Item = &Match> {
        self.matches.iter().filter(|m| m.role == Role::Declaration)
    }

    /// The declaration a use row belongs to, when the results hold exactly
    /// one declaration of that name.
    pub fn declaration_of(&self, m: &Match) -> Option<&Match> {
        let mut same = self
            .declarations()
            .filter(|d| d.language == m.language)
            .filter(|d| d.symbol.as_ref().is_some_and(|s| s.name == m.text));
        let first = same.next()?;
        same.next().is_none().then_some(first)
    }

    /// The declaration a row would enter: a declaration's own name, kind
    /// and file; a use row resolves through a unique same-named declaration.
    pub fn subject_at(&self, m: &Match) -> Option<ReferencesQuery> {
        let mut query = match &m.symbol {
            Some(s) => ReferencesQuery::new(s.name.as_str())
                .of_symbol(s.kind)
                .declared_in(m.path.clone()),
            None => {
                let d = self.declaration_of(m)?;
                let s = d.symbol.as_ref()?;
                ReferencesQuery::new(s.name.as_str())
                    .of_symbol(s.kind)
                    .declared_in(d.path.clone())
            }
        };
        query.language = self.query.as_ref().and_then(|q| q.language().cloned());
        Some(query)
    }

    /// The name a rename would act on from the cursor: a declaration's name,
    /// kind and file, or an identifier hit's token.
    pub fn rename_target(&self) -> Option<RenameTarget> {
        let m = self.current()?;
        if let Some(s) = &m.symbol {
            return Some(RenameTarget {
                name: s.name.clone(),
                symbol: Some(s.kind),
                declared_in: Some(m.path.clone()),
            });
        }
        let text = m.text.as_str();
        let mut chars = text.chars();
        let bare = chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_');
        bare.then(|| RenameTarget {
            name: text.to_owned(),
            symbol: None,
            declared_in: None,
        })
    }
}

/// Stable offsets into immutable answers; the index never copies source matches.
#[derive(Debug, Clone, Copy)]
enum MatchSlot {
    Search(usize),
    Reference(usize),
}

/// Grouping changes with the answer and scope, independently of fuzzy ranking.
#[derive(Debug)]
struct ResultFiles {
    matches: Arc<Vec<Match>>,
    references: Option<Arc<References>>,
    anchored: bool,
    category: Category,
    relation: Relation,
    location: Option<RelPath>,
    eligible: Vec<MatchSlot>,
    groups: Vec<Vec<MatchSlot>>,
}

impl ResultFiles {
    fn matches(&self, results: &Results) -> bool {
        Arc::ptr_eq(&self.matches, &results.matches)
            && match (&self.references, &results.references) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
            && self.anchored == results.is_anchored()
            && self.category == results.category
            && self.relation == results.relation
            && self.location == results.location
    }

    fn new(results: &Results) -> Self {
        let mut eligible: Vec<_> = if results.is_anchored() {
            results
                .reference_matches()
                .into_iter()
                .map(|(index, _)| MatchSlot::Reference(index))
                .collect()
        } else {
            results
                .matches
                .iter()
                .enumerate()
                .filter(|(_, m)| results.category.includes(m.role))
                .map(|(index, _)| MatchSlot::Search(index))
                .collect()
        };
        if !results.is_anchored() {
            eligible.sort_by(|&a, &b| {
                let (a, b) = (results.match_at(a), results.match_at(b));
                a.path
                    .cmp(&b.path)
                    .then(a.start.cmp(&b.start))
                    .then(a.span.start.cmp(&b.span.start))
            });
        }
        let mut grouped = std::collections::BTreeMap::<&RelPath, Vec<MatchSlot>>::new();
        for &slot in &eligible {
            grouped
                .entry(&results.match_at(slot).path)
                .or_default()
                .push(slot);
        }
        let groups = grouped
            .into_values()
            .map(|mut group| {
                group.sort_by(|&a, &b| {
                    let (a, b) = (results.match_at(a), results.match_at(b));
                    a.start.cmp(&b.start).then(a.span.cmp(&b.span))
                });
                group
            })
            .collect();
        Self {
            matches: results.matches.clone(),
            references: results.references.clone(),
            anchored: results.is_anchored(),
            category: results.category,
            relation: results.relation,
            location: results.location.clone(),
            eligible,
            groups,
        }
    }
}

/// One memoized navigation projection per retained page. Views and selection
/// share the same order; only answer/scope/filter changes rebuild derived data.
#[derive(Debug)]
struct ResultNavigation {
    base: Arc<ResultFiles>,
    filter: String,
    visible: Vec<(usize, files::PathMatch)>,
    listed: Vec<MatchSlot>,
}

impl ResultNavigation {
    fn matches(&self, results: &Results) -> bool {
        self.base.matches(results) && self.filter == results.files.filter
    }

    fn new(results: &Results, previous: Option<&Self>) -> Self {
        let base = previous
            .filter(|n| n.base.matches(results))
            .map_or_else(|| Arc::new(ResultFiles::new(results)), |n| n.base.clone());
        let mut visible: Vec<_> = base
            .groups
            .iter()
            .enumerate()
            .filter_map(|(index, group)| {
                files::PathMatch::find(
                    results.match_at(group[0]).path.as_str(),
                    &results.files.filter,
                )
                .map(|rank| (index, rank))
            })
            .collect();
        visible.sort_by(|(a, ar), (b, br)| br.score.cmp(&ar.score).then(a.cmp(b)));
        let listed = visible
            .iter()
            .flat_map(|(index, _)| base.groups[*index].iter().copied())
            .collect();
        Self {
            base,
            filter: results.files.filter.clone(),
            visible,
            listed,
        }
    }

    fn retained_bytes(&self) -> usize {
        (self.base.eligible.capacity()
            + self.listed.capacity()
            + self.base.groups.iter().map(Vec::capacity).sum::<usize>())
            * std::mem::size_of::<MatchSlot>()
            + self.base.groups.capacity() * std::mem::size_of::<Vec<MatchSlot>>()
            + self.visible.capacity() * std::mem::size_of::<(usize, files::PathMatch)>()
            + self
                .visible
                .iter()
                .map(|(_, rank)| rank.positions.capacity() * std::mem::size_of::<usize>())
                .sum::<usize>()
            + self.filter.capacity()
            + self
                .base
                .location
                .as_ref()
                .map_or(0, |path| path.as_str().len())
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;

    #[test]
    fn memoized_navigation_agrees_with_fresh_grouping_after_every_filter_and_answer_change() {
        let fresh = |results: &Results| {
            let eligible: Vec<_> = if results.is_anchored() {
                results
                    .references
                    .as_ref()
                    .unwrap()
                    .occurrences
                    .iter()
                    .filter(|o| {
                        results
                            .relation
                            .confidence()
                            .is_none_or(|c| c == o.confidence)
                    })
                    .filter(|o| {
                        results
                            .location
                            .as_ref()
                            .is_none_or(|p| o.m.path.starts_with(p))
                    })
                    .map(|o| &o.m)
                    .collect()
            } else {
                results
                    .matches
                    .iter()
                    .filter(|m| results.category.includes(m.role))
                    .collect()
            };
            let mut grouped = std::collections::BTreeMap::<&RelPath, Vec<&Match>>::new();
            for m in eligible {
                grouped.entry(&m.path).or_default().push(m);
            }
            let mut groups: Vec<_> = grouped
                .into_iter()
                .filter_map(|(path, mut matches)| {
                    let rank = files::PathMatch::find(path.as_str(), &results.files.filter)?;
                    matches.sort_by(|a, b| a.start.cmp(&b.start).then(a.span.cmp(&b.span)));
                    Some((
                        path.clone(),
                        matches.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
                        rank.score,
                        rank.positions,
                    ))
                })
                .collect();
            groups.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
            groups
        };
        for anchored in [false, true] {
            let mut results = Results::default();
            results.replace(crate::fixtures::search().matches);
            if anchored {
                results.entered(crate::fixtures::references());
            }
            for category in [
                Category::All,
                Category::Declarations,
                Category::Imports,
                Category::Uses,
            ] {
                results.set_category(category);
                for relation in [
                    Relation::References,
                    Relation::Resolved,
                    Relation::Unresolved,
                    Relation::Other,
                ] {
                    results.set_relation(relation);
                    for location in [
                        None,
                        Some(RelPath::from("src/lib.rs")),
                        Some(RelPath::from("src/lang")),
                    ] {
                        results.set_location(location);
                        for filter in ["", "lib", "lang", "zzzz", "src l"] {
                            results.files.filter = filter.into();
                            let expected = fresh(&results);
                            let actual = results
                                .file_groups()
                                .into_iter()
                                .map(|g| {
                                    (
                                        g.path.clone(),
                                        g.matches.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
                                        g.rank.score,
                                        g.rank.positions,
                                    )
                                })
                                .collect::<Vec<_>>();
                            assert_eq!(
                                actual, expected,
                                "anchored={anchored}, category={category:?}, relation={relation:?}, filter={filter}"
                            );
                            let ids = expected.into_iter().flat_map(|g| g.1).collect::<Vec<_>>();
                            assert_eq!(results.len(), ids.len());
                            for (i, id) in ids.into_iter().enumerate() {
                                results.cursor.index = i;
                                assert_eq!(results.current().unwrap().id, id);
                            }
                        }
                    }
                }
            }
            let saved = results.clone();
            results.replace(vec![crate::fixtures::m("new.rs", 0, 0, "New", "New()")]);
            results.files.filter.clear();
            assert_eq!(
                results.current().unwrap().path.as_path(),
                std::path::Path::new("new.rs")
            );
            assert_eq!(
                saved.file_groups().len(),
                fresh(&saved).len(),
                "saved pages keep their own answer and projection"
            );
        }
    }
}

/// A retained-hub navigation either follows its changed selection or requests data.
/// The application previews its active mode, which can differ from the retained hub.
pub(crate) enum Navigation {
    Selection,
    Effects(Vec<Effect>),
}

/// Search roles are filters on spellings, never evidence of a resolved reference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    All,
    Declarations,
    Imports,
    Uses,
}

impl Category {
    pub const ALL: &'static [Self] = &[Self::All, Self::Declarations, Self::Imports, Self::Uses];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Declarations => "Declarations",
            Self::Imports => "Imports",
            Self::Uses => "Uses",
        }
    }

    pub fn count_label(self, count: usize) -> &'static str {
        match (self, count) {
            (Self::Declarations, 1) => "Declaration",
            (Self::Imports, 1) => "Import",
            (Self::Uses, 1) => "Use",
            _ => self.label(),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Declarations => "declarations",
            Self::Imports => "imports",
            Self::Uses => "uses",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.key() == key)
    }

    pub fn includes(self, role: Role) -> bool {
        match self {
            Self::All => true,
            Self::Declarations => role == Role::Declaration,
            Self::Imports => role == Role::Import,
            Self::Uses => role == Role::Use,
        }
    }
}
