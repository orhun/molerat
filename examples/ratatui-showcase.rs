//! Molerat showcase exposed through AT-SPI and MCP at the same time.

use molerat::Molerat;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::{
        event::{self, DisableFocusChange, EnableFocusChange, Event, KeyCode, KeyEvent},
        execute,
    },
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols,
    text::Line,
    widgets::{
        Block, BorderType, Gauge, List, ListItem, Padding, Paragraph, Wrap,
        canvas::{Canvas, Circle, Line as CanvasLine, Points},
    },
};
use std::{io, time::Duration};

const PINK: Color = Color::Rgb(232, 170, 164);
const PINK_LIGHT: Color = Color::Rgb(255, 205, 199);
const CHEESE: Color = Color::Rgb(255, 222, 102);
const MINT: Color = Color::Rgb(128, 224, 176);
const STEP: f64 = 5.0;

#[derive(Clone, Copy, Debug)]
struct Cheese {
    name: &'static str,
    position: (f64, f64),
}

const CHEESES: [Cheese; 3] = [
    Cheese {
        name: "Brie",
        position: (22.0, 28.0),
    },
    Cheese {
        name: "Gouda",
        position: (78.0, 28.0),
    },
    Cheese {
        name: "Cheddar",
        position: (72.0, 78.0),
    },
];

#[derive(Clone, Copy, Debug)]
enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug)]
enum Message {
    Move(Direction),
    Chase,
    Reset,
    Quit,
}

