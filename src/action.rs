use accesskit::{Action, ActionData, ActionRequest, NodeId};
use std::{collections::HashMap, fmt};

/// Routes supported actions to their application message handlers.
///
/// A route is identified by an [`ActionRoute`]. The map is rebuilt whenever the semantic tree is
/// published so it contains only routes advertised by the current tree.
///
/// Each entry contains:
///
/// - [`ActionRoute`]: the target node plus either a standard action or custom-action ID.
/// - [`ActionHandler<M>`]: the callback that produces the application's message type `M`.
pub(crate) type ActionHandlers<M> = HashMap<ActionRoute, ActionHandler<M>>;

/// Identifies an action handler for a semantic node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ActionRoute {
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

impl ActionRoute {
    /// Resolves the handler route described by an AccessKit request.
    pub(crate) fn from_request(request: &ActionRequest) -> Option<Self> {
        if request.action == Action::CustomAction {
            let Some(ActionData::CustomAction(id)) = request.data.as_ref() else {
                return None;
            };
            return Some(Self::Custom {
                node: request.target_node,
                id: *id,
            });
        }

        Some(Self::Standard {
            node: request.target_node,
            action: request.action,
        })
    }
}

/// Assigns stable numeric AccessKit IDs to application-defined custom-action keys.
#[derive(Debug, Default)]
pub(crate) struct CustomActionIds {
    /// Numeric IDs indexed by application-provided string keys.
    ids: HashMap<String, i32>,
    /// The next numeric ID available for allocation.
    next_id: i32,
}

impl CustomActionIds {
    /// Returns the stable numeric ID for `key`, allocating it when first seen.
    pub(crate) fn id_for(&mut self, key: impl Into<String>) -> i32 {
        let key = key.into();
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }

        let id = self.next_id;
        let Some(next_id) = self.next_id.checked_add(1) else {
            panic!("custom action ID space exhausted");
        };
        self.next_id = next_id;
        self.ids.insert(key, id);
        id
    }
}

/// Converts an AccessKit action request into an application-defined message of type `M`.
///
/// The callback is boxed to erase the concrete closure type, allowing different nodes and actions
/// to capture different values while being stored in the same [`ActionHandlers`] map. It receives
/// the complete request so handlers can use action-specific data such as a replacement value.
///
/// The callback parameters are:
///
/// - `&ActionRequest`: the requested action, target tree, target node, and optional action data.
/// - `M`: the application-defined message returned to its update loop.
pub(crate) struct ActionHandler<M> {
    callback: Box<dyn Fn(&ActionRequest) -> M>,
}

impl<M> ActionHandler<M> {
    /// Creates an action handler from an application callback.
    pub(crate) fn new(callback: impl Fn(&ActionRequest) -> M + 'static) -> Self {
        Self {
            callback: Box::new(callback),
        }
    }

    /// Converts an action request into an application message.
    pub(crate) fn call(&self, request: &ActionRequest) -> M {
        (self.callback)(request)
    }
}

impl<M> fmt::Debug for ActionHandler<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ActionHandler")
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
    fn publishes_and_routes_multiple_custom_actions() {
        #[derive(Debug, PartialEq, Eq)]
        enum Message {
            CopyFingerprint,
            Export,
        }

        let mut molerat = Molerat::<Message>::new("example");
        molerat.publish(|ui| {
            ui.list_item("key", "Alice's key")
                .custom_action("copy-fingerprint", "Copy fingerprint", |_| {
                    Message::CopyFingerprint
                })
                .custom_action("export", "Export public key", |_| Message::Export);
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
                .custom_action("export", "Export public key", |_| Message::Export)
                .custom_action("copy-fingerprint", "Copy fingerprint", |_| {
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
}
