use ast_grep_core::Node;
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc};
use vvv_core::{Highlight, HighlightKind, HighlightRule};

/// Walks a tree and applies a grammar's [`HighlightRule`] table.
pub(crate) struct Highlighter<'r> {
    rules: &'r [HighlightRule],
}

impl<'r> Highlighter<'r> {
    pub(crate) fn new(rules: &'r [HighlightRule]) -> Self {
        Self { rules }
    }

    pub(crate) fn extract<L: LanguageExt>(&self, root: &Node<'_, StrDoc<L>>) -> Vec<Highlight> {
        let mut out = Vec::new();
        self.visit(root, &mut out);
        out
    }

    fn visit<L: LanguageExt>(&self, node: &Node<'_, StrDoc<L>>, out: &mut Vec<Highlight>) {
        if let Some(rule) = self.rule_for(node) {
            out.push(Highlight {
                span: node.range().into(),
                kind: rule.kind,
            });
            return;
        }
        if node.children().len() == 0 {
            if Self::is_keyword(node) {
                out.push(Highlight {
                    span: node.range().into(),
                    kind: HighlightKind::Keyword,
                });
            }
            return;
        }
        for child in node.children() {
            self.visit(&child, out);
        }
    }

    fn rule_for<L: LanguageExt>(&self, node: &Node<'_, StrDoc<L>>) -> Option<&HighlightRule> {
        let kind = node.kind();
        self.rules.iter().filter(|r| r.node == kind).find(|r| {
            r.under
                .is_none_or(|p| node.parent().is_some_and(|parent| parent.kind() == p))
        })
    }

    /// Anonymous nodes made of letters are the grammar's keywords.
    fn is_keyword<L: LanguageExt>(node: &Node<'_, StrDoc<L>>) -> bool {
        !node.is_named() && {
            let text = node.text();
            !text.is_empty() && text.chars().all(|c| c.is_alphabetic() || c == '_')
        }
    }
}
