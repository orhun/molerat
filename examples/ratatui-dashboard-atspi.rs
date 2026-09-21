//! System monitoring dashboard exposed to AT-SPI through Molerat.

use molerat::{Molerat, accesskit::Action};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, KeyCode},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Style, Stylize},
    text::Line,
    widgets::{
        Bar, BarChart, BarGroup, Block, BorderType, Gauge, List, ListItem, Padding, Paragraph, Row,
        Sparkline, Table, TableState, Wrap,
    },
};
use std::{
    io,
    time::{Duration, Instant},
};

const CPU_HISTORY: [u64; 24] = [
    24, 31, 28, 42, 47, 39, 35, 52, 61, 58, 44, 37, 49, 55, 68, 63, 51, 46, 41, 33, 38, 45, 53, 48,
];
const MEMORY_HISTORY: [u64; 24] = [
    54, 55, 55, 56, 57, 57, 58, 59, 58, 60, 61, 61, 62, 63, 62, 62, 64, 65, 64, 63, 64, 65, 66, 65,
];
const NETWORK_HISTORY: [u64; 24] = [
    8, 12, 9, 18, 35, 28, 22, 41, 38, 19, 14, 26, 52, 64, 47, 31, 25, 18, 39, 44, 29, 21, 16, 24,
];

const PROCESSES: &[Process] = &[
    Process::new(4218, "molerat", "orhun", 18, 142, "running"),
    Process::new(1932, "wayland", "orhun", 11, 284, "sleeping"),
    Process::new(864, "pipewire", "orhun", 7, 96, "running"),
    Process::new(5280, "rust-analyzer", "orhun", 6, 612, "sleeping"),
    Process::new(722, "systemd-journal", "root", 3, 74, "sleeping"),
    Process::new(6114, "cargo", "orhun", 2, 188, "sleeping"),
];

const FILE_SYSTEMS: &[FileSystem] = &[
    FileSystem::new("root", "/", 68, "326 of 480 gigabytes used"),
    FileSystem::new("home", "/home", 43, "412 of 960 gigabytes used"),
    FileSystem::new("boot", "/boot", 21, "214 of 1024 megabytes used"),
];

const ALERTS: &[(&str, &str)] = &[
    ("Warning", "CPU temperature reached 78 degrees Celsius"),
    ("Info", "System packages were updated 12 minutes ago"),
    ("Info", "Backup completed successfully at 02:15"),
];

#[derive(Debug)]
enum Message {
    SelectProcess(usize),
}

#[derive(Clone, Copy, Debug)]
struct Process {
    pid: u32,
    name: &'static str,
    user: &'static str,
    cpu: u16,
    memory: u16,
    status: &'static str,
}

