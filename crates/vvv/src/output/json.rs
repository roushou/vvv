//! Machine output: one JSON document per invocation, errors included, so a
//! client never needs stderr.

use std::io::{self, Stdout, Write};

use serde::Serialize;
use vvv_engine::protocol::{Answer, Response};

use crate::output::{Diagnose, Reporter};

pub struct JsonReporter<W: Write = Stdout> {
    out: W,
}

impl JsonReporter {
    pub fn stdio() -> Self {
        Self::new(io::stdout())
    }
}

impl<W: Write> JsonReporter<W> {
    pub fn new(out: W) -> Self {
        Self { out }
    }

    #[cfg(test)]
    pub fn into_inner(self) -> W {
        self.out
    }

    fn emit<T: Serialize>(&mut self, response: &Response<T>) -> anyhow::Result<()> {
        serde_json::to_writer_pretty(&mut self.out, response)?;
        writeln!(self.out)?;
        Ok(())
    }
}

impl<W: Write> Reporter for JsonReporter<W> {
    fn report(&mut self, answer: &Answer) -> anyhow::Result<()> {
        self.emit(&Response::ok(answer))
    }

    fn error(&mut self, error: &anyhow::Error) {
        let response: Response<()> = Response::error(error.failure());
        let _ = self.emit(&response);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::fixtures as fx;

    #[test]
    fn search_is_an_ok_envelope() {
        let mut r = JsonReporter::new(Vec::new());
        r.report(&Answer::Search(fx::search())).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.into_inner()).unwrap();
        assert_eq!(v["status"], "ok");
        assert_eq!(v["result"]["matches"].as_array().unwrap().len(), 4);
        assert_eq!(
            v["result"]["matches"][0]["line"],
            "pub trait Language: Send + Sync {"
        );
        assert_eq!(v["result"]["matches"][0]["role"], "declaration");
        assert_eq!(v["result"]["matches"][1]["role"], "import");
    }

    #[test]
    fn understanding_answers_flatten_their_declarations() {
        let mut r = JsonReporter::new(Vec::new());
        r.report(&Answer::Surface(fx::surface())).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.into_inner()).unwrap();
        let first = &v["result"]["items"][0];
        assert_eq!(first["name"], "Plan");
        assert_eq!(first["address"]["path"][1], "Plan");
        assert_eq!(first["via"][0]["path"][0], "Plan");
        assert_eq!(first["importers"], 4);
        assert!(
            v["result"]["items"][1].get("via").is_none(),
            "empty via is omitted"
        );

        let mut r = JsonReporter::new(Vec::new());
        r.report(&Answer::Imports(fx::imports())).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.into_inner()).unwrap();
        let unresolved = &v["result"]["unresolved"][0];
        assert_eq!(unresolved["path"], "missing::Thing");
        assert!(unresolved.get("address").is_none());
        assert_eq!(v["result"]["unplaced"][0], "tests/plan.rs");
    }

    #[test]
    fn answers_carry_the_schema_and_flatten_symbols() {
        let mut r = JsonReporter::new(Vec::new());
        r.report(&Answer::Outline(fx::outline())).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.into_inner()).unwrap();
        assert_eq!(v["schema"], 1);
        let first = &v["result"]["items"][0];
        assert_eq!(
            first["name"], "ApplyError",
            "symbol fields are flattened into the item"
        );
        assert_eq!(first["visibility"]["text"], "pub");
        assert_eq!(first["reach"], "everyone");
        assert_eq!(v["result"]["items"][4]["reach"]["package"], "vvv_core");
    }

    #[test]
    fn errors_are_an_error_envelope() {
        let mut r = JsonReporter::new(Vec::new());
        r.error(&anyhow::anyhow!("boom"));
        let v: serde_json::Value = serde_json::from_slice(&r.into_inner()).unwrap();
        assert_eq!(v["status"], "error");
        assert_eq!(v["message"], "boom");
    }
}
