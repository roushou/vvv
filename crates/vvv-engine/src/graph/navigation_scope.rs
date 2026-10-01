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
    /// An immutable pattern may refer to a module constant or constructor.
    /// Consult those bindings before treating its spelling as a new local.
    pub(super) fn pattern_module(&self, name: &str) -> bool {
        self.facts.lexical.iter().any(|binding| {
            binding.symbol.name == name
                && binding.symbol.kind == SymbolKind::Variable
                && !binding.explicit
                && (binding.symbol.name_span == self.span
                    || binding.visible(name, self.span, BindingNamespace::Value))
        }) && self.pattern_evidence(name)
    }

    pub(super) fn pattern_evidence(&self, name: &str) -> bool {
        self.facts
            .module_scopes
            .iter()
            .filter(|scope| scope.span.contains(&self.span))
            .min_by_key(|scope| scope.span.len())
            .is_some_and(|scope| {
                self.facts.symbols.iter().any(|symbol| {
                    symbol.name == name
                        && matches!(
                            symbol.kind,
                            SymbolKind::Const
                                | SymbolKind::Static
                                | SymbolKind::Struct
                                | SymbolKind::Variant
                        )
                        && scope
                            .declarations
                            .iter()
                            .any(|declaration| declaration.name_span == symbol.name_span)
                }) || self.facts.imports.iter().any(|import| {
                    (import.glob || import.binding().is_some_and(|bound| bound.as_str() == name))
                        && scope
                            .imports
                            .iter()
                            .any(|binding| binding.span == import.span)
                })
            })
    }

    pub(super) fn import_scope(&self, name: &str) -> Option<Span> {
        self.facts
            .import_scopes
            .iter()
            .filter(|scope| scope.span.contains(&self.span))
            .filter(|scope| {
                scope.imports.iter().any(|binding| {
                    self.facts.imports.iter().any(|import| {
                        import.span == binding.span
                            && import.binding().is_some_and(|bound| bound.as_str() == name)
                    })
                })
            })
            .min_by_key(|scope| scope.span.len())
            .map(|scope| scope.span)
    }
}