impl Process {
    const fn new(
        pid: u32,
        name: &'static str,
        user: &'static str,
        cpu: u16,
        memory: u16,
        status: &'static str,
    ) -> Self {
        Self {
            pid,
            name,
            user,
            cpu,
            memory,
            status,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct FileSystem {
    name: &'static str,
    mount: &'static str,
    used: u16,
    description: &'static str,
}

impl FileSystem {
    const fn new(
        name: &'static str,
        mount: &'static str,
        used: u16,
        description: &'static str,
    ) -> Self {
        Self {
            name,
            mount,
            used,
            description,
        }
    }
}

#[derive(Debug, Default)]
struct App {
    selected_process: usize,
    sample: usize,
    paused: bool,
}

impl App {
    fn cpu(&self) -> u16 {
        CPU_HISTORY[self.sample % CPU_HISTORY.len()] as u16
    }

    fn memory(&self) -> u16 {
        MEMORY_HISTORY[self.sample % MEMORY_HISTORY.len()] as u16
    }

    fn network(&self) -> u16 {
        NETWORK_HISTORY[self.sample % NETWORK_HISTORY.len()] as u16
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let [summary_area, dashboard_area, key_bindings_area] =
            frame.area().layout(&Layout::vertical([
                Constraint::Length(4),
                Constraint::Fill(1),
                Constraint::Length(1),
            ]));
        let [left, right] = dashboard_area.layout(
            &Layout::horizontal([Constraint::Percentage(58), Constraint::Fill(1)]).spacing(1),
        );
        let [resources_area, processes_area] =
            left.layout(&Layout::vertical([Constraint::Length(9), Constraint::Fill(1)]).spacing(1));
        let [history_area, disks_area, alerts_area] = right.layout(
            &Layout::vertical([
                Constraint::Fill(1),
                Constraint::Length(9),
                Constraint::Length(7),
            ])
            .spacing(1),
        );

        render_summary(frame, summary_area, self.paused);
        render_resources(frame, resources_area, self.cpu(), self.memory());
        render_processes(frame, processes_area, self.selected_process);
        render_history(frame, history_area, self.sample, self.cpu(), self.network());
        render_disks(frame, disks_area);
        render_alerts(frame, alerts_area);
        render_key_bindings(frame, key_bindings_area, self.paused);
    }

    fn publish(&self, molerat: &mut Molerat<Message>) {
        let cpu = self.cpu();
        let memory = self.memory();
        let network = self.network();

        molerat.publish(|ui| {
            ui.group("host", "Host summary", |ui| {
                ui.text("operating-system", "Operating system: Arch Linux");
                ui.text("uptime", "Uptime: 8 days, 4 hours, 17 minutes");
                ui.text(
                    "update-status",
                    if self.paused {
                        "Metric updates are paused"
                    } else {
                        "Metric updates are live"
                    },
                );
            });

            ui.group("resources", "Resource usage", |ui| {
                ui.progress("cpu", "CPU usage", f64::from(cpu), 0.0, 100.0)
                    .description(format!(
                        "{cpu} percent, {} over the recent sample window",
                        trend(&CPU_HISTORY, self.sample)
                    ));
                ui.progress("memory", "Memory usage", f64::from(memory), 0.0, 100.0)
                    .description(format!(
                        "{memory} percent, 10.4 of 16 gigabytes used, {} over the recent sample window",
                        trend(&MEMORY_HISTORY, self.sample)
                    ));
                ui.progress("swap", "Swap usage", 12.0, 0.0, 100.0)
                    .description("12 percent, 1 of 8 gigabytes used");
                ui.progress(
                    "network",
                    "Network receive rate",
                    f64::from(network),
                    0.0,
                    100.0,
                )
                .description(format!("{network} megabytes per second"));
            });

            ui.list_box("processes", "Processes by CPU usage", |ui| {
                for (index, process) in PROCESSES.iter().enumerate() {
                    ui.list_box_option(process.pid.to_string(), process.name)
                        .description(format!(
                            "Process ID {}, user {}, CPU {} percent, memory {} megabytes, status {}",
                            process.pid,
                            process.user,
                            process.cpu,
                            process.memory,
                            process.status
                        ))
                        .selected(index == self.selected_process)
                        .focused(index == self.selected_process)
                        .on_action(Action::Focus, move |_| Message::SelectProcess(index))
                        .on_action(Action::Click, move |_| Message::SelectProcess(index));
                }
            })
            .description(
                "Use J or Down Arrow for the next process and K or Up Arrow for the previous process",
            );

            ui.group("filesystems", "Filesystem usage", |ui| {
                for file_system in FILE_SYSTEMS {
                    ui.progress(
                        file_system.name,
                        format!("{} mounted at {}", file_system.name, file_system.mount),
                        f64::from(file_system.used),
                        0.0,
                        100.0,
                    )
                    .description(file_system.description);
                }
            });

            ui.list("alerts", "Recent alerts", |ui| {
                for (index, (severity, message)) in ALERTS.iter().enumerate() {
                    ui.list_item(index.to_string(), format!("{severity}: {message}"));
                }
            });

            ui.text(
                "key-bindings",
                "Keyboard shortcuts: J or Down Arrow selects the next process; K or Up Arrow selects the previous process; Space pauses or resumes updates; Q or Escape quits.",
            );
        });
    }
}

fn render_summary(frame: &mut Frame<'_>, area: Rect, paused: bool) {
    let status = if paused { "PAUSED" } else { "LIVE" };
    let status = if paused {
        status.yellow()
    } else {
        status.green()
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            "Arch Linux  •  uptime 8d 4h 17m  •  load 1.21 0.98 0.76  •  ".dark_gray(),
            status,
        ]))
        .block(panel(" System monitor ").padding(Padding::horizontal(1)))
        .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_resources(frame: &mut Frame<'_>, area: Rect, cpu: u16, memory: u16) {
    let [cpu_area, memory_area, swap_area] =
        area.layout(&Layout::vertical([Constraint::Length(3); 3]));
    frame.render_widget(
        Gauge::default()
            .block(panel(" CPU "))
            .gauge_style(Style::new().light_green().on_black())
            .label(format!("{cpu}%"))
            .percent(cpu),
        cpu_area,
    );
    frame.render_widget(
        Gauge::default()
            .block(panel(" Memory "))
            .gauge_style(Style::new().light_blue().on_black())
            .label(format!("{memory}% • 10.4 / 16 GiB"))
            .percent(memory),
        memory_area,
    );
    frame.render_widget(
        Gauge::default()
            .block(panel(" Swap "))
            .gauge_style(Style::new().light_magenta().on_black())
            .label("12% • 1 / 8 GiB")
            .percent(12),
        swap_area,
    );
}

fn render_processes(frame: &mut Frame<'_>, area: Rect, selected: usize) {
    let rows = PROCESSES.iter().map(|process| {
        Row::new([
            process.pid.to_string(),
            process.name.to_owned(),
            process.user.to_owned(),
            format!("{}%", process.cpu),
            format!("{} MiB", process.memory),
        ])
        .reset()
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Fill(1),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(9),
        ],
    )
    .header(Row::new(["PID", "Name", "User", "CPU", "Memory"]).green())
    .block(panel(" Processes ").padding(Padding::horizontal(1)))
    .row_highlight_style(Style::new().yellow().bold())
    .highlight_symbol("▸ ");
    let mut state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_history(frame: &mut Frame<'_>, area: Rect, sample: usize, cpu: u16, network: u16) {
    let [cpu_area, network_area] = area.layout(&Layout::vertical([Constraint::Percentage(50); 2]));
    let cpu_history = visible_history(&CPU_HISTORY, sample, cpu_area.width.saturating_sub(2));
    let network_history = visible_history(
        &NETWORK_HISTORY,
        sample,
        network_area.width.saturating_sub(2),
    );

    frame.render_widget(
        Sparkline::default()
            .block(
                panel(" CPU history ")
                    .title_bottom(Line::from(format!(" {cpu}% ")).right_aligned()),
            )
            .data(cpu_history)
            .max(100)
            .light_green(),
        cpu_area,
    );
    frame.render_widget(
        Sparkline::default()
            .block(
                panel(" Network receive ")
                    .title_bottom(Line::from(format!(" {network} MiB/s ")).right_aligned()),
            )
            .data(network_history)
            .max(100)
            .light_cyan(),
        network_area,
    );
}

fn render_disks(frame: &mut Frame<'_>, area: Rect) {
    let bars = FILE_SYSTEMS
        .iter()
        .map(|file_system| {
            Bar::default()
                .value(u64::from(file_system.used))
                .label(Line::from(file_system.name))
                .text_value(format!("{}%", file_system.used))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        BarChart::default()
            .block(panel(" Filesystems ").padding(Padding::horizontal(1)))
            .direction(Direction::Horizontal)
            .data(BarGroup::default().bars(&bars))
            .max(100)
            .bar_gap(1)
            .bar_style(Style::new().light_yellow())
            .value_style(Style::new().black().on_light_yellow()),
        area,
    );
}

fn render_alerts(frame: &mut Frame<'_>, area: Rect) {
    let alerts = ALERTS.iter().map(|(severity, message)| {
        let marker = if *severity == "Warning" { "!" } else { "•" };
        ListItem::new(format!("{marker} {message}"))
    });
    frame.render_widget(
        List::new(alerts)
            .block(panel(" Recent alerts ").padding(Padding::horizontal(1)))
            .white(),
        area,
    );
}

fn render_key_bindings(frame: &mut Frame<'_>, area: Rect, paused: bool) {
    let pause = if paused { "resume" } else { "pause" };
    frame.render_widget(
        Line::from(format!(
            "j/k or arrows: select process • space: {pause} • q: quit"
        ))
        .dark_gray()
        .centered(),
        area,
    );
}

fn visible_history(values: &[u64; 24], sample: usize, width: u16) -> Vec<u64> {
    let length = values.len();
    let current = sample % length;
    let width = usize::from(width);

    (0..width)
        .map(|offset| {
            let distance = width - offset - 1;
            values[(current + length - distance % length) % length]
        })
        .collect()
}

fn trend(values: &[u64], sample: usize) -> &'static str {
    let current = values[sample % values.len()];
    let previous = values[(sample + values.len() - 1) % values.len()];
    match current.cmp(&previous) {
        std::cmp::Ordering::Greater => "rising",
        std::cmp::Ordering::Less => "falling",
        std::cmp::Ordering::Equal => "steady",
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
    let mut molerat = Molerat::<Message>::new_atspi("Molerat System Monitor");
    let mut last_sample = Instant::now();
    app.publish(&mut molerat);

    loop {
        terminal.draw(|frame| app.render(frame))?;

        if event::poll(Duration::from_millis(100))?
            && let Some(key) = event::read()?.as_key_press_event()
        {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    app.selected_process = (app.selected_process + 1) % PROCESSES.len();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    app.selected_process = app
                        .selected_process
                        .checked_sub(1)
                        .unwrap_or(PROCESSES.len() - 1);
                }
                KeyCode::Char(' ') => app.paused = !app.paused,
                KeyCode::Char('q') | KeyCode::Esc => break,
                _ => {}
            }
        }

        if !app.paused && last_sample.elapsed() >= Duration::from_millis(750) {
            app.sample = app.sample.wrapping_add(1);
            last_sample = Instant::now();
        }

        for Message::SelectProcess(index) in molerat.messages() {
            app.selected_process = index;
        }
        app.publish(&mut molerat);
    }

    Ok(())
}

fn main() -> io::Result<()> {
    ratatui::run(run)
}
