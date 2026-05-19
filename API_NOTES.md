# API Notes — Verified API Details

Verified: May 2025. Re-verify if dependency versions change.

---

## Extism (host SDK) — version 1.21.0

Repository: https://github.com/extism/extism
Docs: https://docs.rs/extism/latest/extism/

### Key types

```
Plugin        — a single WASM plugin instance (Send, NOT Sync)
PluginBuilder — constructs Plugin instances
Pool          — manages a set of Plugin instances for concurrent access (Arc internally, Clone)
PoolBuilder   — constructs Pool
PoolPlugin    — a plugin checked out from a Pool; returns to Pool on Drop
CompiledPlugin — pre-compiled WASM module (for fast pool construction)
CurrentPlugin  — available inside host functions only; access to WASM memory
Function       — a host function exposed to plugins
UserData<T>    — enum wrapping data passed to host functions (T: Clone + Send + Sync + 'static)
Manifest       — configures what WASM to load
Wasm           — specifies WASM source (file, bytes, URL)
CancelHandle   — cancels a running plugin call from another thread
```

### Plugin construction

```rust
use extism::{Plugin, PluginBuilder, Manifest, Wasm};

let wasm = Wasm::data(bytes);                         // from &[u8] or Vec<u8>
let wasm = Wasm::file("path/to/plugin.wasm");         // from file path
let wasm = Wasm::url("https://example.com/p.wasm");   // from URL

let manifest = Manifest::new([wasm]);

let plugin = PluginBuilder::new(manifest)
    .with_host_functions([fn1, fn2])    // Vec<Function>
    .build()?;                          // -> Result<Plugin, extism::Error>
```

### Pool construction (use this instead of Arc<Mutex<Plugin>>)

```rust
use extism::{Pool, PoolBuilder};

// Build a Pool from a pre-built Plugin or from a Manifest:
let pool = PoolBuilder::new_with_count(manifest, pool_size)?;
// or from a CompiledPlugin for faster instance creation:
let compiled = CompiledPlugin::new(manifest)?;
let pool = PoolBuilder::new_from_compiled_with_count(compiled, pool_size)?;

// Attach host functions:
// NOTE: verify exact PoolBuilder API — method names may differ from PluginBuilder.
// Check docs.rs/extism/latest/extism/struct.PoolBuilder.html for current API.

// Get a plugin instance (blocks until available):
let mut plugin: extism::PoolPlugin = pool.get()?;

// Call an export (PoolPlugin has same call() interface as Plugin):
let result: extism::convert::Json<MyOutput> =
    plugin.call("export_name", extism::convert::Json(my_input))?;

// plugin is automatically returned to the pool on Drop
```

**IMPORTANT:** Verify the exact `PoolBuilder` method signatures before implementing.
The API may require host functions to be passed differently than `PluginBuilder`.
Check: https://docs.rs/extism/latest/extism/struct.PoolBuilder.html

### Plugin call

```rust
// call() requires &mut self
// Input: implements ToBytes, Output: implements FromBytes
let output: Vec<u8> = plugin.call("export_name", input_bytes)?;

// JSON I/O (most common for this project):
use extism::convert::Json;
let output: Json<MyOutput> = plugin.call("export_name", Json(&my_input))?;
let value: MyOutput = output.0;

// Check if an export exists (before calling optional exports like on_load):
let exists: bool = plugin.function_exists("on_load");
```

### Host functions

```rust
use extism::{Function, CurrentPlugin, Val, ValType, UserData};

// UserData<T> is an enum in Extism 1.x — T must be: Clone + Send + Sync + 'static
#[derive(Clone)]
struct CallCtx { /* ... */ }

let user_data = UserData::new(ctx);  // wraps ctx in UserData::T(Arc<Mutex<ctx>>)

let my_fn = Function::new(
    "function_name_as_seen_by_plugin",
    [ValType::PTR],           // input types  (PTR = pointer/len pair for strings)
    [ValType::PTR],           // output types
    user_data,
    |plugin: &mut CurrentPlugin,
     inputs: &[Val],
     outputs: &mut [Val],
     user_data: UserData<CallCtx>| {
        // Extract context:
        let ctx = match &user_data {
            UserData::T(inner) => inner.lock().unwrap(),
            _ => return,
        };

        // Read string input from WASM memory:
        let input: &str = plugin.input().expect("valid input");
        // or for raw bytes: plugin.input_bytes()

        // Write string output:
        plugin.output("result string").expect("output written");
        // or for bytes: plugin.output_bytes(bytes.as_slice())
    },
);
```

