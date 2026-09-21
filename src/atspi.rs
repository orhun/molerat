use accesskit::{ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_unix::Adapter;
use std::sync::{
    Arc, Mutex, PoisonError,
    mpsc::{self, Receiver, Sender},
};

/// Connects a Molerat tree to the Unix AT-SPI accessibility bus.
///
/// This is a small wrapper around the upstream [`accesskit_unix::Adapter`], which bridges an
/// AccessKit semantic tree to the native Unix accessibility stack. See the
/// [`accesskit_unix` crate documentation] for details about activation, tree updates, and action
/// handling.
///
/// [`accesskit_unix::Adapter`]: https://docs.rs/accesskit_unix/latest/accesskit_unix/struct.Adapter.html
/// [`accesskit_unix` crate documentation]: https://docs.rs/accesskit_unix/latest/accesskit_unix/
#[derive(Debug)]
pub(crate) struct Atspi {
    /// The AccessKit Unix platform adapter.
    adapter: Adapter,
    /// Action requests waiting to be handled on the application thread.
    action_requests: Receiver<ActionRequest>,
    /// The latest full tree available to an activating accessibility client.
    tree: Arc<Mutex<Option<TreeUpdate>>>,
}

impl Atspi {
    /// Creates an adapter with the latest tree, if one has already been published.
    pub(crate) fn new(tree: Option<TreeUpdate>) -> Self {
        let tree = Arc::new(Mutex::new(tree));
        let (action_sender, action_requests) = mpsc::channel();
        let mut adapter = Adapter::new(
            Activation {
                tree: Arc::clone(&tree),
            },
            Actions {
                sender: action_sender,
            },
            Deactivation,
        );
        adapter.update_window_focus_state(true);
        Self {
            adapter,
            action_requests,
            tree,
        }
    }

    /// Stores and forwards a newly published full semantic tree.
    pub(crate) fn update(&mut self, update: &TreeUpdate) {
        *self.tree.lock().unwrap_or_else(PoisonError::into_inner) = Some(update.clone());
        self.adapter.update_if_active(|| update.clone());
    }

    /// Updates whether the application window is focused.
    pub(crate) fn set_window_focused(&mut self, focused: bool) {
        self.adapter.update_window_focus_state(focused);
    }

    /// Drains action requests received on the adapter thread.
    pub(crate) fn drain_actions(&self) -> Vec<ActionRequest> {
        self.action_requests.try_iter().collect()
    }
}

/// Supplies the current full tree when an accessibility client activates the adapter.
struct Activation {
    /// The latest full tree published by the application.
    tree: Arc<Mutex<Option<TreeUpdate>>>,
}

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.tree
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// Forwards platform action requests to the application thread.
struct Actions {
    /// Channel used by AccessKit's adapter thread.
    sender: Sender<ActionRequest>,
}

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        let _ = self.sender.send(request);
    }
}

/// Keeps the latest tree available so accessibility can be reactivated without rebuilding it.
struct Deactivation;

impl DeactivationHandler for Deactivation {
    fn deactivate_accessibility(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo};

    #[test]
    fn activation_returns_the_latest_tree() {
        let root = NodeId(0);
        let update = TreeUpdate {
            nodes: vec![(root, Node::new(Role::Application))],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        };
        let mut activation = Activation {
            tree: Arc::new(Mutex::new(Some(update.clone()))),
        };

        assert_eq!(activation.request_initial_tree(), Some(update));
    }

    #[test]
    fn actions_are_forwarded_to_the_application_thread() {
        let (sender, receiver) = mpsc::channel();
        let mut actions = Actions { sender };
        let request = ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: NodeId(1),
            data: None,
        };

        actions.do_action(request.clone());

        assert_eq!(receiver.try_recv(), Ok(request));
    }
}
