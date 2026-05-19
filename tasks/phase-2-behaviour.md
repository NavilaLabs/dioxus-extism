# Phase 2 — Behaviour: Hooks, Events, Interactions, Invocations, Tests

**Prerequisite:** Phase 1 stop condition confirmed.

**Goal:** Wire actual WASM loading via Extism Pool, implement all host functions with
real logic, hook chains, event bus, interactions, invocations, per-plugin error
isolation, and the test crate. The `hook-example` and `invocation-example` work.

**Stop condition:** `cargo check --workspace && cargo test --workspace --lib` passes.

---

## Step 1 — Wire WASM loading in build()

Now make `PluginRuntimeBuilder::build()` actually load WASM using `extism::Pool`.

### First, verify the PoolBuilder API:
```bash
cargo doc --open -p extism
# Navigate to PoolBuilder and check exact method signatures before writing any code
```

### Write tests first in `crates/dioxus-extism-host/tests/plugin_loading.rs`:

```rust
const HELLO_WASM: &[u8] = include_bytes!(
    "../../target/wasm32-unknown-unknown/release/hello_plugin_plugin.wasm"
);

#[tokio::test]
async fn loads_plugin_and_reads_manifest() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HELLO_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let map = runtime.override_map();
    // hello-plugin registers "hello-slot"
    assert!(map.version == 0);
}

#[tokio::test]
async fn rejects_plugin_with_too_high_protocol_version() {
    // If a plugin's min_protocol_version > PROTOCOL_VERSION, build() should fail
    // (difficult to test without a real plugin — leave as TODO for now)
}
```

### Implement build():

```rust
pub async fn build(self) -> Result<Arc<PluginRuntime>, PluginRuntimeError> {
    let (override_map_tx, _) = tokio::sync::broadcast::channel(32);
    let mut all_plugins: indexmap::IndexMap<PluginId, LoadedPlugin> = Default::default();

    for (source, config) in self.sources {
        // 1. Fetch bytes and verify SHA-256 if Url variant
        let bytes = self.fetch_and_verify(&source).await?;

        // 2. Build Extism Manifest
        let wasm = extism::Wasm::data(bytes);
        let manifest = extism::Manifest::new([wasm]);

        // 3. Create pool
        let pool_size = config.pool_size
            .unwrap_or_else(|| std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4));

        // Construct pool — verify exact PoolBuilder API from docs
        let host_fns = make_host_functions(/* runtime ref */);
        let pool = extism::PoolBuilder::new_with_count(manifest, pool_size)
            // attach host functions — check PoolBuilder docs for exact method
            .build()
            .map_err(|e| PluginRuntimeError::CallFailed { source: e })?;

        // 4. Read manifest from pool (call "manifest" export on one instance)
        let plugin_manifest = {
            let pool_clone = pool.clone();
            tokio::task::spawn_blocking(move || {
                let mut p = pool_clone.get().map_err(|e| {
                    PluginRuntimeError::CallFailed { source: e }
                })?;
                p.call::<(), extism::convert::Json<PluginManifest>>("manifest", ())
                    .map(|r| r.0)
                    .map_err(|e| PluginRuntimeError::CallFailed { source: e })
            })
            .await
            .map_err(|e| PluginRuntimeError::TaskPanic(e.to_string()))??
        };

        // 5. Protocol version check
        if plugin_manifest.min_protocol_version > PROTOCOL_VERSION {
            return Err(PluginRuntimeError::ProtocolIncompatible {
                plugin: plugin_manifest.id.clone(),
                required: plugin_manifest.min_protocol_version,
                supported: PROTOCOL_VERSION,
            });
        }

        // 6. Validate Invoke capabilities
        for cap in &plugin_manifest.host_capabilities {
            if let HostCapability::Invoke { names } = cap {
                for name in names {
                    if !self.invocations.contains_key(name.as_str()) {
                        return Err(PluginRuntimeError::UnknownInvocation {
                            plugin: plugin_manifest.id.clone(),
                            name: name.clone(),
                        });
                    }
                }
            }
        }

        // 7. Call on_load if exported
        if self.should_call_on_load(&pool).await? {
            // call "on_load" export
        }

        all_plugins.insert(plugin_manifest.id.clone(), LoadedPlugin {
            manifest: plugin_manifest,
            pool,
            enabled: AtomicBool::new(true),
            config,
        });
    }

    let registries = build_registries(&all_plugins);
    let (tx, _) = tokio::sync::broadcast::channel(32);

    Ok(Arc::new(PluginRuntime {
        plugins: RwLock::new(all_plugins),
        registries: RwLock::new(registries),
        override_map_tx: tx,
        // ... other fields
    }))
}
```

