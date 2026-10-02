use super::Rust;
use vvv_core::Language;

#[test]
fn expression_positions_do_not_introduce_outer_uncertainty() {
    for source in [
        "fn f(x: u8) { let v = opaque!(); consume(x); }",
        "fn f(x: u8) { consume(opaque!()); consume(x); }",
        "fn f(x: u8) { let v = (opaque!(), x); }",
        "fn f(x: u8) { let v = [opaque!(), x]; }",
        "fn f(x: u8) { let v = opaque!() + x; }",
    ] {
        let facts = Rust::default().facts(source).unwrap();
        assert!(facts.scope_uncertainties.is_empty(), "{source}");
        assert!(
            facts
                .lexical_tokens
                .contains(&facts.tokens_named("x").last().unwrap().0)
        );
        for (span, _) in facts.tokens_named("opaque") {
            assert!(!facts.navigation.contains(&span));
        }
    }
}

#[test]
fn binding_markers_are_per_identifier_not_per_pattern() {
    let source = "fn f(mut input: u8, plain: u8) { let mut value = 0; let (mut left, right) = (0, 0); let c = |mut a, b| a; }";
    let facts = Rust::default().facts(source).unwrap();
    for name in ["input", "value", "left", "a"] {
        assert!(
            facts
                .lexical
                .iter()
                .find(|b| b.symbol.name == name)
                .unwrap()
                .explicit,
            "{name}"
        );
    }
    for name in ["plain", "right", "b"] {
        assert!(
            !facts
                .lexical
                .iter()
                .find(|b| b.symbol.name == name)
                .unwrap()
                .explicit,
            "{name}"
        );
    }
}

#[test]
fn unknown_positions_and_nested_macros_retain_their_own_block() {
    for source in [
        "fn f() { opaque!(); }",
        "fn f() { opaque!{} }",
        "fn f() { opaque!() }",
        "fn f() { { opaque!(); } consume(); }",
        "fn f() { let v = { opaque!(); 1 }; }",
    ] {
        let facts = Rust::default().facts(source).unwrap();
        assert_eq!(facts.scope_uncertainties.len(), 1, "{source}");
        let unknown = &facts.scope_uncertainties[0];
        assert!(unknown.scope.contains(&unknown.invocation));
        assert!(source[unknown.scope.start..unknown.scope.end].starts_with('{'));
    }
    let facts = Rust::default()
        .facts("fn f() { outer!(inner!()); }")
        .unwrap();
    assert_eq!(facts.scope_uncertainties.len(), 1);
}

// Rust compiles these precedence examples: generated items are visible
// before invocation and can shadow parameters; later locals shadow generated
// locals, and identifier patterns can name constants instead of binding.
#[test]
fn rust_expansion_precedence_matches_navigation_contract() {
    macro_rules! introduce {
        ($item:ident, $local:ident) => {
            fn $item() -> usize {
                7
            }
            let $local = 8;
        };
    }
    let local = 3;
    assert_eq!(local, 3);
    assert_eq!(generated(), 7);
    introduce!(generated, local);
    assert_eq!(local, 8);
    let local = 9;
    assert_eq!(local, 9);
    fn nested(local: usize) -> usize {
        generated() + local
    }
    assert_eq!(nested(2), 9);
    macro_rules! item {
        ($name:ident) => {
            #[allow(dead_code)]
            fn $name() -> usize {
                7
            }
        };
    }

    fn parameter(_value: usize) -> usize {
        let before = _value();
        item!(_value);
        before
    }
    assert_eq!(parameter(3), 7);
    let local = 3;
    let before = local;
    item!(local);
    assert_eq!(before, 3);
    assert_eq!(local, 3);
    macro_rules! type_item {
        ($name:ident) => {
            struct $name;
        };
    }

    fn generic<T>(_: T) -> usize {
        let before = std::mem::size_of::<T>();
        type_item!(T);
        before
    }
    assert_eq!(generic(3_u64), 0);
    macro_rules! constant {
        ($name:ident) => {
            const $name: () = ();
        };
    }
    constant!(VALUE);
    let VALUE = ();
    let _: () = VALUE;
}
