//! Generic bridge connecting an MCP client to a running Molerat-enabled TUI:
//!
//! `MCP client <-- stdio --> molerat-mcp <-- local IPC --> TUI application`
//!
//! # App authors
//!
//! Enable Molerat's `mcp` feature and call [`molerat::Molerat::new_mcp`] in the application. The
//! application publishes its semantic state over local IPC; it does not launch or interact with
//! this binary. Poll [`molerat::Molerat::messages`] in the event loop to handle agent actions.
//!
//! # Users
//!
//! Install `molerat-mcp` and register it with the MCP client once. The client launches this binary
//! over stdio, and the binary connects to whichever Molerat-enabled TUI is currently running. The
//! same registration works for every Molerat-enabled application.

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    molerat::Server::serve_stdio().await
}
