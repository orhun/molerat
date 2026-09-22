use accesskit::{
    Action, ActionData, ActionRequest, CustomAction, Node as AccessNode, NodeId, TreeId, TreeUpdate,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

/// Standard AccessKit actions that can be advertised independently of custom actions.
const STANDARD_ACTIONS: &[Action] = &[
    Action::Click,
    Action::Focus,
    Action::Blur,
    Action::Collapse,
    Action::Expand,
    Action::Decrement,
    Action::Increment,
    Action::HideTooltip,
    Action::ShowTooltip,
    Action::ReplaceSelectedText,
    Action::ScrollDown,
    Action::ScrollLeft,
    Action::ScrollRight,
    Action::ScrollUp,
    Action::ScrollIntoView,
    Action::ScrollToPoint,
    Action::SetScrollOffset,
    Action::SetTextSelection,
    Action::SetSequentialFocusNavigationStartingPoint,
    Action::SetValue,
    Action::ShowContextMenu,
];

/// Thread-safe semantic tree storage shared with the local control endpoint.
#[derive(Clone, Debug)]
pub(super) struct Store(Arc<RwLock<Tree>>);

impl Store {
    pub(super) fn new(application: &str) -> Self {
        Self(Arc::new(RwLock::new(Tree {
            application: application.to_owned(),
            root: None,
            nodes: HashMap::new(),
            focus: NodeId(0),
        })))
    }

    pub(super) fn update(&self, update: &TreeUpdate) {
        let mut tree = self.0.write().unwrap_or_else(|error| error.into_inner());
        tree.apply(update);
    }

    pub(super) fn snapshot(&self) -> Snapshot {
        self.0
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .snapshot()
    }

    /// Returns whether an action is currently advertised by its target node.
    pub(super) fn supports_action(&self, request: &ActionRequest) -> bool {
        if request.target_tree != TreeId::ROOT {
            return false;
        }

        let tree = self.0.read().unwrap_or_else(|error| error.into_inner());
        let Some(node) = tree.exposed_node(request.target_node) else {
            return false;
        };
        if node.is_disabled() || !valid_action_data(request.action, request.data.as_ref()) {
            return false;
        }
        if request.action != Action::CustomAction {
            return node.supports_action(request.action);
        }

        let Some(ActionData::CustomAction(id)) = request.data.as_ref() else {
            return false;
        };
        node.custom_actions().iter().any(|action| action.id == *id)
    }
}

/// Checks that an action carries exactly the payload required by AccessKit.
fn valid_action_data(action: Action, data: Option<&ActionData>) -> bool {
    match action {
        Action::CustomAction => matches!(data, Some(ActionData::CustomAction(_))),
        Action::ReplaceSelectedText => matches!(data, Some(ActionData::Value(_))),
        Action::ScrollDown | Action::ScrollLeft | Action::ScrollRight | Action::ScrollUp => {
            matches!(data, Some(ActionData::ScrollUnit(_)))
        }
        Action::ScrollIntoView => data.is_none() || matches!(data, Some(ActionData::ScrollHint(_))),
        Action::ScrollToPoint => matches!(data, Some(ActionData::ScrollToPoint(_))),
        Action::SetScrollOffset => matches!(data, Some(ActionData::SetScrollOffset(_))),
        Action::SetTextSelection => matches!(data, Some(ActionData::SetTextSelection(_))),
        Action::SetValue => matches!(
            data,
            Some(ActionData::Value(_) | ActionData::NumericValue(_))
        ),
        Action::Click
        | Action::Focus
        | Action::Blur
        | Action::Collapse
        | Action::Expand
        | Action::Decrement
        | Action::Increment
        | Action::HideTooltip
        | Action::ShowTooltip
        | Action::SetSequentialFocusNavigationStartingPoint
        | Action::ShowContextMenu => data.is_none(),
    }
}

#[derive(Debug)]
struct Tree {
    application: String,
    root: Option<NodeId>,
    nodes: HashMap<NodeId, AccessNode>,
    focus: NodeId,
}

impl Tree {
    fn apply(&mut self, update: &TreeUpdate) {
        if let Some(tree) = &update.tree {
            self.root = Some(tree.root);
        }
        self.nodes.extend(update.nodes.iter().cloned());
        self.focus = update.focus;
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            application: self.application.clone(),
            root: self
                .root
                .and_then(|root| Node::from_tree(root, self.focus, &self.nodes)),
        }
    }

    /// Finds a node reachable through the current visible semantic tree.
    fn exposed_node(&self, target: NodeId) -> Option<&AccessNode> {
        let mut pending = vec![self.root?];
        let mut visited = HashSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if node.is_hidden() {
                continue;
            }
            if id == target {
                return Some(node);
            }
            pending.extend(node.children());
        }
        None
    }
}

