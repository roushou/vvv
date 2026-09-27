//! The retained query hub and its read-only relations.
pub(crate) mod query;
pub(crate) mod screen;
use crate::action::{Action, Effect};
use crate::model::{Cursor, FilePreview, Panels};
use crate::modes::context::ModeContext;
use crate::modes::rename::RenameTarget;
use crate::overlays::MenuTarget;
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
    pub query: QueryBar,
    pub results: Results,
    pub focus: SearchPanel,
    pub preview: Option<FilePreview>,
    /// A manual context scroll position; `None` follows the cursor.
    pub preview_scroll: Option<usize>,
}

impl Search {
    pub fn search(&mut self, context: &mut ModeContext<'_>) -> Vec<Effect> {
        let generation = context.next_generation();
        match self.query.parse() {
            Ok(query) => {
                context.status.busy = true;
                context.status.clear();
                self.results.query = Some(query.clone());
                vec![Effect::Search { generation, query }]
            }
            Err(e) => {
                self.results.replace(Vec::new());
                self.results.query = None;
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
        let Some(query) = self.results.subject_at(&m) else {
            return context.fail("put the cursor on a declaration to enter its scope");
        };
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
        match target {
            MenuTarget::Symbol => {
                self.query.set_filter(Filter::Symbol, value.as_deref());
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
    pub fn goto_declaration(&mut self, context: &mut ModeContext<'_>) -> Navigation {
        if let Some(row) = self.results.declaration_row() {
            self.results.cursor.index = row;
            self.preview_scroll = None;
            return Navigation::Selection;
        }
        if !self.results.is_anchored() && self.results.current().is_some() {
            return Navigation::Effects(self.enter_subject(context));
        }
        Navigation::Effects(Vec::new())
    }
    pub fn focus_by(&mut self, by: i32) {
        self.focus = self.focus.step(by);
    }
    pub fn focus_nth(&mut self, n: u8) {
        if let Some(p) = SearchPanel::nth(n) {
            self.focus = p;
        }
    }
    pub fn moved(&mut self, by: i32) {
        let len = self.results.len();
        self.results.cursor.move_by(by, len);
        self.preview_scroll = None;
    }
    pub fn scrolled(&mut self, by: i32) {
        let max = self
            .preview
            .as_ref()
            .map_or(0, |p| p.line_count().saturating_sub(1));
        let current = self.preview_scroll.unwrap_or_else(|| self.preview_anchor());
        self.preview_scroll = Some((current as i32 + by).clamp(0, max as i32) as usize);
    }
    /// The line the context centres on when following the cursor.
    pub fn preview_anchor(&self) -> usize {
        self.results
            .current()
            .map_or(0, |m| (m.start.line as usize).saturating_sub(5))
    }
    pub fn preview_effect(&self) -> Vec<Effect> {
        match self.results.current_site().map(|(path, _)| path) {
            Some(path) if self.preview.as_ref().map(|p| &p.path) != Some(&path) => {
                vec![Effect::Preview { path }]
            }
            _ => Vec::new(),
        }
    }
    pub fn scroll_focused(&self) -> bool {
        self.focus == SearchPanel::Context
    }
    pub fn jump(&mut self, top: bool) -> Vec<Effect> {
        if self.scroll_focused() {
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
        if self.results.is_anchored() {
            self.results.leave();
            context.status.clear();
            self.preview_effect()
        } else {
            self.focus = SearchPanel::Query;
            Vec::new()
        }
    }
    pub fn update(&mut self, action: Action, context: &mut ModeContext<'_>) -> Vec<Effect> {
        match action {
            Action::FocusNext => self.focus_by(1),
            Action::FocusPrev => self.focus_by(-1),
            Action::FocusNth(n) => self.focus_nth(n),
            Action::Move(n) => self.moved(n),
            Action::Top => return self.jump(true),
            Action::Bottom => return self.jump(false),
            Action::Scroll(n) if self.scroll_focused() => {
                self.scrolled(n);
                return Vec::new();
            }
            Action::Scroll(n) => self.moved(n),
            Action::Input(c) => {
                self.query.push(c);
                return self.search(context);
            }
            Action::Backspace => {
                self.query.pop();
                return self.search(context);
            }
            Action::Clear => {
                self.query.clear();
                return self.search(context);
            }
            Action::Enter => {
                return match self.focus {
                    SearchPanel::Query => {
                        self.focus = SearchPanel::Results;
                        Vec::new()
                    }
                    SearchPanel::Results => self.enter_subject(context),
                    SearchPanel::Context => Vec::new(),
                };
            }
            _ => return Vec::new(),
        }
        self.preview_effect()
    }
    pub fn previewed(&mut self, preview: FilePreview) {
        self.preview = Some(preview);
        self.preview_scroll = None;
    }
    pub fn searched(
        &mut self,
        matches: Vec<Match>,
        skipped: Vec<Skipped>,
        context: &mut ModeContext<'_>,
    ) {
        context.status.busy = false;
        self.results.replace(matches);
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
            Answer::References(references) => self.results.entered(references),
            Answer::Impact(impact) => self.results.show_impact(impact),
            Answer::Explain(definition) => self.results.show_definition(definition),
            Answer::Deps(deps) => self.results.show_deps(deps),
            _ => return false,
        }
        true
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchPanel {
    #[default]
    Query,
    Results,
    Context,
}

impl Panels for SearchPanel {
    const ALL: &'static [Self] = &[Self::Query, Self::Results, Self::Context];
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

/// Search results in the engine's order — declarations first — with the
/// cursor. A row can be *entered* as the search's subject: the rows then
/// become that declaration's judged occurrences instead of its spellings.
#[derive(Debug, Default)]
pub struct Results {
    pub query: Option<Query>,
    pub matches: Vec<Match>,
    /// The declaration the rows were narrowed to, when a row was entered:
    /// name, kind and file, exactly what `references` disambiguates by.
    pub subject: Option<ReferencesQuery>,
    /// What is shown about the subject.
    pub relation: Relation,
    /// The subject's occurrences once the engine answered; `None` while the
    /// request is out.
    pub references: Option<References>,
    /// The subject's consumer modules, once `impact` was asked for.
    pub impact: Option<Impact>,
    /// The declaration's definition, once `definition` was asked for.
    pub definition: Option<Explanation>,
    /// The declaring file's imports and importers, once `deps` was asked for.
    pub deps: Option<Deps>,
    pub cursor: Cursor,
}

impl Results {
    pub fn replace(&mut self, matches: Vec<Match>) {
        self.matches = matches;
        self.subject = None;
        self.references = None;
        self.clear_lenses();
        self.cursor.clamp(self.matches.len());
    }

    /// The engine's answer for a declaration entered: the subject is read
    /// off the answer and the rows become its references. Nothing changes
    /// on screen until this arrives, so the pane never blanks mid-request.
    pub fn entered(&mut self, references: References) {
        self.subject = Self::subject_of(&references);
        self.relation = Relation::References;
        self.references = Some(references);
        self.clear_lenses();
        self.cursor = Cursor::default();
    }

    /// The subject's consumer modules.
    pub fn show_impact(&mut self, impact: Impact) {
        self.relation = Relation::Impact;
        self.impact = Some(impact);
        self.cursor = Cursor::default();
    }

    /// Where the subject is declared: its address, reach and importers.
    pub fn show_definition(&mut self, definition: Explanation) {
        self.relation = Relation::Definition;
        self.definition = Some(definition);
        self.cursor = Cursor::default();
    }

    /// The declaring file's imports and importers.
    pub fn show_deps(&mut self, deps: Deps) {
        self.relation = Relation::Deps;
        self.deps = Some(deps);
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

    /// Show another relation for the same subject; the cursor starts over.
    pub fn set_relation(&mut self, relation: Relation) {
        self.relation = relation;
        self.cursor = Cursor::default();
    }

    /// Leave the subject: the rows are the search's spellings again.
    pub fn leave(&mut self) {
        self.subject = None;
        self.references = None;
        self.clear_lenses();
        self.cursor.clamp(self.matches.len());
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
    fn shown(&self) -> Vec<&Occurrence> {
        if !self.relation.is_references() {
            return Vec::new();
        }
        let Some(r) = &self.references else {
            return Vec::new();
        };
        match self.relation.confidence() {
            Some(confidence) => r
                .occurrences
                .iter()
                .filter(|o| o.confidence == confidence)
                .collect(),
            None => r.occurrences.iter().collect(),
        }
    }

    /// How many rows the cursor walks: the search's matches, or the
    /// subject's rows once anchored.
    pub fn len(&self) -> usize {
        if !self.is_anchored() {
            return self.matches.len();
        }
        if self.relation.is_impact() {
            self.impact.as_ref().map_or(0, |i| i.consumers.len())
        } else {
            self.shown().len()
        }
    }

    pub fn current(&self) -> Option<&Match> {
        let i = self.cursor.index;
        if !self.is_anchored() {
            return self.matches.get(i);
        }
        if self.relation.is_impact() {
            return None;
        }
        self.shown().get(i).map(|o| &o.m)
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

    /// The declaration a use row names, as an index into the current list:
    /// what the jump key moves the cursor to.
    pub fn declaration_row(&self) -> Option<usize> {
        if self.is_anchored() {
            return self
                .shown()
                .iter()
                .position(|o| o.m.role == Role::Declaration);
        }
        let current = self.current()?;
        let declaration = self.declaration_of(current)?;
        self.matches.iter().position(|m| m.id == declaration.id)
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

/// A retained-hub navigation either follows its changed selection or requests data.
/// The application previews its active mode, which can differ from the retained hub.
pub(crate) enum Navigation {
    Selection,
    Effects(Vec<Effect>),
}
