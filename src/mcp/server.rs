use super::{control::Client, snapshot::Snapshot};
use rmcp::{
    Json, ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};

/// MCP server exposing the semantic state of a live Molerat application.
#[derive(Clone, Debug)]
pub struct Server {
    client: Client,
    tool_router: ToolRouter<Self>,
}

impl Server {
    /// Serves the MCP protocol over standard input and output.
    ///
    /// Users normally register the bundled `molerat-mcp` binary with their MCP client. Alternative
    /// launchers can call this method to host the same server.
    pub async fn serve_stdio() -> Result<(), Box<dyn std::error::Error>> {
        let service = Self::new().serve(rmcp::transport::stdio()).await?;
        service.waiting().await?;
        Ok(())
    }

    fn new() -> Self {
        Self::with_client(Client::new())
    }

    fn with_client(client: Client) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl Server {
    #[tool(
        name = "get_ui",
        description = "Get the current semantic state of the terminal user interface"
    )]
    async fn get_ui(&self) -> Result<Json<Snapshot>, String> {
        self.client
            .snapshot()
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("molerat", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Inspect the current semantic state of the live terminal user interface with \
                 get_ui.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Molerat, mcp::control::Endpoint};
    use std::{
        io,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_ENDPOINT: AtomicU64 = AtomicU64::new(1);

    #[tokio::test(flavor = "multi_thread")]
    async fn reads_the_current_semantic_tree() -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!(
            "molerat-test-{}-{}.json",
            std::process::id(),
            NEXT_ENDPOINT.fetch_add(1, Ordering::Relaxed)
        ));
        let endpoint = Endpoint::spawn_at("Agent tasks", path.clone())?;
        let mut molerat = Molerat::<()>::new("Agent tasks");
        molerat.publish(|ui| {
            ui.list_box("tasks", "Tasks", |ui| {
                ui.list_box_option("build", "Build project")
                    .selected(true)
                    .focused(true);
                ui.list_box_option("test", "Run tests");
            });
            ui.progress("progress", "Progress", 50.0, 0.0, 100.0);
        });
        let Some(update) = molerat.last_update() else {
            return Err("publication did not produce an update".into());
        };
        endpoint.update(update);

        let server = Server::with_client(Client::at_path(path));
        assert!(server.tool_router.map.contains_key("get_ui"));
        let snapshot = server.get_ui().await.map_err(io::Error::other)?.0;
        assert_eq!(snapshot.application, "Agent tasks");
        let Some(root) = snapshot.root else {
            return Err("published tree has no root".into());
        };
        assert_eq!(root.role, "Window");
        assert_eq!(root.children[0].label.as_deref(), Some("Tasks"));
        assert!(root.children[0].children[0].focused);
        assert_eq!(root.children[1].numeric_value, Some(50.0));
        Ok(())
    }
}
