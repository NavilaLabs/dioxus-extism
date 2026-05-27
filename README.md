# dioxus-extism

A Rust workspace that extends [Dioxus](https://dioxuslabs.com/) fullstack applications with [Extism](https://extism.org/) WASM plugins.

Plugins run **server-side only** inside a wasmtime sandbox. Each plugin describes its UI as a serialisable `PluginView` tree; the host Dioxus frontend fetches and renders it. Plugins can contribute to named **slots**, wrap or inject around **routes**, and override individual **components** — all without any changes to the host application code.

## Architecture at a glance

```
┌─────────────────────────────────────────────┐
│  Browser (WASM)                             │
│  Dioxus frontend                            │
│    PluginSlot / PluginAwareRouter           │
│      └─ calls server functions via HTTP     │
└──────────────────┬──────────────────────────┘
                   │ server functions
┌──────────────────▼──────────────────────────┐
│  Server (native)                            │
│  Dioxus fullstack (axum)                    │
│    PluginRuntime (Arc<PluginRuntime>)       │
│      └─ calls plugin exports via extism     │
└──────────────────┬──────────────────────────┘
                   │ WASM function calls
┌──────────────────▼──────────────────────────┐
│  Plugin (wasm32-unknown-unknown .wasm)      │
│  Returns PluginView — serialised UI tree    │
└─────────────────────────────────────────────┘
```

## Capabilities

| Capability | Description |
|---|---|
| **Slots** | Plugins contribute UI fragments to named insertion points in the host |
| **Route transforms** | Wrap, inject-before, or inject-after any host route |
| **Component overrides** | Replace or augment any `#[overridable]` host component |
| **Tree transforms** | Inject into specific DOM nodes within a plugin's own output (`Within` selector) |
| **Plugin pages** | Plugins declare full-page routes (e.g. `/p/stats`) served by the host router |
| **Named API routes** | Plugins declare HTTP endpoints (GET/POST/…) mounted on the host's Axum router |
| **Hook handlers** | Plugins intercept named server-side operations; can `Continue`, `Cancel`, or `Replace` |
| **Event bus** | Plugins emit and subscribe to named JSON events routed through the host |
| **Interactions** | Button clicks and input changes round-trip to the plugin and return partial view updates — no full page reload |
| **State — per-session** | Isolated key-value state per user session |
| **State — global** | Shared key-value state across all sessions |
| **Cross-plugin state reads** | One plugin can read another plugin's state (capability-gated) |
| **`dx_invoke`** | Plugins call named host-registered async handlers (DB queries, service calls, etc.) |
| **Lifecycle hooks** | `on_load` / `on_unload` called when the plugin pool is initialised or torn down |
| **Priority hints** | Plugins declare `First`/`High`/`Normal`/`Low`/`Last` ordering for slots and transforms |
| **Capability model** | Plugins declare required host capabilities in their manifest; the host enforces access at runtime |
| **Hot-reload** | `PluginRuntime::reload_plugin` atomically swaps a running plugin without restarting the server |

## Prerequisites

- Rust stable (`rustup update stable`)
- WASM target: `rustup target add wasm32-unknown-unknown`
- Dioxus CLI: `cargo install dioxus-cli` (provides the `dx` command)

## Running the examples

Each example is two crates: a **plugin** (compiled to WASM by cargo) and a **host** (a Dioxus fullstack server launched via `dx serve`).

`dx serve` compiles both the native server and the browser WASM client, then starts the server. Without it the browser receives only the initial SSR HTML with no WASM bundle, so plugin content never appears.

---

### hello-plugin

The simplest example. A single plugin contributes a "hello-slot" that renders a greeting.

**1. Build the plugin**

```bash
cargo build -p hello-plugin-plugin --target wasm32-unknown-unknown --release
```

**2. Start the host**

```bash
cd examples/hello-plugin/host
dx serve
```

**3. Open the browser**

Navigate to [http://localhost:3010](http://localhost:3010).

Expected: the page shows "Hello Plugin Example" as the static heading, and "Hello from a WASM plugin!" inside a `div.hello-from-plugin` contributed by the plugin.

---

### route-injection-example

A plugin that intercepts `/product/:id` routes. It wraps the product page with a header and footer, and injects a "related products" banner below it — without any changes to the host's `ProductPage` component.

**1. Build the plugin**

```bash
cargo build -p route-injection-example-plugin --target wasm32-unknown-unknown --release
```

**2. Start the host**

```bash
cd examples/route-injection-example/host
dx serve
```

**3. Open the browser**

Navigate to [http://localhost:3010](http://localhost:3010), then click "Go to product 42".

Expected on the product page:
- `✨ Enhanced by plugin — product 42` header added by the plugin
- The original product page content in the middle (rendered by the host, unmodified)
- `Plugin: see also our bestsellers` footer added by the plugin
- `Related products — injected by plugin below the page` banner below everything

---

### tree-selector-example

Two plugins collaborating. `plugin_a` provides an "activity-feed" slot. `plugin_b` finds the `div[data-plugin-slot="feed-actions"]` node inside `plugin_a`'s output and injects a Share button into it — neither plugin knows about the host, and the host knows nothing about either plugin.

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

Navigate to [http://localhost:3010](http://localhost:3010).

Expected: an activity feed card with "Latest activity / User alice posted a comment." and a "👍 Like" button, plus a "🔗 Share — injected by plugin_b" button injected by `plugin_b` into the actions area.

---

### notes-plugin

A plugin that demonstrates `dx_invoke` — calling host-registered business logic from inside a WASM plugin. The plugin renders a per-article notes section. It reads a `current_page` key from its session state (set by the host before calling `render_slot`), fetches notes from a host-owned in-memory store via `dx_invoke("get_notes", …)`, and lets the user add new notes via `dx_invoke("add_note", …)`.

**Interaction model:** each interactive element in the plugin's `PluginView` (the input field, the "Add note" button) carries a `HandlerId`. When the user types or clicks, the frontend posts the event to the server via a server function. The plugin's `on_interaction` handler runs and returns a `ViewUpdate` — a partial re-render of the slot. Only the plugin's output updates; there is no full page reload.

**State scoping:** the plugin's notes are stored in global state (shared across sessions). The draft text uses per-session state so each user has an independent draft. State scope is declared in the plugin manifest; the host enforces it.

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

Navigate to [http://localhost:3010](http://localhost:3010), then click any article link.

Expected on an article page:
- Static article content rendered by the host
- A "Notes" section below the `<hr>`, contributed entirely by the plugin
- An input field + "Add note" button; typing and clicking adds notes that persist in the host store for the session
- Navigating to a different article shows that article's notes independently

---

### showcase

A blog platform that exercises every dioxus-extism capability in one place. Two
independent plugins extend the host: `showcase/comments` handles the comment
section and `showcase/stats` handles view counts and reactions. Together they
demonstrate slots, named API routes, plugin pages, global and session state,
interactions, event emission and subscription, hook handlers, route transforms,
`dx_invoke`, and cross-plugin state reads — without any plugin-specific code in
the host.

**1. Build both plugins**

```bash
cargo build -p showcase-plugin-comments --target wasm32-unknown-unknown --release
cargo build -p showcase-plugin-stats    --target wasm32-unknown-unknown --release
```

**2. Start the host**

```bash
cd examples/showcase/host
dx serve
```

**3. Open the browser**

Navigate to [http://localhost:3010](http://localhost:3010).

Expected:
- Home page has a "trending" banner injected before the page content by the stats plugin (route `inject-before` on `/`)
- Post pages show a stats slot (view count + reactions) and a comments slot (comment list + live draft form)
- Typing in the comment box updates the draft via `on_interaction` without a page reload; submitting adds the comment
- Submitting a comment emits `comment_posted`; the stats plugin subscribes to this event and increments its counters
- Viewing a post triggers the `hook_post_viewed` hook; the stats plugin intercepts it to record the view count
- Comments persist in global state (shared across all users); draft text is per-session
- `/p/comments` serves a plugin-declared page listing recent comments across all posts
- `/p/stats` serves a plugin-declared page with the full statistics dashboard
- `/api/comments/:slug` (GET) and `/api/comments` (POST) are named API routes declared by the comments plugin
- `/api/stats` (GET) is a named API route declared by the stats plugin

---

### ssr-example

Demonstrates the correct pattern for server-side rendering when plugins may contribute
slot content. Unlike the other examples this is a standalone binary — there is no
browser client or `dx serve`. It shows the three-step SSR flow:

1. Call `PluginRuntime::ssr_render_route()` to pre-fetch all async plugin data.
2. Wrap the page in `SsrPluginDataProvider` so child components can read from the
   pre-fetched data without making server function calls.
3. Call `dioxus_ssr::render_element()` (synchronous) to produce the final HTML string.

**1. Optionally build the hello-plugin (for non-empty output)**

```bash
cargo build -p hello-plugin-plugin --target wasm32-unknown-unknown --release
```

**2. Run the example**

```bash
cargo run -p ssr-example
```

Expected: prints a complete HTML document to stdout. If the hello-plugin WASM was built,
the `hello-slot` content appears inside `.slot-container`; otherwise the slot renders
empty with a warning logged.

---

## Plugin API at a glance

This section is a quick reference for plugin authors. See the `notes-plugin` and `showcase` examples for working code.

### Declare the plugin (implement traits in your WASM crate)

| Trait | Purpose |
|---|---|
| `DioxusPlugin::manifest()` | Declare slots, API routes, page routes, hooks, events, state scope, and required host capabilities |
| `SlotProvider` | Render content into a named slot |
| `HookHandler` | Intercept a named server-side hook; return `Continue`, `Cancel`, or `Replace(data)` |
| `EventSubscriber` | Receive a named event emitted by the host or another plugin |
| `InteractionHandler` | Handle button clicks / input events from the plugin's own UI; return `ViewUpdate` |
| `TransformProvider` | Provide wrap / inject-before / inject-after transforms for host components or routes |
| `OnLoad` / `OnUnload` | Lifecycle callbacks when the plugin pool starts and stops |

### Wire the exports (macros)

```
plugin!            — required; wires manifest + slot exports
hook_export!       — export a HookHandler
transform_export!  — export a TransformProvider
events_export!     — export an EventSubscriber
interactions_export! — export an InteractionHandler
api_route_fn!      — export a named API route handler
on_load_export!    — export an OnLoad handler
on_unload_export!  — export an OnUnload handler
```

### Inside a handler (host functions)

All state, event, and invocation calls go through unsafe FFI host functions exposed in
`dioxus_extism_pdk::host_fns`. Values are exchanged as JSON strings. Plugins typically
wrap these in small helper functions to handle (de)serialisation.

| Host function | What it does |
|---|---|
| `host_fns::dx_state_get(key)` | Read a per-session value (returns JSON string) |
| `host_fns::dx_state_set(key, json)` | Write a per-session value |
| `host_fns::dx_state_delete(key)` | Delete a per-session value |
| `host_fns::dx_global_state_get(key)` | Read a global value |
| `host_fns::dx_global_state_set(key, json)` | Write a global value |
| `host_fns::dx_emit_event(json)` | Emit a named event (JSON-serialised `PluginEvent`) |
| `host_fns::dx_invoke(name, json)` | Call a host-registered handler; returns JSON |
| `host_fns::dx_plugin_state_get(plugin_id, key)` | Read another plugin's state (requires `ReadPluginState` capability) |

All host functions are `unsafe` and return `Result<String, extism_pdk::Error>`. See
`examples/showcase/plugin-comments/src/lib.rs` for a typical wrapping pattern.

### Declaring API routes and plugin pages

In `DioxusPlugin::manifest()`:

```rust
api_routes: vec![
    ApiRouteDeclaration::get("/api/items/:id", "api_items_get"),
    ApiRouteDeclaration::post("/api/items", "api_items_post"),
],
page_routes: vec![PageRouteDeclaration {
    path: "/items".into(),
    title: Some("All Items".into()),
    render_fn: "render_items_page".into(),
    bypass_layout: false,
}],
```

The host mounts these on its Axum router automatically. `page_routes` paths are served
under the prefix configured via `PluginRuntimeBuilder::with_plugin_page_prefix` (default `/p`).

---

## Custom bind address / port

Pass `--addr` and `--port` to `dx serve`:

```bash
dx serve --addr 0.0.0.0 --port 3000
```

| Flag     | Default     | Description              |
|----------|-------------|--------------------------|
| `--addr` | `127.0.0.1` | IP address to bind on    |
| `--port` | `3010`      | TCP port to listen on    |

---

## Project structure

```
dioxus-extism/
├── crates/
│   ├── dioxus-extism-protocol/   # shared types (PluginView, SessionCtx, …)
│   ├── dioxus-extism-macros/     # proc-macro helpers for plugin authors
│   ├── dioxus-extism-host/       # PluginRuntime — loads and calls WASM plugins
│   ├── dioxus-extism-pdk/        # plugin development kit (WASM side)
│   ├── dioxus-extism-frontend/   # Dioxus components and server functions
│   └── dioxus-extism-test/       # integration test helpers
├── examples/
│   ├── hello-plugin/             # minimal slot example
│   ├── route-injection-example/  # route wrap + inject-after example
│   ├── tree-selector-example/    # two plugins, Within transform
│   ├── notes-plugin/             # dx_invoke + interactive slot (input/button)
│   ├── showcase/                 # comprehensive demo — all capabilities, two plugins
│   └── ssr-example/              # server-side rendering with ssr_render_route
└── dioxus-extism/                # thin re-export crate
```
