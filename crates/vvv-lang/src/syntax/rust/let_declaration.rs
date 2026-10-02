use super::{block::Block, navigation::UnsupportedSyntax, pattern::PatternNavigation};
use crate::syntax::navigation::{NavigationCoverage, NavigationFacts};
use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{BindingRule, Facts};

pub(super) struct LetDeclaration<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl LetDeclaration {
        kinds: ["let_declaration"],
        required: {
            pattern: "pattern"
        },
        optional: {
            value: "value",
            alternative: "alternative"
        }
    }
}

pub(super) struct LetBinding<'tree, L: LanguageExt> {
    view: LetDeclaration<'tree, L>,
    pattern: Node<'tree, StrDoc<L>>,
}

impl<'tree, L: LanguageExt> LetBinding<'tree, L> {
    pub fn from_node(node: Node<'tree, StrDoc<L>>) -> Result<Self, UnsupportedSyntax> {
        UnsupportedSyntax::validate(&node)?;
        let view =
            LetDeclaration::cast(node.clone()).ok_or_else(|| UnsupportedSyntax::at(&node))?;
        let pattern = view.pattern().map_err(|_| UnsupportedSyntax::at(&node))?;
        if let Some(alternative) = view
            .alternative()
            .map_err(|_| UnsupportedSyntax::at(&node))?
        {
            if Block::cast(alternative.clone()).is_none()
                || view
                    .value()
                    .map_err(|_| UnsupportedSyntax::at(&node))?
                    .is_none()
            {
                return Err(UnsupportedSyntax::at(&node));
            }
            UnsupportedSyntax::validate(&alternative)?;
        }
        Ok(Self { pattern, view })
    }

    pub fn extract(
        &self,
        owner: &Block<'tree, L>,
        shared: &NavigationFacts<'_>,
        rule: &BindingRule,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) {
        match PatternNavigation::new(
            self.pattern.clone(),
            self.view.syntax().clone(),
            shared.grammar,
        )
        .bindings()
        {
            Ok(names) => names.emit(
                shared.site(self.view.syntax(), owner.syntax(), rule, coverage),
                facts,
            ),
            Err(_) => coverage.block(owner.span()),
        }
    }
}

#[cfg(test)]
mod let_else_tests {
    use crate::rust::Rust;
    use vvv_core::Language;

    #[test]
    fn tuple_constructors_are_not_bindings_and_let_else_visibility_starts_after_else() {
        let source = "fn f(value: Option<usize>) { let Some(value) = value else { let _ = value; return; }; let _ = value; }";
        let facts = Rust::default().facts(source).unwrap();
        let local = facts
            .lexical
            .iter()
            .find(|binding| {
                binding.symbol.kind == vvv_core::SymbolKind::Variable
                    && binding.symbol.name == "value"
            })
            .unwrap();
        assert_eq!(
            local.visible_from,
            source.find("; let _ = value;").unwrap() + 1
        );
        assert!(
            !facts
                .lexical
                .iter()
                .any(|binding| binding.symbol.name == "Some")
        );
        for (span, _) in facts.tokens_named("value") {
            assert!(facts.lexical_tokens.contains(&span), "{span:?}");
        }
        let nested = "fn f() { let crate::Wrap(Some((left, mut right))) = input else { return; }; left; right; }";
        let facts = Rust::default().facts(nested).unwrap();
        let variables: Vec<_> = facts
            .lexical
            .iter()
            .filter(|binding| binding.symbol.kind == vvv_core::SymbolKind::Variable)
            .collect();
        assert_eq!(variables.len(), 2);
        assert_eq!(variables[0].symbol.name, "left");
        assert!(!variables[0].explicit);
        assert_eq!(variables[1].symbol.name, "right");
        assert!(variables[1].explicit);
    }

    #[test]
    fn unsupported_nested_patterns_keep_the_scope_conservative() {
        let source =
            "fn f() { let Some(Point { x: pattern!() }) = input else { return; }; work(); }";
        let facts = Rust::default().facts(source).unwrap();
        let start = source.find("work").unwrap();
        assert!(
            !facts
                .lexical_tokens
                .contains(&vvv_core::Span::new(start, start + 4))
        );
    }

    #[test]
    fn rust_compiles_let_else_outer_and_inner_binding_visibility() {
        struct Probe;
        impl Probe {
            fn check(value: Option<usize>) -> usize {
                let Some(value) = value else {
                    assert!(value.is_none());
                    return 0;
                };
                value
            }
        }
        assert_eq!(Probe::check(None), 0);
        assert_eq!(Probe::check(Some(7)), 7);
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn let_headers_distinguish_uninitialized_initialized_and_else_forms() {
        let tree = Rust.ast_grep("fn f() { let uninitialized: u8; let value = input; let Some(x) = input else { return; }; }");
        let root = tree.root();
        let mut forms = root.dfs().filter_map(LetDeclaration::cast);
        let first = forms.next().unwrap();
        assert_eq!(first.pattern().unwrap().text(), "uninitialized");
        assert!(first.value().unwrap().is_none());
        assert!(first.alternative().unwrap().is_none());
        let initialized = forms.next().unwrap();
        assert_eq!(initialized.value().unwrap().unwrap().text(), "input");
        assert!(initialized.alternative().unwrap().is_none());
        let guarded = forms.next().unwrap();
        assert_eq!(guarded.pattern().unwrap().text(), "Some(x)");
        assert_eq!(guarded.alternative().unwrap().unwrap().kind(), "block");
    }
}
