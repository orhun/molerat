# Migrating a Ratatui application to Molerat

This is an implementation guide for coding agents. When this file is supplied with a Ratatui
repository, inspect that repository and add Molerat support while preserving its existing UI,
architecture, controls, and behavior.

The result must expose the application's meaningful state and operations through one of these
integrations:

- **MCP** for agents, using `Molerat::new_mcp`.
- **AT-SPI** for assistive technology on supported Unix desktops, using
  `Molerat::new_atspi`.

Do not treat Molerat as a terminal-screen parser. Do not expose the rendered character buffer.
Build the semantic tree directly from application state, next to the Ratatui rendering code.

## Agent execution contract

Treat this document as an implementation task, not as a request for a plan. Inspect the target
repository, make the required code and manifest changes, run the relevant checks, and report the
result.

Resolve the integration mode before editing:

- If the request mentions agents, Codex, or MCP, use the `mcp` feature and `Molerat::new_mcp`.
- If the request mentions accessibility, a screen reader, Orca, Accerciser, or AT-SPI, use the
  `atspi` feature and `Molerat::new_atspi`.
- If neither goal is stated and the surrounding task does not establish it, ask which integration
  is wanted. Do not silently add both.

Use the Molerat version requested by the user or already present in the repository. If neither is
available, use the Git dependency shown below. Treat the checked-out Molerat API as authoritative
if it differs from a snippet in this guide.

## Non-negotiable design rules

1. **Application state is the source of truth.** Ratatui rendering and Molerat publication must
   read the same state.
2. **Reuse the existing message or action type.** Keyboard, mouse, AT-SPI, and MCP operations must
   converge on the same update path.
3. **Publish a complete semantic tree.** `Molerat::publish` describes the current UI; it is not an
   incremental patch API.
4. **Use stable, unique semantic keys.** Keys identify nodes across publications. They are not
   visible labels.
5. **Republish after state changes.** An action is not observable to consumers until the updated
   state is published.
6. **Expose meaning, not terminal decoration.** Borders, padding, colors, and layout spacers do not
   need nodes. Labels, values, relationships, selection, focus, status, and operations do.
7. **Do not simulate keystrokes for agents.** Register semantic actions that produce the same
   application messages as keyboard bindings.
8. **Keep the migration focused.** Do not rewrite unrelated rendering or application logic unless
   a small refactor is needed to share the existing state transition path.

## Phase 1: inspect the application before editing

Find and understand all of the following:

- `Cargo.toml`, workspace manifests, feature flags, and the minimum supported Rust version.
- The main `App`, `Model`, or state type.
- The event loop and terminal initialization/cleanup path.
- The keyboard and mouse handlers.
- The existing message, command, action, or event enum.
- The state mutation function, such as `update`, `handle_action`, or `handle_event`.
- Every rendered screen, modal, tab, list, table, input, status line, and canvas/chart.
- How visual focus and selection are represented.
- Which operations are important but currently available only through keyboard shortcuts.
- Existing tests, lint policy, formatting commands, and repository-specific instructions.

Do not begin by adding a second app model. If keyboard handling mutates fields directly, first make
the smallest practical refactor so keyboard input and Molerat callbacks can produce a shared
message handled in one place.

For example, prefer this:

```rust
#[derive(Clone, Debug)]
enum Message {
    SelectItem(usize),
    SetSearch(Option<String>),
    Refresh,
    Quit,
}

impl App {
    fn update(&mut self, message: Message) -> bool {
        match message {
            Message::SelectItem(index) if index < self.items.len() => {
                self.selected = index;
            }
            Message::SetSearch(Some(search)) => self.search = search,
            Message::Refresh => self.refresh(),
            Message::Quit => self.should_quit = true,
            Message::SelectItem(_) | Message::SetSearch(None) => return false,
        }
        true
    }
}
```

Keyboard and mouse handlers should produce these messages instead of duplicating the mutations.

## Phase 2: add the dependency and choose one integration

Follow the repository's existing dependency-management style. If Molerat is not yet published at
the version required by the application, use its Git repository:

```toml
[dependencies]
molerat = { git = "https://github.com/orhun/molerat", features = ["mcp"] }
```

For AT-SPI:

```toml
[dependencies]
molerat = { git = "https://github.com/orhun/molerat", features = ["atspi"] }
```

If the application is inside the same workspace or a local checkout is explicitly requested, use
the appropriate `path` dependency instead. If a compatible crates.io release is already used by
the repository, preserve that versioning policy and enable the required feature.

