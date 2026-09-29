//! Expected validation evidence derived from reviewed effects, before filesystem writes.
use crate::graph::query_snapshot::QuerySnapshot;
use crate::{ContentId, Engine, EngineError, FilePreview, RelPath};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ValidationBaseline {
    snapshot: QuerySnapshot,
    inputs: BTreeMap<RelPath, ContentId>,
    before: BTreeMap<RelPath, ContentId>,
    moves: BTreeMap<RelPath, RelPath>,
    configs: BTreeMap<RelPath, Option<ContentId>>,
}
/// Presence and entry spelling are part of move validation observations, even
/// for hidden/ignored old paths and configuration files absent at preparation.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub(super) struct MoveObservation {
    entries: BTreeMap<RelPath, Option<RelPath>>,
    configs: BTreeMap<RelPath, Option<ContentId>>,
}
impl ValidationBaseline {
    pub(crate) fn capture(
        engine: &Engine,
        snapshot: QuerySnapshot,
        files: &[FilePreview],
    ) -> Result<Self, EngineError> {
        let moves: BTreeMap<_, _> = files
            .iter()
            .filter_map(|file| {
                file.moved_to
                    .as_ref()
                    .map(|to| (file.path.clone(), to.clone()))
            })
            .collect();
        let mut baseline = Self {
            inputs: snapshot.inputs().clone(),
            snapshot,
            before: files
                .iter()
                .map(|file| (file.path.clone(), ContentId::of(&file.before)))
                .collect(),
            moves,
            configs: BTreeMap::new(),
        };
        for from in baseline.moves.keys() {
            baseline.inputs.remove(from);
        }
        for file in files {
            baseline.inputs.insert(
                file.moved_to.as_ref().unwrap_or(&file.path).clone(),
                ContentId::of(&file.after),
            );
        }
        if !baseline.moves.is_empty() {
            let mut configs = BTreeSet::from([RelPath::from(".git/info/exclude")]);
            for path in baseline.moves.keys().chain(baseline.moves.values()) {
                let mut directory = path.parent();
                while let Some(parent) = directory {
                    configs.insert(parent.join(".gitignore").into());
                    configs.insert(parent.join(".ignore").into());
                    directory = parent.parent();
                }
            }
            for path in configs {
                engine.check_read()?;
                let content = baseline.configuration(engine, &path)?;
                if let Some(content) = &content {
                    baseline.inputs.insert(path.clone(), content.clone());
                }
                baseline.configs.insert(path, content);
            }
        }
        baseline.validate_before(engine)?;
        Ok(baseline)
    }
    fn configuration(
        &self,
        engine: &Engine,
        path: &RelPath,
    ) -> Result<Option<ContentId>, EngineError> {
        let absolute = engine.workspace().absolute(path);
        if engine.workspace().vfs().entry_kind(&absolute)?.is_none() {
            return Ok(None);
        }
        Ok(Some(ContentId::of_bytes(
            &engine.workspace().vfs().read_bytes(&absolute)?,
        )))
    }
    fn validate_configs(&self, engine: &Engine) -> Result<(), EngineError> {
        for (path, expected) in &self.configs {
            engine.check_read()?;
            if &self.configuration(engine, path)? != expected {
                return Err(EngineError::StalePlan);
            }
        }
        Ok(())
    }
    pub(crate) fn validate_before(&self, engine: &Engine) -> Result<(), EngineError> {
        self.snapshot.validate(engine)?;
        self.validate_configs(engine)?;
        for (path, content) in &self.before {
            engine.check_read()?;
            if ContentId::of_bytes(
                &engine
                    .workspace()
                    .vfs()
                    .read_bytes(&engine.workspace().absolute(path))?,
            ) != *content
            {
                return Err(EngineError::StalePlan);
            }
        }
        Ok(())
    }
    pub(super) fn observe(&self, engine: &Engine) -> Result<Option<MoveObservation>, EngineError> {
        if self.moves.is_empty() {
            return Ok(None);
        }
        let mut entries = BTreeMap::new();
        for path in self.moves.keys().chain(self.moves.values()) {
            engine.check_read()?;
            let stored = engine
                .workspace()
                .vfs()
                .entry_path(&engine.workspace().absolute(path))?;
            entries.insert(
                path.clone(),
                stored.map(|path| engine.workspace().relative(&path).into()),
            );
        }
        let mut configs = BTreeMap::new();
        for path in self.configs.keys() {
            engine.check_read()?;
            configs.insert(path.clone(), self.configuration(engine, path)?);
        }
        Ok(Some(MoveObservation { entries, configs }))
    }
    pub(super) fn matches(&self, observation: Option<&MoveObservation>) -> bool {
        if self.moves.is_empty() {
            return observation.is_none();
        }
        let Some(observation) = observation else {
            return false;
        };
        observation.configs == self.configs
            && self.moves.iter().all(|(from, to)| {
                observation.entries.get(to) == Some(&Some(to.clone()))
                    && observation
                        .entries
                        .get(from)
                        .is_some_and(|entry| entry.is_none() || entry.as_ref() == Some(to))
            })
    }
    pub(crate) fn inputs(&self) -> &BTreeMap<RelPath, ContentId> {
        &self.inputs
    }
    pub(crate) fn validate(&self, engine: &Engine) -> Result<(), EngineError> {
        if self.moves.is_empty() {
            let versions = self
                .inputs
                .iter()
                .map(|(path, content)| crate::SourceVersion {
                    path: path.clone(),
                    content: content.clone(),
                })
                .collect::<Vec<_>>();
            return self
                .snapshot
                .clone()
                .with_versions(&versions)
                .validate(engine);
        }
        self.validate_configs(engine)?;
        let (_, current) = QuerySnapshot::capture(engine)?;
        let affected: BTreeSet<_> = self.moves.keys().chain(self.moves.values()).collect();
        let inventory = |snapshot: &QuerySnapshot| {
            snapshot
                .inventory()
                .iter()
                .filter(|path| !affected.contains(path))
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        if self.snapshot.languages() != current.languages()
            || inventory(&self.snapshot) != inventory(&current)
        {
            return Err(EngineError::StalePlan);
        }
        for (from, to) in &self.moves {
            engine.check_read()?;
            // On insensitive filesystems the old spelling may resolve to the new
            // entry. A separate entry recreated at the old path is still stale.
            if let Some(stored) = engine
                .workspace()
                .vfs()
                .entry_path(&engine.workspace().absolute(from))?
                && stored != engine.workspace().absolute(to)
            {
                return Err(EngineError::StalePlan);
            }
            let stored = engine
                .workspace()
                .vfs()
                .entry_path(&engine.workspace().absolute(to))?;
            if stored.as_ref() != Some(&engine.workspace().absolute(to)) {
                return Err(EngineError::StalePlan);
            }
        }
        for (path, expected) in &self.inputs {
            engine.check_read()?;
            let content = ContentId::of_bytes(
                &engine
                    .workspace()
                    .vfs()
                    .read_bytes(&engine.workspace().absolute(path))?,
            );
            if &content != expected {
                return Err(EngineError::StalePlan);
            }
        }
        self.validate_configs(engine)?;
        Ok(())
    }
}