---

## Step 2 — Real host functions with CallCtx

Replace all stubs. `CallCtx` carries the caller's `PluginId` for capability checks.

```rust
#[derive(Clone)]
pub(crate) struct CallCtx {
    pub session_id: SessionId,
    pub caller: Option<PluginId>,
    pub client: ClientCapabilities,
    pub session_states: Arc<RwLock<SessionStateMap>>,
    pub global_states: Arc<RwLock<GlobalStateMap>>,
    pub invocation_registry: Arc<InvocationRegistry>,
}
```

Implement each host function. Pattern for `dx_state_get`:

```rust
fn make_state_get(runtime_ctx: Arc<RwLock<SessionStateMap>>) -> extism::Function {
    let ctx = CallCtx { session_states: runtime_ctx, /* ... */ };
    extism::Function::new(
        "dx_state_get",
        [extism::ValType::PTR],
        [extism::ValType::PTR],
        extism::UserData::new(ctx),
        |plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         _outputs: &mut [extism::Val],
         user_data: extism::UserData<CallCtx>| {
            let ctx = match &user_data {
                extism::UserData::T(inner) => inner.lock().unwrap(),
                _ => return,
            };
            let key: String = plugin.input().unwrap_or_default();
            let handle = tokio::runtime::Handle::current();
            let result = handle.block_on(async {
                let states = ctx.session_states.read().await;
                states
                    .get(&ctx.session_id)
                    .and_then(|s| s.get(ctx.caller.as_ref()?))
                    .and_then(|p| p.get(&key))
                    .cloned()
            });
            let json = serde_json::to_string(&result).unwrap_or("null".into());
            plugin.output(json.as_str()).ok();
        },
    )
}
```

Implement all 10 host functions with proper capability checks for gated ones.

---

## Step 3 — Hook chain

Implement `PluginRuntime::run_hook`. Error isolation is critical:

```rust
pub async fn run_hook<T>(
    &self,
    hook_name: &str,
    context: T,
    session: &SessionCtx,
) -> Result<HookOutcome<T>, PluginRuntimeError>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
{
    let entries = {
        let regs = self.registries.read().await;
        regs.hooks.get(hook_name).cloned().unwrap_or_default()
    };

    let mut current = serde_json::to_value(&context)?;

    for (_, plugin_id) in &entries {
        let (pool, enabled) = {
            let plugins = self.plugins.read().await;
            match plugins.get(plugin_id) {
                Some(p) => (p.pool.clone(), p.enabled.load(Ordering::Relaxed)),
                None => continue,
            }
        };
        if !enabled { continue }

        let input = HookCall {
            hook_name: hook_name.to_owned(),
            context: current.clone(),
        };

        let result = call_export::<Json<HookCall>, Json<HookResult>>(
            pool,
            format!("hook_{hook_name}"),
            Json(input),
        ).await;

        match result {
            Ok(Json(HookResult::Continue { context: c })) => current = c,
            Ok(Json(HookResult::Replace  { context: c })) => current = c,
            Ok(Json(HookResult::Cancel { reason })) => {
                return Ok(HookOutcome::Cancelled { by: plugin_id.clone(), reason });
            }
            Err(e) => {
                // Error isolation: log and continue to next plugin
                tracing::warn!(plugin = %plugin_id.0, error = %e, "hook call failed, skipping");
            }
        }
    }

    Ok(HookOutcome::Passed(serde_json::from_value(current)?))
}
```

---

## Step 4 — Event bus

```rust
pub(crate) struct EventBus {
    subscribers: std::collections::HashMap<String, Vec<(i32, PluginId)>>,
}

impl EventBus {
    pub async fn dispatch(
        &self,
        event: &PluginEvent,
        runtime: &PluginRuntime,
        session: &SessionCtx,
    ) {
        let subs = self.subscribers.get(&event.name).cloned().unwrap_or_default();
        for (_, plugin_id) in &subs {
            let pool = {
                let plugins = runtime.plugins.read().await;
                match plugins.get(plugin_id) {
                    Some(p) if p.enabled.load(Ordering::Relaxed) => p.pool.clone(),
                    _ => continue,
                }
            };
            // Error isolation: log + continue
            let _ = call_export::<Json<(PluginEvent, SessionCtx)>, ()>(
                pool,
                "on_event".into(),
                Json((event.clone(), session.clone())),
            ).await;
        }
    }
}
```

