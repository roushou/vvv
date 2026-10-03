//! Shared delivery contracts and dispatch over the closed set of retained query kinds.
use super::{context::ContextPage, search::SearchPage};
use crate::graph::{Graph, query_snapshot::QuerySnapshot};
use crate::query_store::{Checkpoint, QueryData, QueryRoot};
use crate::{ContentId, Cursor, Engine, EngineError, SnapshotId};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Instant};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct PageBudget {
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 64)))]
    pub max_items: usize,
    #[cfg_attr(feature = "schema", schemars(range(min = 1024, max = 1048576)))]
    pub max_bytes: usize,
}
impl Default for PageBudget {
    fn default() -> Self {
        Self {
            max_items: 20,
            max_bytes: 16_384,
        }
    }
}
impl PageBudget {
    pub const MAXIMUM: Self = Self {
        max_items: 64,
        max_bytes: 1_048_576,
    };
    pub fn validate(&self) -> Result<(), EngineError> {
        if !(1..=Self::MAXIMUM.max_items).contains(&self.max_items)
            || !(1024..=Self::MAXIMUM.max_bytes).contains(&self.max_bytes)
        {
            return Err(EngineError::InvalidBudget);
        }
        Ok(())
    }
    pub(crate) fn check(&self, value: &impl Serialize) -> Result<(), EngineError> {
        let required_bytes = serde_json::to_vec(value).expect("page serializes").len();
        if required_bytes > self.max_bytes {
            return Err(EngineError::OutputLimit {
                max_bytes: self.max_bytes,
                required_bytes,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default, deny_unknown_fields)]
pub struct WorkBudget {
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 512)))]
    pub max_lookups: usize,
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 1024)))]
    pub max_files: usize,
}
impl Default for WorkBudget {
    fn default() -> Self {
        Self {
            max_lookups: 64,
            max_files: 64,
        }
    }
}
impl WorkBudget {
    pub const MAXIMUM: Self = Self {
        max_lookups: 512,
        max_files: 1024,
    };
    pub fn validate(&self) -> Result<(), EngineError> {
        if !(1..=512).contains(&self.max_lookups) || !(1..=1024).contains(&self.max_files) {
            return Err(EngineError::InvalidBudget);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum PageReply {
    Search(SearchPage),
    Context(ContextPage),
    Relationships(super::relationships::Relationships),
}
impl PageReply {
    pub fn next_cursor(&self) -> Option<&Cursor> {
        match self {
            Self::Search(p) => p.next_cursor.as_ref(),
            Self::Context(p) => p.next_cursor.as_ref(),
            Self::Relationships(p) => p.next_cursor.as_ref(),
        }
    }
    pub fn snapshot(&self) -> &SnapshotId {
        match self {
            Self::Search(p) => &p.snapshot,
            Self::Context(p) => &p.snapshot,
            Self::Relationships(p) => &p.snapshot,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ContinueQuery {
    pub cursor: Cursor,
    #[serde(default)]
    pub page: PageBudget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<WorkBudget>,
}
impl ContinueQuery {
    pub fn execute(self, engine: &Engine) -> Result<PageReply, EngineError> {
        let _operation = engine.operation();
        self.execute_in(engine)
    }
    pub(crate) fn execute_in(self, engine: &Engine) -> Result<PageReply, EngineError> {
        self.page.validate()?;
        if let Some(work) = &self.work {
            work.validate()?;
        }
        let (mut session, checkpoint, mut graph) =
            PageSession::resume(engine, &self.cursor, false)?;
        let result = match checkpoint {
            Checkpoint::Search(state) => {
                if self.work.is_some() {
                    return Err(EngineError::InvalidBudget);
                }
                state.page(engine, &mut session, self.page)
            }
            Checkpoint::Context(state) => state.page(
                engine,
                &mut graph,
                &mut session,
                self.page,
                self.work.unwrap_or_default(),
            ),
            Checkpoint::Relationships(state) => {
                let work = self.work.unwrap_or_default();
                state.page(
                    engine,
                    &mut graph,
                    &mut session,
                    super::relationships::RelationshipBudget {
                        max_items: self.page.max_items,
                        max_bytes: self.page.max_bytes,
                        max_lookups: work.max_lookups,
                        max_files: work.max_files,
                    },
                )
            }
            Checkpoint::Excerpt(_) => Err(EngineError::InvalidCursor),
        };
        session.finish(engine, result)
    }
}

pub(crate) struct PageSession {
    pub id: u64,
    pub root: Arc<QueryRoot>,
    checkpoints: Vec<Checkpoint>,
}
impl PageSession {
    pub(crate) fn new(
        engine: &Engine,
        snapshot: QuerySnapshot,
        query: &impl Serialize,
        data: QueryData,
    ) -> Self {
        let identity =
            ContentId::of(&serde_json::to_string(&(&snapshot, query)).expect("query serializes"))
                .into();
        Self {
            id: engine.queries().allocate(),
            root: Arc::new(QueryRoot {
                snapshot,
                identity,
                data,
                revision: engine.query_revision(),
                created: Instant::now(),
            }),
            checkpoints: vec![],
        }
    }
    pub(crate) fn resume(
        engine: &Engine,
        cursor: &Cursor,
        excerpt: bool,
    ) -> Result<(Self, Checkpoint, Graph), EngineError> {
        let lease =
            engine
                .queries()
                .lease(cursor, excerpt, engine.query_revision(), Instant::now())?;
        let (graph, snapshot) = match QuerySnapshot::capture(engine) {
            Err(EngineError::StaleQuery | EngineError::StaleSource { .. }) => {
                engine.check_read()?;
                engine.queries().invalidate(lease.id);
                return Err(EngineError::StaleQuery);
            }
            other => other?,
        };
        engine.check_read()?;
        if snapshot != lease.root.snapshot {
            engine.queries().invalidate(lease.id);
            return Err(EngineError::StaleQuery);
        }
        Ok((
            Self {
                id: lease.id,
                root: lease.root,
                checkpoints: vec![],
            },
            lease.checkpoint,
            graph,
        ))
    }
    pub(crate) fn token(&self, engine: &Engine, checkpoint: &Checkpoint) -> Cursor {
        engine.queries().cursor(self.id, checkpoint)
    }
    pub(crate) fn retain(&mut self, checkpoint: Checkpoint) {
        self.checkpoints.push(checkpoint);
    }
    pub(crate) fn finish<T>(
        self,
        engine: &Engine,
        result: Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let result = result.and_then(|answer| {
            self.root.snapshot.validate(engine)?;
            if self.root.revision != engine.query_revision() {
                return Err(EngineError::StaleQuery);
            }
            Ok(answer)
        });
        engine.check_read()?;
        match result {
            Err(EngineError::StaleQuery | EngineError::StaleSource { .. }) => {
                engine.queries().invalidate(self.id);
                Err(EngineError::StaleQuery)
            }
            Err(error) => Err(error),
            Ok(answer) => {
                engine.publish_read(|| {
                    engine
                        .queries()
                        .publish(self.id, self.root, self.checkpoints, Instant::now())
                })?;
                Ok(answer)
            }
        }
    }
}

impl crate::report::Document {
    pub(crate) fn page(reply: &PageReply) -> Self {
        match reply {
            PageReply::Search(page) => Self::search_page(page),
            PageReply::Context(page) => Self::context_page(page),
            PageReply::Relationships(page) => Self::relationships(page),
        }
    }
}
