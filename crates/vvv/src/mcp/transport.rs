//! Bounded SDK codec transport. Framing and JSON-RPC parsing remain SDK-owned.
use super::queue::WorkQueue;
use crate::output::mcp::McpIssue;
use futures::{SinkExt, StreamExt};
use rmcp::{
    RoleServer,
    model::{ClientJsonRpcMessage, JsonRpcMessage, RequestId, ServerJsonRpcMessage},
    transport::{
        Transport,
        async_rw::{JsonRpcMessageCodec, JsonRpcMessageCodecError},
    },
};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{FramedRead, FramedWrite};

pub(crate) struct BoundedTransport<R: AsyncRead, W: AsyncWrite> {
    read: FramedRead<R, JsonRpcMessageCodec<ClientJsonRpcMessage>>,
    write: Arc<tokio::sync::Mutex<FramedWrite<W, JsonRpcMessageCodec<ServerJsonRpcMessage>>>>,
    inflight: Arc<Mutex<HashSet<RequestId>>>,
    queue: Arc<WorkQueue>,
}
impl<R: AsyncRead, W: AsyncWrite> BoundedTransport<R, W> {
    pub const MAX_INPUT: usize = 64 * 1024;
    pub const MAX_OUTPUT: usize = 8 * 1024 * 1024;
    pub const MAX_INFLIGHT: usize = 32;
    pub fn new(read: R, write: W, queue: Arc<WorkQueue>) -> Self {
        Self {
            read: FramedRead::new(
                read,
                JsonRpcMessageCodec::new_with_max_length(Self::MAX_INPUT),
            ),
            write: Arc::new(tokio::sync::Mutex::new(FramedWrite::new(
                write,
                JsonRpcMessageCodec::new(),
            ))),
            inflight: Arc::new(Mutex::new(HashSet::new())),
            queue,
        }
    }
}
impl<R, W> Transport<RoleServer> for BoundedTransport<R, W>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type Error = std::io::Error;
    fn send(
        &mut self,
        message: ServerJsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let write = self.write.clone();
        let inflight = self.inflight.clone();
        async move {
            // The tool budget reserves duplication/envelope space before execution.
            // This guard also covers SDK replies such as tools/list and protocol errors.
            if serde_json::to_vec(&message)?.len() > Self::MAX_OUTPUT {
                return Err(std::io::Error::other(McpIssue::TransportLimit.error()));
            }
            let id = match &message {
                JsonRpcMessage::Response(r) => Some(r.id.clone()),
                JsonRpcMessage::Error(e) => e.id.clone(),
                _ => None,
            };
            write
                .lock()
                .await
                .send(message)
                .await
                .map_err(std::io::Error::from)?;
            if let Some(id) = id {
                inflight
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&id);
            }
            Ok(())
        }
    }
    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        loop {
            let message = match self.read.next().await {
                Some(Ok(message)) => message,
                Some(Err(JsonRpcMessageCodecError::Serde(_))) => {
                    if self
                        .send(ServerJsonRpcMessage::error(McpIssue::Parse.error(), None))
                        .await
                        .is_err()
                    {
                        self.queue.close();
                        return None;
                    }
                    continue;
                }
                _ => {
                    self.queue.close();
                    return None;
                }
            };
            if let JsonRpcMessage::Request(request) = &message {
                let oversized_id = serde_json::to_vec(&request.id).ok()?.len() > 256;
                let issue = {
                    let mut inflight = self
                        .inflight
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if oversized_id {
                        Some(McpIssue::RequestId)
                    } else if inflight.contains(&request.id) {
                        Some(McpIssue::DuplicateId)
                    } else if inflight.len() >= Self::MAX_INFLIGHT {
                        Some(McpIssue::Busy)
                    } else {
                        inflight.insert(request.id.clone());
                        None
                    }
                };
                if let Some(issue) = issue {
                    // Duplicate/oversized IDs cannot identify a rejected call safely.
                    // Send directly: this is not completion of an admitted request.
                    let id = if matches!(issue, McpIssue::DuplicateId | McpIssue::RequestId) {
                        None
                    } else {
                        Some(request.id.clone())
                    };
                    if self
                        .write
                        .lock()
                        .await
                        .send(ServerJsonRpcMessage::error(issue.error(), id))
                        .await
                        .is_err()
                    {
                        self.queue.close();
                        return None;
                    }
                    continue;
                }
            }
            return Some(message);
        }
    }
    async fn close(&mut self) -> Result<(), Self::Error> {
        self.queue.close();
        self.write.lock().await.close().await.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    type Wire = BoundedTransport<Cursor<Vec<u8>>, Vec<u8>>;

    #[tokio::test]
    async fn bounds_requests_and_ids_without_consuming_the_original_id() {
        let mut input = String::new();
        for id in 0..Wire::MAX_INFLIGHT {
            input.push_str(&format!(
                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"ping\"}}\n"
            ));
        }
        input.push_str("{\"jsonrpc\":\"2.0\",\"id\":0,\"method\":\"ping\"}\n");
        input.push_str("{\"jsonrpc\":\"2.0\",\"id\":100,\"method\":\"ping\"}\n");
        input.push_str(&format!(
            "{{\"jsonrpc\":\"2.0\",\"id\":\"{}\",\"method\":\"ping\"}}\n",
            "x".repeat(256)
        ));
        let queue = WorkQueue::new();
        let mut wire = Wire::new(Cursor::new(input.into_bytes()), Vec::new(), queue.clone());
        for _ in 0..Wire::MAX_INFLIGHT {
            assert!(wire.receive().await.is_some());
        }
        assert!(wire.receive().await.is_none());
        assert_eq!(wire.inflight.lock().unwrap().len(), Wire::MAX_INFLIGHT);
        let output = wire.write.lock().await;
        let errors = output
            .get_ref()
            .split(|b| *b == b'\n')
            .filter(|s| !s.is_empty())
            .map(|s| serde_json::from_slice::<serde_json::Value>(s).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(errors.len(), 3);
        assert!(errors[0]["id"].is_null());
        assert_eq!(errors[1]["id"], 100);
        assert!(errors[2]["id"].is_null());
        let call = serde_json::from_str(r#"{"command":"discover"}"#).unwrap();
        assert!(matches!(queue.submit(call), Err(McpIssue::Closed)));
    }

    #[tokio::test]
    async fn oversized_frames_close_the_queue_and_malformed_json_is_a_protocol_error() {
        for input in [vec![b'x'; Wire::MAX_INPUT + 1], b"{bad json}\n".to_vec()] {
            let queue = WorkQueue::new();
            let mut wire = Wire::new(Cursor::new(input.clone()), Vec::new(), queue.clone());
            assert!(wire.receive().await.is_none());
            let output = wire.write.lock().await;
            if input.len() < Wire::MAX_INPUT {
                let error: serde_json::Value = serde_json::from_slice(output.get_ref()).unwrap();
                assert_eq!(error["error"]["code"], -32700);
            }
            let call = serde_json::from_str(r#"{"command":"discover"}"#).unwrap();
            assert!(matches!(queue.submit(call), Err(McpIssue::Closed)));
        }
    }

    #[tokio::test]
    async fn completion_releases_admission_and_oversized_output_is_never_written() {
        let mut wire = Wire::new(
            Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n".to_vec()),
            Vec::new(),
            WorkQueue::new(),
        );
        wire.receive().await.unwrap();
        wire.send(ServerJsonRpcMessage::error(
            McpIssue::Busy.error(),
            Some(RequestId::Number(1)),
        ))
        .await
        .unwrap();
        assert!(wire.inflight.lock().unwrap().is_empty());
        let before = wire.write.lock().await.get_ref().len();
        let huge = rmcp::ErrorData::new(
            rmcp::model::ErrorCode::INTERNAL_ERROR,
            "x".repeat(Wire::MAX_OUTPUT),
            None,
        );
        assert!(
            wire.send(ServerJsonRpcMessage::error(
                huge,
                Some(RequestId::Number(2))
            ))
            .await
            .is_err()
        );
        assert_eq!(wire.write.lock().await.get_ref().len(), before);
    }
}
