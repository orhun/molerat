use accesskit::{Action, CustomAction, Node as AccessNode, NodeId, TreeUpdate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
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
}

/// Serializable view returned by the `get_ui` MCP tool.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Snapshot {
    pub(super) application: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) root: Option<Node>,
}

/// One node in a serializable semantic view.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Node {
    id: u64,
    pub(super) role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selected: Option<bool>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(super) focused: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_numeric_value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_numeric_value: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    actions: Vec<Action>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    custom_actions: Vec<CustomAction>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) children: Vec<Self>,
}

impl Node {
    fn from_tree(id: NodeId, focus: NodeId, nodes: &HashMap<NodeId, AccessNode>) -> Option<Self> {
        let node = nodes.get(&id)?;
        if node.is_hidden() {
            return None;
        }

        Some(Self {
            id: id.0,
            role: format!("{:?}", node.role()),
            label: node.label().map(str::to_owned),
            value: node.value().map(str::to_owned),
            description: node.description().map(str::to_owned),
            selected: node.is_selected(),
            focused: id == focus,
            disabled: node.is_disabled(),
            numeric_value: node.numeric_value(),
            min_numeric_value: node.min_numeric_value(),
            max_numeric_value: node.max_numeric_value(),
            actions: STANDARD_ACTIONS
                .iter()
                .copied()
                .filter(|action| node.supports_action(*action))
                .collect(),
            custom_actions: node.custom_actions().to_vec(),
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
                .with_accesskit(|node| node.set_hidden());
        });
        let Some(update) = molerat.last_update() else {
            return Err("publication did not produce an update".into());
        };
        store.update(update);

        let snapshot = store.snapshot();
        let Some(root) = snapshot.root else {
            return Err("published tree has no root".into());
        };
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].value.as_deref(), Some("Visible"));
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
