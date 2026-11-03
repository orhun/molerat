# molerat

An accessibility & agentic layer for Rust UI applications 🦫

## Quickstart

```bash
cargo add molerat
```

Add **molerat** to your app and publish a semantic view alongside your existing UI:

```rust
use molerat::Molerat;

let mut molerat = Molerat::new("Notes");
molerat.publish(|ui| {
    ui.text("status", "All changes saved");
});
```

If you have actions in your app, define an enum and return them from your UI handlers:

```rust
#[derive(Debug)]
enum Message {
    Save,
}

let mut molerat = Molerat::<Message>::new("Notes");
molerat.publish(|ui| {
    ui.button("save", "Save").on_click(|| Message::Save);
});
```

To handle the incoming actions:

```rust
for message in molerat.messages() {
    match message {
        Message::Save => { /* update app state */ }
    }
}
```

## Integrations

### MCP

> [!NOTE]
> MCP (Model Context Protocol) is a protocol for exposing application state to agents.

Enable `mcp` feature to expose the UI to agents through MCP:

```bash
cargo add molerat --features mcp
```

MCP clients connect through the companion `molerat-mcp` stdio bridge. Install it with:

```bash
cargo install molerat --features mcp --bin molerat-mcp
```

Register the installed binary with your MCP client.

And then you can change your constructor to use the MCP bridge:

```rust
let mut molerat = Molerat::<Message>::new_mcp("Notes")?;
```

### AT-SPI

> [!NOTE]
> AT-SPI (Assistive Technology Service Provider Interface) is a protocol for exposing application state to Unix desktop accessibility clients.

Enable `atspi` feature to expose the UI to agents through AT-SPI:

```bash
cargo add molerat --features atspi
```

```rust
let mut molerat = Molerat::<Message>::new_atspi("Notes");
```

Nothing else is needed! **molerat** exposes your UI on the Unix accessibility bus; handle requested actions through `molerat.messages()`.

## For Ratatui applications

**molerat** is not designed for any specific UI framework. However, it has first-class integration with [Ratatui](https://ratatui.rs/)!

See the [Ratatui migration guide](./RATATUI_MIGRATION.md) and [examples](./examples).

## Examples

Here is [knife](https://github.com/bl4ckr0ss3/knife), a reverse-engineering TUI solving a crackme challenge using **molerat** + Codex:

https://github.com/user-attachments/assets/586710de-214c-468c-91b7-0126d7a25592

Here is [BugStalker](https://github.com/godzie44/BugStalker), a Rust debugger debugging an application using **molerat** + Codex:

https://github.com/user-attachments/assets/ff256f9a-4f7b-4374-a7d3-5d91e2ebec55

## License

Licensed under either of [Apache License Version 2.0](./LICENSE-APACHE) or [The MIT License](./LICENSE-MIT) at your option 🐶

🦀 ノ( º \_ º ノ) - respect crables!

## Copyright

Copyright © 2026, [Orhun Parmaksız](mailto:orhunparmaksiz@gmail.com)
