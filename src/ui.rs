use crate::node::Node;
use accesskit::{Node as AccessNode, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
use std::{collections::HashMap, marker::PhantomData};

/// The node ID reserved for the application root.
const ROOT_ID: NodeId = NodeId(0);

/// The application-owned Molerat integration handle.
#[derive(Debug)]
pub struct Molerat<M = ()> {
    /// The application name exposed to accessibility and MCP consumers.
    name: String,
    /// The most recent semantic-tree update published by the application.
    last_update: Option<TreeUpdate>,
    /// Stable AccessKit node IDs indexed by application-provided keys.
    node_ids: HashMap<String, NodeId>,
    /// The next AccessKit node ID available for allocation.
    next_node_id: u64,
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
            node_ids: HashMap::new(),
            next_node_id: ROOT_ID.0 + 1,
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

    /// Builds and publishes a complete semantic tree.
    pub fn publish(&mut self, build: impl FnOnce(&mut Ui<'_, M>)) {
        let mut ui = Ui::new(&self.name, &mut self.node_ids, &mut self.next_node_id);
        build(&mut ui);
        let update = ui.finish();
        self.update(update);
    }
}

/// Builds one complete semantic representation of an application.
#[derive(Debug)]
pub struct Ui<'a, M = ()> {
    /// The application name used to label the root node.
    application_name: &'a str,
    /// Stable AccessKit node IDs indexed by application-provided keys.
    node_ids: &'a mut HashMap<String, NodeId>,
    /// The next AccessKit node ID available for allocation.
    next_node_id: &'a mut u64,
    /// Nodes added during this publication.
    nodes: Vec<(NodeId, AccessNode)>,
    /// Direct children of the application root.
    root_children: Vec<NodeId>,
    /// Associates the builder with the application's message type.
    message: PhantomData<fn() -> M>,
}

impl<'a, M> Ui<'a, M> {
    /// Creates a semantic-tree builder using the handle's persistent ID state.
    fn new(
        application_name: &'a str,
        node_ids: &'a mut HashMap<String, NodeId>,
        next_node_id: &'a mut u64,
    ) -> Self {
        Self {
            application_name,
            node_ids,
            next_node_id,
            nodes: Vec::new(),
            root_children: Vec::new(),
            message: PhantomData,
        }
    }

    /// Adds a semantic node with the given AccessKit role.
    pub fn node(&mut self, key: impl Into<String>, role: Role) -> Node<'_> {
        let id = self.node_id(key);
        let node = AccessNode::new(role);

        self.root_children.push(id);
        self.nodes.push((id, node));
        let node_index = self.nodes.len() - 1;

        Node::new(&mut self.nodes[node_index].1)
    }

    /// Adds a paragraph containing `text`.
    pub fn paragraph(&mut self, key: impl Into<String>, text: impl Into<String>) -> Node<'_> {
        self.node(key, Role::Paragraph).value(text)
    }

    /// Adds a static text node.
    pub fn text(&mut self, key: impl Into<String>, text: impl Into<String>) -> Node<'_> {
        self.node(key, Role::Label).value(text)
    }

    /// Adds a button with an accessible label.
    pub fn button(&mut self, key: impl Into<String>, label: impl Into<String>) -> Node<'_> {
        self.node(key, Role::Button).label(label)
    }

    /// Adds a text input with a label and current value.
    pub fn text_input(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Node<'_> {
        self.node(key, Role::TextInput).label(label).value(value)
    }

    /// Adds a checkbox with a label and checked state.
    pub fn checkbox(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        checked: bool,
    ) -> Node<'_> {
        self.node(key, Role::CheckBox).label(label).toggled(checked)
    }

    /// Adds a progress indicator with a label and numeric range.
    pub fn progress(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        value: f64,
        minimum: f64,
        maximum: f64,
    ) -> Node<'_> {
        self.node(key, Role::ProgressIndicator)
            .label(label)
            .numeric_range(minimum, maximum)
            .numeric_value(value)
    }

    /// Returns the stable node ID associated with `key`, allocating it if necessary.
    fn node_id(&mut self, key: impl Into<String>) -> NodeId {
        let key = key.into();
        if let Some(id) = self.node_ids.get(&key) {
            return *id;
        }

        let id = NodeId(*self.next_node_id);
        *self.next_node_id += 1;
        self.node_ids.insert(key, id);
        id
    }

    /// Finishes the builder and returns the complete AccessKit tree update.
    fn finish(mut self) -> TreeUpdate {
        let mut root = AccessNode::new(Role::Application);
        root.set_label(self.application_name);
        root.set_children(self.root_children);
        self.nodes.insert(0, (ROOT_ID, root));

        TreeUpdate {
            nodes: self.nodes,
            tree: Some(TreeInfo::new(ROOT_ID)),
            tree_id: TreeId::ROOT,
            focus: ROOT_ID,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            nodes: vec![(root, AccessNode::new(Role::Application))],
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        };
        let mut molerat: Molerat = Molerat::new("example");

        molerat.update(update.clone());

        assert_eq!(molerat.last_update(), Some(&update));
    }

    #[test]
    fn preserves_node_ids_across_publications() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.paragraph("details", "First");
        });
        let first_id = molerat.last_update().map(|update| update.nodes[1].0);

        molerat.publish(|ui| {
            ui.paragraph("other", "Other");
            ui.paragraph("details", "Second");
        });
        let second_id = molerat.last_update().map(|update| update.nodes[2].0);

        assert_eq!(first_id, second_id);
    }
}
