#![cfg(feature = "mcp")]
use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, ClientInfo, ProtocolVersion},
    service::RunningService,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

struct Client {
    child: tokio::process::Child,
    service: RunningService<RoleClient, ClientInfo>,
    schemas: BTreeMap<String, jsonschema::Validator>,
}
impl Client {
    async fn new(root: &Path) -> Self {
        Self::with_version(root, ProtocolVersion::V_2025_11_25).await
    }
    async fn with_version(root: &Path, version: ProtocolVersion) -> Self {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_vvv"))
            .arg("-C")
            .arg(root)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let transport = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
        let mut info = ClientInfo::default();
        info.protocol_version = version.clone();
        let service = tokio::time::timeout(Duration::from_secs(15), info.serve(transport))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(service.peer_info().unwrap().protocol_version, version);
        let tools = service.list_all_tools().await.unwrap();
        assert_eq!(tools.len(), 17);
        assert!(
            tools
                .iter()
                .all(|t| t.annotations.as_ref().unwrap().read_only_hint
                    == Some(!matches!(
                        t.name.as_ref(),
                        "vvv_apply_plan" | "vvv_validate_plan"
                    )))
        );
        for tool in &tools {
            assert_eq!(
                tool.annotations.as_ref().unwrap().open_world_hint,
                Some(tool.name == "vvv_validate_plan")
            );
        }
        let schemas = tools
            .iter()
            .map(|tool| {
                (
                    tool.name.to_string(),
                    jsonschema::validator_for(&Value::Object(
                        tool.output_schema.as_ref().unwrap().as_ref().clone(),
                    ))
                    .unwrap(),
                )
            })
            .collect();
        for tool in tools {
            jsonschema::draft202012::meta::validate(&Value::Object(
                tool.input_schema.as_ref().clone(),
            ))
            .unwrap();
        }
        Self {
            child,
            service,
            schemas,
        }
    }
    async fn call(&self, tool: &'static str, arguments: Value) -> Value {
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            self.service.call_tool(
                CallToolRequestParams::new(tool)
                    .with_arguments(arguments.as_object().unwrap().clone()),
            ),
        )
        .await
        .unwrap_or_else(|_| panic!("{tool} {arguments}: timed out"))
        .unwrap_or_else(|error| panic!("{tool} {arguments}: {error}"));
        let value = result.structured_content.unwrap();
        assert_eq!(result.is_error, Some(value["status"] == "error"));
        assert!(
            self.schemas[tool].is_valid(&value),
            "schema mismatch: {value}"
        );
        let text = result.content[0].as_text().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text.text).unwrap(), value);
        value
    }
    async fn close(mut self) {
        self.service.cancel().await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(10), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
        use tokio::io::AsyncReadExt;
        let mut diagnostics = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut diagnostics)
            .await
            .unwrap();
        assert!(diagnostics.is_empty(), "{diagnostics}");
    }
    #[cfg(feature = "rust")]
    fn normalized(mut value: Value) -> Value {
        match &mut value {
            Value::Object(map) => {
                for (name, value) in map {
                    if ["next_cursor", "expansion", "body_expansion"].contains(&name.as_str())
                        && value.is_string()
                    {
                        *value = json!("<cursor>");
                    } else {
                        *value = Self::normalized(value.take());
                    }
                }
            }
            Value::Array(items) => {
                for value in items {
                    *value = Self::normalized(value.take());
                }
            }
            _ => {}
        }
        value
    }
}
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "vvv-mcp-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let source = format!(
            "pub struct Engine {{ pub running: bool }}\npub fn run(engine: Engine) -> Engine {{\n{}    engine\n}}\npub fn other(engine: Engine) -> Engine {{ run(engine) }}\n",
            "    // é \\\" 🙂 context\r\n".repeat(100)
        );
        std::fs::write(root.join("src/lib.rs"), source).unwrap();
        Self { root }
    }
    #[cfg(feature = "rust")]
    fn engine_call(&self, request: Value) -> Value {
        let engine = vvv_engine::Engine::new(
            vvv_engine::Workspace::disk(&self.root).unwrap(),
            vvv_rs::Builtins::registry(),
        );
        let call: vvv_engine::Call = serde_json::from_value(request).unwrap();
        serde_json::to_value(call.execute(&engine).response).unwrap()
    }
    #[cfg(feature = "rust")]
    fn serve_call(&self, request: Value) -> Value {
        use std::io::Write;
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_vvv"))
            .arg("-C")
            .arg(&self.root)
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn sdk_client_initializes_discovers_schemas_and_rejects_mutation_and_bad_arguments() {
    let fixture = Fixture::new();
    let client = Client::new(&fixture.root).await;
    let discovery = client.call("vvv_discover", json!({})).await;
    assert_eq!(discovery["status"], "ok");
    for request in [
        CallToolRequestParams::new("vvv_rename"),
        CallToolRequestParams::new("vvv_search").with_arguments(
            json!({"query":{"pattern":"Engine"},"command":"undo"})
                .as_object()
                .unwrap()
                .clone(),
        ),
        CallToolRequestParams::new("vvv_context")
            .with_arguments(json!({"origin":4}).as_object().unwrap().clone()),
    ] {
        assert!(client.service.call_tool(request).await.is_err());
    }
    let bad = client.call("vvv_continue", json!({"cursor":"bad"})).await;
    assert_eq!(bad["code"], "invalid_cursor");
    client.close().await;
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn sdk_context_workflow_matches_engine_and_serve_and_expands_exact_source() {
    let fixture = Fixture::new();
    let client = Client::new(&fixture.root).await;
    let arguments = json!({"query":{"pattern":"Engine"},"page":{"max_items":1,"max_bytes":4096}});
    let first = client.call("vvv_search", arguments.clone()).await;
    assert_eq!(first["status"], "ok");
    let selected = &first["result"]["items"][0];
    let followed = client
        .call(
            "vvv_navigate",
            json!({"origin":{
                "kind":"position", "path":selected["path"], "position":selected["start"],
                "expected_content":selected["content"]
            }}),
        )
        .await;
    assert_eq!(followed["result"]["outcome"], "resolved");
    let request = json!({"command":"search_page","query":arguments["query"],"page":arguments["page"],"max_output_bytes":16384});
    assert_eq!(
        Client::normalized(first.clone()),
        Client::normalized(fixture.engine_call(request.clone()))
    );
    assert_eq!(
        Client::normalized(first.clone()),
        Client::normalized(fixture.serve_call(request))
    );
    let next = client
        .call(
            "vvv_continue",
            json!({"cursor":first["result"]["next_cursor"],"page":{"max_items":1}}),
        )
        .await;
    assert_eq!(next["result"]["items"][0]["ordinal"], 2);
    let again = client
        .call(
            "vvv_continue",
            json!({"cursor":first["result"]["next_cursor"],"page":{"max_items":1}}),
        )
        .await;
    assert_eq!(next, again);
    let origin = json!({"kind":"position","path":"src/lib.rs","position":{"line":1,"column":7}});
    let nav = client
        .call(
            "vvv_navigate",
            json!({"origin":origin,"max_output_bytes":1048576}),
        )
        .await;
    assert_eq!(nav["result"]["outcome"], "resolved");
    let compact = client
        .call(
            "vvv_navigate",
            json!({"origin":origin,"max_output_bytes":1024}),
        )
        .await;
    assert_eq!(compact["status"], "ok");
    assert!(compact["result"].get("preview").is_none());
    assert!(serde_json::to_vec(&compact["result"]).unwrap().len() <= 1024);
    assert_eq!(
        fixture.engine_call(json!({"command":"navigate","origin":origin,"max_output_bytes":1024}))
            ["code"],
        "output_limit"
    );
    let context = client
        .call(
            "vvv_context",
            json!({"origin":origin,"page":{"max_items":1,"max_bytes":2048}}),
        )
        .await;
    assert_eq!(context["status"], "ok");
    let item = &context["result"]["items"][0];
    assert_eq!(item["complete"], false);
    let mut text = item["text"].as_str().unwrap().to_owned();
    let mut cursor = item["expansion"].clone();
    let mut chunks = 0;
    while !cursor.is_null() {
        chunks += 1;
        assert!(chunks < 50);
        let chunk = client
            .call("vvv_expand", json!({"cursor":cursor,"max_bytes":2048}))
            .await;
        assert_eq!(chunk["status"], "ok");
        text.push_str(chunk["result"]["text"].as_str().unwrap());
        cursor = chunk["result"]["next_cursor"].clone();
    }
    let source = std::fs::read(fixture.root.join("src/lib.rs")).unwrap();
    let span = &item["target"]["declaration"]["span"];
    assert_eq!(
        text.as_bytes(),
        &source[span["start"].as_u64().unwrap() as usize..span["end"].as_u64().unwrap() as usize]
    );
    let related = client
        .call(
            "vvv_continue",
            json!({"cursor":context["result"]["next_cursor"]}),
        )
        .await;
    assert_eq!(related["result"]["kind"], "context");
    std::fs::write(fixture.root.join("src/new.rs"), "pub struct New;\n").unwrap();
    let stale = client
        .call(
            "vvv_continue",
            json!({"cursor":first["result"]["next_cursor"]}),
        )
        .await;
    assert_eq!(stale["code"], "stale");
    assert_eq!(stale["continuation"], "restart_query");
    let expired = client
        .call(
            "vvv_continue",
            json!({"cursor":first["result"]["next_cursor"]}),
        )
        .await;
    assert_eq!(expired["code"], "cursor_expired");
    client.close().await;
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn ambiguous_and_unavailable_navigation_are_successful_schema_valid_data() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/rust-navigation");
    let client = Client::new(&root).await;
    for (line, column, expected) in [(2, 25, "ambiguous"), (0, 3, "unavailable")] {
        let reply = client.call("vvv_navigate", json!({"origin":{"kind":"position","path":"src/ambiguous.rs","position":{"line":line,"column":column}},"max_output_bytes":1048576})).await;
        assert_eq!(reply["status"], "ok", "{reply}");
        assert_eq!(reply["result"]["outcome"], expected, "{reply}");
    }
    client.close().await;
}

#[tokio::test]
async fn codex_protocol_initializes_lists_tools_and_calls_discovery() {
    let fixture = Fixture::new();
    let client = Client::with_version(&fixture.root, ProtocolVersion::V_2025_06_18).await;
    let reply = client.call("vvv_discover", json!({})).await;
    assert_eq!(reply["status"], "ok", "{reply}");
    client.close().await;
}

#[tokio::test]
async fn unsupported_protocol_and_oversized_input_exit_without_non_protocol_stdout() {
    use tokio::io::AsyncWriteExt;
    let fixture = Fixture::new();
    for input in [
        format!(
            "{}\n",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}})
        ),
        "x".repeat(65537),
    ] {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_vvv"))
            .arg("-C")
            .arg(&fixture.root)
            .arg("--json")
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .await
            .unwrap();
        let output = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
            .await
            .unwrap()
            .unwrap();
        for line in output
            .stdout
            .split(|b| *b == b'\n')
            .filter(|s| !s.is_empty())
        {
            let message: Value = serde_json::from_slice(line).unwrap();
            assert_eq!(message["jsonrpc"], "2.0");
            assert!(message["error"].is_object());
        }
    }
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn scoped_search_and_compact_context_work_for_real_packages_and_methods() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("Cargo.toml"),
        "[package]\nname = \"probe-package\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("src/lib.rs"),
        "pub struct Engine;\nimpl Engine { pub fn method() {} }\n",
    )
    .unwrap();
    std::fs::create_dir_all(fixture.root.join("src/nested/src")).unwrap();
    std::fs::write(
        fixture.root.join("src/nested/Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("src/nested/src/lib.rs"),
        "pub struct Engine;\n",
    )
    .unwrap();
    let client = Client::new(&fixture.root).await;
    for package in ["probe-package", "probe_package"] {
        let result = client.call("vvv_search", json!({"query":{"name":"Engine","symbol":"struct"},"scope":{"paths":["src"],"packages":[package]}})).await;
        assert_eq!(result["result"]["total_items"], 1, "{result}");
        assert_eq!(result["result"]["items"][0]["path"], "src/lib.rs");
    }
    let found = client
        .call("vvv_search", json!({"query":{"name":"method"}}))
        .await;
    let item = &found["result"]["items"][0];
    let origin = json!({"kind":"occurrence","anchor":{"path":item["path"],"content":item["content"],"span":item["symbol"]["name_span"]}});
    let compact = client.call("vvv_context", json!({"origin":origin})).await;
    let owner = &compact["result"]["enclosing"];
    assert_eq!(owner["kind"], "impl");
    assert!(
        compact["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| &i["target"] != owner)
    );
    let full = client
        .call(
            "vvv_context",
            json!({"origin":origin,"include_enclosing":true}),
        )
        .await;
    assert!(
        full["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| &i["target"] == owner && i["relation"] == "enclosing_declaration")
    );
    client.close().await;
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn relationships_follow_import_aliases_and_preserve_unresolved_call_sites() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("src/lib.rs"),
        "pub mod origin;\npub mod consumer;\npub mod bridge;\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("src/origin.rs"),
        "pub fn work() {}\npub fn other() {}\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("src/bridge.rs"),
        "pub use crate::origin::work as task;\n",
    )
    .unwrap();
    std::fs::write(fixture.root.join("src/consumer.rs"), "use crate::bridge::task as execute;\nuse crate::origin::other as ignore;\npub fn caller() { execute(); ignore(); }\npub fn unknown(receiver: Unknown) { receiver.work(); }\npub fn shadow(execute: fn()) { execute(); }\n").unwrap();
    let client = Client::new(&fixture.root).await;
    let args = json!({"origin":{"kind":"position","path":"src/origin.rs","position":{"line":0,"column":7}},"kind":"callers","scope":{"paths":["src/consumer.rs"]}});
    let reply = client.call("vvv_relationships", args.clone()).await;
    assert_eq!(reply, fixture.engine_call(json!({"command":"relationships","origin":args["origin"],"kind":"callers","scope":args["scope"]})), "{reply}");
    let items = reply["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3, "{reply}");
    assert_eq!(items[0]["spelling"], "execute");
    assert_eq!(items[0]["outcome"], "confirmed");
    assert_eq!(items[0]["target"]["declaration"]["path"], "src/origin.rs");
    assert!(items[0]["evidence"]["addresses"].as_array().unwrap().len() >= 2);
    assert_eq!(items[1]["outcome"], "unavailable");
    assert_eq!(items[2]["outcome"], "indirect");
    assert_eq!(reply["result"]["coverage"]["scan_complete"], true);
    let limited = client.call("vvv_relationships", json!({"origin":args["origin"],"kind":"callers","scope":args["scope"],"budget":{"max_lookups":1}})).await;
    assert_eq!(limited["result"]["coverage"]["stopped_by"], "lookups");
    let references = client
        .call(
            "vvv_relationships",
            json!({"origin":args["origin"],"kind":"references","scope":args["scope"]}),
        )
        .await;
    assert!(
        references["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["spelling"] == "execute" && i["outcome"] == "confirmed")
    );
    client.close().await;
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn signature_context_matches_serve_and_expands_both_views_through_sdk() {
    let fixture = Fixture::new();
    let signature = format!(
        "{}pub fn run<T: Copy>(input: T) -> T where T: Send",
        "/// é🙂 documentation\r\n".repeat(120)
    );
    let source = format!(
        "{signature} {{\n{} input\n}}",
        "// implementation\n".repeat(120)
    );
    std::fs::write(fixture.root.join("src/lib.rs"), &source).unwrap();
    let client = Client::new(&fixture.root).await;
    let arguments = json!({"origin":{"kind":"position","path":"src/lib.rs","position":{"line":120,"column":7}},"detail":"signature","page":{"max_items":1,"max_bytes":2048}});
    let context = client.call("vvv_context", arguments.clone()).await;
    assert_eq!(context["status"], "ok");
    let mut request = arguments;
    request["command"] = "context_page".into();
    request["max_output_bytes"] = 16384.into();
    assert_eq!(
        Client::normalized(context.clone()),
        Client::normalized(fixture.engine_call(request.clone()))
    );
    assert_eq!(
        Client::normalized(context.clone()),
        Client::normalized(fixture.serve_call(request))
    );
    let item = &context["result"]["items"][0];
    assert_eq!(item["signature"]["outcome"], "available");
    assert_eq!(item["complete"], false);
    for (handle, initial, expected) in [
        (
            "expansion",
            item["text"].as_str().unwrap(),
            signature.as_str(),
        ),
        ("body_expansion", "", source.as_str()),
    ] {
        let mut cursor = item[handle].clone();
        assert!(cursor.is_string());
        let mut text = initial.to_owned();
        while !cursor.is_null() {
            let arguments = json!({"cursor":cursor,"max_bytes":2048});
            let reply = client.call("vvv_expand", arguments.clone()).await;
            assert_eq!(reply["status"], "ok");
            assert_eq!(reply, client.call("vvv_expand", arguments).await);
            let result = &reply["result"];
            assert!(serde_json::to_vec(result).unwrap().len() <= 2048);
            assert_eq!(result["requested"]["span"]["end"], expected.len());
            assert_eq!(result["excerpt"]["span"]["start"], text.len());
            text.push_str(result["text"].as_str().unwrap());
            cursor = result["next_cursor"].clone();
        }
        assert_eq!(text, expected);
    }
    client.close().await;
}

#[cfg(any(feature = "rust", feature = "typescript"))]
#[tokio::test]
async fn reviewed_rename_applies_once_and_stale_plans_never_write() {
    let fixture = Fixture::new();
    #[cfg(feature = "rust")]
    let path = "src/lib.rs";
    #[cfg(all(not(feature = "rust"), feature = "typescript"))]
    let path = "src/lib.ts";
    #[cfg(all(not(feature = "rust"), feature = "typescript"))]
    {
        std::fs::write(fixture.root.join("package.json"), r#"{"name":"probe"}"#).unwrap();
        std::fs::write(fixture.root.join(path), "export class Engine {}\nexport function run(engine: Engine): Engine { return engine; }\n").unwrap();
    }
    let before = std::fs::read_to_string(fixture.root.join(path)).unwrap();
    let client = Client::new(&fixture.root).await;
    let prepared = client.call("vvv_prepare_rename", json!({"intent":{"name":"Engine","to":"Runtime","declared_in":path},"max_bytes":65536,"max_output_bytes":65536})).await;
    assert_eq!(prepared["status"], "ok", "{prepared}");
    assert_eq!(prepared["result"]["state"], "prepared");
    assert_eq!(prepared["result"]["preview"]["applied"], false);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(path)).unwrap(),
        before
    );
    let handle = json!({"plan_id":prepared["result"]["plan_id"]});
    assert_eq!(
        client.call("vvv_inspect_plan", handle.clone()).await,
        prepared
    );
    let invalid_budget = client
        .service
        .call_tool(
            CallToolRequestParams::new("vvv_apply_plan").with_arguments(
                json!({"plan_id":prepared["result"]["plan_id"],"max_output_bytes":1024})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await;
    assert!(invalid_budget.is_err());
    let applied = client.call("vvv_apply_plan", handle.clone()).await;
    assert_eq!(applied["status"], "ok", "{applied}");
    assert_eq!(client.call("vvv_apply_plan", handle.clone()).await, applied);
    assert_eq!(
        client.call("vvv_inspect_plan", handle.clone()).await["result"]["receipt"],
        applied["result"]
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(path)).unwrap(),
        before.replace("Engine", "Runtime")
    );
    #[cfg(any(unix, windows))]
    {
        let checked = client.call("vvv_validate_plan", json!({
            "plan_id": handle["plan_id"],
            "checks": [{"name":"source assertion", "program":std::env::current_exe().unwrap(),"args":["--exact","validation_source_fixture","--ignored","--nocapture"]}],
            "budget":{"timeout_ms":2000,"max_bytes":4096}
        })).await;
        assert_eq!(checked["status"], "ok", "{checked}");
        assert_eq!(checked["result"]["passed"], true, "{checked}");
        assert_eq!(checked["result"]["sources"], applied["result"]["files"]);
        assert!(serde_json::to_vec(&checked["result"]).unwrap().len() <= 4096);
        let inspected = client.call("vvv_inspect_plan", handle.clone()).await;
        assert_eq!(inspected["result"]["validation"], checked["result"]);
        assert_eq!(client.call("vvv_apply_plan", handle.clone()).await, applied);
    }
    let next = client.call("vvv_prepare_rename", json!({"intent":{"name":"Runtime","to":"Worker","declared_in":path},"max_bytes":65536,"max_output_bytes":65536})).await;
    assert_eq!(next["status"], "ok");
    let changed = format!("{}\n// editor change", before.replace("Engine", "Runtime"));
    std::fs::write(fixture.root.join(path), &changed).unwrap();
    let next_handle = json!({"plan_id":next["result"]["plan_id"]});
    let stale = client.call("vvv_apply_plan", next_handle.clone()).await;
    assert_eq!(stale["code"], "stale");
    assert_eq!(
        client.call("vvv_inspect_plan", next_handle.clone()).await["result"]["failure"]["code"],
        "stale"
    );
    assert_eq!(
        client.call("vvv_apply_plan", next_handle).await["code"],
        "plan_consumed"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(path)).unwrap(),
        changed
    );
    let discard = client.call("vvv_prepare_rename", json!({"intent":{"name":"Runtime","to":"Worker","declared_in":path},"max_bytes":65536,"max_output_bytes":65536})).await;
    let handle = json!({"plan_id":discard["result"]["plan_id"]});
    assert_eq!(
        client.call("vvv_discard_plan", handle.clone()).await["result"]["state"],
        "discarded"
    );
    assert_eq!(
        client.call("vvv_apply_plan", handle).await["code"],
        "plan_consumed"
    );
    client.close().await;
}

#[cfg(all(unix, any(feature = "rust", feature = "typescript")))]
#[test]
#[ignore = "subprocess used by MCP validation workflow"]
fn validation_source_fixture() {
    #[cfg(feature = "rust")]
    let path = "src/lib.rs";
    #[cfg(all(not(feature = "rust"), feature = "typescript"))]
    let path = "src/lib.ts";
    let source = std::fs::read_to_string(path).unwrap();
    assert!(source.contains("Runtime"));
    assert!(!source.contains("Engine"));
    println!("renamed source checked");
}

#[cfg(any(feature = "rust", feature = "typescript"))]
#[tokio::test]
async fn paged_rewrite_reviews_exact_capture_expansions_and_applies_once() {
    let fixture = Fixture::new();
    #[cfg(feature = "rust")]
    let (path, header, function) = (
        "src/lib.rs",
        "fn increment(value: i32, amount: i32) -> i32 { value + amount }\n",
        "pub fn value_INDEX(value: i32) -> i32 { increment(value, 1) }\n",
    );
    #[cfg(all(not(feature = "rust"), feature = "typescript"))]
    let (path, header, function) = (
        "src/lib.ts",
        "function increment(value: number, amount: number) { return value + amount; }\n",
        "export function value_INDEX(value: number) { return increment(value, 1); }\n",
    );
    let before = format!(
        "{header}{}",
        (0..30)
            .map(|i| function.replace("INDEX", &i.to_string()))
            .collect::<String>()
    );
    std::fs::write(fixture.root.join(path), &before).unwrap();
    let client = Client::new(&fixture.root).await;
    let prepared = client.call("vvv_prepare_rewrite", json!({"intent":{"query":{"pattern":"increment($X, 1)"},"template":"increment($X, 2)"},"page":{"max_items":3,"max_bytes":1024},"max_output_bytes":1024})).await;
    assert_eq!(prepared["status"], "ok", "{prepared}");
    let first = prepared["result"].clone();
    assert_eq!(first["totals"]["edits"], 30);
    let handle = json!({"plan_id":first["plan_id"]});
    let mut page = first.clone();
    let mut replacements = BTreeMap::<(u64, u64), String>::new();
    let mut page_count = 0;
    loop {
        assert!(serde_json::to_vec(&page).unwrap().len() <= 1024);
        assert_eq!(page["review_id"], first["review_id"]);
        for item in page["items"].as_array().unwrap() {
            if item["kind"] == "text" && item["section"] == "edit" {
                let text = replacements
                    .entry((
                        item["file_index"].as_u64().unwrap(),
                        item["index"].as_u64().unwrap(),
                    ))
                    .or_default();
                assert_eq!(text.len() as u64, item["offset"].as_u64().unwrap());
                text.push_str(item["text"].as_str().unwrap());
            }
        }
        page_count += 1;
        assert!(page_count < 300);
        if page["next_cursor"].is_null() {
            break;
        }
        let args = json!({"cursor":page["next_cursor"],"page":{"max_items":3,"max_bytes":1024},"max_output_bytes":1024});
        let continued = client.call("vvv_review_plan", args.clone()).await;
        assert_eq!(continued["status"], "ok", "{continued}");
        assert_eq!(client.call("vvv_review_plan", args).await, continued);
        page = continued["result"].clone();
    }
    assert!(page_count > 10);
    assert_eq!(replacements.len(), 30);
    assert!(replacements.values().all(|r| r == "increment(value, 2)"));
    let inspected = client.call("vvv_inspect_plan", json!({"plan_id":handle["plan_id"],"page":{"max_items":3,"max_bytes":1024},"max_output_bytes":1024})).await;
    assert_eq!(inspected, prepared);
    let applied = client.call("vvv_apply_plan", handle.clone()).await;
    assert_eq!(applied["status"], "ok", "{applied}");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join(path)).unwrap(),
        before.replace("increment(value, 1)", "increment(value, 2)")
    );
    assert_eq!(client.call("vvv_apply_plan", handle).await, applied);
    if !first["next_cursor"].is_null() {
        assert_eq!(
            client
                .call(
                    "vvv_review_plan",
                    json!({"cursor":first["next_cursor"],"page":{"max_items":3,"max_bytes":1024}})
                )
                .await["status"],
            "ok"
        );
    }
    client.close().await;
}

#[cfg(any(feature = "rust", feature = "typescript"))]
#[tokio::test]
async fn reviewed_move_pages_apply_validate_and_preserve_the_receipt() {
    let fixture = Fixture::new();
    #[cfg(feature = "rust")]
    let (from, to) = ("src/origin.rs", "src/relocated.rs");
    #[cfg(all(not(feature = "rust"), feature = "typescript"))]
    let (from, to) = ("src/origin.ts", "src/relocated.ts");
    #[cfg(feature = "rust")]
    {
        std::fs::write(
            fixture.root.join("src/lib.rs"),
            "pub mod origin;\npub use origin::Engine;\n",
        )
        .unwrap();
        std::fs::write(fixture.root.join(from), "pub struct Engine;\n").unwrap();
    }
    #[cfg(all(not(feature = "rust"), feature = "typescript"))]
    {
        std::fs::write(fixture.root.join("package.json"), "{\"name\":\"probe\"}").unwrap();
        std::fs::write(
            fixture.root.join("src/lib.ts"),
            "export { Engine } from './origin';\n",
        )
        .unwrap();
        std::fs::write(fixture.root.join(from), "export class Engine {}\n").unwrap();
    }
    let client = Client::new(&fixture.root).await;
    let prepared = client.call("vvv_prepare_move", json!({"intent":{"from":from,"to":to},"page":{"max_items":2,"max_bytes":1024},"max_output_bytes":1024})).await;
    assert_eq!(prepared["status"], "ok", "{prepared}");
    let first = prepared["result"].clone();
    assert_eq!(first["mutation"], "move");
    let mut page = first.clone();
    let mut count = 0;
    loop {
        assert!(serde_json::to_vec(&page).unwrap().len() <= 1024);
        count += 1;
        assert!(count < 200);
        if page["next_cursor"].is_null() {
            break;
        }
        let result = client.call("vvv_review_plan", json!({"cursor":page["next_cursor"],"page":{"max_items":2,"max_bytes":1024},"max_output_bytes":1024})).await;
        assert_eq!(result["status"], "ok", "{result}");
        page = result["result"].clone();
    }
    assert!(count > 1);
    let handle = json!({"plan_id":first["plan_id"]});
    let applied = client.call("vvv_apply_plan", handle.clone()).await;
    assert_eq!(applied["status"], "ok", "{applied}");
    assert!(!fixture.root.join(from).exists());
    assert!(fixture.root.join(to).exists());
    assert!(
        applied["result"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["path"] == to)
    );
    #[cfg(any(unix, windows))]
    {
        let checked = client.call("vvv_validate_plan", json!({"plan_id":handle["plan_id"],"checks":[{"name":"native move assertion","program":std::env::current_exe().unwrap(),"args":["--exact","move_validation_fixture","--ignored","--nocapture"]}],"budget":{"timeout_ms":5000,"max_bytes":4096}})).await;
        assert_eq!(checked["status"], "ok", "{checked}");
        assert_eq!(checked["result"]["passed"], true, "{checked}");
        let inspected = client.call("vvv_inspect_plan", handle.clone()).await;
        assert_eq!(inspected["result"]["validation"], checked["result"]);
        assert_eq!(inspected["result"]["receipt"], applied["result"]);
    }
    assert_eq!(client.call("vvv_apply_plan", handle).await, applied);
    client.close().await;
}

#[test]
#[ignore = "subprocess fixture used by the MCP move validation workflow"]
fn move_validation_fixture() {
    let extension = if Path::new("src/relocated.rs").exists() {
        "rs"
    } else {
        "ts"
    };
    assert!(Path::new(&format!("src/relocated.{extension}")).exists());
    assert!(!Path::new(&format!("src/origin.{extension}")).exists());
}

#[cfg(any(feature = "rust", feature = "typescript"))]
#[tokio::test]
async fn symbol_move_candidates_select_review_apply_and_validate_exact_pieces() {
    let fixture = Fixture::new();
    #[cfg(feature = "rust")]
    let (from, to, source, destination) = (
        "src/origin.rs",
        "src/destination.rs",
        "/// retained docs\n#[derive(Clone)]\npub struct Engine;\nimpl Engine { pub fn answer() -> i32 { 42 } }\n",
        "pub fn untouched() {}\n",
    );
    #[cfg(all(not(feature = "rust"), feature = "typescript"))]
    let (from, to, source, destination) = (
        "src/origin.ts",
        "src/destination.ts",
        "/** retained docs */\nexport class Engine { answer() { return 42; } }\n",
        "export function untouched() {}\n",
    );
    #[cfg(feature = "rust")]
    std::fs::write(
        fixture.root.join("src/lib.rs"),
        "pub mod origin;\npub mod destination;\npub use origin::Engine;\n",
    )
    .unwrap();
    #[cfg(all(not(feature = "rust"), feature = "typescript"))]
    {
        std::fs::write(fixture.root.join("package.json"), "{\"name\":\"probe\"}").unwrap();
        std::fs::write(
            fixture.root.join("src/lib.ts"),
            "export { Engine } from './origin';\n",
        )
        .unwrap();
    }
    std::fs::write(fixture.root.join(from), source).unwrap();
    std::fs::write(fixture.root.join(to), destination).unwrap();
    let client = Client::new(&fixture.root).await;
    let discovered = client
        .call(
            "vvv_symbol_move_candidates",
            json!({"name":"Engine", "from":from}),
        )
        .await;
    assert_eq!(discovered["status"], "ok", "{discovered}");
    assert_eq!(
        discovered["result"]["candidates"].as_array().unwrap().len(),
        1
    );
    assert!(discovered["result"]["candidates"][0]["unsupported"].is_null());
    let prepared = client.call("vvv_prepare_move_symbol", json!({"intent":{"name":"Engine","from":from,"to":to,"selection":{"ids":[discovered["result"]["candidates"][0]["declaration"]["id"]]},"expected_content":discovered["result"]["content"]},"page":{"max_items":2,"max_bytes":2048},"max_output_bytes":2048})).await;
    assert_eq!(prepared["status"], "ok", "{prepared}");
    assert_eq!(prepared["result"]["mutation"], "move_symbol");
    let mut page = prepared["result"].clone();
    let mut count = 0;
    loop {
        assert!(serde_json::to_vec(&page).unwrap().len() <= 2048);
        count += 1;
        assert!(count < 200);
        if page["next_cursor"].is_null() {
            break;
        }
        let reviewed = client.call("vvv_review_plan", json!({"cursor":page["next_cursor"],"page":{"max_items":2,"max_bytes":2048},"max_output_bytes":2048})).await;
        assert_eq!(reviewed["status"], "ok", "{reviewed}");
        page = reviewed["result"].clone();
    }
    assert!(count > 1);
    let handle = json!({"plan_id":prepared["result"]["plan_id"]});
    let applied = client.call("vvv_apply_plan", handle.clone()).await;
    assert_eq!(applied["status"], "ok", "{applied}");
    assert!(
        !std::fs::read_to_string(fixture.root.join(from))
            .unwrap()
            .contains("struct Engine")
    );
    assert!(
        std::fs::read_to_string(fixture.root.join(to))
            .unwrap()
            .contains("retained docs")
    );
    #[cfg(any(unix, windows))]
    {
        let checked = client.call("vvv_validate_plan", json!({"plan_id":handle["plan_id"],"checks":[{"name":"native symbol assertion","program":std::env::current_exe().unwrap(),"args":["--exact","symbol_validation_fixture","--ignored","--nocapture"]}],"budget":{"timeout_ms":5000,"max_bytes":4096}})).await;
        assert_eq!(checked["status"], "ok", "{checked}");
        assert_eq!(checked["result"]["passed"], true, "{checked}");
        let inspected = client
            .call(
                "vvv_inspect_plan",
                json!({"plan_id":handle["plan_id"],"max_bytes":65536,"max_output_bytes":65536}),
            )
            .await;
        assert_eq!(inspected["result"]["validation"], checked["result"]);
        assert_eq!(inspected["result"]["receipt"], applied["result"]);
    }
    assert_eq!(client.call("vvv_apply_plan", handle).await, applied);
    client.close().await;
}

#[test]
#[ignore = "subprocess fixture used by the MCP symbol move validation workflow"]
fn symbol_validation_fixture() {
    let extension = if Path::new("src/destination.rs").exists() {
        "rs"
    } else {
        "ts"
    };
    let source = std::fs::read_to_string(format!("src/origin.{extension}")).unwrap();
    let destination = std::fs::read_to_string(format!("src/destination.{extension}")).unwrap();
    assert!(!source.contains("struct Engine") && !source.contains("class Engine"));
    assert!(
        destination.contains("retained docs")
            && destination.contains("answer")
            && destination.contains("untouched")
    );
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn codex_client_follows_parent_aliases_and_reviews_the_confirmed_reference() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("Cargo.toml"),
        "[package]\nname = \"alias-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    std::fs::write(
        fixture.root.join("src/lib.rs"),
        "pub mod a;\npub mod parent;\n",
    )
    .unwrap();
    std::fs::write(fixture.root.join("src/a.rs"), "pub struct Foo;\n").unwrap();
    std::fs::write(
        fixture.root.join("src/parent.rs"),
        "use crate::a as parent;\npub mod nested;\n",
    )
    .unwrap();
    std::fs::create_dir_all(fixture.root.join("src/parent")).unwrap();
    std::fs::write(
        fixture.root.join("src/parent/nested.rs"),
        "use super::parent as local;\npub fn from_parent(_: local::Foo) {}\n",
    )
    .unwrap();
    let client = Client::with_version(&fixture.root, ProtocolVersion::V_2025_06_18).await;
    let origin = json!({"kind":"position", "path":"src/parent/nested.rs", "position":{"line":1,"column":29}});
    let navigation = client.call("vvv_navigate", json!({"origin":origin})).await;
    assert_eq!(navigation["result"]["outcome"], "resolved", "{navigation}");
    assert_eq!(
        navigation["result"]["target"]["declaration"]["path"],
        "src/a.rs"
    );
    let context = client
        .call("vvv_context", json!({"origin":origin,"detail":"signature"}))
        .await;
    assert_eq!(context["result"]["outcome"], "resolved", "{context}");
    assert_eq!(context["result"]["items"][0]["text"], "pub struct Foo;");
    let review = client.call("vvv_prepare_rename", json!({"intent":{"name":"Foo","to":"Renamed","declared_in":"src/a.rs"},"max_bytes":32768,"max_output_bytes":65536})).await;
    assert_eq!(review["status"], "ok", "{review}");
    let occurrence = review["result"]["preview"]["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["path"] == "src/parent/nested.rs")
        .unwrap();
    assert_eq!(occurrence["confidence"], "resolved");
    client
        .call(
            "vvv_discard_plan",
            json!({"plan_id":review["result"]["plan_id"]}),
        )
        .await;
    client.close().await;
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn codex_client_navigates_inline_helpers_and_scoped_imports() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("Cargo.toml"),
        "[package]\nname=\"inline-probe\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    let source = "pub mod inner { pub struct Root; }\npub use inner::{Root as First};\npub use inner::{Root as Second};\npub fn production(_: First) -> u32 { 42 }\n#[cfg(test)]\nmod tests {\n    use super::production as answer;\n    use super::Second as Input;\n    fn helper(value: Input) -> u32 { answer(value) }\n    fn exercise(value: Input) -> u32 { helper(value) }\n}\n";
    std::fs::write(fixture.root.join("src/lib.rs"), source).unwrap();
    let client = Client::with_version(&fixture.root, ProtocolVersion::V_2025_06_18).await;
    let helper_column = source
        .lines()
        .nth(9)
        .unwrap()
        .find("helper(value)")
        .unwrap();
    let origin =
        json!({"kind":"position","path":"src/lib.rs","position":{"line":9,"column":helper_column}});
    let reply = client.call("vvv_navigate", json!({"origin":origin})).await;
    assert_eq!(reply["result"]["name"], "helper", "{reply}");
    assert_eq!(reply["result"]["start"]["line"], 8);
    let context = client
        .call("vvv_context", json!({"origin":origin,"detail":"signature"}))
        .await;
    assert_eq!(context["result"]["outcome"], "resolved", "{context}");
    assert!(
        context["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["text"]
                .as_str()
                .unwrap()
                .contains("fn helper(value: Input) -> u32"))
    );
    let callers = client
        .call(
            "vvv_relationships",
            json!({"origin":{"kind":"symbol","symbol":reply["result"]["target"]},"kind":"callers"}),
        )
        .await;
    assert_eq!(callers["status"], "ok", "{callers}");
    assert!(
        callers["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["spelling"] == "helper" && item["outcome"] == "confirmed"),
        "{callers}"
    );
    let type_column = source.lines().nth(8).unwrap().find("Input").unwrap();
    let imported = client.call("vvv_navigate", json!({"origin":{"kind":"position","path":"src/lib.rs","position":{"line":8,"column":type_column}}})).await;
    assert_eq!(imported["result"]["name"], "Root", "{imported}");
    client.close().await;
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn codex_client_preserves_macro_uncertainty_and_recovers_proven_bindings() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("Cargo.toml"),
        "[package]\nname=\"macro-probe\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    let source = "pub fn helper() -> usize { 1 }\nfn exercise(input: usize) {\n    let before = input;\n    assert_eq!(before, 1);\n    helper();\n    consume(before);\n    let mut after = 2;\n    consume(after);\n    crate::helper();\n}\nfn consume(_: usize) {}\nfn expression_only() { let x = opaque!(); helper(); }\n";
    std::fs::write(fixture.root.join("src/lib.rs"), source).unwrap();
    let client = Client::new(&fixture.root).await;
    for (line, spelling, outcome) in [
        (2, "input", "unavailable"),
        (4, "helper", "unavailable"),
        (5, "before", "unavailable"),
        (7, "after", "resolved"),
        (8, "helper", "resolved"),
        (11, "helper", "resolved"),
    ] {
        let column = source.lines().nth(line).unwrap().find(spelling).unwrap();
        let origin =
            json!({"kind":"position","path":"src/lib.rs","position":{"line":line,"column":column}});
        let reply = client.call("vvv_navigate", json!({"origin":origin})).await;
        assert_eq!(reply["result"]["outcome"], outcome, "{reply}");
        if outcome == "unavailable" {
            assert_eq!(reply["result"]["reason"], "unsupported_context", "{reply}");
        }
    }
    let column = source.lines().nth(8).unwrap().find("helper").unwrap();
    let origin =
        json!({"kind":"position","path":"src/lib.rs","position":{"line":8,"column":column}});
    let context = client
        .call("vvv_context", json!({"origin":origin,"detail":"signature"}))
        .await;
    assert_eq!(context["result"]["outcome"], "resolved", "{context}");
    let sites = client.call("vvv_relationships", json!({"origin":{"kind":"position","path":"src/lib.rs","position":{"line":0,"column":7}},"kind":"callers"})).await;
    let items = sites["result"]["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|item| item["start"]["line"] == 8 && item["outcome"] == "confirmed"),
        "{sites}"
    );
    assert!(
        items
            .iter()
            .any(|item| item["start"]["line"] == 4 && item["outcome"] == "unavailable"),
        "{sites}"
    );
    client.close().await;
}

#[cfg(feature = "rust")]
#[tokio::test]
async fn codex_client_navigates_local_imports_and_discovers_repeated_alias_callers() {
    let fixture = Fixture::new();
    std::fs::write(
        fixture.root.join("Cargo.toml"),
        "[package]\nname=\"local-probe\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )
    .unwrap();
    let source = "mod a { pub fn run() {} }\nmod b { pub fn run() {} }\nfn first() { work(); use crate::a::run as work; }\nfn second() { use crate::b::run as work; work(); }\nfn ambiguous() { use crate::a::run as work; use crate::b::run as work; work(); }\n";
    std::fs::write(fixture.root.join("src/lib.rs"), source).unwrap();
    let client = Client::new(&fixture.root).await;
    let origin = json!({"kind":"position","path":"src/lib.rs","position":{"line":2,"column":13}});
    let reply = client.call("vvv_navigate", json!({"origin":origin})).await;
    assert_eq!(reply["result"]["outcome"], "resolved", "{reply}");
    assert_eq!(reply["result"]["start"]["line"], 0);
    let context = client
        .call("vvv_context", json!({"origin":origin,"detail":"signature"}))
        .await;
    assert_eq!(context["result"]["outcome"], "resolved", "{context}");
    let sites = client
        .call(
            "vvv_relationships",
            json!({"origin":{"kind":"symbol","symbol":reply["result"]["target"]},"kind":"callers"}),
        )
        .await;
    let items = sites["result"]["items"].as_array().unwrap();
    assert!(
        items
            .iter()
            .any(|item| item["start"]["line"] == 2 && item["outcome"] == "confirmed"),
        "{sites}"
    );
    assert!(
        !items.iter().any(|item| item["start"]["line"] == 3),
        "{sites}"
    );
    let column = source.lines().nth(4).unwrap().rfind("work()").unwrap();
    let origin =
        json!({"kind":"position","path":"src/lib.rs","position":{"line":4,"column":column}});
    let ambiguous = client.call("vvv_navigate", json!({"origin":origin})).await;
    assert_eq!(ambiguous["result"]["outcome"], "ambiguous", "{ambiguous}");
    assert_eq!(
        ambiguous["result"]["candidates"].as_array().unwrap().len(),
        2
    );
    let selected = client
        .call(
            "vvv_navigate",
            json!({"origin":origin,"selection":{"ordinals":[2]}}),
        )
        .await;
    assert_eq!(selected["result"]["outcome"], "resolved", "{selected}");
    client.close().await;
}
