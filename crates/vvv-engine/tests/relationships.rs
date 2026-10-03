mod common;
use common::Fake;
use std::{path::Path, sync::Arc};
use vvv_core::{CallKind, CallSite, Facts, NamedImport, Symbol, SymbolKind};
use vvv_engine::{
    Engine, EngineError, Languages, MemoryVfs, NavigationQuery, Position, RelationshipKind,
    RelationshipLimit, RelationshipResolution, RelationshipsQuery, Span, Vfs, Workspace,
};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
    source: String,
}
impl Fixture {
    fn new() -> Self {
        let source =
            "def work\ndef caller\nwork()\nalias()\nunknown()\nobj.work()\ncallback()\n".to_owned();
        let span = |name: &str| {
            let start = source.find(name).unwrap();
            Span::new(start, start + name.len())
        };
        let work = Symbol::plain(SymbolKind::Function, "work", span("work"), Span::new(0, 8));
        let caller = Symbol::plain(
            SymbolKind::Function,
            "caller",
            span("caller"),
            Span::new(9, source.len()),
        );
        let mut facts = Facts::new(vec![work, caller.clone()], vec![], vec![]);
        facts.calls_supported = true;
        facts.named_modules = true;
        facts.named_imports.push(NamedImport {
            local: "alias".into(),
            imported: "work".into(),
            module: None,
            name_span: span("alias"),
            alias_span: span("alias"),
            reexport: false,
            type_only: false,
        });
        for name in ["work", "caller", "alias", "unknown", "callback"] {
            for (start, _) in source.match_indices(name) {
                let token = Span::new(start, start + name.len());
                facts.push_token(name, "word", token);
                if !source[..start].ends_with("obj.") {
                    facts.navigation.push(token);
                }
                if source[token.end..].starts_with("()") {
                    facts.calls.push(CallSite {
                        span: Span::new(start, token.end + 2),
                        callee: token,
                        kind: if source[..start].ends_with("obj.") {
                            CallKind::Member
                        } else {
                            CallKind::Direct
                        },
                        owner: Some(caller.name_span),
                    });
                }
            }
        }
        facts.calls.sort_by_key(|c| c.callee.start);
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/a.p", &source)
                .with_file("/ws/package", "ws"),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default().with_navigation_facts(facts)),
        );
        Self {
            engine,
            vfs,
            source,
        }
    }
    fn query(&self, kind: RelationshipKind) -> RelationshipsQuery {
        RelationshipsQuery::new(
            NavigationQuery::at(
                "a.p",
                Position::new(
                    if kind == RelationshipKind::Callees {
                        1
                    } else {
                        0
                    },
                    4,
                ),
            )
            .origin,
            kind,
        )
    }
}
#[test]
fn aliases_are_confirmed_and_receiver_calls_remain_unresolved() {
    let f = Fixture::new();
    let result = f
        .query(RelationshipKind::Callers)
        .execute(&f.engine)
        .unwrap();
    assert!(result.coverage.scan_complete);
    assert_eq!(result.items.len(), 3);
    assert!(matches!(
        result.items[0].resolution,
        RelationshipResolution::Confirmed { .. }
    ));
    assert_eq!(result.items[1].spelling, "alias");
    assert!(matches!(
        result.items[1].resolution,
        RelationshipResolution::Confirmed { .. }
    ));
    assert!(matches!(
        result.items[2].resolution,
        RelationshipResolution::Unavailable { .. }
    ));
    for item in &result.items {
        assert_eq!(item.site.path.as_path(), Path::new("a.p"));
        assert_eq!(
            &f.source[item.site.span.start..item.site.span.end],
            item.spelling
        );
        assert!(item.caller.is_some());
    }
    let outgoing = f
        .query(RelationshipKind::Callees)
        .execute(&f.engine)
        .unwrap();
    assert_eq!(outgoing.items.len(), 5);
    assert!(outgoing.items.iter().any(|i| i.spelling == "unknown"
        && matches!(i.resolution, RelationshipResolution::Unavailable { .. })));
}
#[test]
fn limits_and_scopes_are_explicit_and_output_is_bounded() {
    let f = Fixture::new();
    let mut query = f.query(RelationshipKind::Callers);
    query.budget.max_lookups = 1;
    let limited = query.execute(&f.engine).unwrap();
    assert_eq!(
        limited.coverage.stopped_by,
        Some(RelationshipLimit::Lookups)
    );
    assert!(!limited.coverage.scan_complete);
    let mut query = f.query(RelationshipKind::Callers);
    query.budget.max_items = 1;
    let limited = query.execute(&f.engine).unwrap();
    assert_eq!(limited.coverage.stopped_by, Some(RelationshipLimit::Items));
    assert_eq!(limited.items.len(), 1);
    let mut query = f.query(RelationshipKind::Callers);
    query.scope.paths.push("elsewhere".into());
    let scoped = query.execute(&f.engine).unwrap();
    assert!(scoped.items.is_empty());
    assert!(scoped.coverage.scan_complete);
    let mut query = f.query(RelationshipKind::Callers);
    query.budget.max_bytes = 1024;
    match query.execute(&f.engine) {
        Ok(reply) => assert!(serde_json::to_vec(&reply).unwrap().len() <= 1024),
        Err(
            EngineError::OutputLimit {
                max_bytes: 1024, ..
            }
            | EngineError::PageOutputLimit {
                max_bytes: 1024, ..
            },
        ) => {}
        other => panic!("{other:?}"),
    }
}
#[test]
fn versioned_relationship_origins_reject_edits_and_invalid_budgets() {
    let f = Fixture::new();
    let nav = NavigationQuery::at("a.p", Position::new(0, 4))
        .execute(&f.engine)
        .unwrap();
    let vvv_engine::NavigationOutcome::Resolved { target, .. } = nav.outcome else {
        panic!()
    };
    let query = RelationshipsQuery::new(
        vvv_engine::NavigationOrigin::Symbol { symbol: target },
        RelationshipKind::References,
    );
    f.vfs
        .write(Path::new("/ws/a.p"), &(f.source + "changed"))
        .unwrap();
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::StaleSource { .. })
    ));
    let mut query = RelationshipsQuery::new(
        NavigationQuery::at("missing.p", Position::new(0, 0)).origin,
        RelationshipKind::Callees,
    );
    query.budget.max_items = 0;
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::InvalidBudget)
    ));
}

