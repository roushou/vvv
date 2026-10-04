mod common;
use common::Fake;
use std::{path::Path, sync::Arc};
use vvv_engine::{
    ApplyPlanQuery, ContentId, DiscardPlanQuery, Engine, EngineError, Failure, Languages, Ledger,
    MemoryVfs, MoveSymbolIntent, PlanStatus, PrepareMoveSymbolIntent, PrepareMoveSymbolQuery,
    Selection, SymbolMoveCandidatesQuery, SymbolMoveUnsupported, Vfs, Workspace,
};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
}
impl Fixture {
    fn new(source: &str) -> Self {
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/a.p", source)
                .with_file("/ws/b.p", "def bar\n")
                .with_file("/ws/use.p", "use a.p/foo\nfoo\n"),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default()),
        );
        Self { engine, vfs }
    }
    fn candidates(&self) -> vvv_engine::SymbolMoveCandidates {
        SymbolMoveCandidatesQuery {
            name: "foo".into(),
            from: "a.p".into(),
        }
        .execute(&self.engine)
        .unwrap()
    }
    fn query(&self) -> PrepareMoveSymbolQuery {
        let candidates = self.candidates();
        PrepareMoveSymbolQuery {
            intent: PrepareMoveSymbolIntent {
                name: "foo".into(),
                from: "a.p".into(),
                to: "b.p".into(),
                selection: Selection::ids([candidates.candidates[0].declaration.id.clone()]),
                expected_content: Some(candidates.content),
            },
            max_bytes: 65536,
            page: None,
        }
    }
}
#[test]
fn candidates_are_stable_and_ambiguity_never_picks_the_first_declaration() {
    let f = Fixture::new("pub def foo\ndef foo\n");
    let candidates = f.candidates();
    assert_eq!(candidates, f.candidates());
    assert_eq!(candidates.candidates.len(), 2);
    assert_ne!(
        candidates.candidates[0].declaration.id,
        candidates.candidates[1].declaration.id
    );
    assert!(candidates.candidates.iter().all(|candidate| candidate.unsupported == Some(SymbolMoveUnsupported::CompetingBinding)));
    let error = MoveSymbolIntent::new("foo", "a.p", "b.p")
        .plan(&f.engine)
        .unwrap_err();
    assert!(
        matches!(&error, EngineError::SymbolMoveSelection { candidates } if candidates.len() == 2)
    );
    assert_eq!(
        Failure::from(&error).symbol_move_candidates.unwrap(),
        candidates.candidates
    );
    let selected = MoveSymbolIntent::new("foo", "a.p", "b.p")
        .selecting(Selection::ordinals([2]))
        .plan(&f.engine);
    assert!(matches!(
        selected,
        Err(EngineError::UnsupportedSymbolMove {
            reason: SymbolMoveUnsupported::CompetingBinding,
            ..
        })
    ));
    assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
}
#[test]
fn selection_requires_one_existing_candidate_and_matching_source_content() {
    let f = Fixture::new("pub def foo\n");
    for selection in [
        Selection::ids(["000000000000".to_owned().into()]),
        Selection::ordinals([0]),
        Selection::ordinals([2]),
    ] {
        assert!(matches!(
            MoveSymbolIntent::new("foo", "a.p", "b.p")
                .selecting(selection)
                .plan(&f.engine),
            Err(EngineError::Selection(_))
        ));
    }
    assert!(matches!(
        MoveSymbolIntent::new("foo", "a.p", "b.p")
            .selecting(Selection::ids([]))
            .plan(&f.engine),
        Err(EngineError::SymbolMoveSelection { .. })
    ));
    assert!(matches!(
        MoveSymbolIntent::new("foo", "a.p", "b.p")
            .expecting(ContentId::of("old"))
            .plan(&f.engine),
        Err(EngineError::StaleSource { .. })
    ));
    for from in ["", ".", "../outside.p", "/outside.p"] {
        assert!(matches!(
            SymbolMoveCandidatesQuery {
                name: "foo".into(),
                from: from.into()
            }
            .execute(&f.engine),
            Err(EngineError::InvalidMovePath { .. })
        ));
    }
    let mut query = f.query();
    query.intent.to = "../outside.p".into();
    assert!(matches!(
        query.execute(&f.engine),
        Err(EngineError::InvalidMovePath { .. })
    ));
    assert!(matches!(
        MoveSymbolIntent::new("foo", "a.p", "a.p").plan(&f.engine),
        Err(EngineError::UnsupportedSymbolMove {
            reason: SymbolMoveUnsupported::SameFile,
            ..
        })
    ));
    f.vfs.write(Path::new("/ws/b.p"), "def foo\n").unwrap();
    assert!(matches!(
        f.query().execute(&f.engine),
        Err(EngineError::UnsupportedSymbolMove {
            reason: SymbolMoveUnsupported::DestinationBindingConflict,
            ..
        })
    ));
}
#[test]
fn retained_symbol_move_applies_exact_review_once_and_undoes_to_original() {
    let f = Fixture::new("pub def foo\n");
    let expected = MoveSymbolIntent::new("foo", "a.p", "b.p")
        .plan(&f.engine)
        .unwrap();
    let review = f
        .query()
        .execute(&f.engine)
        .unwrap()
        .into_complete()
        .unwrap();
    let PlanStatus::Prepared { preview } = &review.status else {
        panic!()
    };
    assert_eq!(
        serde_json::to_value(preview.files()).unwrap(),
        serde_json::to_value(&expected.files).unwrap()
    );
    let apply = ApplyPlanQuery {
        plan_id: review.plan_id,
    };
    let receipt = apply.clone().execute(&f.engine).unwrap();
    for file in expected.preview() {
        assert_eq!(
            f.vfs.read(&Path::new("/ws").join(&file.path)).unwrap(),
            file.after
        );
        assert!(
            receipt.files.iter().any(|version| version.path == file.path
                && version.content == ContentId::of(&file.after))
        );
    }
    assert_eq!(apply.clone().execute(&f.engine).unwrap(), receipt);
    Ledger::new(&f.engine).undo().unwrap();
    for file in expected.preview() {
        assert_eq!(
            f.vfs.read(&Path::new("/ws").join(&file.path)).unwrap(),
            file.before
        );
    }
    f.vfs.write(Path::new("/ws/b.p"), "later edit").unwrap();
    assert_eq!(apply.execute(&f.engine).unwrap(), receipt);
}
#[test]
fn stale_sources_destinations_and_resolution_inputs_consume_without_writes() {
    for path in ["a.p", "b.p", "use.p", "package"] {
        let f = Fixture::new("pub def foo\n");
        let review = f
            .query()
            .execute(&f.engine)
            .unwrap()
            .into_complete()
            .unwrap();
        f.vfs
            .write(&Path::new("/ws").join(path), "external edit")
            .unwrap();
        let apply = ApplyPlanQuery {
            plan_id: review.plan_id,
        };
        assert!(apply.clone().execute(&f.engine).is_err());
        assert!(matches!(
            apply.execute(&f.engine),
            Err(EngineError::PlanConsumed)
        ));
        assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
    }
    let f = Fixture::new("pub def foo\n");
    let review = f
        .query()
        .execute(&f.engine)
        .unwrap()
        .into_complete()
        .unwrap();
    DiscardPlanQuery {
        plan_id: review.plan_id.clone(),
    }
    .execute(&f.engine)
    .unwrap();
    assert!(matches!(
        ApplyPlanQuery {
            plan_id: review.plan_id
        }
        .execute(&f.engine),
        Err(EngineError::PlanConsumed)
    ));
}

