use crate::syntax::navigation::{NavigationCoverage, NavigationFacts};
use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{Facts, Span};

/// Navigation coverage of a block import; mutation import facts stay file-root scoped.
pub(crate) struct Import<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Import {
        kinds: ["use_declaration", "extern_crate_declaration"],
        required: {},
        optional: {
            argument: "argument"
        }
    }
}

impl<'tree, L: LanguageExt> Import<'tree, L> {
    pub fn extract(
        &self,
        shared: &NavigationFacts<'_>,
        facts: &Facts,
        coverage: &mut NavigationCoverage,
    ) {
        for rule in shared
            .grammar
            .bindings
            .iter()
            .filter(|rule| rule.node == self.syntax().kind())
        {
            let Some(scope) = shared.scope(self.syntax(), rule, coverage) else {
                continue;
            };
            let supported = facts.import_scopes.iter().any(|imports| {
                imports.span == Span::from(scope.range())
                    && imports
                        .imports
                        .iter()
                        .any(|binding| Span::from(self.syntax().range()).contains(&binding.span))
            });
            if !supported {
                coverage.block(scope.range().into());
            }
        }
    }
}

/// A path and its explicit alias remain distinct syntax handles.
pub(crate) struct UseAlias<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl UseAlias {
        kinds: ["use_as_clause"],
        required: {
            path: "path",
            alias: "alias"
        },
        optional: {}
    }
}

pub(crate) struct UseGroup<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl UseGroup {
        kinds: ["scoped_use_list"],
        required: {
            list: "list"
        },
        optional: {
            prefix: "path"
        }
    }
}

pub(crate) struct UseList<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl UseList { kinds: ["use_list"], required: {}, optional: {}, children: {entries: []} } }
impl<'tree, L: LanguageExt> UseList<'tree, L> {
    pub fn entry_for(&self, node: &Node<'tree, StrDoc<L>>) -> Option<Node<'tree, StrDoc<L>>> {
        let range = node.range();
        self.syntax()
            .children()
            .find(|child| child.range().start <= range.start && range.end <= child.range().end)
    }
    /// The outer grouped argument has a scope wrapper directly inside its statement.
    pub fn scoped_argument_of(&self, statement: &Node<'tree, StrDoc<L>>) -> bool {
        let Some(group) = self.syntax().parent().and_then(UseGroup::cast) else {
            return false;
        };
        let Some(import) = group.syntax().parent().and_then(Import::cast) else {
            return false;
        };
        import.syntax().range() == statement.range()
            && import
                .argument()
                .ok()
                .flatten()
                .is_some_and(|argument| argument.range() == group.syntax().range())
    }
}

pub(crate) struct UseGlob<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl UseGlob {
        kinds: ["use_wildcard"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> UseGlob<'tree, L> {
    pub fn prefix(&self) -> Option<Node<'tree, StrDoc<L>>> {
        self.syntax().children().find(|child| {
            child.is_named() && !matches!(child.kind().as_ref(), "line_comment" | "block_comment")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn grouped_entries_keep_prefix_alias_and_glob_handles_distinct() {
        let tree = Rust.ast_grep("use crate::outer::{inner::{Thing as Local, other::*}, Value};");
        let root = tree.root();
        let groups: Vec<_> = root.dfs().filter_map(UseGroup::cast).collect();
        assert_eq!(groups[0].prefix().unwrap().unwrap().text(), "crate::outer");
        assert_eq!(groups[0].list().unwrap().kind(), "use_list");
        assert_eq!(groups[1].prefix().unwrap().unwrap().text(), "inner");
        let alias = root.dfs().find_map(UseAlias::cast).unwrap();
        assert_eq!(alias.path().unwrap().text(), "Thing");
        assert_eq!(alias.alias().unwrap().text(), "Local");
        let list = UseList::cast(groups[1].list().unwrap()).unwrap();
        assert_eq!(list.entries().count(), 2);
        assert_eq!(
            list.entry_for(alias.syntax()).unwrap().text(),
            "Thing as Local"
        );
        let glob = root.dfs().find_map(UseGlob::cast).unwrap();
        assert_eq!(glob.prefix().unwrap().text(), "other");
        let bare = Rust.ast_grep("use crate::outer::{*};");
        assert!(
            bare.root()
                .dfs()
                .find_map(UseGlob::cast)
                .unwrap()
                .prefix()
                .is_none()
        );
    }

    #[test]
    fn typed_import_lowering_preserves_table_facts_and_unfinished_spans() {
        let language = crate::rust::Rust::default();
        let typed = language.searcher();
        let tables = typed
            .clone()
            .with_navigation_syntax(crate::syntax::navigation::NavigationSyntax::Tables);
        for source in [
            "pub(crate) use crate::a::{b::{C as D, e::*}, F}; mod nested { pub(super) use super::F; }",
            "pub(in crate::a) mod inner { pub struct S; mod file; use self::S as Local; }",
            "use crate::{a, /* comment */ b::*}; use {c, d};",
            "use crate::a::{b as };",
            "use crate::a::{b,",
            "mod incomplete { use crate::",
        ] {
            let actual = typed.facts(source).unwrap();
            let expected = tables.facts(source).unwrap();
            assert_eq!(
                format!("{:?}", actual.imports),
                format!("{:?}", expected.imports),
                "{source}"
            );
            assert_eq!(
                format!("{:?}", actual.import_bindings),
                format!("{:?}", expected.import_bindings),
                "{source}"
            );
            assert_eq!(
                format!("{:?}", actual.module_scopes),
                format!("{:?}", expected.module_scopes),
                "{source}"
            );
            assert_eq!(
                format!("{:?}", actual.import_scopes),
                format!("{:?}", expected.import_scopes),
                "{source}"
            );
        }
    }
}
