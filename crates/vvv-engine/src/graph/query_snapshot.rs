//! Content-verified input universe for paged queries, independent of graph trust windows.
use super::{Graph, Retention};
use crate::{ContentId, Engine, EngineError, LanguageId, RelPath};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct QuerySnapshot {
    inventory: Vec<RelPath>,
    sources: BTreeMap<RelPath, ContentId>,
    languages: Vec<LanguageId>,
}

impl QuerySnapshot {
    pub(crate) fn with_versions(mut self, versions: &[crate::SourceVersion]) -> Self {
        for version in versions {
            self.sources
                .insert(version.path.clone(), version.content.clone());
        }
        self
    }
    pub(crate) fn inputs(&self) -> &BTreeMap<RelPath, ContentId> {
        &self.sources
    }
    pub(crate) fn capture(engine: &Engine) -> Result<(Graph, Self), EngineError> {
        let mut graph = Graph::new(engine.workspace().clone(), engine.languages().clone());
        graph.cancellation = engine.cancellation();
        graph.refresh(Retention::PerCall)?;
        let mut snapshot = Self {
            inventory: graph
                .walked
                .iter()
                .map(|p| graph.workspace.relative(p).into())
                .collect(),
            sources: graph
                .entries
                .iter()
                .map(|e| (e.candidate.path().into(), e.candidate.file().content_id()))
                .collect(),
            languages: engine.language_ids(),
        };
        snapshot.inventory.sort();
        snapshot.languages.sort();
        // Build all layouts now, so every later namespace uses exactly these manifests.
        for language in snapshot.languages.clone() {
            engine.check_read()?;
            let manifests: Vec<_> = graph
                .languages
                .get(&language)
                .and_then(|l| l.layout().map(|layout| layout.manifests().to_vec()))
                .unwrap_or_default();
            graph.project_build(&language);
            engine.check_read()?;
            for path in &graph.walked {
                engine.check_read()?;
                if !path
                    .file_name()
                    .is_some_and(|n| manifests.iter().any(|m| n == *m))
                {
                    continue;
                }
                let relative = graph.workspace.relative(path);
                let fresh = graph.workspace.load(path)?;
                engine.check_read()?;
                let captured = graph
                    .project_sources
                    .get(&language)
                    .and_then(|files| files.iter().find(|f| f.path() == relative));
                if captured.is_none_or(|f| f.content_id() != fresh.content_id()) {
                    return Err(EngineError::StaleQuery);
                }
                snapshot.record(relative.into(), fresh.content_id())?;
            }
        }
        // Ignore files are normally hidden from the walk. Fingerprint them explicitly;
        // a fresh inventory also observes effective ancestor/global ignore changes.
        let mut directories = BTreeSet::from([std::path::PathBuf::new()]);
        for path in &snapshot.inventory {
            let mut parent = path.parent();
            while let Some(dir) = parent {
                directories.insert(dir.to_path_buf());
                parent = dir.parent();
            }
        }
        let configs = directories
            .into_iter()
            .flat_map(|dir| [dir.join(".gitignore"), dir.join(".ignore")])
            .chain([std::path::PathBuf::from(".git/info/exclude")]);
        for path in configs {
            engine.check_read()?;
            if engine
                .workspace()
                .vfs()
                .exists(&engine.workspace().absolute(&path))
            {
                let source = engine.workspace().load(&path)?;
                snapshot.record(path.into(), source.content_id())?;
            }
        }
        Ok((graph, snapshot))
    }

    /// A file can be both source and a manifest (or serve several layouts).
    /// Its last read must never overwrite evidence of an earlier, different read.
    fn record(&mut self, path: RelPath, content: ContentId) -> Result<(), EngineError> {
        if self
            .sources
            .get(&path)
            .is_some_and(|captured| captured != &content)
        {
            return Err(EngineError::StaleQuery);
        }
        self.sources.insert(path, content);
        Ok(())
    }

    pub(crate) fn validate(&self, engine: &Engine) -> Result<(), EngineError> {
        let (_, current) = Self::capture(engine)?;
        if self != &current {
            return Err(EngineError::StaleQuery);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_source_and_manifest_must_agree_on_the_same_file_version() {
        let path = RelPath::from("package.p");
        let original = ContentId::of("original");
        let mut snapshot = QuerySnapshot {
            inventory: vec![path.clone()],
            sources: BTreeMap::from([(path.clone(), original.clone())]),
            languages: vec![],
        };
        snapshot.record(path.clone(), original.clone()).unwrap();
        assert!(matches!(
            snapshot.record(path.clone(), ContentId::of("changed")),
            Err(EngineError::StaleQuery)
        ));
        assert_eq!(snapshot.sources[&path], original);
    }
}
