use super::{control::Client, snapshot::Snapshot};
use accesskit::{Action, ActionData, ActionRequest, NodeId, TreeId};
use rmcp::{
    Json, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

/// Parameters identifying an advertised semantic action.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct PerformActionParams {
    /// Node ID returned by `get_ui`.
    node_id: u64,
    /// Standard action returned in `actions`, or `customAction` for a custom action.
    action: Action,
    /// Custom action ID returned in `customActions`.
    #[serde(default)]
    custom_action_id: Option<i32>,
    /// Optional AccessKit payload required by actions such as `setValue`.
    #[serde(default)]
    data: Option<ActionData>,
}

impl PerformActionParams {
    /// Converts tool parameters into an AccessKit request for the application root tree.
    fn into_request(self) -> Result<ActionRequest, String> {
        let data = match (self.action, self.custom_action_id, self.data) {
            (Action::CustomAction, Some(id), None) => Some(ActionData::CustomAction(id)),
            (Action::CustomAction, None, _) => {
                return Err("customActionId is required for a custom action".to_owned());
            }
            (Action::CustomAction, Some(_), Some(_)) => {
                return Err("data cannot be combined with customActionId".to_owned());
            }
            (_, Some(_), _) => {
                return Err("customActionId is only valid for a custom action".to_owned());
            }
            (_, None, data) => data,
        };

        Ok(ActionRequest {
            action: self.action,
            target_tree: TreeId::ROOT,
            target_node: NodeId(self.node_id),
            data,
        })
    }
}

/// MCP server exposing the semantic state of a live Molerat application.
#[derive(Clone, Debug)]
pub struct Server {
    /// Client used to exchange snapshots and action requests with the running application.
    client: Client,
    /// Router generated from the MCP tool methods implemented by this server.
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

    /// Creates a server connected through the default Molerat discovery path.
    fn new() -> Self {
        Self::with_client(Client::new())
    }

    /// Creates a server backed by a specific application-side IPC client.
    fn with_client(client: Client) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl Server {
    /// Returns the latest semantic UI tree published by the running application.
    ///
    /// The snapshot contains stable node IDs, current state, and the standard or custom actions
    /// that can be passed to [`Server::perform_action`].
    #[tool(
        name = "get_ui",
        description = "Get the current semantic state and available actions of the terminal user interface"
    )]
    async fn get_ui(&self) -> Result<Json<Snapshot>, String> {
        self.client
            .snapshot()
            .await
            .map(Json)
            .map_err(|error| error.to_string())
    }

    /// Dispatches an action advertised by [`Server::get_ui`] to the running application.
    ///
    /// The request is validated against the latest semantic tree before being queued for handling
    /// on the application's TUI thread.
    #[tool(
        name = "perform_action",
        description = "Perform an action advertised by get_ui on a semantic node"
    )]
    async fn perform_action(
        &self,
        Parameters(params): Parameters<PerformActionParams>,
    ) -> Result<String, String> {
        let request = params.into_request()?;
        self.client
            .perform_action(request)
            .await
            .map_err(|error| error.to_string())?;
        Ok("Action dispatched to the application".to_owned())
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("molerat", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Inspect the current semantic state and available actions of the live terminal \
                 user interface with get_ui, then invoke advertised actions with perform_action.",
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

    #[tokio::test(flavor = "multi_thread")]
    async fn dispatches_standard_and_custom_actions() -> Result<(), Box<dyn std::error::Error>> {
        #[derive(Debug, PartialEq, Eq)]
        enum Message {
            Select,
            Refresh,
        }

        let path = std::env::temp_dir().join(format!(
            "molerat-test-{}-{}.json",
            std::process::id(),
            NEXT_ENDPOINT.fetch_add(1, Ordering::Relaxed)
        ));
        let endpoint = Endpoint::spawn_at("Agent tasks", path.clone())?;
        let mut molerat = Molerat::new("Agent tasks");
        molerat.publish(|ui| {
            ui.button("select", "Select").on_click(|| Message::Select);
            ui.command("refresh", "Refresh application", || Message::Refresh);
        });
        let Some(update) = molerat.last_update() else {
            return Err("publication did not produce an update".into());
        };
        let button_id = update.nodes[1].0.0;
        let Some(custom_action) = update.nodes[0].1.custom_actions().first() else {
            return Err("root command was not published".into());
        };
        let custom_action_id = custom_action.id;
        endpoint.update(update);

        let server = Server::with_client(Client::at_path(path));
        assert!(server.tool_router.map.contains_key("perform_action"));
        server
            .perform_action(Parameters(PerformActionParams {
                node_id: button_id,
                action: Action::Click,
                custom_action_id: None,
                data: None,
            }))
            .await
            .map_err(io::Error::other)?;
        server
            .perform_action(Parameters(PerformActionParams {
                node_id: 0,
                action: Action::CustomAction,
                custom_action_id: Some(custom_action_id),
                data: None,
            }))
            .await
            .map_err(io::Error::other)?;

        for request in endpoint.drain_actions() {
            assert!(molerat.handle_action(request));
        }
        assert_eq!(
            molerat.messages().collect::<Vec<_>>(),
            [Message::Select, Message::Refresh]
        );
        Ok(())
    }
}
