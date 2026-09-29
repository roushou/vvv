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
        info.protocol_version = ProtocolVersion::V_2025_11_25;
        let service = tokio::time::timeout(Duration::from_secs(15), info.serve(transport))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            service.peer_info().unwrap().protocol_version,
            ProtocolVersion::V_2025_11_25
        );
        let tools = service.list_all_tools().await.unwrap();
        assert_eq!(tools.len(), 7);
        assert!(
            tools
                .iter()
                .all(|t| t.annotations.as_ref().unwrap().read_only_hint == Some(true))
        );
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
        .unwrap()
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
                    if ["next_cursor", "expansion"].contains(&name.as_str()) && value.is_string() {
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
