//! Grammar-driven lexical and named-import facts. Parser nodes stay in this adapter.

use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use std::collections::{BTreeMap, BTreeSet};
use vvv_core::{
    BindingNamespace, BindingRule, Facts, Grammar, LexicalBinding, NamedImport, Span, Symbol,
    SymbolKind,
};

/// Selects the parser-side navigation implementation; the plugin facts remain data.
#[derive(Debug, Clone, Copy, Default)]
pub enum NavigationSyntax {
    #[default]
    Tables,
    #[cfg(feature = "rust")]
    Rust,
    #[cfg(feature = "typescript")]
    TypeScript,
}

#[derive(Default)]
pub(crate) struct NavigationCoverage {
    blocked: Vec<Span>,
    modeled: BTreeSet<Span>,
    exclusions: BTreeMap<Span, Vec<Span>>,
}

impl NavigationCoverage {
    pub(crate) fn block(&mut self, span: Span) {
        self.blocked.push(span);
    }
    #[cfg(feature = "rust")]
    pub(crate) fn model(&mut self, span: Span) {
        self.modeled.insert(span);
    }

    pub(crate) fn supports(&self, span: Span) -> bool {
        !self.blocked.iter().any(|blocked| blocked.contains(&span))
    }
}

pub(crate) struct BindingName {
    pub name: String,
    pub span: Span,
    pub explicit: bool,
}

impl BindingName {
    pub(crate) fn from_node<L: LanguageExt>(
        name: &Node<'_, StrDoc<L>>,
        declaration: &Node<'_, StrDoc<L>>,
        grammar: &Grammar,
    ) -> Self {
        Self {
            name: name.text().into_owned(),
            span: name.range().into(),
            explicit: declaration
                .children()
                .any(|child| grammar.binding_markers.contains(&child.kind().as_ref()))
                || name
                    .ancestors()
                    .take_while(|ancestor| ancestor.range() != declaration.range())
                    .any(|ancestor| {
                        ancestor
                            .children()
                            .any(|child| grammar.binding_markers.contains(&child.kind().as_ref()))
                    }),
        }
    }
}

pub(crate) struct BindingSite {
    pub declaration: Span,
    pub scope: Span,
    pub excluded: Vec<Span>,
    pub visible_from: usize,
    pub namespace: BindingNamespace,
    pub kind: SymbolKind,
}

impl BindingSite {
    pub(crate) fn emit(&self, names: Vec<BindingName>, facts: &mut Facts) {
        for name in names {
            facts.lexical.push(LexicalBinding {
                symbol: facts
                    .symbols
                    .iter()
                    .find(|symbol| symbol.name_span == name.span && symbol.kind == self.kind)
                    .cloned()
                    .unwrap_or_else(|| {
                        Symbol::plain(self.kind, name.name, name.span, self.declaration)
                    }),
                scope: self.scope,
                excluded: self.excluded.clone(),
                visible_from: self.visible_from,
                namespace: self.namespace,
                explicit: name.explicit,
            });
        }
    }
}

pub struct NavigationFacts<'a> {
    pub(crate) grammar: &'a Grammar,
}

impl<'a> NavigationFacts<'a> {
    pub fn new(grammar: &'a Grammar) -> Self {
        Self { grammar }
    }

    pub(crate) fn extract<L: LanguageExt>(
        &self,
        root: &Node<'_, StrDoc<L>>,
        facts: &mut Facts,
        syntax: NavigationSyntax,
    ) -> NavigationCoverage {
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
        let mut coverage = NavigationCoverage::default();
        #[cfg(feature = "rust")]
        let rust = super::rust::RustNavigation::new(self);
        #[cfg(feature = "typescript")]
        let typescript = super::typescript::TypeScriptNavigation::new(self);
        for node in root.dfs() {
            if matches!(syntax, NavigationSyntax::Tables) {
                self.macro_scope(&node, facts);
            }
            let handled = match syntax {
                NavigationSyntax::Tables => false,
                #[cfg(feature = "typescript")]
                NavigationSyntax::TypeScript => {
                    typescript.extract(&node, facts, &mut coverage);
                    true
                }
                #[cfg(feature = "rust")]
                NavigationSyntax::Rust => rust.extract(&node, facts, &mut coverage),
            };
            if !handled {
                self.table_bindings(&node, facts, &mut coverage);
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
                let name = self.import_name(&node, rule.name, syntax);
                let Some(name) = name else {
                    continue;
                };
                let alias = self
                    .import_alias(&node, syntax)
                    .unwrap_or_else(|| name.clone());
                let module = if let Some(source) = self.import_source(&statement, syntax) {
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
                    type_only: self.import_type_only(&node, &statement, syntax),
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
            let supported = coverage.supports(span)
                && !node
                    .ancestors()
                    .any(|a| self.barrier(&a, facts, self.grammar.lexical_barriers, &coverage));
            if supported
                && facts
                    .patterns
                    .iter()
                    .flat_map(|pattern| &pattern.references)
                    .any(|reference| reference.span == span)
            {
                facts.navigation.push(span);
            }
            if supported && !qualified {
                if !facts
                    .patterns
                    .iter()
                    .flat_map(|pattern| &pattern.references)
                    .any(|reference| {
                        reference.span == span
                            && reference.role != vvv_core::PatternRole::Identifier
                    })
                {
                    facts.lexical_tokens.push(span);
                }
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
        coverage
    }

    fn import_name<'tree, L: LanguageExt>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
        field: Option<&str>,
        syntax: NavigationSyntax,
    ) -> Option<Node<'tree, StrDoc<L>>> {
        #[cfg(feature = "typescript")]
        if matches!(syntax, NavigationSyntax::TypeScript) {
            if field == Some("name")
                && let Some(specifier) = super::typescript::NamedSpecifier::cast(node.clone())
                && let Ok(name) = specifier.name()
            {
                return Some(name);
            }
            if field.is_none()
                && let Some(clause) = super::typescript::ImportClause::cast(node.clone())
            {
                return clause.name();
            }
        }
        let _ = syntax;
        match field {
            Some(field) => node.field(field),
            None => node.children().find(|node| node.kind() == "identifier"),
        }
    }

    fn import_alias<'tree, L: LanguageExt>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
        syntax: NavigationSyntax,
    ) -> Option<Node<'tree, StrDoc<L>>> {
        #[cfg(feature = "typescript")]
        if matches!(syntax, NavigationSyntax::TypeScript)
            && let Some(specifier) = super::typescript::NamedSpecifier::cast(node.clone())
            && let Ok(alias) = specifier.alias()
        {
            return alias;
        }
        let _ = syntax;
        node.field("alias")
    }

