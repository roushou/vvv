//! The read-only questions: `outline`, `where`, `deps`, `explain`, and a
//! file with its highlights. Each reads the graph and answers with plain
//! data; nothing is planned.

use vvv_core::{Address, Query};

use crate::command::{Command, Context};
use crate::{
    Deps, DepsQuery, EngineError, ExplainQuery, Explanation, File, FileQuery, Locations, Outline,
    OutlineItem, OutlineQuery, ReferencesQuery, Search, Site, WhereQuery,
};

/// What a file declares, in order, with where a path reaches each item and
/// who may name it.
impl Command for OutlineQuery {
    type Output = Outline;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let path = cx.workspace.normalize(&self.path);
        let graph = &mut *cx.graph;
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
impl Command for WhereQuery {
    type Output = Locations;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let name = self.name.as_str();
        let graph = &mut *cx.graph;
        let declarations = graph.declarations(&ReferencesQuery::new(name))?;
        let from = self.from.as_deref().map(|f| cx.workspace.normalize(f));
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

/// What a file imports and, across its language, who imports it — through
/// re-exports both ways: an import is followed to the declaration it
/// reaches, and a file importing one of this file's declarations under an
/// address a `pub use` offers it at is an importer.
impl Command for DepsQuery {
    type Output = Deps;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let path = cx.workspace.normalize(&self.path);
        let graph = &mut *cx.graph;
        let ns = graph.namespace_of(&path)?;
        let imports = graph.imports_of(&path)?;
        let own = ns.address(&path).ok();
        let importers = match &own {
            Some(own) => {
                let mut offered: Vec<Address> = Vec::new();
                for declared in &graph.file(&path)?.fragment(&ns)?.declarations {
                    offered.extend(graph.aliases_of(
                        &ns,
                        &declared.address,
                        &declared.symbol.name,
                    )?);
                }
                graph.importers(&ns, &path, |a| a.starts_with(own) || offered.contains(a))?
            }
            None => Vec::new(),
        };
        Ok(Deps {
            module: own,
            path: path.into(),
            imports,
            importers,
            skipped: Vec::new(),
        })
    }
}

/// What is at a position: the enclosing declaration, its address and reach,
/// and the files whose imports lead to it.
impl Command for ExplainQuery {
    type Output = Explanation;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let position = self.position;
        let path = cx.workspace.normalize(&self.path);
        let graph = &mut *cx.graph;
        let candidate = graph.file(&path)?;
        let source = candidate.file().source();
        let offset = source
            .offset(position)
            .ok_or_else(|| EngineError::NoSuchPosition {
                path: path.clone().into(),
                position,
            })?;
        let symbol = candidate.facts()?.enclosing(offset).cloned();
        let declared = symbol.as_ref().map(|s| source.position(s.name_span.start));
        let line = declared.and_then(|d| source.line(d.line as usize).map(str::to_owned));
        let mut explanation = Explanation {
            path: path.clone().into(),
            position,
            symbol,
            declared,
            line,
            module: None,
            address: None,
            reach: None,
            via: Vec::new(),
            import: None,
            importers: Vec::new(),
        };
        let Some(ns) = graph.namespace(&candidate.language()) else {
            return Ok(explanation);
        };
        let fragment = candidate.fragment(&ns)?;
        // Inside an import statement: the entry under the position, or the
        // statement's own path when the position is on none of its entries.
        let statement = fragment.imports().find(|e| {
            e.import.span.contains_offset(offset)
                || e.import
                    .group
                    .as_ref()
                    .is_some_and(|g| g.statement.contains_offset(offset))
        });
        if let Some(edge) = statement {
            explanation.import = Some(graph.dep(&ns, source, edge)?);
        }
        let Ok(module) = ns.address(&path) else {
            return Ok(explanation);
        };
        if let Some(symbol) = &explanation.symbol {
            explanation.address = ns.address_of(&module, symbol);
            explanation.reach = Some(ns.reach(&module, symbol));
        }
        // An import of the item (at any address a re-export offers it), of
        // something under it, or of an enclosing module leads to it; a bare
        // package root (`use vvv_core::{…}`'s prefix) leads everywhere and
        // counts for nothing.
        let target = explanation
            .address
            .clone()
            .unwrap_or_else(|| module.clone());
        let aliases = match (&explanation.address, &explanation.symbol) {
            (Some(address), Some(symbol)) => graph.aliases_of(&ns, address, &symbol.name)?,
            _ => vec![target.clone()],
        };
        explanation.importers = graph
            .importers(&ns, &path, |a| {
                aliases.contains(a)
                    || a.starts_with(&target)
                    || (!a.is_root() && target.starts_with(a))
            })?
            .into_iter()
            .map(|importer| importer.path)
            .collect();
        explanation.via = aliases[1..].to_vec();
        explanation.module = Some(module);
        Ok(explanation)
    }
}

/// One file as it is now, coloured by its language when one claims it.
impl Command for FileQuery {
    type Output = File;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        let path = self.path.as_path();
        let file = cx.workspace.load(path)?;
        let highlights = match cx.graph.language_of(path) {
            Some(language) => {
                language
                    .highlights(file.text())
                    .map_err(|source| EngineError::Search {
                        path: path.into(),
                        source,
                    })?
            }
            None => Vec::new(),
        };
        Ok(File {
            path: file.path().into(),
            text: file.text().to_owned(),
            highlights,
        })
    }
}

/// Structural or symbolic search across the workspace: only files spelling
/// the query's literal words are parsed.
impl Command for Query {
    type Output = Search;

    fn run(self, cx: &mut Context<'_>) -> Result<Self::Output, EngineError> {
        self.check()?;
        cx.graph.search(&self)
    }
}
