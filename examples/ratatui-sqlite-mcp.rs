//! SQLite database explorer exposed over MCP through Molerat.

use molerat::{
    Molerat,
    accesskit::{Action, ActionData, Role},
};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    layout::{Constraint, Layout, Margin},
    style::{Color, Style, Stylize},
    symbols,
    text::Line,
    widgets::{
        Block, BorderType, List, ListItem, ListState, Padding, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, Tabs, Wrap,
        canvas::{Canvas, Circle, Line as CanvasLine},
    },
};
use ratatui_textarea::TextArea;
use rusqlite::{Connection, types::ValueRef};
use std::{
    f64::consts::TAU,
    io,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
struct TableInfo {
    name: String,
    columns: Vec<String>,
    rows: i64,
}

#[derive(Clone, Debug)]
struct Relation {
    child: usize,
    parent: usize,
    from_column: String,
    to_column: String,
}

#[derive(Debug, Default)]
struct SqlResult {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    query_plan: Vec<String>,
    summary: String,
    elapsed_micros: u128,
}

// Keyboard input and MCP actions converge on the same application messages.
#[derive(Debug)]
enum Message {
    SetSql(Option<String>),
    RunSql,
    ToggleFocus,
    FocusEditor,
    SelectView(View),
    SelectTable(usize),
    PreviewTable(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Tables,
    Editor,
    Views,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    #[default]
    Results,
    Schema,
    QueryPlan,
}

impl View {
    const ALL: [Self; 3] = [Self::Results, Self::Schema, Self::QueryPlan];

    const fn index(self) -> usize {
        match self {
            Self::Results => 0,
            Self::Schema => 1,
            Self::QueryPlan => 2,
        }
    }

    const fn key(self) -> &'static str {
        match self {
            Self::Results => "results",
            Self::Schema => "schema",
            Self::QueryPlan => "query-plan",
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Results => "Results",
            Self::Schema => "Schema",
            Self::QueryPlan => "Query Plan",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InputResult {
    Unchanged,
    Changed,
    Exit,
}

struct App {
    connection: Connection,
    editor: TextArea<'static>,
    result: SqlResult,
    tables: Vec<TableInfo>,
    relations: Vec<Relation>,
    selected_table: usize,
    focus: Focus,
    view: View,
    status: String,
    error: Option<String>,
}

impl App {
    fn new() -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        seed_database(&connection)?;
        let (tables, relations) = read_schema(&connection)?;
        let selected_table = tables
            .iter()
            .position(|table| table.name == "services")
            .map_or(0, |index| index);
        let mut app = Self {
            connection,
            editor: sql_editor("SELECT * FROM services;", true),
            result: SqlResult::default(),
            tables,
            relations,
            selected_table,
            focus: Focus::Editor,
            view: View::Results,
            status: "Ready".to_owned(),
            error: None,
        };
        app.run_sql();
        Ok(app)
    }

    fn render(&self, frame: &mut Frame<'_>) {
        let [header_area, body_area, footer_area] = frame.area().layout(
            &Layout::vertical([
                Constraint::Length(1),
                Constraint::Fill(1),
                Constraint::Length(1),
            ])
            .spacing(1),
        );
        let [browser_area, tabs_area, view_area] = body_area.layout(
            &Layout::vertical([
                Constraint::Length(8),
                Constraint::Length(1),
                Constraint::Fill(1),
            ])
            .spacing(1),
        );
        let [tables_area, editor_area] = browser_area.layout(
            &Layout::horizontal([Constraint::Percentage(28), Constraint::Fill(1)]).spacing(1),
        );

        let state = if self.error.is_some() {
            "ERROR".light_red()
        } else {
            "READY".green()
        };
        frame.render_widget(
            Line::from(vec![
                " Molerat Database Explorer ".yellow().bold(),
                "• SQLite :memory: • ".white(),
                state,
                format!(" • {}", self.status).white(),
            ]),
            header_area,
        );

        let tables =
            List::new(self.tables.iter().map(|table| {
                ListItem::new(format!("{}  ({} rows)", table.name, table.rows)).white()
            }))
            .block(
                panel(" Tables ")
                    .padding(Padding::horizontal(1))
                    .border_style(if self.focus == Focus::Tables {
                        Color::Yellow
                    } else {
                        Color::DarkGray
                    }),
            )
            .highlight_style(Style::new().yellow().bold())
            .highlight_symbol("▸ ");
        let selected = (!self.tables.is_empty()).then_some(self.selected_table);
        let mut state = ListState::default().with_selected(selected);
        frame.render_stateful_widget(tables, tables_area, &mut state);

        frame.render_widget(&self.editor, editor_area);

        frame.render_widget(
            Tabs::new([" Results ", " Schema ", " Query Plan "])
                .select(self.view.index())
                .divider("│".dark_gray())
                .white()
                .highlight_style(if self.focus == Focus::Views {
                    Style::new().black().on_yellow()
                } else {
                    Style::new().yellow()
                }),
            tabs_area,
        );

        match self.view {
            View::Results => {
                if let Some(error) = &self.error {
                    frame.render_widget(
                        Paragraph::new(error.as_str())
                            .light_red()
                            .block(panel(" SQL error ").padding(Padding::horizontal(1)))
                            .wrap(Wrap { trim: true }),
                        view_area,
                    );
                } else {
                    let rows = self
                        .result
                        .rows
                        .iter()
                        .map(|row| Row::new(row.clone()).white());
                    let widths = vec![Constraint::Fill(1); self.result.columns.len()];
                    frame.render_widget(
                        Table::new(rows, widths)
                            .header(Row::new(self.result.columns.clone()).green())
                            .block(
                                panel(" Results ")
                                    .title_bottom(
                                        Line::from(format!(
                                            " {} rows • {} µs ",
                                            self.result.rows.len(),
                                            self.result.elapsed_micros
                                        ))
                                        .white()
                                        .right_aligned(),
                                    )
                                    .padding(Padding::horizontal(1)),
                            )
                            .column_spacing(1),
                        view_area,
                    );
                    let mut scrollbar = ScrollbarState::new(self.result.rows.len());
                    frame.render_stateful_widget(
                        Scrollbar::new(ScrollbarOrientation::VerticalRight)
                            .begin_symbol(None)
                            .end_symbol(None),
                        view_area.inner(Margin {
                            vertical: 1,
                            horizontal: 0,
                        }),
                        &mut scrollbar,
                    );
                }
            }
            View::QueryPlan => {
                let steps = self
                    .result
                    .query_plan
                    .iter()
                    .enumerate()
                    .map(|(index, step)| Line::from(format!("{}. {step}", index + 1)))
                    .collect::<Vec<_>>();
                frame.render_widget(
                    Paragraph::new(steps)
                        .white()
                        .block(panel(" Query Plan ").padding(Padding::horizontal(1)))
                        .wrap(Wrap { trim: true }),
                    view_area,
                );
            }
            View::Schema => {
                let referenced = self.referenced_tables();
                let positions = table_positions(self.tables.len());
                frame.render_widget(
                    Canvas::default()
                        .block(
                            panel(" Schema map ")
                                .title_bottom(
                                    Line::from(vec![
                                        " query ".yellow(),
                                        "• ".dark_gray(),
                                        "selected ".light_cyan(),
                                        "• ".dark_gray(),
                                        "both ".light_green(),
                                    ])
                                    .right_aligned(),
                                )
                                .padding(Padding::horizontal(1)),
                        )
                        .marker(symbols::Marker::Braille)
                        .x_bounds([0.0, 100.0])
                        .y_bounds([0.0, 100.0])
                        .paint(|context| {
                            for relation in &self.relations {
                                let color =
                                    if referenced[relation.child] && referenced[relation.parent] {
                                        Color::Yellow
                                    } else {
                                        Color::DarkGray
                                    };
                                context.draw(&CanvasLine {
                                    x1: positions[relation.child].0,
                                    y1: positions[relation.child].1,
                                    x2: positions[relation.parent].0,
                                    y2: positions[relation.parent].1,
                                    color,
                                });
                            }
                            for (index, table) in self.tables.iter().enumerate() {
                                let (x, y) = positions[index];
                                let color = match (index == self.selected_table, referenced[index])
                                {
                                    (true, true) => Color::LightGreen,
                                    (true, false) => Color::LightCyan,
                                    (false, true) => Color::Yellow,
                                    (false, false) => Color::White,
                                };
                                context.draw(&Circle {
                                    x,
                                    y,
                                    radius: if index == self.selected_table {
                                        4.0
                                    } else {
                                        2.5
                                    },
                                    color,
                                });
                                let width = table.name.chars().count() as f64 * 1.4;
                                let label_x = if x + width + 3.0 > 96.0 {
                                    (x - width - 3.0).max(1.0)
                                } else {
                                    x + 3.0
                                };
                                context.print(label_x, y, table.name.clone().fg(color));
                            }
                        }),
                    view_area,
                );
            }
        }

        let (focus, keys, action) = match self.focus {
            Focus::Tables => ("tables", "↑/↓ + Enter", "select / preview"),
            Focus::Editor => ("editor", "F5/Ctrl-Enter", "run"),
            Focus::Views => ("views", "←/→ or 1-3", "select view"),
        };
        frame.render_widget(
            Line::from(vec![
                format!("Focus: {focus} • ").white(),
                "Tab".green(),
                " focus • ".white().italic(),
                keys.green(),
                format!(" {action} • ").white().italic(),
                "Esc".green(),
                " quit".white().italic(),
            ])
            .centered(),
            footer_area,
        );
    }

    fn publish(&self, molerat: &mut Molerat<Message>) {
        molerat.publish(|ui| {
            ui.list_box("tables", "Database tables", |ui| {
                for (index, table) in self.tables.iter().enumerate() {
                    ui.list_box_option(&table.name, &table.name)
                        .description(format!(
                            "{} rows; columns: {}",
                            table.rows,
                            table.columns.join(", ")
                        ))
                        .selected(index == self.selected_table)
                        .focused(self.focus == Focus::Tables && index == self.selected_table)
                        .on_focus(move || Message::SelectTable(index))
                        .on_click(move || Message::SelectTable(index))
                        .custom_action("preview", "Preview the first 20 rows", move || {
                            Message::PreviewTable(index)
                        });
                }
            });
            ui.text_input("sql", "SQL editor", self.sql())
                .description("SQL to execute against the in-memory SQLite database")
                .focused(self.focus == Focus::Editor)
                .on_action(Action::SetValue, |request| {
                    Message::SetSql(match request.data.as_ref() {
                        Some(ActionData::Value(value)) => Some(value.to_string()),
                        _ => None,
                    })
                })
                .on_focus(|| Message::FocusEditor);
            ui.command(
                "switch-focus",
                "Switch focus between tables, SQL editor, and view tabs (Tab)",
                || Message::ToggleFocus,
            );
            ui.command("run-sql", "Execute the SQL", || Message::RunSql);
            ui.container("views", Role::TabList, |ui| {
                for view in View::ALL {
                    ui.node(view.key(), Role::Tab)
                        .label(view.label())
                        .selected(self.view == view)
                        .focused(self.focus == Focus::Views && self.view == view)
                        .on_focus(move || Message::SelectView(view))
                        .on_click(move || Message::SelectView(view));
                }
            })
            .label("Explorer views");
            ui.container("active-view", Role::TabPanel, |ui| match self.view {
                View::Results => {
                    ui.table("sql-result", "SQL result", |ui| {
                        ui.row("columns", |ui| {
                            for column in &self.result.columns {
                                ui.column_header(column, column);
                            }
                        });
                        for (row_index, row) in self.result.rows.iter().enumerate() {
                            ui.row(row_index.to_string(), |ui| {
                                for (column_index, value) in row.iter().enumerate() {
                                    let key = self
                                        .result
                                        .columns
                                        .get(column_index)
                                        .map_or_else(|| column_index.to_string(), Clone::clone);
                                    ui.cell(key, value);
                                }
                            });
                        }
                    });
                    ui.text(
                        "result-summary",
                        format!(
                            "{} in {} microseconds",
                            self.result.summary, self.result.elapsed_micros
                        ),
                    );
                }
                View::Schema => {
                    let referenced = self.referenced_tables();
                    ui.group("schema-map", "Database relationships", |ui| {
                        for (index, table) in self.tables.iter().enumerate() {
                            let state = match (index == self.selected_table, referenced[index]) {
                                (true, true) => "selected and referenced by the query",
                                (true, false) => "selected",
                                (false, true) => "referenced by the query",
                                (false, false) => "not referenced",
                            };
                            ui.status(&table.name, format!("{}; {} rows", state, table.rows));
                        }
                        ui.list("relationships", "Foreign-key relationships", |ui| {
                            for relation in &self.relations {
                                let child = &self.tables[relation.child];
                                let parent = &self.tables[relation.parent];
                                ui.list_item(
                                    &child.name,
                                    format!(
                                        "{}.{} references {}.{}",
                                        child.name,
                                        relation.from_column,
                                        parent.name,
                                        relation.to_column
                                    ),
                                );
                            }
                        });
                    });
                }
                View::QueryPlan => {
                    ui.list("query-plan", "SQLite query plan", |ui| {
                        for (index, step) in self.result.query_plan.iter().enumerate() {
                            ui.list_item(index.to_string(), step);
                        }
                    });
                }
            })
            .label(self.view.label());
            if let Some(error) = &self.error {
                ui.alert("sql-error", error);
            } else {
                ui.status("sql-status", &self.status);
            }
        });
    }

    fn apply_message(&mut self, message: Message) -> bool {
        match message {
            Message::SetSql(Some(sql)) => {
                self.editor = sql_editor(&sql, self.focus == Focus::Editor);
            }
            Message::RunSql => self.run_sql(),
            Message::ToggleFocus => self.toggle_focus(),
            Message::FocusEditor => self.set_focus(Focus::Editor),
            Message::SelectView(view) => {
                self.view = view;
                self.set_focus(Focus::Views);
            }
            Message::SelectTable(index) if index < self.tables.len() => {
                self.selected_table = index;
                self.set_focus(Focus::Tables);
            }
            Message::PreviewTable(index) if index < self.tables.len() => {
                self.selected_table = index;
                self.preview_table();
            }
            Message::SetSql(None) | Message::SelectTable(_) | Message::PreviewTable(_) => {
                return false;
            }
        }
        true
    }

    fn handle_key(&mut self, key: KeyEvent) -> InputResult {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                InputResult::Exit
            }
            KeyCode::Esc => InputResult::Exit,
            KeyCode::Tab | KeyCode::BackTab => {
                self.toggle_focus();
                InputResult::Changed
            }
            KeyCode::Enter if self.focus == Focus::Tables => {
                self.preview_table();
                InputResult::Changed
            }
            KeyCode::Left | KeyCode::Right if self.focus == Focus::Views => {
                let offset = if key.code == KeyCode::Left {
                    View::ALL.len() - 1
                } else {
                    1
                };
                self.view = View::ALL[(self.view.index() + offset) % View::ALL.len()];
                InputResult::Changed
            }
            KeyCode::Char('1' | '2' | '3') if self.focus == Focus::Views => {
                self.view = match key.code {
                    KeyCode::Char('1') => View::Results,
                    KeyCode::Char('2') => View::Schema,
                    _ => View::QueryPlan,
                };
                InputResult::Changed
            }
            KeyCode::Up if self.focus == Focus::Tables => {
                if !self.tables.is_empty() {
                    self.selected_table = self
                        .selected_table
                        .checked_sub(1)
                        .unwrap_or(self.tables.len() - 1);
                }
                InputResult::Changed
            }
            KeyCode::Down if self.focus == Focus::Tables => {
                if !self.tables.is_empty() {
                    self.selected_table = (self.selected_table + 1) % self.tables.len();
                }
                InputResult::Changed
            }
            KeyCode::F(5) => {
                self.run_sql();
                InputResult::Changed
            }
            KeyCode::Enter
                if self.focus == Focus::Editor
                    && key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.run_sql();
                InputResult::Changed
            }
            _ if self.focus == Focus::Editor && self.editor.input(key) => InputResult::Changed,
            _ => InputResult::Unchanged,
        }
    }

    fn preview_table(&mut self) {
        let Some(table) = self.tables.get(self.selected_table) else {
            return;
        };
        self.editor = sql_editor(
            &format!(
                "SELECT *\nFROM {}\nLIMIT 20;",
                quote_identifier(&table.name)
            ),
            self.focus == Focus::Editor,
        );
        self.run_sql();
    }

    fn run_sql(&mut self) {
        let previous_schema_version = schema_version(&self.connection);
        match execute_sql(&self.connection, &self.sql()) {
            Ok(result) => {
                self.status = format!("{} in {} µs", result.summary, result.elapsed_micros);
                self.result = result;
                self.error = None;
                if let Err(error) = self.refresh_schema() {
                    self.status = "Schema refresh failed".to_owned();
                    self.error = Some(error);
                } else if previous_schema_version != schema_version(&self.connection) {
                    self.view = View::Schema;
                } else {
                    self.view = View::Results;
                }
            }
            Err(error) => {
                self.status = "Query failed".to_owned();
                self.error = Some(error);
            }
        }
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        self.editor
            .set_block(editor_block(self.focus == Focus::Editor));
    }

    fn toggle_focus(&mut self) {
        self.set_focus(match self.focus {
            Focus::Tables => Focus::Editor,
            Focus::Editor => Focus::Views,
            Focus::Views => Focus::Tables,
        });
    }

    fn sql(&self) -> String {
        self.editor.lines().join("\n")
    }

    fn refresh_schema(&mut self) -> Result<(), String> {
        let selected = self
            .tables
            .get(self.selected_table)
            .map(|table| table.name.clone());
        let (tables, relations) =
            read_schema(&self.connection).map_err(|error| error.to_string())?;
        self.selected_table = selected
            .and_then(|selected| tables.iter().position(|table| table.name == selected))
            .map_or(0, |index| index);
        self.tables = tables;
        self.relations = relations;
        Ok(())
    }

    fn referenced_tables(&self) -> Vec<bool> {
        let sql = self
            .sql()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || character == '_' {
                    character.to_ascii_lowercase()
                } else {
                    ' '
                }
            })
            .collect::<String>();
        let words = sql.split_whitespace().collect::<Vec<_>>();
        self.tables
            .iter()
            .map(|table| words.contains(&table.name.to_ascii_lowercase().as_str()))
            .collect()
    }
}