#[derive(Debug)]
struct App {
    rat: (f64, f64),
    trail: Vec<(f64, f64)>,
    collected: [bool; CHEESES.len()],
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let rat = (14.0, 78.0);
        Self {
            rat,
            trail: vec![rat],
            collected: [false; CHEESES.len()],
            should_quit: false,
        }
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let [header, content] = frame.area().layout(&Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
        ]));
        let [body, progress, footer] = content.layout(
            &Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .spacing(1),
        );
        let [canvas, sidebar] = body.layout(
            &Layout::horizontal([Constraint::Percentage(68), Constraint::Fill(1)]).spacing(1),
        );

        frame.render_widget(Line::from(" MOLERAT ").fg(PINK).bold(), header);
        self.render_canvas(frame, canvas);
        self.render_sidebar(frame, sidebar);

        let found = self.found();
        frame.render_widget(
            Gauge::default()
                .block(panel(" Mission progress "))
                .gauge_style(Style::new().fg(CHEESE).bg(Color::Black).bold())
                .label(format!("{found}/{} cheese collected", CHEESES.len()))
                .ratio(found as f64 / CHEESES.len() as f64),
            progress,
        );
        frame.render_widget(
            Line::from(vec![
                "Arrows".fg(PINK),
                " move  •  ".white().italic(),
                "Space".fg(CHEESE),
                " chase  •  ".white().italic(),
                "R".fg(MINT),
                " reset  •  Esc quit".white().italic(),
            ])
            .centered(),
            footer,
        );
    }

    fn render_canvas(&self, frame: &mut Frame<'_>, area: Rect) {
        let canvas = Canvas::default()
            .block(
                panel(" Accessible canvas ")
                    .padding(Padding::horizontal(1))
                    .title_bottom(
                        Line::from(format!(" rat at {:.0}, {:.0} ", self.rat.0, self.rat.1))
                            .white()
                            .right_aligned(),
                    ),
            )
            .marker(symbols::Marker::Braille)
            .x_bounds([0.0, 100.0])
            .y_bounds([0.0, 100.0])
            .paint(|context| {
                for points in self.trail.windows(2) {
                    context.draw(&CanvasLine {
                        x1: points[0].0,
                        y1: points[0].1,
                        x2: points[1].0,
                        y2: points[1].1,
                        color: PINK,
                    });
                }
                for (index, cheese) in CHEESES.iter().enumerate() {
                    if !self.collected[index] {
                        draw_cheese(context, cheese.position);
                    }
                }
                draw_rat(context, self.rat);
            });
        frame.render_widget(canvas, area);
    }

    fn render_sidebar(&self, frame: &mut Frame<'_>, area: Rect) {
        let [mission, cheese, actions] = area.layout(
            &Layout::vertical([
                Constraint::Length(6),
                Constraint::Length(6),
                Constraint::Fill(1),
            ])
            .spacing(1),
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("AT-SPI + MCP • ready").fg(MINT),
                Line::from("Guide the rat to every snack.").fg(PINK_LIGHT),
                Line::from("Shared by humans and agents.").white(),
            ])
            .block(panel(" Mission ").padding(Padding::horizontal(1))),
            mission,
        );
        frame.render_widget(
            List::new(CHEESES.iter().enumerate().map(|(index, cheese)| {
                let (symbol, color) = if self.collected[index] {
                    ("✓", MINT)
                } else {
                    ("◆", CHEESE)
                };
                ListItem::new(Line::from(vec![
                    format!("{symbol} ").fg(color),
                    cheese.name.white(),
                ]))
            }))
            .block(panel(" Cheese ").padding(Padding::horizontal(1))),
            cheese,
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from("An accessibility & agentic layer").fg(PINK_LIGHT),
                Line::from("for Rust UI applications.").white(),
                Line::default(),
                Line::from("https://github.com/orhun/molerat").fg(CHEESE),
            ])
            .wrap(Wrap { trim: true })
            .block(panel(" About ").padding(Padding::horizontal(1))),
            actions,
        );
    }

    fn publish(&self, molerat: &mut Molerat<Message>) {
        let status = self.status();
        molerat.publish(|ui| {
            ui.status("status", status);
            ui.group("canvas", "Molerat canvas", |ui| {
                ui.text(
                    "rat-position",
                    format!("Rat position: {:.0}, {:.0}", self.rat.0, self.rat.1),
                );
                ui.list("cheese", "Cheese", |ui| {
                    for (index, cheese) in CHEESES.iter().enumerate() {
                        let state = if self.collected[index] {
                            "collected"
                        } else {
                            "waiting"
                        };
                        ui.list_item(cheese.name, format!("{}: {state}", cheese.name));
                    }
                });
            })
            .description("A canvas showing the rat, cheese, and the path travelled")
            .focused(true)
            .custom_action("move-left", "Move the rat left", || {
                Message::Move(Direction::Left)
            })
            .custom_action("move-right", "Move the rat right", || {
                Message::Move(Direction::Right)
            })
            .custom_action("move-up", "Move the rat up", || {
                Message::Move(Direction::Up)
            })
            .custom_action("move-down", "Move the rat down", || {
                Message::Move(Direction::Down)
            })
            .custom_action("chase", "Chase the next cheese", || Message::Chase);
            ui.progress(
                "progress",
                "Cheese collected",
                self.found() as f64,
                0.0,
                CHEESES.len() as f64,
            );
            ui.group("actions", "Actions", |ui| {
                ui.button("chase", "Chase next cheese")
                    .on_click(|| Message::Chase);
                ui.button("reset", "Reset the hunt")
                    .on_click(|| Message::Reset);
            });
            ui.paragraph(
                "help",
                "Arrow keys move the rat. Space chases the next cheese. R resets. Escape quits.",
            )
            .label("Keyboard controls");
            ui.command("reset", "Reset the showcase", || Message::Reset);
            ui.command("quit", "Quit the showcase", || Message::Quit);
        });
    }

    fn apply(&mut self, message: Message) {
        match message {
            Message::Move(direction) => {
                match direction {
                    Direction::Left => self.rat.0 = (self.rat.0 - STEP).max(0.0),
                    Direction::Right => self.rat.0 = (self.rat.0 + STEP).min(100.0),
                    Direction::Up => self.rat.1 = (self.rat.1 + STEP).min(100.0),
                    Direction::Down => self.rat.1 = (self.rat.1 - STEP).max(0.0),
                }
                self.trail.push(self.rat);
                self.collect_nearby();
            }
            Message::Chase => {
                if let Some((index, cheese)) = CHEESES
                    .iter()
                    .enumerate()
                    .find(|(index, _)| !self.collected[*index])
                {
                    self.rat = cheese.position;
                    self.trail.push(self.rat);
                    self.collected[index] = true;
                }
            }
            Message::Reset => *self = Self::new(),
            Message::Quit => self.should_quit = true,
        }
    }

    fn key_message(key: KeyEvent) -> Option<Message> {
        match key.code {
            KeyCode::Left => Some(Message::Move(Direction::Left)),
            KeyCode::Right => Some(Message::Move(Direction::Right)),
            KeyCode::Up => Some(Message::Move(Direction::Up)),
            KeyCode::Down => Some(Message::Move(Direction::Down)),
            KeyCode::Char(' ') => Some(Message::Chase),
            KeyCode::Char('r') => Some(Message::Reset),
            KeyCode::Esc | KeyCode::Char('q') => Some(Message::Quit),
            _ => None,
        }
    }

    fn collect_nearby(&mut self) {
        for (index, cheese) in CHEESES.iter().enumerate() {
            let dx = self.rat.0 - cheese.position.0;
            let dy = self.rat.1 - cheese.position.1;
            if dx * dx + dy * dy <= STEP * STEP {
                self.collected[index] = true;
            }
        }
    }

    fn found(&self) -> usize {
        self.collected
            .iter()
            .filter(|collected| **collected)
            .count()
    }

    fn next_cheese(&self) -> Option<&Cheese> {
        CHEESES
            .iter()
            .enumerate()
            .find(|(index, _)| !self.collected[*index])
            .map(|(_, cheese)| cheese)
    }

    fn status(&self) -> String {
        match self.next_cheese() {
            Some(cheese) => format!(
                "{} of {} cheese collected. Next: {} at {:.0}, {:.0}.",
                self.found(),
                CHEESES.len(),
                cheese.name,
                cheese.position.0,
                cheese.position.1
            ),
            None => "All cheese collected. Mission complete!".to_owned(),
        }
    }
}

