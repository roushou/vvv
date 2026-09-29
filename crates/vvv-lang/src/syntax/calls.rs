//! Lower call syntax without guessing runtime targets or crossing closure boundaries.
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{CallKind, CallSite, Facts, Grammar, Span, SymbolKind};

pub struct Calls<'a> {
    grammar: &'a Grammar,
}
impl<'a> Calls<'a> {
    pub fn new(grammar: &'a Grammar) -> Self {
        Self { grammar }
    }
    pub fn extract<L: LanguageExt>(&self, root: &Node<'_, StrDoc<L>>, facts: &mut Facts) {
        facts.calls_supported = !self.grammar.calls.is_empty();
        for node in root.dfs() {
            let Some(rule) = self.grammar.calls.iter().find(|r| r.node == node.kind()) else {
                continue;
            };
            let Some(mut callee) = node.field(rule.callee) else {
                continue;
            };
            let mut kind = CallKind::Direct;
            while let Some(rule) = self
                .grammar
                .callees
                .iter()
                .find(|r| r.node == callee.kind())
            {
                let Some(child) = callee.field(rule.field) else {
                    break;
                };
                callee = child;
                if let Some(classification) = rule.kind {
                    kind = classification;
                }
            }
            if !self.grammar.identifiers.contains(&callee.kind().as_ref()) {
                kind = CallKind::Indirect;
            }
            let mut owner = None;
            for ancestor in node.ancestors() {
                if self
                    .grammar
                    .anonymous_callables
                    .contains(&ancestor.kind().as_ref())
                {
                    break;
                }
                if let Some(symbol) = facts.symbols.iter().find(|s| {
                    s.span == Span::from(ancestor.range())
                        && matches!(s.kind, SymbolKind::Function | SymbolKind::Method)
                }) {
                    owner = Some(symbol.name_span);
                    break;
                }
            }
            facts.calls.push(CallSite {
                span: node.range().into(),
                callee: callee.range().into(),
                kind,
                owner,
            });
        }
        facts.calls.sort_by_key(|c| (c.callee.start, c.span.start));
    }
}
