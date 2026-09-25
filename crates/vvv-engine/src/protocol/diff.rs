//! A file's change as a unified diff: the rendered text that crosses the
//! wire, and the hunks it is made of.
//!
//! The text is the wire form (a `--json` client reads it, `Serialize` writes
//! it) and nothing here parses it. The model is [`Hunk`]/[`DiffLine`], built
//! straight from `similar` when the diff is made: the `@@` grammar exists only
//! as the text `similar` formats, never as something a consumer re-reads.

use std::fmt;
use std::ops::Range;
use std::path::Path;

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff};

/// How many context lines `similar` keeps around a change.
const CONTEXT: usize = 3;

/// Lines a hunk covers in one file: `start` is the first line (1-based) and
/// `len` how many it spans. `len == 0` is an insertion between lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub len: u32,
}

impl LineRange {
    /// The range `side` picks out of a hunk's ops, 1-based.
    fn of(ops: &[similar::DiffOp], side: fn(&similar::DiffOp) -> Range<usize>) -> Self {
        let first = side(&ops[0]).start;
        let last = side(ops.last().expect("a hunk has an op")).end;
        Self {
            start: first as u32 + 1,
            len: (last - first) as u32,
        }
    }

    /// Whether 1-based `line` falls inside the range.
    pub fn contains(self, line: u32) -> bool {
        self.len > 0 && line >= self.start && line < self.start + self.len
    }
}

/// What a diff line does to the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// Unchanged, kept for context.
    Context,
    /// Added by the change.
    Added,
    /// Removed by the change.
    Removed,
}

/// One line of a hunk: its kind and its text, without the `+`/`-`/space
/// marker. Its number is implied by the hunk's range and the lines before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub text: String,
}

/// One `@@` block: the lines it covers in each file, the header it renders
/// as, and the lines themselves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old: LineRange,
    pub new: LineRange,
    /// The `@@ -a,b +c,d @@` header, exactly as `similar` renders it.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// One file's change: the unified diff text (the wire) and its hunks (the
/// model).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "String", from = "String")]
pub struct Diff {
    text: String,
    hunks: Vec<Hunk>,
}

impl Diff {
    /// A diff whose headers name different paths, as `git diff` shows a rename.
    pub fn of(path: &Path, before: &str, after: &str) -> Self {
        Self::between(path, path, before, after)
    }

    /// The diff of `before` → `after`, its hunks read straight from `similar`.
    pub fn between(old_path: &Path, new_path: &Path, before: &str, after: &str) -> Self {
        let changes = TextDiff::from_lines(before, after);
        let mut rendered = changes.unified_diff();
        rendered.context_radius(CONTEXT).header(
            &format!("a/{}", old_path.display()),
            &format!("b/{}", new_path.display()),
        );
        let hunks = rendered
            .iter_hunks()
            .map(|hunk| {
                let ops = hunk.ops();
                let lines = hunk
                    .iter_changes()
                    .map(|change| DiffLine {
                        kind: match change.tag() {
                            ChangeTag::Equal => DiffKind::Context,
                            ChangeTag::Delete => DiffKind::Removed,
                            ChangeTag::Insert => DiffKind::Added,
                        },
                        text: change.value().trim_end_matches(['\r', '\n']).to_owned(),
                    })
                    .collect();
                Hunk {
                    old: LineRange::of(ops, |op| op.old_range()),
                    new: LineRange::of(ops, |op| op.new_range()),
                    header: hunk.header().to_string(),
                    lines,
                }
            })
            .collect();
        let mut text = rendered.to_string();
        if text.is_empty() && old_path != new_path {
            text = format!(
                "--- a/{}\n+++ b/{}\n",
                old_path.display(),
                new_path.display()
            );
        }
        Self { text, hunks }
    }

    /// The rendered unified diff: the wire form.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The hunks the diff is made of.
    pub fn hunks(&self) -> &[Hunk] {
        &self.hunks
    }

    /// The index of the hunk opening at or before 1-based old-file `line`;
    /// the first when the line falls before them all.
    pub fn opening(&self, line: u32) -> usize {
        let mut open = 0;
        for (i, hunk) in self.hunks.iter().enumerate() {
            if hunk.old.start > line {
                break;
            }
            open = i;
        }
        open
    }
}

impl From<Diff> for String {
    fn from(diff: Diff) -> Self {
        diff.text
    }
}

/// The text alone: the hunks a deserialized diff never had are left empty,
/// which is fine because only `Serialize` crosses a wire.
impl From<String> for Diff {
    fn from(text: String) -> Self {
        Self {
            text,
            hunks: Vec::new(),
        }
    }
}

impl fmt::Display for Diff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// A twelve-line file with a change on line 2 and another on line 10:
    /// two hunks, `old.start` 1 and 7.
    fn two_hunks() -> Diff {
        let before: String = (1..=12).map(|k| format!("line {k}\n")).collect();
        let after = before
            .replace("line 2\n", "line 2 changed\n")
            .replace("line 10\n", "line 10 changed\n");
        Diff::between(Path::new("a.rs"), Path::new("a.rs"), &before, &after)
    }

    #[test]
    fn hunks_carry_their_ranges_and_lines() {
        let diff = two_hunks();
        assert_eq!(diff.hunks().len(), 2);
        let first = &diff.hunks()[0];
        assert_eq!((first.old.start, first.old.len), (1, 5));
        assert!(first.old.contains(2), "the changed line is in the range");
        assert!(
            first
                .lines
                .iter()
                .any(|l| l.kind == DiffKind::Removed && l.text == "line 2")
        );
        assert!(
            first
                .lines
                .iter()
                .any(|l| l.kind == DiffKind::Added && l.text == "line 2 changed")
        );
    }

    #[test]
    fn opening_finds_the_hunk_at_or_before_a_line() {
        let diff = two_hunks();
        assert_eq!(diff.opening(1), 0, "before the first hunk, the first hunk");
        assert_eq!(diff.opening(2), 0);
        assert_eq!(diff.opening(7), 1, "the second hunk opens at old line 7");
        assert_eq!(diff.opening(10), 1);
    }

    #[test]
    fn a_single_line_hunk_has_no_count_in_its_header() {
        let diff = Diff::of(Path::new("a.rs"), "a\n", "b\n");
        let hunk = &diff.hunks()[0];
        assert_eq!(hunk.header, "@@ -1 +1 @@");
        assert_eq!((hunk.old.start, hunk.old.len), (1, 1));
    }
}
