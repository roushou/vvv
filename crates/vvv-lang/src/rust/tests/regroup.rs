//! `regroup` on one statement at a time. The workspace is what a move of
//! `crate::util::parse` to `crate::net::parse` sees before it happens.

use std::path::Path;

use vvv_core::{
    Address, ChangeSet, GroupedImport, GroupedImports, Language, RegroupedOutcome, SourceText,
};

use super::*;
use crate::syntax::fixture::Fixture;

const OLD: &str = "crate::util::parse";
const NEW: &str = "crate::net::parse";

struct RegroupCase<'a> {
    file: &'a str,
    src: &'a str,
}

impl<'a> RegroupCase<'a> {
    fn new(file: &'a str, src: &'a str) -> Self {
        Self { file, src }
    }

    fn fixture<'l>(&self, lang: &'l Rust) -> Fixture<'l> {
        Fixture::new(
            lang,
            &[
                ("Cargo.toml", "[package]\nname = \"fixture\"\n"),
                ("src/lib.rs", "pub mod util;\npub mod net;\n"),
                ("src/util.rs", "pub mod parse;\npub mod strings;\n"),
                ("src/util/parse.rs", ""),
                ("src/util/strings.rs", ""),
                ("src/net.rs", "pub mod client;\n"),
                ("src/net/client.rs", ""),
            ],
        )
    }

    /// Apply `regroup` to every grouped entry of `src` (as seen from `file`)
    /// whose resolved address is under OLD, rebased onto NEW.
    fn render(&self) -> String {
        let (file, src) = (self.file, self.src);
        let lang = Rust::default();
        let fx = self.fixture(&lang);
        let file = Path::new(file);
        let old = Address::new("fixture", OLD.trim_start_matches("crate::").split("::"));
        let new = Address::new("fixture", NEW.trim_start_matches("crate::").split("::"));
        let entries: Vec<GroupedImport> = lang
            .imports(src)
            .unwrap()
            .into_iter()
            .filter(|r| r.group.is_some())
            .filter_map(|r| {
                let resolved = fx.resolve_path(file, &r.path)?;
                let prefix = &r.group.as_ref().unwrap().prefix;
                let prefix_under_old = fx
                    .resolve_path(file, prefix)
                    .is_some_and(|p| p.starts_with(&old));
                if prefix_under_old {
                    return None;
                }
                let target = resolved.rebase(&old, &new)?;
                let prefix = fx.resolve_path(file, prefix);
                Some(GroupedImport {
                    import: r,
                    target,
                    prefix,
                })
            })
            .collect();
        let source = SourceText::new(src);
        let imports = GroupedImports::new(entries).unwrap();
        let out = fx.regroup(file, &source, &imports);
        assert!(
            out.validate(&imports)
                .unwrap()
                .iter()
                .all(|(_, o)| !matches!(o, RegroupedOutcome::Skipped))
        );
        let mut cs = ChangeSet::new();
        for e in out.edits {
            cs.insert("f.rs", e).unwrap();
        }
        cs.apply_to(Path::new("f.rs"), src)
    }
}

#[test]
fn entry_stays_in_group_when_prefix_still_covers_it() {
    assert_eq!(
        RegroupCase::new(
            "src/lib.rs",
            "use crate::{util::parse::parse_line, net::Client};\n"
        )
        .render(),
        "use crate::{net::parse::parse_line, net::Client};\n"
    );
}

#[test]
fn entry_leaves_group_into_its_own_statement() {
    assert_eq!(
        RegroupCase::new("src/lib.rs", "use crate::util::{parse::Config, strings};\n").render(),
        "use crate::util::{strings};\nuse crate::net::parse::Config;\n"
    );
}

#[test]
fn alias_wildcard_and_nested_tails_are_kept() {
    assert_eq!(
        RegroupCase::new(
            "src/lib.rs",
            "use crate::util::{parse::Config as Cfg, strings};\n"
        )
        .render(),
        "use crate::util::{strings};\nuse crate::net::parse::Config as Cfg;\n"
    );
    assert_eq!(
        RegroupCase::new("src/lib.rs", "use crate::util::{strings, parse::*};\n").render(),
        "use crate::util::{strings};\nuse crate::net::parse::*;\n"
    );
    assert_eq!(
        RegroupCase::new(
            "src/lib.rs",
            "use crate::util::{strings, parse::{self, Config}};\n"
        )
        .render(),
        "use crate::util::{strings};\nuse crate::net::parse::{self, Config};\n"
    );
}

#[test]
fn whole_statement_is_replaced_when_every_entry_leaves() {
    assert_eq!(
        RegroupCase::new(
            "src/net.rs",
            "    pub(crate) use crate::util::{parse::*, parse::Config};\n"
        )
        .render(),
        "    pub(crate) use crate::net::parse::*;\n    pub(crate) use crate::net::parse::Config;\n"
    );
}

#[test]
fn multi_line_groups_lose_whole_lines() {
    let src =
        "use crate::util::{\n    strings::trim,\n    parse::Config,\n    parse::parse_line,\n};\n";
    assert_eq!(
        RegroupCase::new("src/net/client.rs", src).render(),
        "use crate::util::{\n    strings::trim,\n};\nuse crate::net::parse::Config;\nuse crate::net::parse::parse_line;\n"
    );
}

#[test]
fn indentation_and_visibility_follow_the_statement() {
    assert_eq!(
        RegroupCase::new(
            "src/lib.rs",
            "fn f() {\n    pub use crate::util::{parse::Config, strings};\n}\n"
        )
        .render(),
        "fn f() {\n    pub use crate::util::{strings};\n    pub use crate::net::parse::Config;\n}\n"
    );
}
