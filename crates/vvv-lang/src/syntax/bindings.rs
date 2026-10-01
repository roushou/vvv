//! Grammar-owned name policy over borrowed structural pattern views.

use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::Grammar;

/// Generic table syntax and TypeScript views answer the same structural questions.
pub(super) trait PatternView<'tree, L: LanguageExt>: Sized {
    fn from_node(node: Node<'tree, StrDoc<L>>) -> Self;
    fn syntax(&self) -> &Node<'tree, StrDoc<L>>;
    fn field(&self, field: &str) -> Option<Node<'tree, StrDoc<L>>>;
    fn children(&self) -> impl Iterator<Item = Node<'tree, StrDoc<L>>>;
}

pub(super) struct TablePattern<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

impl<'tree, L: LanguageExt> TablePattern<'tree, L> {
    pub fn new(node: Node<'tree, StrDoc<L>>) -> Self {
        Self { node }
    }
}

impl<'tree, L: LanguageExt> PatternView<'tree, L> for TablePattern<'tree, L> {
    fn from_node(node: Node<'tree, StrDoc<L>>) -> Self {
        Self::new(node)
    }

    fn syntax(&self) -> &Node<'tree, StrDoc<L>> {
        &self.node
    }

    fn field(&self, field: &str) -> Option<Node<'tree, StrDoc<L>>> {
        self.node.field(field)
    }

    fn children(&self) -> impl Iterator<Item = Node<'tree, StrDoc<L>>> {
        self.node.children().filter(Node::is_named)
    }
}

pub(super) struct PatternNames<'g> {
    grammar: &'g Grammar,
}

impl<'g> PatternNames<'g> {
    pub fn new(grammar: &'g Grammar) -> Self {
        Self { grammar }
    }

    pub fn names<'tree, L: LanguageExt, P: PatternView<'tree, L>>(
        &self,
        pattern: &P,
    ) -> Option<Vec<Node<'tree, StrDoc<L>>>> {
        let node = pattern.syntax();
        let kind = node.kind();
        if matches!(kind.as_ref(), "identifier" | "type_identifier") {
            return Some(vec![node.clone()]);
        }
        if matches!(kind.as_ref(), "_" | "mutable_specifier") {
            return Some(vec![]);
        }
        // A binding rule lowers these parameter declarations independently.
        if self
            .grammar
            .bindings
            .iter()
            .any(|rule| rule.node == kind && rule.name == Some("pattern"))
        {
            return Some(vec![]);
        }
        if !self.grammar.pattern_containers.contains(&kind.as_ref()) {
            return None;
        }
        let constructor = self
            .grammar
            .pattern_constructors
            .iter()
            .find(|(kind, _)| *kind == node.kind())
            .and_then(|(_, field)| pattern.field(field));
        let mut names = Vec::new();
        for child in pattern.children() {
            if constructor
                .as_ref()
                .is_some_and(|head| head.range() == child.range())
            {
                continue;
            }
            names.extend(self.names(&P::from_node(child))?);
        }
        Some(names)
    }
}