#[test]
fn ambiguity_and_missing_call_facts_are_not_empty_complete_results() {
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/a.p", "def work")
            .with_file("/ws/b.p", "def work")
            .with_file("/ws/use.p", "use a.p/work\nuse b.p/work\nwork")
            .with_file("/ws/package", "ws"),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new().with(Fake::default()),
    );
    let query = RelationshipsQuery::new(
        NavigationQuery::at("use.p", Position::new(2, 0)).origin,
        RelationshipKind::Callers,
    );
    let ambiguous = query.execute(&engine).unwrap();
    let vvv_engine::ResolutionOutcome::Ambiguous { candidates } = ambiguous.subject else {
        panic!()
    };
    assert_eq!(candidates.len(), 2);
    assert!(!ambiguous.coverage.scan_complete);
    let result = RelationshipsQuery::new(
        NavigationQuery::at("a.p", Position::new(0, 4)).origin,
        RelationshipKind::Callers,
    )
    .execute(&engine)
    .unwrap();
    assert_eq!(result.coverage.unsupported_files, 3);
    assert!(!result.coverage.scan_complete);
}

#[test]
fn edits_during_scanning_and_cancellation_do_not_publish_results() {
    use common::{FaultAction, FaultOperation, FaultVfs};
    for cancel in [false, true] {
        let base = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/a.p", "def work")
                .with_file("/ws/package", "ws"),
        );
        let fault = Arc::new(FaultVfs::over(base));
        let token = vvv_engine::ReadCancellation::default();
        if cancel {
            fault.cancel_on_read(Path::new("/ws/a.p"), 0, token.clone());
        } else {
            fault.arm(
                FaultOperation::Read,
                Path::new("/ws/a.p"),
                0,
                FaultAction::ReadThenReplace("def changed".into()),
            );
        }
        let engine = Engine::new(
            Workspace::new("/ws", fault),
            Languages::new().with(Fake::default()),
        );
        let query = RelationshipsQuery::new(
            NavigationQuery::at("a.p", Position::new(0, 4)).origin,
            RelationshipKind::Callers,
        );
        let call = vvv_engine::Call {
            id: None,
            max_output_bytes: None,
            request: vvv_engine::Request::Relationships(query),
        };
        let reply = call.execute_with_cancellation(&engine, &token);
        let value = serde_json::to_value(reply).unwrap();
        assert_eq!(value["status"], "error", "{value}");
        assert_eq!(
            value["code"],
            if cancel { "cancelled" } else { "stale" },
            "{value}"
        );
    }
}

