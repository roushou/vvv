//! Rename's request, answer, execution, and report. References uses the same
//! graph evidence, so its command implementation lives here too.

use serde::{Deserialize, Serialize};
use vvv_core::{Edit, LanguageId, RelPath, SymbolKind};

use crate::change::Change;
use crate::command::{Command, Context};
use crate::graph::Evidence;
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::{IntentLine, Plural};
use crate::report::{Block, Document, Options, lines as l};
use crate::{
    Confidence, EngineError, FileChange, Intent, Match, Mutation, Occurrence, Planned, References,
    ReferencesQuery, Selection,
};

/// Rename the declaration(s) called `name` and every identifier that spells it.
///
/// Resolution is syntactic: every occurrence of the identifier in files of
/// the declaring language is an occurrence. The `selection` is how a human or
/// agent excludes the ones that are not the same symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameIntent {
    pub name: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<SymbolKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageId>,
    /// The file declaring the symbol meant, when several share the name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_in: Option<RelPath>,
    #[serde(default, skip_serializing_if = "Selection::is_all")]
    pub selection: Selection,
}

impl RenameIntent {
    pub fn new(name: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            to: to.into(),
            symbol: None,
            language: None,
            declared_in: None,
            selection: Selection::All,
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

    pub fn selecting(mut self, selection: Selection) -> Self {
        self.selection = selection;
        self
    }

    /// The question a rename asks before it plans.
    pub fn references(&self) -> ReferencesQuery {
        ReferencesQuery {
            name: self.name.clone(),
            symbol: self.symbol,
            language: self.language.clone(),
            declared_in: self.declared_in.clone(),
        }
    }
}

/// `vvv rename`: the declaration and every occurrence, each judged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rename {
    pub intent: RenameIntent,
    pub applied: bool,
    /// The history entry the apply made, when `applied`; what `undo` reverses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub history_id: Option<u64>,
    /// Where `name` is declared; more than one means the rename is ambiguous.
    pub declarations: Vec<Match>,
    /// Every identifier spelling the name, each judged against the target;
    /// their ids feed a later `--select`.
    pub occurrences: Vec<Occurrence>,
    pub files: Vec<FileChange>,
}

impl Mutation for Rename {
    fn intent(&self) -> Intent {
        Intent::Rename(self.intent.clone())
    }

    fn applied(&mut self, id: u64) {
        self.applied = true;
        self.history_id = Some(id);
    }
}

/// Plan a rename. Nothing is written; see [`Apply`](crate::Apply).
///
/// Occurrences are gathered only from the languages in which a matching
/// declaration exists, so a Rust `foo` never touches a TypeScript `foo`.
impl Command for RenameIntent {
    type Output = Planned<Rename>;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let intent = &self;
        let graph = &mut *cx.graph;
        let Evidence {
            declarations,
            occurrences,
            ambiguous,
        } = graph.references(&intent.references())?;

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
            change.edit(&m.path, Edit::replace(m.span, &intent.to));
        }
        Planned::of(cx.workspace, change, |_, files| Rename {
            intent: intent.clone(),
            applied: false,
            history_id: None,
            declarations,
            occurrences,
            files,
        })
    }
}

/// The declarations called `name` and every token spelling it, judged:
/// what `rename` acts on, answered without a plan.
impl Command for ReferencesQuery {
    type Output = References;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let query = &self;
        let graph = &mut *cx.graph;
        let evidence = graph.references(query)?;
        Ok(References {
            name: query.name.clone(),
            declarations: evidence.declarations,
            occurrences: evidence.occurrences,
        })
    }
}

impl Document {
    pub(crate) fn rename(result: &Rename, options: Options) -> Self {
        let mut report = Self::new();
        report.title(IntentLine(&Intent::Rename(result.intent.clone())));
        report.declarations(&result.declarations);
        report.block_body(Block::Verdicts {
            occurrences: result.occurrences.clone(),
            files: Some(result.files.clone()),
        });
        // The plan's patch, when asked for: a rename's rows are the verdicts,
        // so unlike a rewrite its diff is not the default view.
        if options.diff {
            report.block_body(Block::Changes(result.files.clone()));
        }
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
        if result.applied {
            report.block_note(Block::Summary(strip));
            report.receipt(true, result.history_id, plan);
            return report;
        }
        report.block_note(Block::Summary(strip.and(Role::Plain, "   ").and_line(plan)));
        let unsure = l::Verdicts::selection(&result.occurrences, Confidence::Unresolved);
        let mut flags = vec!["--apply to write".to_owned()];
        if !unsure.is_empty() {
            flags.push(format!("--select {unsure} for the ? rows alone"));
        }
        if !options.verbose
            && result
                .occurrences
                .iter()
                .any(|o| o.confidence == Confidence::Resolved)
        {
            flags.push("-v to list the ✓ rows".to_owned());
        }
        report.hint(flags.join(" · "));
        report
    }
}
