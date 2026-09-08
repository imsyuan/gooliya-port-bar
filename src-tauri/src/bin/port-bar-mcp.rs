//! Standalone MCP server for Gooliya Port Bar.
//!
//! Runs over stdio transport (no network socket) and reuses `hq_app_lib`'s
//! `scan_ports()` so it reports exactly the same ports/uptime/idle status as
//! the GUI popover. Read-only by design: it does not link against, and has
//! no access to, the private `kill_port_impl` in `hq_app_lib` — closing a
//! service is only ever done through the GUI's own confirmation flow.

use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{
        CallToolResult, ContentBlock, Implementation, ProtocolVersion, ServerCapabilities,
        ServerInfo,
    },
    tool, tool_handler, tool_router,
    transport::stdio,
};

#[derive(Clone)]
struct PortBarMcp {
    // Read by the `#[tool_handler]`-generated trait impl, not directly by
    // this file — rustc's dead-code analysis doesn't see through that macro
    // expansion. The upstream rmcp examples hit the same false positive.
    #[allow(dead_code)]
    tool_router: ToolRouter<PortBarMcp>,
}

#[tool_router]
impl PortBarMcp {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List all currently detected listening ports (npm/node dev servers and Docker containers), including uptime in seconds and whether each is idle."
    )]
    fn list_ports(&self) -> Result<CallToolResult, McpError> {
        let entries =
            hq_app_lib::scan_ports_checked().map_err(|e| McpError::internal_error(e, None))?;
        let json = serde_json::to_string(&entries)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }

    #[tool(
        description = "List only the currently detected ports considered idle (running past the idle threshold) — a hint they may have been forgotten. This tool cannot close or restart anything; use the Gooliya Port Bar GUI for that."
    )]
    fn list_idle_ports(&self) -> Result<CallToolResult, McpError> {
        let entries: Vec<_> = hq_app_lib::scan_ports_checked()
            .map_err(|e| McpError::internal_error(e, None))?
            .into_iter()
            .filter(|entry| entry.is_idle)
            .collect();
        let json = serde_json::to_string(&entries)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![ContentBlock::text(json)]))
    }
}

#[tool_handler]
impl ServerHandler for PortBarMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("port-bar-mcp", env!("CARGO_PKG_VERSION")))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Read-only MCP server for Gooliya Port Bar. Tools: list_ports (all detected \
                 ports with uptime/idle status), list_idle_ports (only ports idle past the \
                 threshold). This server exposes no tool that can terminate a process or stop \
                 a container — closing a service must be done through the Gooliya Port Bar GUI."
                    .to_string(),
            )
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if !cfg!(target_os = "macos") {
        anyhow::bail!(
            "port-bar-mcp only supports macOS (it shells out to lsof/ps/docker the same way the Gooliya Port Bar GUI does)."
        );
    }

    let service = PortBarMcp::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