Choose the integration requested by the user:

| Goal                        | Cargo feature | Constructor                           | Constructor result       |
| --------------------------- | ------------- | ------------------------------------- | ------------------------ |
| Agent control               | `mcp`         | `Molerat::<Message>::new_mcp(name)`   | `io::Result<Molerat<_>>` |
| Screen-reader accessibility | `atspi`       | `Molerat::<Message>::new_atspi(name)` | `Molerat<_>`             |

Enabling a feature only compiles the integration. The matching constructor activates it. The
current API is intended for choosing MCP **or** AT-SPI at construction time; do not assume enabling
both features activates both transports on one handle.

The application does not need Tokio, Axum, an HTTP server, or its own IPC implementation. MCP
transport and local IPC are managed by Molerat.

## Phase 3: create and retain one Molerat handle

Create one handle for the lifetime of the TUI. Its generic parameter must be the application's
shared message type.

### MCP

```rust
let mut molerat = Molerat::<Message>::new_mcp("Application name")?;
```

`new_mcp` can fail while creating the private local endpoint. Propagate or report that error using
the application's normal error strategy.

### AT-SPI

```rust
let mut molerat = Molerat::<Message>::new_atspi("Application name");
```

The name should be concise and user-facing. Do not recreate the handle on every frame or state
change: stable handles preserve stable semantic node IDs and transport state.

The handle may live in the event-loop owner or in the app type. Choose whichever produces the
smallest, most idiomatic change in the target repository.

## Phase 4: publish a semantic view of the current state

Add a method near the app's rendering or state code:

```rust
use molerat::Molerat;

impl App {
    fn publish(&self, molerat: &mut Molerat<Message>) {
        molerat.publish(|ui| {
            // Describe the complete current UI here.
        });
    }
}
```

Call it once after initialization and again after every meaningful state change:

```rust
let mut app = App::new()?;
let mut molerat = Molerat::<Message>::new_mcp("Tasks")?;
app.publish(&mut molerat);

while !app.should_quit {
    terminal.draw(|frame| app.render(frame))?;
    let mut changed = false;

    if event::poll(Duration::from_millis(100))? {
        if let Some(message) = app.message_for_event(event::read()?) {
            changed |= app.update(message);
        }
    }

    for message in molerat.messages() {
        changed |= app.update(message);
    }

    if changed && !app.should_quit {
        app.publish(&mut molerat);
    }
}
```

`Molerat::messages` drains messages produced by MCP or AT-SPI actions. Always poll it in the normal
TUI loop and process the results on the TUI thread. Do not process application state inside an MCP
worker or accessibility callback.

If the existing app uses an async event loop, preserve it. Poll `molerat.messages()` wherever local
input messages are applied; no separate Molerat runtime is required.

## Semantic keys

Every node receives a key. The key is combined with its container path to produce a stable
AccessKit node ID.

Use keys such as:

- Stable entity IDs: `task.id.to_string()`, `process.pid.to_string()`.
- Stable field names: `"status"`, `"search"`, `"cpu"`, `"results"`.
- A column index plus name when names can repeat: `format!("{index}:{name}")`.

Avoid:

- Display text that can change.
- A row index for reorderable data that has a stable domain ID.
- Duplicate sibling keys.
- Random values or newly generated UUIDs on each publication.

Duplicate keys under the same semantic parent produce duplicate node IDs and an invalid or
misleading tree. This is especially easy to do with SQL results and tables containing duplicate
column names.

## Widget-to-semantic mapping

Use the closest semantic component rather than mirroring Ratatui's widget type mechanically.

| Ratatui/application concept          | Molerat API                                                                     |
| ------------------------------------ | ------------------------------------------------------------------------------- |
| Static short text                    | `ui.text(key, value)`                                                           |
| Paragraph or instructions            | `ui.paragraph(key, text).label(label)`                                          |
| Group/panel with meaningful children | `ui.group(key, label, children)`                                                |
| Plain list                           | `ui.list` and `ui.list_item`                                                    |
| Selectable list/menu                 | `ui.list_box` and `ui.list_box_option`                                          |
| Table                                | `ui.table`, `ui.row`, `ui.column_header`, `ui.row_header`, `ui.cell`            |
| Button-like operation                | `ui.button(...).on_click(...)`                                                  |
| Editable text                        | `ui.text_input(...).on_action(Action::SetValue, ...)`                           |
| Checkbox/toggle                      | `ui.checkbox(...).on_click(...)`                                                |
| Gauge/progress bar                   | `ui.progress(key, label, value, min, max)`                                      |
| Non-urgent changing state            | `ui.status(key, message)`                                                       |
| Urgent error requiring announcement  | `ui.alert(key, message)`                                                        |
| Application-wide operation           | `ui.command(key, description, handler)`                                         |
| Tabs                                 | `Role::TabList`, `Role::Tab`, and `Role::TabPanel` via `ui.container`/`ui.node` |
| Canvas, chart, or graph              | Meaningful group, status/text/list data, plus semantic actions                  |

