mod common;
use common::{Fake, FaultAction, FaultFixture, FaultOperation};
use std::{path::Path, sync::Arc};
use vvv_engine::{
    ApplyPlanQuery, DiscardPlanQuery, Engine, EngineError, InspectPlanQuery, Languages, Ledger,
    MemoryVfs, MoveIntent, PlanStatus, PrepareMoveIntent, PrepareMoveQuery, Vfs, Workspace,
};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
}
impl Fixture {
    fn new() -> Self {
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/manifest.p", "")
                .with_file("/ws/a/x.p", "def foo\n")
                .with_file("/ws/a/y.p", "def bar\n")
                .with_file("/ws/use.p", "use a/x.p\nuse {a/x.p}\n"),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default()),
        );
        Self { engine, vfs }
    }
    fn query(from: &str, to: &str) -> PrepareMoveQuery {
        PrepareMoveQuery {
            intent: PrepareMoveIntent {
                from: from.into(),
                to: to.into(),
            },
            max_bytes: 65536,
            page: None,
        }
    }
}
#[test]
fn retained_file_and_directory_moves_apply_exactly_once_and_undo() {
    for from in ["a/x.p", "a"] {
        let f = Fixture::new();
        let to = if from == "a" { "b" } else { "b/z.p" };
        let expected = MoveIntent::new(from, to).plan(&f.engine).unwrap();
        let review = Fixture::query(from, to)
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
            plan_id: review.plan_id.clone(),
        };
        let receipt = apply.clone().execute(&f.engine).unwrap();
        for file in expected.preview() {
            let path = file.moved_to.as_ref().unwrap_or(&file.path);
            assert_eq!(
                f.vfs.read(&Path::new("/ws").join(path)).unwrap(),
                file.after
            );
            assert!(receipt.files.iter().any(|version| &version.path == path
                && version.content == vvv_engine::ContentId::of(&file.after)));
        }
        assert_eq!(receipt, apply.clone().execute(&f.engine).unwrap());
        assert_eq!(Ledger::new(&f.engine).history().unwrap().entries.len(), 1);
        Ledger::new(&f.engine).undo().unwrap();
        for file in expected.preview() {
            assert_eq!(
                f.vfs.read(&Path::new("/ws").join(&file.path)).unwrap(),
                file.before
            );
        }
        assert_eq!(receipt, apply.execute(&f.engine).unwrap());
    }
}
#[test]
fn invalid_paths_stale_inputs_and_occupied_destinations_never_write() {
    for path in ["", ".", "../outside.p", "/outside.p"] {
        let f = Fixture::new();
        assert!(matches!(
            Fixture::query("a/x.p", path).execute(&f.engine),
            Err(EngineError::InvalidMovePath { .. })
        ));
    }
    for path in ["a/x.p", "use.p", "package", "b/z.p", "b/.gitignore"] {
        let f = Fixture::new();
        let review = Fixture::query("a/x.p", "b/z.p")
            .execute(&f.engine)
            .unwrap()
            .into_complete()
            .unwrap();
        f.vfs
            .write(&Path::new("/ws").join(path), "editor change")
            .unwrap();
        let apply = ApplyPlanQuery {
            plan_id: review.plan_id.clone(),
        };
        assert!(apply.clone().execute(&f.engine).is_err());
        assert!(matches!(
            apply.execute(&f.engine),
            Err(EngineError::PlanConsumed)
        ));
        assert!(f.vfs.exists(Path::new("/ws/a/x.p")));
        assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
        assert!(matches!(
            InspectPlanQuery {
                plan_id: review.plan_id,
                max_bytes: 65536,
                page: None
            }
            .execute(&f.engine)
            .unwrap()
            .into_complete()
            .unwrap()
            .status,
            PlanStatus::Failed { .. }
        ));
    }
}
#[test]
fn discard_releases_a_move_and_write_failure_recovers_original_paths() {
    let f = Fixture::new();
    let review = Fixture::query("a/x.p", "b/z.p")
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
    let f = FaultFixture::new(&[
        ("a.p", "def foo\n"),
        ("manifest.p", ""),
        ("use.p", "use a.p\n"),
    ]);
    let review = Fixture::query("a.p", "b.p")
        .execute(&f.engine)
        .unwrap()
        .into_complete()
        .unwrap();
    f.arm(FaultOperation::Write, "use.p", 0, FaultAction::After);
    assert!(
        ApplyPlanQuery {
            plan_id: review.plan_id
        }
        .execute(&f.engine)
        .is_err()
    );
    assert_eq!(f.vfs.read(Path::new("/ws/a.p")).unwrap(), "def foo\n");
    assert!(!f.vfs.exists(Path::new("/ws/b.p")));
}
