//! Names and paths as types, never as text.
//!
//! A [`Name`] is one identifier. A [`ModulePath`] is a path *as a language
//! spells it* — `crate::a::b`, `../x/y`, `react/jsx-runtime` — parsed once
//! by the grammar's [`PathSyntax`] when facts are extracted, and spelled
//! back only by that syntax. What a path *means* is an
//! [`Address`](crate::resolve::Address), which the layout resolves; the two
//! never mix. Nothing above the grammar ever sees a separator character.

use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One identifier: a module, a declaration, a file stem. Compared by text.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Name(String);

impl Name {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path as this machine spells it, for an actual file call.
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl Deref for Name {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Name {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for Name {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Name {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for Name {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl From<&str> for Name {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for Name {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl From<&String> for Name {
    fn from(text: &String) -> Self {
        Self(text.clone())
    }
}

impl From<Name> for String {
    fn from(name: Name) -> Self {
        name.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// A file's path inside the workspace, spelled the way the project does: `/`
/// on every platform.
///
/// A workspace path is a project path — like a `mod` path or a
/// `crate::a::b` address — not a path of the machine, so it carries its own
/// spelling and is what goes on the wire, into a row, or into a diagnostic.
/// It reads as a [`Path`] (an OS path is made from one at the edge, where a
/// file is actually read), but it cannot spell a separator the host's way:
/// `Display`, serde and the stored bytes are always `/`.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelPath(String);

impl RelPath {
    /// Whatever the host spells a path with, a workspace path is `/`.
    pub fn new(path: &Path) -> Self {
        Self(spell(path.display().to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path as this machine spells it, for an actual file call.
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }
}

/// `/` on every platform; on Unix this is the text unchanged.
fn spell(text: String) -> String {
    if std::path::MAIN_SEPARATOR == '/' {
        text
    } else {
        text.replace(std::path::MAIN_SEPARATOR, "/")
    }
}

impl Deref for RelPath {
    type Target = Path;
    fn deref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl AsRef<Path> for RelPath {
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<str> for RelPath {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Borrow<Path> for RelPath {
    fn borrow(&self) -> &Path {
        self
    }
}

impl PartialEq<Path> for RelPath {
    fn eq(&self, other: &Path) -> bool {
        self.as_path() == other
    }
}

impl PartialEq<&Path> for RelPath {
    fn eq(&self, other: &&Path) -> bool {
        self.as_path() == *other
    }
}

impl PartialEq<RelPath> for Path {
    fn eq(&self, other: &RelPath) -> bool {
        self == other.as_path()
    }
}

impl PartialEq<PathBuf> for RelPath {
    fn eq(&self, other: &PathBuf) -> bool {
        self.as_path() == other.as_path()
    }
}

impl PartialEq<RelPath> for PathBuf {
    fn eq(&self, other: &RelPath) -> bool {
        self.as_path() == other.as_path()
    }
}

impl PartialEq<str> for RelPath {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for RelPath {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl From<&RelPath> for RelPath {
    fn from(path: &RelPath) -> Self {
        path.clone()
    }
}

impl From<&str> for RelPath {
    fn from(path: &str) -> Self {
        Self(spell(path.to_owned()))
    }
}

impl From<String> for RelPath {
    fn from(path: String) -> Self {
        Self(spell(path))
    }
}

impl From<&Path> for RelPath {
    fn from(path: &Path) -> Self {
        Self::new(path)
    }
}

impl From<PathBuf> for RelPath {
    fn from(path: PathBuf) -> Self {
        Self::new(&path)
    }
}

impl From<&PathBuf> for RelPath {
    fn from(path: &PathBuf) -> Self {
        Self::new(path)
    }
}

impl From<&RelPath> for PathBuf {
    fn from(path: &RelPath) -> Self {
        PathBuf::from(&path.0)
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

/// Where a path starts: the part before its segments that says what they
/// are relative to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathHead {
    /// The root of the package the file belongs to: Rust's `crate::`.
    Package,
    /// The file's own module or directory: `self::`, `./`.
    Here,
    /// `n` levels up from the file's module or directory: `super::super::`,
    /// `../../`.
    Up(u8),
    /// The enclosing type, not a module: Rust's `Self::`. Never a module path.
    SelfType,
    /// A named package or an in-scope name, which only the layout can tell
    /// apart: `serde::`, `react`, `@scope/pkg`, a child module, an imported
    /// name. The name is the first segment.
    Named,
    /// The file system root: `/x/y`.
    Root,
}

/// How a language spells paths: the separator between segments and the
/// words it uses for a head. A grammar declares one; it parses import text
/// into [`ModulePath`]s and spells them back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSyntax {
    /// `crate::a::b`, `self::a`, `super::super::a`, `Self::A`, `serde::X`.
    Scoped,
    /// `./a/b`, `../../a`, `/a/b`, `react/x`, `@scope/pkg`.
    Posix,
}

impl PathSyntax {
    pub const fn separator(self) -> &'static str {
        match self {
            Self::Scoped => "::",
            Self::Posix => "/",
        }
    }

    /// Read a path as written. Lossless for what a grammar hands over:
    /// `spell(parse(text)) == text` for every well-formed path.
    pub fn parse(self, text: &str) -> ModulePath {
        let mut segments: Vec<&str> = text.split(self.separator()).collect();
        let head = match self {
            Self::Scoped => match segments.first().copied() {
                Some("crate") => {
                    segments.remove(0);
                    PathHead::Package
                }
                Some("self") => {
                    segments.remove(0);
                    PathHead::Here
                }
                Some("Self") => {
                    segments.remove(0);
                    PathHead::SelfType
                }
                Some("super") => {
                    let ups = segments.iter().take_while(|s| **s == "super").count();
                    segments.drain(..ups);
                    PathHead::Up(ups as u8)
                }
                _ => PathHead::Named,
            },
            Self::Posix => match segments.first().copied() {
                Some("") if segments.len() > 1 => {
                    segments.remove(0);
                    PathHead::Root
                }
                Some(".") => {
                    segments.remove(0);
                    PathHead::Here
                }
                Some("..") => {
                    let ups = segments.iter().take_while(|s| **s == "..").count();
                    segments.drain(..ups);
                    PathHead::Up(ups as u8)
                }
                // `@scope/pkg` is one package name.
                Some(scope) if scope.starts_with('@') && segments.len() > 1 => {
                    let package = format!("{scope}/{}", segments[1]);
                    segments.drain(..2);
                    segments.insert(0, "");
                    let mut path = ModulePath {
                        syntax: self,
                        head: PathHead::Named,
                        segments: segments.iter().map(|s| Name::new(*s)).collect(),
                    };
                    path.segments[0] = Name::new(package);
                    return path;
                }
                _ => PathHead::Named,
            },
        };
        ModulePath {
            syntax: self,
            head,
            segments: segments
                .into_iter()
                .filter(|s| !s.is_empty())
                .map(Name::new)
                .collect(),
        }
    }

    /// The path as this syntax writes it.
    pub fn spell(self, path: &ModulePath) -> String {
        let sep = self.separator();
        let segments = path
            .segments
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>()
            .join(sep);
        match (self, &path.head) {
            (Self::Scoped, PathHead::Package) => Self::joined("crate", sep, &segments),
            (Self::Scoped, PathHead::Here) => Self::joined("self", sep, &segments),
            (Self::Scoped, PathHead::SelfType) => Self::joined("Self", sep, &segments),
            (Self::Scoped, PathHead::Up(n)) => {
                let ups = vec!["super"; *n as usize].join(sep);
                Self::joined(&ups, sep, &segments)
            }
            (Self::Scoped, PathHead::Named | PathHead::Root) => segments,
            (Self::Posix, PathHead::Here) => Self::joined(".", sep, &segments),
            (Self::Posix, PathHead::Up(n)) => {
                let ups = vec![".."; *n as usize].join(sep);
                Self::joined(&ups, sep, &segments)
            }
            (Self::Posix, PathHead::Root) => format!("/{segments}"),
            (Self::Posix, PathHead::Package | PathHead::SelfType | PathHead::Named) => segments,
        }
    }

    fn joined(head: &str, sep: &str, segments: &str) -> String {
        if segments.is_empty() {
            head.to_owned()
        } else {
            format!("{head}{sep}{segments}")
        }
    }
}

/// A path as spelled in source: its head and its segments, with the syntax
/// that spells it. Serialises as the text, so the wire is what the user
/// wrote.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModulePath {
    syntax: PathSyntax,
    pub head: PathHead,
    pub segments: Vec<Name>,
}

impl ModulePath {
    pub fn new(
        syntax: PathSyntax,
        head: PathHead,
        segments: impl IntoIterator<Item = impl Into<Name>>,
    ) -> Self {
        Self {
            syntax,
            head,
            segments: segments.into_iter().map(Into::into).collect(),
        }
    }

    pub fn syntax(&self) -> PathSyntax {
        self.syntax
    }

    pub fn first(&self) -> Option<&Name> {
        self.segments.first()
    }

    pub fn last(&self) -> Option<&Name> {
        self.segments.last()
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The segments after `prefix`, when this path continues it: same head,
    /// its segments first.
    pub fn strip_prefix(&self, prefix: &ModulePath) -> Option<&[Name]> {
        (self.head == prefix.head)
            .then(|| self.segments.strip_prefix(prefix.segments.as_slice()))
            .flatten()
    }

    /// Whether this path continues `prefix` by at least one segment.
    pub fn continues(&self, prefix: &ModulePath) -> bool {
        self.strip_prefix(prefix)
            .is_some_and(|rest| !rest.is_empty())
    }

    /// The same path with `segments` in place of its own.
    pub fn with_segments(&self, segments: impl IntoIterator<Item = impl Into<Name>>) -> Self {
        Self::new(self.syntax, self.head.clone(), segments)
    }

    /// The path up to and including segment `index`: what a token in the
    /// middle of a path names.
    pub fn prefix(&self, index: usize) -> Self {
        self.with_segments(self.segments[..=index].iter().cloned())
    }

    /// Which segment is spelled at byte `offset` from the path's start, as
    /// its syntax writes it; `None` on a separator or the head.
    pub fn segment_at(&self, offset: usize) -> Option<usize> {
        let sep = self.syntax.separator().len();
        let head = self
            .syntax
            .spell(&self.with_segments(std::iter::empty::<Name>()));
        let mut at = match (&self.head, head.len()) {
            (PathHead::Root, n) => n,
            (_, 0) => 0,
            (_, n) => n + sep,
        };
        for (i, segment) in self.segments.iter().enumerate() {
            if (at..at + segment.len()).contains(&offset) {
                return Some(i);
            }
            at += segment.len() + sep;
        }
        None
    }

    /// `segments` alone, spelled in this path's syntax: the tail after a
    /// group's prefix.
    pub fn spell_segments(&self, segments: &[Name]) -> String {
        segments
            .iter()
            .map(Name::as_str)
            .collect::<Vec<_>>()
            .join(self.syntax.separator())
    }
}

impl fmt::Display for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.syntax.spell(self))
    }
}

impl fmt::Debug for ModulePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ModulePath({:?})", self.to_string())
    }
}

impl Serialize for ModulePath {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// A path read back from text carries no syntax; `::` anywhere means
/// scoped, anything else Posix.
impl<'de> Deserialize<'de> for ModulePath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let syntax = if text.contains("::") {
            PathSyntax::Scoped
        } else {
            PathSyntax::Posix
        };
        Ok(syntax.parse(&text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(syntax: PathSyntax, text: &str) -> ModulePath {
        let path = syntax.parse(text);
        assert_eq!(syntax.spell(&path), text, "{path:?}");
        path
    }

    #[test]
    fn scoped_paths_parse_and_spell_losslessly() {
        assert_eq!(
            round_trip(PathSyntax::Scoped, "crate::a::b").head,
            PathHead::Package
        );
        assert_eq!(
            round_trip(PathSyntax::Scoped, "self::a").head,
            PathHead::Here
        );
        assert_eq!(
            round_trip(PathSyntax::Scoped, "super::super::x").head,
            PathHead::Up(2)
        );
        assert_eq!(round_trip(PathSyntax::Scoped, "super").segments.len(), 0);
        assert_eq!(
            round_trip(PathSyntax::Scoped, "Self::X").head,
            PathHead::SelfType
        );
        let named = round_trip(PathSyntax::Scoped, "serde::Serialize");
        assert_eq!(named.head, PathHead::Named);
        assert_eq!(named.first().map(Name::as_str), Some("serde"));
        assert_eq!(round_trip(PathSyntax::Scoped, "Foo").segments.len(), 1);
    }

    /// The one platform that spells its separator differently must not reach
    /// a workspace path: built by `join`, it is still shown with `/`.
    #[test]
    fn a_workspace_path_is_spelled_with_slashes() {
        let native = PathBuf::from("a").join("b.rs");
        assert_eq!(RelPath::new(&native).as_str(), "a/b.rs");
        assert_eq!(RelPath::from(&native).to_string(), "a/b.rs");
    }

    #[test]
    fn posix_paths_parse_and_spell_losslessly() {
        assert_eq!(round_trip(PathSyntax::Posix, "./x.js").head, PathHead::Here);
        assert_eq!(round_trip(PathSyntax::Posix, ".").segments.len(), 0);
        assert_eq!(
            round_trip(PathSyntax::Posix, "../../a/b").head,
            PathHead::Up(2)
        );
        assert_eq!(round_trip(PathSyntax::Posix, "../..").head, PathHead::Up(2));
        assert_eq!(round_trip(PathSyntax::Posix, "/etc/x").head, PathHead::Root);
        let scoped = round_trip(PathSyntax::Posix, "@scope/pkg/sub");
        assert_eq!(scoped.first().map(Name::as_str), Some("@scope/pkg"));
        assert_eq!(scoped.segments.len(), 2);
        assert_eq!(
            round_trip(PathSyntax::Posix, "react/jsx-runtime").head,
            PathHead::Named
        );
    }

    #[test]
    fn prefixes_compare_by_head_and_segments() {
        let group = PathSyntax::Scoped.parse("crate::util");
        let entry = PathSyntax::Scoped.parse("crate::util::parse::X");
        assert_eq!(
            entry.strip_prefix(&group).map(|r| entry.spell_segments(r)),
            Some("parse::X".to_owned())
        );
        assert!(!PathSyntax::Scoped.parse("crate::util").continues(&group));
        assert!(
            PathSyntax::Scoped
                .parse("self::util::x")
                .strip_prefix(&group)
                .is_none()
        );
    }

    #[test]
    fn segments_are_found_by_offset() {
        let path = PathSyntax::Scoped.parse("crate::commands::batch::BatchCmd");
        assert_eq!(path.segment_at(0), None, "the head");
        assert_eq!(path.segment_at(7), Some(0));
        assert_eq!(path.segment_at(17), Some(1));
        assert_eq!(path.segment_at(24), Some(2));
        assert_eq!(path.segment_at(15), None, "a separator");
        assert_eq!(path.prefix(1).to_string(), "crate::commands::batch");
        let bare = PathSyntax::Scoped.parse("a::b");
        assert_eq!(bare.segment_at(0), Some(0));
        assert_eq!(bare.segment_at(3), Some(1));
        let posix = PathSyntax::Posix.parse("../x/y.ts");
        assert_eq!(posix.segment_at(3), Some(0));
        assert_eq!(posix.segment_at(5), Some(1));
        assert_eq!(PathSyntax::Posix.parse("/etc/x").segment_at(1), Some(0));
    }

    #[test]
    fn serialises_as_text() {
        let path = PathSyntax::Scoped.parse("crate::a");
        assert_eq!(serde_json::to_string(&path).unwrap(), "\"crate::a\"");
        let back: ModulePath = serde_json::from_str("\"../x/y\"").unwrap();
        assert_eq!(back, PathSyntax::Posix.parse("../x/y"));
    }
}
