use accesskit::{Action, ActionData, ActionRequest, NodeId};
use std::{collections::HashMap, fmt};

/// Converts an AccessKit action request into an application-defined message of type `M`.
type ActionHandler<M> = Box<dyn Fn(&ActionRequest) -> M>;

/// Identifies an action handler for a semantic node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum ActionRoute {
    /// A standard AccessKit action supported by a node.
    Standard {
        /// The stable semantic node receiving the action.
        node: NodeId,
        /// The operation requested on the node.
        action: Action,
    },
    /// An application-defined action advertised by a node.
    Custom {
        /// The stable semantic node receiving the action.
        node: NodeId,
        /// The numeric custom-action ID exposed through AccessKit.
        id: i32,
    },
}

/// Registers supported actions and routes their requests to application messages.
pub(crate) struct Actions<M> {
    /// Message handlers indexed by target node and action.
    handlers: HashMap<ActionRoute, ActionHandler<M>>,
    /// Stable numeric AccessKit IDs indexed by application-defined string keys.
    custom_ids: HashMap<String, i32>,
}

impl<M> Actions<M> {
    /// Removes handlers advertised by the previous semantic tree.
    pub(crate) fn clear(&mut self) {
        self.handlers.clear();
    }

    /// Registers a standard action for a semantic node.
    pub(crate) fn register(
        &mut self,
        node: NodeId,
        action: Action,
        handler: impl Fn(&ActionRequest) -> M + 'static,
    ) {
        assert!(
            action != Action::CustomAction,
            "use Node::custom_action to register custom actions"
        );
        self.handlers
            .insert(ActionRoute::Standard { node, action }, Box::new(handler));
    }

    /// Registers a custom action and returns its stable numeric AccessKit ID.
    pub(crate) fn register_custom(
        &mut self,
        node: NodeId,
        key: impl Into<String>,
        handler: impl Fn() -> M + 'static,
    ) -> i32 {
        let key = key.into();
        let id = self.custom_id(key.clone());
        let route = ActionRoute::Custom { node, id };
        assert!(
            !self.handlers.contains_key(&route),
            "custom action key {key:?} registered more than once on node {node:?}"
        );
        self.handlers.insert(route, Box::new(move |_| handler()));
        id
    }

    /// Converts a supported action request into an application message.
    pub(crate) fn handle(&self, request: &ActionRequest) -> Option<M> {
        let route = if request.action == Action::CustomAction {
            let Some(ActionData::CustomAction(id)) = request.data.as_ref() else {
                return None;
            };
            ActionRoute::Custom {
                node: request.target_node,
                id: *id,
            }
        } else {
            ActionRoute::Standard {
                node: request.target_node,
                action: request.action,
            }
        };
        self.handlers.get(&route).map(|handler| handler(request))
    }

    /// Returns the numeric ID for a custom-action key, allocating it when first seen.
    fn custom_id(&mut self, key: String) -> i32 {
        if let Some(id) = self.custom_ids.get(&key) {
            return *id;
        }

        let Ok(id) = i32::try_from(self.custom_ids.len()) else {
            panic!("custom action ID space exhausted");
        };
        self.custom_ids.insert(key, id);
        id
    }
}

impl<M> Default for Actions<M> {
    fn default() -> Self {
        Self {
            handlers: HashMap::new(),
            custom_ids: HashMap::new(),
        }
    }
}

