# dioxus-extism — Claude Code Instructions

## Project Overview

`dioxus-extism` is a Rust workspace that extends Dioxus fullstack applications with
Extism WASM plugins. Plugins run server-side only, describe their UI as a serialisable
`PluginView` tree, and the host Dioxus frontend renders it.

Architecture document: `dioxus-extism-architecture.md` — read this fully before writing
any code. Task files for each phase are in `tasks/`.

---

## Dependency Versions (verified May 2025)

```toml
[workspace.dependencies]
extism        = "1.21"
extism-pdk    = "1.4"
dioxus        = { version = "0.7", features = ["fullstack"] }
dioxus-ssr    = "0.7"
axum          = "0.8"
tokio         = { version = "1", features = ["full"] }
serde         = { version = "1", features = ["derive"] }
serde_json    = "1"
thiserror     = "2"
indexmap      = { version = "2", features = ["serde"] }
async-trait   = "0.1"
uuid          = { version = "1", features = ["v4"] }
sha2          = "0.10"
futures       = "0.3"
tracing       = "0.1"
syn           = { version = "2", features = ["full"] }
quote         = "1"
proc-macro2   = "1"
reqwest       = { version = "0.12", features = ["json"] }
dirs          = "5"
keyring       = "3"
fd-lock       = "4"
trybuild      = "1"
async-stream  = "0.3"
```

Do not change these versions without updating this file and `API_NOTES.md`.

---

## Workspace Structure and Build Order

```
dioxus-extism/
├── crates/
│   ├── dioxus-extism-protocol/   <- build FIRST (no deps on extism or dioxus)
│   ├── dioxus-extism-macros/     <- build SECOND (proc-macro crate)
│   ├── dioxus-extism-host/       <- build THIRD  (depends on extism + protocol)
│   ├── dioxus-extism-pdk/        <- build THIRD  (depends on extism-pdk + protocol)
│   ├── dioxus-extism-frontend/   <- build FOURTH (depends on dioxus + macros + protocol)
│   └── dioxus-extism-test/       <- build FIFTH  (depends on host + pdk + protocol)
├── dioxus-extism/                <- thin re-export (build LAST)
└── examples/
```

Never write code for a crate before all its dependencies compile cleanly.

---

## Working Through Phases

Each phase has a task file in `tasks/`. Before starting any phase:
1. Read the task file completely.
2. Read `API_NOTES.md` for all crates used in that phase.
3. Run `cargo check --workspace` to confirm the baseline compiles.
4. Stop and report if any verification step fails unexpectedly.

Do not begin a phase until the previous phase's stop condition is confirmed.

---

## Code Conventions (non-negotiable)

### Errors
- Library code: `thiserror` 2.x only. Never `anyhow` in lib crates.
- No `.unwrap()` in library code. Use `?` or structured error variants.
- `.expect("...")` is allowed only in `examples/` and `#[cfg(test)]` blocks.
  The message must explain why the failure is impossible.

### Public API
- All public enums carry `#[non_exhaustive]`. No exceptions.
- All public types derive `Debug`. Derive `Clone` only if semantically meaningful.
- All public items have `///` doc comments with at least one sentence.
- Prefer `impl Into<String>` over `String` in constructor arguments.
- Never expose internal types (registries, pool internals) in the public API.

### Async
- All Extism plugin calls go through `tokio::task::spawn_blocking`. See Pattern 1.
- Never hold a `tokio::sync::MutexGuard` across an `.await` point.
- Use `tokio::sync::RwLock` for read-heavy shared state (registries).
- Use `extism::Pool` for plugin instance management — do NOT build a custom pool.

### Imports
- Group: std -> external crates -> internal crates -> local modules.
- No glob imports in library code (`use crate::*` is forbidden).

### Testing
- Write tests before implementing. A failing test is progress.
- Integration tests live in `crates/<name>/tests/<module>.rs`.
- Unit tests in `#[cfg(test)] mod tests {}` within the source file.

---

## Critical Implementation Patterns

