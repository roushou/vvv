use super::{condition::Condition, navigation::UnsupportedSyntax, pattern::PatternNavigation};
use crate::syntax::navigation::{BindingSite, NavigationCoverage, NavigationFacts};
use crate::syntax::views::{FieldIssue, SyntaxError, syntax_view};
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{BindingNamespace, Facts, SymbolKind};

pub(super) struct MatchArm<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl MatchArm {
        kinds: ["match_arm"],
        required: {
            pattern: "pattern",
            value: "value"
        },
        optional: {}
    }
}

pub(super) struct MatchPattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl MatchPattern {
        kinds: ["match_pattern"],
        required: {},
        optional: {
            guard: "condition"
        }
    }
}

impl<'tree, L: LanguageExt> MatchPattern<'tree, L> {
    pub fn pattern(&self) -> Result<Node<'tree, StrDoc<L>>, SyntaxError> {
        let guard = self.guard()?;
        self.syntax()
            .children()
            .find(|child| {
                child.kind() != "if"
                    && guard
                        .as_ref()
                        .is_none_or(|guard| child.range() != guard.range())
                    && (child.is_named() || child.kind() == "_")
            })
            .ok_or_else(|| SyntaxError {
                span: self.syntax().range().into(),
                field: "pattern",
                issue: FieldIssue::Absent,
            })
    }
}

pub(super) struct ArmNavigation<'tree, L: LanguageExt> {
    view: MatchArm<'tree, L>,
    pattern: Node<'tree, StrDoc<L>>,
    guard: Option<Node<'tree, StrDoc<L>>>,
}

impl<'tree, L: LanguageExt> ArmNavigation<'tree, L> {
    pub fn new(view: MatchArm<'tree, L>) -> Result<Self, UnsupportedSyntax> {
        let node = view.syntax();
        UnsupportedSyntax::validate(node)?;
        let wrapper = view.pattern().map_err(|_| UnsupportedSyntax::at(node))?;
        UnsupportedSyntax::validate(&wrapper)?;
        let wrapper =
            MatchPattern::cast(wrapper.clone()).ok_or_else(|| UnsupportedSyntax::at(&wrapper))?;
        let guard = wrapper
            .guard()
            .map_err(|_| UnsupportedSyntax::at(wrapper.syntax()))?;
        let pattern = wrapper
            .pattern()
            .map_err(|_| UnsupportedSyntax::at(wrapper.syntax()))?;
        let value = view.value().map_err(|_| UnsupportedSyntax::at(node))?;
        UnsupportedSyntax::validate(&value)?;
        Ok(Self {
            view,
            pattern,
            guard,
        })
    }

