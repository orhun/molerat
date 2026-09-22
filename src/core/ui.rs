use super::{action::Actions, node::Node};
#[cfg(feature = "atspi")]
use crate::atspi::Atspi;
#[cfg(feature = "mcp")]
use crate::mcp::Endpoint;
use accesskit::{
    ActionRequest, Live, Node as AccessNode, NodeId, Role, TreeId, TreeInfo, TreeUpdate,
};
use std::collections::{HashMap, VecDeque};

/// The node ID reserved for the application root.
const ROOT_ID: NodeId = NodeId(0);

/// The application-owned Molerat integration handle.
#[derive(Debug)]
pub struct Molerat<M = ()> {
    /// The application name exposed to accessibility and MCP consumers.
    name: String,
    /// The most recent semantic-tree update published by the application.
    last_update: Option<TreeUpdate>,
    /// Stable AccessKit node IDs indexed by hierarchical application-provided keys.
    node_ids: HashMap<Vec<String>, NodeId>,
    /// The next AccessKit node ID available for allocation.
    next_node_id: u64,
    /// Registry for actions supported by the current semantic tree.
    actions: Actions<M>,
    /// Messages produced by handled action requests.
    messages: VecDeque<M>,
    /// Optional connection to the Unix AT-SPI accessibility bus.
    #[cfg(feature = "atspi")]
    atspi: Option<Atspi>,
    /// Optional local endpoint serving semantic snapshots to the MCP facade.
    #[cfg(feature = "mcp")]
    mcp: Option<Endpoint>,
}

