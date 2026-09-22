//! Import references as data, and the declarative rules that find them.
//!
//! A language names other files or modules with paths: `crate::foo::bar` in
//! Rust, `'./foo/bar'` in TypeScript. [`ImportRef`] is one such path in one
//! file; [`ImportRule`]s tell the generic extractor in `vvv-lang` where the
//! grammar puts them. Resolving what a path *means* is the job of a
//! [`Layout`](crate::resolve::Layout).

use serde::{Deserialize, Serialize};

use crate::paths::{ModulePath, Name, PathSyntax};
use crate::text::Span;

/// One path in one file. `span` covers exactly the text a rewrite replaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportRef {
    pub span: Span,
    /// The full path as written (`crate::a::b`, `../x/y`), parsed by the
    /// grammar's [`PathSyntax`]. For a grouped entry this includes the
    /// group's prefix.
    pub path: ModulePath,
    /// Present when the path is an entry of a grouped import such as
    /// `use a::{b, c::d}`; replacing `span` alone can then only express
    /// paths that stay under the group's prefix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<ImportGroup>,
    /// `use a::b::*`: every name under the path is brought into scope.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub glob: bool,
    /// Part of an import statement (`use`, `import`), so it brings its last
    /// segment into the file's scope. A qualified path in an expression
    /// (`fff::GrepMatch` in a type position) is a reference to rewrite on a
    /// move, but declares nothing.
    #[serde(default = "yes", skip_serializing_if = "std::ops::Not::not")]
    pub declares: bool,
    /// A `pub use`, an `export … from`: the name it brings in is offered on
    /// again under this file's address, so a path to it here reaches the
    /// original declaration.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reexport: bool,
    /// The name the statement binds instead of the path's last segment:
    /// `Cfg` in `use a::Config as Cfg`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<Name>,
}

fn yes() -> bool {
    true
}

impl ImportRef {
    pub fn new(span: Span, path: ModulePath) -> Self {
        Self {
            span,
            path,
            group: None,
            glob: false,
            declares: true,
            reexport: false,
            alias: None,
        }
    }

    pub fn glob(mut self) -> Self {
        self.glob = true;
        self
    }

    pub fn reexporting(mut self) -> Self {
        self.reexport = true;
        self
    }

    pub fn aliased(mut self, alias: impl Into<Name>) -> Self {
        self.alias = Some(alias.into());
        self
    }

    /// The name the import brings into scope: its alias, else the path's
    /// last segment; a glob binds nothing nameable.
    pub fn binding(&self) -> Option<&Name> {
        if self.glob {
            return None;
        }
        self.alias.as_ref().or_else(|| self.path.last())
    }

    /// True when replacing `span` with any path is a complete rewrite.
    pub fn is_standalone(&self) -> bool {
        self.group.is_none()
    }
}

/// Where a grouped entry sits, so a surgery can rewrite it in place or move
/// it out of the group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportGroup {
    /// Combined prefix of the enclosing groups (`crate::util`).
    pub prefix: ModulePath,
    /// The whole entry, alias or nested list included (`parse::Config as Cfg`).
    pub item: Span,
    /// The innermost list the entry belongs to, braces included.
    pub list: Span,
    /// How many entries that list has.
    pub items: usize,
    /// The enclosing statement (`pub use crate::util::{…};`).
    pub statement: Span,
    /// Whether `list` is the statement's own list rather than a nested one.
    pub top_level: bool,
}

/// Where a grammar keeps import paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportRule {
    /// Nodes of this kind carry a path: in `field` when given, else the node
    /// itself. Only the outermost such node counts, so `crate::a::b` is one
    /// reference even though its parser tree nests three `scoped_identifier`s.
    Node {
        kind: &'static str,
        field: Option<&'static str>,
        /// Only when the direct parent has this kind.
        under: Option<&'static str>,
    },
    /// An ast-grep pattern whose `capture` holds the path.
    Pattern {
        pattern: &'static str,
        capture: &'static str,
    },
}

