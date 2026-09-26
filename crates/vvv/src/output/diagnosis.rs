//! A CLI error as the wire describes it: the engine's own code and hint when
//! the error is the engine's; the codes for what only the CLI can get wrong
//! — a query it could not build, a `--select` it could not read; `io` for
//! the rest.

use vvv_engine::{EngineError, ErrorCode, Failure, QueryError};

use crate::cli::select::BadSelect;

pub trait Diagnose {
    fn failure(&self) -> Failure;
}

impl Diagnose for EngineError {
    /// Preserve the CLI's error wording while the engine keeps interface-neutral errors.
    fn failure(&self) -> Failure {
        let mut failure = Failure::from(self);
        if matches!(self, Self::AmbiguousSymbol { .. }) {
            failure.message.push_str("; pick one with --in <file>");
        }
        failure
    }
}

impl Diagnose for anyhow::Error {
    fn failure(&self) -> Failure {
        if let Some(engine) = self.downcast_ref::<EngineError>() {
            return engine.failure();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambiguous_symbol_flag_advice_is_cli_only() {
        let error = EngineError::AmbiguousSymbol {
            name: "Config".to_owned(),
            declarations: vec![],
        };
        assert_eq!(
            error.to_string(),
            "`Config` is declared in several places ()"
        );
        assert_eq!(
            error.failure().message,
            "`Config` is declared in several places (); pick one with --in <file>"
        );
    }
}
