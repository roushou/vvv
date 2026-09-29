mod common;
use common::Fake;
use std::{path::Path, sync::Arc, time::Duration};
use vvv_engine::*;

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
    source: String,
}
impl Fixture {
    fn search() -> Self {
        let source = "def Foo\nFoo Foo Foo Foo Foo Foo\n".to_owned();
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/a.p", &source)
                .with_file("/ws/b.p", "nothing"),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default()),
        )
        .with_retention(Retention::session().trusting(Duration::from_secs(3600)));
        Self {
            engine,
            vfs,
            source,
        }
    }
    fn context(tail: &str) -> Self {
        let source = format!("def Helper\ndef Root Helper Helper {tail}");
        let symbols = vec![
            Symbol::plain(
                SymbolKind::Function,
                "Helper",
                Span::new(4, 10),
                Span::new(0, 10),
            ),
            Symbol::plain(
                SymbolKind::Function,
                "Root",
                Span::new(15, 19),
                Span::new(11, source.len()),
            ),
        ];
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/a.p", &source),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default().with_symbols(symbols)),
        );
        Self {
            engine,
            vfs,
            source,
        }
    }
    fn page(&self, count: usize) -> PageReply {
        PageReply::Search(
            SearchPageQuery {
                scope: Default::default(),
                query: Query::pattern("Foo"),
                page: PageBudget {
                    max_items: count,
                    ..Default::default()
                },
            }
            .execute(&self.engine)
            .unwrap(),
        )
    }
    fn continue_query(&self, cursor: Cursor, count: usize) -> ContinueQuery {
        ContinueQuery {
            cursor,
            page: PageBudget {
                max_items: count,
                ..Default::default()
            },
            work: None,
        }
    }
    fn context_query(&self, references: bool, bytes: usize) -> ContextPageQuery {
        ContextPageQuery {
            detail: vvv_engine::ContextDetail::Body,
            include_enclosing: false,
            origin: NavigationQuery::at("a.p", Position::new(1, 4)).origin,
            selection: Selection::All,
            references,
            page: PageBudget {
                max_items: 1,
                max_bytes: bytes,
            },
            work: WorkBudget {
                max_lookups: 1,
                max_files: 1,
            },
        }
    }
}
#[test]
fn search_pages_equal_unpaged_order_and_retry_across_engine_clones() {
    let f = Fixture::search();
    let expected = SearchQuery::from(Query::pattern("Foo"))
        .execute(&f.engine)
        .unwrap();
    let first = f.page(2);
    let snapshot = first.snapshot().clone();
    let cursor = first.next_cursor().unwrap().clone();
    let query = f.continue_query(cursor.clone(), 2);
    let a = query.clone().execute(&f.engine).unwrap();
    let b = query.execute(&f.engine.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    let PageReply::Search(wider) = f.continue_query(cursor, 3).execute(&f.engine).unwrap() else {
        panic!()
    };
    assert_eq!(wider.items[0].ordinal, 3);
    assert_eq!(wider.items.len(), 3);
    let mut next = Some(first);
    let mut items = vec![];
    while let Some(reply) = next.take() {
        assert_eq!(reply.snapshot(), &snapshot);
        next = reply
            .next_cursor()
            .map(|c| f.continue_query(c.clone(), 2).execute(&f.engine).unwrap());
        let PageReply::Search(page) = reply else {
            panic!()
        };
        items.extend(page.items);
    }
    assert_eq!(
        items.iter().map(|i| i.ordinal).collect::<Vec<_>>(),
        (1..=expected.matches.len()).collect::<Vec<_>>()
    );
    assert_eq!(
        items.into_iter().map(|i| i.item).collect::<Vec<_>>(),
        expected.matches
    );
}
#[test]
fn all_input_changes_invalidate_even_unmatched_files_and_trusted_sessions() {
    for (path, body) in [
        ("/ws/b.p", "Foo____"),
        ("/ws/new.p", "Foo"),
        ("/ws/package", "renamed"),
        ("/ws/.gitignore", "b.p"),
    ] {
        let f = Fixture::search();
        let cursor = f.page(1).next_cursor().unwrap().clone();
        f.vfs.write(Path::new(path), body).unwrap();
        assert!(
            matches!(
                f.continue_query(cursor.clone(), 1).execute(&f.engine),
                Err(EngineError::StaleQuery)
            ),
            "{path}"
        );
        assert!(matches!(
            f.continue_query(cursor, 1).execute(&f.engine),
            Err(EngineError::CursorExpired)
        ));
    }
    let f = Fixture::search();
    let cursor = f.page(1).next_cursor().unwrap().clone();
    f.engine.touched();
    assert!(matches!(
        f.continue_query(cursor, 1).execute(&f.engine),
        Err(EngineError::StaleQuery)
    ));
}
#[test]
fn malformed_foreign_and_wrong_kind_tokens_are_typed_and_non_consuming() {
    let f = Fixture::search();
    let cursor = f.page(1).next_cursor().unwrap().clone();
    assert!(matches!(
        ExpandQuery {
            cursor: cursor.clone(),
            max_bytes: 2048
        }
        .execute(&f.engine),
        Err(EngineError::InvalidCursor)
    ));
    assert!(matches!(
        f.continue_query(cursor.clone(), 1)
            .execute(&Fixture::search().engine),
        Err(EngineError::CursorExpired)
    ));
    let malformed = serde_json::from_value(serde_json::json!("nonsense")).unwrap();
    assert!(matches!(
        f.continue_query(malformed, 1).execute(&f.engine),
        Err(EngineError::InvalidCursor)
    ));
    let mut query = f.continue_query(cursor.clone(), 1);
    query.work = Some(WorkBudget::default());
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::InvalidBudget)
    ));
    f.continue_query(cursor, 1).execute(&f.engine).unwrap();
}
#[test]
fn context_resumes_each_frontier_with_first_evidence_and_bounded_work() {
    let f = Fixture::context("body");
    let query = f.context_query(true, 4096);
    let expected = ContextQuery {
        detail: vvv_engine::ContextDetail::Body,
        include_enclosing: false,
        origin: query.origin.clone(),
        selection: Selection::All,
        references: true,
        budget: ContextBudget::MAXIMUM,
    }
    .execute(&f.engine)
    .unwrap();
    let first = PageReply::Context(query.execute(&f.engine).unwrap());
    let snapshot = first.snapshot().clone();
    let mut next = Some(first);
    let mut items = vec![];
    let mut pages = 0;
    let mut lookups = 0;
    while let Some(reply) = next.take() {
        pages += 1;
        assert!(pages < 30);
        assert_eq!(reply.snapshot(), &snapshot);
        let PageReply::Context(page) = reply else {
            panic!()
        };
        assert!(page.work.lookups <= 1 && page.work.files <= 1);
        lookups += page.work.lookups;
        assert_eq!(page.traversal_complete, page.next_cursor.is_none());
        if page.items.is_empty() && page.next_cursor.is_some() {
            assert!(page.work.lookups + page.work.files > 0);
        }
        if let Some(cursor) = page.next_cursor {
            next = Some(
                ContinueQuery {
                    cursor,
                    page: PageBudget {
                        max_items: 1,
                        max_bytes: 4096,
                    },
                    work: Some(WorkBudget {
                        max_lookups: 1,
                        max_files: 1,
                    }),
                }
                .execute(&f.engine)
                .unwrap(),
            );
        }
        items.extend(page.items.into_iter().map(|p| p.item));
    }
    assert_eq!(items, expected.items);
    // Fake exposes `def`, both Helper occurrences and `body` as identifiers.
    // Each is looked up once; incoming Root's own declaration is skipped.
    assert_eq!(lookups, 4);
}
#[test]
fn expansion_reconstructs_exact_unicode_and_escaped_source_independent_of_traversal() {
    let f = Fixture::context(&"é\"\\\r\n🙂".repeat(300));
    let first = PageReply::Context(f.context_query(false, 2048).execute(&f.engine).unwrap());
    let PageReply::Context(page) = first else {
        panic!()
    };
    let item = &page.items[0];
    assert!(!item.item.complete);
    let mut text = item.item.text.clone();
    let mut offset = item.item.excerpt.span.end;
    let mut cursor = item.expansion.clone();
    let mut count = 0;
    while let Some(token) = cursor.take() {
        count += 1;
        assert!(count < 50);
        assert!(matches!(
            f.continue_query(token.clone(), 1).execute(&f.engine),
            Err(EngineError::InvalidCursor)
        ));
        let query = ExpandQuery {
            cursor: token,
            max_bytes: 2048,
        };
        let chunk = query.clone().execute(&f.engine).unwrap();
        assert_eq!(
            serde_json::to_value(&chunk).unwrap(),
            serde_json::to_value(query.execute(&f.engine).unwrap()).unwrap()
        );
        assert!(serde_json::to_vec(&chunk).unwrap().len() <= 2048);
        assert_eq!(chunk.snapshot, page.snapshot);
        assert_eq!(chunk.excerpt.span.start, offset);
        let prefix = &f.source[..offset];
        assert_eq!(chunk.start.line as usize, prefix.matches('\n').count());
        assert_eq!(
            chunk.start.column as usize,
            prefix.rsplit('\n').next().unwrap().chars().count()
        );
        assert_eq!(chunk.requested, item.item.target.declaration);
        assert_eq!(
            chunk.text,
            f.source[chunk.excerpt.span.start..chunk.excerpt.span.end]
        );
        assert!(!chunk.text.is_empty());
        assert_eq!(chunk.done, chunk.next_cursor.is_none());
        offset = chunk.excerpt.span.end;
        text.push_str(&chunk.text);
        cursor = chunk.next_cursor;
    }
    assert_eq!(text, f.source[11..]);
    assert_eq!(offset, f.source.len());
    f.continue_query(page.next_cursor.unwrap(), 1)
        .execute(&f.engine)
        .unwrap();
}
#[test]
fn outer_byte_limit_is_applied_before_publishing_and_oversized_matches_are_indivisible() {
    let f = Fixture::search();
    let call: Call = serde_json::from_value(serde_json::json!({"command":"search_page", "query":{"pattern":"Foo"}, "page":{"max_items":64,"max_bytes":16384}, "max_output_bytes":1024})).unwrap();
    let value = serde_json::to_value(call.execute(&f.engine)).unwrap();
    assert!(serde_json::to_vec(&value["result"]).unwrap().len() <= 1024);
    assert!(value["result"]["next_cursor"].is_string());
    let source = format!("Foo:{}", "x".repeat(4000));
    f.vfs.write(Path::new("/ws/a.p"), &source).unwrap();
    let query = SearchPageQuery {
        scope: Default::default(),
        query: Query::pattern("Foo"),
        page: PageBudget {
            max_items: 1,
            max_bytes: 1024,
        },
    };
    let error = query.execute(&f.engine).unwrap_err();
    let failure = Failure::from(&error);
    assert_eq!(failure.code, ErrorCode::OutputLimit);
    assert_eq!(
        failure.continuation,
        Some(ContinuationRecovery::IncreaseBudgetOrNarrowQuery)
    );
    let details = failure.output_limit.unwrap();
    assert!(details.required_bytes > 1024);
    assert_eq!(details.anchor.unwrap().path.as_path(), Path::new("a.p"));
}

