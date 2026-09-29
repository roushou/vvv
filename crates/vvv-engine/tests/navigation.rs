mod common;
use common::Fake;
use std::path::Path;
use std::sync::Arc;
use vvv_engine::{
    Answer, ContentId, Engine, Languages, MemoryVfs, NavigationOrigin, NavigationOutcome,
    NavigationQuery, Position, Query, Request, Retention, SearchQuery, SourceAnchor, Span,
    UnavailableReason, Vfs, Workspace,
};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
}
impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let vfs = Arc::new(MemoryVfs::new());
        vfs.write(Path::new("/ws/package"), "ws").unwrap();
        for (path, text) in files {
            vfs.write(&Path::new("/ws").join(path), text).unwrap();
        }
        let languages = Languages::new().with(Fake::default());
        Self {
            engine: Engine::new(Workspace::new("/ws", vfs.clone()), languages)
                .with_retention(Retention::session()),
            vfs,
        }
    }
    fn at(&self, path: &str, line: u32, column: u32) -> vvv_engine::NavigationReply {
        NavigationQuery::at(path, Position::new(line, column))
            .execute(&self.engine)
            .unwrap()
    }
}

#[test]
fn imports_resolve_the_exact_occurrence_despite_duplicate_names() {
    let f = Fixture::new(&[
        ("a.p", "def Engine"),
        ("other.p", "def Engine"),
        ("use.p", "use a.p/Engine\nEngine"),
    ]);
    for (line, column) in [(0, 8), (1, 0)] {
        let reply = f.at("use.p", line, column);
        let NavigationOutcome::Resolved {
            target,
            preview,
            evidence,
        } = reply.outcome
        else {
            panic!("{reply:?}");
        };
        assert_eq!(target.declaration.path.as_path(), Path::new("a.p"));
        assert_eq!(
            target.declaration.content,
            ContentId::of(&preview.source.text)
        );
        assert_eq!(preview.source.text, "def Engine");
        assert!(!evidence.addresses.is_empty());
        assert!(
            preview
                .identifiers
                .iter()
                .all(|a| a.content == target.declaration.content)
        );
        let selected = NavigationQuery {
            origin: NavigationOrigin::Symbol { symbol: target },
            selection: vvv_engine::Selection::All,
        }
        .execute(&f.engine)
        .unwrap();
        assert!(matches!(
            selected.outcome,
            NavigationOutcome::Resolved { .. }
        ));
    }
}

#[test]
fn competing_imports_are_returned_as_candidates_and_dispatch_is_read_only() {
    let f = Fixture::new(&[
        ("a.p", "def Engine"),
        ("b.p", "def Engine"),
        ("use.p", "use a.p/Engine\nuse b.p/Engine\nEngine"),
    ]);
    let request = Request::Navigate(NavigationQuery::at("use.p", Position::new(2, 0)));
    assert!(request.is_read_only());
    let Answer::Navigate(reply) = f.engine.run(request).unwrap().into_answer() else {
        panic!()
    };
    let NavigationOutcome::Ambiguous { candidates } = reply.outcome else {
        panic!("{reply:?}")
    };
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].declaration.path.as_path(), Path::new("a.p"));
    assert_eq!(candidates[1].declaration.path.as_path(), Path::new("b.p"));
    assert_ne!(candidates[0].declaration.id, candidates[1].declaration.id);
}

#[test]
fn search_anchors_validate_the_whole_source_and_bad_ranges_are_errors() {
    let f = Fixture::new(&[("a.p", "def Engine\nEngine")]);
    let found = SearchQuery::from(Query::pattern("Engine"))
        .execute(&f.engine)
        .unwrap();
    let anchor = found.matches.last().unwrap().anchor().unwrap();
    assert!(matches!(
        NavigationQuery::occurrence(anchor.clone())
            .execute(&f.engine)
            .unwrap()
            .outcome,
        NavigationOutcome::Resolved { .. }
    ));
    f.vfs
        .write(
            Path::new("/ws/a.p"),
            "def Engine\nEngine\nchanged elsewhere",
        )
        .unwrap();
    assert!(matches!(
        NavigationQuery::occurrence(anchor).execute(&f.engine),
        Err(vvv_engine::EngineError::StaleSource { .. })
    ));
    let bad = SourceAnchor {
        path: "a.p".into(),
        content: ContentId::of("def Engine\nEngine\nchanged elsewhere"),
        span: Span::new(99, 100),
    };
    assert!(matches!(
        NavigationQuery::occurrence(bad).execute(&f.engine),
        Err(vvv_engine::EngineError::InvalidAnchor { .. })
    ));
}

