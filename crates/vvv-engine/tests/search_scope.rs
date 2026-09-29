mod common;
use common::Fake;
use std::{path::Path, sync::Arc};
use vvv_engine::*;

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
}
impl Fixture {
    fn new() -> Self {
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "root")
                .with_file("/ws/src/a.p", "def Engine\nEngine")
                .with_file("/ws/src/b.p", "def Engine\nEngine")
                .with_file("/ws/src-old/a.p", "def Engine")
                .with_file("/ws/src/nested/package", "nested")
                .with_file("/ws/src/nested/a.p", "def Engine"),
        );
        Self {
            engine: Engine::new(
                Workspace::new("/ws", vfs.clone()),
                Languages::new().with(Fake::default()),
            ),
            vfs,
        }
    }
    fn scope(paths: &[&str], packages: &[&str]) -> SearchScope {
        SearchScope {
            paths: paths.iter().map(|p| (*p).into()).collect(),
            packages: packages.iter().map(|p| (*p).into()).collect(),
        }
    }
    fn search(&self, scope: SearchScope) -> Search {
        SearchQuery::from(Query::named("Engine"))
            .scoped(scope)
            .execute(&self.engine)
            .unwrap()
    }
}
#[test]
fn paths_match_components_and_packages_use_the_deepest_owner() {
    let f = Fixture::new();
    let all = f.search(Fixture::scope(&[], &[]));
    assert_eq!(all.matches.len(), 4);
    let within = f.search(Fixture::scope(&["src"], &["root"]));
    assert_eq!(
        within
            .matches
            .iter()
            .map(|m| m.path.as_path())
            .collect::<Vec<_>>(),
        [Path::new("src/a.p"), Path::new("src/b.p")]
    );
    assert_eq!(
        f.search(Fixture::scope(&["src/a.p", "src-old"], &[]))
            .matches
            .len(),
        2
    );
    assert_eq!(
        f.search(Fixture::scope(&["./src"], &["root", "nested"]))
            .matches
            .len(),
        3
    );
    assert_eq!(f.search(Fixture::scope(&["."], &[])).matches.len(), 4);
    assert_eq!(
        f.search(Fixture::scope(&[], &["nested"])).matches[0]
            .path
            .as_path(),
        Path::new("src/nested/a.p")
    );
    assert!(
        f.search(Fixture::scope(&[], &["missing"]))
            .matches
            .is_empty()
    );
}
#[test]
fn scoped_pages_keep_the_filter_ordinals_and_replay_after_budget_changes() {
    let f = Fixture::new();
    let scope = Fixture::scope(&["src"], &["root"]);
    let page = SearchPageQuery {
        query: Query::pattern("Engine"),
        scope: scope.clone(),
        page: PageBudget {
            max_items: 1,
            ..Default::default()
        },
    }
    .execute(&f.engine)
    .unwrap();
    assert_eq!(page.scope, scope);
    assert_eq!(page.total_items, 4);
    assert_eq!(page.items[0].ordinal, 1);
    let next = ContinueQuery {
        cursor: page.next_cursor.unwrap(),
        page: PageBudget {
            max_items: 3,
            max_bytes: 8192,
        },
        work: None,
    };
    let reply = next.clone().execute(&f.engine).unwrap();
    let PageReply::Search(page) = &reply else {
        panic!()
    };
    assert_eq!(page.scope, scope);
    assert_eq!(
        page.items.iter().map(|i| i.ordinal).collect::<Vec<_>>(),
        [2, 3, 4]
    );
    assert_eq!(
        serde_json::to_value(&reply).unwrap(),
        serde_json::to_value(next.clone().execute(&f.engine).unwrap()).unwrap()
    );
    f.vfs
        .write(Path::new("/ws/src/nested/package"), "renamed")
        .unwrap();
    assert!(matches!(
        next.execute(&f.engine),
        Err(EngineError::StaleQuery)
    ));
}
#[test]
fn invalid_scopes_are_rejected_and_json_search_retains_existing_predicates() {
    let f = Fixture::new();
    for path in ["../src", "/src", "a/../b", "a\\b", "C:/src"] {
        assert!(matches!(
            SearchQuery::from(Query::pattern("Engine"))
                .scoped(Fixture::scope(&[path], &[]))
                .execute(&f.engine),
            Err(EngineError::InvalidSearchScope)
        ));
    }
    let call: Call = serde_json::from_value(serde_json::json!({"command":"search","name":"Engine","scope":{"paths":["src"],"packages":["root"]}})).unwrap();
    let reply = serde_json::to_value(call.execute(&f.engine)).unwrap();
    assert_eq!(reply["result"]["matches"].as_array().unwrap().len(), 2);
}