#[test]
fn deletions_and_failed_postvalidation_do_not_publish_or_consume_checkpoints() {
    use common::{FaultAction, FaultOperation, FaultVfs};
    let f = Fixture::search();
    let fault = Arc::new(FaultVfs::over(f.vfs.clone()));
    let engine = Engine::new(
        Workspace::new("/ws", fault.clone()),
        Languages::new().with(Fake::default()),
    );
    let first = SearchPageQuery {
        scope: Default::default(),
        query: Query::pattern("Foo"),
        page: PageBudget {
            max_items: 1,
            ..Default::default()
        },
    }
    .execute(&engine)
    .unwrap();
    let query = f.continue_query(first.next_cursor.unwrap(), 1);
    // One read builds the fresh graph; a second read is postvalidation.
    fault.arm(
        FaultOperation::Read,
        Path::new("/ws/b.p"),
        1,
        FaultAction::Before,
    );
    assert!(query.clone().execute(&engine).is_err());
    let retry = query.clone().execute(&engine).unwrap();
    let PageReply::Search(retry) = retry else {
        panic!()
    };
    assert_eq!(retry.items[0].ordinal, 2);
    f.vfs.remove_file(Path::new("/ws/b.p")).unwrap();
    assert!(matches!(
        query.execute(&engine),
        Err(EngineError::StaleQuery)
    ));
}
#[test]
fn output_failure_does_not_advance_a_search_cursor() {
    let f = Fixture::search();
    f.vfs
        .write(
            Path::new("/ws/a.p"),
            &format!("Foo\nFoo:{}", "x".repeat(4000)),
        )
        .unwrap();
    let first = f.page(1);
    let mut query = f.continue_query(first.next_cursor().unwrap().clone(), 1);
    query.page.max_bytes = 1024;
    let failure = query.clone().execute(&f.engine).unwrap_err();
    assert_eq!(failure.code(), ErrorCode::OutputLimit);
    let Failure {
        output_limit: Some(limit),
        ..
    } = Failure::from(&failure)
    else {
        panic!()
    };
    query.page.max_bytes = limit.required_bytes;
    let PageReply::Search(reply) = query.execute(&f.engine).unwrap() else {
        panic!()
    };
    assert_eq!(reply.items[0].ordinal, 2);
    assert_eq!(reply.items[0].item.text.len(), 4004);
}
#[test]
fn ambiguity_is_preserved_and_expansion_tokens_become_stale_with_their_query() {
    let f = Fixture::context(&"é".repeat(3000));
    let page = f.context_query(false, 2048).execute(&f.engine).unwrap();
    let expansion = page.items[0].expansion.clone().unwrap();
    f.vfs
        .write(Path::new("/ws/a.p"), &(f.source.clone() + " "))
        .unwrap();
    assert!(matches!(
        ExpandQuery {
            cursor: expansion,
            max_bytes: 2048
        }
        .execute(&f.engine),
        Err(EngineError::StaleQuery)
    ));
    assert!(matches!(
        f.continue_query(page.next_cursor.unwrap(), 1)
            .execute(&f.engine),
        Err(EngineError::CursorExpired)
    ));

    let vfs = Arc::new(MemoryVfs::new().with_file("/ws/a.p", "def Foo\ndef Foo\nFoo"));
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new().with(Fake::default()),
    );
    let query = ContextPageQuery {
        detail: vvv_engine::ContextDetail::Body,
        include_enclosing: false,
        origin: NavigationQuery::at("a.p", Position::new(2, 0)).origin,
        selection: Selection::All,
        references: false,
        page: PageBudget::default(),
        work: WorkBudget::default(),
    };
    let page = query.clone().execute(&engine).unwrap();
    let ContextOutcome::Ambiguous { candidates } = page.outcome else {
        panic!()
    };
    assert_eq!(candidates.len(), 2);
    assert!(page.traversal_complete && page.next_cursor.is_none());
    let mut small = query;
    small.page.max_bytes = 1024;
    match small.execute(&engine) {
        Ok(page) => {
            let ContextOutcome::Ambiguous { candidates } = page.outcome else {
                panic!()
            };
            assert_eq!(candidates.len(), 2);
        }
        Err(error) => assert_eq!(error.code(), ErrorCode::OutputLimit),
    }
}

