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

    pub fn new() -> Self {
        AstGrepLanguage::describe(
            Self::ID,
            &["ts", "mts", "cts"],
            ast_grep_language::TypeScript,
            grammar::GRAMMAR,
            &grammar::SEMANTICS,
        )
        .with_layout(TsLayout)
        .with_surgery(TsSurgery)
    }
}

impl Default for TypeScript {
    fn default() -> Self {
        Self::new()
    }
}

impl Tsx {
    pub const ID: LanguageId = LanguageId::new("tsx");

    pub fn new() -> Self {
        AstGrepLanguage::describe(
            Self::ID,
            &["tsx"],
            ast_grep_language::Tsx,
            grammar::GRAMMAR,
            &grammar::SEMANTICS,
        )
        .with_layout(TsLayout)
        .with_surgery(TsSurgery)
    }
}

impl Default for Tsx {
    fn default() -> Self {
        Self::new()
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
        let found = TypeScript::new()
            .find(src, &Query::pattern("interface $N { $$$ }"))
            .unwrap();
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn imports_are_import_statements_and_re_exports_not_exported_declarations() {
        let src = "import { Engine } from './b';\nexport { Engine as E } from './b';\nexport const Engine = 1;\nnew Engine();\n";
        let found = TypeScript::new()
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
        let symbols = TypeScript::new().symbols(src).unwrap();
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
            TypeScript::new().semantics().reach_kind(Some("export")),
            vvv_core::ReachKind::Everyone
        );
    }

    #[test]
    fn extracts_declarations() {
        use SymbolKind::*;
        let got: Vec<(SymbolKind, String)> = TypeScript::new()
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
        let syms = Tsx::new().symbols(src).unwrap();
        assert_eq!(syms[0].name, "App");
        assert_eq!(Tsx::new().references(src, "f").unwrap().len(), 1);
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

    static TS: std::sync::LazyLock<TypeScript> = std::sync::LazyLock::new(TypeScript::new);

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
        let got: Vec<String> = TypeScript::new()
            .imports(src)
            .unwrap()
            .into_iter()
            .map(|i| i.path.to_string())
            .collect();
        assert_eq!(got, ["./a/b", "../t", "./e", "./dyn", "./req", "side"]);
    }
}
