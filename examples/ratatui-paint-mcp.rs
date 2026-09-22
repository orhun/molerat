//! Interactive Ratatui canvas exposed over MCP through Molerat.

use molerat::Molerat;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::{
        event::{
            self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
            MouseButton, MouseEvent, MouseEventKind,
        },
        execute,
    },
    layout::{Constraint, Layout, Margin, Position, Rect},
    style::{Color, Style, Stylize},
    symbols,
    text::Line,
    widgets::{
        Block, BorderType, List, ListItem, ListState, Padding,
        canvas::{Canvas, Circle, Line as CanvasLine, Points},
    },
};
use std::{io, time::Duration};

const STEP: f64 = 5.0;
const ERASER_RADIUS: f64 = 7.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Brush {
    Dot,
    Round,
    Horizontal,
    Vertical,
    Eraser,
}

impl Brush {
    const ALL: [Self; 5] = [
        Self::Dot,
        Self::Round,
        Self::Horizontal,
        Self::Vertical,
        Self::Eraser,
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::Dot => "dot",
            Self::Round => "round",
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
            Self::Eraser => "eraser",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Dot => "Dot",
            Self::Round => "Round",
            Self::Horizontal => "Horizontal line",
            Self::Vertical => "Vertical line",
            Self::Eraser => "Eraser",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaintColor {
    White,
    Red,
    Yellow,
    Green,
    Cyan,
    Magenta,
}

impl PaintColor {
    const ALL: [Self; 6] = [
        Self::White,
        Self::Red,
        Self::Yellow,
        Self::Green,
        Self::Cyan,
        Self::Magenta,
    ];

    const fn key(self) -> &'static str {
        match self {
            Self::White => "white",
            Self::Red => "red",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Cyan => "cyan",
            Self::Magenta => "magenta",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::White => "White",
            Self::Red => "Red",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Cyan => "Cyan",
            Self::Magenta => "Magenta",
        }
    }

    const fn color(self) -> Color {
        match self {
            Self::White => Color::White,
            Self::Red => Color::LightRed,
            Self::Yellow => Color::Yellow,
            Self::Green => Color::LightGreen,
            Self::Cyan => Color::LightCyan,
            Self::Magenta => Color::LightMagenta,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Stroke {
    x: f64,
    y: f64,
    brush: Brush,
    color: PaintColor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Brushes,
    Colors,
    Canvas,
}

#[derive(Clone, Copy, Debug)]
enum Direction {
    Left,
    Right,
    Up,
    Down,
}

// Keyboard input and MCP actions converge on the same application messages.
#[derive(Clone, Copy, Debug)]
enum Message {
    SelectBrush(Brush),
    SelectColor(PaintColor),
    FocusCanvas,
    ToggleFocus,
    Move(Direction),
    MoveTo(f64, f64),
    Paint,
    PaintAt(f64, f64),
    Undo,
    Clear,
    Quit,
}

struct App {
    brush: Brush,
    color: PaintColor,
    focus: Focus,
    cursor: (f64, f64),
    strokes: Vec<Stroke>,
    should_quit: bool,
}

#[derive(Clone, Copy, Debug)]
struct Areas {
    header: Rect,
    brushes: Rect,
    colors: Rect,
    canvas: Rect,
    footer: Rect,
}

impl Default for App {
    fn default() -> Self {
        Self {
            brush: Brush::Dot,
            color: PaintColor::Yellow,
            focus: Focus::Canvas,
            cursor: (50.0, 50.0),
            strokes: Vec::new(),
            should_quit: false,
        }
    }
}

impl App {
    fn render(&self, frame: &mut Frame<'_>) {
        let areas = drawing_areas(frame.area());
        let tool = if self.brush == Brush::Eraser {
            format!("• {} ", self.brush.label()).white()
        } else {
            format!("• {} brush • ", self.brush.label()).white()
        };
        frame.render_widget(
            Line::from(vec![
                " Molerat Paint ".yellow().bold(),
                tool,
                if self.brush == Brush::Eraser {
                    "".into()
                } else {
                    self.color.label().fg(self.color.color())
                },
                format!(" • {} strokes", self.strokes.len()).white(),
            ]),
            areas.header,
        );

        let brushes = List::new(
            Brush::ALL
                .into_iter()
                .map(|brush| ListItem::new(brush.label()).white()),
        )
        .block(tool_block(" Brushes ", self.focus == Focus::Brushes))
        .highlight_style(Style::new().yellow().bold())
        .highlight_symbol("▸ ");
        let mut state = ListState::default().with_selected(Some(self.brush as usize));
        frame.render_stateful_widget(brushes, areas.brushes, &mut state);

        let colors = List::new(PaintColor::ALL.into_iter().map(|color| {
            ListItem::new(Line::from(vec![
                "● ".fg(color.color()),
                color.label().white(),
            ]))
        }))
        .block(tool_block(" Colors ", self.focus == Focus::Colors))
        .highlight_style(Style::new().bold())
        .highlight_symbol("▸ ");
        let mut state = ListState::default().with_selected(Some(self.color as usize));
        frame.render_stateful_widget(colors, areas.colors, &mut state);

        let canvas = Canvas::default()
            .block(
                tool_block(" Canvas ", self.focus == Focus::Canvas)
                    .padding(Padding::horizontal(1))
                    .title_bottom(
                        Line::from(format!(
                            " {}, {} • drag draw/erase • right-click undo ",
                            self.cursor.0, self.cursor.1
                        ))
                        .white()
                        .right_aligned(),
                    ),
            )
            .marker(symbols::Marker::Braille)
            .x_bounds([0.0, 100.0])
            .y_bounds([0.0, 100.0])
            .paint(|context| {
                for stroke in &self.strokes {
                    draw_stroke(context, *stroke);
                }
                let y = 100.0 - self.cursor.1;
                if self.brush == Brush::Eraser {
                    context.draw(&Circle {
                        x: self.cursor.0,
                        y,
                        radius: ERASER_RADIUS,
                        color: Color::LightRed,
                    });
                } else {
                    context.draw(&CanvasLine {
                        x1: self.cursor.0 - 1.5,
                        y1: y,
                        x2: self.cursor.0 + 1.5,
                        y2: y,
                        color: Color::White,
                    });
                    context.draw(&CanvasLine {
                        x1: self.cursor.0,
                        y1: y - 3.0,
                        x2: self.cursor.0,
                        y2: y + 3.0,
                        color: Color::White,
                    });
                }
            });
        frame.render_widget(canvas, areas.canvas);

        let controls = match self.focus {
            Focus::Brushes | Focus::Colors => "↑/↓ or mouse select",
            Focus::Canvas => "Arrows move • Space/drag draw or erase • U/right-click undo",
        };
        frame.render_widget(
            Line::from(vec![
                "Tab".green(),
                " focus • ".white().italic(),
                controls.green(),
                " • Esc quit".white().italic(),
            ])
            .centered(),
            areas.footer,
        );
    }

    fn publish(&self, molerat: &mut Molerat<Message>) {
        molerat.publish(|ui| {
            ui.list_box("brushes", "Brushes", |ui| {
                for brush in Brush::ALL {
                    ui.list_box_option(brush.key(), brush.label())
                        .selected(self.brush == brush)
                        .focused(self.focus == Focus::Brushes && self.brush == brush)
                        .on_focus(move || Message::SelectBrush(brush))
                        .on_click(move || Message::SelectBrush(brush));
                }
            });
            ui.list_box("colors", "Colors", |ui| {
                for color in PaintColor::ALL {
                    ui.list_box_option(color.key(), color.label())
                        .selected(self.color == color)
                        .focused(self.focus == Focus::Colors && self.color == color)
                        .on_focus(move || Message::SelectColor(color))
                        .on_click(move || Message::SelectColor(color));
                }
            });
            let tool = if self.brush == Brush::Eraser {
                self.brush.label().to_owned()
            } else {
                format!("{} brush in {}", self.brush.label(), self.color.label())
            };
            ui.group("canvas", "Drawing canvas", |ui| {
                ui.text(
                    "cursor",
                    format!("Cursor at {}, {}", self.cursor.0, self.cursor.1),
                );
                ui.text("tool", tool);
                ui.list("strokes", "Painted strokes", |ui| {
                    for (index, stroke) in self.strokes.iter().enumerate() {
                        ui.list_item(
                            index.to_string(),
                            format!(
                                "{} {} at {}, {}",
                                stroke.color.label(),
                                stroke.brush.label(),
                                stroke.x,
                                stroke.y
                            ),
                        );
                    }
                });
            })
            .focused(self.focus == Focus::Canvas)
            .on_focus(|| Message::FocusCanvas)
            .custom_action("move-left", "Move cursor left", || {
                Message::Move(Direction::Left)
            })
            .custom_action("move-right", "Move cursor right", || {
                Message::Move(Direction::Right)
            })
            .custom_action("move-up", "Move cursor up", || Message::Move(Direction::Up))
            .custom_action("move-down", "Move cursor down", || {
                Message::Move(Direction::Down)
            })
            .custom_action("paint", "Apply the selected brush at the cursor", || {
                Message::Paint
            })
            .custom_action("undo", "Undo the last stroke", || Message::Undo)
            .custom_action("clear", "Clear the canvas", || Message::Clear);
            ui.command("switch-focus", "Switch focused widget (Tab)", || {
                Message::ToggleFocus
            });
            ui.command("quit", "Quit the drawing application (Esc)", || {
                Message::Quit
            });
            ui.status(
                "drawing-status",
                format!(
                    "{} strokes; {} brush; {} color; cursor at {}, {}",
                    self.strokes.len(),
                    self.brush.label(),
                    self.color.label(),
                    self.cursor.0,
                    self.cursor.1
                ),
            );
        });
    }

    fn apply_message(&mut self, message: Message) {
        match message {
            Message::SelectBrush(brush) => {
                self.brush = brush;
                self.focus = Focus::Brushes;
            }
            Message::SelectColor(color) => {
                self.color = color;
                self.focus = Focus::Colors;
            }
            Message::FocusCanvas => self.focus = Focus::Canvas,
            Message::ToggleFocus => {
                self.focus = match self.focus {
                    Focus::Brushes => Focus::Colors,
                    Focus::Colors => Focus::Canvas,
                    Focus::Canvas => Focus::Brushes,
                };
            }
            Message::Move(direction) => self.move_cursor(direction),
            Message::MoveTo(x, y) => self.cursor = (x, y),
            Message::Paint => self.paint(),
            Message::PaintAt(x, y) => {
                self.cursor = (x, y);
                self.paint();
            }
            Message::Undo => {
                self.strokes.pop();
            }
            Message::Clear => self.strokes.clear(),
            Message::Quit => self.should_quit = true,
        }
    }

    fn paint(&mut self) {
        if self.brush == Brush::Eraser {
            self.strokes.retain(|stroke| {
                let x = stroke.x - self.cursor.0;
                let y = stroke.y - self.cursor.1;
                x * x + y * y > ERASER_RADIUS * ERASER_RADIUS
            });
            return;
        }
        self.strokes.push(Stroke {
            x: self.cursor.0,
            y: self.cursor.1,
            brush: self.brush,
            color: self.color,
        });
    }

    fn move_cursor(&mut self, direction: Direction) {
        match direction {
            Direction::Left => self.cursor.0 = (self.cursor.0 - STEP).max(0.0),
            Direction::Right => self.cursor.0 = (self.cursor.0 + STEP).min(100.0),
            Direction::Up => self.cursor.1 = (self.cursor.1 - STEP).max(0.0),
            Direction::Down => self.cursor.1 = (self.cursor.1 + STEP).min(100.0),
        }
    }

    fn key_message(&self, key: KeyEvent) -> Option<Message> {
        match key.code {
            KeyCode::Esc => Some(Message::Quit),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Message::Quit)
            }
            KeyCode::Tab | KeyCode::BackTab => Some(Message::ToggleFocus),
            KeyCode::Up if self.focus == Focus::Brushes => {
                Some(Message::SelectBrush(cycle(self.brush, Brush::ALL, false)))
            }
            KeyCode::Down if self.focus == Focus::Brushes => {
                Some(Message::SelectBrush(cycle(self.brush, Brush::ALL, true)))
            }
            KeyCode::Up if self.focus == Focus::Colors => Some(Message::SelectColor(cycle(
                self.color,
                PaintColor::ALL,
                false,
            ))),
            KeyCode::Down if self.focus == Focus::Colors => Some(Message::SelectColor(cycle(
                self.color,
                PaintColor::ALL,
                true,
            ))),
            KeyCode::Left if self.focus == Focus::Canvas => Some(Message::Move(Direction::Left)),
            KeyCode::Right if self.focus == Focus::Canvas => Some(Message::Move(Direction::Right)),
            KeyCode::Up if self.focus == Focus::Canvas => Some(Message::Move(Direction::Up)),
            KeyCode::Down if self.focus == Focus::Canvas => Some(Message::Move(Direction::Down)),
            KeyCode::Char(' ') if self.focus == Focus::Canvas => Some(Message::Paint),
            KeyCode::Char('u') if self.focus == Focus::Canvas => Some(Message::Undo),
            KeyCode::Char('c') if self.focus == Focus::Canvas => Some(Message::Clear),
            _ => None,
        }
    }

    fn mouse_message(&self, mouse: MouseEvent, area: Rect) -> Option<Message> {
        let areas = drawing_areas(area);
        let position = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) if areas.brushes.contains(position) => {
                list_index(areas.brushes, mouse.row, Brush::ALL.len())
                    .map(|index| Message::SelectBrush(Brush::ALL[index]))
            }
            MouseEventKind::Down(MouseButton::Left) if areas.colors.contains(position) => {
                list_index(areas.colors, mouse.row, PaintColor::ALL.len())
                    .map(|index| Message::SelectColor(PaintColor::ALL[index]))
            }
            MouseEventKind::ScrollUp if areas.brushes.contains(position) => {
                Some(Message::SelectBrush(cycle(self.brush, Brush::ALL, false)))
            }
            MouseEventKind::ScrollDown if areas.brushes.contains(position) => {
                Some(Message::SelectBrush(cycle(self.brush, Brush::ALL, true)))
            }
            MouseEventKind::ScrollUp if areas.colors.contains(position) => Some(
                Message::SelectColor(cycle(self.color, PaintColor::ALL, false)),
            ),
            MouseEventKind::ScrollDown if areas.colors.contains(position) => Some(
                Message::SelectColor(cycle(self.color, PaintColor::ALL, true)),
            ),
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
                if areas.canvas.contains(position) =>
            {
                canvas_position(areas.canvas, mouse).map(|(x, y)| Message::PaintAt(x, y))
            }
            MouseEventKind::Moved if areas.canvas.contains(position) => {
                canvas_position(areas.canvas, mouse).map(|(x, y)| Message::MoveTo(x, y))
            }
            MouseEventKind::Down(MouseButton::Right) if areas.canvas.contains(position) => {
                Some(Message::Undo)
            }
            _ => None,
        }
    }
}