#[test]
fn content_hashes_detect_same_stamp_edits_and_changes_during_page_generation() {
    use common::{FaultAction, FaultOperation, FaultVfs};
    for during_capture in [false, true] {
        let f = Fixture::search();
        let stamp = f.vfs.stamp(Path::new("/ws/b.p")).unwrap();
        let fault = Arc::new(FaultVfs::over(f.vfs.clone()).with_fixed_stamp(stamp));
        let engine = Engine::new(
            Workspace::new("/ws", fault.clone()),
            Languages::new().with(Fake::default()),
        );
        let first = SearchPageQuery {
            scope: Default::default(),
            query: Query::pattern("Foo"),
            page: PageBudget {
                max_items: 1,
                ..Default::default()
            },
        }
        .execute(&engine)
        .unwrap();
        if during_capture {
            fault.arm(
                FaultOperation::Read,
                Path::new("/ws/b.p"),
                0,
                FaultAction::ReadThenReplace("Foo____".into()),
            );
        } else {
            f.vfs.write(Path::new("/ws/b.p"), "Foo____").unwrap();
            assert_eq!(fault.stamp(Path::new("/ws/b.p")).unwrap(), stamp);
        }
        assert!(matches!(
            f.continue_query(first.next_cursor.unwrap(), 1)
                .execute(&engine),
            Err(EngineError::StaleQuery)
        ));
    }
}
#[test]
fn failed_mutation_invalidates_cursors_even_when_rollback_restores_the_original_bytes() {
    use common::{FaultAction, FaultOperation, FaultVfs};
    let f = Fixture::search();
    let fault = Arc::new(FaultVfs::over(f.vfs.clone()));
    let engine = Engine::new(
        Workspace::new("/ws", fault.clone()),
        Languages::new().with(Fake::default()),
    );
    let first = SearchPageQuery {
        scope: Default::default(),
        query: Query::pattern("Foo"),
        page: PageBudget {
            max_items: 1,
            ..Default::default()
        },
    }
    .execute(&engine)
    .unwrap();
    let plan = RenameIntent::new("Foo", "Bar").plan(&engine).unwrap();
    fault.arm(
        FaultOperation::Write,
        Path::new("/ws/a.p"),
        0,
        FaultAction::Before,
    );
    assert!(Apply(plan).apply(&engine).is_err());
    assert_eq!(f.vfs.read(Path::new("/ws/a.p")).unwrap(), f.source);
    assert!(matches!(
        f.continue_query(first.next_cursor.unwrap(), 1)
            .execute(&engine),
        Err(EngineError::StaleQuery)
    ));
}
#[test]
fn concurrent_retries_share_immutable_checkpoints() {
    let f = Fixture::search();
    let query = f.continue_query(f.page(1).next_cursor().unwrap().clone(), 2);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let engine = f.engine.clone();
            let query = query.clone();
            std::thread::spawn(move || {
                serde_json::to_value(query.execute(&engine).unwrap()).unwrap()
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
}