#[test]
fn imported_source_and_project_changes_change_snapshot_identity() {
    let f = Fixture::new(&[("a.p", "def Engine"), ("use.p", "use a.p/Engine\nEngine")]);
    let before = f.at("use.p", 1, 0);
    f.vfs
        .write(Path::new("/ws/a.p"), "def Engine\n// changed")
        .unwrap();
    let after = f.at("use.p", 1, 0);
    assert_ne!(before.snapshot, after.snapshot);
    f.vfs.write(Path::new("/ws/new.p"), "def New").unwrap();
    assert_ne!(after.snapshot, f.at("use.p", 1, 0).snapshot);
}

#[test]
fn unicode_positions_and_missing_identifiers_are_distinct_from_bad_positions() {
    let f = Fixture::new(&[("a.p", "def Éclair\nÉclair")]);
    assert!(matches!(
        f.at("a.p", 1, 1).outcome,
        NavigationOutcome::Resolved { .. }
    ));
    assert!(matches!(
        f.at("a.p", 0, 3).outcome,
        NavigationOutcome::Unavailable {
            reason: UnavailableReason::NoIdentifier
        }
    ));
    assert!(matches!(
        NavigationQuery::at("a.p", Position::new(99, 0)).execute(&f.engine),
        Err(vvv_engine::EngineError::NoSuchPosition { .. })
    ));
    let anchor = SourceAnchor {
        path: "a.p".into(),
        content: ContentId::of("def Éclair\nÉclair"),
        span: Span::new(5, 6),
    };
    assert!(matches!(
        NavigationQuery::occurrence(anchor).execute(&f.engine),
        Err(vvv_engine::EngineError::InvalidAnchor { .. })
    ));
}

#[test]
fn selecting_a_candidate_uses_the_same_resolution_and_rejects_missing_ids() {
    let f = Fixture::new(&[
        ("a.p", "def Engine"),
        ("b.p", "def Engine"),
        ("use.p", "use a.p/Engine\nuse b.p/Engine\nEngine"),
    ]);
    let query = NavigationQuery::at("use.p", Position::new(2, 0));
    let NavigationOutcome::Ambiguous { candidates } =
        query.clone().execute(&f.engine).unwrap().outcome
    else {
        panic!()
    };
    for selection in [
        vvv_engine::Selection::ordinals([2]),
        vvv_engine::Selection::ids([candidates[1].declaration.id.clone()]),
    ] {
        let reply = query.clone().select(selection).execute(&f.engine).unwrap();
        let NavigationOutcome::Resolved { target, .. } = reply.outcome else {
            panic!()
        };
        assert_eq!(target, candidates[1].target);
    }
    assert_eq!(
        query
            .select(vvv_engine::Selection::ordinals([3]))
            .execute(&f.engine)
            .unwrap_err()
            .code(),
        vvv_engine::ErrorCode::BadSelection
    );
}

#[test]
fn observed_edits_expire_a_trusted_graph_and_never_pair_old_source_with_new_metadata() {
    let mut f = Fixture::new(&[("a.p", "def Engine"), ("use.p", "use a.p/Engine\nEngine")]);
    f.engine = f
        .engine
        .with_retention(Retention::session().trusting(std::time::Duration::from_secs(3600)));
    f.at("use.p", 1, 0);
    f.vfs
        .write(Path::new("/ws/a.p"), "// new line\ndef Engine")
        .unwrap();
    let stale = NavigationQuery::at("use.p", Position::new(1, 0))
        .execute(&f.engine)
        .unwrap_err();
    assert_eq!(stale.code(), vvv_engine::ErrorCode::Stale);
    let NavigationOutcome::Resolved {
        target, preview, ..
    } = f.at("use.p", 1, 0).outcome
    else {
        panic!()
    };
    assert_eq!(target.name_span.start, 16);
    assert_eq!(
        target.declaration.content,
        ContentId::of(&preview.source.text)
    );
}

