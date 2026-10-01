//! Shared declaration questions retain language-specific structural shapes.

use super::navigation::NavigationSyntax;
use ast_grep_core::{
    Node,
    tree_sitter::{LanguageExt, StrDoc},
};
use vvv_core::{ModifierAt, SymbolRule};

pub(super) struct Declaration<'tree, L: LanguageExt> {
    node: Node<'tree, StrDoc<L>>,
    shape: Shape<'tree, L>,
}

enum Shape<'tree, L: LanguageExt> {
    #[cfg(feature = "rust")]
    Rust(super::rust::Declaration<'tree, L>),
    #[cfg(feature = "typescript")]
    TypeScript(super::typescript::Declaration<'tree, L>),
    Other(Node<'tree, StrDoc<L>>),
}

impl<'tree, L: LanguageExt> Declaration<'tree, L> {
    pub fn new(node: Node<'tree, StrDoc<L>>, syntax: NavigationSyntax) -> Self {
        let shape = match syntax {
            #[cfg(feature = "rust")]
            NavigationSyntax::Rust => super::rust::Declaration::cast(node.clone()).map(Shape::Rust),
            #[cfg(feature = "typescript")]
            NavigationSyntax::TypeScript => {
                super::typescript::Declaration::cast(node.clone()).map(Shape::TypeScript)
            }
            _ => None,
        }
        .unwrap_or_else(|| Shape::Other(node.clone()));
        Self { node, shape }
    }

    pub fn field(&self, field: &str) -> Option<Node<'tree, StrDoc<L>>> {
        let captured = match &self.shape {
            #[cfg(feature = "rust")]
            Shape::Rust(view) => view.field(field).and_then(Result::ok),
            #[cfg(feature = "typescript")]
            Shape::TypeScript(view) => view.field(field).and_then(Result::ok),
            Shape::Other(node) => Some(node.field(field)),
        };
        // Rules may name custom fields. Missing/error captures keep their spans;
        // structural completeness and navigation coverage are separate questions.
        captured.unwrap_or_else(|| self.node.field(field))
    }

    pub fn name(&self, rule: &SymbolRule) -> Option<Node<'tree, StrDoc<L>>> {
        let mut name = match rule.name_field {
            Some(field) => self.field(field)?,
            None => self.node.clone(),
        };
        if let Some(inner) = rule.name_inner {
            while let Some(child) = name.field(inner) {
                name = child;
            }
        }
        Some(name)
    }

    pub fn shadows(&self, name: &str) -> bool {
        self.field("type_parameters").is_some_and(|parameters| {
            parameters.dfs().any(|parameter| {
                parameter
                    .field("name")
                    .is_some_and(|node| node.text() == name)
            })
        })
    }

    pub fn modifier(&self, at: ModifierAt) -> Option<Node<'tree, StrDoc<L>>> {
        match at {
            ModifierAt::Child(kind) => {
                #[cfg(feature = "rust")]
                if matches!(&self.shape, Shape::Rust(_)) && kind == "visibility_modifier" {
                    return super::rust::Visibility::of(&self.node)
                        .map(|view| view.syntax().clone());
                }
                self.node.children().find(|child| child.kind() == kind)
            }
            ModifierAt::Parent(kind) => self
                .node
                .ancestors()
                .take(2)
                .find(|parent| parent.kind() == kind),
        }
    }
}

#[cfg(all(test, feature = "rust"))]
mod rust_tests {
    use super::*;
    use ast_grep_language::Rust;

    #[test]
    fn rule_overrides_and_generic_impl_targets_keep_exact_handles() {
        let tree = Rust.ast_grep("pub fn f() -> u8 { 1 } impl<T> crate::S<T> {}");
        let root = tree.root();
        let function = root
            .dfs()
            .find(|node| node.kind() == "function_item")
            .unwrap();
        let declaration = Declaration::new(function.clone(), NavigationSyntax::Rust);
        let custom = SymbolRule::new(
            "function_item",
            "return_type",
            vvv_core::SymbolKind::Function,
        );
        assert_eq!(declaration.name(&custom).unwrap().text(), "u8");
        assert_eq!(
            declaration.field("parameters").unwrap().range(),
            function.field("parameters").unwrap().range()
        );
        let implementation = root.dfs().find(|node| node.kind() == "impl_item").unwrap();
        let declaration = Declaration::new(implementation, NavigationSyntax::Rust);
        let rule =
            SymbolRule::new("impl_item", "type", vvv_core::SymbolKind::Impl).name_inner("type");
        assert_eq!(declaration.name(&rule).unwrap().text(), "crate::S");
        assert!(declaration.shadows("T"));
        assert!(!declaration.shadows("S"));
    }

    #[test]
    fn rust_declaration_contract_matches_tables_for_wrappers_and_unfinished_trees() {
        let language = crate::rust::Rust::new();
        let typed = language.searcher();
        let tables = typed
            .clone()
            .with_navigation_syntax(NavigationSyntax::Tables);
        for source in [
            "/// docs\r\n#[repr(C)]\r\npub(crate) struct S<T>(T); impl<T> S<T> {} impl<S> Trait for S {}",
            "pub struct Unit; pub struct Tuple(u8); pub union U { x: u8 } pub enum E { A, B(u8) }",
            "pub trait T { fn required(&self); } pub mod file; mod inline {} type Alias = [u8; {2}];",
            "#[cfg(a)] struct S; #[cfg(b)] struct S; impl S {} mod child { impl super::S {} }",
            "/// detached\n\nfn f() {}",
            "pub struct Missing {",
            "fn incomplete(",
            "impl<T> Missing<T> {",
        ] {
            let actual = typed.facts(source).unwrap();
            let expected = tables.facts(source).unwrap();
            assert_eq!(actual.symbols, expected.symbols, "{source}");
            assert_eq!(actual.signatures, expected.signatures, "{source}");
            assert_eq!(
                actual.declaration_pieces, expected.declaration_pieces,
                "{source}"
            );
        }
    }
}
#[cfg(all(test, feature = "typescript"))]
mod typescript_tests {
    use super::*;

    struct DeclarationCases {
        sources: &'static [&'static str],
    }

    impl DeclarationCases {
        fn verify<L: LanguageExt>(&self, typed: &crate::syntax::AstGrepSearcher<L>) {
            let tables = typed
                .clone()
                .with_navigation_syntax(NavigationSyntax::Tables);
            for source in self.sources {
                let actual = typed.facts(source).unwrap();
                let expected = tables.facts(source).unwrap();
                assert_eq!(actual.symbols, expected.symbols, "{source}");
                assert_eq!(actual.signatures, expected.signatures, "{source}");
                assert_eq!(
                    actual.declaration_pieces, expected.declaration_pieces,
                    "{source}"
                );
            }
        }
    }
    #[test]
    fn ts_and_tsx_declaration_contracts_preserve_export_and_ambient_extents() {
        let cases = DeclarationCases {
            sources: &[
                "/** docs */ export default class C { method(): number { return 1; } }",
                "export declare function f(value: number): number; declare class Ambient { value: number; }",
                "export abstract class C { abstract method(): void; } interface I { field: number; method(): void; }",
                "export enum E { A, B = 2 } export type Alias = { value: number }; const { x, y: z } = input;",
                "export class Incomplete {",
                "export function unfinished(",
            ],
        };
        cases.verify(crate::typescript::TypeScript::new().searcher());
        cases.verify(crate::typescript::Tsx::new().searcher());
    }
}
