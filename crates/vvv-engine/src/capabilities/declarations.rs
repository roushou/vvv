//! Declaration locations and placement: outline and where.
use crate::graph::{Candidate, Declared};
use crate::protocol::display::{Line, Role};
use crate::protocol::vocabulary::Mark;
use crate::report::{Block, Document};
use crate::{EngineError, Match, Placed, Reach, ReferencesQuery, SymbolKind};
use serde::{Deserialize, Serialize};
use vvv_core::{Address, Position, RelPath, Symbol};

/// `vvv outline <path>`: what a file declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineQuery {
    pub path: RelPath,
}

/// `vvv where <name> [--from <file>]`: where a name is declared, and the
/// import that reaches each site from `from`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WhereQuery {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<RelPath>,
}

/// `outline <path>`: what a file declares, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outline {
    pub path: RelPath,
    /// The file's own module address, when the language has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<Address>,
    pub items: Vec<OutlineItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineItem {
    #[serde(flatten)]
    pub symbol: Symbol,
    /// Where the extent starts and ends, as lines and columns.
    pub start: Position,
    pub end: Position,
    /// The module address the declaration is reached by, when a path can
    /// reach it in this language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    /// Who may name it, from its modifier and its module.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reach: Option<Reach>,
}

/// `where <name>`: where `name` is declared and how to reach each site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Locations {
    pub name: String,
    pub sites: Vec<Site>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Site {
    pub declaration: Match,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<Address>,
    /// The import statement that brings it into the file asked from, spelled
    /// as that language writes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import: Option<String>,
}

impl Placed {
    /// A fragment's declaration as an answer names it: with its file and line.
    pub(crate) fn of(candidate: &Candidate, declared: &Declared) -> Self {
        Self {
            path: candidate.path().into(),
            symbol: declared.symbol.clone(),
            start: candidate
                .file()
                .source()
                .position(declared.symbol.name_span.start),
            address: declared.address.clone(),
            reach: declared.reach.clone(),
        }
    }
}

/// What a file declares, in order, with where a path reaches each item and
/// who may name it.
impl OutlineQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Outline, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph, engine.workspace())
    }

    pub(crate) fn execute_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<Outline, EngineError> {
        let path = workspace.normalize(&self.path);
        let candidate = graph.file(&path)?;
        let ns = graph.namespace(&candidate.language());
        let module = ns.as_ref().and_then(|ns| ns.address(&path).ok());
        let placed = ns.as_ref().zip(module.as_ref());
        let source = candidate.file().source();
        let items = candidate
            .facts()?
            .symbols
            .iter()
            .map(|symbol| OutlineItem {
                symbol: symbol.clone(),
                start: source.position(symbol.extent.start),
                end: source.position(symbol.extent.end),
                address: placed.and_then(|(ns, m)| ns.address_of(m, symbol)),
                reach: placed.map(|(ns, m)| ns.reach(m, symbol)),
            })
            .collect();
        Ok(Outline {
            path: path.into(),
            module,
            items,
        })
    }
}

/// Where `name` is declared, and the import that reaches each site from
/// `from`, spelled as that language writes it.
impl WhereQuery {
    /// Answer with the concrete result of this query.
    pub fn execute(self, engine: &crate::Engine) -> Result<Locations, EngineError> {
        let _operation = engine.operation();
        let mut graph = engine.graph()?;
        self.execute_in(&mut graph, engine.workspace())
    }

    pub(crate) fn execute_in(
        self,
        graph: &mut crate::graph::Graph,
        workspace: &crate::Workspace,
    ) -> Result<Locations, EngineError> {
        let name = self.name.as_str();
        let declarations = graph.declarations(&ReferencesQuery::new(name))?;
        let from = self.from.as_deref().map(|f| workspace.normalize(f));
        let mut sites = Vec::new();
        for declaration in declarations {
            let placed = graph.namespace(&declaration.language).and_then(|ns| {
                let module = ns.address(&declaration.path).ok()?;
                let address = declaration
                    .symbol
                    .as_ref()
                    .and_then(|s| ns.address_of(&module, s));
                let import = address.as_ref().and_then(|address| {
                    ns.surgery().ok()?.import_statement(
                        ns.project(),
                        from.as_deref()?,
                        address,
                        name,
                    )
                });
                Some((address, import))
            });
            let (address, import) = placed.unwrap_or((None, None));
            sites.push(Site {
                declaration,
                address,
                import,
            });
        }
        Ok(Locations {
            name: name.to_owned(),
            sites,
        })
    }
}

impl Document {
    pub(crate) fn outline(result: &Outline) -> Self {
        let mut report = Self::new();
        report.file_header(&result.path);
        if result.items.is_empty() {
            report.block_note(Block::Summary(
                Line::mark(Mark::Nothing).and(Role::Plain, " no declarations"),
            ));
            return report;
        }
        report.block_body(Block::Blank);
        report.block_body(Block::Outline {
            path: result.path.clone(),
            items: result.items.clone(),
        });
        report.block_note(Block::Summary(Self::outline_summary(result)));
        report
    }

    /// `● 14   pub 9  pub(crate) 1  · 4`: how many of each modifier.
    fn outline_summary(result: &Outline) -> Line {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for item in &result.items {
            if item.symbol.kind == SymbolKind::Impl {
                continue;
            }
            let key = item.symbol.modifier().unwrap_or("·").to_owned();
            match counts.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += 1,
                None => counts.push((key, 1)),
            }
        }
        counts.sort_by_key(|(k, _)| k == "·");
        let counts: Vec<Line> = counts
            .iter()
            .map(|(k, n)| {
                Line::of(Role::Dim, k.clone())
                    .and(Role::Plain, " ")
                    .and(Role::Plain, n.to_string())
            })
            .collect();
        Line::mark(Mark::Declaration)
            .and(Role::Plain, " ")
            .and(Role::Plain, result.items.len().to_string())
            .and(Role::Plain, "   ")
            .and_line(Self::join(counts, "  "))
    }

    pub(crate) fn locations(result: &Locations) -> Self {
        let mut report = Self::new();
        report.block_body(Block::Sites(result.sites.clone()));
        report.block_note(Block::Summary(
            Line::mark(Mark::Declaration)
                .and(Role::Plain, " ")
                .and(Role::Plain, result.sites.len().to_string()),
        ));
        report
    }
}
