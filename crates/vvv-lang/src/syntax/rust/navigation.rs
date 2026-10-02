use super::{
    block::Block,
    conditional::Conditional,
    header::{Closure, Constructor, Enum, Function, HeaderBindings, Impl, Struct, Trait, Type},
    let_declaration::LetBinding,
    loop_expression::LoopBindings,
    macro_invocation::MacroInvocation,
    match_expression::MatchNavigation,
    pattern::FieldPattern,
};
use crate::syntax::navigation::{NavigationCoverage, NavigationFacts};
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{Facts, Span};

/// Coverage evidence for a recognized construct whose binding syntax is unknown.
#[derive(Debug)]
pub(super) struct UnsupportedSyntax {
    pub span: Span,
}

impl UnsupportedSyntax {
    pub fn at<L: LanguageExt>(node: &Node<'_, StrDoc<L>>) -> Self {
        Self {
            span: node.range().into(),
        }
    }

    pub fn validate<L: LanguageExt>(node: &Node<'_, StrDoc<L>>) -> Result<(), Self> {
        if node.is_error()
            || node.is_missing()
            || node
                .children()
                .any(|child| child.is_error() || child.is_missing())
        {
            Err(Self::at(node))
        } else {
            Ok(())
        }
    }
}

pub(crate) struct RustNavigation<'a, 'g> {
    shared: &'a NavigationFacts<'g>,
}

impl<'a, 'g> RustNavigation<'a, 'g> {
    pub fn new(shared: &'a NavigationFacts<'g>) -> Self {
        Self { shared }
    }
    /// Returns whether this view owns binding extraction for this node.
    pub fn extract<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) -> bool {
        if let Some(fact) = Constructor::cast(node.clone()).and_then(|view| view.fact()) {
            facts.pattern_constructors.push(fact);
        }
        if let Some(field) = FieldPattern::cast(node.clone()) {
            if let Some(label) = field.explicit_label() {
                coverage.block(label.range().into());
            }
            return true;
        }
        if let Some(invocation) = MacroInvocation::cast(node.clone()) {
            invocation.extract(self.shared, facts);
            return true;
        }
        if let Some(import) = super::Import::cast(node.clone()) {
            import.extract(self.shared, facts, coverage);
            return true;
        }
        if node.kind() == "match_expression" {
            match MatchNavigation::from_node(node.clone()) {
                Ok(expression) => expression.extract(self.shared, facts, coverage),
                Err(unknown) => coverage.block(unknown.span),
            }
            return true;
        }
        if matches!(
            node.kind().as_ref(),
            "loop_expression" | "while_expression" | "for_expression"
        ) {
            match LoopBindings::from_node(node.clone()) {
                Ok(expression) => expression.extract(self.shared, facts, coverage),
                Err(unknown) => coverage.block(unknown.span),
            }
            return true;
        }
        if self.header(node, facts, coverage) {
            return true;
        }
        if node.kind() == "if_expression" {
            match Conditional::from_node(node.clone()) {
                Ok(conditional) => conditional.extract(self.shared, facts, coverage),
                Err(unknown) => coverage.block(unknown.span),
            }
            return true;
        }
        if node.kind() == "let_declaration" {
            let Some(rule) = self
                .shared
                .grammar
                .bindings
                .iter()
                .find(|rule| rule.node == "let_declaration")
            else {
                return false;
            };
            if let Some(scope) = self.shared.scope(node, rule, coverage) {
                let Some(owner) = Block::cast(scope) else {
                    return false;
                };
                debug_assert!(
                    owner
                        .statements()
                        .any(|statement| statement.range() == node.range())
                );
                match LetBinding::from_node(node.clone()) {
                    Ok(declaration) => {
                        declaration.extract(&owner, self.shared, rule, facts, coverage)
                    }
                    Err(_) => coverage.block(owner.span()),
                }
            }
            return true;
        }
        if matches!(node.kind().as_ref(), "parameter" | "type_parameter") {
            return true;
        }
        false
    }

    fn header<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        facts: &mut Facts,
        coverage: &mut NavigationCoverage,
    ) -> bool {
        use crate::syntax::views::SyntaxError;
        let shape: Option<Result<_, SyntaxError>> = match node.kind().as_ref() {
            "function_item" | "function_signature_item" => {
                Function::cast(node.clone()).map(|view| {
                    debug_assert_eq!(view.syntax().range(), node.range());
                    let parameters = view.parameters()?;
                    Ok((view.generics()?, Some(parameters)))
                })
            }
            "closure_expression" => Closure::cast(node.clone()).map(|view| {
                debug_assert_eq!(view.syntax().range(), node.range());
                let parameters = view.parameters()?;
                let body = view.body()?;
                let _ = body;
                Ok((view.generics()?, Some(parameters)))
            }),
            "impl_item" => Impl::cast(node.clone()).map(|view| {
                debug_assert_eq!(view.syntax().range(), node.range());
                let target = view.target()?;
                let _ = target;
                Ok((view.generics()?, None))
            }),
            "trait_item" => Trait::cast(node.clone()).map(|view| {
                debug_assert_eq!(view.syntax().range(), node.range());
                let name = view.name()?;
                let body = view.body()?;
                let _ = name;
                let _ = body;
                Ok((view.generics()?, None))
            }),
            "struct_item" => Struct::cast(node.clone()).map(|view| {
                debug_assert_eq!(view.syntax().range(), node.range());
                let name = view.name()?;
                let _ = name;
                Ok((view.generics()?, None))
            }),
            "enum_item" => Enum::cast(node.clone()).map(|view| {
                debug_assert_eq!(view.syntax().range(), node.range());
                let name = view.name()?;
                let body = view.body()?;
                let _ = name;
                let _ = body;
                Ok((view.generics()?, None))
            }),
            "type_item" => Type::cast(node.clone()).map(|view| {
                debug_assert_eq!(view.syntax().range(), node.range());
                let name = view.name()?;
                let value = view.value()?;
                let _ = name;
                let _ = value;
                Ok((view.generics()?, None))
            }),
            _ => None,
        };
        let Some(shape) = shape else {
            return false;
        };
        // Local item bindings precede header bindings, as in the source walk.
        if node.kind() != "closure_expression" {
            self.shared.table_bindings(node, facts, coverage);
        }
        let mut header = HeaderBindings::new(node.clone(), self.shared);
        match shape {
            Ok((generics, parameters)) => {
                if let Some(generics) = generics {
                    header.declarations(generics, facts, coverage);
                }
                if node.kind() == "closure_expression"
                    && let Some(rule) = self
                        .shared
                        .grammar
                        .bindings
                        .iter()
                        .find(|rule| rule.node == "closure_expression")
                {
                    header.extract(node, rule, facts, coverage);
                }
                if let Some(parameters) = parameters {
                    header.declarations(parameters, facts, coverage);
                }
            }
            Err(_) => coverage.block(header.span()),
        }
        true
    }
}

