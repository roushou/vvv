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
    ) -> Vec<DeclarationSignature> {
        declarations
            .iter()
            .filter_map(|(node, symbol)| {
                let rule = self.rules.iter().find(|rule| node.kind() == rule.node)?;
                let body = rule
                    .body
                    .and_then(|field| node.field(field))
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
