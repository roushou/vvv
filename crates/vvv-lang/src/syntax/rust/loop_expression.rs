use super::{condition::Condition, navigation::UnsupportedSyntax, pattern::PatternNavigation};
use crate::syntax::navigation::{BindingSite, NavigationCoverage, NavigationFacts};
use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{BindingNamespace, Facts, SymbolKind};

pub(super) struct InfiniteLoop<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl InfiniteLoop {
        kinds: ["loop_expression"],
        required: {
            body: "body"
        },
        optional: {}
    }
}

pub(super) struct WhileLoop<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl WhileLoop {
        kinds: ["while_expression"],
        required: {
            condition: "condition",
            body: "body"
        },
        optional: {}
    }
}

pub(super) struct ForLoop<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl ForLoop {
        kinds: ["for_expression"],
        required: {
            pattern: "pattern",
            value: "value",
            body: "body"
        },
        optional: {}
    }
}

pub(super) enum Loop<'tree, L: LanguageExt> {
    Infinite(InfiniteLoop<'tree, L>),
    While(WhileLoop<'tree, L>),
    For(ForLoop<'tree, L>),
}

impl<'tree, L: LanguageExt> Loop<'tree, L> {
    pub fn cast(node: Node<'tree, StrDoc<L>>) -> Option<Self> {
        match node.kind().as_ref() {
            "loop_expression" => InfiniteLoop::cast(node).map(Self::Infinite),
            "while_expression" => WhileLoop::cast(node).map(Self::While),
            "for_expression" => ForLoop::cast(node).map(Self::For),
            _ => None,
        }
    }

    pub fn body(&self) -> Result<Node<'tree, StrDoc<L>>, crate::syntax::views::SyntaxError> {
        match self {
            Self::Infinite(view) => view.body(),
            Self::While(view) => view.body(),
            Self::For(view) => view.body(),
        }
    }
}

pub(super) enum LoopBindings<'tree, L: LanguageExt> {
    Infinite {
        view: InfiniteLoop<'tree, L>,
    },
    While {
        view: WhileLoop<'tree, L>,
        condition: Node<'tree, StrDoc<L>>,
    },
    For {
        view: ForLoop<'tree, L>,
        pattern: Node<'tree, StrDoc<L>>,
        value: Node<'tree, StrDoc<L>>,
    },
}

impl<'tree, L: LanguageExt> LoopBindings<'tree, L> {
    pub fn from_node(node: Node<'tree, StrDoc<L>>) -> Result<Self, UnsupportedSyntax> {
        UnsupportedSyntax::validate(&node)?;
        let expression = Loop::cast(node.clone()).ok_or_else(|| UnsupportedSyntax::at(&node))?;
        let body = expression
            .body()
            .map_err(|_| UnsupportedSyntax::at(&node))?;
        if super::block::Block::cast(body.clone()).is_none() {
            return Err(UnsupportedSyntax::at(&node));
        }
        UnsupportedSyntax::validate(&body)?;
        Ok(match expression {
            Loop::Infinite(view) => Self::Infinite { view },
            Loop::While(view) => {
                let condition = view.condition().map_err(|_| UnsupportedSyntax::at(&node))?;
                Self::While { view, condition }
            }
            Loop::For(view) => {
                let pattern = view.pattern().map_err(|_| UnsupportedSyntax::at(&node))?;
                let value = view.value().map_err(|_| UnsupportedSyntax::at(&node))?;
                Self::For {
                    view,
                    pattern,
                    value,
                }
            }
        })
    }

