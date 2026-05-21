# showcase

A blog platform that exercises every dioxus-extism capability in one place. Two
independent plugins extend the host without any plugin-specific code in the host:

| Plugin | Capabilities |
|--------|-------------|
| `showcase/comments` | slot, API routes, plugin page, global state, session state, interactions, event emission, `dx_invoke` |
| `showcase/stats` | slot, API route, plugin page, event subscription, hook handler, route transform, global state, interactions, `on_load`, `dx_invoke`, `dx_plugin_state_get` |

**Concepts demonstrated:** all of the above plus `PluginAwareRouter`, `PluginPageOutlet`,
`PluginBootProvider`, `on_event`, `dx_emit_event`, `dx_plugin_state_get`,
`HookRegistration`, and cross-plugin data access.

## Structure

```
showcase/
├── plugin-comments/   # comments plugin (wasm32-unknown-unknown)
├── plugin-stats/      # stats plugin    (wasm32-unknown-unknown)
└── host/              # Dioxus fullstack host (dx serve)
```

## Prerequisites

- `rustup target add wasm32-unknown-unknown`
- `cargo install dioxus-cli`

## Run

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

Navigate to [http://localhost:8080](http://localhost:8080).

## Expected output

**Home page (`/`)**
- A "trending" banner injected by the stats plugin above the post list
- List of sample blog posts

**Post page (`/posts/:slug`)**
- Static post content (host)
- **Post stats** slot: per-post view count + like/dislike buttons (stats plugin)
- **Comments** slot: comment list + live draft form (comments plugin)

**Plugin pages**
- `/p/comments` — recent comments across all posts (comments plugin page)
- `/p/stats` — full statistics dashboard (stats plugin page)

**Cross-plugin interaction**
- Submitting a comment emits a `comment_posted` event
- The stats plugin receives it and updates its comment counts without a page reload
- Navigating to any post triggers the `post_viewed` hook; the stats plugin
  increments the view counter
