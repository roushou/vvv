use super::{navigation::UnsupportedSyntax, pattern::PatternNavigation};
use crate::syntax::navigation::{BindingSite, NavigationCoverage, NavigationFacts};
use crate::syntax::views::{DirectChildren, syntax_view};
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{BindingNamespace, Span, SymbolKind};

pub(super) struct LetCondition<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl LetCondition {
        kinds: ["let_condition"],
        required: {
            pattern: "pattern",
            value: "value"
        },
        optional: {}
    }
}

pub(super) struct LetChain<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl LetChain { kinds: ["let_chain"], required: {}, optional: {}, children: {operands: []} } }
enum ConditionOperands<'tree, L: LanguageExt> {
    Chain(DirectChildren<'tree, L>),
    Single(Option<Node<'tree, StrDoc<L>>>),
}

impl<'tree, L: LanguageExt> Iterator for ConditionOperands<'tree, L> {
    type Item = Node<'tree, StrDoc<L>>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Chain(nodes) => nodes.next(),
            Self::Single(node) => node.take(),
        }
    }
}

/// Ordered condition operands shared by if and while expressions.
pub(super) struct Condition<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

impl<'tree, L: LanguageExt> Condition<'tree, L> {
    pub fn new(node: Node<'tree, StrDoc<L>>) -> Self {
        Self { node }
    }

    fn operands(&self) -> ConditionOperands<'tree, L> {
        match LetChain::cast(self.node.clone()) {
            Some(chain) => ConditionOperands::Chain(chain.operands()),
            None => ConditionOperands::Single(Some(self.node.clone())),
        }
    }

    pub fn bindings(
        &self,
        owner: &Node<'tree, StrDoc<L>>,
        scope: Span,
        shared: &NavigationFacts<'_>,
        coverage: &mut NavigationCoverage,
    ) -> Result<Vec<(BindingSite, super::pattern::PatternBindings)>, UnsupportedSyntax> {
        if self
            .node
            .dfs()
            .any(|node| node.is_error() || node.is_missing())
        {
            return Err(UnsupportedSyntax::at(&self.node));
        }
        let mut bindings = Vec::new();
        for operand in self.operands() {
            let Some(condition) = LetCondition::cast(operand.clone()) else {
                continue;
            };
            let pattern = condition
                .pattern()
                .map_err(|_| UnsupportedSyntax::at(&operand))?;
            condition
                .value()
                .map_err(|_| UnsupportedSyntax::at(&operand))?;
            let names =
                PatternNavigation::new(pattern, operand.clone(), shared.grammar).bindings()?;
            bindings.push((
                BindingSite {
                    declaration: operand.range().into(),
                    scope,
                    excluded: shared.exclusions(owner, coverage),
                    visible_from: operand.range().end,
                    namespace: BindingNamespace::Value,
                    kind: SymbolKind::Variable,
                },
                names,
            ));
        }
        Ok(bindings)
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn chain_operands_preserve_boolean_steps_and_initializer_boundaries() {
        let tree = Rust.ast_grep(
            "fn f() { if let Some(x) = input && ready(x) && let Some(y) = next(x) { y; } }",
        );
        let chain = tree.root().dfs().find_map(LetChain::cast).unwrap();
        let mut operands = chain.operands();
        let first = LetCondition::cast(operands.next().unwrap()).unwrap();
        assert_eq!(first.pattern().unwrap().text(), "Some(x)");
        assert_eq!(first.value().unwrap().text(), "input");
        assert_eq!(operands.next().unwrap().text(), "ready(x)");
        let last = LetCondition::cast(operands.next().unwrap()).unwrap();
        assert_eq!(last.value().unwrap().text(), "next(x)");
        assert!(operands.next().is_none());
    }
}