fn seed_database(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute_batch(
        "
        CREATE TABLE services (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            team TEXT NOT NULL,
            status TEXT NOT NULL,
            replicas INTEGER NOT NULL
        );
        CREATE TABLE deployments (
            id INTEGER PRIMARY KEY,
            service_id INTEGER NOT NULL REFERENCES services(id),
            version TEXT NOT NULL,
            environment TEXT NOT NULL,
            status TEXT NOT NULL,
            deployed_at TEXT NOT NULL
        );
        CREATE TABLE incidents (
            id INTEGER PRIMARY KEY,
            service_id INTEGER NOT NULL REFERENCES services(id),
            severity TEXT NOT NULL,
            title TEXT NOT NULL,
            resolved INTEGER NOT NULL,
            opened_at TEXT NOT NULL
        );
        CREATE TABLE metrics (
            service_id INTEGER PRIMARY KEY REFERENCES services(id),
            cpu_percent REAL NOT NULL,
            latency_ms INTEGER NOT NULL,
            error_rate REAL NOT NULL
        );

        INSERT INTO services VALUES
            (1, 'gateway', 'platform', 'healthy', 4),
            (2, 'checkout', 'commerce', 'degraded', 6),
            (3, 'payments', 'commerce', 'healthy', 3),
            (4, 'catalog', 'storefront', 'healthy', 5),
            (5, 'search', 'storefront', 'healthy', 4),
            (6, 'telemetry', 'platform', 'healthy', 2);

        INSERT INTO deployments VALUES
            (1, 2, 'v2.18.4', 'production', 'succeeded', '2026-09-22 12:41'),
            (2, 1, 'v4.3.1', 'production', 'succeeded', '2026-09-22 11:18'),
            (3, 5, 'v1.9.0', 'staging', 'running', '2026-09-22 10:55'),
            (4, 3, 'v3.7.2', 'production', 'succeeded', '2026-09-21 18:22'),
            (5, 4, 'v5.1.6', 'production', 'rolled_back', '2026-09-21 16:04'),
            (6, 4, 'v5.1.5', 'production', 'succeeded', '2026-09-21 16:19');

        INSERT INTO incidents VALUES
            (1, 2, 'SEV-2', 'Checkout p95 latency above 500 ms', 0, '2026-09-22 12:47'),
            (2, 3, 'SEV-3', 'Payment provider timeouts', 0, '2026-09-22 12:44'),
            (3, 4, 'SEV-3', 'Catalog deployment rollback', 1, '2026-09-21 16:08'),
            (4, 1, 'SEV-4', 'Gateway certificate warning', 1, '2026-09-20 09:31'),
            (5, 2, 'SEV-3', 'Checkout queue saturation', 1, '2026-09-18 14:12');

        INSERT INTO metrics VALUES
            (1, 42.1, 38, 0.02),
            (2, 81.4, 684, 7.81),
            (3, 67.8, 219, 2.34),
            (4, 38.5, 72, 0.11),
            (5, 54.2, 96, 0.08),
            (6, 29.7, 24, 0.01);
        ",
    )
}

