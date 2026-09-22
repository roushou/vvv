use std::collections::BTreeMap;

use ast_grep_core::meta_var::{MetaVarEnv, MetaVariable};
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_core::{Doc, Node, NodeMatch};
use vvv_core::{
    Capture, CaptureValue, Facts, Grammar, Highlight, ImportRef, ImportRule, PathSyntax, Query,
    RawMatch, Role, SearchError, Span, Symbol, SymbolKind,
};

use super::compiled::CompiledQuery;
use super::highlights::Highlighter;
use super::imports::ImportExtractor;
use super::symbols::SymbolExtractor;

/// What a query found in one file before declarations are attached.
enum Hits {
    /// Identifier tokens spelling a bare name.
    Names(Vec<RawMatch>),
    /// Nodes matched by a pattern or kind.
    Nodes(Vec<RawMatch>),
    /// No structural part: the declarations themselves are the matches.
    Declarations,
}

/// Search and symbol extraction over one grammar.
///
/// The grammar comes from `L`; what counts as a declaration or an identifier
/// is declared by the plugin as data.
#[derive(Debug, Clone)]
pub struct AstGrepSearcher<L> {
    lang: L,
    grammar: Grammar,
}

impl<L: LanguageExt> AstGrepSearcher<L> {
    pub fn new(lang: L, grammar: Grammar) -> Self {
        Self { lang, grammar }
    }

    /// How this grammar spells paths.
    pub fn paths(&self) -> PathSyntax {
        self.grammar.imports.syntax
    }

