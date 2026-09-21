use accesskit::Node;

/// Configures a paragraph added to a semantic tree.
#[derive(Debug)]
pub struct Paragraph<'a> {
    /// The AccessKit node being configured.
    node: &'a mut Node,
}

impl<'a> Paragraph<'a> {
    /// Creates a paragraph builder for an AccessKit node.
    pub(crate) fn new(node: &'a mut Node) -> Self {
        Self { node }
    }

    /// Sets the paragraph's accessible label.
    pub fn label(self, label: impl Into<String>) -> Self {
        self.node.set_label(label);
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::Molerat;
    use accesskit::{NodeId, Role, TreeId, TreeInfo};

    const ROOT_ID: NodeId = NodeId(0);

    #[test]
    fn publishes_a_labelled_paragraph() {
        let mut molerat: Molerat = Molerat::new("example");

        molerat.publish(|ui| {
            ui.paragraph("details", "Three tasks are overdue")
                .label("Details");
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
    }
}