#[test]
fn missing_nested_and_ambiguous_piece_evidence_remain_visible_and_unmovable() {
    use vvv_core::{CompanionOwnership, CompanionPiece, DeclarationPieces, Span};
    for (pieces, expected) in [
        (vec![], SymbolMoveUnsupported::MissingEvidence),
        (
            vec![DeclarationPieces {
                declaration: Span::new(4, 11),
                supported: true,
                scope: Span::new(4, 12),
                top_level: false,
                companions: vec![],
            }],
            SymbolMoveUnsupported::NestedDeclaration,
        ),
        (
            vec![DeclarationPieces {
                declaration: Span::new(4, 11),
                supported: true,
                scope: Span::new(0, 12),
                top_level: true,
                companions: vec![CompanionPiece {
                    span: Span::new(4, 11),
                    ownership: CompanionOwnership::AmbiguousTarget,
                }],
            }],
            SymbolMoveUnsupported::AmbiguousCompanion,
        ),
        (
            vec![DeclarationPieces {
                declaration: Span::new(4, 11),
                supported: true,
                scope: Span::new(0, 12),
                top_level: true,
                companions: vec![CompanionPiece {
                    span: Span::new(4, 11),
                    ownership: CompanionOwnership::UnsupportedTarget,
                }],
            }],
            SymbolMoveUnsupported::UnsupportedCompanionTarget,
        ),
    ] {
        let f = Fixture::new("pub def foo\n");
        let engine = Engine::new(
            Workspace::new("/ws", f.vfs.clone()),
            Languages::new().with(Fake::default().with_move_pieces(pieces)),
        );
        let candidates = SymbolMoveCandidatesQuery {
            name: "foo".into(),
            from: "a.p".into(),
        }
        .execute(&engine)
        .unwrap();
        assert_eq!(candidates.candidates.len(), 1);
        assert_eq!(candidates.candidates[0].unsupported, Some(expected));
        assert!(
            matches!(MoveSymbolIntent::new("foo","a.p","b.p").plan(&engine), Err(EngineError::UnsupportedSymbolMove {reason,..}) if reason == expected)
        );
    }
}