impl<M> Molerat<M> {
    /// Creates an integration handle for an application using message type `M`.
    ///
    /// # Examples
    ///
    /// ```
    /// use molerat::Molerat;
    ///
    /// let molerat = Molerat::<()>::new("Tasks");
    /// assert_eq!(molerat.name(), "Tasks");
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            last_update: None,
            node_ids: HashMap::new(),
            next_node_id: ROOT_ID.0 + 1,
            actions: Actions::default(),
            messages: VecDeque::new(),
            #[cfg(feature = "atspi")]
            atspi: None,
            #[cfg(feature = "mcp")]
            mcp: None,
        }
    }

    /// Creates an integration handle with MCP exposure enabled.
    ///
    /// The handle automatically maintains a private local endpoint for the bundled `molerat-mcp`
    /// stdio server. No async runtime or transport setup is required in the application.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use molerat::Molerat;
    ///
    /// # fn main() -> std::io::Result<()> {
    /// let mut molerat = Molerat::<()>::new_mcp("Tasks")?;
    /// molerat.publish(|ui| {
    ///     ui.status("status", "Ready");
    /// });
    /// # Ok(())
    /// # }
    /// ```
    #[cfg(feature = "mcp")]
    pub fn new_mcp(name: impl Into<String>) -> std::io::Result<Self> {
        let mut molerat = Self::new(name);
        molerat.mcp = Some(Endpoint::spawn(&molerat.name)?);
        Ok(molerat)
    }

    /// Creates an integration handle with Unix AT-SPI exposure enabled.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use molerat::Molerat;
    ///
    /// let molerat = Molerat::<()>::new_atspi("Tasks");
    /// ```
    #[cfg(feature = "atspi")]
    pub fn new_atspi(name: impl Into<String>) -> Self {
        let mut molerat = Self::new(name);
        molerat.atspi = Some(Atspi::new(None));
        molerat
    }

    /// Updates whether the AT-SPI application window is focused.
    ///
    /// This has no effect on handles created with [`Molerat::new`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use molerat::Molerat;
    ///
    /// let mut molerat = Molerat::<()>::new_atspi("Tasks");
    /// molerat.set_window_focused(false);
    /// ```
    #[cfg(feature = "atspi")]
    pub fn set_window_focused(&mut self, focused: bool) {
        if let Some(atspi) = &mut self.atspi {
            atspi.set_window_focused(focused);
        }
    }

    /// Returns the application name exposed to consumers.
    ///
    /// # Examples
    ///
    /// ```
    /// use molerat::Molerat;
    ///
    /// let molerat = Molerat::<()>::new("Tasks");
    /// assert_eq!(molerat.name(), "Tasks");
    /// ```
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Publishes an AccessKit tree update.
    ///
    /// # Examples
    ///
    /// ```
    /// use molerat::Molerat;
    /// # use molerat::accesskit::{Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
    /// # let root = NodeId(0);
    /// # let update = TreeUpdate {
    /// #     nodes: vec![(root, Node::new(Role::Window))],
    /// #     tree: Some(TreeInfo::new(root)),
    /// #     tree_id: TreeId::ROOT,
    /// #     focus: root,
    /// # };
    ///
    /// let mut molerat: Molerat = Molerat::new("Tasks");
    /// molerat.update(update);
    /// ```
    pub fn update(&mut self, update: TreeUpdate) {
        self.actions.clear();
        self.store_update(update);
    }

    /// Returns the most recently published AccessKit tree update.
    ///
    /// # Examples
    ///
    /// ```
    /// use molerat::Molerat;
    ///
    /// let mut molerat: Molerat = Molerat::new("Tasks");
    /// molerat.publish(|ui| {
    ///     ui.text("status", "Ready");
    /// });
    /// assert!(molerat.last_update().is_some());
    /// ```
    pub fn last_update(&self) -> Option<&TreeUpdate> {
        self.last_update.as_ref()
    }

    /// Builds and publishes a complete semantic tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use molerat::Molerat;
    ///
    /// let mut molerat: Molerat = Molerat::new("Tasks");
    /// molerat.publish(|ui| {
    ///     ui.text("status", "Ready");
    /// });
    /// ```
    pub fn publish(&mut self, build: impl FnOnce(&mut Ui<'_, M>)) {
        self.actions.clear();
        let mut ui = Ui::new(
            &self.name,
            &mut self.node_ids,
            &mut self.next_node_id,
            &mut self.actions,
        );
        build(&mut ui);
        let update = ui.finish();
        self.store_update(update);
    }

    /// Handles an AccessKit action request and queues its application message.
    ///
    /// # Examples
    ///
    /// ```
    /// use molerat::Molerat;
    /// # use molerat::accesskit::{Action, ActionRequest, TreeId};
    /// # #[derive(Debug, PartialEq)]
    /// # enum Message { Save }
    ///
    /// let mut molerat = Molerat::new("Editor");
    /// molerat.publish(|ui| {
    ///     ui.button("save", "Save").on_click(|| Message::Save);
    /// });
    /// # let Some(update) = molerat.last_update() else { return };
    /// # let request = ActionRequest {
    /// #     action: Action::Click,
    /// #     target_tree: TreeId::ROOT,
    /// #     target_node: update.nodes[1].0,
    /// #     data: None,
    /// # };
    ///
    /// assert!(molerat.handle_action(request));
    /// ```
    pub fn handle_action(&mut self, request: ActionRequest) -> bool {
        if request.target_tree != TreeId::ROOT {
            return false;
        }

        let Some(message) = self.actions.handle(&request) else {
            return false;
        };
        self.messages.push_back(message);
        true
    }

    /// Drains the application messages produced by handled action requests.
    ///
    /// When constructed with `Molerat::new_atspi`, pending platform action requests are handled
    /// before messages are drained. This keeps application action handlers on the TUI thread.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # use molerat::accesskit::{Action, ActionRequest, TreeId};
    /// # #[derive(Debug, PartialEq)]
    /// # enum Message { Save }
    /// # let mut molerat = Molerat::new("Editor");
    /// # molerat.publish(|ui| {
    /// #     ui.button("save", "Save").on_click(|| Message::Save);
    /// # });
    /// # let Some(update) = molerat.last_update() else { return };
    /// # let request = ActionRequest {
    /// #     action: Action::Click,
    /// #     target_tree: TreeId::ROOT,
    /// #     target_node: update.nodes[1].0,
    /// #     data: None,
    /// # };
    /// # assert!(molerat.handle_action(request));
    /// let messages = molerat.messages().collect::<Vec<_>>();
    /// assert_eq!(messages, [Message::Save]);
    /// ```
    pub fn messages(&mut self) -> impl Iterator<Item = M> + '_ {
        #[cfg(feature = "atspi")]
        {
            let requests = self
                .atspi
                .as_ref()
                .map_or_else(Vec::new, Atspi::drain_actions);
            for request in requests {
                self.handle_action(request);
            }
        }
        self.messages.drain(..)
    }

    /// Stores an update and forwards it to active consumers.
    fn store_update(&mut self, update: TreeUpdate) {
        #[cfg(feature = "mcp")]
        if let Some(mcp) = &self.mcp {
            mcp.update(&update);
        }
        #[cfg(feature = "atspi")]
        if let Some(atspi) = &mut self.atspi {
            atspi.update(&update);
        }
        self.last_update = Some(update);
    }
}

