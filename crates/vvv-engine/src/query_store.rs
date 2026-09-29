//! Immutable query checkpoints, bounded as whole query trees. Lock after operation exclusion.
use crate::capabilities::{
    context::{ContextSeed, ContextSession},
    excerpts::Excerpt,
    search::SearchSession,
};
use crate::graph::query_snapshot::QuerySnapshot;
use crate::{Cursor, EngineError, Search, SnapshotId};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::hash::BuildHasher;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Serialize, serde::Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QueryLimits {
    pub max_queries: usize,
    pub max_query_bytes: usize,
    pub max_total_bytes: usize,
    pub lifetime_seconds: u64,
}
impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            max_queries: 16,
            max_query_bytes: 32 * 1024 * 1024,
            max_total_bytes: 128 * 1024 * 1024,
            lifetime_seconds: 600,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) enum QueryData {
    Search(Search),
    Context(ContextSeed),
}
#[derive(Debug, Clone, Serialize)]
pub(crate) enum Checkpoint {
    Search(SearchSession),
    Context(ContextSession),
    Excerpt(Excerpt),
}
impl Checkpoint {
    fn kind(&self) -> &'static str {
        if matches!(self, Self::Excerpt(_)) {
            "e"
        } else {
            "q"
        }
    }
}
#[derive(Debug, Serialize)]
pub(crate) struct QueryRoot {
    pub snapshot: QuerySnapshot,
    pub identity: SnapshotId,
    pub data: QueryData,
    pub revision: u64,
    #[serde(skip)]
    pub created: Instant,
}
#[derive(Clone)]
pub(crate) struct Lease {
    pub id: u64,
    pub root: Arc<QueryRoot>,
    pub checkpoint: Checkpoint,
}
struct StoredQuery {
    root: Arc<QueryRoot>,
    checkpoints: BTreeMap<Cursor, Checkpoint>,
    created: Instant,
    used: Instant,
    bytes: usize,
}
pub(crate) struct QueryStore {
    nonce: String,
    next: u64,
    roots: HashMap<u64, StoredQuery>,
    limits: QueryLimits,
}
impl Default for QueryStore {
    fn default() -> Self {
        Self {
            nonce: format!(
                "{:016x}",
                std::collections::hash_map::RandomState::new().hash_one("vvv query store")
            ),
            next: 0,
            roots: HashMap::new(),
            limits: QueryLimits::default(),
        }
    }
}
impl QueryStore {
    // Conservative charge includes allocation, map nodes and token overhead. Shared
    // query data is charged once; checkpoints contain no complete source documents.
    pub(crate) fn weight(value: &impl Serialize) -> usize {
        serde_json::to_vec(value)
            .expect("query state serializes")
            .len()
            .saturating_mul(4)
            .saturating_add(1024)
    }
    pub(crate) fn allocate(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
    pub(crate) fn cursor(&self, id: u64, checkpoint: &Checkpoint) -> Cursor {
        let digest = blake3::hash(&serde_json::to_vec(checkpoint).expect("checkpoint serializes"));
        Cursor(format!(
            "v1.{}.{id:016x}.{}.{}",
            self.nonce,
            checkpoint.kind(),
            digest.to_hex()
        ))
    }
    pub(crate) fn lease(
        &mut self,
        cursor: &Cursor,
        excerpt: bool,
        revision: u64,
        now: Instant,
    ) -> Result<Lease, EngineError> {
        let parts: Vec<_> = cursor.0.split('.').collect();
        if parts.len() != 5
            || parts[0] != "v1"
            || parts[1].len() != 16
            || parts[2].len() != 16
            || parts[4].len() != 64
            || ![parts[1], parts[2], parts[4]]
                .iter()
                .all(|s| s.bytes().all(|c| c.is_ascii_hexdigit()))
            || !["q", "e"].contains(&parts[3])
        {
            return Err(EngineError::InvalidCursor);
        }
        if (parts[3] == "e") != excerpt {
            return Err(EngineError::InvalidCursor);
        }
        if parts[1] != self.nonce {
            return Err(EngineError::CursorExpired);
        }
        let id = u64::from_str_radix(parts[2], 16).map_err(|_| EngineError::InvalidCursor)?;
        self.expire(now);
        let root = self.roots.get_mut(&id).ok_or(EngineError::CursorExpired)?;
        if root.root.revision != revision {
            self.roots.remove(&id);
            return Err(EngineError::StaleQuery);
        }
        let checkpoint = root
            .checkpoints
            .get(cursor)
            .ok_or(EngineError::InvalidCursor)?
            .clone();
        root.used = now;
        Ok(Lease {
            id,
            root: root.root.clone(),
            checkpoint,
        })
    }
    fn expire(&mut self, now: Instant) {
        let ttl = Duration::from_secs(self.limits.lifetime_seconds);
        self.roots
            .retain(|_, root| now.saturating_duration_since(root.created) < ttl);
    }
    pub(crate) fn invalidate(&mut self, id: u64) {
        self.roots.remove(&id);
    }
    pub(crate) fn publish(
        &mut self,
        id: u64,
        root: Arc<QueryRoot>,
        checkpoints: Vec<Checkpoint>,
        now: Instant,
    ) -> Result<(), EngineError> {
        if now.saturating_duration_since(root.created)
            >= Duration::from_secs(self.limits.lifetime_seconds)
        {
            self.invalidate(id);
            return Err(EngineError::CursorExpired);
        }
        if checkpoints.is_empty() {
            return Ok(());
        }
        self.expire(now);
        let existing = self.roots.get(&id);
        let mut additions = BTreeMap::new();
        for checkpoint in checkpoints {
            let cursor = self.cursor(id, &checkpoint);
            if existing.is_none_or(|q| !q.checkpoints.contains_key(&cursor)) {
                additions.insert(cursor, checkpoint);
            }
        }
        let old_bytes = existing.map_or(0, |q| q.bytes);
        let added_bytes = additions
            .iter()
            .map(|(k, v)| Self::weight(&(k, v)))
            .sum::<usize>()
            + if existing.is_none() {
                Self::weight(root.as_ref())
            } else {
                0
            };
        let bytes = old_bytes.saturating_add(added_bytes);
        if bytes > self.limits.max_query_bytes || bytes > self.limits.max_total_bytes {
            return Err(EngineError::RetentionLimit);
        }
        while self
            .roots
            .values()
            .map(|q| q.bytes)
            .sum::<usize>()
            .saturating_add(added_bytes)
            > self.limits.max_total_bytes
            || (!self.roots.contains_key(&id) && self.roots.len() >= self.limits.max_queries)
        {
            let oldest = self
                .roots
                .iter()
                .filter(|(other, _)| **other != id)
                .min_by_key(|(key, q)| (q.used, **key))
                .map(|(key, _)| *key)
                .ok_or(EngineError::RetentionLimit)?;
            self.roots.remove(&oldest);
        }
        let entry = self.roots.entry(id).or_insert_with(|| StoredQuery {
            created: root.created,
            root,
            checkpoints: BTreeMap::new(),
            used: now,
            bytes: 0,
        });
        entry.checkpoints.extend(additions);
        entry.bytes = bytes;
        entry.used = now;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        store: QueryStore,
        root: Arc<QueryRoot>,
        now: Instant,
    }
    impl Fixture {
        fn new() -> Self {
            let now = Instant::now();
            let engine = crate::Engine::new(
                crate::Workspace::new("/ws", Arc::new(crate::MemoryVfs::new())),
                crate::Languages::new(),
            );
            let (_, snapshot) = QuerySnapshot::capture(&engine).unwrap();
            let root = Arc::new(QueryRoot {
                snapshot,
                identity: crate::ContentId::of("query").into(),
                data: QueryData::Search(Search {
                    query: crate::Query::pattern("x"),
                    matches: vec![],
                    skipped: vec![],
                }),
                revision: 0,
                created: now,
            });
            Self {
                store: QueryStore::default(),
                root,
                now,
            }
        }
        fn add(&mut self) -> (u64, Cursor) {
            let id = self.store.allocate();
            let checkpoint = Checkpoint::Search(SearchSession { next: 1 });
            let token = self.store.cursor(id, &checkpoint);
            self.store
                .publish(id, self.root.clone(), vec![checkpoint], self.now)
                .unwrap();
            (id, token)
        }
    }
    #[test]
    fn fixed_lifetime_is_not_extended_by_reads_or_checkpoint_publication() {
        let mut f = Fixture::new();
        let (id, token) = f.add();
        let later = f.now + Duration::from_secs(599);
        f.store.lease(&token, false, 0, later).unwrap();
        f.store
            .publish(
                id,
                f.root.clone(),
                vec![Checkpoint::Search(SearchSession { next: 2 })],
                later,
            )
            .unwrap();
        let expired = f.now + Duration::from_secs(600);
        assert!(matches!(
            f.store.lease(&token, false, 0, expired),
            Err(EngineError::CursorExpired)
        ));
        assert!(matches!(
            f.store.publish(
                id,
                f.root,
                vec![Checkpoint::Search(SearchSession { next: 3 })],
                expired
            ),
            Err(EngineError::CursorExpired)
        ));
    }
    #[test]
    fn lru_evicts_whole_roots_and_replay_does_not_grow_retention() {
        let mut f = Fixture::new();
        f.store.limits.max_queries = 2;
        let (id, token) = f.add();
        let (_, old) = f.add();
        let later = f.now + Duration::from_secs(1);
        f.store.lease(&token, false, 0, later).unwrap();
        let before = f.store.roots[&id].bytes;
        f.store
            .publish(
                id,
                f.root.clone(),
                vec![Checkpoint::Search(SearchSession { next: 1 })],
                later,
            )
            .unwrap();
        assert_eq!(f.store.roots[&id].bytes, before);
        f.add();
        assert!(matches!(
            f.store.lease(&old, false, 0, later),
            Err(EngineError::CursorExpired)
        ));
        f.store.lease(&token, false, 0, later).unwrap();
    }
    #[test]
    fn failed_admission_keeps_existing_checkpoint_and_bounds_total_bytes() {
        let mut f = Fixture::new();
        let (id, token) = f.add();
        let retained = f.store.roots[&id].bytes;
        f.store.limits.max_query_bytes = retained;
        assert!(matches!(
            f.store.publish(
                id,
                f.root.clone(),
                vec![Checkpoint::Search(SearchSession { next: 2 })],
                f.now
            ),
            Err(EngineError::RetentionLimit)
        ));
        assert_eq!(f.store.roots[&id].bytes, retained);
        f.store.lease(&token, false, 0, f.now).unwrap();
        f.store.limits.max_total_bytes = retained;
        let (_, replacement) = f.add();
        assert!(matches!(
            f.store.lease(&token, false, 0, f.now),
            Err(EngineError::CursorExpired)
        ));
        f.store.lease(&replacement, false, 0, f.now).unwrap();
        assert!(f.store.roots.values().map(|q| q.bytes).sum::<usize>() <= retained);
    }
}
