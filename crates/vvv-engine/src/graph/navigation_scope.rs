//! Navigation precedence in scopes with unknown macro expansions.
use vvv_core::{BindingNamespace, Facts, LexicalBinding, PathHead, Span, SymbolKind};

pub(super) struct NavigationScope<'a> {
    facts: &'a Facts,
    span: Span,
}
impl<'a> NavigationScope<'a> {
    pub(super) fn new(facts: &'a Facts, span: Span) -> Self {
        Self { facts, span }
    }
    pub(super) fn permits_binding(&self, binding: &LexicalBinding) -> bool {
        self.facts
            .scope_uncertainties
            .iter()
            .filter(|unknown| unknown.scope.contains(&self.span))
            .all(|unknown| {
                if matches!(
                    binding.symbol.kind,
                    SymbolKind::Variable | SymbolKind::Parameter
                ) && !binding.explicit
                {
                    // An identifier pattern may name an unknown generated constant
                    // instead of introducing a local, even in an inner scope.
                    return false;
                }
                // A binding owned by a strict inner scope takes precedence over
                // any item/local introduced into the outer block.
                (unknown.scope != binding.scope && unknown.scope.contains(&binding.scope))
                    || (binding.namespace == BindingNamespace::Value
                        && binding.symbol.kind == SymbolKind::Variable
                        && binding.scope == unknown.scope
                        && (self.span.end <= unknown.invocation.start
                            || binding.visible_from >= unknown.invocation.end))
            })
    }
    pub(super) fn permits_module(&self) -> bool {
        !self
            .facts
            .scope_uncertainties
            .iter()
            .any(|unknown| unknown.scope.contains(&self.span))
            || self.facts.imports.iter().any(|import| {
                !import.declares
                    && import.span.contains(&self.span)
                    && import.path.head == PathHead::Package
            })
    }
}