/// Builds one complete semantic representation of an application.
#[derive(Debug)]
pub struct Ui<'a, M = ()> {
    /// Stable AccessKit node IDs indexed by hierarchical application-provided keys.
    node_ids: &'a mut HashMap<Vec<String>, NodeId>,
    /// The next AccessKit node ID available for allocation.
    next_node_id: &'a mut u64,
    /// Nodes added during this publication.
    nodes: Vec<(NodeId, AccessNode)>,
    /// Indices of the containers that currently receive child nodes.
    parent_nodes: Vec<usize>,
    /// Key segments identifying the current container path.
    path: Vec<String>,
    /// The node selected as semantic focus in this publication.
    focus: Option<NodeId>,
    /// Registry for actions supported by nodes in this publication.
    actions: &'a mut Actions<M>,
}

impl<'a, M> Ui<'a, M> {
    /// Creates a semantic-tree builder using the handle's persistent ID state.
    fn new(
        application_name: &'a str,
        node_ids: &'a mut HashMap<Vec<String>, NodeId>,
        next_node_id: &'a mut u64,
        actions: &'a mut Actions<M>,
    ) -> Self {
        let mut root = AccessNode::new(Role::Window);
        root.set_label(application_name);
        Self {
            node_ids,
            next_node_id,
            nodes: vec![(ROOT_ID, root)],
            parent_nodes: vec![0],
            path: Vec::new(),
            focus: None,
            actions,
        }
    }

    /// Adds a semantic node with the given AccessKit role.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::{Molerat, accesskit::Role};
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.node("status", Role::Status)
    ///     .label("Application status")
    ///     .value("Ready");
    /// # });
    /// ```
    pub fn node(&mut self, key: impl Into<String>, role: Role) -> Node<'_, M> {
        let node_index = self.add_node(key, role);

