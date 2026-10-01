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
    syntax: super::navigation::NavigationSyntax,
}

impl<'g, L: LanguageExt> ImportExtractor<'g, L> {
    pub(crate) fn new(
        grammar: &'g ImportGrammar,
        lang: L,
        syntax: super::navigation::NavigationSyntax,
    ) -> Self {
        Self {
            grammar,
            lang,
            syntax,
        }
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
                        let Some(path_node) =
                            field.map_or(Some(node.clone()), |field| self.path_field(node, field))
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

    /// Only direct module statements expose bindings across source files.
    pub(crate) fn bindings(
        &self,
        root: &Node<'_, StrDoc<L>>,
        imports: &[ImportRef],
    ) -> Vec<vvv_core::ImportBinding> {
        if self
            .grammar
            .module_root
            .is_none_or(|kind| root.kind() != kind)
        {
            return vec![];
        }
        self.bindings_in(root, imports)
    }

    pub(crate) fn bindings_in(
        &self,
        root: &Node<'_, StrDoc<L>>,
        imports: &[ImportRef],
    ) -> Vec<vvv_core::ImportBinding> {
        let statements: Vec<_> = root
            .children()
            .filter(|node| self.grammar.statements.contains(&node.kind().as_ref()))
            .collect();
        imports
            .iter()
            .filter(|import| import.declares)
            .filter_map(|import| {
                let statement = statements
                    .iter()
                    .find(|node| Span::from(node.range()).contains(&import.span))?;
                let modifier = self.modifier(statement);
                Some(vvv_core::ImportBinding {
                    span: import.span,
                    visibility: modifier.as_ref().map(|node| vvv_core::Modifier {
                        span: node.range().into(),
                        text: node.text().into_owned(),
                    }),
                    restriction: modifier
                        .and_then(|node| self.restriction(&node))
                        .map(|node| self.grammar.syntax.parse(node.text().as_ref())),
                })
            })
            .collect()
    }

    fn path_field<'tree>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
        field: &str,
    ) -> Option<Node<'tree, StrDoc<L>>> {
        #[cfg(feature = "rust")]
        if matches!(self.syntax, super::navigation::NavigationSyntax::Rust)
            && field == "path"
            && let Some(alias) = super::rust::UseAlias::cast(node.clone())
            && let Ok(path) = alias.path()
        {
            return Some(path);
        }
        #[cfg(feature = "typescript")]
        if matches!(self.syntax, super::navigation::NavigationSyntax::TypeScript)
            && field == "source"
            && let Some(statement) = super::typescript::SourceStatement::cast(node.clone())
            && let Ok(source) = statement.source()
        {
            return source;
        }
        let _ = self.syntax;
        node.field(field)
    }

    pub(super) fn alias<'tree>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
    ) -> Option<Node<'tree, StrDoc<L>>> {
        let rule = self.grammar.alias?;
        if node.kind() != rule.under {
            return None;
        }
        #[cfg(feature = "rust")]
        if matches!(self.syntax, super::navigation::NavigationSyntax::Rust)
            && rule.field == "alias"
            && let Some(alias) = super::rust::UseAlias::cast(node.clone())
            && let Ok(name) = alias.alias()
        {
            return Some(name);
        }
        node.field(rule.field)
    }

    pub(super) fn modifier<'tree>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
    ) -> Option<Node<'tree, StrDoc<L>>> {
        let ReExportRule::Modifier(kind) = self.grammar.reexports else {
            return None;
        };
        #[cfg(feature = "rust")]
        if matches!(self.syntax, super::navigation::NavigationSyntax::Rust)
            && kind == "visibility_modifier"
        {
            return super::rust::Visibility::of(node).map(|visibility| visibility.syntax().clone());
        }
        node.children().find(|child| child.kind() == kind)
    }

    pub(super) fn restriction<'tree>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
    ) -> Option<Node<'tree, StrDoc<L>>> {
        #[cfg(feature = "rust")]
        if matches!(self.syntax, super::navigation::NavigationSyntax::Rust)
            && let Some(visibility) = super::rust::Visibility::cast(node.clone())
        {
            return visibility.restriction();
        }
        node.children().find(Node::is_named)
    }

    fn glob(&self, node: &Node<'_, StrDoc<L>>) -> bool {
        let Some(kind) = self.grammar.glob_under else {
            return false;
        };
        let Some(parent) = node.parent().filter(|parent| parent.kind() == kind) else {
            return false;
        };
        #[cfg(feature = "rust")]
        if matches!(self.syntax, super::navigation::NavigationSyntax::Rust)
            && let Some(glob) = super::rust::UseGlob::cast(parent.clone())
        {
            return glob
                .prefix()
                .is_some_and(|prefix| prefix.range() == node.range());
        }
        let _ = parent;
        true
    }
    #[cfg(feature = "rust")]
    fn rust_group(&self, node: &Node<'_, StrDoc<L>>) -> Option<(String, ImportGroup)> {
        let list = node.ancestors().find_map(super::rust::UseList::cast)?;
        if let Some(group) = list.syntax().parent().and_then(super::rust::UseGroup::cast)
            && let Ok(declared_list) = group.list()
            && declared_list.range() != list.syntax().range()
        {
            return None;
        }
        let item = list.entry_for(node)?;
        let statement = node.ancestors().find_map(super::rust::Import::cast)?;
        let own = node.range();
        let mut prefixes: Vec<_> = node
            .ancestors()
            .filter_map(super::rust::UseGroup::cast)
            .filter_map(|group| {
                group
                    .prefix()
                    .ok()
                    .flatten()
                    .or_else(|| group.syntax().field("path"))
            })
            .filter(|prefix| !(prefix.range().start <= own.start && own.end <= prefix.range().end))
            .map(|prefix| prefix.text().into_owned())
            .collect();
        prefixes.reverse();
        let prefix = prefixes.join(self.grammar.syntax.separator());
        let group = ImportGroup {
            prefix: self.grammar.syntax.parse(&prefix),
            item: item.range().into(),
            list: list.syntax().range().into(),
            items: list.entries().count(),
            statement: statement.syntax().range().into(),
            top_level: list.scoped_argument_of(statement.syntax()),
        };
        Some((prefix, group))
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
        let glob = self.glob(&node);
        let statement = node
            .ancestors()
            .find(|a| self.grammar.statements.contains(&a.kind().as_ref()));
        let in_modifier = match self.grammar.reexports {
            ReExportRule::Modifier(kind) => {
                node.ancestors().any(|ancestor| ancestor.kind() == kind)
            }
            _ => false,
        };
        let declares = !in_modifier && (self.grammar.statements.is_empty() || statement.is_some());
        let reexport = declares
            && match self.grammar.reexports {
                ReExportRule::Never => false,
                ReExportRule::Modifier(kind) => statement
                    .as_ref()
                    .is_some_and(|s| s.children().any(|c| c.kind() == kind)),
                ReExportRule::Statement(kind) => node.ancestors().any(|a| a.kind() == kind),
            };
        let alias = self.grammar.alias.and_then(|rule| {
            let under = node.parent().filter(|p| p.kind() == rule.under)?;
            Some(Name::from(self.alias(&under)?.text().as_ref()))
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
        #[cfg(feature = "rust")]
        if matches!(self.syntax, super::navigation::NavigationSyntax::Rust)
            && nesting.list == "use_list"
            && nesting.scope == "scoped_use_list"
            && nesting.prefix_field == "path"
            && nesting.statement == "use_declaration"
        {
            return self.rust_group(node);
        }
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