/// Serializable view returned by the `get_ui` MCP tool.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    /// Name of the application that published this semantic tree.
    pub(super) application: String,
    /// Root window of the semantic tree, if the application has published one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) root: Option<Node>,
}

/// One node in a serializable semantic view.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Node {
    /// Stable node ID accepted by the `perform_action` tool.
    id: u64,
    /// AccessKit role describing the semantic kind of this node.
    pub(super) role: String,
    /// Human-readable name of the node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) label: Option<String>,
    /// Current textual value exposed by the node.
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    /// Additional context describing the node.
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    /// Whether this node is currently selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    selected: Option<bool>,
    /// Whether this node currently has semantic focus.
    #[serde(default, skip_serializing_if = "is_false")]
    pub(super) focused: bool,
    /// Whether this node is unavailable for interaction.
    #[serde(default, skip_serializing_if = "is_false")]
    disabled: bool,
    /// Current numeric value exposed by the node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) numeric_value: Option<f64>,
    /// Minimum value in the node's numeric range.
    #[serde(skip_serializing_if = "Option::is_none")]
    min_numeric_value: Option<f64>,
    /// Maximum value in the node's numeric range.
    #[serde(skip_serializing_if = "Option::is_none")]
    max_numeric_value: Option<f64>,
    /// Standard AccessKit actions accepted by `perform_action` for this node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<Action>,
    /// Application-defined actions accepted by `perform_action` for this node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    custom_actions: Vec<CustomAction>,
    /// Visible semantic descendants of this node.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) children: Vec<Self>,
}

impl Node {
    fn from_tree(id: NodeId, focus: NodeId, nodes: &HashMap<NodeId, AccessNode>) -> Option<Self> {
        let node = nodes.get(&id)?;
        if node.is_hidden() {
            return None;
        }

        let disabled = node.is_disabled();
        Some(Self {
            id: id.0,
            role: format!("{:?}", node.role()),
            label: node.label().map(str::to_owned),
            value: node.value().map(str::to_owned),
            description: node.description().map(str::to_owned),
            selected: node.is_selected(),
            focused: id == focus,
            disabled,
            numeric_value: node.numeric_value(),
            min_numeric_value: node.min_numeric_value(),
            max_numeric_value: node.max_numeric_value(),
            actions: if disabled {
                Vec::new()
            } else {
                STANDARD_ACTIONS
                    .iter()
                    .copied()
                    .filter(|action| node.supports_action(*action))
                    .collect()
            },
            custom_actions: if disabled {
                Vec::new()
            } else {
                node.custom_actions().to_vec()
            },
            children: node
                .children()
                .iter()
                .filter_map(|child| Self::from_tree(*child, focus, nodes))
                .collect(),
        })
    }
}

