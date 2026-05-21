# notes-plugin

Demonstrates `dx_invoke` — calling host-registered business logic from inside a
WASM plugin. The plugin renders a per-article notes section. It reads
`current_page` from its session state (set by the host before calling
`render_slot`), fetches notes from a host-owned in-memory store via
`dx_invoke("get_notes", …)`, and lets the user add notes via
`dx_invoke("add_note", …)`. Interactions return a fresh view without a full
page reload.

**Concepts demonstrated:** `dx_invoke`, session state pre-population,
`on_interaction`, plugin page routes (`PluginPageOutlet`), host-side invocation
handler registration (`register_invocation`).

## Structure

```
notes-plugin/
├── plugin/   # WASM plugin (wasm32-unknown-unknown)
└── host/     # Dioxus fullstack host (dx serve)
```

## Prerequisites

- `rustup target add wasm32-unknown-unknown`
- `cargo install dioxus-cli`

## Run

**1. Build the plugin**

```bash
cargo build -p notes-plugin-plugin --target wasm32-unknown-unknown --release
```

**2. Start the host**

```bash
cd examples/notes-plugin/host
dx serve
```

**3. Open the browser**

Navigate to [http://localhost:8080](http://localhost:8080), then click any article
link.

## Expected output

On an article page:

- Static article content rendered by the host
- A **Notes** section below the `<hr>`, contributed entirely by the plugin
- An input field + **Add note** button; typing and clicking adds notes that persist
  in the host store for the session
- Navigating to a different article shows that article's notes independently
- `/p/notes` shows a plugin-declared page listing all notes across articles
