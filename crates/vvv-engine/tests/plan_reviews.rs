mod common;
use common::Fake;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use vvv_engine::{Call, Engine, Languages, Ledger, MemoryVfs, Vfs, Workspace};

struct Fixture {
    engine: Engine,
    vfs: Arc<MemoryVfs>,
}
impl Fixture {
    fn new(source: &str) -> Self {
        let vfs = Arc::new(
            MemoryVfs::new()
                .with_file("/ws/package", "ws")
                .with_file("/ws/a.p", source),
        );
        let engine = Engine::new(
            Workspace::new("/ws", vfs.clone()),
            Languages::new().with(Fake::default()),
        );
        Self { engine, vfs }
    }
    fn call(&self, request: Value) -> Value {
        serde_json::to_value(
            serde_json::from_value::<Call>(request)
                .unwrap()
                .execute(&self.engine),
        )
        .unwrap()
    }
    fn prepare(&self, command: &str, intent: Value, paged: bool) -> Value {
        let mut request = json!({"command":command,"intent":intent,"max_bytes":1048576});
        if paged {
            request["page"] = json!({"max_items":3,"max_bytes":2048});
        }
        let reply = self.call(request);
        assert_eq!(reply["status"], "ok", "{reply}");
        reply["result"].clone()
    }
    fn pages(&self, first: Value) -> Vec<Value> {
        let mut pages = vec![first];
        while !pages.last().unwrap()["next_cursor"].is_null() {
            assert!(pages.len() < 2000, "nonprogressing cursor");
            let request = json!({"command":"review_plan","cursor":pages.last().unwrap()["next_cursor"],"page":{"max_items":3,"max_bytes":2048}});
            let reply = self.call(request.clone());
            assert_eq!(reply["status"], "ok", "{reply}");
            assert_eq!(reply, self.call(request), "cursor replay changed");
            let page = reply["result"].clone();
            assert!(serde_json::to_vec(&page).unwrap().len() <= 2048);
            assert_eq!(page["review_id"], pages[0]["review_id"]);
            pages.push(page);
        }
        pages
    }
    fn reconstruct(pages: &[Value]) -> Value {
        let mut records = BTreeMap::<(String, usize, Option<usize>), Value>::new();
        for page in pages {
            for item in page["items"].as_array().unwrap() {
                let key = (
                    item["section"].as_str().unwrap().to_owned(),
                    item["index"].as_u64().unwrap() as usize,
                    item["file_index"].as_u64().map(|i| i as usize),
                );
                if item["kind"] == "metadata" {
                    assert!(records.insert(key, item["value"].clone()).is_none());
                } else {
                    let record = records.entry(key).or_insert_with(|| json!({"diff":""}));
                    let field = item["field"].as_str().unwrap();
                    let text = record.pointer_mut(field).unwrap();
                    let current = text.as_str().unwrap();
                    assert_eq!(current.len(), item["offset"].as_u64().unwrap() as usize);
                    let joined = format!("{current}{}", item["text"].as_str().unwrap());
                    assert_eq!(
                        joined.len() == item["total_bytes"].as_u64().unwrap() as usize,
                        item["complete"].as_bool().unwrap()
                    );
                    *text = joined.into();
                }
            }
        }
        let total = &pages[0]["totals"];
        let mut files = vec![];
        for index in 0..total["files"].as_u64().unwrap() as usize {
            let mut file = records.remove(&("file".into(), index, None)).unwrap();
            file["diff"] = records.remove(&("diff".into(), index, None)).unwrap()["diff"].clone();
            file["edits"] = records
                .iter()
                .filter(|((section, _, owner), _)| section == "edit" && *owner == Some(index))
                .map(|(_, v)| v.clone())
                .collect::<Vec<_>>()
                .into();
            files.push(file);
        }
        let mut result = json!({"intent":pages[0]["intent"],"applied":false,"files":files});
        result["intent"].as_object_mut().unwrap().remove("command");
        if pages[0]["mutation"] == "rename" {
            for (section, field) in [
                ("declaration", "declarations"),
                ("occurrence", "occurrences"),
            ] {
                result[field] = records
                    .iter()
                    .filter(|((s, _, _), _)| s == section)
                    .map(|(_, v)| v.clone())
                    .collect::<Vec<_>>()
                    .into();
            }
        }
        result
    }
}

