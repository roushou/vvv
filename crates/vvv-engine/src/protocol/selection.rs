use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{Match, MatchId};

/// Which matches of a search a later command acts on.
///
/// Ids are content-derived, so a selection made from `vvv search` can be
/// handed to `vvv rewrite` in a separate process. Ordinals are the 1-based
/// positions in the same search's result order, which is what human output
/// numbers rows with. Either kind that no longer resolves means the files
/// changed in between; the selection is then refused rather than silently
/// narrowed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Selection {
    #[default]
    All,
    Ids(BTreeSet<MatchId>),
    Ordinals(BTreeSet<usize>),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SelectionError {
    #[error("no match with id(s) {}; the files may have changed since the search", ids.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))]
    Unresolved { ids: BTreeSet<MatchId> },
    #[error("no match numbered {}; the search found {found}", ordinals.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))]
    OutOfRange {
        ordinals: BTreeSet<usize>,
        found: usize,
    },
}

impl Selection {
    pub fn ids(ids: impl IntoIterator<Item = MatchId>) -> Self {
        Self::Ids(ids.into_iter().collect())
    }

    pub fn ordinals(ordinals: impl IntoIterator<Item = usize>) -> Self {
        Self::Ordinals(ordinals.into_iter().collect())
    }

    pub fn is_all(&self) -> bool {
        matches!(self, Self::All)
    }

    pub fn narrow(&self, matches: Vec<Match>) -> Result<Vec<Match>, SelectionError> {
        match self {
            Self::All => Ok(matches),
            Self::Ordinals(wanted) => {
                let found = matches.len();
                let out: BTreeSet<usize> = wanted
                    .iter()
                    .filter(|&&n| n == 0 || n > found)
                    .copied()
                    .collect();
                if !out.is_empty() {
                    return Err(SelectionError::OutOfRange {
                        ordinals: out,
                        found,
                    });
                }
                Ok(matches
                    .into_iter()
                    .enumerate()
                    .filter(|(i, _)| wanted.contains(&(i + 1)))
                    .map(|(_, m)| m)
                    .collect())
            }
            Self::Ids(wanted) => {
                let kept: Vec<Match> = matches
                    .into_iter()
                    .filter(|m| wanted.contains(&m.id))
                    .collect();
                let found: BTreeSet<&MatchId> = kept.iter().map(|m| &m.id).collect();
                let missing: BTreeSet<MatchId> = wanted
                    .iter()
                    .filter(|id| !found.contains(id))
                    .cloned()
                    .collect();
                if missing.is_empty() {
                    Ok(kept)
                } else {
                    Err(SelectionError::Unresolved { ids: missing })
                }
            }
        }
    }
}

impl FromIterator<MatchId> for Selection {
    fn from_iter<I: IntoIterator<Item = MatchId>>(iter: I) -> Self {
        Self::ids(iter)
    }
}
