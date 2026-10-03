//! TypeScript support. `.ts` and `.tsx` are distinct grammars, so they are
//! distinct plugins sharing one grammar description.

mod grammar;
mod layout;
mod surgery;

use vvv_core::LanguageId;

use crate::syntax::AstGrepLanguage;

pub use layout::TsLayout;
pub use surgery::TsSurgery;

pub type TypeScript = AstGrepLanguage<ast_grep_language::TypeScript>;
pub type Tsx = AstGrepLanguage<ast_grep_language::Tsx>;

impl TypeScript {
    pub const ID: LanguageId = LanguageId::new("typescript");
}

impl Default for TypeScript {
    fn default() -> Self {
        AstGrepLanguage::new(
            Self::ID,
            &["ts", "mts", "cts"],
            ast_grep_language::TypeScript,
            grammar::GRAMMAR,
            &grammar::SEMANTICS,
        )
        .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript)
        .with_layout(TsLayout)
        .with_surgery(TsSurgery)
    }
}

impl Tsx {
    pub const ID: LanguageId = LanguageId::new("tsx");
}

impl Default for Tsx {
    fn default() -> Self {
        AstGrepLanguage::new(
            Self::ID,
            &["tsx"],
            ast_grep_language::Tsx,
            grammar::GRAMMAR,
            &grammar::SEMANTICS,
        )
        .with_reference_group(TypeScript::ID)
        .with_navigation_syntax(crate::syntax::NavigationSyntax::TypeScript)
        .with_layout(TsLayout)
        .with_surgery(TsSurgery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vvv_core::{Language, Query, Role, SymbolKind};

    const DECLS: &str = r#"
export function f() {}
export class C { x = 1; m() {} }
interface I { y: number; n(): void }
type T = string;
enum E { A, B }
const arrow = () => {};
"#;

    #[test]
    fn finds_interfaces() {
        let src = "interface A { x: number }\nfunction f() {}\ninterface B {}\n";
        let found = TypeScript::default()
            .find(src, &Query::pattern("interface $N { $$$ }"))
            .unwrap();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn imports_are_import_statements_and_re_exports_not_exported_declarations() {
        let src = "import { Engine } from './b';\nexport { Engine as E } from './b';\nexport const Engine = 1;\nnew Engine();\n";
        let found = TypeScript::default()
            .find(src, &Query::pattern("Engine"))
            .unwrap();
        let roles: Vec<Role> = found.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [Role::Import, Role::Import, Role::Declaration, Role::Use]
        );
    }

    #[test]
    fn export_is_the_modifier_and_joins_the_extent() {
        let src = "/** Doc */\nexport class A {}\nexport default function f() {}\nexport const k = 1, j = 2;\nclass B {}\n";
        let symbols = TypeScript::default().symbols(src).unwrap();
        let view: Vec<(String, Option<String>, String)> = symbols
            .iter()
            .map(|s| {
                (
                    s.name.clone(),
                    s.modifier().map(str::to_owned),
                    src[s.extent.start..s.extent.end].to_owned(),
                )
            })
            .collect();
        assert_eq!(
            view,
            [
                (
                    "A".to_owned(),
                    Some("export".to_owned()),
                    "/** Doc */\nexport class A {}".to_owned()
                ),
                (
                    "f".to_owned(),
                    Some("export default".to_owned()),
                    "export default function f() {}".to_owned()
                ),
                (
                    "k".to_owned(),
                    Some("export".to_owned()),
                    "export const k = 1, j = 2;".to_owned()
                ),
                (
                    "j".to_owned(),
                    Some("export".to_owned()),
                    "export const k = 1, j = 2;".to_owned()
                ),
                ("B".to_owned(), None, "class B {}".to_owned()),
            ]
        );
        assert_eq!(
            TypeScript::default().semantics().reach_kind(Some("export")),
            vvv_core::ReachKind::Everyone
        );
    }

    #[test]
    fn extracts_declarations() {
        use SymbolKind::*;
        let got: Vec<(SymbolKind, String)> = TypeScript::default()
            .symbols(DECLS)
            .unwrap()
            .into_iter()
            .map(|s| (s.kind, s.name))
            .collect();
        let expected = [
            (Function, "f"),
            (Class, "C"),
            (Field, "x"),
            (Method, "m"),
            (Interface, "I"),
            (Field, "y"),
            (Method, "n"),
            (TypeAlias, "T"),
            (Enum, "E"),
            (Variant, "A"),
            (Variant, "B"),
            (Variable, "arrow"),
        ];
        let got: Vec<(SymbolKind, &str)> = got.iter().map(|(k, n)| (*k, n.as_str())).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn tsx_shares_the_rules() {
        let src = "function App() { return <div>{f()}</div>; }";
        let syms = Tsx::default().symbols(src).unwrap();
        assert_eq!(syms[0].name, "App");
        assert_eq!(Tsx::default().references(src, "f").unwrap().len(), 1);
    }
}

#[cfg(test)]
mod resolver_tests {
    use std::path::Path;

    use vvv_core::{Address, Language};

    use super::*;
    use crate::syntax::fixture::Fixture;

    fn addr(s: &str) -> Address {
        Address::new("", s.split('/'))
    }

    static TS: std::sync::LazyLock<TypeScript> = std::sync::LazyLock::new(TypeScript::default);

    fn resolver() -> Fixture<'static> {
        Fixture::new(
            &*TS,
            &[
                ("src/a/b.ts", ""),
                ("src/a/index.ts", ""),
                ("src/c.tsx", ""),
                ("src/main.ts", ""),
            ],
        )
    }

    #[test]
    fn resolves_extensions_and_index() {
        let r = resolver();
        let main = Path::new("src/main.ts");
        assert_eq!(r.resolve(main, "./a/b"), Some(addr("src/a/b.ts")));
        assert_eq!(r.resolve(main, "./a/b.js"), Some(addr("src/a/b.ts")));
        assert_eq!(r.resolve(main, "./a"), Some(addr("src/a/index.ts")));
        assert_eq!(r.resolve(main, "./c"), Some(addr("src/c.tsx")));
        assert_eq!(
            r.resolve(Path::new("src/a/b.ts"), "../c"),
            Some(addr("src/c.tsx"))
        );
        assert_eq!(r.resolve(main, "./nope"), None);
        assert_eq!(r.resolve(main, "react"), None);
    }

    #[test]
    fn render_preserves_style() {
        let r = resolver();
        let main = Path::new("src/main.ts");
        assert_eq!(r.render(main, &addr("src/x/b.ts"), "./a/b"), "./x/b");
        assert_eq!(r.render(main, &addr("src/x/b.ts"), "./a/b.js"), "./x/b.js");
        assert_eq!(r.render(main, &addr("src/x/index.ts"), "./a"), "./x");
        assert_eq!(
            r.render(main, &addr("src/x/index.ts"), "./a/index"),
            "./x/index"
        );
        assert_eq!(
            r.render(
                Path::new("src/deep/dir/f.ts"),
                &addr("src/c.tsx"),
                "../../c"
            ),
            "../../c"
        );
        assert_eq!(
            r.render(Path::new("lib/f.ts"), &addr("src/c.tsx"), "./c"),
            "../src/c"
        );
        assert_eq!(
            r.render(Path::new("src/a/p/f.ts"), &addr("src/a/index.ts"), "../a"),
            ".."
        );
        assert_eq!(
            r.render(Path::new("src/a/f.ts"), &addr("src/a/index.ts"), "./index"),
            "./index"
        );
    }

    #[test]
    fn imports_cover_static_dynamic_and_reexports() {
        let src = "import { a } from './a/b';\nimport type T from '../t';\nexport * from './e';\nconst m = import('./dyn');\nconst r = require('./req');\nimport 'side';";
        let got: Vec<String> = TypeScript::default()
            .imports(src)
            .unwrap()
            .into_iter()
            .map(|i| i.path.to_string())
            .collect();
        assert_eq!(got, ["./a/b", "../t", "./e", "./dyn", "./req", "side"]);
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    use vvv_core::Language;

    #[test]
    fn named_imports_record_exact_bound_names() {
        let source = "import { Other } from './origin'; type Alias = Engine;";
        let facts = TypeScript::default().facts(source).unwrap();
        assert_eq!(facts.named_imports.len(), 1);
        assert_eq!(facts.named_imports[0].local, "Other");
        assert_eq!(facts.named_imports[0].imported, "Other");
        assert!(facts.named_modules);
    }
}

#[cfg(test)]
mod lexical_navigation_tests {
    use super::*;
    use vvv_core::Language;
    #[test]
    fn aliases_generics_and_parameters_are_lowered_without_changing_declarations() {
        let source = "import type { Engine as Runtime } from './origin'; function f<Runtime>(value: Runtime): Runtime { return value; }";
        let facts = TypeScript::default().facts(source).unwrap();
        let binding = &facts.named_imports[0];
        assert_eq!((&*binding.local, &*binding.imported), ("Runtime", "Engine"));
        assert!(binding.type_only);
        assert_eq!(
            facts
                .lexical
                .iter()
                .map(|b| b.symbol.name.as_str())
                .collect::<Vec<_>>(),
            ["f", "Runtime", "value"]
        );
    }
    #[test]
    fn parameter_var_redeclarations_retain_body_navigation_and_both_source_sites() {
        let source = "function f(value: number) { if (true) { var value = 2; } return value; }";
        let facts = TypeScript::default().facts(source).unwrap();
        assert!(
            facts
                .lexical_tokens
                .contains(&facts.tokens_named("value").last().unwrap().0)
        );
    }
    #[test]
    fn nested_signature_bindings_do_not_leak_and_type_aliases_preserve_outer_bindings() {
        let source =
            "function f<T>(value: T) { type Local = <U>(x: U) => U; let other: U; return value; }";
        let facts = TypeScript::default().facts(source).unwrap();
        let outer_use = facts.tokens_named("U").last().unwrap().0;
        let generic = facts
            .lexical
            .iter()
            .find(|binding| binding.symbol.name == "U")
            .unwrap();
        assert!(!generic.scope.contains(&outer_use));
        assert!(
            facts
                .lexical_tokens
                .contains(&facts.tokens_named("value").last().unwrap().0)
        );
        let facts = TypeScript::default()
            .facts("export default class Engine {} export class Named {}")
            .unwrap();
        assert_eq!(
            facts.non_named_exports,
            [facts.tokens_named("Engine").next().unwrap().0]
        );
    }
}

#[cfg(test)]
mod call_tests {
    use super::*;
    use vvv_core::{CallKind, Language};
    #[test]
    fn call_facts_distinguish_dispatch_and_anonymous_ownership() {
        let source = "function outer() { work<number>(); api.work(); obj[method](); factory()(); const cb = () => work(); function nested() { work(); } }";
        let facts = TypeScript::default().facts(source).unwrap();
        assert!(facts.calls_supported);
        let outer = facts.symbols.iter().find(|s| s.name == "outer").unwrap();
        let nested = facts.symbols.iter().find(|s| s.name == "nested").unwrap();
        let calls = &facts.calls;
        assert!(
            calls
                .iter()
                .any(|c| c.kind == CallKind::Direct && c.owner == Some(outer.name_span))
        );
        assert!(calls.iter().any(|c| c.kind == CallKind::Member));
        assert!(calls.iter().any(|c| c.kind == CallKind::Indirect));
        let work: Vec<_> = calls
            .iter()
            .filter(|c| &source[c.callee.start..c.callee.end] == "work")
            .collect();
        assert!(work.iter().any(|c| c.owner.is_none()));
        assert!(work.iter().any(|c| c.owner == Some(nested.name_span)));
        assert!(calls.iter().all(|c| c.span.contains(&c.callee)));
    }
}

#[cfg(test)]
mod signature_tests {
    use super::{Tsx, TypeScript};
    use vvv_core::Language;

    #[test]
    fn signatures_preserve_export_docs_and_object_types_in_both_grammars() {
        let cases = [
            (
                "/** café */\r\nexport function build<T extends { x: number }>(x: T): { value: T }",
                " { return { value: x }; }",
                "build",
            ),
            (
                "export class Worker<T>",
                " { work(x: T): { value: T } { return { value: x }; } }",
                "Worker",
            ),
            (
                "export interface Work<T>",
                " { work(x: T): { value: T }; }",
                "Work",
            ),
            ("export type Alias = { value: number };", "", "Alias"),
        ];
        for (header, body, name) in cases {
            let source = format!("{header}{body}");
            for facts in [
                TypeScript::default().facts(&source).unwrap(),
                Tsx::default().facts(&source).unwrap(),
            ] {
                let symbol = facts.symbols.iter().find(|s| s.name == name).unwrap();
                let signature = facts
                    .signatures
                    .iter()
                    .find(|s| s.name_span == symbol.name_span)
                    .unwrap();
                assert_eq!(
                    &source[signature.span.start..signature.span.end],
                    header,
                    "{name}"
                );
            }
        }
        let source = "class Worker { /** doc */ work(x: number): { value: number } { return { value: x }; } }\nexport const factory = () => 1;";
        let facts = TypeScript::default().facts(source).unwrap();
        let method = facts.symbols.iter().find(|s| s.name == "work").unwrap();
        let signature = facts
            .signatures
            .iter()
            .find(|s| s.name_span == method.name_span)
            .unwrap();
        assert_eq!(
            &source[signature.span.start..signature.span.end],
            "/** doc */ work(x: number): { value: number }"
        );
        let variable = facts.symbols.iter().find(|s| s.name == "factory").unwrap();
        let signature = facts
            .signatures
            .iter()
            .find(|signature| signature.name_span == variable.name_span)
            .unwrap();
        assert_eq!(
            &source[signature.span.start..signature.span.end],
            "export const factory = () =>"
        );
    }
}

#[cfg(test)]
mod move_pieces_tests {
    use super::TypeScript;
    use vvv_core::{Language, SymbolKind};
    #[test]
    fn exported_wrapper_and_docs_belong_to_one_declaration() {
        let source = "/** owned docs */\nexport class Widget { method() {} }\nfunction outer() { class Widget {} }";
        let facts = TypeScript::default().facts(source).unwrap();
        let declarations: Vec<_> = facts
            .symbols
            .iter()
            .filter(|symbol| symbol.name == "Widget" && symbol.kind == SymbolKind::Class)
            .collect();
        assert_eq!(declarations.len(), 2);
        let root = facts
            .declaration_pieces
            .iter()
            .find(|pieces| pieces.declaration == declarations[0].span)
            .unwrap();
        assert!(root.top_level);
        assert!(root.companions.is_empty());
        assert!(
            source[declarations[0].extent.start..declarations[0].extent.end]
                .starts_with("/** owned docs */")
        );
        assert!(
            !facts
                .declaration_pieces
                .iter()
                .find(|pieces| pieces.declaration == declarations[1].span)
                .unwrap()
                .top_level
        );
    }
}

#[cfg(test)]
mod move_overload_tests {
    use super::TypeScript;
    use vvv_core::Language;
    #[test]
    fn overload_signatures_remain_distinct_root_declarations() {
        let source = "export function f(x: string): string;\nexport function f(x: number): number;\nexport function f(x: string | number) { return x; }";
        let facts = TypeScript::default().facts(source).unwrap();
        let declarations: Vec<_> = facts
            .symbols
            .iter()
            .filter(|symbol| symbol.name == "f")
            .collect();
        assert_eq!(declarations.len(), 3);
        assert!(declarations.iter().all(|symbol| {
            facts
                .declaration_pieces
                .iter()
                .any(|pieces| pieces.declaration == symbol.span && pieces.top_level)
        }));
        let ambient = TypeScript::default()
            .facts("export declare function f(): void;")
            .unwrap();
        assert_eq!(ambient.symbols.len(), 1);
        assert!(ambient.declaration_pieces[0].top_level);
        assert!(!ambient.declaration_pieces[0].supported);
        assert_eq!(
            facts
                .declaration_pieces
                .iter()
                .filter(|pieces| !pieces.supported)
                .count(),
            2
        );
    }
}
