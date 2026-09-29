mod common;
use common::{Fake, FaultVfs};
use std::{path::Path, sync::Arc};
use vvv_engine::*;

struct Fixture {
    engine: Engine,
    vfs: Arc<FaultVfs>,
}
impl Fixture {
    fn new() -> Self {
        let vfs = Arc::new(FaultVfs::over(Arc::new(
            MemoryVfs::new().with_file("/ws/a.p", "def Foo\nFoo Foo Foo Foo\n"),
        )));
        Self {
            engine: Engine::new(
                Workspace::new("/ws", vfs.clone()),
                Languages::new().with(Fake::default()),
            ),
            vfs,
        }
    }
    fn call(value: serde_json::Value) -> Call {
        serde_json::from_value(value).unwrap()
    }
    fn search() -> Call {
        Self::call(
            serde_json::json!({"command":"search_page","query":{"pattern":"Foo"},"page":{"max_items":1}}),
        )
    }
    fn value(reply: Reply<Answer>) -> serde_json::Value {
        serde_json::to_value(reply.response).unwrap()
    }
}

#[test]
fn cancellation_is_single_use_read_only_and_pre_cancelled_calls_do_no_io() {
    let fixture = Fixture::new();
    let token = ReadCancellation::default();
    token.cancel();
    let reply =
        Fixture::value(Fixture::search().execute_with_cancellation(&fixture.engine, &token));
    assert_eq!(reply["code"], "cancelled");
    assert!(fixture.vfs.trace().is_empty());
    let reused =
        Fixture::value(Fixture::search().execute_with_cancellation(&fixture.engine, &token));
    assert_eq!(reused["code"], "bad_request");
    let mutation = Fixture::call(serde_json::json!({"command":"undo","apply":true}));
    let refused = Fixture::value(
        mutation.execute_with_cancellation(&fixture.engine, &ReadCancellation::default()),
    );
    assert_eq!(refused["code"], "bad_request");
    assert!(fixture.vfs.trace().is_empty());
}

#[test]
fn cancelling_each_read_checkpoint_preserves_continuation_replay() {
    // Count all reads in this deterministic one-file query, then cancel at each
    // one, including validation reads immediately before cursor publication.
    let probe = Fixture::new();
    let first = Fixture::value(Fixture::search().execute(&probe.engine));
    let request = serde_json::json!({"command":"continue","cursor":first["result"]["next_cursor"],"page":{"max_items":1}});
    probe.vfs.clear_trace();
    assert_eq!(
        Fixture::value(Fixture::call(request).execute(&probe.engine))["status"],
        "ok"
    );
    let reads = probe
        .vfs
        .trace()
        .iter()
        .filter(|(op, path)| *op == common::FaultOperation::Read && path == Path::new("/ws/a.p"))
        .count();
    assert!(reads > 0);
    for skip in 0..reads {
        let fixture = Fixture::new();
        let first = Fixture::value(Fixture::search().execute(&fixture.engine));
        let request = serde_json::json!({"command":"continue","cursor":first["result"]["next_cursor"],"page":{"max_items":1}});
        let token = ReadCancellation::default();
        fixture
            .vfs
            .cancel_on_read(Path::new("/ws/a.p"), skip, token.clone());
        let cancelled = Fixture::value(
            Fixture::call(request.clone()).execute_with_cancellation(&fixture.engine, &token),
        );
        assert_eq!(
            cancelled["code"], "cancelled",
            "checkpoint {skip}: {cancelled}"
        );
        let retry = Fixture::value(Fixture::call(request.clone()).execute(&fixture.engine));
        assert_eq!(retry["result"]["items"][0]["ordinal"], 2);
        assert_eq!(
            retry,
            Fixture::value(Fixture::call(request).execute(&fixture.engine))
        );
    }
}

#[test]
fn cancellation_during_initial_read_does_not_poison_the_next_query() {
    let fixture = Fixture::new();
    let token = ReadCancellation::default();
    fixture
        .vfs
        .cancel_on_read(Path::new("/ws/a.p"), 0, token.clone());
    let reply =
        Fixture::value(Fixture::search().execute_with_cancellation(&fixture.engine, &token));
    assert_eq!(reply["code"], "cancelled");
    let next = Fixture::value(Fixture::search().execute(&fixture.engine));
    assert_eq!(next["result"]["items"][0]["ordinal"], 1);
}
