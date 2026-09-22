use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use similar::TextDiff;

/// A `git diff`-style rendering of one file's change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnifiedDiff(String);

impl UnifiedDiff {
    pub fn of(path: &Path, before: &str, after: &str) -> Self {
        Self::between(path, path, before, after)
    }

    /// A diff whose headers name different paths, as `git diff` shows a rename.
    pub fn between(old_path: &Path, new_path: &Path, before: &str, after: &str) -> Self {
        let mut text = TextDiff::from_lines(before, after)
            .unified_diff()
            .context_radius(3)
            .header(
                &format!("a/{}", old_path.display()),
                &format!("b/{}", new_path.display()),
            )
            .to_string();
        if text.is_empty() && old_path != new_path {
            text = format!(
                "--- a/{}\n+++ b/{}\n",
                old_path.display(),
                new_path.display()
            );
        }
        Self(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UnifiedDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
