//! Grammar-driven lexical and named-import facts. Parser nodes stay in this adapter.
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{Facts, Grammar, LexicalBinding, NamedImport, Span, Symbol};

pub struct NavigationFacts<'a> {
    grammar: &'a Grammar,
}
impl<'a> NavigationFacts<'a> {
    pub fn new(grammar: &'a Grammar) -> Self {
        Self { grammar }
    }
    pub fn extract<L: LanguageExt>(&self, root: &Node<'_, StrDoc<L>>, facts: &mut Facts) {
        facts.named_modules = self.grammar.named_modules;
        facts.non_named_exports = facts
            .symbols
            .iter()
            .filter(|s| {
                s.modifier()
                    .is_some_and(|m| self.grammar.non_named_exports.contains(&m))
            })
            .map(|s| s.name_span)
            .collect();
        let mut unsupported = Vec::new();
        for node in root.dfs() {
            for rule in self
                .grammar
                .bindings
                .iter()
                .filter(|r| r.node == node.kind())
            {
                let Some(scope) = std::iter::once(node.clone())
                    .chain(node.ancestors())
                    .take_while(|a| {
                        a.range() == node.range()
                            || rule.scopes.contains(&a.kind().as_ref())
                            || (!self.grammar.lexical_boundaries.contains(&a.kind().as_ref())
                                && !self.grammar.lexical_barriers.contains(&a.kind().as_ref()))
                    })
                    .find(|a| rule.scopes.contains(&a.kind().as_ref()))
                else {
                    continue;
                };
                let Some(name) = rule.name.and_then(|field| node.field(field)) else {
                    unsupported.push(scope.range().into());
                    continue;
                };
                let Some(names) = self.pattern_names(&name) else {
                    unsupported.push(scope.range().into());
                    continue;
                };
                let scope_span: Span = scope.range().into();
                let excluded: Vec<Span> = scope
                    .dfs()
                    .filter(|n| {
                        n.range() != scope.range()
                            && self.grammar.lexical_boundaries.contains(&n.kind().as_ref())
                    })
                    .filter(|n| {
                        // Impl/trait type parameters are visible in their direct methods,
                        // but a nested item inside a method cannot capture them.
                        !self
                            .grammar
                            .lexical_containers
                            .contains(&scope.kind().as_ref())
                            || n.ancestors()
                                .take_while(|a| a.range() != scope.range())
                                .any(|a| {
                                    self.grammar.lexical_boundaries.contains(&a.kind().as_ref())
                                })
                    })
                    .map(|n| n.range().into())
                    .collect();
                for name in names {
                    facts.lexical.push(LexicalBinding {
                        symbol: facts
                            .symbols
                            .iter()
                            .find(|s| {
                                s.name_span == Span::from(name.range()) && s.kind == rule.kind
                            })
                            .cloned()
                            .unwrap_or_else(|| {
                                Symbol::plain(
                                    rule.kind,
                                    name.text().as_ref(),
                                    name.range().into(),
                                    node.range().into(),
                                )
                            }),
                        scope: scope_span,
                        excluded: excluded.clone(),
                        visible_from: if rule.after {
                            node.range().end
                        } else {
                            scope_span.start
                        },
                        namespace: rule.namespace,
                    });
                }
            }
            for rule in self
                .grammar
                .named_imports
                .iter()
                .filter(|r| r.node == node.kind())
            {
                let Some(statement) = node.ancestors().find(|a| a.kind() == rule.statement) else {
                    continue;
                };
                let name = match rule.name {
                    Some(field) => node.field(field),
                    None => node.children().find(|n| n.kind() == "identifier"),
                };
                let Some(name) = name else {
                    continue;
                };
                let alias = node.field("alias").unwrap_or_else(|| name.clone());
                let module = if let Some(source) = statement.field("source") {
                    let Some(import) = facts
                        .imports
                        .iter()
                        .find(|i| Span::from(source.range()).contains(&i.span))
                    else {
                        continue;
                    };
                    Some(import.path.clone())
                } else if rule.reexport {
                    None
                } else {
                    continue;
                };
                facts.named_imports.push(NamedImport {
                    local: alias.text().into_owned(),
                    imported: rule
                        .imported
                        .map(str::to_owned)
                        .unwrap_or_else(|| name.text().into_owned()),
                    module,
                    name_span: name.range().into(),
                    alias_span: alias.range().into(),
                    reexport: rule.reexport,
                    type_only: node.text().starts_with("type ")
                        || statement.text().starts_with("import type ")
                        || statement.text().starts_with("export type "),
                });
            }
        }
        for node in root
            .dfs()
            .filter(|n| self.grammar.identifiers.contains(&n.kind().as_ref()))
        {
            let span: Span = node.range().into();
            // Field/member and qualified-path segments are not bare lexical uses.
            let qualified = node
                .parent()
                .is_some_and(|p| self.grammar.lexical_qualified.contains(&p.kind().as_ref()));
            let supported = !unsupported.iter().any(|s: &Span| s.contains(&span))
                && !node
                    .ancestors()
                    .any(|a| self.barrier(&a, facts, self.grammar.lexical_barriers));
            if supported && !qualified {
                facts.lexical_tokens.push(span);
                if self
                    .grammar
                    .navigation_values
                    .contains(&node.kind().as_ref())
                {
                    facts.navigation.push(span);
                }
            }
            // Path facts distinguish qualified module uses from receiver members.
            // A lexical type binding at the head needs associated-item inference.
            if supported
                && facts.imports.iter().any(|import| {
                    !import.declares
                        && import.span.contains(&span)
                        && !facts.lexical.iter().any(|binding| {
                            import
                                .path
                                .first()
                                .is_some_and(|head| head.as_str() == binding.symbol.name)
                                && binding.namespace == vvv_core::BindingNamespace::Type
                                && binding.scope.contains(&span)
                        })
                })
            {
                facts.navigation.push(span);
            }
            if supported && let Some(parent) = node.parent() {
                for rule in self
                    .grammar
                    .qualified_imports
                    .iter()
                    .filter(|r| r.node == parent.kind())
                {
                    if let (Some(object), Some(member)) =
                        (parent.field(rule.object), parent.field(rule.member))
                        && object.kind() == "identifier"
                        && member.range() == node.range()
                        && !facts
                            .lexical
                            .iter()
                            .any(|b| b.symbol.name == object.text() && b.scope.contains(&span))
                    {
                        facts.qualified_imports.push(vvv_core::QualifiedImport {
                            span,
                            binding: object.text().into_owned(),
                            member: member.text().into_owned(),
                        });
                    }
                }
            }
        }
    }
    pub(crate) fn barrier<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        facts: &Facts,
        barriers: &[&str],
    ) -> bool {
        if !barriers.contains(&node.kind().as_ref()) {
            return false;
        }
        !self.grammar.module_scopes.is_some_and(|rule| {
            node.kind() == rule.node
                && node.field(rule.name).is_some_and(|name| {
                    facts
                        .module_scopes
                        .iter()
                        .any(|scope| scope.declaration == Some(name.range().into()))
                })
        })
    }
    fn pattern_names<'t, L: LanguageExt>(
        &self,
        node: &Node<'t, StrDoc<L>>,
    ) -> Option<Vec<Node<'t, StrDoc<L>>>> {
        if matches!(node.kind().as_ref(), "identifier" | "type_identifier") {
            return Some(vec![node.clone()]);
        }
        if node.kind() == "_" || node.kind() == "mutable_specifier" {
            return Some(vec![]);
        }
        // Typed parameters are lowered independently by their binding rule.
        if self
            .grammar
            .bindings
            .iter()
            .any(|r| r.node == node.kind() && r.name == Some("pattern"))
        {
            return Some(vec![]);
        }
        if !self
            .grammar
            .pattern_containers
            .contains(&node.kind().as_ref())
        {
            return None;
        }
        let mut names = Vec::new();
        for child in node.children().filter(Node::is_named) {
            names.extend(self.pattern_names(&child)?);
        }
        Some(names)
    }
}
