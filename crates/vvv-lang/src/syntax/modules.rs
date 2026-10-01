//! Explicit file and inline-module ownership; no parser nodes cross this boundary.

use super::imports::ImportExtractor;
use crate::syntax::NavigationSyntax;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{Facts, Grammar, ModuleDeclaration, ModuleScope, Span};

pub(super) struct Modules<'a, L: LanguageExt> {
    grammar: &'a Grammar,
    imports: ImportExtractor<'a, L>,
    syntax: NavigationSyntax,
}

impl<'a, L: LanguageExt> Modules<'a, L> {
    pub(super) fn new(grammar: &'a Grammar, language: L, syntax: NavigationSyntax) -> Self {
        Self {
            grammar,
            imports: ImportExtractor::new(&grammar.imports, language, syntax),
            syntax,
        }
    }

    pub(super) fn extract(&self, root: &Node<'_, StrDoc<L>>, facts: &mut Facts) {
        facts.import_scopes = root
            .dfs()
            .filter(|node| self.grammar.import_scopes.contains(&node.kind().as_ref()))
            .filter_map(|body| {
                let imports: Vec<_> = self
                    .imports
                    .bindings_in(&body, &facts.imports)
                    .into_iter()
                    .filter(|binding| {
                        let Some(prefix) = facts
                            .imports
                            .iter()
                            .find(|import| import.span == binding.span)
                        else {
                            return false;
                        };
                        !facts.imports.iter().any(|entry| {
                            entry.group.as_ref().is_some_and(|group| {
                                group.prefix == prefix.path
                                    && group.statement.contains(&prefix.span)
                                    && prefix.span.end <= group.list.start
                            })
                        })
                    })
                    .collect();
                // Globs need namespace and precedence evidence beyond named bindings.
                (!imports.is_empty()
                    && imports.iter().all(|binding| {
                        facts
                            .imports
                            .iter()
                            .any(|import| import.span == binding.span && !import.glob)
                    }))
                .then(|| vvv_core::ImportScope {
                    span: body.range().into(),
                    aliases: body
                        .dfs()
                        .filter_map(|node| {
                            let rule = self.grammar.imports.alias?;
                            if node.kind() != rule.under
                                || !imports
                                    .iter()
                                    .any(|binding| Span::from(node.range()).contains(&binding.span))
                            {
                                return None;
                            }
                            self.imports.alias(&node).map(|alias| alias.range().into())
                        })
                        .collect(),
                    imports,
                })
            })
            .collect();
        let Some(rule) = self.grammar.module_scopes else {
            return;
        };
        let mut owners = vec![(root.clone(), None, vec![])];
        for node in root.dfs().filter(|node| node.kind() == rule.node) {
            #[cfg(feature = "rust")]
            if matches!(self.syntax, NavigationSyntax::Rust) {
                if let Some(module) = super::rust::Module::cast(node)
                    && let Some(owner) =
                        super::rust::ModuleOwner::from_module(&module, self.grammar)
                {
                    owners.push((owner.body, Some(owner.declaration), owner.path));
                }
                continue;
            }
            #[cfg(not(feature = "rust"))]
            let _ = self.syntax;

            // Modules in functions or other item scopes are not file module owners.
            if node.ancestors().any(|parent| {
                parent.kind() != rule.node
                    && (self
                        .grammar
                        .lexical_boundaries
                        .contains(&parent.kind().as_ref())
                        || self
                            .grammar
                            .lexical_barriers
                            .contains(&parent.kind().as_ref()))
            }) {
                continue;
            }
            let (Some(body), Some(name)) = (node.field(rule.body), node.field(rule.name)) else {
                continue;
            };
            let mut path: Vec<_> = node
                .ancestors()
                .filter(|parent| parent.kind() == rule.node)
                .filter_map(|parent| parent.field(rule.name))
                .map(|name| name.text().into_owned().into())
                .collect();
            path.reverse();
            path.push(name.text().into_owned().into());
            owners.push((body, Some(Span::from(name.range())), path));
        }
        facts.module_scopes = owners
            .into_iter()
            .map(|(body, declaration, path)| {
                let declarations = self.declarations(&body, facts);
                ModuleScope {
                    path,
                    span: body.range().into(),
                    declaration,
                    declarations,
                    imports: self.imports.bindings_in(&body, &facts.imports),
                }
            })
            .collect();
    }

    fn declarations(&self, body: &Node<'_, StrDoc<L>>, facts: &Facts) -> Vec<ModuleDeclaration> {
        #[cfg(feature = "rust")]
        if matches!(self.syntax, NavigationSyntax::Rust)
            && let Some(body) = super::rust::ModuleBody::cast(body.clone())
        {
            return body
                .items()
                .filter_map(|node| self.declaration(&node, facts))
                .collect();
        }
        body.children()
            .filter_map(|node| self.declaration(&node, facts))
            .collect()
    }

