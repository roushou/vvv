use super::*;
use vvv_core::{Language, Span, SymbolKind};

#[test]
fn construct_views_preserve_complete_table_facts_for_existing_binding_forms() {
    for source in [
        "fn f<T>(mut input: T) { let value = input; let (left, mut right) = pair; let Some(next) = value else { let _ = input; return; }; next; }",
        "fn f() { let value = 1; let closure = |parameter: usize| value + parameter; fn inner<U>(parameter: U) { parameter; } closure; }",
        "trait T<A> { fn f<B>(parameter: B); } impl<A> T<A> for A { fn f<B>(parameter: B) { fn inner() {} parameter; } }",
        "fn f() { use crate::a::work; unknown!(); let mut value = 1; { let mut inner = value; inner; } work(); }",
        "fn f() { let closure = |left, right| left; let (a, (b, c)) = input; }",
        "struct S<T>(T); enum E<T> { Value(T) } trait Tr<T> { fn run<U>(value: U); } impl<T> Tr<T> for S<T> { fn run<U>(value: U) { value; } } type Alias<T> = S<T>;",
        "fn f() { use crate::work as local; receiver.work(); module::work::<usize>(); (factory())(); } mod inner { pub(super) use crate::work as local; fn run() { local(); } }",
    ] {
        let mut tables = Rust::new()
            .with_navigation_syntax(NavigationSyntax::Tables)
            .facts(source)
            .unwrap();
        let mut constructs = Rust::new().facts(source).unwrap();
        // Pattern-role evidence is an extension of the former table contract.
        let references: Vec<_> = constructs
            .patterns
            .iter()
            .flat_map(|pattern| &pattern.references)
            .filter(|reference| reference.role != vvv_core::PatternRole::Identifier)
            .map(|reference| reference.span)
            .collect();
        for facts in [&mut tables, &mut constructs] {
            facts.patterns.clear();
            facts.pattern_constructors.clear();
            facts
                .lexical_tokens
                .retain(|span| !references.contains(span));
            facts.navigation.sort();
            facts.navigation.dedup();
        }
        assert_eq!(constructs, tables, "{source}");
    }
}

#[test]
fn condition_chain_bindings_start_after_each_operand_and_exclude_alternatives() {
    let source = "fn f(value: usize, input: Option<usize>) { if let Some(value) = input && value > 0 && let Some(value) = Some(value) && value > 1 { value; fn inner() { value; } } else { value; } value; }";
    let facts = Rust::new().facts(source).unwrap();
    let bindings: Vec<_> = facts
        .lexical
        .iter()
        .filter(|binding| {
            binding.symbol.kind == SymbolKind::Variable && binding.symbol.name == "value"
        })
        .collect();
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].scope, bindings[1].scope);
    assert_eq!(bindings[0].scope.start, source.find("if let").unwrap());
    assert_eq!(bindings[0].scope.end, source.find(" else").unwrap());
    let first_use = source.find("value > 0").unwrap();
    let second_initializer = source.find("Some(value) &&").unwrap() + 5;
    let second_use = source.find("value > 1").unwrap();
    assert!(bindings[0].visible(
        "value",
        Span::new(first_use, first_use + 5),
        vvv_core::BindingNamespace::Value
    ));
    assert!(!bindings[1].visible(
        "value",
        Span::new(second_initializer, second_initializer + 5),
        vvv_core::BindingNamespace::Value
    ));
    assert!(bindings[1].visible(
        "value",
        Span::new(second_use, second_use + 5),
        vvv_core::BindingNamespace::Value
    ));
    let nested = source.find("fn inner").unwrap();
    assert!(
        bindings
            .iter()
            .all(|binding| binding.excluded.iter().any(|span| span.start == nested))
    );
    let alternative = source.rfind("else { value").unwrap() + 7;
    assert!(bindings.iter().all(|binding| {
        !binding
            .scope
            .contains(&Span::new(alternative, alternative + 5))
    }));
}

#[test]
fn unsupported_conditional_patterns_publish_no_prefix_and_do_not_escape() {
    for pattern in [
        "(left, Point { field: pattern!() })",
        "Some(Point { field: pattern!() })",
        "(left, [.., ..])",
        "Some(left @)",
    ] {
        let source = format!(
            "fn f(value: usize) {{ if let {pattern} = input {{ value; }} else {{ value; }} value; }}"
        );
        let facts = Rust::new().facts(&source).unwrap();
        assert!(
            !facts
                .lexical
                .iter()
                .any(|binding| binding.symbol.kind == SymbolKind::Variable),
            "{source}"
        );
        let outside = source.rfind("value;").unwrap();
        assert!(
            facts
                .lexical_tokens
                .contains(&Span::new(outside, outside + 5)),
            "{source}"
        );
        let inside = source.find("{ value;").unwrap() + 2;
        assert!(
            !facts
                .lexical_tokens
                .contains(&Span::new(inside, inside + 5)),
            "{source}"
        );
    }
}

#[test]
fn nested_loop_coverage_and_mutable_marker_ownership_remain_independent() {
    let source = "fn f() { if let Some((plain, mut explicit)) = input { while ready { plain; } let _ = explicit; } }";
    let facts = Rust::new().facts(source).unwrap();
    let plain = facts
        .lexical
        .iter()
        .find(|binding| binding.symbol.name == "plain")
        .unwrap();
    let explicit = facts
        .lexical
        .iter()
        .find(|binding| binding.symbol.name == "explicit")
        .unwrap();
    assert!(!plain.explicit);
    assert!(explicit.explicit);
    let inside = source.find("plain;").unwrap();
    assert!(
        facts
            .lexical_tokens
            .contains(&Span::new(inside, inside + 5))
    );
    let outside = source.rfind("explicit;").unwrap();
    assert!(
        facts
            .lexical_tokens
            .contains(&Span::new(outside, outside + 8))
    );
}

#[test]
fn rust_compiles_success_failure_and_ordered_chain_visibility() {
    let value = 10;
    let input = Some(1);
    if let Some(value) = input
        && value == 1
        && let Some(value) = Some(value + 1)
        && value == 2
    {
        assert_eq!(value, 2);
        let capture = || value;
        assert_eq!(capture(), 2);
    } else {
        assert_eq!(value, 10);
    }
    assert_eq!(value, 10);
    if let Some(value) = None::<usize> {
        assert_eq!(value, 0);
    } else if let Some(value) = Some(3) {
        assert_eq!(value, 3);
    } else {
        assert_eq!(value, 10);
    }
}
