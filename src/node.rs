use crate::action::{ActionHandler, ActionHandlers, ActionRoute, CustomActionIds};
use accesskit::{
    Action, ActionRequest, CustomAction as AccessCustomAction, Node as AccessNode, NodeId, Toggled,
};

/// Configures a node added to a semantic tree.
#[derive(Debug)]
pub struct Node<'a, M = ()> {
    /// The stable ID of the AccessKit node being configured.
    id: NodeId,
    /// The AccessKit node being configured.
    inner: &'a mut AccessNode,
    /// The node selected as semantic focus in the current publication.
    focus: &'a mut Option<NodeId>,
    /// Application message handlers for actions supported by the node.
    action_handlers: &'a mut ActionHandlers<M>,
    /// Stable numeric IDs for application-defined custom actions.
    custom_action_ids: &'a mut CustomActionIds,
}

impl<'a, M> Node<'a, M> {
    /// Creates a semantic node builder for an AccessKit node.
    pub(crate) fn new(
        id: NodeId,
        inner: &'a mut AccessNode,
        focus: &'a mut Option<NodeId>,
        action_handlers: &'a mut ActionHandlers<M>,
        custom_action_ids: &'a mut CustomActionIds,
    ) -> Self {
        Self {
            id,
            inner,
            focus,
            action_handlers,
            custom_action_ids,
        }
    }

    /// Sets the node's accessible label.
    pub fn label(self, label: impl Into<String>) -> Self {
        self.inner.set_label(label);
        self
    }

    /// Sets the node's value.
    pub fn value(self, value: impl Into<String>) -> Self {
        self.inner.set_value(value);
        self
    }

    /// Sets the node's accessible description.
    pub fn description(self, description: impl Into<String>) -> Self {
        self.inner.set_description(description);
        self
    }

    /// Sets whether the node is toggled.
    pub fn toggled(self, toggled: bool) -> Self {
        self.inner.set_toggled(Toggled::from(toggled));
        self
    }

    /// Sets whether the node is selected.
    pub fn selected(self, selected: bool) -> Self {
        self.inner.set_selected(selected);
        self
    }

    /// Sets whether the node is disabled.
    pub fn disabled(self, disabled: bool) -> Self {
        if disabled {
            self.inner.set_disabled();
        } else {
            self.inner.clear_disabled();
        }
        self
    }

    /// Sets whether the node is expanded.
    pub fn expanded(self, expanded: bool) -> Self {
        self.inner.set_expanded(expanded);
        self
    }

    /// Sets whether the node and its descendants are hidden.
    pub fn hidden(self, hidden: bool) -> Self {
        if hidden {
            self.inner.set_hidden();
        } else {
            self.inner.clear_hidden();
        }
        self
    }

    /// Sets whether the node has semantic focus.
    pub fn focused(self, focused: bool) -> Self {
        if focused {
            assert!(
                self.focus.is_none() || *self.focus == Some(self.id),
                "only one semantic node can be focused"
            );
            *self.focus = Some(self.id);
        } else if *self.focus == Some(self.id) {
            *self.focus = None;
        }
        self
    }

    /// Sets the node's current numeric value.
    pub fn numeric_value(self, value: f64) -> Self {
        self.inner.set_numeric_value(value);
        self
    }

    /// Sets the minimum and maximum numeric values for the node.
    pub fn numeric_range(self, minimum: f64, maximum: f64) -> Self {
        self.inner.set_min_numeric_value(minimum);
        self.inner.set_max_numeric_value(maximum);
        self
    }

    /// Registers an action supported by this node and maps it to an application message.
    pub fn on_action(
        self,
        action: Action,
        handler: impl Fn(&ActionRequest) -> M + 'static,
    ) -> Self {
        assert!(
            action != Action::CustomAction,
            "use Node::custom_action to register custom actions"
        );
        self.inner.add_action(action);
        self.action_handlers.insert(
            ActionRoute::Standard {
                node: self.id,
                action,
            },
            ActionHandler::new(handler),
        );
        self
    }

    /// Registers an application-defined action supported by this node.
    ///
    /// `key` preserves the action's numeric AccessKit ID across publications, `description` is
    /// exposed to consumers, and `handler` maps requests for the action to application messages.
    pub fn custom_action(
        self,
        key: impl Into<String>,
        description: impl Into<String>,
        handler: impl Fn(&ActionRequest) -> M + 'static,
    ) -> Self {
        let id = self.custom_action_ids.id_for(key);
        self.inner.push_custom_action(AccessCustomAction {
            id,
            description: description.into(),
        });
        self.action_handlers.insert(
            ActionRoute::Custom { node: self.id, id },
            ActionHandler::new(handler),
        );
        self
    }