    fn import_source<'tree, L: LanguageExt>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
        syntax: NavigationSyntax,
    ) -> Option<Node<'tree, StrDoc<L>>> {
        #[cfg(feature = "typescript")]
        if matches!(syntax, NavigationSyntax::TypeScript)
            && let Some(statement) = super::typescript::SourceStatement::cast(node.clone())
            && let Ok(source) = statement.source()
        {
            return source;
        }
        let _ = syntax;
        node.field("source")
    }

    fn import_type_only<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        statement: &Node<'_, StrDoc<L>>,
        syntax: NavigationSyntax,
    ) -> bool {
        #[cfg(feature = "typescript")]
        if matches!(syntax, NavigationSyntax::TypeScript)
            && let Some(statement) = super::typescript::SourceStatement::cast(statement.clone())
        {
            return statement.type_only()
                || super::typescript::NamedSpecifier::cast(node.clone()).map_or_else(
                    || node.text().starts_with("type "),
                    |specifier| specifier.type_only(),
                );
        }
        let _ = syntax;
        node.text().starts_with("type ")
            || statement.text().starts_with("import type ")
            || statement.text().starts_with("export type ")
    }

    pub(crate) fn table_bindings<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) {
        for rule in self
            .grammar
            .bindings
            .iter()
            .filter(|r| r.node == node.kind())
        {
            let Some(scope) = self.scope(node, rule, coverage) else {
                continue;
            };
            let Some(name) = rule.name.and_then(|field| node.field(field)) else {
                if self
                    .grammar
                    .imports
                    .statements
                    .contains(&node.kind().as_ref())
                    && facts.import_scopes.iter().any(|imports| {
                        imports.span == Span::from(scope.range())
                            && imports
                                .imports
                                .iter()
                                .any(|binding| Span::from(node.range()).contains(&binding.span))
                    })
                {
                    continue;
                }
                coverage.block(scope.range().into());
                continue;
            };
            let Some(names) = super::bindings::PatternNames::new(self.grammar)
                .names(&super::bindings::TablePattern::new(name))
            else {
                coverage.block(scope.range().into());
                continue;
            };
            let names = names
                .iter()
                .map(|name| BindingName::from_node(name, node, self.grammar))
                .collect();
            self.site(node, &scope, rule, coverage).emit(names, facts);
        }
    }

    pub(crate) fn scope<'t, L: LanguageExt>(
        &self,
        node: &Node<'t, StrDoc<L>>,
        rule: &BindingRule,
        coverage: &NavigationCoverage,
    ) -> Option<Node<'t, StrDoc<L>>> {
        std::iter::once(node.clone())
            .chain(node.ancestors())
            .take_while(|ancestor| {
                ancestor.range() == node.range()
                    || rule.scopes.contains(&ancestor.kind().as_ref())
                    || (!self
                        .grammar
                        .lexical_boundaries
                        .contains(&ancestor.kind().as_ref())
                        && (!self
                            .grammar
                            .lexical_barriers
                            .contains(&ancestor.kind().as_ref())
                            || coverage.modeled.contains(&Span::from(ancestor.range()))))
            })
            .find(|ancestor| rule.scopes.contains(&ancestor.kind().as_ref()))
    }

    pub(crate) fn exclusions<L: LanguageExt>(
        &self,
        scope: &Node<'_, StrDoc<L>>,
        coverage: &mut NavigationCoverage,
    ) -> Vec<Span> {
        coverage
            .exclusions
            .entry(scope.range().into())
            .or_insert_with(|| {
                scope
                    .dfs()
                    .filter(|node| {
                        node.range() != scope.range()
                            && self
                                .grammar
                                .lexical_boundaries
                                .contains(&node.kind().as_ref())
                    })
                    .filter(|node| {
                        !self
                            .grammar
                            .lexical_containers
                            .contains(&scope.kind().as_ref())
                            || node
                                .ancestors()
                                .take_while(|ancestor| ancestor.range() != scope.range())
                                .any(|ancestor| {
                                    self.grammar
                                        .lexical_boundaries
                                        .contains(&ancestor.kind().as_ref())
                                })
                    })
                    .map(|node| node.range().into())
                    .collect()
            })
            .clone()
    }

    pub(crate) fn site<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        scope: &Node<'_, StrDoc<L>>,
        rule: &BindingRule,
        coverage: &mut NavigationCoverage,
    ) -> BindingSite {
        BindingSite {
            declaration: node.range().into(),
            scope: scope.range().into(),
            excluded: self.exclusions(scope, coverage),
            visible_from: if rule.after {
                node.range().end
            } else {
                scope.range().start
            },
            namespace: rule.namespace,
            kind: rule.kind,
        }
    }

    fn macro_scope<L: LanguageExt>(&self, node: &Node<'_, StrDoc<L>>, facts: &mut Facts) {
        let Some(rule) = self.grammar.macro_scopes.filter(|r| r.node == node.kind()) else {
            return;
        };
        let expression = node.parent().is_some_and(|parent| {
            rule.expression_containers.contains(&parent.kind().as_ref())
                || rule.expression_fields.iter().any(|(kind, field)| {
                    parent.kind() == *kind
                        && parent
                            .field(field)
                            .is_some_and(|child| child.range() == node.range())
                })
        });
        if expression {
            return;
        }
        if let Some(scope) = node
            .ancestors()
            .take_while(|a| {
                !self.grammar.lexical_boundaries.contains(&a.kind().as_ref())
                    && a.kind() != rule.node
            })
            .find(|a| rule.scopes.contains(&a.kind().as_ref()))
        {
            facts.scope_uncertainties.push(vvv_core::ScopeUncertainty {
                scope: scope.range().into(),
                invocation: node.range().into(),
            });
        }
    }

    pub(crate) fn barrier<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        facts: &Facts,
        barriers: &[&str],
        coverage: &NavigationCoverage,
    ) -> bool {
        if !barriers.contains(&node.kind().as_ref())
            || coverage.modeled.contains(&Span::from(node.range()))
        {
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
}

#[cfg(all(test, feature = "rust"))]
mod local_import_tests {
    use crate::rust::Rust;
    use vvv_core::Language;

    #[test]
    fn named_block_imports_keep_navigation_ownership_separate_from_mutation() {
        let source = "use crate::Root; fn f() { use crate::a::{run as work, Data}; work(); { use crate::b::run as work; work(); } }";
        let facts = Rust::new().facts(source).unwrap();
        assert_eq!(facts.import_bindings.len(), 1);
        assert_eq!(facts.module_scopes[0].imports.len(), 1);
        assert_eq!(facts.import_scopes.len(), 2);
        assert_eq!(facts.import_scopes[0].imports.len(), 2);
        assert_eq!(facts.import_scopes[1].imports.len(), 1);
        for scope in &facts.import_scopes {
            assert_eq!(scope.aliases.len(), 1);
            for alias in &scope.aliases {
                assert_eq!(&source[alias.start..alias.end], "work");
            }
        }
        let calls: Vec<_> = facts
            .tokens_named("work")
            .filter(|(span, _)| source[span.end..].starts_with("()"))
            .map(|(span, _)| span)
            .collect();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|span| facts.lexical_tokens.contains(span)));
    }

    #[test]
    fn local_globs_and_unsupported_patterns_keep_the_block_conservative() {
        for source in [
            "fn f() { use crate::a::*; work(); }",
            "fn f() { use crate::a::work; let Point { x: pattern!() } = point; work(); }",
        ] {
            let facts = Rust::new().facts(source).unwrap();
            let call = source.rfind("work").unwrap();
            let span = vvv_core::Span::new(call, call + 4);
            assert!(!facts.lexical_tokens.contains(&span));
        }
    }

    #[test]
    fn rust_compiles_local_import_hoisting_shadowing_and_constant_patterns() {
        mod values {
            pub fn answer() -> usize {
                7
            }
            pub const UNIT: () = ();
        }
        #[allow(unused_variables)]
        fn imported(answer: fn() -> usize) -> usize {
            let _ = answer;
            assert_eq!(answer(), 7);
            use values::answer;
            fn nested() -> usize {
                answer()
            }
            let answer = || 9;
            assert_eq!(answer(), 9);
            nested()
        }
        assert_eq!(imported(|| 3), 7);
        use values::UNIT;
        let UNIT = ();
        let _: () = UNIT;
    }
}
