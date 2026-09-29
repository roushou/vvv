//! Optional, read-only MCP adapter. The engine owns all source intelligence.
pub(crate) mod catalog;
mod queue;
mod transport;

use crate::output::mcp::{McpIssue, McpOutput};
use catalog::{ToolEntry, ToolKind};
use queue::WorkQueue;
use rmcp::{RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use std::sync::Arc;

pub(crate) struct McpSession {
    tools: Vec<ToolEntry>,
    queue: Arc<WorkQueue>,
}
impl McpSession {
    pub fn run(engine: vvv_engine::Engine) -> anyhow::Result<()> {
        let queue = WorkQueue::new();
        let tools = ToolKind::ALL
            .into_iter()
            .map(ToolEntry::new)
            .collect::<Result<_, _>>()?;
        let session = Self {
            tools,
            queue: queue.clone(),
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let work = queue.clone();
        let worker = std::thread::Builder::new()
            .name("vvv-mcp-engine".into())
            .spawn(move || work.run(engine))?;
        let result = runtime.block_on(async {
            let (read, write) = rmcp::transport::stdio();
            let transport = transport::BoundedTransport::new(read, write, queue.clone());
            let service = session.serve(transport).await?;
            service.waiting().await?;
            Ok(())
        });
        queue.close();
        let joined = worker.join();
        // Tokio's stdin reader is a blocking OS read; never wait indefinitely for
        // that read after rejecting a frame or closing a session.
        runtime.shutdown_background();
        if joined.is_err() {
            return Err(McpIssue::Worker.error().into());
        }
        result
    }
}
impl ServerHandler for McpSession {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.protocol_version = ProtocolVersion::V_2025_11_25;
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = Implementation::new("vvv", env!("CARGO_PKG_VERSION"));
        info
    }
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, rmcp::ErrorData> {
        if request.protocol_version != ProtocolVersion::V_2025_11_25 {
            return Err(McpIssue::ProtocolVersion.error());
        }
        context.peer.set_peer_info(request);
        Ok(self.get_info())
    }
    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, rmcp::ErrorData> {
        if request.is_some_and(|p| p.cursor.is_some()) {
            return Err(McpIssue::InvalidCursor.error());
        }
        Ok(ListToolsResult {
            tools: self.tools.iter().map(|entry| entry.tool.clone()).collect(),
            ..Default::default()
        })
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.tools
            .iter()
            .find(|entry| entry.tool.name == name)
            .map(|entry| entry.tool.clone())
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, rmcp::ErrorData> {
        let entry = self
            .tools
            .iter()
            .find(|entry| entry.tool.name == request.name)
            .ok_or_else(|| McpIssue::UnknownTool.error())?;
        let call = entry.decode(request.arguments).map_err(McpIssue::error)?;
        if context.ct.is_cancelled() {
            return Ok(McpOutput::cancelled().result());
        }
        let (pending, mut response) = self.queue.submit(call).map_err(McpIssue::error)?;
        let result = tokio::select! {
            biased;
            _ = context.ct.cancelled() => {
                if pending.cancel() {
                    McpOutput::cancelled().result()
                } else {
                    // Publication won the race; preserve the completed response.
                    McpOutput::new(response.await.map_err(|_| McpIssue::Closed.error())?).result()
                }
            },
            reply = &mut response => McpOutput::new(reply.map_err(|_| McpIssue::Closed.error())?).result(),
        };
        drop(pending);
        Ok(result)
    }
}