    pub fn glob_marker(&self) -> Option<&'static str> {
        self.grammar.imports.glob_marker
    }

    pub fn highlights(&self, source: &str) -> Vec<Highlight> {
        let root = self.lang.ast_grep(source);
        Highlighter::new(self.grammar.highlights).extract(&root.root())
    }

    pub fn imports(&self, source: &str) -> Result<Vec<ImportRef>, SearchError> {
        let root = self.lang.ast_grep(source);
        ImportExtractor::new(&self.grammar.imports, self.lang.clone()).extract(&root.root())
    }

    /// What `find` would compile for `query`, without a file: a bare name
    /// needs no pattern, anything else must parse in this grammar.
    pub fn accepts(&self, query: &Query) -> Result<(), SearchError> {
        let bare = query
            .pattern_str()
            .is_some_and(|p| Self::is_bare_name(p) && query.kind_str().is_none());
        if !bare {
            CompiledQuery::compile(query, &self.lang)?;
        }
        Ok(())
    }

    pub fn find(&self, source: &str, query: &Query) -> Result<Vec<RawMatch>, SearchError> {
        let root = self.lang.ast_grep(source);
        let node = root.root();

        // A bare word with no kind is a name, not a structure: every
        // identifier token spelling it, whatever node kind the grammar gives
        // that position.
        let bare = query
            .pattern_str()
            .filter(|p| Self::is_bare_name(p) && query.kind_str().is_none());
        let hits = match bare {
            Some(name) => Hits::Names(self.identifiers(&node, name)),
            None => match CompiledQuery::compile(query, &self.lang)? {
                Some(compiled) => Hits::Nodes(
                    self.structural(&node, compiled)
                        .iter()
                        .map(|m| self.lower(m))
                        .collect(),
                ),
                None => Hits::Declarations,
            },
        };

        // Symbols only pay off once something matched; most files miss.
        let mut matches = match hits {
            Hits::Names(m) | Hits::Nodes(m) if m.is_empty() => return Ok(m),
            Hits::Declarations => SymbolExtractor::new(self.grammar.symbols)
                .extract(&node, source)
                .into_iter()
                .map(|(node, symbol)| RawMatch {
                    symbol: Some(symbol),
                    ..RawMatch::plain(node.range().into(), node.kind(), node.text())
                })
                .collect(),
            // A structural hit is a whole declaration; a name hit is its identifier.
            Hits::Names(m) => self.attach_symbols(&node, source, m, |s| s.name_span),
            Hits::Nodes(m) => self.attach_symbols(&node, source, m, |s| s.span),
        };

        // An `impl` block is a fact for moves, not a declaration: a search
        // neither lists it nor labels the type name in `impl Foo` with it,
        // unless asked for `--symbol impl` by name.
        if query.symbol() != Some(SymbolKind::Impl) {
            for m in &mut matches {
                if m.symbol
                    .as_ref()
                    .is_some_and(|s| s.kind == SymbolKind::Impl)
                {
                    m.symbol = None;
                }
            }
            if !query.is_structural() {
                matches.retain(|m| m.symbol.is_some());
            }
        }
        if query.is_symbolic() {
            matches.retain(|m| {
                m.symbol.as_ref().is_some_and(|s| {
                    query.symbol().is_none_or(|k| k == s.kind)
                        && query.name().is_none_or(|n| n == s.name)
                })
            });
        }
        for m in &mut matches {
            if m.symbol.is_some() {
                m.role = Role::Declaration;
            }
        }
        Ok(matches)
    }

    /// Whether `node` sits in an import statement: under one of the kinds
    /// the import grammar names, or, when it names none, under a node an
    /// import rule reads a path field from (`import_statement` with a
    /// `source`; not a bare `export const`).
    fn in_import(&self, node: &Node<'_, StrDoc<L>>) -> bool {
        let imports = &self.grammar.imports;
        std::iter::once(node.clone())
            .chain(node.ancestors())
            .any(|n| {
                if !imports.statements.is_empty() {
                    return imports.statements.contains(&n.kind().as_ref());
                }
                imports.rules.iter().any(|rule| match rule {
                    ImportRule::Node {
                        kind,
                        field: Some(field),
                        ..
                    } => n.kind() == *kind && n.field(field).is_some(),
                    _ => false,
                })
            })
    }

    /// Give each match the declaration whose `key` span it sits on exactly.
    fn attach_symbols(
        &self,
        node: &Node<'_, StrDoc<L>>,
        source: &str,
        matches: Vec<RawMatch>,
        key: fn(&Symbol) -> Span,
    ) -> Vec<RawMatch> {
        let symbols = SymbolExtractor::new(self.grammar.symbols).extract(node, source);
        let by_key: BTreeMap<Span, &Symbol> = symbols.iter().map(|(_, s)| (key(s), s)).collect();
        matches
            .into_iter()
            .map(|mut raw| {
                raw.symbol = by_key.get(&raw.span).map(|s| (*s).clone());
                raw
            })
            .collect()
    }

    /// A single identifier-like token: no meta-variables, no punctuation.
    fn is_bare_name(pattern: &str) -> bool {
        let mut chars = pattern.chars();
        chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
            && chars.all(|c| c.is_alphanumeric() || c == '_')
    }

    pub fn symbols(&self, source: &str) -> Vec<Symbol> {
        let root = self.lang.ast_grep(source);
        SymbolExtractor::new(self.grammar.symbols)
            .extract(&root.root(), source)
            .into_iter()
            .map(|(_, symbol)| symbol)
            .collect()
    }

    /// Everything about `source` from one parse.
    pub fn facts(&self, source: &str) -> Result<Facts, SearchError> {
        let root = self.lang.ast_grep(source);
        let node = root.root();
        let symbols = SymbolExtractor::new(self.grammar.symbols)
            .extract(&node, source)
            .into_iter()
            .map(|(_, symbol)| symbol)
            .collect();
        let imports =
            ImportExtractor::new(&self.grammar.imports, self.lang.clone()).extract(&node)?;
        let highlights = Highlighter::new(self.grammar.highlights).extract(&node);
        let mut facts = Facts::new(symbols, imports, highlights);
        let kinds: Vec<u16> = self
            .grammar
            .identifiers
            .iter()
            .map(|kind| self.lang.kind_to_id(kind))
            .collect();
        for n in node.dfs().filter(|n| kinds.contains(&n.kind_id())) {
            facts.push_token(&n.text(), &n.kind(), n.range().into());
        }
        Ok(facts)
    }

    pub fn references(&self, source: &str, name: &str) -> Vec<RawMatch> {
        let root = self.lang.ast_grep(source);
        self.identifiers(&root.root(), name)
    }

    /// Every identifier token spelling `name`, in source order: one pre-order
    /// walk, comparing kind ids before touching text.
    fn identifiers(&self, node: &Node<'_, StrDoc<L>>, name: &str) -> Vec<RawMatch> {
        let kinds: Vec<u16> = self
            .grammar
            .identifiers
            .iter()
            .map(|kind| self.lang.kind_to_id(kind))
            .collect();
        node.dfs()
            .filter(|n| kinds.contains(&n.kind_id()) && n.text() == name)
            .map(|n| RawMatch {
                role: self.role_of(&n),
                ..RawMatch::plain(n.range().into(), n.kind(), n.text())
            })
            .collect()
    }

    fn structural<'t>(
        &self,
        node: &Node<'t, StrDoc<L>>,
        compiled: CompiledQuery,
    ) -> Vec<NodeMatch<'t, StrDoc<L>>> {
        match compiled {
            CompiledQuery::Pattern(pattern) => node.find_all(pattern).collect(),
            CompiledQuery::Kind(kind) => node.find_all(kind).collect(),
            CompiledQuery::PatternOfKind { pattern, kind } => node
                .find_all(pattern)
                .filter(|m| m.get_node().kind_id() == kind)
                .collect(),
        }
    }

    fn lower(&self, m: &NodeMatch<'_, StrDoc<L>>) -> RawMatch {
        let node = m.get_node();
        RawMatch {
            captures: Self::captures(m.get_env()),
            role: self.role_of(node),
            ..RawMatch::plain(node.range().into(), node.kind(), node.text())
        }
    }

    /// `Import` inside an import statement, else `Use`; `Declaration` is
    /// decided once symbols are attached.
    fn role_of(&self, node: &Node<'_, StrDoc<L>>) -> Role {
        if self.in_import(node) {
            Role::Import
        } else {
            Role::Use
        }
    }

    fn captures(env: &MetaVarEnv<'_, StrDoc<L>>) -> BTreeMap<String, CaptureValue> {
        env.get_matched_variables()
            .filter_map(|var| match var {
                MetaVariable::Capture(name, _) => env
                    .get_match(&name)
                    .map(|node| (name, CaptureValue::Single(Self::capture(node)))),
                MetaVariable::MultiCapture(name) => {
                    let nodes = env.get_multiple_matches(&name);
                    Some((
                        name,
                        CaptureValue::Multiple(nodes.iter().map(Self::capture).collect()),
                    ))
                }
                MetaVariable::Dropped(_) | MetaVariable::Multiple => None,
            })
            .collect()
    }

    fn capture<D: Doc>(node: &Node<'_, D>) -> Capture {
        Capture {
            span: Span::from(node.range()),
            text: node.text().into_owned(),
        }
    }
}
