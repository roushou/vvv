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
            .filter_map(|(node, symbol)| {
                let rule = self.rules.iter().find(|rule| node.kind() == rule.node)?;
                let body = rule
                    .body
                    .and_then(|field| {
                        super::declarations::Declaration::new(node.clone(), syntax).field(field)
                    })
                    .filter(|body| rule.body_kind.is_none_or(|kind| body.kind() == kind));
                let end = body.map_or(symbol.extent.end, |body| {
                    let prefix = &source[symbol.extent.start..body.range().start];
                    symbol.extent.start + prefix.trim_end().len()
                });
                Some(DeclarationSignature {
                    name_span: symbol.name_span,
                    span: Span::new(symbol.extent.start, end),
                })
            })
            .collect()
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
