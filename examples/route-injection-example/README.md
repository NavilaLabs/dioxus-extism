# route-injection-example

A plugin that intercepts `/product/:id` routes without touching the host's
`ProductPage` component. It registers two transforms on the same route pattern:
one that wraps the page (adding a header and footer around the host outlet) and
one that injects a banner after the page.

**Concepts demonstrated:** `TransformOp::Wrap`, `TransformOp::InjectAfter`,
`original_content()`, route params in `TransformInput`, `PluginAwareRouter`.

## Structure

```
route-injection-example/
├── plugin/   # WASM plugin (wasm32-unknown-unknown)
└── host/     # Dioxus fullstack host (dx serve)
```

## Prerequisites

- `rustup target add wasm32-unknown-unknown`
- `cargo install dioxus-cli`

## Run

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

Navigate to [http://localhost:3010](http://localhost:3010), then click
**Go to product 42**.

## Expected output

On the product page:

| Source | Content |
|--------|---------|
| Plugin wrap — header | `✨ Enhanced by plugin — product 42` |
| Host | `Product #42` — the original product page, unmodified |
| Plugin wrap — footer | `Plugin: see also our bestsellers` |
| Plugin inject-after | `Related products — injected by plugin below the page` |
