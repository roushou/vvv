//! How the answers are read: the output vocabulary of glyphs and words, how
//! an intent is named, how a count and a time are said. Every interface
//! renders these — colour is the renderer's (the CLI's `Palette`, the
//! picker's `Theme`); this is what is being coloured, and it lives with the
//! shapes it describes so a client reads them the way the CLI prints them.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{BatchIntent, Confidence, Intent, Reason};

/// One mark of the vocabulary. A verdict or a reason maps onto it; a panel
/// title or a section header names it with its word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mark {
    /// `●` — a declaration.
    Declaration,
    /// `●` — a declaration of the same name that is not the one meant.
    OtherDeclaration,
    /// `→` — an import, a rewrite's target.
    Import,
    /// `→` — the tail of a qualified path.
    Path,
    /// `→` — a glob import opens the declaring module.
    Glob,
    /// `→` — a path into a package outside the workspace.
    External,
    /// `←` — imported by.
    ImportedBy,
    /// `↗` — reached through a re-export.
    ReExport,
    /// `◎` — an oracle (a build, a language server) placed what syntax could not.
    Oracle,
    /// `✓` — will happen; the site resolves to the declaration.
    Safe,
    /// `?` — vvv could not verify the site refers to the declaration.
    Unverified,
    /// `✗` — will not happen; the site is another declaration's.
    Other,
    /// `∅` — nothing; a bare name nothing imports.
    Nothing,
    /// `∅` — no declaration to judge against; matched by name.
    ByName,
    /// `!` — something to do by hand.
    ByHand,
    /// `±` — a structural edit: a `mod` line, a modifier, a file moved.
    Structure,
    /// `±` — a match with its replacement.
    Rewrite,
    /// `▪` — a row ticked for the commit (the picker's).
    Ticked,
    /// `▫` — a row left out (the picker's).
    Unticked,
    /// `↩` — what undo reverses.
    Undo,
}

impl Mark {
    pub fn glyph(self) -> char {
        match self {
            Self::Declaration | Self::OtherDeclaration => '●',
            Self::Import | Self::Path | Self::Glob | Self::External => '→',
            Self::ImportedBy => '←',
            Self::ReExport => '↗',
            Self::Oracle => '◎',
            Self::Safe => '✓',
            Self::Unverified => '?',
            Self::Other => '✗',
            Self::Nothing | Self::ByName => '∅',
            Self::ByHand => '!',
            Self::Structure | Self::Rewrite => '±',
            Self::Ticked => '▪',
            Self::Unticked => '▫',
            Self::Undo => '↩',
        }
    }

    /// The mark spelled out, for a title or a tag.
    pub fn word(self) -> &'static str {
        match self {
            Self::Declaration => "declaring module",
            Self::OtherDeclaration => "other decl",
            Self::Import => "imported",
            Self::Path => "path",
            Self::Glob => "glob",
            Self::External => "external",
            Self::ImportedBy => "imported by",
            Self::ReExport => "re-export",
            Self::Oracle => "oracle",
            Self::Safe => "safe",
            Self::Unverified => "unverified",
            Self::Other => "another declaration's",
            Self::Nothing => "unresolved",
            Self::ByName => "by name",
            Self::ByHand => "by hand",
            Self::Structure => "structure",
            Self::Rewrite => "rewrites",
            Self::Ticked => "ticked",
            Self::Unticked => "unticked",
            Self::Undo => "undo",
        }
    }

    /// One line on what a reason's mark means at a site (the picker's detail).
    pub fn meaning(self) -> &'static str {
        match self {
            Self::Declaration => "written in the module that declares it",
            Self::Import => "an import resolving to the declaration brings the name in",
            Self::Path => "the tail of a path resolving to the declaration",
            Self::Glob => "a glob import opens the declaring module",
            Self::ReExport => "reached through a re-export vvv followed to the declaration",
            Self::Oracle => "syntax could not place it; an oracle that knows the code did",
            Self::Nothing => "a bare name nothing imports, or a path vvv cannot place",
            Self::ByName => "no declaration to judge against; matched by name",
            Self::OtherDeclaration => "resolves to another declaration of the same name",
            Self::External => "resolves into a crate outside the workspace",
            Self::ImportedBy => "files whose imports lead here",
            Self::Safe => "resolves to the declaration",
            Self::Unverified => "nothing connects it to the declaration",
            Self::Other => "belongs to another declaration",
            Self::ByHand => "vvv stops short here; the change is yours to write",
            Self::Structure => "a mod line, a modifier or a file moved",
            Self::Rewrite => "a match and what it becomes",
            Self::Ticked => "in the commit",
            Self::Unticked => "left out of the commit",
            Self::Undo => "what undo reverses",
        }
    }

    pub fn ticked(ticked: bool) -> Self {
        if ticked { Self::Ticked } else { Self::Unticked }
    }
}

