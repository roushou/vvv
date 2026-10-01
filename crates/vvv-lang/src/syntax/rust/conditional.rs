use super::{condition::Condition, navigation::UnsupportedSyntax};
use crate::syntax::navigation::{BindingSite, NavigationCoverage, NavigationFacts};
use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{Facts, Span};

pub(super) struct If<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl If {
        kinds: ["if_expression"],
        required: {
            condition: "condition",
            consequence: "consequence"
        },
        optional: {
            alternative: "alternative"
        }
    }
}

pub(super) struct Else<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Else {
        kinds: ["else_clause"],
        required: {},
        optional: {}
    }
}

pub(super) enum ElseBranch<'tree, L: LanguageExt> {
    Block(super::block::Block<'tree, L>),
    If(If<'tree, L>),
}

impl<'tree, L: LanguageExt> Else<'tree, L> {
    pub fn branch(&self) -> Option<ElseBranch<'tree, L>> {
        self.syntax()
            .children()
            .find_map(|child| match child.kind().as_ref() {
                "block" => super::block::Block::cast(child).map(ElseBranch::Block),
                "if_expression" => If::cast(child).map(ElseBranch::If),
                _ => None,
            })
    }
}

impl<'tree, L: LanguageExt> ElseBranch<'tree, L> {
    pub fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        match self {
            Self::Block(view) => view.syntax(),
            Self::If(view) => view.syntax(),
        }
    }
}

pub(super) struct Conditional<'tree, L: LanguageExt> {
    view: If<'tree, L>,
    condition: Node<'tree, StrDoc<L>>,
    consequence: Node<'tree, StrDoc<L>>,
    alternative: Option<Node<'tree, StrDoc<L>>>,
}

impl<'tree, L: LanguageExt> Conditional<'tree, L> {
    pub fn from_node(node: Node<'tree, StrDoc<L>>) -> Result<Self, UnsupportedSyntax> {
        UnsupportedSyntax::validate(&node)?;
        let view = If::cast(node.clone()).ok_or_else(|| UnsupportedSyntax::at(&node))?;
        let condition = view.condition().map_err(|_| UnsupportedSyntax::at(&node))?;
        let consequence = Some(
            view.consequence()
                .map_err(|_| UnsupportedSyntax::at(&node))?,
        )
        .filter(|body| super::block::Block::cast(body.clone()).is_some())
        .ok_or_else(|| UnsupportedSyntax::at(&node))?;
        Ok(Self {
            condition,
            consequence,
            alternative: view
                .alternative()
                .map_err(|_| UnsupportedSyntax::at(&node))?,
            view,
        })
    }

    fn binding_scope(&self) -> Span {
        Span::new(
            self.view.syntax().range().start,
            self.consequence.range().end,
        )
    }

    fn bindings(
        &self,
        shared: &NavigationFacts<'_>,
        coverage: &mut NavigationCoverage,
    ) -> Result<Vec<(BindingSite, super::pattern::PatternBindings)>, UnsupportedSyntax> {
        UnsupportedSyntax::validate(&self.consequence)?;
        if let Some(alternative) = &self.alternative {
            UnsupportedSyntax::validate(alternative)?;
            let branch = Else::cast(alternative.clone())
                .and_then(|view| view.branch())
                .ok_or_else(|| UnsupportedSyntax::at(alternative))?;
            // Inspect only the branch shape; deeper coverage belongs to that construct.
            debug_assert!(
                vvv_core::Span::from(alternative.range()).contains(&branch.syntax().range().into())
            );
        }
        Condition::new(self.condition.clone()).bindings(
            self.view.syntax(),
            self.binding_scope(),
            shared,
            coverage,
        )
    }

    pub fn extract(
        &self,
        shared: &NavigationFacts<'_>,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) {
        match self.bindings(shared, coverage) {
            Ok(bindings) => {
                for (site, names) in bindings {
                    names.emit(site, facts);
                }
                coverage.model(self.view.syntax().range().into());
            }
            Err(_) => coverage.block(self.view.syntax().range().into()),
        }
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn branches_retain_else_if_and_block_shapes() {
        let tree = Rust.ast_grep(
            "fn f() { if let Some(x) = input { x; } else if ready { work(); } else { stop(); } }",
        );
        let view = tree.root().dfs().find_map(If::cast).unwrap();
        assert_eq!(view.condition().unwrap().kind(), "let_condition");
        assert_eq!(view.consequence().unwrap().kind(), "block");
        let alternative = Else::cast(view.alternative().unwrap().unwrap()).unwrap();
        let ElseBranch::If(nested) = alternative.branch().unwrap() else {
            panic!("expected else if");
        };
        assert_eq!(nested.condition().unwrap().text(), "ready");
        let alternative = Else::cast(nested.alternative().unwrap().unwrap()).unwrap();
        assert!(matches!(alternative.branch(), Some(ElseBranch::Block(_))));
        let unfinished = Rust.ast_grep("fn f() { if ready { @ } }");
        assert!(unfinished.root().dfs().find_map(If::cast).is_some());
    }
}
