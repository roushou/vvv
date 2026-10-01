use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{CallKind, Grammar};

pub(crate) struct Call<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! {
    impl Call {
        kinds: ["call_expression"],
        required: {
            function: "function"
        },
        optional: {}
    }
}

pub(super) struct MemberAccess<'tree, L: ast_grep_core::tree_sitter::LanguageExt> {
    node: ast_grep_core::Node<'tree, ast_grep_core::tree_sitter::StrDoc<L>>,
}

syntax_view! {
    impl MemberAccess {
        kinds: ["field_expression"],
        required: {
            receiver: "value",
            member: "field"
        },
        optional: {}
    }
}

impl<'tree, L: LanguageExt> Call<'tree, L> {
    pub fn callee(&self, grammar: &Grammar) -> Option<(Node<'tree, StrDoc<L>>, CallKind)> {
        let mut callee = self.function().ok()?;
        let mut kind = CallKind::Direct;
        loop {
            if let Some(member) = MemberAccess::cast(callee.clone()) {
                debug_assert_eq!(member.syntax().range(), callee.range());
                // Reading the receiver is structural; no type inference occurs here.
                let _receiver = member.receiver().ok();
                let Ok(child) = member.member() else {
                    break;
                };
                callee = child;
                kind = CallKind::Member;
                continue;
            }
            let (field, classification) = match callee.kind().as_ref() {
                "generic_function" => ("function", None),
                "scoped_identifier" => ("name", Some(CallKind::Direct)),
                _ => break,
            };
            let Some(child) = callee.field(field) else {
                break;
            };
            callee = child;
            if let Some(classification) = classification {
                kind = classification;
            }
        }
        if !grammar.identifiers.contains(&callee.kind().as_ref()) {
            kind = CallKind::Indirect;
        }
        Some((callee, kind))
    }
}