### Static text and paragraphs

Do not publish fragmented spans, border titles, or every rendered line independently unless they
carry distinct meaning. Publish the logical content:

```rust
ui.paragraph("help", &self.help_text).label("Keyboard help");
ui.text("version", format!("Version {}", self.version));
```

### Selectable lists

Selection and semantic focus are different concepts. Mark the selected item, and mark as focused
only the item currently receiving keyboard focus:

```rust
ui.list_box("tasks", "Tasks", |ui| {
    for (index, task) in self.tasks.iter().enumerate() {
        ui.list_box_option(task.id.to_string(), &task.title)
            .description(&task.description)
            .selected(index == self.selected)
            .focused(self.focus == Focus::Tasks && index == self.selected)
            .on_focus(move || Message::SelectItem(index))
            .on_click(move || Message::SelectItem(index));
    }
});
```

Only one node may be `.focused(true)` in a publication. Multiple focused nodes cause publication
to panic. A selected item does not automatically need semantic focus when another widget, such as
a text editor, owns focus.

### Tables

Expose headers and cell values, not a preformatted text table:

```rust
ui.table("processes", "Processes", |ui| {
    ui.row("headers", |ui| {
        ui.column_header("pid", "PID");
        ui.column_header("name", "Name");
        ui.column_header("cpu", "CPU usage");
    });

    for process in &self.processes {
        ui.row(process.pid.to_string(), |ui| {
            ui.cell("pid", process.pid.to_string());
            ui.row_header("name", &process.name);
            ui.cell("cpu", format!("{} percent", process.cpu));
        });
    }
});
```

For generated columns, include the index in both header and cell keys:

```rust
ui.row("headers", |ui| {
    for (index, column) in columns.iter().enumerate() {
        ui.column_header(format!("{index}:{column}"), column);
    }
});

for (row_index, row) in rows.iter().enumerate() {
    ui.row(row_index.to_string(), |ui| {
        for (index, (column, value)) in columns.iter().zip(row).enumerate() {
            ui.cell(format!("{index}:{column}"), value);
        }
    });
}
```

### Text input

Expose the current value and register `Action::SetValue`. Avoid `unwrap` in the callback because
the same handler may also receive platform accessibility requests:

```rust
use molerat::accesskit::{Action, ActionData};

ui.text_input("search", "Search", &self.search)
    .focused(self.focus == Focus::Search)
    .on_focus(|| Message::FocusSearch)
    .on_action(Action::SetValue, |request| {
        Message::SetSearch(match request.data.as_ref() {
            Some(ActionData::Value(value)) => Some(value.to_string()),
            _ => None,
        })
    });
```

Handle `SetSearch(None)` as an ignored message. Do not update the visual editor in the callback;
update it when applying the message on the application thread.

### Tabs

```rust
use molerat::accesskit::Role;

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
.label("Views");

ui.container("active-view", Role::TabPanel, |ui| {
    // Publish the active view's content.
})
.label(self.view.label());
```

### Canvas, charts, and custom widgets

A visual canvas has no useful accessibility meaning by itself. Publish a semantic description of
its model:

```rust
ui.group("canvas", "Drawing canvas", |ui| {
    ui.text("cursor", format!("Cursor at {}, {}", self.cursor.x, self.cursor.y));
    ui.text("tool", self.tool.label());
    ui.list("objects", "Canvas objects", |ui| {
        for object in &self.objects {
            ui.list_item(object.id.to_string(), object.description());
        }
    });
})
.focused(self.focus == Focus::Canvas)
.on_focus(|| Message::FocusCanvas)
.custom_action("move-left", "Move cursor left", || Message::MoveLeft)
.custom_action("move-right", "Move cursor right", || Message::MoveRight)
.custom_action("apply-tool", "Apply the selected tool", || Message::ApplyTool);
```

