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
│    PluginRuntime (Arc<PluginRuntime>)        │
│      └─ calls plugin exports via extism     │
└──────────────────┬──────────────────────────┘
                   │ WASM function calls
┌──────────────────▼──────────────────────────┐
│  Plugin (wasm32-unknown-unknown .wasm)       │
│  Returns PluginView — serialised UI tree     │
└─────────────────────────────────────────────┘
```

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

Navigate to [http://localhost:8080](http://localhost:8080).

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

Navigate to [http://localhost:8080](http://localhost:8080), then click "Go to product 42".

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

Navigate to [http://localhost:8080](http://localhost:8080).

Expected: an activity feed card with "Latest activity / User alice posted a comment." and a "👍 Like" button, plus a "🔗 Share — injected by plugin_b" button injected by `plugin_b` into the actions area.

---

## Custom bind address / port

Pass `--addr` and `--port` to `dx serve`:

```bash
dx serve --addr 0.0.0.0 --port 3000
```

| Flag     | Default     | Description              |
|----------|-------------|--------------------------|
| `--addr` | `127.0.0.1` | IP address to bind on    |
| `--port` | `8080`      | TCP port to listen on    |

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
│   └── tree-selector-example/    # two plugins, Within transform
└── dioxus-extism/                # thin re-export crate
```