#[test]
fn byte_fitting_retains_whole_sites_for_continuation() {
    let f = Fixture::new();
    let full = f
        .query(RelationshipKind::Callers)
        .execute(&f.engine)
        .unwrap();
    let mut query = f.query(RelationshipKind::Callers);
    query.budget.max_bytes = serde_json::to_vec(&full).unwrap().len() - 1;
    let limit = query.budget.max_bytes;
    let fitted = query.execute(&f.engine).unwrap();
    assert!(serde_json::to_vec(&fitted).unwrap().len() <= limit);
    assert_eq!(fitted.coverage.stopped_by, Some(RelationshipLimit::Bytes));
    assert_eq!(fitted.coverage.omitted_items, 0);
    assert_eq!(fitted.items, full.items[..fitted.items.len()]);
    let vvv_engine::PageReply::Relationships(rest) = (vvv_engine::ContinueQuery {
        cursor: fitted.next_cursor.unwrap(),
        page: vvv_engine::PageBudget::default(),
        work: None,
    })
    .execute(&f.engine)
    .unwrap() else {
        panic!()
    };
    let mut items = fitted.items;
    items.extend(rest.items);
    assert_eq!(items, full.items);
}

#[test]
fn ambiguous_incoming_sites_keep_all_candidates() {
    use vvv_core::Language;
    let source = "def work\ndef work\nwork\nwork\nwork";
    let fake = Fake::default();
    let mut facts = fake.facts(source).unwrap();
    facts.calls_supported = true;
    for (start, _) in source.match_indices("work").skip(2) {
        facts.calls.push(CallSite {
            span: Span::new(start, start + 4),
            callee: Span::new(start, start + 4),
            kind: CallKind::Direct,
            owner: None,
        });
    }
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(
                MemoryVfs::new()
                    .with_file("/ws/a.p", source)
                    .with_file("/ws/package", "ws"),
            ),
        ),
        Languages::new().with(fake.with_navigation_facts(facts)),
    );
    let reply = RelationshipsQuery::new(
        NavigationQuery::at("a.p", Position::new(0, 4)).origin,
        RelationshipKind::Callers,
    )
    .execute(&engine)
    .unwrap();
    let RelationshipResolution::Ambiguous { candidates } = &reply.items[0].resolution else {
        panic!("{reply:?}")
    };
    assert_eq!(candidates.len(), 2);
    assert_ne!(candidates[0].id, candidates[1].id);
    let mut query = RelationshipsQuery::new(
        NavigationQuery::at("a.p", Position::new(0, 4)).origin,
        RelationshipKind::Callers,
    );
    query.budget.max_items = 1;
    let first = query.execute(&engine).unwrap();
    let cursor = first.next_cursor.clone().unwrap();
    let limited = vvv_engine::ContinueQuery {
        cursor: cursor.clone(),
        page: vvv_engine::PageBudget {
            max_items: 1,
            max_bytes: 1024,
        },
        work: None,
    }
    .execute(&engine);
    assert!(matches!(limited, Err(EngineError::PageOutputLimit { .. })));
    let mut sites = first.items;
    let mut next = Some(cursor);
    while let Some(cursor) = next {
        let vvv_engine::PageReply::Relationships(page) = (vvv_engine::ContinueQuery {
            cursor,
            page: vvv_engine::PageBudget::default(),
            work: None,
        })
        .execute(&engine)
        .unwrap() else {
            panic!()
        };
        sites.extend(page.items);
        next = page.next_cursor;
    }
    assert_eq!(sites, reply.items);
}

#[test]
fn relationship_pages_preserve_alias_progress_sites_and_retry_identity() {
    use vvv_engine::{ContinueQuery, PageBudget, PageReply, WorkBudget};
    for kind in [
        RelationshipKind::Callers,
        RelationshipKind::Callees,
        RelationshipKind::References,
    ] {
        for (items, lookups, bytes) in [(1, 1, 8192), (2, 2, 2048)] {
            let fixture = Fixture::new();
            let expected = fixture.query(kind).execute(&fixture.engine).unwrap();
            let mut query = fixture.query(kind);
            query.budget.max_items = items;
            query.budget.max_lookups = lookups;
            query.budget.max_bytes = bytes;
            let first = query.execute(&fixture.engine).unwrap();
            let mut found = first.items.clone();
            let mut next = first.next_cursor;
            let mut total_lookups = first.coverage.lookups;
            let mut pages = 0;
            while let Some(cursor) = next {
                pages += 1;
                assert!(pages < 50);
                let request = ContinueQuery {
                    cursor,
                    page: PageBudget {
                        max_items: items,
                        max_bytes: bytes,
                    },
                    work: Some(WorkBudget {
                        max_lookups: lookups,
                        max_files: 1,
                    }),
                };
                let reply = request.clone().execute(&fixture.engine).unwrap();
                let retry = request.execute(&fixture.engine).unwrap();
                assert_eq!(
                    serde_json::to_value(&reply).unwrap(),
                    serde_json::to_value(&retry).unwrap()
                );
                let PageReply::Relationships(page) = reply else {
                    panic!("wrong page kind")
                };
                assert!(serde_json::to_vec(&page).unwrap().len() <= bytes);
                assert!(page.coverage.lookups <= lookups);
                total_lookups += page.coverage.lookups;
                found.extend(page.items);
                next = page.next_cursor;
                if next.is_none() {
                    assert!(page.coverage.scan_complete);
                }
            }
            assert_eq!(found, expected.items);
            assert_eq!(total_lookups, expected.coverage.lookups);
        }
    }
}

