# tree-selector-example

Two plugins collaborating without knowing about each other or the host.
`plugin_a` provides an `activity-feed` slot and marks its action area with a
`data-plugin-slot="feed-actions"` attribute. `plugin_b` uses a `Within` tree
selector to find that node inside `plugin_a`'s output and inject a Share button
into it.

**Concepts demonstrated:** multi-plugin setup, `Selector::Within` / tree
selectors, plugin-to-plugin injection without coupling.

## Structure

```
tree-selector-example/
├── plugin_a/   # provides the activity-feed slot
├── plugin_b/   # injects into plugin_a's output via tree selector
└── host/       # Dioxus fullstack host (dx serve)
```

## Prerequisites

- `rustup target add wasm32-unknown-unknown`
- `cargo install dioxus-cli`

## Run

**1. Build both plugins**

```bash
cargo build -p tree-selector-example-plugin-a --target wasm32-unknown-unknown --release
cargo build -p tree-selector-example-plugin-b --target wasm32-unknown-unknown --release
```

**2. Start the host**

```bash
cd examples/tree-selector-example/host
dx serve
```

**3. Open the browser**

Navigate to [http://localhost:8080](http://localhost:8080).

## Expected output

An activity feed card containing:

- `Latest activity / User alice posted a comment.` — from `plugin_a`
- `👍 Like` — from `plugin_a`'s action area
- `🔗 Share — injected by plugin_b` — injected by `plugin_b` into the same action area
