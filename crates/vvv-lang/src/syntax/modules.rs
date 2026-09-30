//! Explicit file and inline-module ownership; no parser nodes cross this boundary.
use super::imports::ImportExtractor;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{Facts, Grammar, ModuleDeclaration, ModuleScope, ReExportRule, Span};

pub(super) struct Modules<'a, L: LanguageExt> {
    grammar: &'a Grammar,
    imports: ImportExtractor<'a, L>,
}
impl<'a, L: LanguageExt> Modules<'a, L> {
    pub(super) fn new(grammar: &'a Grammar, language: L) -> Self {
        Self {
            grammar,
            imports: ImportExtractor::new(&grammar.imports, language),
        }
    }
    pub(super) fn extract(&self, root: &Node<'_, StrDoc<L>>, facts: &mut Facts) {
        let Some(rule) = self.grammar.module_scopes else {
            return;
        };
        let mut owners = vec![(root.clone(), None, vec![])];
        for node in root.dfs().filter(|node| node.kind() == rule.node) {
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
                let declarations = body
                    .children()
                    .filter_map(|node| {
                        let symbol = facts
                            .symbols
                            .iter()
                            .find(|symbol| symbol.span == Span::from(node.range()))?;
                        let restriction = match self.grammar.imports.reexports {
                            ReExportRule::Modifier(kind) => node
                                .children()
                                .find(|child| child.kind() == kind)
                                .and_then(|modifier| modifier.children().find(Node::is_named))
                                .map(|path| {
                                    self.grammar.imports.syntax.parse(path.text().as_ref())
                                }),
                            _ => None,
                        };
                        Some(ModuleDeclaration {
                            name_span: symbol.name_span,
                            restriction,
                        })
                    })
                    .collect();
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
}
