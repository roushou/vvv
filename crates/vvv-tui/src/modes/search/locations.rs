//! Result locations, independent of the workspace used to resolve definitions.

use std::collections::BTreeSet;
use vvv_engine::{EngineError, Match, RelPath, SearchScope};

#[derive(Debug, Clone, Default)]
pub struct Locations {
    pub selected: Option<RelPath>,
    known: BTreeSet<RelPath>,
}

impl Locations {
    pub fn observe_path(&mut self, path: &RelPath) {
        self.known.insert(path.clone());
    }
    pub fn scope(&self) -> SearchScope {
        SearchScope {
            paths: self.selected.iter().cloned().collect(),
            packages: Vec::new(),
        }
    }

    pub fn select(&mut self, value: Option<&str>) -> Result<(), EngineError> {
        let selected = value
            .map(|v| RelPath::from(v.trim_end_matches('/')))
            .filter(|p| !p.as_str().is_empty() && p.as_str() != ".");
        SearchScope {
            paths: selected.iter().cloned().collect(),
            packages: Vec::new(),
        }
        .validate()?;
        if let Some(path) = &selected {
            self.known.insert(path.clone());
        }
        self.selected = selected;
        Ok(())
    }

    pub fn observe(&mut self, matches: &[Match]) {
        for m in matches {
            let mut parts: Vec<_> = m.path.as_str().split('/').collect();
            parts.pop();
            while !parts.is_empty() {
                self.known.insert(RelPath::from(parts.join("/")));
                parts.pop();
            }
        }
    }

    pub fn choices(&self) -> Vec<String> {
        self.known.iter().map(|p| p.as_str().to_owned()).collect()
    }

    pub fn includes(&self, path: &RelPath) -> bool {
        self.selected
            .as_ref()
            .is_none_or(|prefix| path.starts_with(prefix))
    }

    pub fn retained_bytes(&self) -> usize {
        self.known
            .iter()
            .map(|p| p.as_str().len() + 128)
            .sum::<usize>()
            + self.selected.as_ref().map_or(0, |p| p.as_str().len())
    }
}