#[test]
fn reexport_cycles_terminate_and_long_chains_are_explicitly_incomplete() {
    let f = Fixture::new(&[
        ("a.p", "pub use b.p/*"),
        ("b.p", "pub use a.p/*"),
        ("use.p", "use a.p/Engine\nEngine"),
    ]);
    assert!(matches!(
        f.at("use.p", 1, 0).outcome,
        NavigationOutcome::Unavailable {
            reason: UnavailableReason::CyclicImports
        }
    ));
    for i in 0..130 {
        f.vfs
            .write(
                &Path::new("/ws").join(format!("chain{i}.p")),
                &format!("pub use chain{}.p/*", i + 1),
            )
            .unwrap();
    }
    f.vfs
        .write(Path::new("/ws/use.p"), "use chain0.p/Engine\nEngine")
        .unwrap();
    assert_eq!(
        NavigationQuery::at("use.p", Position::new(1, 0))
            .execute(&f.engine)
            .unwrap_err()
            .code(),
        vvv_engine::ErrorCode::Incomplete
    );
}

#[test]
fn wire_origins_and_outcomes_roundtrip() {
    let f = Fixture::new(&[("a.p", "def Engine")]);
    let query = NavigationQuery::at("a.p", Position::new(0, 5));
    let request = Request::Navigate(query.clone());
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["command"], "navigate");
    assert_eq!(json["origin"]["kind"], "position");
    assert_eq!(serde_json::from_value::<Request>(json).unwrap(), request);
    let reply = query.execute(&f.engine).unwrap();
    let json = serde_json::to_value(&reply).unwrap();
    assert_eq!(json["outcome"], "resolved");
    assert_eq!(
        serde_json::from_value::<vvv_engine::NavigationReply>(json).unwrap(),
        reply
    );
}

#[test]
fn local_declarations_shadow_glob_imports_without_workspace_name_guessing() {
    let f = Fixture::new(&[
        ("a.p", "def Engine"),
        ("use.p", "use a.p/*\ndef Engine\nEngine"),
    ]);
    let NavigationOutcome::Resolved { target, .. } = f.at("use.p", 2, 0).outcome else {
        panic!()
    };
    assert_eq!(target.declaration.path.as_path(), Path::new("use.p"));
}

#[test]
fn unreadable_manifests_remain_io_errors_instead_of_external_source_outcomes() {
    use common::{FaultAction, FaultOperation, FaultVfs};
    let base = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/package", "ws")
            .with_file("/ws/a.p", "def Engine\nEngine"),
    );
    let faults = Arc::new(FaultVfs::over(base));
    faults.arm(
        FaultOperation::Read,
        Path::new("/ws/package"),
        0,
        FaultAction::Always,
    );
    let engine = Engine::new(
        Workspace::new("/ws", faults),
        Languages::new().with(Fake::default()),
    );
    let error = NavigationQuery::at("a.p", Position::new(1, 0))
        .execute(&engine)
        .unwrap_err();
    assert_eq!(error.code(), vvv_engine::ErrorCode::Io);
}

#[test]
fn lexical_facts_select_the_innermost_binding_and_preserve_symbol_roundtrips() {
    use vvv_core::{BindingNamespace, Facts, LexicalBinding, Symbol, SymbolKind};
    let source = "x x x x";
    let mut facts = Facts::default();
    for start in [0, 2, 4, 6] {
        let span = Span::new(start, start + 1);
        facts.push_token("x", "identifier", span);
        facts.lexical_tokens.push(span);
    }
    for (name, scope, from) in [(0, Span::new(0, 7), 0), (2, Span::new(2, 5), 4)] {
        facts.lexical.push(LexicalBinding {
            symbol: Symbol::plain(
                SymbolKind::Variable,
                "x",
                Span::new(name, name + 1),
                Span::new(name, name + 1),
            ),
            scope,
            visible_from: from,
            excluded: vec![],
            namespace: BindingNamespace::Value,
        });
    }
    let engine = Engine::new(
        Workspace::new(
            "/ws",
            Arc::new(MemoryVfs::new().with_file("/ws/a.p", source)),
        ),
        Languages::new().with(Fake::default().with_navigation_facts(facts)),
    );
    for (column, expected) in [(4, 2), (6, 0)] {
        let reply = NavigationQuery::at("a.p", Position::new(0, column))
            .execute(&engine)
            .unwrap();
        let NavigationOutcome::Resolved {
            target, preview, ..
        } = reply.outcome
        else {
            panic!()
        };
        assert_eq!(target.name_span.start, expected);
        assert!(
            preview
                .source
                .symbols
                .iter()
                .any(|s| s.name_span == target.name_span)
        );
        assert!(matches!(
            NavigationQuery {
                origin: NavigationOrigin::Symbol { symbol: target },
                selection: vvv_engine::Selection::All
            }
            .execute(&engine)
            .unwrap()
            .outcome,
            NavigationOutcome::Resolved { .. }
        ));
    }
}