#[test]
fn large_rewrite_reassembles_exact_edits_and_diff_then_applies_and_undoes() {
    let source = "def foo\nfoo\nfoo";
    let f = Fixture::new(source);
    let template = "é🙂\"\\\n".repeat(1200);
    let intent =
        json!({"query":{"pattern":"foo"},"template":template,"selection":{"ordinals":[2]}});
    let full = f.prepare("prepare_rewrite", intent.clone(), false);
    let prepared = f.call(json!({"command":"prepare_rewrite","intent":intent,"page":{"max_items":2,"max_bytes":65536},"max_bytes":65536}));
    assert_eq!(prepared["status"], "ok", "{prepared}");
    let first = prepared["result"].clone();
    assert_eq!(first["totals"]["edits"], 1);
    let id = first["plan_id"].clone();
    let pages = f.pages(first.clone());
    assert!(pages.len() > 10);
    assert_eq!(Fixture::reconstruct(&pages), full["preview"]);
    let inspect = f.call(
        json!({"command":"inspect_plan","plan_id":id,"page":{"max_items":2,"max_bytes":65536},"max_bytes":65536}),
    );
    assert_eq!(inspect["result"], first);
    let receipt = f.call(json!({"command":"apply_plan","plan_id":id}));
    assert_eq!(receipt["status"], "ok", "{receipt}");
    assert_eq!(
        f.vfs.read(Path::new("/ws/a.p")).unwrap(),
        format!("def foo\n{template}\nfoo")
    );
    assert_eq!(f.pages(pages[0].clone()), pages, "apply changed review");
    assert_eq!(
        f.call(json!({"command":"apply_plan","plan_id":id})),
        receipt
    );
    Ledger::new(&f.engine).undo().unwrap();
    assert_eq!(f.vfs.read(Path::new("/ws/a.p")).unwrap(), source);
    assert_eq!(
        f.call(json!({"command":"apply_plan","plan_id":id})),
        receipt
    );
}

#[test]
fn rename_pages_preserve_all_occurrences_and_survive_stale_apply_failure() {
    let source = format!("def foo\n{}", "foo é🙂\"\\\n".repeat(80));
    let f = Fixture::new(&source);
    let intent = json!({"name":"foo","to":"bar"});
    let full = f.prepare("prepare_rename", intent.clone(), false);
    let first = f.prepare("prepare_rename", intent, true);
    let id = first["plan_id"].clone();
    let pages = f.pages(first.clone());
    assert_eq!(Fixture::reconstruct(&pages), full["preview"]);
    f.vfs
        .write(Path::new("/ws/a.p"), "def foo\nuser edit")
        .unwrap();
    assert_eq!(
        f.pages(first.clone()),
        pages,
        "external edit changed review"
    );
    assert_eq!(
        f.call(json!({"command":"apply_plan","plan_id":id}))["code"],
        "stale"
    );
    assert_eq!(f.pages(first), pages, "failure discarded review");
    assert!(Ledger::new(&f.engine).history().unwrap().entries.is_empty());
}

#[test]
fn cancellation_bad_cursors_and_discard_do_not_corrupt_other_reviews() {
    let f = Fixture::new("def foo\nfoo");
    let first = f.prepare(
        "prepare_rewrite",
        json!({"query":{"pattern":"foo"},"template":"bar"}),
        true,
    );
    let cursor = first["next_cursor"].clone();
    let request =
        json!({"command":"review_plan","cursor":cursor,"page":{"max_items":3,"max_bytes":2048}});
    let cancelled = vvv_engine::ReadCancellation::default();
    cancelled.cancel();
    let call: Call = serde_json::from_value(request.clone()).unwrap();
    assert_eq!(
        serde_json::to_value(call.execute_with_cancellation(&f.engine, &cancelled)).unwrap()["code"],
        "cancelled"
    );
    assert_eq!(f.call(request)["status"], "ok");
    assert_eq!(
        f.call(json!({"command":"continue","cursor":cursor}))["code"],
        "invalid_cursor"
    );
    assert_eq!(
        f.call(json!({"command":"review_plan","cursor":"bad"}))["code"],
        "invalid_cursor"
    );
    assert_eq!(
        Fixture::new("foo").call(json!({"command":"review_plan","cursor":cursor}))["code"],
        "plan_expired"
    );
    let invalid = f.call(json!({"command":"prepare_rewrite","intent":{"query":{"pattern":"foo"},"template":"$MISSING"}}));
    assert_eq!(invalid["code"], "bad_template");
    f.call(json!({"command":"discard_plan","plan_id":first["plan_id"]}));
    assert_eq!(
        f.call(json!({"command":"review_plan","cursor":cursor}))["code"],
        "plan_consumed"
    );
}

