//! Ratatouille recipe tracker exposed to AT-SPI through Molerat.

use molerat::Molerat;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::{
        event::{self, DisableFocusChange, EnableFocusChange, Event, KeyCode},
        execute,
    },
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, BorderType, Gauge, List, ListState, Padding, Paragraph, Wrap},
};
use std::{io, time::Duration};

const STEPS: &[(&str, &str, u16)] = &[
    (
        "Prepare the vegetables",
        "Thinly slice the eggplant, zucchini, tomatoes, and peppers.",
        25,
    ),
    (
        "Cook the sauce",
        "Sauté onion and garlic, then add tomatoes and herbs.",
        50,
    ),
    (
        "Arrange the ratatouille",
        "Layer the vegetable slices over the tomato sauce.",
        75,
    ),
    (
        "Bake and serve",
        "Bake until tender, then finish with olive oil and fresh basil.",
        100,
    ),
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
        let [steps_area, details_area] = main_area.layout(&horizontal);
        let (step_title, details, progress) = STEPS[self.selected];

        render_steps(frame, steps_area, self.selected);
        render_instructions(frame, details_area, step_title, details);
        render_progress(frame, progress_area, progress);
        render_key_bindings(frame, key_bindings_area);
    }

    fn publish(&self, molerat: &mut Molerat<Message>) {
        molerat.publish(|ui| {
            ui.list_box("steps", "Ratatouille steps", |ui| {
                for (index, (title, instructions, _)) in STEPS.iter().enumerate() {
                    ui.list_box_option(index.to_string(), *title)
                        .description(*instructions)
                        .selected(index == self.selected)
                        .focused(index == self.selected)
                        .on_focus(move || Message::Select(index))
                        .on_click(move || Message::Select(index));
                }
            })
            .description("Use J or Down Arrow for the next step and K or Up Arrow for the previous step.");
            let (title, details, progress) = STEPS[self.selected];
            ui.paragraph("instructions", details)
                .label(format!("Instructions for {title}"));
            ui.progress(
                "progress",
                "Cooking progress",
                f64::from(progress),
                0.0,
                100.0,
            )
            .description(format!(
                "Step {} of {}: {title}",
                self.selected + 1,
                STEPS.len()
            ));
            ui.text(
                "key-bindings",
                "Keyboard shortcuts: J or Down Arrow selects the next step; K or Up Arrow selects the previous step; Q or Escape quits.",
            );
        });
    }
}

fn render_steps(frame: &mut Frame<'_>, area: Rect, selected: usize) {
    let steps = List::new(STEPS.iter().map(|(title, _, _)| *title))
        .block(panel(" Recipe steps ").padding(Padding::horizontal(1)))
        .white()
        .highlight_style(Style::new().yellow().bold())
        .highlight_symbol("▸ ");
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(steps, area, &mut state);
}

fn render_instructions(frame: &mut Frame<'_>, area: Rect, title: &str, instructions: &str) {
    let instructions = Paragraph::new(vec![
        title.green().into(),
        "".into(),
        instructions.white().italic().into(),
    ])
    .block(panel(" Instructions ").padding(Padding::horizontal(1)))
    .white()
    .wrap(Wrap { trim: true });
    frame.render_widget(instructions, area);
}

fn render_progress(frame: &mut Frame<'_>, area: Rect, progress: u16) {
    let progress = Gauge::default()
        .block(panel(" Progress "))
        .gauge_style(Style::new().light_green().on_black().bold())
        .label(format!("Cooking progress: {progress}%"))
        .percent(progress);
    frame.render_widget(progress, area);
}

fn render_key_bindings(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(
        Line::from("j/k or arrows: select • q: quit".dark_gray()).centered(),
        area,
    );
}

fn panel(title: &'static str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .dark_gray()
        .title(title.white())
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::default();
    let mut molerat = Molerat::<Message>::new_atspi("Ratatouille");
    app.publish(&mut molerat);

    loop {
        terminal.draw(|frame| app.render(frame))?;

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.is_press() => match key.code {
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.selected = (app.selected + 1) % STEPS.len();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.selected = app.selected.checked_sub(1).unwrap_or(STEPS.len() - 1);
                    }
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => {}
                },
                Event::FocusGained => molerat.set_window_focused(true),
                Event::FocusLost => molerat.set_window_focused(false),
                _ => {}
            }
        }

        for Message::Select(index) in molerat.messages() {
            app.selected = index;
        }
        app.publish(&mut molerat);
    }

    Ok(())
}

fn main() -> io::Result<()> {
    execute!(io::stdout(), EnableFocusChange)?;
    let result = ratatui::run(run);
    execute!(io::stdout(), DisableFocusChange)?;
    result
}
