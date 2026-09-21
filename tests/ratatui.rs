use molerat::{
    Molerat,
    accesskit::{Node as AccessNode, NodeId, Role, TreeUpdate},
};
use ratatui::{
    Frame, Terminal,
    backend::TestBackend,
    layout::{Constraint, Layout},
    widgets::{Block, List, ListItem, ListState, Paragraph},
};
use std::error::Error;

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Debug)]
struct Task {
    id: &'static str,
    title: &'static str,
    details: &'static str,
}

#[derive(Debug)]
struct App {
    tasks: Vec<Task>,
    selected: usize,
}

impl Default for App {
    fn default() -> Self {
        Self {
            tasks: vec![
                Task {
                    id: "first",
                    title: "First task",
                    details: "Details for the first task",
                },
                Task {
                    id: "second",
                    title: "Second task",
                    details: "Details for the second task",
                },
            ],
            selected: 0,
        }
    }
}

impl App {
    /// Renders the application into a Ratatui frame.
    fn render(&self, frame: &mut Frame<'_>) {
        let [list_area, details_area] =
            Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)])
                .areas(frame.area());
        let items = self.tasks.iter().map(|task| ListItem::new(task.title));
        let list = List::new(items)
            .block(Block::bordered().title("Tasks"))
            .highlight_symbol("> ");
        let mut state = ListState::default().with_selected(Some(self.selected));
        frame.render_stateful_widget(list, list_area, &mut state);

        let details = Paragraph::new(self.tasks[self.selected].details)
            .block(Block::bordered().title("Details"));
        frame.render_widget(details, details_area);
    }

    /// Publishes the application state as a Molerat semantic tree.
    fn publish(&self, molerat: &mut Molerat) {
        molerat.publish(|ui| {
            ui.list("tasks", "Tasks", |ui| {
                for (index, task) in self.tasks.iter().enumerate() {
                    ui.list_item(task.id, task.title)
                        .selected(index == self.selected)
                        .focused(index == self.selected);
                }
            });
            ui.paragraph("details", self.tasks[self.selected].details)
                .label("Details");
        });
    }
}

/// Returns the latest semantic tree published by the application.
fn semantic_tree(molerat: &Molerat) -> &TreeUpdate {
    molerat
        .last_update()
        .unwrap_or_else(|| panic!("a semantic tree should have been published"))
}

/// Finds a semantic node by its role and accessible label.
fn node<'a>(update: &'a TreeUpdate, role: Role, label: &str) -> (NodeId, &'a AccessNode) {
    update
        .nodes
        .iter()
        .find(|(_, node)| node.role() == role && node.label() == Some(label))
        .map(|(id, node)| (*id, node))
        .unwrap_or_else(|| panic!("{role:?} node labeled {label:?} should be present"))
}

#[test]
fn view_and_semantic_tree_describe_the_same_state() -> TestResult {
    let app = App::default();
    let mut terminal = Terminal::new(TestBackend::new(48, 10))?;
    let mut molerat: Molerat = Molerat::new("tasks");

    terminal.draw(|frame| app.render(frame))?;
    app.publish(&mut molerat);

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("> First task"));
    assert!(rendered.contains("Details for the first task"));

    let update = semantic_tree(&molerat);
    let (list_id, list) = node(update, Role::List, "Tasks");
    let (first_id, first) = node(update, Role::ListItem, "First task");
    let (second_id, second) = node(update, Role::ListItem, "Second task");
    let (details_id, details) = node(update, Role::Paragraph, "Details");
    let root = &update.nodes[0].1;

    assert_eq!(root.children(), [list_id, details_id]);
    assert_eq!(list.children(), [first_id, second_id]);
    assert_eq!(first.is_selected(), Some(true));
    assert_eq!(second.is_selected(), Some(false));
    assert_eq!(update.focus, first_id);
    assert_eq!(details.value(), Some("Details for the first task"));

    Ok(())
}

#[test]
fn state_changes_preserve_semantic_node_ids() -> TestResult {
    let mut app = App::default();
    let mut terminal = Terminal::new(TestBackend::new(48, 10))?;
    let mut molerat: Molerat = Molerat::new("tasks");

    app.publish(&mut molerat);
    let initial = semantic_tree(&molerat);
    let (first_id, _) = node(initial, Role::ListItem, "First task");
    let (second_id, _) = node(initial, Role::ListItem, "Second task");

    app.selected = 1;
    terminal.draw(|frame| app.render(frame))?;
    app.publish(&mut molerat);

    let rendered = terminal.backend().to_string();
    assert!(rendered.contains("> Second task"));
    assert!(rendered.contains("Details for the second task"));

    let updated = semantic_tree(&molerat);
    let (updated_first_id, first) = node(updated, Role::ListItem, "First task");
    let (updated_second_id, second) = node(updated, Role::ListItem, "Second task");

    assert_eq!(updated_first_id, first_id);
    assert_eq!(updated_second_id, second_id);
    assert_eq!(first.is_selected(), Some(false));
    assert_eq!(second.is_selected(), Some(true));
    assert_eq!(updated.focus, second_id);

    Ok(())
}