### Pattern 1: Plugin call via Pool + spawn_blocking

Extism 1.x ships a built-in `Pool` / `PoolPlugin`. Use it instead of any custom
`Vec<Arc<Mutex<Plugin>>>` scheme. The pool handles concurrent access, round-robin
selection, and automatic return-to-pool on `PoolPlugin` drop.

```rust
use extism::{Pool, PoolPlugin};

struct LoadedPlugin {
    manifest: PluginManifest,
    pool: extism::Pool,        // Extism built-in pool (Arc internally, cheap to clone)
    enabled: std::sync::atomic::AtomicBool,
    config: PluginInstallConfig,
}

async fn call_export<I, O>(
    pool: extism::Pool,   // Pool is Clone
    export: String,
    input: I,
) -> Result<O, PluginRuntimeError>
where
    I: extism::convert::ToBytes<'static> + Send + 'static,
    O: for<'a> extism::convert::FromBytes<'a> + Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        // pool.get() is synchronous — blocks until an instance is available.
        // This is why the whole thing must be inside spawn_blocking.
        let mut plugin: PoolPlugin = pool
            .get()
            .map_err(|e| PluginRuntimeError::CallFailed { source: e })?;
        plugin
            .call::<I, O>(&export, input)
            .map_err(|e| PluginRuntimeError::CallFailed { source: e })
    })
    .await
    .map_err(|e| PluginRuntimeError::TaskPanic(e.to_string()))?
}
```

### Pattern 2: UserData in host functions

In Extism 1.x, `UserData` is an **enum**. Use `UserData::new()` to wrap data:

```rust
#[derive(Clone)]  // required: cloned per host function invocation
struct CallCtx {
    pub session_id: SessionId,
    /// Which plugin is invoking this host function.
    /// Used for capability checks — without this, all plugins bypass capability enforcement.
    pub caller: Option<PluginId>,
    pub client: ClientCapabilities,
    pub session_states: Arc<tokio::sync::RwLock<SessionStateMap>>,
    pub global_states: Arc<tokio::sync::RwLock<GlobalStateMap>>,
    pub invocation_registry: Arc<InvocationRegistry>,
}

let host_fn = extism::Function::new(
    "dx_state_get",
    [extism::ValType::PTR],
    [extism::ValType::PTR],
    UserData::new(ctx),
    |plugin: &mut extism::CurrentPlugin,
     inputs: &[extism::Val],
     outputs: &mut [extism::Val],
     user_data: UserData<CallCtx>| {
        let ctx = match &user_data {
            UserData::T(inner) => inner.lock().unwrap(),
            _ => return,  // should not happen
        };
        // capability check using ctx.caller before doing anything
        // read input, write output via plugin.memory_* methods
    },
);
```

### Pattern 3: Atomic registry rebuild for hot-reload

```rust
pub async fn reload_plugin(
    &self, id: &PluginId, source: PluginSource, config: PluginInstallConfig,
) -> Result<(), PluginRuntimeError> {
    // Step 1: Build new Pool OUTSIDE the write lock (WASM compilation is expensive)
    let new_pool = self.build_pool(&source, &config).await?;
    let new_manifest = self.read_manifest_from_pool(&new_pool).await?;

    // Step 2: on_unload — outside lock, best-effort, never fail
    {
        let plugins = self.plugins.read().await;
        if let Some(old) = plugins.get(id) {
            self.try_on_unload(old).await;
        }
    }  // read lock released

    // Step 3: Atomic swap — single write lock, single rebuild, version bump
    let new_map = {
        let mut plugins = self.plugins.write().await;
        let mut regs = self.registries.write().await;
        plugins.insert(id.clone(), LoadedPlugin {
            manifest: new_manifest,
            pool: new_pool,
            enabled: AtomicBool::new(true),
            config,
        });
        *regs = self.build_registries(&plugins);
        regs.override_map.version += 1;
        regs.override_map.clone()
    };  // both locks released here

    // Step 4: Broadcast AFTER releasing locks
    let _ = self.override_map_tx.send(new_map);
    Ok(())
}
```

