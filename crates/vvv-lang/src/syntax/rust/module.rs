use crate::syntax::views::syntax_view;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{Grammar, Name, Span};

/// File-backed and inline declarations have distinct body presence.
pub(crate) struct Module<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Module {
        kinds: ["mod_item"],
        required: {
            name: "name"
        },
        optional: {
            body: "body"
        }
    }
}

pub(crate) struct ModuleBody<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! { impl ModuleBody { kinds: ["source_file", "declaration_list"], required: {}, optional: {}, children: {items: []} } }

pub(crate) struct Visibility<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
}

syntax_view! {
    impl Visibility {
        kinds: ["visibility_modifier"],
        required: {},
        optional: {}
    }
}

impl<'tree, L: LanguageExt> Visibility<'tree, L> {
    pub fn of(node: &Node<'tree, StrDoc<L>>) -> Option<Self> {
        node.children().find_map(Self::cast)
    }

    pub fn restriction(&self) -> Option<Node<'tree, StrDoc<L>>> {
        self.syntax().children().find(Node::is_named)
    }
}

/// Navigation ownership interpretation retains no mutation address.
pub(crate) struct ModuleOwner<'tree, L: LanguageExt> {
    pub body: Node<'tree, StrDoc<L>>,
    pub declaration: Span,
    pub path: Vec<Name>,
}

impl<'tree, L: LanguageExt> ModuleOwner<'tree, L> {
    pub fn from_module(module: &Module<'tree, L>, grammar: &Grammar) -> Option<Self> {
        if module.syntax().ancestors().any(|parent| {
            Module::cast(parent.clone()).is_none()
                && (grammar.lexical_boundaries.contains(&parent.kind().as_ref())
                    || grammar.lexical_barriers.contains(&parent.kind().as_ref()))
        }) {
            return None;
        }
        // Keep captured unfinished-tree spans; coverage is interpreted separately.
        let body = module
            .body()
            .ok()
            .flatten()
            .or_else(|| module.syntax().field("body"))?;
        let name = module
            .name()
            .ok()
            .or_else(|| module.syntax().field("name"))?;
        let mut path: Vec<Name> = module
            .syntax()
            .ancestors()
            .filter_map(Module::cast)
            .filter_map(|parent| parent.name().ok().or_else(|| parent.syntax().field("name")))
            .map(|name| name.text().into_owned().into())
            .collect();
        path.reverse();
        path.push(name.text().into_owned().into());
        Some(Self {
            body,
            declaration: name.range().into(),
            path,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn module_bodies_and_visibility_keep_language_shapes() {
        let tree = Rust.ast_grep("mod file; pub(crate) mod inline { pub(super) struct S; pub(in crate::inline) mod nested {} }");
        let root = tree.root();
        let modules: Vec<_> = root.dfs().filter_map(Module::cast).collect();
        assert!(modules[0].body().unwrap().is_none());
        assert_eq!(modules[1].name().unwrap().text(), "inline");
        let body = ModuleBody::cast(modules[1].body().unwrap().unwrap()).unwrap();
        assert_eq!(body.items().count(), 2);
        assert_eq!(
            Visibility::of(modules[1].syntax())
                .unwrap()
                .restriction()
                .unwrap()
                .text(),
            "crate"
        );
        assert_eq!(
            Visibility::of(modules[2].syntax())
                .unwrap()
                .restriction()
                .unwrap()
                .text(),
            "crate::inline"
        );
        let public = Rust.ast_grep("pub mod plain {}");
        assert!(
            Visibility::of(&public.root().dfs().find_map(Module::cast).unwrap().node)
                .unwrap()
                .restriction()
                .is_none()
        );
    }
}