For charts, expose the current value, units, trend, range, and important series—not Braille glyphs
or rendered coordinates. For diagrams, expose the nodes and relationships as lists or groups.

### Status and alerts

Use `status` for ordinary changing information that may be announced politely. Use `alert` only for
urgent failures that should interrupt current screen-reader output:

```rust
if let Some(error) = &self.error {
    ui.alert("operation-error", error);
} else {
    ui.status("operation-status", &self.status);
}
```

Keep the key stable. Changing the key on every update prevents consumers from recognizing the same
live region.

## Actions and application commands

### Common actions

Use convenience methods for ordinary interaction:

```rust
ui.button("save", "Save")
    .disabled(!self.can_save)
    .on_click(|| Message::Save);

ui.list_box_option("settings", "Settings")
    .on_focus(|| Message::OpenSettings)
    .on_click(|| Message::OpenSettings);
```

Disabled nodes do not expose or accept actions through MCP. Still set `.disabled(true)` so both
agents and assistive technology receive the correct state.

### Custom widget actions

Use `custom_action` for a meaningful operation that has no suitable standard AccessKit action:

```rust
ui.list_item(item.id.to_string(), &item.name)
    .custom_action("retry", "Retry this job", move || Message::Retry(item.id))
    .custom_action("cancel", "Cancel this job", move || Message::Cancel(item.id));
```

The key identifies the action and keeps its numeric AccessKit ID stable. The description is what
agents and assistive technology use to understand the operation. Both must be concise and
specific.

### Application-wide commands

`ui.command` is a wrapper that registers a custom action on the semantic root:

```rust
ui.command("refresh", "Refresh all data", || Message::Refresh);
ui.command("switch-focus", "Move focus to the next panel", || {
    Message::ToggleFocus
});
ui.command("quit", "Quit the application", || Message::Quit);
```

Expose important keyboard-only operations this way. Do not expose every implementation detail or
redundant key alias. Prefer intent such as `refresh` or `clear-canvas` over `press-r`.

## MCP-specific integration

The architecture is:

```text
MCP client <-- stdio --> molerat-mcp <-- private local IPC --> TUI application
```

The application calls `Molerat::new_mcp` and publishes state. It does not launch or communicate
with the `molerat-mcp` binary directly. The application must poll `Molerat::messages` so agent
actions reach its update function.

The separate `molerat-mcp` binary is installed and registered once by the user. That registration
can be reused across Molerat-enabled TUIs, but the default discovery path exposes one running
application at a time. If multiple enabled applications are running, the most recently started
one owns the discoverable endpoint. Adding `molerat` to the application's dependencies does not
itself install this user-facing binary.

To install the bridge from Git:

```console
cargo install --git https://github.com/orhun/molerat --features mcp --bin molerat-mcp
```

Keep the installed bridge compatible with the Molerat version used by the application. Reinstall
it after moving the app to an incompatible Molerat revision.

For a repository checkout, a typical development flow is:

```console
cargo build --features mcp --bin molerat-mcp
codex mcp add molerat -- /absolute/path/to/target/debug/molerat-mcp
```

If `CARGO_TARGET_DIR` is set, use the binary from that target directory. If the binary is installed
with Cargo, register the absolute path reported by `command -v molerat-mcp`. App authors only add
the library integration; end users install and register the bridge with their MCP client.

To verify MCP behavior:

1. Start the Molerat-enabled TUI and leave it running.
2. Start or restart the MCP client after registering the bridge.
3. Call `get_ui` and confirm the application name, tree, state, and actions are meaningful.
4. Invoke only an action advertised by `get_ui`, using its returned node ID.
5. Confirm the visible TUI changes.
6. Call `get_ui` again and confirm the updated state is published.
7. Test standard actions, custom widget actions, and root commands.
8. Confirm disabled and hidden nodes cannot be operated.

Do not add a network listener, authentication layer, or app-specific MCP binary unless the user
explicitly requests a different architecture.

## AT-SPI-specific integration

AT-SPI needs terminal focus notifications in addition to semantic focus inside the published tree.
For Crossterm-based applications, enable focus-change reporting during terminal setup:

```rust
use ratatui::crossterm::{
    event::{DisableFocusChange, EnableFocusChange},
    execute,
};

execute!(std::io::stdout(), EnableFocusChange)?;
let result = ratatui::run(run);
execute!(std::io::stdout(), DisableFocusChange)?;
result
```

