//! Borrowed structural views. Interpretation and facts belong to their consumers.

use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::Span;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum FieldIssue {
    Absent,
    Missing,
    Error,
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SyntaxError {
    pub span: Span,
    pub field: &'static str,
    pub issue: FieldIssue,
}

pub(super) struct Field {
    pub name: &'static str,
}

impl Field {
    pub fn optional<'tree, L: LanguageExt>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
    ) -> Result<Option<Node<'tree, StrDoc<L>>>, SyntaxError> {
        let Some(child) = node.field(self.name) else {
            return Ok(None);
        };
        self.checked(child).map(Some)
    }

    fn checked<'tree, L: LanguageExt>(
        &self,
        child: Node<'tree, StrDoc<L>>,
    ) -> Result<Node<'tree, StrDoc<L>>, SyntaxError> {
        let issue = if child.is_missing() {
            Some(FieldIssue::Missing)
        } else if child.is_error() {
            Some(FieldIssue::Error)
        } else {
            None
        };
        match issue {
            Some(issue) => Err(SyntaxError {
                span: child.range().into(),
                field: self.name,
                issue,
            }),
            None => Ok(child),
        }
    }

    pub fn required<'tree, L: LanguageExt>(
        &self,
        node: &Node<'tree, StrDoc<L>>,
    ) -> Result<Node<'tree, StrDoc<L>>, SyntaxError> {
        self.optional(node)?.ok_or_else(|| SyntaxError {
            span: node.range().into(),
            field: self.name,
            issue: FieldIssue::Absent,
        })
    }
}

/// Owns a cheap parent handle so callers can iterate a field's children lazily.
pub(super) struct DirectChildren<'tree, L: LanguageExt> {
    parent: Node<'tree, StrDoc<L>>,
    position: usize,
    excluded: &'static [&'static str],
}

impl<'tree, L: LanguageExt> DirectChildren<'tree, L> {
    pub fn new(parent: Node<'tree, StrDoc<L>>, excluded: &'static [&'static str]) -> Self {
        Self {
            parent,
            position: 0,
            excluded,
        }
    }
}

impl<'tree, L: LanguageExt> Iterator for DirectChildren<'tree, L> {
    type Item = Node<'tree, StrDoc<L>>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let child = self.parent.child(self.position)?;
            self.position += 1;
            if child.is_named() && !self.excluded.contains(&child.kind().as_ref()) {
                return Some(child);
            }
        }
    }
}

macro_rules! syntax_view {
    (impl $name:ident { kinds: [$($kind:literal),+ $(,)?], required: {$($method:ident: $field:literal),* $(,)?}, optional: {$($optional:ident: $optional_field:literal),* $(,)?} $(, children: {$($children:ident: [$($excluded:literal),* $(,)?]),* $(,)?})? $(,)? }) => {
        impl<'tree, L: ast_grep_core::tree_sitter::LanguageExt> $name<'tree, L> {
            pub fn cast(node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>) -> Option<Self> {
                matches!(node.kind().as_ref(), $($kind)|+).then_some(Self { node })
            }

            pub fn syntax(&self) -> &ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>> { &self.node }
            $(pub fn $method(&self) -> Result<ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>, $crate::syntax::views::SyntaxError> {
                $crate::syntax::views::Field { name: $field }.required(self.syntax())
            })*
            $(pub fn $optional(&self) -> Result<Option<ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>>, $crate::syntax::views::SyntaxError> {
                $crate::syntax::views::Field { name: $optional_field }.optional(self.syntax())
            })*
            $($(pub fn $children(&self) -> $crate::syntax::views::DirectChildren<'tree, L> {
                $crate::syntax::views::DirectChildren::new(self.syntax().clone(), &[$($excluded),*])
            })*)?

        }
    };
}
pub(super) use syntax_view;

#[cfg(any(feature = "rust", feature = "typescript"))]
pub(super) trait CallableView<'tree, L: LanguageExt> {
    fn syntax(&self) -> &Node<'tree, StrDoc<L>>;
    fn anonymous(&self) -> bool;
}

#[cfg(all(test, feature = "rust"))]
mod tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn absent_optional_and_required_fields_have_distinct_results() {
        let tree = Rust.ast_grep("fn f() {}");
        let function = tree
            .root()
            .dfs()
            .find(|node| node.kind() == "function_item")
            .unwrap();
        let field = Field {
            name: "type_parameters",
        };
        assert!(field.optional(&function).unwrap().is_none());
        assert_eq!(
            field.required(&function).err().unwrap(),
            SyntaxError {
                span: function.range().into(),
                field: "type_parameters",
                issue: FieldIssue::Absent,
            }
        );
    }

    #[test]
    fn malformed_nodes_retain_their_location_and_issue() {
        let tree = Rust.ast_grep("fn f() { @ }");
        let missing = tree.root().dfs().find(Node::is_error).unwrap();
        let span = missing.range().into();
        let error = Field { name: "body" }.checked(missing).err().unwrap();
        assert_eq!(error.issue, FieldIssue::Error);
        assert_eq!(error.span, span);
    }

    #[test]
    fn direct_children_do_not_flatten_nested_blocks_or_keep_comments() {
        let tree = Rust.ast_grep("fn f() { // comment\n let outer = 0; { let inner = 1; } }");
        let block = tree
            .root()
            .dfs()
            .find(|node| node.kind() == "block")
            .unwrap();
        let children: Vec<_> =
            DirectChildren::new(block, &["line_comment", "block_comment"]).collect();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0].kind(), "let_declaration");
        assert_eq!(children[1].kind(), "expression_statement");
        assert_eq!(
            children[1].children().find(Node::is_named).unwrap().kind(),
            "block"
        );
    }

    #[test]
    fn casts_recognize_shapes_without_requiring_complete_bodies() {
        let tree = Rust.ast_grep("fn f(value: usize) { let incomplete = ; }");
        let function = tree
            .root()
            .dfs()
            .find(|node| node.kind() == "function_item")
            .unwrap();
        let view = crate::syntax::rust::Function::cast(function).unwrap();
        assert_eq!(view.parameters().unwrap().kind(), "parameters");
        assert!(crate::syntax::rust::Function::cast(tree.root()).is_none());
    }
}
