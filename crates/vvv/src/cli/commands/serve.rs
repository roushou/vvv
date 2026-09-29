use std::io::{BufRead, Write};

use clap::Args;
use serde_json::Value;
use vvv_engine::protocol::Response;
use vvv_engine::{Answer, Call, Engine, ErrorCode, Failure, Reply, Retention};

use crate::context::Context;

/// Answer requests from stdin, one JSON object per line, with one JSON
/// object per line: a session that keeps the tree between requests
#[derive(Debug, Args)]
pub struct ServeCmd;

impl ServeCmd {
    pub fn run(self, ctx: &Context) -> anyhow::Result<()> {
        let session = Session {
            engine: ctx.engine().clone().with_retention(Retention::session()),
        };
        let stdin = std::io::stdin().lock();
        let mut stdout = std::io::stdout().lock();
        for line in stdin.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let reply = session.answer(&line);
            serde_json::to_writer(&mut stdout, &reply)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
        Ok(())
    }
}

/// One line in, one line out.
struct Session {
    engine: Engine,
}

impl Session {
    fn answer(&self, line: &str) -> Reply<Answer> {
        // The id is echoed even when the rest of the line makes no sense.
        let id = serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|v| v.get("id").cloned());
        let response = match serde_json::from_str::<Call>(line) {
            Ok(call) => return call.execute(&self.engine),
            Err(error) => Response::error(Failure::new(
                ErrorCode::BadRequest,
                anyhow::Error::from(error).to_string(),
            )),
        };
        Reply { id, response }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vvv_engine::{Languages, MemoryVfs, Workspace};

    fn engine() -> Engine {
        let vfs = MemoryVfs::new().with_file("/ws/a.txt", "foo");
        Engine::new(
            Workspace::new("/ws", std::sync::Arc::new(vfs)),
            Languages::new(),
        )
        .with_retention(Retention::session())
    }

    #[test]
    fn a_bad_line_is_a_bad_request_with_its_id() {
        let reply = Session { engine: engine() }.answer(r#"{"id": 7, "command": "nope"}"#);
        let v = serde_json::to_value(&reply).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["status"], "error");
        assert_eq!(v["code"], "bad_request");
        let reply = Session { engine: engine() }.answer("not json");
        let v = serde_json::to_value(&reply).unwrap();
        assert!(v.get("id").is_none());
        assert_eq!(v["code"], "bad_request");
    }

    #[test]
    fn an_engine_error_carries_its_code_and_hint() {
        let reply = Session { engine: engine() }.answer(r#"{"id": "x", "command": "search"}"#);
        let v = serde_json::to_value(&reply).unwrap();
        assert_eq!(v["id"], "x");
        assert_eq!(v["code"], "bad_query");
        assert!(v["hint"].as_str().unwrap().contains("vvv search"));
        let reply = Session { engine: engine() }.answer(r#"{"command": "history"}"#);
        let v = serde_json::to_value(&reply).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["schema"], 1);
        assert_eq!(v["result"]["entries"], serde_json::json!([]));
    }
    #[test]
    fn session_mutations_keep_preview_and_applied_envelopes() {
        let engine = engine();
        let preview = Session {
            engine: engine.clone(),
        }
        .answer(r#"{"id": 7, "command": "batch", "intents": []}"#);
        assert_eq!(
            serde_json::to_value(preview).unwrap(),
            serde_json::json!({
                "id": 7, "status": "ok", "schema": 1,
                "result": {"intents": [], "applied": false, "files": []}
            })
        );
        let applied = Session { engine }
            .answer(r#"{"id": 8, "command": "batch", "intents": [], "apply": true}"#);
        assert_eq!(
            serde_json::to_value(applied).unwrap(),
            serde_json::json!({
                "id": 8, "status": "ok", "schema": 1,
                "result": {"intents": [], "applied": true, "history_id": 1, "files": []}
            })
        );
    }
}