fn read_schema(connection: &Connection) -> rusqlite::Result<(Vec<TableInfo>, Vec<Relation>)> {
    let names = {
        let mut statement = connection.prepare(
            "SELECT name FROM sqlite_schema \
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut tables = Vec::with_capacity(names.len());
    for name in names {
        let identifier = quote_identifier(&name);
        let columns = {
            let mut statement = connection.prepare(&format!("PRAGMA table_info({identifier})"))?;
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let rows =
            connection.query_row(&format!("SELECT COUNT(*) FROM {identifier}"), [], |row| {
                row.get(0)
            })?;
        tables.push(TableInfo {
            name,
            columns,
            rows,
        });
    }

    let mut relations = Vec::new();
    for (child, table) in tables.iter().enumerate() {
        let identifier = quote_identifier(&table.name);
        let mut statement =
            connection.prepare(&format!("PRAGMA foreign_key_list({identifier})"))?;
        let foreign_keys = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (parent_name, from_column, to_column) in foreign_keys {
            if let Some(parent) = tables.iter().position(|table| table.name == parent_name) {
                relations.push(Relation {
                    child,
                    parent,
                    from_column,
                    to_column,
                });
            }
        }
    }
    Ok((tables, relations))
}

fn schema_version(connection: &Connection) -> Option<i64> {
    connection
        .query_row("PRAGMA schema_version", [], |row| row.get(0))
        .ok()
}

fn execute_sql(connection: &Connection, sql: &str) -> Result<SqlResult, String> {
    if sql.trim().is_empty() {
        return Err("Enter some SQL first".to_owned());
    }

    let started = Instant::now();
    let mut statement = connection.prepare(sql).map_err(|error| error.to_string())?;
    let columns = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if columns.is_empty() {
        drop(statement);
        connection
            .execute_batch(sql)
            .map_err(|error| error.to_string())?;
        let changed = connection.changes();
        let summary = if changed == 1 {
            "Statement executed; 1 row changed".to_owned()
        } else {
            format!("Statement executed; {changed} rows changed")
        };
        return Ok(SqlResult {
            columns: vec!["Result".to_owned()],
            rows: vec![vec![summary.clone()]],
            query_plan: vec![
                "Query plans are available for statements that return rows".to_owned(),
            ],
            summary,
            elapsed_micros: started.elapsed().as_micros(),
        });
    }

    let query_plan = explain_query_plan(connection, sql);
    let mut values = Vec::new();
    let mut rows = statement.query([]).map_err(|error| error.to_string())?;
    while let Some(row) = rows.next().map_err(|error| error.to_string())? {
        let mut result_row = Vec::with_capacity(columns.len());
        for index in 0..columns.len() {
            let value = row.get_ref(index).map_err(|error| error.to_string())?;
            result_row.push(value_to_string(value));
        }
        values.push(result_row);
        if values.len() == 100 {
            break;
        }
    }

    let summary = format!("{} rows returned", values.len());
    Ok(SqlResult {
        columns,
        rows: values,
        query_plan,
        summary,
        elapsed_micros: started.elapsed().as_micros(),
    })
}

fn explain_query_plan(connection: &Connection, sql: &str) -> Vec<String> {
    let explained = format!("EXPLAIN QUERY PLAN {sql}");
    let Ok(mut statement) = connection.prepare(&explained) else {
        return vec!["SQLite did not provide a query plan for this statement".to_owned()];
    };
    match statement.query_map([], |row| row.get::<_, String>(3)) {
        Ok(steps) => steps.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(_) => vec!["SQLite did not provide a query plan for this statement".to_owned()],
    }
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

fn value_to_string(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Null => "NULL".to_owned(),
        ValueRef::Integer(value) => value.to_string(),
        ValueRef::Real(value) => format!("{value:.2}"),
        ValueRef::Text(value) => String::from_utf8_lossy(value).into_owned(),
        ValueRef::Blob(value) => format!("<{} bytes>", value.len()),
    }
}

fn sql_editor(sql: &str, focused: bool) -> TextArea<'static> {
    let mut editor = TextArea::new(sql.lines().map(str::to_owned).collect());
    editor.set_block(editor_block(focused));
    editor.set_style(Color::White.into());
    editor.set_cursor_line_style(Style::default());
    editor.set_cursor_style(Style::new().black().on_white());
    editor.set_line_number_style(Color::Gray.into());
    editor
}

fn editor_block(focused: bool) -> Block<'static> {
    panel(" SQL editor ").border_style(if focused {
        Color::Yellow
    } else {
        Color::DarkGray
    })
}

fn table_positions(count: usize) -> Vec<(f64, f64)> {
    if count == 1 {
        return vec![(50.0, 50.0)];
    }
    (0..count)
        .map(|index| {
            let angle = TAU * index as f64 / count as f64;
            (50.0 + 36.0 * angle.cos(), 50.0 + 32.0 * angle.sin())
        })
        .collect()
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .dark_gray()
        .title(title.into().white())
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::new().map_err(|error| io::Error::other(error.to_string()))?;
    let mut molerat = Molerat::<Message>::new_mcp("Molerat Database Explorer")?;
    app.publish(&mut molerat);

    loop {
        terminal.draw(|frame| app.render(frame))?;
        let mut changed = false;
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.is_press()
        {
            match app.handle_key(key) {
                InputResult::Exit => break,
                InputResult::Changed => changed = true,
                InputResult::Unchanged => {}
            }
        }
        for message in molerat.messages() {
            changed |= app.apply_message(message);
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