**Verify:** `CurrentPlugin::input()` and `CurrentPlugin::output()` method names — they
may be `plugin.input::<T>()` / `plugin.output(value)` using the convert traits.
Check: https://docs.rs/extism/latest/extism/struct.CurrentPlugin.html

### Resource limits

Extism 1.x does NOT expose Wasmtime fuel via the public API.

Available limits:
- **`plugin.set_deadline(instant)`** — cancels the plugin call at a wall-clock time.
  Only available on `Plugin`, not on `PoolPlugin`. Investigate if `PoolPlugin` exposes it.
- **`CancelHandle`** — get via `plugin.cancel_handle()`, call `handle.cancel()` from
  another thread. Use with `tokio::time::sleep + spawn` for wall-clock timeouts.

Recommended approach for pool-based calls:
```rust
// Spawn a cancel task alongside the plugin call
let handle = plugin.cancel_handle();
let cancel_task = tokio::spawn(async move {
    tokio::time::sleep(max_duration).await;
    handle.cancel();
});
let result = plugin.call(...);
cancel_task.abort();
result
```

Note: this requires access to the `PoolPlugin` to get its cancel handle, which means
the cancellation logic must be inside the `spawn_blocking` closure alongside the call.

### Plugin Send/Sync
- `Plugin: Send` — YES
- `Plugin: Sync` — NO
- `Pool: Send + Sync` — YES (it's Arc internally)
- `PoolPlugin: Send` — YES; `PoolPlugin: Sync` — NO

---

## Extism PDK — version 1.4.1

Repository: https://github.com/extism/extism/tree/main/runtime
Docs: https://docs.rs/extism-pdk/latest/extism_pdk/

### Plugin export

```rust
use extism_pdk::*;

#[plugin_fn]
pub fn my_export(input: Json<MyInput>) -> FnResult<Json<MyOutput>> {
    Ok(Json(MyOutput { /* ... */ }))
}
```

`FnResult<T>` is `Result<T, extism_pdk::Error>`.

### Host function import

```rust
#[host_fn]
extern "ExtismHost" {
    fn dx_state_get(key: &str) -> String;
    fn dx_state_set(key: &str, value: String);
    fn dx_emit_event(event: Json<PluginEvent>);
    fn dx_invoke(name: &str, args: String) -> String;
    fn dx_log(level: &str, message: &str);
}

// Call (unsafe):
let value = unsafe { dx_state_get("my_key")? };
```

---

## Dioxus — version 0.7.9

Repository: https://github.com/DioxusLabs/dioxus
Key changes from 0.6: `use_server_future` renamed to `use_resource`, new fullstack
launch API, Fullstack WebSockets, improved server function extraction.

### Launch (0.7)

```rust
fn main() {
    dioxus::LaunchBuilder::new()
        .with_cfg(server_only!(
            dioxus::fullstack::Config::new()
                .with_axum_state(runtime)  // Arc<PluginRuntime>
        ))
        .launch(App);
}
```

### Server functions (0.7)

```rust
use dioxus::prelude::*;

#[server]
async fn my_fn(arg: String) -> Result<MyOutput, ServerFnError> {
    // Extract Axum state:
    let State(runtime) = extract::<State<Arc<PluginRuntime>>>().await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    Ok(/* ... */)
}
```

### Hooks (0.7)

```rust
// Async data fetching — use use_resource (was use_server_future in 0.5/0.6)
let data = use_resource(move || async move {
    my_server_fn(arg.clone()).await
});

// Read result:
match data.read().as_ref() {
    None          => rsx! { "Loading..." },
    Some(Ok(v))   => rsx! { "{v}" },
    Some(Err(e))  => rsx! { "Error: {e}" },
}

// Context
provide_context(MyValue { });
let val = use_context::<MyValue>();
```

### SSR (0.7)

```rust
// dioxus_ssr::render is SYNCHRONOUS
// Pre-fetch all async data before calling it
let html: String = dioxus_ssr::render(rsx! { MyComponent {} });
```

### EventSource (SSE client)

Dioxus 0.7 does NOT include a built-in SSE client.
- **Web (wasm32):** Use `web_sys::EventSource` directly.
- **Desktop:** Use polling via `use_resource` on a timer as a fallback.

```rust
// Web-only SSE:
#[cfg(target_arch = "wasm32")]
fn connect_sse(url: &str) -> web_sys::EventSource {
    web_sys::EventSource::new(url).expect("EventSource")
}
```

---

## Axum — version 0.8.6

### SSE endpoint

```rust
use axum::{routing::get, response::sse::{Event, Sse}, extract::State};
use futures::stream::Stream;
use std::convert::Infallible;

async fn sse_handler(
    State(runtime): State<Arc<PluginRuntime>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = runtime.override_map_updates();

    let stream = async_stream::stream! {
        while let Ok(map) = rx.recv().await {
            if let Ok(data) = serde_json::to_string(&map) {
                yield Ok(Event::default().data(data));
            }
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

// Register:
router.route("/_dioxus_extism/override_map_updates", get(sse_handler))
```

Axum 0.8 uses `http` 1.0 and `hyper` 1.0. State is registered via
`.with_state(value)` on the router, or via the Dioxus fullstack integration.

---

## tokio — version 1.52.3

```rust
use tokio::sync::{RwLock, broadcast};

// broadcast channel for OverrideMap hot-reload notifications
let (tx, _rx) = broadcast::channel::<OverrideMap>(32);
let rx = tx.subscribe();  // subscribe new receivers later
let _ = tx.send(map);     // non-blocking; drops oldest if full

// spawn_blocking for sync work
tokio::task::spawn_blocking(move || { /* ... */ }).await
```

Use `std::sync::Mutex` (not `tokio::sync::Mutex`) for anything inside `spawn_blocking`.
`tokio::sync::MutexGuard` cannot be sent across blocking threads.

---

## Additional crates

### sha2 (0.10) — SHA-256 for URL plugin integrity

```rust
use sha2::{Sha256, Digest};
let mut hasher = Sha256::new();
hasher.update(&bytes);
let result: [u8; 32] = hasher.finalize().into();
```

### dirs (5) — OS-appropriate paths for DesktopSessionProvider

```rust
let data_dir = dirs::data_local_dir()
    .expect("OS has data local dir")
    .join("my-app")
    .join("dioxus-extism");
```

### keyring (3) — OS keychain for MobileSessionProvider

```rust
let entry = keyring::Entry::new("my-app", "dioxus_plugin_session")?;
let id = entry.get_password().unwrap_or_else(|_| {
    let new_id = uuid::Uuid::new_v4().to_string();
    entry.set_password(&new_id).ok();
    new_id
});
```

### fd-lock (4) — File locking for DesktopSessionProvider

```rust
use fd_lock::RwLock as FdRwLock;
let file = std::fs::OpenOptions::new().read(true).write(true).create(true).open(&path)?;
let mut lock = FdRwLock::new(file);
let mut guard = lock.write()?;
// write to guard via std::io::Write
```

---

## Things to verify before starting each phase

Run these before Phase 1:
```bash
# Confirm PoolBuilder API exists and check method signatures
cargo doc --open -p extism

# Confirm dioxus 0.7 use_resource hook name
cargo doc --open -p dioxus

# Confirm axum 0.8 SSE API
cargo doc --open -p axum
```

If any API differs from this document, update this file immediately before proceeding.
