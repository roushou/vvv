//! A CLI error as the wire describes it: the engine's own code and hint when
//! the error is the engine's; the codes for what only the CLI can get wrong
//! — a query it could not build, a `--select` it could not read; `io` for
//! the rest.

use vvv_engine::{EngineError, ErrorCode, Failure, QueryError};

use crate::cli::select::BadSelect;

pub trait Diagnose {
    fn failure(&self) -> Failure;
}

impl Diagnose for anyhow::Error {
    fn failure(&self) -> Failure {
        if let Some(engine) = self.downcast_ref::<EngineError>() {
            return Failure::from(engine);
        }
        if let Some(query) = self.downcast_ref::<QueryError>() {
            return Failure::from(&EngineError::Query(match query {
                QueryError::Empty => QueryError::Empty,
            }));
        }
        let code = if self.downcast_ref::<BadSelect>().is_some() {
            ErrorCode::BadSelection
        } else if self.downcast_ref::<serde_json::Error>().is_some() {
            ErrorCode::BadRequest
        } else {
            ErrorCode::Io
        };
        Failure::new(code, format!("{self:#}"))
    }
}