impl<M> fmt::Debug for Actions<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Actions")
            .field("handler_count", &self.handlers.len())
            .field("custom_ids", &self.custom_ids)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Molerat;
    use accesskit::TreeId;

    #[test]
    fn routes_actions_to_application_messages() {
        #[derive(Debug, PartialEq, Eq)]
        enum Message {
            Save,
        }

        let mut molerat = Molerat::<Message>::new("example");
        molerat.publish(|ui| {
            ui.button("save", "Save")
                .on_action(Action::Click, |_| Message::Save);
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        let (button_id, button) = &update.nodes[1];
        assert!(button.supports_action(Action::Click));
        let request = ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: *button_id,
            data: None,
        };

        assert!(molerat.handle_action(request));
        assert_eq!(molerat.messages().collect::<Vec<_>>(), [Message::Save]);
        assert_eq!(molerat.messages().next(), None);
    }

    #[test]
    fn routes_common_actions_to_application_messages() {
        #[derive(Debug, PartialEq, Eq)]
        enum Message {
            Click,
            Focus,
        }

        let mut molerat = Molerat::<Message>::new("example");
        molerat.publish(|ui| {
            ui.button("select", "Select")
                .on_click(|| Message::Click)
                .on_focus(|| Message::Focus);
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        let (button_id, button) = &update.nodes[1];
        assert!(button.supports_action(Action::Click));
        assert!(button.supports_action(Action::Focus));
        let button_id = *button_id;

        for (action, expected) in [
            (Action::Click, Message::Click),
            (Action::Focus, Message::Focus),
        ] {
            assert!(molerat.handle_action(ActionRequest {
                action,
                target_tree: TreeId::ROOT,
                target_node: button_id,
                data: None,
            }));
            assert_eq!(molerat.messages().collect::<Vec<_>>(), [expected]);
        }
    }

    #[test]
    fn publishes_and_routes_application_commands() {
        #[derive(Debug, PartialEq, Eq)]
        enum Message {
            Refresh,
        }

        let mut molerat = Molerat::<Message>::new("example");
        molerat.publish(|ui| {
            ui.command("refresh", "Refresh application", || Message::Refresh);
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        let (root_id, root) = &update.nodes[0];
        let [refresh] = root.custom_actions() else {
            panic!("the application root should advertise the refresh command");
        };
        assert_eq!(refresh.description, "Refresh application");
        let request = ActionRequest {
            action: Action::CustomAction,
            target_tree: TreeId::ROOT,
            target_node: *root_id,
            data: Some(ActionData::CustomAction(refresh.id)),
        };

        assert!(molerat.handle_action(request));
        assert_eq!(molerat.messages().collect::<Vec<_>>(), [Message::Refresh]);
    }

    #[test]
    fn publishes_and_routes_multiple_custom_actions() {
        #[derive(Debug, PartialEq, Eq)]
        enum Message {
            CopyFingerprint,
            Export,
        }

        let mut molerat = Molerat::<Message>::new("example");
        molerat.publish(|ui| {
            ui.list_item("key", "Alice's key")
                .custom_action("copy-fingerprint", "Copy fingerprint", || {
                    Message::CopyFingerprint
                })
                .custom_action("export", "Export public key", || Message::Export);
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        let (node_id, node) = &update.nodes[1];
        let [copy, export] = node.custom_actions() else {
            panic!("the node should advertise both custom actions");
        };
        let copy_id = copy.id;
        let export_id = export.id;
        let requests = [
            (copy_id, Message::CopyFingerprint),
            (export_id, Message::Export),
        ];
        let node_id = *node_id;

        for (id, expected) in requests {
            assert!(molerat.handle_action(ActionRequest {
                action: Action::CustomAction,
                target_tree: TreeId::ROOT,
                target_node: node_id,
                data: Some(ActionData::CustomAction(id)),
            }));
            assert_eq!(molerat.messages().collect::<Vec<_>>(), [expected]);
        }

        molerat.publish(|ui| {
            ui.list_item("key", "Alice's key")
                .custom_action("export", "Export public key", || Message::Export)
                .custom_action("copy-fingerprint", "Copy fingerprint", || {
                    Message::CopyFingerprint
                });
        });
        let Some(update) = molerat.last_update() else {
            panic!("an updated tree should have been published");
        };
        let [export, copy] = update.nodes[1].1.custom_actions() else {
            panic!("the node should still advertise both custom actions");
        };
        assert_eq!(copy.id, copy_id);
        assert_eq!(export.id, export_id);
    }

    #[test]
    #[should_panic(expected = "registered more than once")]
    fn rejects_duplicate_custom_action_keys_on_a_node() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.button("save", "Save")
                .custom_action("copy", "Copy one", || ())
                .custom_action("copy", "Copy two", || ());
        });
    }
}