fn drawing_areas(area: Rect) -> Areas {
    let [header, body, footer] = area.layout(
        &Layout::vertical([
            Constraint::Length(1),
            Constraint::Fill(1),
            Constraint::Length(1),
        ])
        .spacing(1),
    );
    let [tools, canvas] =
        body.layout(&Layout::horizontal([Constraint::Length(24), Constraint::Fill(1)]).spacing(1));
    let [brushes, colors] =
        tools.layout(&Layout::vertical([Constraint::Length(8), Constraint::Fill(1)]).spacing(1));
    Areas {
        header,
        brushes,
        colors,
        canvas,
        footer,
    }
}

fn list_index(area: Rect, row: u16, length: usize) -> Option<usize> {
    let inner = area.inner(Margin {
        horizontal: 1,
        vertical: 1,
    });
    let index = usize::from(row.saturating_sub(inner.y));
    (inner.contains(Position::new(inner.x, row)) && index < length).then_some(index)
}

fn canvas_position(area: Rect, mouse: MouseEvent) -> Option<(f64, f64)> {
    let plot = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    if !plot.contains(Position::new(mouse.column, mouse.row)) {
        return None;
    }
    let width = f64::from(plot.width.saturating_sub(1).max(1));
    let height = f64::from(plot.height.saturating_sub(1).max(1));
    let x = f64::from(mouse.column.saturating_sub(plot.x)) / width * 100.0;
    let y = f64::from(mouse.row.saturating_sub(plot.y)) / height * 100.0;
    Some((x.round(), y.round()))
}