#[test]
fn incoming_context_follows_the_same_aliases_and_resumes_import_discovery() {
    use vvv_engine::{
        ContextPageQuery, ContextQuery, ContextRelation, ContinueQuery, PageBudget, PageReply,
        WorkBudget,
    };
    let fixture = Fixture::new();
    let origin = fixture.query(RelationshipKind::References).origin;
    let mut query = ContextQuery::new(origin.clone());
    query.references = true;
    let expected = query.execute(&fixture.engine).unwrap();
    assert!(
        expected.items.iter().any(
            |item| item.relation == ContextRelation::Reference && item.text.contains("alias()")
        )
    );
    let first = ContextPageQuery {
        origin,
        selection: Default::default(),
        detail: Default::default(),
        references: true,
        include_enclosing: false,
        page: PageBudget {
            max_items: 1,
            max_bytes: 8192,
        },
        work: WorkBudget {
            max_lookups: 1,
            max_files: 1,
        },
    }
    .execute(&fixture.engine)
    .unwrap();
    let mut items = first
        .items
        .into_iter()
        .map(|item| item.item)
        .collect::<Vec<_>>();
    let mut next = first.next_cursor;
    let mut pages = 0;
    while let Some(cursor) = next {
        pages += 1;
        assert!(pages < 50);
        let PageReply::Context(page) = (ContinueQuery {
            cursor,
            page: PageBudget {
                max_items: 1,
                max_bytes: 8192,
            },
            work: Some(WorkBudget {
                max_lookups: 1,
                max_files: 1,
            }),
        })
        .execute(&fixture.engine)
        .unwrap() else {
            panic!()
        };
        items.extend(page.items.into_iter().map(|item| item.item));
        next = page.next_cursor;
    }
    assert_eq!(items, expected.items);
}

#[test]
fn relationship_continuations_reject_stale_sources_and_cancel_without_consuming_cursor() {
    use vvv_engine::{Call, ContinueQuery, PageBudget, ReadCancellation, Request, WorkBudget};
    let fixture = Fixture::new();
    let mut query = fixture.query(RelationshipKind::References);
    query.budget.max_items = 1;
    let first = query.execute(&fixture.engine).unwrap();
    let request = ContinueQuery {
        cursor: first.next_cursor.unwrap(),
        page: PageBudget::default(),
        work: Some(WorkBudget::default()),
    };
    let cancellation = ReadCancellation::default();
    cancellation.cancel();
    let reply = Call {
        id: None,
        request: Request::Continue(request.clone()),
        max_output_bytes: None,
    }
    .execute_with_cancellation(&fixture.engine, &cancellation);
    assert!(
        serde_json::to_value(reply)
            .unwrap()
            .to_string()
            .contains("cancelled")
    );
    request.clone().execute(&fixture.engine).unwrap();
    fixture
        .vfs
        .write(Path::new("/ws/a.p"), &(fixture.source + "changed"))
        .unwrap();
    assert!(matches!(
        request.execute(&fixture.engine),
        Err(EngineError::StaleQuery)
    ));
}

#[test]
fn incoming_discovery_scans_declared_reference_groups_without_naming_languages() {
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/package", "ws")
            .with_file("/ws/a.p", "def work")
            .with_file("/ws/b.q", "use a.p/work\ndef caller work")
            .with_file("/ws/unrelated.r", "def work\ndef unrelated work"),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs),
        Languages::new()
            .with(Fake::new("first", &["p"]).with_reference_group("family"))
            .with(Fake::new("second", &["q"]).with_reference_group("family"))
            .with(Fake::new("unrelated", &["r"])),
    );
    let query = RelationshipsQuery::new(
        NavigationQuery::at("a.p", Position::new(0, 4)).origin,
        RelationshipKind::References,
    );
    let result = query.execute(&engine).unwrap();
    assert_eq!(result.coverage.files_scanned, 2);
    assert!(
        result
            .items
            .iter()
            .any(|item| item.site.path.as_path() == Path::new("b.q")
                && matches!(item.resolution, RelationshipResolution::Confirmed { .. }))
    );
    assert!(
        !result
            .items
            .iter()
            .any(|item| item.site.path.as_path() == Path::new("unrelated.r"))
    );
}