    fn declaration(&self, node: &Node<'_, StrDoc<L>>, facts: &Facts) -> Option<ModuleDeclaration> {
        let symbol = facts
            .symbols
            .iter()
            .find(|symbol| symbol.span == Span::from(node.range()))?;
        let restriction = self
            .imports
            .modifier(node)
            .and_then(|modifier| self.imports.restriction(&modifier))
            .map(|path| self.grammar.imports.syntax.parse(path.text().as_ref()));
        Some(ModuleDeclaration {
            name_span: symbol.name_span,
            restriction,
        })
    }
}

#[cfg(all(test, feature = "rust"))]
mod import_binding_tests {
    use crate::rust::Rust;
    use vvv_core::Language;

    #[test]
    fn module_import_bindings_retain_visibility_and_exclude_inner_scopes() {
        let source = "use crate::a as private;\npub(crate) use crate::a as package;\npub(super) use crate::a as parent;\npub(in crate::restricted) use crate::a as limited;\nfn f() { use crate::b as local; }\nmod inner { use crate::b as nested; }\n";
        let facts = Rust::new().facts(source).unwrap();
        let bindings: Vec<_> = facts
            .import_bindings
            .iter()
            .map(|binding| {
                let import = facts
                    .imports
                    .iter()
                    .find(|i| i.span == binding.span)
                    .unwrap();
                (
                    import.alias.as_ref().unwrap().as_str(),
                    binding.visibility.as_ref().map(|v| v.text.as_str()),
                    binding.restriction.as_ref().map(ToString::to_string),
                )
            })
            .collect();
        assert_eq!(
            bindings,
            vec![
                ("private", None, None),
                ("package", Some("pub(crate)"), Some("crate".into())),
                ("parent", Some("pub(super)"), Some("super".into())),
                (
                    "limited",
                    Some("pub(in crate::restricted)"),
                    Some("crate::restricted".into())
                ),
            ]
        );
    }
}

#[cfg(all(test, feature = "rust"))]
mod module_scope_tests {
    use crate::rust::Rust;
    use vvv_core::{Language, SymbolKind};

    #[test]
    fn module_scopes_own_only_direct_items_and_imports() {
        let source = "use crate::Root; mod tests { use super::Root as Local; struct Owned; mod nested { pub(super) use super::Local as Alias; fn call(_: Alias) {} } fn outer() { struct Hidden; mod invalid { struct Unowned; } } }";
        let facts = Rust::new().facts(source).unwrap();
        let paths: Vec<Vec<&str>> = facts
            .module_scopes
            .iter()
            .map(|scope| scope.path.iter().map(|name| name.as_str()).collect())
            .collect();
        assert_eq!(paths, [vec![], vec!["tests"], vec!["tests", "nested"]]);
        let tests = &facts.module_scopes[1];
        let names: Vec<_> = tests
            .declarations
            .iter()
            .map(|decl| {
                facts
                    .symbols
                    .iter()
                    .find(|symbol| symbol.name_span == decl.name_span)
                    .unwrap()
                    .name
                    .as_str()
            })
            .collect();
        assert_eq!(names, ["Owned", "nested", "outer"]);
        assert_eq!(tests.imports.len(), 1);
        assert_eq!(
            facts.module_scopes[2].imports[0]
                .visibility
                .as_ref()
                .unwrap()
                .text,
            "pub(super)"
        );
        assert_eq!(
            facts.import_bindings.len(),
            1,
            "mutation binding facts stay file-root only"
        );
        assert!(
            facts
                .symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Struct && symbol.name == "Hidden")
        );
    }

    #[test]
    fn supported_inline_modules_allow_navigation_without_capturing_outer_locals() {
        let source = "mod tests { fn outer(x: u8) { let y = x; fn inner() { y; } x; } fn local() { use crate::Root as Alias; let _: Alias; } }";
        let facts = Rust::new().facts(source).unwrap();
        let x_use = source.rfind("x;").unwrap();
        assert!(facts.lexical_tokens.iter().any(|span| span.start == x_use));
        let y_use = source.find("y;").unwrap();
        assert!(!facts.lexical.iter().any(|binding| binding.visible(
            "y",
            vvv_core::Span::new(y_use, y_use + 1),
            vvv_core::BindingNamespace::Value
        )));
        let alias_use = source.rfind("Alias").unwrap();
        assert!(facts.navigation.iter().any(|span| span.start == alias_use));
    }
}
