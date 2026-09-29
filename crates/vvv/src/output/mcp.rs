//! MCP descriptions, typed protocol failures, and structured result presentation.
use crate::mcp::catalog::ToolKind;
use rmcp::model::{CallToolResult, Content, ErrorData};
use vvv_engine::{Answer, EngineError, Failure, Reply, protocol::Response};

#[derive(Debug, Clone, Copy)]
pub(crate) enum McpIssue {
    Parse,
    UnknownTool,
    InvalidArguments,
    InvalidCursor,
    Busy,
    Closed,
    ProtocolVersion,
    RequestId,
    DuplicateId,
    TransportLimit,
    Worker,
}
impl McpIssue {
    pub fn error(self) -> ErrorData {
        use rmcp::model::ErrorCode;
        let (code, message) = match self {
            Self::Parse => (ErrorCode::PARSE_ERROR, "Invalid JSON-RPC message"),
            Self::UnknownTool => (ErrorCode::INVALID_PARAMS, "Unknown tool; use tools/list"),
            Self::InvalidArguments => (
                ErrorCode::INVALID_PARAMS,
                "Arguments do not match the tool's input schema",
            ),
            Self::InvalidCursor => (
                ErrorCode::INVALID_PARAMS,
                "tools/list does not accept a continuation cursor",
            ),
            Self::Busy => (
                ErrorCode(-32000),
                "Session is busy; retry after an outstanding call finishes",
            ),
            Self::Closed => (ErrorCode(-32000), "Session closed"),
            Self::ProtocolVersion => (
                ErrorCode::INVALID_PARAMS,
                "This server supports MCP 2025-11-25",
            ),
            Self::RequestId => (
                ErrorCode::INVALID_REQUEST,
                "Request id exceeds 256 encoded bytes",
            ),
            Self::DuplicateId => (
                ErrorCode::INVALID_REQUEST,
                "Request id is already in flight",
            ),
            Self::TransportLimit => (
                ErrorCode::INVALID_REQUEST,
                "Message exceeds the transport limit",
            ),
            Self::Worker => (ErrorCode::INTERNAL_ERROR, "Engine worker stopped"),
        };
        ErrorData::new(code, message, None)
    }
}
impl ToolKind {
    pub fn description(self) -> &'static str {
        match self {
            Self::Discover => {
                "List the build's languages, engine commands, schemas, and limits. The MCP tool allowlist is tools/list."
            }
            Self::Search => {
                "Search source structure or symbol names. Use scope.paths for file/directory prefixes and scope.packages for owning package names or IDs. Returns bounded pages with stable IDs and a continuation cursor."
            }
            Self::Navigate => {
                "Resolve an exact source position, occurrence, or symbol to compact locations and evidence. Inspect resolved, ambiguous, or unavailable outcomes. Fetch source bodies with vvv_context."
            }
            Self::Context => {
                "Retrieve exact declaration excerpts and related declarations. Enclosing locations are metadata; set include_enclosing to request their bodies. Follow next_cursor even on empty progress pages; expand text with each item's expansion handle."
            }
            Self::Continue => {
                "Continue a retained search or context query in this session. Retryable; edits require restarting the original query."
            }
            Self::Expand => {
                "Read the next exact UTF-8 excerpt chunk. Append text until done; this does not advance relationship traversal."
            }
        }
    }
}
pub(crate) struct McpOutput {
    response: Response<Answer>,
}
impl McpOutput {
    pub fn new(reply: Reply<Answer>) -> Self {
        Self {
            response: reply.response,
        }
    }
    pub fn cancelled() -> Self {
        Self {
            response: Response::error(Failure::from(&EngineError::ReadCancelled)),
        }
    }
    pub fn result(self) -> CallToolResult {
        let is_error = matches!(self.response, Response::Error { .. });
        let mut structured =
            serde_json::to_value(self.response).expect("engine response serializes");
        let bytes = serde_json::to_vec(&structured)
            .expect("JSON serializes")
            .len();
        // Successful result budgets were applied before query publication. Error
        // messages can be larger; replace an oversized failure with typed limit data.
        if bytes > Self::MAX_ENVELOPE_BYTES {
            structured = serde_json::to_value(Response::<Answer>::error(Failure::from(
                &EngineError::OutputLimit {
                    max_bytes: Self::MAX_ENVELOPE_BYTES,
                    required_bytes: bytes,
                },
            )))
            .expect("failure serializes");
        }
        let mut result = CallToolResult::success(vec![Content::text(
            serde_json::to_string(&structured).expect("JSON serializes"),
        )]);
        result.structured_content = Some(structured);
        result.is_error = Some(is_error || bytes > Self::MAX_ENVELOPE_BYTES);
        result
    }
    // A result <= 1 MiB plus its envelope. Structured + JSON text serialization
    // is at most three times this, leaving ample room in the 8 MiB frame limit.
    pub const MAX_ENVELOPE_BYTES: usize = 1_048_576 + 1024;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaped_compatibility_text_fits_and_oversized_failures_are_replaced() {
        for size in [100_000, McpOutput::MAX_ENVELOPE_BYTES] {
            let mut failure = Failure::from(&EngineError::ReadCancelled);
            failure.message = "\"\\\n".repeat(size);
            let result = McpOutput {
                response: Response::error(failure),
            }
            .result();
            let structured = result.structured_content.as_ref().unwrap();
            assert_eq!(result.is_error, Some(true));
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    &result.content[0].as_text().unwrap().text
                )
                .unwrap(),
                *structured
            );
            assert!(serde_json::to_vec(&result).unwrap().len() < 8 * 1024 * 1024);
            assert_eq!(
                structured["code"],
                if size == 100_000 {
                    "cancelled"
                } else {
                    "output_limit"
                }
            );
        }
    }
}