    pub fn extract(
        &self,
        shared: &NavigationFacts<'_>,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) {
        let node = match self {
            Self::Infinite { view } => view.syntax(),
            Self::While { view, .. } => view.syntax(),
            Self::For { view, .. } => view.syntax(),
        };
        let bindings = match self {
            Self::Infinite { .. } => Ok(Vec::new()),
            Self::While { condition, .. } => Condition::new(condition.clone()).bindings(
                node,
                node.range().into(),
                shared,
                coverage,
            ),
            Self::For { pattern, value, .. } => {
                if value.dfs().any(|node| node.is_error() || node.is_missing()) {
                    Err(UnsupportedSyntax::at(value))
                } else {
                    PatternNavigation::new(pattern.clone(), node.clone(), shared.grammar)
                        .bindings()
                        .map(|names| {
                            vec![(
                                BindingSite {
                                    declaration: pattern.range().into(),
                                    scope: node.range().into(),
                                    excluded: shared.exclusions(node, coverage),
                                    visible_from: value.range().end,
                                    namespace: BindingNamespace::Value,
                                    kind: SymbolKind::Variable,
                                },
                                names,
                            )]
                        })
                }
            }
        };
        match bindings {
            Ok(bindings) => {
                for (site, names) in bindings {
                    names.emit(site, facts);
                }
                coverage.model(node.range().into());
            }
            Err(_) => coverage.block(node.range().into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rust::Rust;
    use vvv_core::{BindingNamespace, Language, Span, SymbolKind};

    #[test]
    fn for_binding_starts_after_iterator_and_stays_in_its_loop() {
        let source = "fn f(value: usize) { for value in values(value) { value; fn nested() { value; } } value; }";
        let facts = Rust::new().facts(source).unwrap();
        let binding = facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.kind == SymbolKind::Variable)
            .unwrap();
        for (needle, expected) in [("value)", false), ("value; fn", true), ("value; }", false)] {
            let start = if expected {
                source.find(needle)
            } else {
                source.rfind(needle)
            }
            .unwrap();
            assert_eq!(
                binding.visible(
                    "value",
                    Span::new(start, start + 5),
                    BindingNamespace::Value
                ),
                expected
            );
        }
        let nested = source.find("fn nested").unwrap();
        assert!(binding.excluded.iter().any(|span| span.start == nested));
    }

    #[test]
    fn while_chain_preserves_order_and_unsupported_patterns_publish_nothing() {
        let source =
            "fn f() { while let Some(value) = input && let Some(value) = next(value) { value; } }";
        let facts = Rust::new().facts(source).unwrap();
        let bindings: Vec<_> = facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.name == "value")
            .collect();
        assert_eq!(bindings.len(), 2);
        let initializer = source.find("next(value)").unwrap() + 5;
        assert!(bindings[0].visible(
            "value",
            Span::new(initializer, initializer + 5),
            BindingNamespace::Value
        ));
        assert!(!bindings[1].visible(
            "value",
            Span::new(initializer, initializer + 5),
            BindingNamespace::Value
        ));
        let source = "fn f(value: usize) { for (prefix, Point { field: pattern!() }) in input { value; } value; }";
        let facts = Rust::new().facts(source).unwrap();
        assert!(
            !facts
                .lexical
                .iter()
                .any(|binding| binding.symbol.name == "prefix")
        );
        let inside = source.find("value;").unwrap();
        let after = source.rfind("value;").unwrap();
        assert!(
            !facts
                .lexical_tokens
                .contains(&Span::new(inside, inside + 5))
        );
        assert!(facts.lexical_tokens.contains(&Span::new(after, after + 5)));
    }

    #[test]
    fn rust_loop_visibility_matches_the_fact_contract() {
        let value = 10;
        let mut sum = 0;
        for value in [1, 2] {
            sum += value;
        }
        let mut input = Some(3);
        while let Some(value) = input.take() {
            sum += value;
        }
        assert_eq!(sum, 6);
        assert_eq!(value, 10);
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn loop_forms_retain_their_distinct_headers_and_bodies() {
        let tree = Rust.ast_grep(
            "fn f() { loop { break; } while ready { work(); } for (x, y) in input { x; } }",
        );
        let root = tree.root();
        let mut forms = root.dfs().filter_map(Loop::cast);
        let infinite = forms.next().unwrap();
        assert!(matches!(infinite, Loop::Infinite(_)));
        assert_eq!(infinite.body().unwrap().kind(), "block");
        let Loop::While(condition) = forms.next().unwrap() else {
            panic!("expected while");
        };
        assert_eq!(condition.condition().unwrap().text(), "ready");
        let Loop::For(iterator) = forms.next().unwrap() else {
            panic!("expected for");
        };
        assert_eq!(iterator.pattern().unwrap().text(), "(x, y)");
        assert_eq!(iterator.value().unwrap().text(), "input");
        assert_eq!(iterator.body().unwrap().kind(), "block");
        assert!(forms.next().is_none());
    }
}
