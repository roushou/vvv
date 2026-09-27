//! Rename's request, answer, execution, and report. References uses the same
//! graph evidence, so its command implementation lives here too.

use serde::{Deserialize, Serialize};
use vvv_core::{Edit, LanguageId, RelPath, SymbolKind};

use crate::change::Change;
use crate::graph::Evidence;
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{IntentLine, Plural};
use crate::report::{Block, Document, ReferencePlan};
use crate::{
    Confidence, EngineError, FileChange, Intent, Match, Mutation, Occurrence, Planned, Selection,
};

/// The declaration(s) called `name`, and every identifier that spells it,
/// each judged against the declaration meant. What `rename` gathers before
/// it plans, and what `references` answers on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferencesQuery {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SymbolKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageId>,
    /// The file declaring the symbol meant, when several share the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_in: Option<RelPath>,
}

impl ReferencesQuery {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            symbol: None,
            language: None,
            declared_in: None,
        }
    }

    pub fn declared_in(mut self, path: impl Into<RelPath>) -> Self {
        self.declared_in = Some(path.into());
        self
    }

    pub fn of_symbol(mut self, symbol: SymbolKind) -> Self {
        self.symbol = Some(symbol);
        self
    }

    pub fn in_language(mut self, language: impl Into<LanguageId>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// Answer with declarations and judged occurrences, without a mutation plan.
    pub fn execute(self, engine: &crate::Engine) -> Result<References, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph)
    }

    pub(crate) fn execute_in(
        self,
        graph: &mut crate::graph::Graph,
    ) -> Result<References, EngineError> {
        let query = &self;
        let evidence = graph.references(query)?;
        Ok(References {
            name: query.name.clone(),
            declarations: evidence.declarations,
            occurrences: evidence.occurrences,
        })
    }
}

/// `references <name>`: the declarations called `name` and every token that
/// spells it, each judged against the declaration meant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct References {
    pub name: String,
    pub declarations: Vec<Match>,
    pub occurrences: Vec<Occurrence>,
}

/// Rename the declaration(s) called `name` and every identifier that spells it.
///
/// Resolution is syntactic: every occurrence of the identifier in files of
/// the declaring language is an occurrence. The `selection` is how a human or
/// agent excludes the ones that are not the same symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameIntent {
    #[serde(flatten)]
    pub references: ReferencesQuery,
    pub to: String,
    #[serde(default, skip_serializing_if = "Selection::is_all")]
    pub selection: Selection,
}

impl RenameIntent {
    pub fn new(name: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            references: ReferencesQuery::new(name),
            to: to.into(),
            selection: Selection::All,
        }
    }

    pub fn declared_in(mut self, path: impl Into<RelPath>) -> Self {
        self.references.declared_in = Some(path.into());
        self
    }

    pub fn of_symbol(mut self, symbol: SymbolKind) -> Self {
        self.references.symbol = Some(symbol);
        self
    }

    pub fn in_language(mut self, language: impl Into<LanguageId>) -> Self {
        self.references.language = Some(language.into());
        self
    }

    pub fn selecting(mut self, selection: Selection) -> Self {
        self.selection = selection;
        self
    }

    /// Plan a rename without writing files; see [`Apply`](crate::Apply).
    /// Occurrences come from languages with a matching declaration, so Rust and
    /// TypeScript declarations with the same name are judged independently.
    pub fn plan(self, engine: &crate::Engine) -> Result<Planned<Rename>, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.plan_in(&mut graph, engine.workspace())
    }

    pub(crate) fn plan_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<Planned<Rename>, EngineError> {
        let intent = &self;
        let Evidence {
            declarations,
            occurrences,
            ambiguous,
        } = graph.references(&intent.references)?;

        // What `Selection::All` means: never an occurrence of another
        // declaration; unresolved ones only when nothing else could be meant.
        let default: Vec<Match> = occurrences
            .iter()
            .filter(|o| match o.confidence {
                Confidence::Resolved => true,
                Confidence::Unresolved => !ambiguous.contains(&o.m.language),
                Confidence::Other => false,
            })
            .map(|o| o.m.clone())
            .collect();
        let chosen = if intent.selection.is_all() {
            default
        } else {
            occurrences.iter().map(|o| o.m.clone()).collect()
        };
        let mut change = Change::new();
        for m in intent.selection.narrow(chosen)? {
            change.edit(
                graph.file(&m.path)?.file().witness(),
                Edit::replace(m.span, &intent.to),
            )?;
        }
        Planned::of(
            workspace,
            change,
            Intent::Rename(intent.clone()),
            |_, files| Rename {
                intent: intent.clone(),
                state: crate::MutationState::Preview,
                declarations,
                occurrences,
                files,
            },
        )
    }
}

/// `vvv rename`: the declaration and every occurrence, each judged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rename {
    pub intent: RenameIntent,
    /// Preview or successful application with its history entry.
    #[serde(flatten)]
    pub state: crate::MutationState,
    /// Where `name` is declared; more than one means the rename is ambiguous.
    pub declarations: Vec<Match>,
    /// Every identifier spelling the name, each judged against the target;
    /// their ids feed a later `--select`.
    pub occurrences: Vec<Occurrence>,
    pub files: Vec<FileChange>,
}

impl Mutation for Rename {
    fn into_mutation(self) -> crate::MutationAnswer {
        crate::MutationAnswer::Rename(self)
    }

    fn applied(&mut self, id: u64) {
        self.state = crate::MutationState::Applied { history_id: id };
    }
}

impl Document {
    pub(crate) fn rename(result: &Rename) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Rename(result.intent.clone())));
        report.declarations(&result.declarations);
        report.block_body(Block::Verdicts {
            occurrences: result.occurrences.clone(),
            plan: Some(ReferencePlan {
                state: result.state,
                files: result.files.clone(),
            }),
        });
        let strip = Self::verdict_counts(&result.occurrences);
        let edits = Self::edits_in(&result.files);
        let plan = Line::of(
            Role::Dim,
            format!(
                "→ {} in {}",
                Plural(edits, "occurrence"),
                Plural(result.files.len(), "file")
            ),
        );
        if result.state.is_applied() {
            report.block_note(Block::Summary(strip));
            report.receipt(result.state, plan);
            return report;
        }
        report.block_note(Block::Summary(strip.and(Role::Plain, "   ").and_line(plan)));
        report
    }

    pub(crate) fn references(result: &References) -> Self {
        let mut report = Self::new();
        report.declarations(&result.declarations);
        report.block_body(Block::Verdicts {
            occurrences: result.occurrences.clone(),
            plan: None,
        });
        report.block_note(Block::Summary(Self::verdict_counts(&result.occurrences)));
        report
    }
}