#[test]
fn paged_preparation_fits_when_full_review_cannot_and_failed_delivery_leaks_no_handles() {
    let f = Fixture::new("def foo\nfoo");
    let intent = json!({"query":{"pattern":"foo"},"template":"bar".repeat(2000)});
    for _ in 0..20 {
        assert_eq!(
            f.call(json!({"command":"prepare_rewrite","intent":intent,"max_bytes":1024}))["code"],
            "output_limit"
        );
    }
    // Intent itself is indivisible, even in paged delivery.
    for _ in 0..20 {
        assert_eq!(
            f.call(json!({"command":"prepare_rewrite","intent":intent,"page":{"max_bytes":1024}}))
                ["code"],
            "output_limit"
        );
    }
    f.prepare(
        "prepare_rewrite",
        json!({"query":{"pattern":"foo"},"template":"bar"}),
        true,
    );
    let f = Fixture::new(&format!("def foo\n{}", "foo\n".repeat(100)));
    assert_eq!(
        f.call(
            json!({"command":"prepare_rename","intent":{"name":"foo","to":"bar"},"max_bytes":1024})
        )["code"],
        "output_limit"
    );
    let first = f.call(json!({"command":"prepare_rename","intent":{"name":"foo","to":"bar"},"page":{"max_bytes":1024}}));
    assert_eq!(first["status"], "ok", "{first}");
}

#[test]
fn cursor_budgets_can_change_and_failed_delivery_preserves_the_exact_position() {
    let f = Fixture::new(&format!("def foo\n{}", "foo é🙂\"\\\n".repeat(60)));
    let first = f.prepare("prepare_rename", json!({"name":"foo","to":"bar"}), true);
    let cursor = first["next_cursor"].clone();
    let request =
        json!({"command":"review_plan","cursor":cursor,"page":{"max_items":1,"max_bytes":1024}});
    let original = f.call(request.clone());
    assert_eq!(original["status"], "ok", "{original}");
    let bigger = f.call(
        json!({"command":"review_plan","cursor":cursor,"page":{"max_items":20,"max_bytes":8192}}),
    );
    assert_eq!(bigger["status"], "ok");
    assert_eq!(bigger["result"]["items"][0], original["result"]["items"][0]);
    assert_eq!(
        f.call(json!({"command":"review_plan","cursor":cursor,"page":{"max_bytes":12}}))["code"],
        "bad_request"
    );
    assert_eq!(f.call(request), original);
    let mut parts = cursor
        .as_str()
        .unwrap()
        .split('.')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    parts[4] = "ffffffffffffffff".into();
    assert_eq!(
        f.call(json!({"command":"review_plan","cursor":parts.join(".")}))["code"],
        "invalid_cursor"
    );
}

#[test]
fn empty_rewrite_page_is_complete_and_full_review_round_trips_with_a_closed_preview() {
    let f = Fixture::new("def foo");
    let first = f.prepare(
        "prepare_rewrite",
        json!({"query":{"pattern":"missing"},"template":"bar"}),
        true,
    );
    assert_eq!(first["totals"]["files"], 0);
    assert_eq!(first["items"], json!([]));
    assert!(first["next_cursor"].is_null());
    let inspected = f.call(json!({"command":"inspect_plan","plan_id":first["plan_id"]}));
    let review: vvv_engine::PlanReview =
        serde_json::from_value(inspected["result"].clone()).unwrap();
    assert_eq!(serde_json::to_value(review).unwrap(), inspected["result"]);
}