        self.node_builder(node_index)
    }

    /// Registers an application-wide command on the semantic root node.
    ///
    /// This is a convenience wrapper around [`Node::custom_action`] that uses the application root
    /// as the action target.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # enum Message { Refresh }
    /// # let mut molerat = Molerat::<Message>::new("example");
    /// # molerat.publish(|ui| {
    /// ui.command("refresh", "Refresh application", || Message::Refresh);
    /// # });
    /// ```
    pub fn command(
        &mut self,
        key: impl Into<String>,
        description: impl Into<String>,
        handler: impl Fn() -> M + 'static,
    ) {
        self.node_builder(0)
            .custom_action(key, description, handler);
    }

    /// Adds a semantic container and builds its children.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::{Molerat, accesskit::Role};
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.container("sidebar", Role::Group, |ui| {
    ///     ui.text("status", "Ready");
    /// });
    /// # });
    /// ```
    pub fn container(
        &mut self,
        key: impl Into<String>,
        role: Role,
        children: impl FnOnce(&mut Self),
    ) -> Node<'_, M> {
        let node_index = self.add_container(key, role, children);

        self.node_builder(node_index)
    }

    /// Adds a labelled table and builds its rows.
    ///
    /// The row and column counts are derived from the completed table structure.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.table("processes", "Processes", |ui| {
    ///     ui.row("headers", |ui| {
    ///         ui.column_header("name", "Name");
    ///         ui.column_header("cpu", "CPU usage");
    ///     });
    ///     ui.row("molerat", |ui| {
    ///         ui.row_header("name", "molerat");
    ///         ui.cell("cpu", "18 percent");
    ///     });
    /// });
    /// # });
    /// ```
    pub fn table(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        rows: impl FnOnce(&mut Self),
    ) -> Node<'_, M> {
        let node_index = self.add_container(key, Role::Table, rows);
        let row_ids = self.nodes[node_index].1.children().to_vec();
        let row_count = row_ids.len();
        let column_count = row_ids
            .iter()
            .filter_map(|row_id| self.nodes.iter().find(|(id, _)| id == row_id))
            .map(|(_, row)| row.children().len())
            .max()
            .unwrap_or_default();

        self.node_builder(node_index)
            .label(label)
            .with_accesskit(|node| {
                node.set_row_count(row_count);
                node.set_column_count(column_count);
            })
    }

    /// Adds a row to the current table and builds its cells.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// # ui.table("processes", "Processes", |ui| {
    /// ui.row("molerat", |ui| {
    ///     ui.cell("name", "molerat");
    ///     ui.cell("cpu", "18 percent");
    /// });
    /// # });
    /// # });
    /// ```
    pub fn row(&mut self, key: impl Into<String>, cells: impl FnOnce(&mut Self)) -> Node<'_, M> {
        let row_index = self.next_child_index(Role::Table);
        self.container(key, Role::Row, cells)
            .with_accesskit(|node| node.set_row_index(row_index))
    }

    /// Adds a cell to the current table row.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// # ui.table("processes", "Processes", |ui| {
    /// # ui.row("molerat", |ui| {
    /// ui.cell("cpu", "18 percent");
    /// # });
    /// # });
    /// # });
    /// ```
    pub fn cell(&mut self, key: impl Into<String>, label: impl Into<String>) -> Node<'_, M> {
        self.table_cell(key, label, Role::Cell)
    }

    /// Adds a column-header cell to the current table row.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// # ui.table("processes", "Processes", |ui| {
    /// # ui.row("headers", |ui| {
    /// ui.column_header("cpu", "CPU usage");
    /// # });
    /// # });
    /// # });
    /// ```
    pub fn column_header(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
    ) -> Node<'_, M> {
        self.table_cell(key, label, Role::ColumnHeader)
    }

    /// Adds a row-header cell to the current table row.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// # ui.table("processes", "Processes", |ui| {
    /// # ui.row("molerat", |ui| {
    /// ui.row_header("name", "molerat");
    /// # });
    /// # });
    /// # });
    /// ```
    pub fn row_header(&mut self, key: impl Into<String>, label: impl Into<String>) -> Node<'_, M> {
        self.table_cell(key, label, Role::RowHeader)
    }

    /// Adds a semantic container, builds its children, and returns its node index.
    fn add_container(
        &mut self,
        key: impl Into<String>,
        role: Role,
        children: impl FnOnce(&mut Self),
    ) -> usize {
        let key = key.into();
        let node_index = self.add_node(key.clone(), role);
        let parent_depth = self.parent_nodes.len();
        let path_depth = self.path.len();

        self.parent_nodes.push(node_index);
        self.path.push(key);
        children(self);
        self.parent_nodes.truncate(parent_depth);
        self.path.truncate(path_depth);

        node_index
    }

    /// Adds a labelled group and builds its children.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.group("summary", "System summary", |ui| {
    ///     ui.text("status", "All systems operational");
    /// });
    /// # });
    /// ```
    pub fn group(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        children: impl FnOnce(&mut Self),
    ) -> Node<'_, M> {
        self.container(key, Role::Group, children).label(label)
    }

    /// Adds a labelled list and builds its items.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.list("alerts", "Recent alerts", |ui| {
    ///     ui.list_item("backup", "Backup completed");
    /// });
    /// # });
    /// ```
    pub fn list(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        children: impl FnOnce(&mut Self),
    ) -> Node<'_, M> {
        self.container(key, Role::List, children).label(label)
    }

    /// Adds a labelled selectable list and builds its options.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.list_box("recipes", "Recipes", |ui| {
    ///     ui.list_box_option("ratatouille", "Ratatouille").selected(true);
    ///     ui.list_box_option("confit", "Confit byaldi");
    /// });
    /// # });
    /// ```
    pub fn list_box(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        children: impl FnOnce(&mut Self),
    ) -> Node<'_, M> {
        self.container(key, Role::ListBox, children)
            .label(label)
            .with_accesskit(|node| {
                let size = node.children().len();
                node.set_size_of_set(size);
            })
    }

    /// Adds a list item with an accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// # ui.list("alerts", "Recent alerts", |ui| {
    /// ui.list_item("backup", "Backup completed");
    /// # });
    /// # });
    /// ```
    pub fn list_item(&mut self, key: impl Into<String>, label: impl Into<String>) -> Node<'_, M> {
        self.node(key, Role::ListItem).label(label)
    }

    /// Adds an option to a selectable list with its position in that list.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// # ui.list_box("recipes", "Recipes", |ui| {
    /// ui.list_box_option("ratatouille", "Ratatouille")
    ///     .selected(true)
    ///     .focused(true);
    /// # });
    /// # });
    /// ```
    pub fn list_box_option(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
    ) -> Node<'_, M> {
        let position = self.next_child_index(Role::ListBox);
        self.node(key, Role::ListBoxOption)
            .label(label)
            .with_accesskit(|node| node.set_position_in_set(position))
    }

    /// Adds a paragraph containing `text`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.paragraph("instructions", "Select a recipe to begin.");
    /// # });
    /// ```
    pub fn paragraph(&mut self, key: impl Into<String>, text: impl Into<String>) -> Node<'_, M> {
        self.node(key, Role::Paragraph).value(text)
    }

    /// Adds a static text node.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.text("status", "Ready");
    /// # });
    /// ```
    pub fn text(&mut self, key: impl Into<String>, text: impl Into<String>) -> Node<'_, M> {
        self.node(key, Role::Label).value(text)
    }

    /// Adds a polite live status message.
    ///
    /// Changes are announced without interrupting the current screen-reader output.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.status("updates", "Metric updates paused");
    /// # });
    /// ```
    pub fn status(&mut self, key: impl Into<String>, message: impl Into<String>) -> Node<'_, M> {
        self.node(key, Role::Status)
            .label(message)
            .with_accesskit(|node| node.set_live(Live::Polite))
    }

    /// Adds an assertive live alert message.
    ///
    /// Use this only for urgent information that should interrupt the current screen-reader output.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.alert("temperature", "CPU temperature is critically high");
    /// # });
    /// ```
    pub fn alert(&mut self, key: impl Into<String>, message: impl Into<String>) -> Node<'_, M> {
        self.node(key, Role::Alert)
            .label(message)
            .with_accesskit(|node| node.set_live(Live::Assertive))
    }

    /// Adds a button with an accessible label.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.button("save", "Save");
    /// # });
    /// ```
    pub fn button(&mut self, key: impl Into<String>, label: impl Into<String>) -> Node<'_, M> {
        self.node(key, Role::Button).label(label)
    }

    /// Adds a text input with a label and current value.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.text_input("search", "Search", "molerat");
    /// # });
    /// ```
    pub fn text_input(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Node<'_, M> {
        self.node(key, Role::TextInput).label(label).value(value)
    }

    /// Adds a checkbox with a label and checked state.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.checkbox("auto-refresh", "Auto refresh", true);
    /// # });
    /// ```
    pub fn checkbox(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        checked: bool,
    ) -> Node<'_, M> {
        self.node(key, Role::CheckBox).label(label).toggled(checked)
    }

    /// Adds a progress indicator with a label and numeric range.
    ///
    /// # Examples
    ///
    /// ```
    /// # use molerat::Molerat;
    /// # let mut molerat: Molerat = Molerat::new("example");
    /// # molerat.publish(|ui| {
    /// ui.progress("cpu", "CPU usage", 42.0, 0.0, 100.0);
    /// # });
    /// ```
    pub fn progress(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        value: f64,
        minimum: f64,
        maximum: f64,
    ) -> Node<'_, M> {
        self.node(key, Role::ProgressIndicator)
            .label(label)
            .numeric_range(minimum, maximum)
            .numeric_value(value)
    }

    /// Adds a table cell with its position in the current row.
    fn table_cell(
        &mut self,
        key: impl Into<String>,
        label: impl Into<String>,
        role: Role,
    ) -> Node<'_, M> {
        let column_index = self.next_child_index(Role::Row);
        self.node(key, role)
            .label(label)
            .with_accesskit(|node| node.set_column_index(column_index))
    }

    /// Returns the next child index after checking the current semantic parent.
    fn next_child_index(&self, expected_parent_role: Role) -> usize {
        let Some(parent_index) = self.parent_nodes.last().copied() else {
            unreachable!("the application root is always a semantic parent");
        };
        let parent = &self.nodes[parent_index].1;
        assert_eq!(
            parent.role(),
            expected_parent_role,
            "semantic child must be added to a {expected_parent_role:?} node"
        );
        parent.children().len()
    }

    /// Adds a node under the current semantic parent and returns its vector index.
    fn add_node(&mut self, key: impl Into<String>, role: Role) -> usize {
        let mut path = self.path.clone();
        path.push(key.into());
        let id = self.node_id(path);
        let node_index = self.nodes.len();

        let Some(parent_index) = self.parent_nodes.last().copied() else {
            unreachable!("the application root is always a semantic parent");
        };
        self.nodes[parent_index].1.push_child(id);
        self.nodes.push((id, AccessNode::new(role)));

        node_index
    }

    /// Returns the stable node ID associated with `path`, allocating it if necessary.
    fn node_id(&mut self, path: Vec<String>) -> NodeId {
        if let Some(id) = self.node_ids.get(&path) {
            return *id;
        }

        let id = NodeId(*self.next_node_id);
        *self.next_node_id += 1;
        self.node_ids.insert(path, id);
        id
    }

    /// Returns a configurable node builder for the node at `node_index`.
    fn node_builder(&mut self, node_index: usize) -> Node<'_, M> {
        let (nodes, focus, actions) = (&mut self.nodes, &mut self.focus, &mut self.actions);
        let id = nodes[node_index].0;
        Node::new(id, &mut nodes[node_index].1, focus, actions)
    }

    /// Finishes the builder and returns the complete AccessKit tree update.
    fn finish(self) -> TreeUpdate {
        TreeUpdate {
            nodes: self.nodes,
            tree: Some(TreeInfo::new(ROOT_ID)),
            tree_id: TreeId::ROOT,
            focus: self.focus.unwrap_or(ROOT_ID),
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

    #[test]
    fn scopes_identical_keys_to_their_parent() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.group("left", "Left", |ui| {
                ui.text("title", "Left title");
            });
            ui.group("right", "Right", |ui| {
                ui.text("title", "Right title");
            });
        });

        let Some(update) = molerat.last_update() else {
            panic!("a tree update should have been published");
        };
        assert_ne!(update.nodes[2].0, update.nodes[4].0);
    }
}
