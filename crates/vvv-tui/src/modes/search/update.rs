//! Pure query, relation, and cursor transitions.
use super::{Relation, Search, SearchPanel};
use crate::action::{Action, Effect};
use crate::model::{FilePreview, Panels};
use crate::modes::context::ModeContext;
use vvv_engine::{Answer, DepsQuery, ExplainQuery, ImpactQuery, Match, Request, Skipped};
/// A retained-hub navigation either follows its changed selection or requests data.
/// The application previews its active mode, which can differ from the retained hub.
pub(crate) enum Navigation {
    Selection,
    Effects(Vec<Effect>),
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
