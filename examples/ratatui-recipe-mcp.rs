//! Agent task recipe exposed over MCP through Molerat.

use molerat::Molerat;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Layout},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, BorderType, Gauge, List, ListState, Padding, Paragraph, Wrap},
};
use std::{io, time::Duration};

struct Task {
    title: &'static str,
    description: &'static str,
    progress: u16,
}

const TASKS: &[Task] = &[
    Task {
        title: "Inspect the pantry",
        description: "Inventory the workspace and identify the relevant files.",
        progress: 25,
    },
    Task {
        title: "Chop the changes",
        description: "Make the smallest focused code changes for the task.",
        progress: 50,
    },
    Task {
        title: "Simmer the tests",
        description: "Run checks and resolve any failures.",
        progress: 75,
    },
    Task {
        title: "Plate the result",
        description: "Summarize the finished work for the user.",
        progress: 100,
    },
];

#[derive(Debug)]
enum Message {
    Select(usize),
}

#[derive(Debug, Default)]
struct App {
    selected: usize,
}

impl App {
    fn render(&self, frame: &mut Frame<'_>) {
        let vertical = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ]);
        let horizontal =
            Layout::horizontal([Constraint::Percentage(40), Constraint::Fill(1)]).spacing(1);
        let [main_area, progress_area, key_bindings_area] = frame.area().layout(&vertical);
        let [tasks_area, details_area] = main_area.layout(&horizontal);

        let tasks = List::new(TASKS.iter().map(|task| task.title))
            .block(panel(" Agent queue ").padding(Padding::horizontal(1)))
            .white()
            .highlight_style(Style::new().yellow().bold())
            .highlight_symbol("▸ ");
        let mut state = ListState::default().with_selected(Some(self.selected));
        frame.render_stateful_widget(tasks, tasks_area, &mut state);

        let task = &TASKS[self.selected];
        let details = Paragraph::new(vec![
            task.title.green().into(),
            "".into(),
            task.description.white().italic().into(),
            "".into(),
            Line::from("MCP: ready for agents").cyan(),
        ])
        .block(panel(" Task details ").padding(Padding::horizontal(1)))
        .wrap(Wrap { trim: true });
        frame.render_widget(details, details_area);

        let progress = Gauge::default()
            .block(panel(" Progress "))
            .gauge_style(Style::new().light_green().on_black().bold())
            .label(format!("{}: {}%", task.title, task.progress))
            .percent(task.progress);
        frame.render_widget(progress, progress_area);

        frame.render_widget(
            Line::from("j/k or arrows: select • q: quit".dark_gray()).centered(),
            key_bindings_area,
        );
    }

    fn publish(&self, molerat: &mut Molerat<Message>) {
        molerat.publish(|ui| {
            ui.list_box("tasks", "Agent task queue", |ui| {
                for (index, task) in TASKS.iter().enumerate() {
                    ui.list_box_option(index.to_string(), task.title)
                        .description(format!(
                            "{} Progress: {} percent.",
                            task.description, task.progress
                        ))
                        .selected(index == self.selected)
                        .focused(index == self.selected)
                        .on_focus(move || Message::Select(index))
                        .on_click(move || Message::Select(index));
                }
            });
            let task = &TASKS[self.selected];
            ui.paragraph("details", task.description)
                .label(format!("Details for {}", task.title));
            ui.progress(
                "progress",
                "Selected task progress",
                f64::from(task.progress),
                0.0,
                100.0,
            );
            ui.status("mcp", "MCP control endpoint ready");
            ui.text(
                "key-bindings",
                "Keyboard shortcuts: J or Down Arrow selects the next task; K or Up Arrow selects the previous task; Q or Escape quits.",
            );
        });
    }
}

fn panel(title: &'static str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .dark_gray()
        .title(title.white())
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::default();
    let mut molerat = Molerat::<Message>::new_mcp("Agent task dashboard")?;
    app.publish(&mut molerat);

    loop {
        terminal.draw(|frame| app.render(frame))?;
        let mut changed = false;
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.is_press()
        {
            changed = match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    app.selected = (app.selected + 1) % TASKS.len();
                    true
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.selected = app.selected.checked_sub(1).unwrap_or(TASKS.len() - 1);
                    true
                }
                KeyCode::Char('q') | KeyCode::Esc => break,
                _ => false,
            };
        }
        for Message::Select(index) in molerat.messages() {
            app.selected = index;
            changed = true;
        }
        if changed {
            app.publish(&mut molerat);
        }
    }

    Ok(())
}

fn main() -> io::Result<()> {
    ratatui::run(run)
}
