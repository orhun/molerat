use accesskit::TreeUpdate;
use std::marker::PhantomData;

/// The application-owned Molerat integration handle.
#[derive(Debug)]
pub struct Molerat<M = ()> {
    /// The application name exposed to accessibility and MCP consumers.
    name: String,
    /// The most recent semantic-tree update published by the application.
    last_update: Option<TreeUpdate>,
    /// Records which message type this handle will emit.
    ///
    /// No message is stored yet. The `fn() -> M` marker means "produces `M`" without making the
    /// handle behave as if it owns an `M`.
    message: PhantomData<fn() -> M>,
}

impl<M> Molerat<M> {
    /// Creates an integration handle for an application using message type `M`.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            last_update: None,
            message: PhantomData,
        }
    }

    /// Returns the application name exposed to consumers.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Publishes an AccessKit tree update.
    pub fn update(&mut self, update: TreeUpdate) {
        self.last_update = Some(update);
    }

    /// Returns the most recently published AccessKit tree update.
    pub fn last_update(&self) -> Option<&TreeUpdate> {
        self.last_update.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::{Node, NodeId, Role, TreeId, TreeInfo};

    #[test]
    fn creates_interactive_instance() {
        enum Message {}

        let molerat = Molerat::<Message>::new("example");

        assert_eq!(molerat.name(), "example");
    }

    #[test]
    fn uses_unit_as_the_default_message_type() {
        let molerat: Molerat = Molerat::new("example");

        assert_eq!(molerat.name(), "example");
    }

    #[test]
    fn retains_the_latest_accesskit_update() {
        let root = NodeId(0);
        let update = TreeUpdate {
            nodes: vec![(root, Node::new(Role::Application))],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        };
        let mut molerat: Molerat = Molerat::new("example");

        molerat.update(update.clone());

        assert_eq!(molerat.last_update(), Some(&update));
    }
}
