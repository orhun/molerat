use accesskit::{Action, ActionRequest, NodeId};
use std::{collections::HashMap, fmt};

/// Routes supported actions to their application message handlers.
///
/// A route is identified by the stable [`NodeId`] receiving the request and the requested
/// [`Action`]. The map is rebuilt whenever the semantic tree is published so it contains only
/// routes advertised by the current tree.
///
/// Each entry contains:
///
/// - [`NodeId`]: the stable semantic node that receives the request.
/// - [`Action`]: the operation requested on that node, such as a click or value change.
/// - [`ActionHandler<M>`]: the callback that produces the application's message type `M`.
pub(crate) type ActionHandlers<M> = HashMap<(NodeId, Action), ActionHandler<M>>;

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
}
