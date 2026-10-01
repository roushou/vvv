use super::{
    match_arm::{ArmNavigation, MatchArm},
    navigation::UnsupportedSyntax,
};
use crate::syntax::navigation::{NavigationCoverage, NavigationFacts};
use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::Facts;

pub(super) struct Match<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Match {
        kinds: ["match_expression"],
        required: {
            value: "value",
            body: "body"
        },
        optional: {}
    }
}

pub(super) struct MatchBody<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl MatchBody { kinds: ["match_block"], required: {}, optional: {}, children: {items: []} } }
impl<'tree, L: LanguageExt> MatchBody<'tree, L> {
    pub fn arms(&self) -> impl Iterator<Item = MatchArm<'tree, L>> {
        self.items().filter_map(MatchArm::cast)
    }
}

pub(super) struct MatchNavigation<'tree, L: LanguageExt> {
    view: Match<'tree, L>,
    body: MatchBody<'tree, L>,
}

impl<'tree, L: LanguageExt> MatchNavigation<'tree, L> {
    pub fn from_node(node: Node<'tree, StrDoc<L>>) -> Result<Self, UnsupportedSyntax> {
        UnsupportedSyntax::validate(&node)?;
        let view = Match::cast(node.clone()).ok_or_else(|| UnsupportedSyntax::at(&node))?;
        let value = view.value().map_err(|_| UnsupportedSyntax::at(&node))?;
        if value
            .dfs()
            .any(|child| child.is_error() || child.is_missing())
        {
            return Err(UnsupportedSyntax::at(&value));
        }
        let body = view.body().map_err(|_| UnsupportedSyntax::at(&node))?;
        let body = MatchBody::cast(body).ok_or_else(|| UnsupportedSyntax::at(&node))?;
        UnsupportedSyntax::validate(body.syntax())?;
        Ok(Self { view, body })
    }

    pub fn extract(
        &self,
        shared: &NavigationFacts<'_>,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) {
        coverage.model(self.view.syntax().range().into());
        for arm in self.body.arms() {
            let span = arm.syntax().range().into();
            match ArmNavigation::new(arm) {
                Ok(arm) => arm.extract(shared, facts, coverage),
                Err(_) => coverage.block(span),
            }
        }
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn match_body_iterates_direct_arms_without_flattening_nested_matches() {
        let tree = Rust.ast_grep(
            "fn f() { match input { // comment\n Some(x) => match x { _ => 1 }, _ => 0 } }",
        );
        let view = tree.root().dfs().find_map(Match::cast).unwrap();
        assert_eq!(view.value().unwrap().text(), "input");
        let body = MatchBody::cast(view.body().unwrap()).unwrap();
        let arms: Vec<_> = body.arms().collect();
        assert_eq!(arms.len(), 2);
        assert!(arms[0].syntax().text().contains("match x"));
    }
}