fn draw_cheese(context: &mut ratatui::widgets::canvas::Context<'_>, (x, y): (f64, f64)) {
    for (x1, y1, x2, y2) in [
        (x - 4.0, y - 3.0, x + 5.0, y - 3.0),
        (x + 5.0, y - 3.0, x - 4.0, y + 4.0),
        (x - 4.0, y + 4.0, x - 4.0, y - 3.0),
    ] {
        context.draw(&CanvasLine {
            x1,
            y1,
            x2,
            y2,
            color: CHEESE,
        });
    }
    context.draw(&Points {
        coords: &[(x - 1.0, y), (x + 2.0, y - 1.5)],
        color: CHEESE,
    });
}

fn draw_rat(context: &mut ratatui::widgets::canvas::Context<'_>, (x, y): (f64, f64)) {
    context.draw(&Circle {
        x,
        y,
        radius: 5.0,
        color: PINK,
    });
    context.draw(&Circle {
        x: x - 3.5,
        y: y + 4.0,
        radius: 2.0,
        color: PINK_LIGHT,
    });
    context.draw(&Circle {
        x: x + 3.5,
        y: y + 4.0,
        radius: 2.0,
        color: PINK_LIGHT,
    });
    context.draw(&Points {
        coords: &[(x - 1.8, y + 1.0), (x + 1.8, y + 1.0), (x, y - 1.5)],
        color: Color::White,
    });
}

fn panel(title: &'static str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(PINK)
        .title(title.white())
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();
    let mut atspi = Molerat::<Message>::new_atspi("Molerat");
    let mut mcp = Molerat::<Message>::new_mcp("Molerat")?;
    app.publish(&mut atspi);
    app.publish(&mut mcp);

    while !app.should_quit {
        terminal.draw(|frame| app.render(frame))?;
        let mut changed = false;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.is_press() || key.is_repeat() => {
                    if let Some(message) = App::key_message(key) {
                        app.apply(message);
                        changed = true;
                    }
                }
                Event::FocusGained => atspi.set_window_focused(true),
                Event::FocusLost => atspi.set_window_focused(false),
                _ => {}
            }
        }
        for message in atspi.messages() {
            app.apply(message);
            changed = true;
        }
        for message in mcp.messages() {
            app.apply(message);
            changed = true;
        }
        if changed && !app.should_quit {
            app.publish(&mut atspi);
            app.publish(&mut mcp);
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    execute!(io::stdout(), EnableFocusChange)?;
    let result = ratatui::run(run);
    execute!(io::stdout(), DisableFocusChange)?;
    result
}
