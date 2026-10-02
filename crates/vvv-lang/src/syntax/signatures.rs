use ast_grep_core::Node;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use vvv_core::{DeclarationSignature, SignatureRule, Span, Symbol};

pub(crate) struct Signatures<'r> {
    rules: &'r [SignatureRule],
}

impl<'r> Signatures<'r> {
    pub(crate) fn new(rules: &'r [SignatureRule]) -> Self {
        Self { rules }
    }

    pub(crate) fn extract<L: LanguageExt>(
        &self,
        declarations: &[(Node<'_, StrDoc<L>>, Symbol)],
        source: &str,
        syntax: super::navigation::NavigationSyntax,
    ) -> Vec<DeclarationSignature> {
        declarations
            .iter()
            .filter_map(|(node, symbol)| self.capture(node, symbol, source, 0, syntax))
            .collect()
    }

    #[cfg(feature = "typescript")]
    pub(crate) fn navigation<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        symbol: &Symbol,
        syntax: super::navigation::NavigationSyntax,
    ) -> Option<DeclarationSignature> {
        self.capture(
            node,
            symbol,
            node.text().as_ref(),
            node.range().start,
            syntax,
        )
    }

    fn capture<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        symbol: &Symbol,
        source: &str,
        offset: usize,
        syntax: super::navigation::NavigationSyntax,
    ) -> Option<DeclarationSignature> {
        let rule = self.rules.iter().find(|rule| node.kind() == rule.node)?;
        let declaration = super::declarations::Declaration::new(node.clone(), syntax);
        let body = if let Some(field) = rule.initializer {
            let initializer = declaration.initializer(field)?;
            super::declarations::Declaration::new(initializer, syntax).callable_body(rule.body?)
        } else {
            rule.body.and_then(|field| declaration.field(field))
        }
        .filter(|body| rule.body_kind.is_none_or(|kind| body.kind() == kind));
        if rule.initializer.is_some() && body.is_none() {
            return None;
        }
        let signature_start = declaration.signature_start(symbol.extent.start);
        let start = signature_start.checked_sub(offset)?;
        let end = match body {
            Some(body) => {
                let prefix = source.get(start..body.range().start.checked_sub(offset)?)?;
                signature_start + prefix.trim_end().len()
            }
            None => symbol.extent.end,
        };
        source.get(start..end.checked_sub(offset)?)?;
        Some(DeclarationSignature {
            name_span: symbol.name_span,
            span: Span::new(signature_start, end),
        })
    }
}

#[cfg(all(test, feature = "rust"))]
mod signature_tests {
    use crate::rust::Rust;
    use vvv_core::Language;

    #[test]
    fn signatures_preserve_docs_attributes_constraints_and_tuple_fields() {
        let cases = [
            (
                "/// café\r\n#[inline]\r\npub fn build<const N: usize>(a: [u8; { 2 }]) -> [u8; N]\r\nwhere [u8; N]: Sized",
                " { [0; N] }",
                "build",
            ),
            ("pub struct Tuple<T>(T) where T: Copy;", "", "Tuple"),
            (
                "pub struct Named<T> where T: Copy",
                " { value: T }",
                "Named",
            ),
            ("pub enum Choice<T>", " { Some(T), None }", "Choice"),
            (
                "pub trait Work<T>: Sized where T: Copy",
                " { fn work(&self); }",
                "Work",
            ),
            ("impl<T: Copy> Work<T> for T", " { fn work(&self) {} }", "T"),
            ("pub type Alias = [u8; { 2 }];", "", "Alias"),
        ];
        for (header, body, name) in cases {
            let source = format!("{header}{body}");
            let facts = Rust::default().facts(&source).unwrap();
            let symbol = facts
                .symbols
                .iter()
                .find(|symbol| symbol.name == name)
                .unwrap();
            let signature = facts
                .signatures
                .iter()
                .find(|s| s.name_span == symbol.name_span)
                .unwrap();
            assert_eq!(
                &source[signature.span.start..signature.span.end],
                header,
                "{name}"
            );
        }
        let source = "trait Work { /// Required.\nfn work(&self) -> u8; }\nconst VALUE: u8 = 1;";
        let facts = Rust::default().facts(source).unwrap();
        let work = facts.symbols.iter().find(|s| s.name == "work").unwrap();
        let signature = facts
            .signatures
            .iter()
            .find(|s| s.name_span == work.name_span)
            .unwrap();
        assert_eq!(
            &source[signature.span.start..signature.span.end],
            "/// Required.\nfn work(&self) -> u8;"
        );
        let constant = facts.symbols.iter().find(|s| s.name == "VALUE").unwrap();
        assert!(
            !facts
                .signatures
                .iter()
                .any(|s| s.name_span == constant.name_span)
        );
    }
}

#[cfg(all(test, feature = "typescript"))]
mod callable_tests {
    use vvv_core::Language;

    struct SignatureFixture {
        source: String,
        facts: vvv_core::Facts,
    }

    impl SignatureFixture {
        fn new(source: &str, language: &dyn Language) -> Self {
            Self {
                source: source.into(),
                facts: language.facts(source).unwrap(),
            }
        }

        fn text(&self, name: &str, kind: vvv_core::SymbolKind) -> Option<&str> {
            let symbol = self
                .facts
                .symbols
                .iter()
                .chain(self.facts.lexical.iter().map(|binding| &binding.symbol))
                .find(|symbol| symbol.name == name && symbol.kind == kind)?;
            let signature = self
                .facts
                .signatures
                .iter()
                .find(|signature| signature.name_span == symbol.name_span)?;
            Some(&self.source[signature.span.start..signature.span.end])
        }
    }

