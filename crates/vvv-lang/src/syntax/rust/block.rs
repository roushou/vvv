use crate::syntax::views::syntax_view;
use ast_grep_core::tree_sitter::LanguageExt;
use vvv_core::Span;

pub(super) struct Block<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! { impl Block { kinds: ["block"], required: {}, optional: {}, children: {statements: ["line_comment", "block_comment"]} } }
impl<'tree, L: LanguageExt> Block<'tree, L> {
    pub fn span(&self) -> Span {
        self.syntax().range().into()
    }
}