#[test]
fn invalid_owned_piece_evidence_is_a_conflict_before_writes() {
    use vvv_core::{CompanionOwnership, CompanionPiece, DeclarationPieces, Span};
    let f = Fixture::new("pub def foo\n");
    let fake = Fake::default().with_move_pieces(vec![DeclarationPieces {
        declaration: Span::new(4, 11),
        supported: true,
        scope: Span::new(0, 12),
        top_level: true,
        companions: vec![CompanionPiece {
            span: Span::new(100, 110),
            ownership: CompanionOwnership::SameScopeTarget,
        }],
    }]);
    let engine = Engine::new(
        Workspace::new("/ws", f.vfs.clone()),
        Languages::new().with(fake),
    );
    assert!(matches!(
        MoveSymbolIntent::new("foo", "a.p", "b.p").plan(&engine),
        Err(EngineError::Search {
            source: vvv_core::SearchError::Facts(vvv_core::FactsError::Span(
                vvv_core::SpanError::OutOfBounds { .. }
            )),
            ..
        })
    ));
    assert!(Ledger::new(&engine).history().unwrap().entries.is_empty());
}

#[test]
fn retained_symbol_move_write_failure_restores_both_files_and_consumes_handle() {
    use common::{FaultAction, FaultFixture, FaultOperation};
    let f = FaultFixture::new(&[("a.p", "pub def foo\n"), ("b.p", "def bar\n")]);
    let review = PrepareMoveSymbolQuery {
        intent: PrepareMoveSymbolIntent {
            name: "foo".into(),
            from: "a.p".into(),
            to: "b.p".into(),
            selection: Selection::All,
            expected_content: None,
        },
        max_bytes: 65536,
        page: None,
    }
    .execute(&f.engine)
    .unwrap()
    .into_complete()
    .unwrap();
    f.arm(FaultOperation::Write, "b.p", 0, FaultAction::After);
    let apply = ApplyPlanQuery {
        plan_id: review.plan_id,
    };
    assert!(apply.clone().execute(&f.engine).is_err());
    assert_eq!(f.read("a.p"), "pub def foo\n");
    assert_eq!(f.read("b.p"), "def bar\n");
    assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
    assert!(matches!(
        apply.execute(&f.engine),
        Err(EngineError::PlanConsumed)
    ));
}

#[test]
fn candidate_report_preserves_actionable_source_anchors() {
    use vvv_engine::report::{Detailed, Document, Options, View};
    let f = Fixture::new("pub def foo\ndef foo\n");
    let answer = vvv_engine::Answer::SymbolMoveCandidates(f.candidates());
    let document = Document::of(&answer);
    let presentation = Detailed.present(&document, Options::default(), usize::MAX);
    assert_eq!(presentation.body.len(), 2);
    for (index, row) in presentation.body.iter().enumerate() {
        let source = row.source.as_ref().unwrap();
        assert_eq!(source.path, vvv_engine::RelPath::from("a.p"));
        assert_eq!(source.line, index as u32);
    }
}
