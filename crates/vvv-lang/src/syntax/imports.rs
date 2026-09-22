use ast_grep_core::matcher::KindMatcher;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use ast_grep_core::{Node, Pattern};
use vvv_core::{
    ImportGrammar, ImportGroup, ImportNesting, ImportRef, ImportRule, Name, ReExportRule,
    SearchError, Span,
};

/// Applies an [`ImportGrammar`] to a tree.
pub(crate) struct ImportExtractor<'g, L> {
    grammar: &'g ImportGrammar,
    lang: L,
}

impl<'g, L: LanguageExt> ImportExtractor<'g, L> {
    pub(crate) fn new(grammar: &'g ImportGrammar, lang: L) -> Self {
        Self { grammar, lang }
    }

    pub(crate) fn extract(
        &self,
        root: &Node<'_, StrDoc<L>>,
    ) -> Result<Vec<ImportRef>, SearchError> {
        let mut found: Vec<ImportRef> = Vec::new();
        for rule in self.grammar.rules {
            match *rule {
                ImportRule::Node { kind, field, under } => {
                    let matcher = KindMatcher::try_new(kind, self.lang.clone())
                        .map_err(|e| SearchError::Kind(e.to_string()))?;
                    for m in root.find_all(matcher) {
                        let node = m.get_node();
                        if under.is_some_and(|p| node.parent().is_none_or(|n| n.kind() != p)) {
                            continue;
                        }
                        let Some(path_node) = field.map_or(Some(node.clone()), |f| node.field(f))
                        else {
                            continue;
                        };
                        self.push(&mut found, &path_node);
                    }
                }
                ImportRule::Pattern { pattern, capture } => {
                    let pattern = Pattern::try_new(pattern, self.lang.clone())
                        .map_err(|e| SearchError::Pattern(e.to_string()))?;
                    for m in root.find_all(pattern) {
                        if let Some(path_node) = m.get_env().get_match(capture) {
                            self.push(&mut found, path_node);
                        }
                    }
                }
            }
        }
        Ok(Self::outermost(found))
    }

    /// Drop candidates nested inside another candidate; `crate::a::b` is one
    /// reference, not three.
    fn outermost(mut candidates: Vec<ImportRef>) -> Vec<ImportRef> {
        candidates.sort_by_key(|r| (r.span.start, std::cmp::Reverse(r.span.end)));
        let mut kept: Vec<ImportRef> = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let covered = kept
                .last()
                .is_some_and(|outer| outer.span.contains(&candidate.span));
            if !covered {
                kept.push(candidate);
            }
        }
        kept
    }

    fn push(&self, found: &mut Vec<ImportRef>, path_node: &Node<'_, StrDoc<L>>) {
        let node = Self::unquote(path_node);
        let span = Span::from(node.range());
        let text = node.text().into_owned();
        let syntax = self.grammar.syntax;
        let (path, group) = match self.group_of(&node) {
            Some((prefix, group)) => (
                syntax.parse(&format!("{prefix}{}{text}", syntax.separator())),
                Some(group),
            ),
            None => (syntax.parse(&text), None),
        };
        let glob = self
            .grammar
            .glob_under
            .is_some_and(|kind| node.parent().is_some_and(|p| p.kind() == kind));
        let statement = node
            .ancestors()
            .find(|a| self.grammar.statements.contains(&a.kind().as_ref()));
        let declares = self.grammar.statements.is_empty() || statement.is_some();
        let reexport = match self.grammar.reexports {
            ReExportRule::Never => false,
            ReExportRule::Modifier(kind) => statement
                .as_ref()
                .is_some_and(|s| s.children().any(|c| c.kind() == kind)),
            ReExportRule::Statement(kind) => node.ancestors().any(|a| a.kind() == kind),
        };
        let alias = self.grammar.alias.and_then(|rule| {
            let under = node.parent().filter(|p| p.kind() == rule.under)?;
            Some(Name::from(under.field(rule.field)?.text().as_ref()))
        });
        found.push(ImportRef {
            span,
            path,
            group,
            glob,
            declares,
            reexport,
            alias,
        });
    }

    /// For a string literal, the fragment between the quotes.
    fn unquote<'t>(node: &Node<'t, StrDoc<L>>) -> Node<'t, StrDoc<L>> {
        if node.kind() != "string" {
            return node.clone();
        }
        node.children()
            .find(|c| c.kind() == "string_fragment")
            .unwrap_or_else(|| node.clone())
    }

    /// The combined prefix and group context of `node`, if it is an entry
    /// of a grouped import.
    fn group_of(&self, node: &Node<'_, StrDoc<L>>) -> Option<(String, ImportGroup)> {
        let syntax = self.grammar.syntax;
        let nesting = self.grammar.nesting?;
        let list = node.ancestors().find(|a| a.kind() == nesting.list)?;
        let item = Self::child_containing(&list, node)?;
        let statement = node.ancestors().find(|a| a.kind() == nesting.statement)?;
        let prefix = self.prefix_of(&nesting, node);
        let top_level = list
            .parent()
            .and_then(|scope| scope.parent())
            .is_some_and(|p| p.range() == statement.range());
        let group = ImportGroup {
            prefix: syntax.parse(&prefix),
            item: item.range().into(),
            list: list.range().into(),
            items: list.children().filter(Node::is_named).count(),
            statement: statement.range().into(),
            top_level,
        };
        Some((prefix, group))
    }

    /// Enclosing group prefixes, outermost first, joined. A scope whose
    /// prefix is `node` itself does not count: `b` in `use a::{b::{c}}` is
    /// prefixed by `a`, not by `a::b`.
    fn prefix_of(&self, nesting: &ImportNesting, node: &Node<'_, StrDoc<L>>) -> String {
        let own = node.range();
        let mut prefixes: Vec<String> = node
            .ancestors()
            .filter(|a| a.kind() == nesting.scope)
            .filter_map(|a| a.field(nesting.prefix_field))
            .filter(|p| !(p.range().start <= own.start && own.end <= p.range().end))
            .map(|p| p.text().into_owned())
            .collect();
        prefixes.reverse();
        prefixes.join(self.grammar.syntax.separator())
    }

    /// The direct child of `parent` whose range contains `node`.
    fn child_containing<'t>(
        parent: &Node<'t, StrDoc<L>>,
        node: &Node<'t, StrDoc<L>>,
    ) -> Option<Node<'t, StrDoc<L>>> {
        let range = node.range();
        parent
            .children()
            .find(|c| c.range().start <= range.start && range.end <= c.range().end)
    }
}
