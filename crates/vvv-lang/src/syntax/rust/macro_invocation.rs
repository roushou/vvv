use crate::syntax::navigation::NavigationFacts;
use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{Facts, ScopeUncertainty};

pub(super) struct MacroInvocation<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl MacroInvocation {
        kinds: ["macro_invocation"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> MacroInvocation<'tree, L> {
    pub fn parent(&self) -> Option<Node<'tree, StrDoc<L>>> {
        self.syntax().parent()
    }

    pub fn occupies_field(&self, parent: &Node<'tree, StrDoc<L>>, field: &str) -> bool {
        parent
            .field(field)
            .is_some_and(|child| child.range() == self.syntax().range())
    }

    pub fn extract(&self, shared: &NavigationFacts<'_>, facts: &mut Facts) {
        let Some(rule) = shared.grammar.macro_scopes else {
            return;
        };
        let expression = self.parent().is_some_and(|parent| {
            rule.expression_containers.contains(&parent.kind().as_ref())
                || rule.expression_fields.iter().any(|(kind, field)| {
                    parent.kind() == *kind && self.occupies_field(&parent, field)
                })
        });
        if expression {
            return;
        }
        if let Some(scope) = self
            .syntax()
            .ancestors()
            .take_while(|ancestor| {
                !shared
                    .grammar
                    .lexical_boundaries
                    .contains(&ancestor.kind().as_ref())
                    && ancestor.kind() != rule.node
            })
            .find(|ancestor| rule.scopes.contains(&ancestor.kind().as_ref()))
        {
            facts.scope_uncertainties.push(ScopeUncertainty {
                scope: scope.range().into(),
                invocation: self.syntax().range().into(),
            });
        }
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn placement_distinguishes_initializer_fields_from_statement_invocations() {
        let tree = Rust.ast_grep("fn f() { let value = compute!(); generate!(); }");
        let root = tree.root();
        let mut calls = root.dfs().filter_map(MacroInvocation::cast);
        let initializer = calls.next().unwrap();
        let parent = initializer.parent().unwrap();
        assert_eq!(parent.kind(), "let_declaration");
        assert!(initializer.occupies_field(&parent, "value"));
        assert!(!initializer.occupies_field(&parent, "pattern"));
        assert_eq!(
            calls.next().unwrap().parent().unwrap().kind(),
            "expression_statement"
        );
    }
}