Lock acquisition order (never reverse this):
1. `plugins`
2. `registries`
3. `session_states`
4. `global_states`

### Pattern 4: Dioxus 0.7 server functions

```rust
use dioxus::prelude::*;

#[server]
async fn get_slot_content(
    slot: String,
    session_id: SessionId,
    client: ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError> {
    let State(runtime) = extract::<State<Arc<PluginRuntime>>>().await
        .map_err(|e| ServerFnError::new(e.to_string()))?;
    let session = build_session(session_id, client);
    runtime.render_slot(&slot, &session).await
        .map_err(|e| ServerFnError::new(e.to_string()))
}
```

Dioxus 0.7 uses `use_resource` (not `use_server_future`, which was 0.5/0.6 API):

```rust
let slot_content = use_resource(move || {
    let slot = slot.clone();
    let sid = session_id.read().clone();
    let caps = client_caps.clone();
    async move { get_slot_content(slot, sid, caps).await }
});

match slot_content.read().as_ref() {
    None => rsx! { /* loading state */ },
    Some(Ok(contents)) => rsx! { /* render */ },
    Some(Err(e)) => rsx! { /* error */ },
}
```

Register Axum state in Dioxus 0.7 fullstack:

```rust
dioxus::LaunchBuilder::new()
    .with_cfg(server_only!(
        dioxus::fullstack::Config::new().with_axum_state(runtime)
    ))
    .launch(App);
```

### Pattern 5: dx_invoke (async from sync host function)

Extism host functions are synchronous. To call async invocation handlers:

```rust
fn dx_invoke_impl(
    plugin: &mut extism::CurrentPlugin,
    inputs: &[extism::Val],
    outputs: &mut [extism::Val],
    user_data: UserData<CallCtx>,
) {
    let handle = tokio::runtime::Handle::current();
    let ctx = /* extract from user_data */;

    // Capability check first
    // ...

    let result = handle.block_on(async {
        ctx.invocation_registry.call(name, args, ctx.session.clone()).await
    });

    // write result to WASM memory
}
```

---

## Known Pitfalls

### Pitfall 1: Pool::get() blocks — must be inside spawn_blocking
`pool.get()` is synchronous. Calling it in async code starves the Tokio executor.
Always: `tokio::task::spawn_blocking(move || { let mut p = pool.get()?; ... })`.

### Pitfall 2: UserData is an enum in Extism 1.x
`UserData<T>` is an enum. Pattern-match on `UserData::T(inner)` to access the data.
Do not treat it as a struct. See API_NOTES.md for the exact pattern.

### Pitfall 3: thiserror 2.x
This project uses `thiserror = "2"`. The `#[error]` and `#[from]` attributes work the
same as 1.x but the error output format has minor improvements. Do not pin to 1.x.

### Pitfall 4: protocol crate must compile for wasm32
After every change to `dioxus-extism-protocol`, run:
`cargo check -p dioxus-extism-protocol --target wasm32-unknown-unknown`

### Pitfall 5: proc-macro crate cannot import protocol types
`dioxus-extism-macros` generates code that *uses* protocol types, but the macro crate
cannot import them as Rust types at expansion time. Re-express as token streams.

### Pitfall 6: Dioxus 0.7 SSE
`EventSource` (SSE client) is web-only (WASM target). For desktop hot-reload, poll
via `use_resource` on a timer. Gate with `#[cfg(target_arch = "wasm32")]`.

### Pitfall 7: dioxus-ssr is synchronous
`dioxus_ssr::render` is sync. All async plugin calls (slot content, transforms) must
complete before calling it. Use `ssr_render_route` to pre-fetch, then pass as context
into the synchronous SSR render pass.

---

## Verification Commands

```bash
cargo check --workspace
cargo test --workspace --lib
cargo check -p dioxus-extism-protocol --target wasm32-unknown-unknown
cargo clippy --workspace -- -D warnings
cargo doc --workspace --no-deps
```