    pub fn extract(
        &self,
        shared: &NavigationFacts<'_>,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) {
        let names = PatternNavigation::new(
            self.pattern.clone(),
            self.view.syntax().clone(),
            shared.grammar,
        )
        .bindings();
        // Do not publish the pattern prefix if the guard has unsupported binding syntax.
        let guard = self
            .guard
            .as_ref()
            .map(|guard| {
                Condition::new(guard.clone()).bindings(
                    self.view.syntax(),
                    self.view.syntax().range().into(),
                    shared,
                    coverage,
                )
            })
            .transpose();
        match (names, guard) {
            (Ok(names), Ok(guard)) => {
                names.emit(
                    BindingSite {
                        declaration: self.pattern.range().into(),
                        scope: self.view.syntax().range().into(),
                        excluded: shared.exclusions(self.view.syntax(), coverage),
                        visible_from: self.pattern.range().end,
                        namespace: BindingNamespace::Value,
                        kind: SymbolKind::Variable,
                    },
                    facts,
                );
                for (site, names) in guard.into_iter().flatten() {
                    names.emit(site, facts);
                }
                coverage.model(self.view.syntax().range().into());
            }
            _ => coverage.block(self.view.syntax().range().into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rust::Rust;
    use vvv_core::{BindingNamespace, Language, Span};

    #[test]
    fn arm_bindings_reach_guard_and_body_but_not_siblings_or_nested_items() {
        let source = "fn f(value: usize) { match input { Some(value) if value > 0 => { let capture = || value; fn inner() { value; } }, _ => value } value; }";
        let facts = Rust::default().facts(source).unwrap();
        let binding = facts
            .lexical
            .iter()
            .find(|binding| {
                binding.symbol.kind == vvv_core::SymbolKind::Variable
                    && binding.symbol.name == "value"
            })
            .unwrap();
        for needle in ["value >", "value; fn"] {
            let start = source.find(needle).unwrap();
            assert!(binding.visible(
                "value",
                Span::new(start, start + 5),
                BindingNamespace::Value
            ));
        }
        for needle in ["value; }", "value }", "value; }"] {
            let start = source.rfind(needle).unwrap();
            assert!(!binding.visible(
                "value",
                Span::new(start, start + 5),
                BindingNamespace::Value
            ));
        }
        let nested = source.find("fn inner").unwrap();
        assert!(binding.excluded.iter().any(|span| span.start == nested));
    }

    #[test]
    fn unsupported_arm_does_not_block_scrutinee_sibling_or_surrounding_scope() {
        let source = "fn f(value: usize) { match value { Some(Point { field: pattern!() }) => value, _ => value } value; }";
        let facts = Rust::default().facts(source).unwrap();
        let spans: Vec<_> = facts.tokens_named("value").map(|(span, _)| span).collect();
        assert!(facts.lexical_tokens.contains(&spans[1]));
        assert!(!facts.lexical_tokens.contains(&spans[2]));
        assert!(facts.lexical_tokens.contains(&spans[3]));
        assert!(facts.lexical_tokens.contains(&spans[4]));
        assert!(
            !facts
                .lexical
                .iter()
                .any(|binding| binding.symbol.name == "field")
        );
    }

    #[test]
    fn rust_arm_visibility_matches_the_fact_contract() {
        let value = 9;
        let result = match Some(2) {
            Some(value) if value > 0 => {
                let capture = || value;
                capture()
            }
            _ => value,
        };
        assert_eq!(result, 2);
        assert_eq!(value, 9);
    }

    #[test]
    fn guard_chain_binding_starts_after_its_initializer() {
        let source = "fn f() { match input { Some(value) if let Some(next) = derive(value) && next > 0 => next, _ => next } }";
        let facts = Rust::default().facts(source).unwrap();
        let value = facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "value")
            .unwrap();
        let next = facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "next")
            .unwrap();
        let initializer = source.find("derive(value)").unwrap() + 7;
        assert!(value.visible(
            "value",
            Span::new(initializer, initializer + 5),
            BindingNamespace::Value
        ));
        assert_eq!(next.visible_from, source.find(" && next").unwrap());
        let use_start = source.find("next >").unwrap();
        assert!(next.visible(
            "next",
            Span::new(use_start, use_start + 4),
            BindingNamespace::Value
        ));
        let peer = source.rfind("next").unwrap();
        assert!(!next.visible("next", Span::new(peer, peer + 4), BindingNamespace::Value));
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn arm_pattern_separates_guards_and_keeps_unnamed_wildcards() {
        let tree = Rust.ast_grep("fn f() { match input { Some(x) if ready(x) => x, _ => 0 } }");
        let root = tree.root();
        let mut arms = root.dfs().filter_map(MatchArm::cast);
        let first = arms.next().unwrap();
        let pattern = MatchPattern::cast(first.pattern().unwrap()).unwrap();
        assert_eq!(pattern.pattern().unwrap().text(), "Some(x)");
        assert_eq!(pattern.guard().unwrap().unwrap().text(), "ready(x)");
        assert_eq!(first.value().unwrap().text(), "x");
        let pattern = MatchPattern::cast(arms.next().unwrap().pattern().unwrap()).unwrap();
        assert_eq!(pattern.pattern().unwrap().text(), "_");
        assert!(pattern.guard().unwrap().is_none());
    }
}