fn is_false(value: &bool) -> bool {
    !value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Molerat;

    #[test]
    fn hidden_nodes_are_not_exposed() -> Result<(), Box<dyn std::error::Error>> {
        let store = Store::new("Agent tasks");
        let mut molerat = Molerat::<()>::new("Agent tasks");
        molerat.publish(|ui| {
            ui.text("visible", "Visible");
            ui.text("hidden", "Hidden")
                .on_click(|| ())
                .with_accesskit(|node| node.set_hidden());
        });
        let Some(update) = molerat.last_update() else {
            return Err("publication did not produce an update".into());
        };
        let hidden_id = update.nodes[2].0;
        store.update(update);

        let snapshot = store.snapshot();
        let Some(root) = snapshot.root else {
            return Err("published tree has no root".into());
        };
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].value.as_deref(), Some("Visible"));
        assert!(!store.supports_action(&ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: hidden_id,
            data: None,
        }));
        Ok(())
    }

    #[test]
    fn disabled_node_actions_are_not_exposed_or_supported() -> Result<(), Box<dyn std::error::Error>>
    {
        let store = Store::new("Agent tasks");
        let mut molerat = Molerat::<()>::new("Agent tasks");
        molerat.publish(|ui| {
            ui.button("delete", "Delete")
                .disabled(true)
                .on_click(|| ())
                .custom_action("force-delete", "Force delete", || ());
        });
        let Some(update) = molerat.last_update() else {
            return Err("publication did not produce an update".into());
        };
        let node_id = update.nodes[1].0;
        let custom_action_id = update.nodes[1].1.custom_actions()[0].id;
        store.update(update);

        let Some(root) = store.snapshot().root else {
            return Err("published tree has no root".into());
        };
        assert!(root.children[0].disabled);
        assert!(root.children[0].actions.is_empty());
        assert!(root.children[0].custom_actions.is_empty());
        assert!(!store.supports_action(&ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: node_id,
            data: None,
        }));
        assert!(!store.supports_action(&ActionRequest {
            action: Action::CustomAction,
            target_tree: TreeId::ROOT,
            target_node: node_id,
            data: Some(ActionData::CustomAction(custom_action_id)),
        }));
        Ok(())
    }

    #[test]
    fn actions_require_their_accesskit_payload() -> Result<(), Box<dyn std::error::Error>> {
        let store = Store::new("Editor");
        let mut molerat = Molerat::<()>::new("Editor");
        molerat.publish(|ui| {
            ui.node("input", accesskit::Role::TextInput)
                .on_click(|| ())
                .on_action(Action::SetValue, |_| ());
        });
        let Some(update) = molerat.last_update() else {
            return Err("publication did not produce an update".into());
        };
        let node_id = update.nodes[1].0;
        store.update(update);

        let request = |action, data| ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: node_id,
            data,
        };
        assert!(!store.supports_action(&request(Action::SetValue, None)));
        assert!(!store.supports_action(&request(
            Action::Click,
            Some(ActionData::Value("unexpected".into())),
        )));
        assert!(store.supports_action(&request(
            Action::SetValue,
            Some(ActionData::Value("updated".into())),
        )));
        assert!(store.supports_action(&request(
            Action::SetValue,
            Some(ActionData::NumericValue(42.0)),
        )));
        Ok(())
    }

    #[test]
    fn supported_actions_are_exposed() -> Result<(), Box<dyn std::error::Error>> {
        let store = Store::new("Agent tasks");
        let mut molerat = Molerat::<()>::new("Agent tasks");
        molerat.publish(|ui| {
            ui.button("run", "Run")
                .on_click(|| ())
                .on_focus(|| ())
                .custom_action("retry", "Retry task", || ());
        });
        let Some(update) = molerat.last_update() else {
            return Err("publication did not produce an update".into());
        };
        store.update(update);

        let snapshot = store.snapshot();
        let json = serde_json::to_value(&snapshot)?;
        assert_eq!(
            json["root"]["children"][0]["actions"],
            serde_json::json!(["click", "focus"])
        );
        assert_eq!(
            json["root"]["children"][0]["customActions"][0]["description"].as_str(),
            Some("Retry task")
        );

        let Some(root) = snapshot.root else {
            return Err("published tree has no root".into());
        };
        let button = &root.children[0];
        assert_eq!(button.actions, [Action::Click, Action::Focus]);
        assert_eq!(button.custom_actions.len(), 1);
        assert_eq!(button.custom_actions[0].description, "Retry task");
        Ok(())
    }
}