impl ImportRule {
    pub const fn node(kind: &'static str) -> Self {
        Self::Node {
            kind,
            field: None,
            under: None,
        }
    }

    pub const fn field(kind: &'static str, field: &'static str) -> Self {
        Self::Node {
            kind,
            field: Some(field),
            under: None,
        }
    }

    pub const fn under(self, parent: &'static str) -> Self {
        match self {
            Self::Node { kind, field, .. } => Self::Node {
                kind,
                field,
                under: Some(parent),
            },
            other => other,
        }
    }

    pub const fn pattern(pattern: &'static str, capture: &'static str) -> Self {
        Self::Pattern { pattern, capture }
    }
}

/// How paths nest inside grouped imports such as Rust's `use a::{b, c::d}`.
///
/// An entry found under a `list` node gets the enclosing `scope` nodes'
/// `prefix_field` texts prepended (outermost first, joined by the grammar's
/// [`PathSyntax`]) and carries an [`ImportGroup`] describing its place in
/// the `statement`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportNesting {
    pub list: &'static str,
    pub scope: &'static str,
    pub prefix_field: &'static str,
    pub statement: &'static str,
}

/// Where an import's alias is written: the path sits under a node of this
/// kind, which holds the alias in `field` (Rust's `use_as_clause`, field
/// `alias`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AliasRule {
    pub under: &'static str,
    pub field: &'static str,
}

/// What makes an import statement a re-export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReExportRule {
    /// The language has none vvv follows.
    #[default]
    Never,
    /// The statement carries a child of this kind (Rust's
    /// `visibility_modifier` on a `use_declaration`).
    Modifier(&'static str),
    /// The statement kind itself re-exports (TypeScript's `export_statement`
    /// with a source).
    Statement(&'static str),
}

/// A language's complete description of where its import paths live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportGrammar {
    /// How the language spells a path: what the rules' text is parsed with.
    pub syntax: PathSyntax,
    pub rules: &'static [ImportRule],
    /// Which statements re-export what they import.
    pub reexports: ReExportRule,
    pub nesting: Option<ImportNesting>,
    /// Node kind whose path child is a glob import (`use_wildcard` in Rust).
    pub glob_under: Option<&'static str>,
    /// Text every glob import contains (`::*` in Rust): what a file must
    /// spell to be worth parsing when only globs are looked for.
    pub glob_marker: Option<&'static str>,
    /// Where an `as` alias is found, in languages that spell one on a path.
    pub alias: Option<AliasRule>,
    /// Node kinds of import statements. A path found under one declares its
    /// name; a path found elsewhere is a reference. Empty means every path
    /// the rules find is a statement.
    pub statements: &'static [&'static str],
}

impl ImportGrammar {
    pub const EMPTY: Self = Self {
        syntax: PathSyntax::Scoped,
        rules: &[],
        reexports: ReExportRule::Never,
        nesting: None,
        glob_under: None,
        glob_marker: None,
        alias: None,
        statements: &[],
    };

    pub const fn new(syntax: PathSyntax, rules: &'static [ImportRule]) -> Self {
        Self {
            syntax,
            rules,
            reexports: ReExportRule::Never,
            nesting: None,
            glob_under: None,
            glob_marker: None,
            alias: None,
            statements: &[],
        }
    }

    pub const fn aliased_by(mut self, parent: &'static str, field: &'static str) -> Self {
        self.alias = Some(AliasRule {
            under: parent,
            field,
        });
        self
    }

    pub const fn statements(mut self, kinds: &'static [&'static str]) -> Self {
        self.statements = kinds;
        self
    }

    pub const fn nested(mut self, nesting: ImportNesting) -> Self {
        self.nesting = Some(nesting);
        self
    }

    pub const fn reexports(mut self, rule: ReExportRule) -> Self {
        self.reexports = rule;
        self
    }

    /// Globs are the paths under `parent`, and every one spells `marker`.
    pub const fn globs_under(mut self, parent: &'static str, marker: &'static str) -> Self {
        self.glob_under = Some(parent);
        self.glob_marker = Some(marker);
        self
    }
}