    /// Applies raw AccessKit configuration to the node.
    pub fn with_accesskit(self, configure: impl FnOnce(&mut AccessNode)) -> Self {
        configure(self.inner);
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::Molerat;
    use accesskit::{NodeId, Role, Toggled, TreeId, TreeInfo};

    const ROOT_ID: NodeId = NodeId(0);

    #[test]
    fn publishes_a_generic_node() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.node("details", Role::Paragraph)
                .label("Details")
                .value("Three tasks are overdue")
                .description("Task summary");
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        assert_eq!(update.tree, Some(TreeInfo::new(ROOT_ID)));
        assert_eq!(update.tree_id, TreeId::ROOT);
        assert_eq!(update.focus, ROOT_ID);
        assert_eq!(update.nodes.len(), 2);

        let (root_id, root) = &update.nodes[0];
        let (paragraph_id, paragraph) = &update.nodes[1];
        assert_eq!(*root_id, ROOT_ID);
        assert_eq!(root.role(), Role::Application);
        assert_eq!(root.label(), Some("example"));
        assert_eq!(root.children(), [*paragraph_id]);
        assert_eq!(paragraph.role(), Role::Paragraph);
        assert_eq!(paragraph.label(), Some("Details"));
        assert_eq!(paragraph.value(), Some("Three tasks are overdue"));
        assert_eq!(paragraph.description(), Some("Task summary"));
    }

    #[test]
    fn publishes_leaf_components() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.paragraph("details", "Details");
            ui.text("status", "Ready");
            ui.button("save", "Save");
            ui.text_input("search", "Search", "molerat");
            ui.checkbox("enabled", "Enabled", true);
            ui.progress("download", "Download", 42.0, 0.0, 100.0);
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        assert_eq!(update.nodes[1].1.role(), Role::Paragraph);
        assert_eq!(update.nodes[1].1.value(), Some("Details"));
        assert_eq!(update.nodes[2].1.role(), Role::Label);
        assert_eq!(update.nodes[2].1.value(), Some("Ready"));
        assert_eq!(update.nodes[3].1.role(), Role::Button);
        assert_eq!(update.nodes[3].1.label(), Some("Save"));
        assert_eq!(update.nodes[4].1.role(), Role::TextInput);
        assert_eq!(update.nodes[4].1.label(), Some("Search"));
        assert_eq!(update.nodes[4].1.value(), Some("molerat"));
        assert_eq!(update.nodes[5].1.role(), Role::CheckBox);
        assert_eq!(update.nodes[5].1.toggled(), Some(Toggled::True));
        assert_eq!(update.nodes[6].1.role(), Role::ProgressIndicator);
        assert_eq!(update.nodes[6].1.numeric_value(), Some(42.0));
        assert_eq!(update.nodes[6].1.min_numeric_value(), Some(0.0));
        assert_eq!(update.nodes[6].1.max_numeric_value(), Some(100.0));
    }

    #[test]
    fn publishes_a_nested_list() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.group("main", "Main", |ui| {
                ui.list("tasks", "Tasks", |ui| {
                    ui.list_item("first", "First task");
                    ui.list_item("second", "Second task");
                });
            });
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        let (root_id, root) = &update.nodes[0];
        let (group_id, group) = &update.nodes[1];
        let (list_id, list) = &update.nodes[2];
        let (first_id, first) = &update.nodes[3];
        let (second_id, second) = &update.nodes[4];

        assert_eq!(*root_id, ROOT_ID);
        assert_eq!(root.children(), [*group_id]);
        assert_eq!(group.role(), Role::Group);
        assert_eq!(group.label(), Some("Main"));
        assert_eq!(group.children(), [*list_id]);
        assert_eq!(list.role(), Role::List);
        assert_eq!(list.label(), Some("Tasks"));
        assert_eq!(list.children(), [*first_id, *second_id]);
        assert_eq!(first.role(), Role::ListItem);
        assert_eq!(first.label(), Some("First task"));
        assert_eq!(second.role(), Role::ListItem);
        assert_eq!(second.label(), Some("Second task"));
    }

    #[test]
    fn publishes_node_state_and_focus() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.list("tasks", "Tasks", |ui| {
                ui.list_item("first", "First task")
                    .selected(true)
                    .disabled(true)
                    .expanded(false)
                    .focused(true);
                ui.list_item("hidden", "Hidden task").hidden(true);
            });
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        let (item_id, item) = &update.nodes[2];
        assert_eq!(update.focus, *item_id);
        assert_eq!(item.is_selected(), Some(true));
        assert!(item.is_disabled());
        assert_eq!(item.is_expanded(), Some(false));
        assert!(!item.is_hidden());
        assert!(update.nodes[3].1.is_hidden());
    }

    #[test]
    #[should_panic(expected = "only one semantic node can be focused")]
    fn rejects_multiple_focused_nodes() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.button("first", "First").focused(true);
            ui.button("second", "Second").focused(true);
        });
    }
}
