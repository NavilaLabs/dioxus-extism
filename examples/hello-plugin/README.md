# hello-plugin

The simplest dioxus-extism example. A single plugin registers a `hello-slot` and
returns a greeting `<div>`. The host declares `<PluginSlot name="hello-slot" />`
and nothing else — all plugin content is contributed at runtime from the WASM
sandbox.

**Concepts demonstrated:** slot registration, `SlotProvider`, `PluginSlot`.

## Structure

```
hello-plugin/
├── plugin/   # WASM plugin (wasm32-unknown-unknown)
└── host/     # Dioxus fullstack host (dx serve)
```

## Prerequisites

- `rustup target add wasm32-unknown-unknown`
- `cargo install dioxus-cli`

## Run

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

## Expected output

- `Hello Plugin Example` as the static heading (from the host)
- `Hello from a WASM plugin!` inside a `div.hello-from-plugin` (from the plugin)
