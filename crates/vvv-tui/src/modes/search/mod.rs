//! The retained query hub and its read-only relations.
pub(crate) mod query;
pub(crate) mod screen;
mod update;
use crate::model::{Cursor, FilePreview, Panels};
use crate::modes::rename::RenameTarget;
use query::QueryBar;
pub(crate) use update::Navigation;
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