#[cfg(test)]
mod navigation_tests {
    use crate::rust::Rust;
    use vvv_core::Language;

    #[test]
    fn type_sites_and_generic_bindings_have_separate_navigation_facts() {
        let source = "use crate::Engine;\nstruct App { engine: Engine }\nfn f(value: Engine) -> Engine { value }\nfn generic<Engine>(value: Engine) {}\nfn local() { type Engine = u8; let x: Engine = 0; }\nimpl App { fn g(value: Engine) {} }";
        let facts = Rust::default().facts(source).unwrap();
        let engine_tokens: Vec<_> = facts.tokens_named("Engine").collect();
        assert_eq!(engine_tokens.len(), 9);
        for (index, (span, _)) in engine_tokens.iter().enumerate() {
            assert!(facts.navigation.contains(span), "token {index}");
        }
    }
}

#[cfg(test)]
mod lexical_navigation_tests {
    use crate::rust::Rust;
    use vvv_core::{BindingNamespace, Language, Span};
    #[test]
    fn locals_shadow_after_initializers_and_generics_do_not_escape_into_nested_items() {
        let source = "fn outer<T>(x: T) { let x = x; { let x = 1; use_it(x); } use_it(x); fn nested() { use_it(x); let y: T; } }";
        let facts = Rust::default().facts(source).unwrap();
        let generic = facts.lexical.iter().find(|b| b.symbol.name == "T").unwrap();
        let nested_t = source.rfind('T').unwrap();
        assert!(!generic.visible(
            "T",
            Span::new(nested_t, nested_t + 1),
            BindingNamespace::Type
        ));
        let initializer = source.find("= x").unwrap() + 2;
        let visible: Vec<_> = facts
            .lexical
            .iter()
            .filter(|b| {
                b.visible(
                    "x",
                    Span::new(initializer, initializer + 1),
                    BindingNamespace::Value,
                )
            })
            .collect();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].symbol.kind, vvv_core::SymbolKind::Parameter);
        assert!(facts.symbols.iter().all(|s| !matches!(
            s.kind,
            vvv_core::SymbolKind::Parameter | vvv_core::SymbolKind::TypeParameter
        )));
    }
    #[test]
    fn complex_patterns_and_receiver_members_do_not_inherit_an_outer_binding() {
        let source = "fn f(x: u8, Point { a: pattern!(), b }: Point) { use_it(x); }";
        let facts = Rust::default().facts(source).unwrap();
        let span = facts.tokens_named("x").last().unwrap().0;
        assert!(!facts.lexical_tokens.contains(&span));
        let facts = Rust::default().facts("fn f(x: u8) { obj.x; }").unwrap();
        assert!(
            !facts
                .lexical_tokens
                .contains(&facts.tokens_named("x").last().unwrap().0)
        );
    }
    #[test]
    fn nested_type_items_cannot_capture_outer_generics_and_macros_record_uncertainty() {
        let source = "fn f<T>(value: T) { type Alias = T; }";
        let facts = Rust::default().facts(source).unwrap();
        let usage = facts.tokens_named("T").last().unwrap().0;
        assert!(!facts.lexical.iter().any(|b| b.visible(
            "T",
            usage,
            vvv_core::BindingNamespace::Type
        )));
        let source = "fn f(value: u8) { introduce_binding!(); consume(value); }";
        let facts = Rust::default().facts(source).unwrap();
        assert!(
            facts
                .lexical_tokens
                .contains(&facts.tokens_named("value").last().unwrap().0)
        );
        assert_eq!(facts.scope_uncertainties.len(), 1);
    }
    #[test]
    fn tuple_bindings_and_closures_preserve_shadowing_and_outer_captures() {
        let source = "fn f((a, b): (u8,u8), value: u8) { let (x,y) = (a,b); let f = |value| value; consume(value); }";
        let facts = Rust::default().facts(source).unwrap();
        for name in ["a", "b", "x", "y"] {
            assert!(facts.lexical.iter().any(|b| b.symbol.name == name));
        }
        let value_tokens: Vec<_> = facts.tokens_named("value").map(|t| t.0).collect();
        let chosen = |span| {
            facts
                .lexical
                .iter()
                .filter(|b| b.visible("value", span, vvv_core::BindingNamespace::Value))
                .min_by_key(|b| b.scope.len())
                .unwrap()
                .symbol
                .name_span
        };
        assert_eq!(chosen(value_tokens[2]), value_tokens[1]);
        assert_eq!(chosen(value_tokens[3]), value_tokens[0]);
    }
}