fn cycle<T: Copy + PartialEq, const N: usize>(selected: T, choices: [T; N], forward: bool) -> T {
    let index = choices
        .iter()
        .position(|choice| *choice == selected)
        .map_or(0, |index| index);
    let index = if forward {
        (index + 1) % N
    } else {
        index.checked_sub(1).unwrap_or(N - 1)
    };
    choices[index]
}

fn draw_stroke(context: &mut ratatui::widgets::canvas::Context<'_>, stroke: Stroke) {
    let color = stroke.color.color();
    let y = 100.0 - stroke.y;
    match stroke.brush {
        Brush::Dot => context.draw(&Points {
            coords: &[(stroke.x, y)],
            color,
        }),
        Brush::Round => context.draw(&Circle {
            x: stroke.x,
            y,
            radius: 4.0,
            color,
        }),
        Brush::Horizontal => context.draw(&CanvasLine {
            x1: stroke.x - 6.0,
            y1: y,
            x2: stroke.x + 6.0,
            y2: y,
            color,
        }),
        Brush::Vertical => context.draw(&CanvasLine {
            x1: stroke.x,
            y1: y - 10.0,
            x2: stroke.x,
            y2: y + 10.0,
            color,
        }),
        Brush::Eraser => {}
    }
}

fn tool_block(title: &'static str, focused: bool) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(if focused {
            Color::Yellow
        } else {
            Color::DarkGray
        })
        .title(title.white())
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::default();
    let mut molerat = Molerat::<Message>::new_mcp("Molerat Paint")?;
    app.publish(&mut molerat);

    while !app.should_quit {
        terminal.draw(|frame| app.render(frame))?;
        let mut changed = false;
        if event::poll(Duration::from_millis(100))? {
            let message = match event::read()? {
                Event::Key(key) if key.is_press() || key.is_repeat() => app.key_message(key),
                Event::Mouse(mouse) => {
                    let size = terminal.size()?;
                    app.mouse_message(mouse, Rect::new(0, 0, size.width, size.height))
                }
                _ => None,
            };
            if let Some(message) = message {
                app.apply_message(message);
                changed = true;
            }
        }
        for message in molerat.messages() {
            app.apply_message(message);
            changed = true;
        }
        if changed && !app.should_quit {
            app.publish(&mut molerat);
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    ratatui::run(|terminal| {
        execute!(terminal.backend_mut(), EnableMouseCapture)?;
        let result = run(terminal);
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
        result
    })
}
