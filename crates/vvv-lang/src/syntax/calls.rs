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

    pub fn extract<L: LanguageExt>(
        &self,
        root: &Node<'_, StrDoc<L>>,
        facts: &mut Facts,
        syntax: super::navigation::NavigationSyntax,
    ) {
        facts.calls_supported = !self.grammar.calls.is_empty();
        for node in root.dfs() {
            #[cfg(feature = "rust")]
            if matches!(syntax, super::navigation::NavigationSyntax::Rust)
                && let Some(call) = super::rust::Call::cast(node.clone())
            {
                if let Some((callee, kind)) = call.callee(self.grammar) {
                    facts
                        .calls
                        .push(self.site(call.syntax(), &callee, kind, facts, syntax));
                }
                continue;
            }
            #[cfg(feature = "typescript")]
            if matches!(syntax, super::navigation::NavigationSyntax::TypeScript)
                && let Some(call) = super::typescript::Call::cast(node.clone())
            {
                if let Some((callee, kind)) = call.callee(self.grammar) {
                    facts
                        .calls
                        .push(self.site(call.syntax(), &callee, kind, facts, syntax));
                }
                continue;
            }
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
            facts
                .calls
                .push(self.site(&node, &callee, kind, facts, syntax));
        }
        facts.calls.sort_by_key(|c| (c.callee.start, c.span.start));
    }
    #[cfg(any(feature = "rust", feature = "typescript"))]
    fn callable_owner<'tree, L: LanguageExt, V: super::views::CallableView<'tree, L>>(
        &self,
        view: &V,
        facts: &Facts,
    ) -> (bool, Option<Span>) {
        if view.anonymous() {
            return (true, None);
        }
        (
            false,
            facts
                .symbols
                .iter()
                .find(|symbol| {
                    symbol.span == Span::from(view.syntax().range())
                        && matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
                })
                .map(|symbol| symbol.name_span),
        )
    }

    fn site<L: LanguageExt>(
        &self,
        node: &Node<'_, StrDoc<L>>,
        callee: &Node<'_, StrDoc<L>>,
        kind: CallKind,
        facts: &Facts,
        syntax: super::navigation::NavigationSyntax,
    ) -> CallSite {
        let mut owner = None;
        for ancestor in node.ancestors() {
            #[cfg(feature = "rust")]
            if matches!(syntax, super::navigation::NavigationSyntax::Rust) {
                let answer = if let Some(function) = super::rust::Function::cast(ancestor.clone()) {
                    Some(self.callable_owner(&function, facts))
                } else {
                    super::rust::Closure::cast(ancestor.clone())
                        .map(|closure| self.callable_owner(&closure, facts))
                };
                if let Some((anonymous, target)) = answer {
                    if anonymous {
                        break;
                    }
                    if target.is_some() {
                        owner = target;
                        break;
                    }
                }
                continue;
            }
            #[cfg(feature = "typescript")]
            if matches!(syntax, super::navigation::NavigationSyntax::TypeScript) {
                if let Some(function) = super::typescript::Function::cast(ancestor.clone()) {
                    let (anonymous, target) = self.callable_owner(&function, facts);
                    if anonymous {
                        break;
                    }
                    if target.is_some() {
                        owner = target;
                        break;
                    }
                }
                continue;
            }
            if self
                .grammar
                .anonymous_callables
                .contains(&ancestor.kind().as_ref())
            {
                break;
            }
            if let Some(symbol) = facts.symbols.iter().find(|symbol| {
                symbol.span == Span::from(ancestor.range())
                    && matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            }) {
                owner = Some(symbol.name_span);
                break;
            }
        }
        let _ = syntax;
        CallSite {
            span: node.range().into(),
            callee: callee.range().into(),
            kind,
            owner,
        }
    }
}

#[cfg(all(test, feature = "rust"))]
mod call_tests {
    use crate::rust::Rust;
    use vvv_core::{CallKind, Language};
    #[test]
    fn call_facts_distinguish_dispatch_and_anonymous_ownership() {
        let source = "fn outer() { work::<u8>(); module::work(); receiver.work(); (factory())(); let cb = || work(); fn nested() { work(); } }";
        let facts = Rust::default().facts(source).unwrap();
        assert!(facts.calls_supported);
        let outer = facts.symbols.iter().find(|s| s.name == "outer").unwrap();
        let nested = facts.symbols.iter().find(|s| s.name == "nested").unwrap();
        let calls = &facts.calls;
        assert!(
            calls
                .iter()
                .any(|c| c.kind == CallKind::Direct && c.owner == Some(outer.name_span))
        );
        assert!(calls.iter().any(|c| c.kind == CallKind::Member));
        assert!(calls.iter().any(|c| c.kind == CallKind::Indirect));
        let work: Vec<_> = calls
            .iter()
            .filter(|c| &source[c.callee.start..c.callee.end] == "work")
            .collect();
        assert!(work.iter().any(|c| c.owner.is_none()));
        assert!(work.iter().any(|c| c.owner == Some(nested.name_span)));
        assert!(calls.iter().all(|c| c.span.contains(&c.callee)));
    }
}
