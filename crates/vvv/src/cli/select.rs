//! What `--select` accepts: row numbers from a preview (`3`, `27-33`) or
//! content ids from `--json` (`ae22071f2d2e`). The two never mix in one
//! selection; a number is a row, anything else is an id.

use std::collections::BTreeSet;

use vvv_engine::{MatchId, Selection};

/// A `--select` the CLI cannot read.
#[derive(Debug)]
pub enum BadSelect {
    /// Neither a row, a range nor an id.
    Value(String),
    /// Rows and ids in one selection.
    Mixed,
}

impl std::fmt::Display for BadSelect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Value(value) => write!(
                f,
                "--select: `{value}` is not a row number, a range like 3-7, or an id"
            ),
            Self::Mixed => f.write_str("--select mixes row numbers and ids; pass one kind"),
        }
    }
}

impl std::error::Error for BadSelect {}

/// Parse the comma-split values of `--select`. Empty input selects all.
pub fn parse(values: &[String]) -> anyhow::Result<Selection> {
    if values.is_empty() {
        return Ok(Selection::All);
    }
    let mut ordinals: BTreeSet<usize> = BTreeSet::new();
    let mut ids: BTreeSet<MatchId> = BTreeSet::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match range(value) {
            Some((from, to)) if from <= to => ordinals.extend(from..=to),
            Some(_) => return Err(BadSelect::Value(value.to_owned()).into()),
            None => {
                ids.insert(MatchId::from(value.to_owned()));
            }
        }
    }
    match (ordinals.is_empty(), ids.is_empty()) {
        (false, false) => Err(BadSelect::Mixed.into()),
        (false, true) => Ok(Selection::ordinals(ordinals)),
        (true, false) => Ok(Selection::ids(ids)),
        (true, true) => Ok(Selection::All),
    }
}

/// `3` as `(3, 3)`, `3-7` as `(3, 7)`; `None` when not all digits.
fn range(value: &str) -> Option<(usize, usize)> {
    let (from, to) = value.split_once('-').unwrap_or((value, value));
    let digits = |s: &str| {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    Some((digits(from)?, digits(to)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn numbers_and_ranges_are_rows() {
        assert_eq!(
            parse(&strings(&["3", "27-29", "5"])).unwrap(),
            Selection::ordinals([3, 5, 27, 28, 29])
        );
    }

    #[test]
    fn anything_else_is_an_id() {
        assert_eq!(
            parse(&strings(&["ae22071f2d2e", "0e1692fd1b56"])).unwrap(),
            Selection::ids([
                MatchId::from("ae22071f2d2e".to_owned()),
                MatchId::from("0e1692fd1b56".to_owned())
            ])
        );
    }

    #[test]
    fn mixing_and_backward_ranges_are_refused() {
        assert!(parse(&strings(&["3", "abc"])).is_err());
        assert!(parse(&strings(&["7-3"])).is_err());
        assert_eq!(parse(&[]).unwrap(), Selection::All);
    }
}