Wire `dx_emit_event` host function to call `event_bus.dispatch()` via `block_on`.

---

## Step 5 — Interaction handling

Implement `PluginRuntime::handle_interaction` and wire `dx_handle_interaction`
server function in the frontend. After a successful `ViewUpdate`, the frontend
`use_resource` re-reads the slot and `PluginViewRenderer` applies keyed diff.

---

## Step 6 — InvocationRegistry with timeout

```rust
type BoxHandler = Box<
    dyn Fn(serde_json::Value, SessionCtx)
        -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<serde_json::Value, InvocationError>> + Send>>
        + Send + Sync,
>;

pub struct InvocationRegistry {
    handlers: std::collections::HashMap<String, (BoxHandler, std::time::Duration)>,
}

impl InvocationRegistry {
    pub async fn call(
        &self,
        name: &str,
        args: serde_json::Value,
        session: SessionCtx,
    ) -> Result<serde_json::Value, InvocationError> {
        let (handler, timeout) = self.handlers.get(name)
            .ok_or_else(|| InvocationError::NotFound(name.into()))?;
        tokio::time::timeout(*timeout, handler(args, session))
            .await
            .map_err(|_| InvocationError::Timeout(*timeout))?
    }
}
```

`dx_invoke` host function: capability check, then `block_on(invocation_registry.call(...))`.

---

## Step 7 — Per-plugin error isolation in all pipelines

Every plugin call in `render_slot`, `render_route_transforms`, and `resolve_component`
must be isolated:

```rust
match call_export::<_, _>(pool, export, input).await {
    Ok(output) => { /* use output */ }
    Err(e) => {
        tracing::warn!(plugin = %plugin_id.0, error = %e, "transform failed, skipping");
        // For slot contributions: emit Incompatible view instead
        // For transforms: skip this transform, continue with next
        // For hooks: skip this plugin, continue chain
    }
}
```

---

## Step 8 — `dioxus-extism-test` crate

```rust
pub struct TestRuntime {
    runtime: Arc<PluginRuntime>,
    rt: tokio::runtime::Runtime,
}

impl TestRuntime {
    pub fn build(plugins: Vec<PluginSource>) -> Result<Self, PluginRuntimeError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all().build().unwrap();
        let runtime = rt.block_on(
            PluginRuntimeBuilder::new().add_plugins(plugins).build()
        )?;
        Ok(Self { runtime, rt })
    }

    pub fn call_slot(
        &self, plugin_id: &PluginId, slot_name: &str, session: &MockSession,
    ) -> Result<PluginView, PluginRuntimeError> {
        self.rt.block_on(async {
            let contents = self.runtime.render_slot(slot_name, &session.as_ctx()).await?;
            contents.into_iter()
                .find(|c| &c.plugin_id == plugin_id)
                .map(|c| c.view)
                .ok_or_else(|| PluginRuntimeError::PluginNotFound(plugin_id.clone()))
        })
    }

    // also: call_hook, call_transform, render_slot, session_state, emitted_events
    // also: mock_invocation for registering test handlers
}

pub struct MockSession {
    pub session_id: SessionId,
    pub user_id: Option<String>,
    pub client: ClientCapabilities,
}

impl MockSession {
    pub fn new() -> Self { /* default values */ }
    pub fn with_app_version(self, v: u32) -> Self { /* ... */ }
    pub fn with_protocol_version(self, v: u32) -> Self { /* ... */ }
    pub fn as_ctx(&self) -> SessionCtx { /* ... */ }
}
```

Implement `assert_view!` macro for structural PluginView assertions.

---

## Step 9 — Examples

**`hook-example`:** Plugin with `before_submit` hook that cancels if input is empty.
Host server function runs `runtime.run_hook(...)`. Tests show Continue/Cancel paths.

**`invocation-example`:** Plugin calls `dx_invoke("get_greeting", { "name": "World" })`.
Host registers invocation returning `"Hello, World!"`. Plugin embeds result in slot view.

---

## Verification

```bash
cargo check --workspace
cargo test --workspace --lib
cargo test -p dioxus-extism-host
cargo test -p dioxus-extism-test
```
