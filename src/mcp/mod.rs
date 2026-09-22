//! MCP exposure for Molerat semantic trees.

mod control;
mod server;
mod snapshot;

pub(crate) use control::Endpoint;
pub use server::Server;