/// The glyph alone: `Mark::Safe` displays as `✓`.
impl fmt::Display for Mark {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.glyph())
    }
}

impl From<Confidence> for Mark {
    fn from(confidence: Confidence) -> Self {
        match confidence {
            Confidence::Resolved => Self::Safe,
            Confidence::Unresolved => Self::Unverified,
            Confidence::Other => Self::Other,
        }
    }
}

impl From<Reason> for Mark {
    fn from(reason: Reason) -> Self {
        match reason {
            Reason::Declaring => Self::Declaration,
            Reason::Imported => Self::Import,
            Reason::Path => Self::Path,
            Reason::Opened => Self::Glob,
            Reason::ReExport => Self::ReExport,
            Reason::Unresolved => Self::Nothing,
            Reason::ByName => Self::ByName,
            Reason::OtherDeclaration => Self::OtherDeclaration,
            Reason::External => Self::External,
            Reason::Oracle => Self::Oracle,
            Reason::OracleOther => Self::OtherDeclaration,
        }
    }
}

/// One line naming a mutating request: `rename Config → Settings`. The only
/// place this wording exists; titles and history lines both use it.
pub struct IntentLine<'a>(pub &'a Intent);

impl fmt::Display for IntentLine<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Intent::Rewrite(i) => write!(
                f,
                "rewrite {} → {}",
                i.query.pattern_str().unwrap_or_default(),
                i.template
            ),
            Intent::Rename(i) => write!(f, "rename {} → {}", i.name, i.to),
            Intent::Move(i) => write!(f, "move {} → {}", i.from.display(), i.to.display()),
            Intent::MoveSymbol(i) => write!(
                f,
                "move {} from {} → {}",
                i.name,
                i.from.display(),
                i.to.display()
            ),
            Intent::Batch(BatchIntent { intents }) => {
                write!(f, "batch of {}", Plural(intents.len(), "step"))
            }
        }
    }
}

/// `1 file` / `3 files` / `1 entry` / `2 entries` / `1 match` / `2 matches`.
pub struct Plural(pub usize, pub &'static str);

impl fmt::Display for Plural {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Plural(n, noun) = *self;
        if n == 1 {
            return write!(f, "1 {noun}");
        }
        let plural = match noun {
            "entry" => "entries".to_owned(),
            "match" => "matches".to_owned(),
            other => format!("{other}s"),
        };
        write!(f, "{n} {plural}")
    }
}

/// The distinct files among some paths, said as `3 files`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Files(usize);

impl Files {
    /// Count the distinct paths.
    pub fn among<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Self {
        Self(paths.into_iter().collect::<BTreeSet<_>>().len())
    }

    pub const fn count(self) -> usize {
        self.0
    }
}

impl fmt::Display for Files {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Plural(self.0, "file"))
    }
}

/// Coarse relative time: `just now`, `5 min ago`, `3 h ago`, `2 days ago`.
pub struct Ago(u64);

impl Ago {
    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    pub fn between(then: u64, now: u64) -> Self {
        Self(now.saturating_sub(then))
    }
}

impl fmt::Display for Ago {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            0..60 => write!(f, "just now"),
            60..3600 => write!(f, "{} min ago", self.0 / 60),
            3600..86400 => write!(f, "{} h ago", self.0 / 3600),
            _ => write!(f, "{} days ago", self.0 / 86400),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_and_reasons_map_onto_the_vocabulary() {
        assert_eq!(Mark::from(Confidence::Unresolved).to_string(), "?");
        assert_eq!(Mark::from(Reason::ReExport).word(), "re-export");
        assert_eq!(Mark::from(Reason::Path).glyph(), Mark::Import.glyph());
        assert_eq!(Mark::ticked(true).glyph(), '▪');
    }

    #[test]
    fn ago_buckets() {
        assert_eq!(Ago::between(100, 130).to_string(), "just now");
        assert_eq!(Ago::between(0, 300).to_string(), "5 min ago");
        assert_eq!(Ago::between(0, 7200).to_string(), "2 h ago");
        assert_eq!(Ago::between(0, 200_000).to_string(), "2 days ago");
    }
}