    #[test]
    fn callable_values_retain_types_comments_modifiers_and_exact_body_boundaries() {
        for language in [
            &crate::typescript::TypeScript::default() as &dyn Language,
            &crate::typescript::Tsx::default(),
        ] {
            for (source, expected) in [
                (
                    "/** café */\r\nexport const build = <T>(value: T): { value: T } => ({ value });",
                    "/** café */\r\nexport const build = <T>(value: T): { value: T } =>",
                ),
                (
                    "export const build = function named(value: { text: string }): string { return value.text; };",
                    "export const build = function named(value: { text: string }): string",
                ),
                (
                    "const build = function*(value: number) { yield value; };",
                    "build = function*(value: number)",
                ),
                (
                    "const build = async value => value;",
                    "build = async value =>",
                ),
            ] {
                let fixture = SignatureFixture::new(source, language);
                assert_eq!(
                    fixture.text("build", vvv_core::SymbolKind::Variable),
                    Some(expected),
                    "{source}"
                );
            }
        }
    }

    #[test]
    fn later_declarators_never_capture_the_first_implementation() {
        let source = "/** callbacks */ export const first = value => value, second = function named() { return first; };";
        let fixture = SignatureFixture::new(source, &crate::typescript::TypeScript::default());
        assert_eq!(
            fixture.text("first", vvv_core::SymbolKind::Variable),
            Some("/** callbacks */ export const first = value =>")
        );
        assert_eq!(
            fixture.text("second", vvv_core::SymbolKind::Variable),
            Some("second = function named()")
        );
        assert_eq!(
            fixture.text("named", vvv_core::SymbolKind::Function),
            Some("function named()")
        );
        assert!(
            !fixture
                .facts
                .symbols
                .iter()
                .any(|symbol| symbol.name == "named")
        );
    }

    #[test]
    fn literal_class_and_unfinished_values_remain_unsupported() {
        for source in [
            "const build = 42;",
            "const build = class { method() {} };",
            "const build = function(value: ) { return value; };",
            "let build;",
        ] {
            let fixture = SignatureFixture::new(source, &crate::typescript::TypeScript::default());
            assert_eq!(
                fixture.text("build", vvv_core::SymbolKind::Variable),
                None,
                "{source}"
            );
        }
    }

    #[test]
    fn class_fields_and_tsx_expression_bodies_use_the_same_callable_contract() {
        let fixture = SignatureFixture::new(
            "class View { /** field */ readonly render = (title: string) => <div>{title}</div>; literal = 42; }",
            &crate::typescript::Tsx::default(),
        );
        assert_eq!(
            fixture.text("render", vvv_core::SymbolKind::Field),
            Some("/** field */ readonly render = (title: string) =>")
        );
        assert_eq!(fixture.text("literal", vvv_core::SymbolKind::Field), None);
    }

    #[test]
    fn callable_wrappers_keep_exact_written_prefixes_and_reject_unknown_or_malformed_forms() {
        for (source, expected) in [
            (
                "const build = ((value: number) => value);",
                Some("build = ((value: number) =>"),
            ),
            (
                "const build = ((value: number) => { return value; }) satisfies (value: number) => number;",
                Some("build = ((value: number) =>"),
            ),
            (
                "const build = (function(value: number) { return value; }) as (value: number) => number;",
                Some("build = (function(value: number)"),
            ),
            (
                "const build = <(value: number) => number>((value: number) => value);",
                Some("build = <(value: number) => number>((value: number) =>"),
            ),
            ("const build = (() => 1)!;", Some("build = (() =>")),
            ("const build = (42 as number);", None),
            ("const build = ((value: ) => value);", None),
            ("const build = (() => 1) as ;", None),
        ] {
            let fixture = SignatureFixture::new(source, &crate::typescript::TypeScript::default());
            assert_eq!(
                fixture.text("build", vvv_core::SymbolKind::Variable),
                expected,
                "{source}"
            );
        }
        let source = format!(
            "const build = {}() => 1{};",
            "(".repeat(129),
            ")".repeat(129)
        );
        let fixture = SignatureFixture::new(&source, &crate::typescript::TypeScript::default());
        assert_eq!(fixture.text("build", vvv_core::SymbolKind::Variable), None);
    }

    #[test]
    fn initializer_rules_remain_field_driven_for_generic_grammar_consumers() {
        use ast_grep_core::tree_sitter::LanguageExt;
        let tree =
            ast_grep_language::TypeScript.ast_grep("const callback = (value: number) => value;");
        let root = tree.root();
        let node = root
            .dfs()
            .find(|node| node.kind() == "variable_declarator")
            .unwrap();
        let name = node.field("name").unwrap();
        let symbol = vvv_core::Symbol::plain(
            vvv_core::SymbolKind::Variable,
            "callback",
            name.range().into(),
            node.range().into(),
        );
        let rules = [vvv_core::SignatureRule::callable(
            "variable_declarator",
            "value",
            "body",
        )];
        let signatures = super::Signatures::new(&rules).extract(
            &[(node, symbol)],
            "const callback = (value: number) => value;",
            crate::syntax::NavigationSyntax::Tables,
        );
        assert_eq!(signatures.len(), 1);
        assert_eq!(signatures[0].span, vvv_core::Span::new(6, 35));
    }
}