Forward focus events from the existing event loop:

```rust
match event::read()? {
    Event::FocusGained => molerat.set_window_focused(true),
    Event::FocusLost => molerat.set_window_focused(false),
    Event::Key(key) if key.is_press() => {
        // Existing keyboard handling.
    }
    _ => {}
}
```

Terminal-window focus and semantic widget focus are separate:

- `set_window_focused` tells AT-SPI whether the terminal application is active.
- `.focused(true)` identifies the active semantic widget or item inside the app.

Always restore focus-change reporting during cleanup, including error paths, using the target
application's existing terminal-cleanup pattern.

To verify AT-SPI behavior on a supported Unix desktop:

1. Start the application in a graphical terminal within the desktop session.
2. Inspect its tree with Accerciser or another AT-SPI inspector.
3. Confirm the application window, lists, tables, values, selection, and focus are present.
4. Navigate with Orca and confirm labels and values are understandable without seeing the TUI.
5. Trigger registered actions and confirm the visible app updates.
6. Change focus between the terminal and another application and confirm focus is reported.
7. Confirm ordinary status changes are polite and alerts are reserved for urgent failures.

## Common mistakes to avoid

- Publishing terminal cells, ANSI text, Braille plots, borders, or Unicode decorations.
- Creating a separate accessibility-only copy of application state.
- Updating state directly inside Molerat callbacks.
- Registering actions but never polling `molerat.messages()`.
- Applying agent messages but failing to republish afterward.
- Creating a new `Molerat` handle for every frame.
- Using changing labels as semantic keys.
- Using duplicate keys for generated table columns.
- Marking multiple nodes as focused.
- Treating selection and focus as the same state in every widget.
- Exposing a canvas or chart without a textual/data representation.
- Exposing key presses instead of semantic operations.
- Calling `with_accesskit` to add actions without registering matching handlers through
  `on_action` or `custom_action`.
- Adding Tokio or an HTTP server to the TUI solely for Molerat MCP support.
- Continuously republishing unchanged live regions, which can cause noisy screen-reader output.

## Validation commands

Use the repository's own commands when present. At minimum, run the applicable equivalents of:

```console
cargo fmt --all -- --check
cargo check --all-targets
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

These commands are sufficient when the `molerat` dependency directly enables `mcp` or `atspi`, as
shown earlier. If the target application places the integration behind its own Cargo feature, run
the checks with that application feature enabled. Do not pass `--features mcp` or
`--features atspi` unless the target application itself defines a feature with that name. If the
repository intentionally supports several feature combinations, also run its all-features and
no-default-features checks. Do not introduce new warnings.

Run the application interactively and test existing keyboard/mouse behavior in addition to the
selected Molerat integration. A successful build is not sufficient.

## Acceptance checklist

Before finishing, verify all of the following:

- [ ] The selected Molerat feature and constructor are used.
- [ ] Exactly one long-lived `Molerat<Message>` handle exists.
- [ ] Initial state is published before consumers inspect the app.
- [ ] The published tree is rebuilt after every meaningful state change.
- [ ] Local input and Molerat actions use the same message/update path.
- [ ] `molerat.messages()` is polled in the application loop.
- [ ] Keys are stable and unique within each semantic parent.
- [ ] Lists expose labels, selection, focus, and selection actions.
- [ ] Tables expose headers, rows, and individual cells with unique keys.
- [ ] Inputs expose their current value and a validated `SetValue` handler.
- [ ] Important keyboard-only operations have semantic custom actions or commands.
- [ ] Disabled, hidden, selected, expanded, toggled, and focused states match the visual UI.
- [ ] Status and error announcements use appropriate live-region semantics.
- [ ] Canvases/charts have a useful non-visual representation.
- [ ] MCP changes appear in a subsequent `get_ui`, or AT-SPI changes appear in the inspector.
- [ ] Existing visual behavior and local controls still work.
- [ ] Formatting, Clippy, tests, and an interactive smoke test pass.

## Expected agent handoff

When reporting completion, state:

1. Which integration was added: MCP or AT-SPI.
2. Which application state and widgets are now exposed.
3. Which keyboard operations gained semantic actions.
4. Where `publish` and `messages` were integrated into the event loop.
5. How the integration was tested.
6. Any meaningful application state that remains intentionally unexposed and why.

Do not claim accessibility merely because the project compiles. The semantic tree and actions must
be inspected from the selected consumer and must remain synchronized with the visible TUI.
