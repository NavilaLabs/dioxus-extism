# Phase 1 — Protocol and Foundation

**Prerequisite:** Read `CLAUDE.md` and `API_NOTES.md` fully. Then:
```bash
cargo check --workspace  # must succeed (empty workspace is fine)
cargo check -p dioxus-extism-protocol --target wasm32-unknown-unknown
```

**Goal:** Compiling workspace with the protocol crate complete, a working host
skeleton (no WASM loading yet), frontend with PluginBootProvider and PluginSlot,
PDK with SlotProvider, and a working `hello-plugin` example end-to-end.

**Stop condition:**
```bash
cargo check --workspace && cargo test --workspace --lib
cargo check -p dioxus-extism-protocol --target wasm32-unknown-unknown
```
Both must pass. Do NOT begin Phase 2 until confirmed.

---

## Step 1 — Workspace Cargo.toml

```toml
[workspace]
members = [
    "crates/dioxus-extism-protocol",
    "crates/dioxus-extism-macros",
    "crates/dioxus-extism-host",
    "crates/dioxus-extism-pdk",
    "crates/dioxus-extism-frontend",
    "crates/dioxus-extism-test",
    "dioxus-extism",
    "examples/hello-plugin/plugin",
    "examples/hello-plugin/host",
]
resolver = "2"

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

Run `cargo check --workspace` after creating this. Fix any version resolution errors
before proceeding.

---

## Step 2 — `dioxus-extism-protocol`

No dependencies on `extism` or `dioxus`. Must compile for `wasm32-unknown-unknown`.

### Write tests first in `tests/route_pattern.rs`:

```rust
use dioxus_extism_protocol::RoutePattern;

#[test] fn matches_single_param() {
    assert!(RoutePattern("/product/:id".into()).matches("/product/42"));
}
#[test] fn rejects_extra_segments() {
    assert!(!RoutePattern("/product/:id".into()).matches("/product/42/reviews"));
}
#[test] fn rejects_wrong_prefix() {
    assert!(!RoutePattern("/product/:id".into()).matches("/products/42"));
}
#[test] fn rejects_trailing_slash() {
    assert!(!RoutePattern("/product/:id".into()).matches("/product/42/"));
}
#[test] fn extracts_single_param() {
    let p = RoutePattern("/product/:id".into());
    assert_eq!(p.extract_params("/product/42").unwrap()["id"], "42");
}
#[test] fn extracts_multiple_params() {
    let p = RoutePattern("/shop/:shop/item/:id".into());
    let params = p.extract_params("/shop/acme/item/99").unwrap();
    assert_eq!(params["shop"], "acme");
    assert_eq!(params["id"], "99");
}
#[test] fn root_matches_root() {
    assert!(RoutePattern("/".into()).matches("/"));
}
```

Run `cargo test -p dioxus-extism-protocol` — tests must FAIL (not yet compiled).

### Implement all types from architecture sections 3.1–3.10:

- `PROTOCOL_VERSION: u32 = 1`
- `PluginId`, `HandlerId`, `SessionId`, `RoutePattern` with `matches()` + `extract_params()`
- `ClientCapabilities`, `AppUpdateRequired`, `PluginClientRequirement`
- `PriorityHint` with `as_numeric()`
- `PluginManifest` with all fields including `min_protocol_version`, `min_app_version`,
  `required_host_components`
- `StateScope`, `SlotRegistration`, `HookRegistration`, `HostCapability`
- `Selector`, `NodeSelector` with `Recursive` and `DataAttr` variants
- `TransformDeclaration`, `TransformOp`, `TransformInput`, `TransformContext`,
  `TransformOutput`
- `PluginView` with `Incompatible` variant, `ViewElement` (with `name` + `key`),
  `AttrValue`, `BoundEventHandler`, `DomEvent`, `HostComponentRef`
- `HookCall`, `HookResult`, `PluginEvent`, `EventSource`
- `SlotContent`, `ViewUpdate` (keyed diff semantics documented in struct comment)
- `SessionCtx` (with `client: ClientCapabilities` and `caller: Option<PluginId>`)
- `OverrideMap` (with `version: u64`, compatibility fields, `plugin_requirements`)
- `SsrRouteOutput`

Run tests — all must PASS.
Run `cargo check -p dioxus-extism-protocol --target wasm32-unknown-unknown` — must PASS.

---

## Step 3 — `dioxus-extism-macros` (skeleton)

```toml
[lib]
proc-macro = true

[dependencies]
syn        = { workspace = true }
quote      = { workspace = true }
proc-macro2 = { workspace = true }
```

```rust
// src/lib.rs
use proc_macro::TokenStream;

/// Placeholder — full implementation in Phase 3.
#[proc_macro_attribute]
pub fn overridable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item  // pass-through for now
}
```

---

## Step 4 — `dioxus-extism-host` skeleton

### Crate dependencies:

```toml
[dependencies]
extism        = { workspace = true }
dioxus-extism-protocol = { path = "../../crates/dioxus-extism-protocol" }
tokio         = { workspace = true }
serde         = { workspace = true }
serde_json    = { workspace = true }
thiserror     = { workspace = true }
indexmap      = { workspace = true }
axum          = { workspace = true }
tracing       = { workspace = true }
sha2          = { workspace = true }
async-trait   = { workspace = true }
```

### Write test first in `tests/builder.rs`:

```rust
use dioxus_extism_host::*;

#[tokio::test]
async fn empty_runtime_builds() {
    let runtime = PluginRuntimeBuilder::new()
        .with_session_ttl(std::time::Duration::from_secs(3600))
        .build()
        .await;
    assert!(runtime.is_ok(), "empty runtime should build: {:?}", runtime.err());
}
```

### Implement (no WASM loading yet):

**`PluginRuntimeError`** (all variants from architecture section 4.11):
```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PluginRuntimeError {
    #[error("plugin not found: {0:?}")]
    PluginNotFound(PluginId),
    #[error("plugin call failed: {source}")]
    CallFailed { #[source] source: extism::Error },
    // ... all variants from architecture doc
}
```

**`PluginInstallConfig`** with `resolve()` method:
```rust
#[derive(Debug, Default)]
pub struct PluginInstallConfig {
    pub base_priority: Option<i32>,
    pub overrides: std::collections::HashMap<String, i32>,
    pub pool_size: Option<usize>,
    pub max_call_duration: Option<std::time::Duration>,
}

impl PluginInstallConfig {
    pub fn resolve(&self, name: &str, hint: &PriorityHint) -> i32 {
        self.overrides.get(name).copied()
            .or(self.base_priority)
            .unwrap_or_else(|| hint.as_numeric())
    }
}
```

**`LoadedPlugin`**:
```rust
struct LoadedPlugin {
    manifest: PluginManifest,
    pool: extism::Pool,              // Extism built-in pool
    enabled: std::sync::atomic::AtomicBool,
    config: PluginInstallConfig,
}
```

**`Registries`** (all four registries + cached OverrideMap):
```rust
struct Registries {
    slots: SlotRegistry,
    hooks: HookRegistry,
    transforms: TransformRegistry,
    override_map: OverrideMap,
}
```

**`PluginRuntime`**:
```rust
pub struct PluginRuntime {
    plugins: tokio::sync::RwLock<indexmap::IndexMap<PluginId, LoadedPlugin>>,
    global_states: std::sync::Arc<tokio::sync::RwLock<GlobalStateMap>>,
    session_states: std::sync::Arc<tokio::sync::RwLock<SessionStateMap>>,
    event_bus: std::sync::Arc<EventBus>,
    registries: tokio::sync::RwLock<Registries>,
    invocation_registry: std::sync::Arc<InvocationRegistry>,
    override_map_tx: tokio::sync::broadcast::Sender<OverrideMap>,
}
```

**`PluginRuntimeBuilder`** with all methods from architecture section 4.3.
`build()` for now constructs an empty `PluginRuntime` (no WASM loading).

**`PluginRuntimeExt`** trait on `axum::Router`:
```rust
pub trait PluginRuntimeExt {
    fn with_plugin_runtime(self, runtime: std::sync::Arc<PluginRuntime>) -> Self;
}

impl PluginRuntimeExt for axum::Router {
    fn with_plugin_runtime(self, runtime: std::sync::Arc<PluginRuntime>) -> Self {
        self.with_state(runtime)
    }
}
```

**`StatePersistenceProvider`** trait skeleton (no impl yet):
```rust
#[async_trait::async_trait]
pub trait StatePersistenceProvider: Send + Sync + 'static {
    async fn save(
        &self, plugin_id: &PluginId,
        state: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), PersistenceError>;
    async fn load(
        &self, plugin_id: &PluginId,
    ) -> Result<Option<std::collections::HashMap<String, serde_json::Value>>, PersistenceError>;
}
```

---

## Step 5 — Host function stubs

Define all host functions as stubs (log the call, return empty):

```rust
pub(crate) fn make_host_functions(_runtime: std::sync::Arc<PluginRuntime>) -> Vec<extism::Function> {
    vec![
        make_state_get_stub(),
        make_state_set_stub(),
        // ... all 10 host functions as stubs
    ]
}

fn make_state_get_stub() -> extism::Function {
    extism::Function::new(
        "dx_state_get",
        [extism::ValType::PTR],
        [extism::ValType::PTR],
        extism::UserData::new(()),   // empty UserData for stubs
        |plugin, _inputs, _outputs, _user_data| {
            tracing::debug!("dx_state_get stub called");
            plugin.output("null").ok();
        },
    )
}
```

---

## Step 6 — `dioxus-extism-frontend` skeleton

Implement with Dioxus 0.7 API:

**`PluginBootProvider`** — fetches OverrideMap at boot, provides context, SSE stub:
```rust
#[component]
pub fn PluginBootProvider(children: Element) -> Element {
    let override_map: Signal<OverrideMap> = use_signal(OverrideMap::default);
    let registry = use_context::<HostComponentRegistry>();
    let caps = ClientCapabilities {
        protocol_version: PROTOCOL_VERSION,
        app_version: option_env!("APP_VERSION")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        registered_host_components: registry.names(),
    };
    provide_context(caps.clone());

    use_resource(move || {
        let caps = caps.clone();
        async move {
            if let Ok(map) = get_override_map(caps).await {
                *override_map.write() = map;
            }
        }
    });

    provide_context(override_map.read_only());
    rsx! { {children} }
}
```

**`PluginSlot`** with `loading` prop (Dioxus 0.7 `use_resource`):
```rust
#[component]
pub fn PluginSlot(
    name: String,
    #[props(default)] loading: Option<Element>,
    #[props(default)] fallback: Option<Element>,
) -> Element {
    let session_id = use_session_id();
    let client_caps = use_context::<ClientCapabilities>();
    let contents = use_resource(move || {
        let name = name.clone();
        let sid = session_id.read().clone();
        let caps = client_caps.clone();
        async move { get_slot_content(name, sid, caps).await }
    });

    match contents.read().as_ref() {
        None => loading.unwrap_or(rsx! {}),
        Some(Ok(c)) if !c.is_empty() => rsx! {
            for content in c {
                PluginViewRenderer { view: content.view.clone(), session_id }
            }
        },
        _ => fallback.unwrap_or(rsx! {}),
    }
}
```

**`PluginViewRenderer`** — all variants including `Incompatible`.
**`SessionProviderRoot<P>`** + `WebSessionProvider` + `DesktopSessionProvider`.
**`HostComponentRegistry`** with `names()` method.
**Server function stubs** returning empty results.

---

## Step 7 — `dioxus-extism-pdk` skeleton

Implement with extism-pdk 1.4:

- `DioxusPlugin` trait
- `SlotProvider`, `HookHandler`, `EventSubscriber`, `InteractionHandler`, `OnLoad`, `OnUnload`
- `TransformProvider`
- `PluginCtx` with `state`, `emit`, `invoke`, `session`, `client` fields
- `StateAccessor`, `EventEmitter`, `InvocationAccessor` (all stubs calling host fns)
- View builder DSL (all functions including `incompatible()`)
- `PdkError`
- `plugin!` macro — generates `manifest()` and `slot_{name}()` exports for Phase 1

---

## Step 8 — `hello-plugin` example

**plugin/Cargo.toml:**
```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
dioxus-extism-pdk = { path = "../../../crates/dioxus-extism-pdk" }
```

**plugin/src/lib.rs:**
```rust
use dioxus_extism_pdk::prelude::*;

struct HelloPlugin;

impl DioxusPlugin for HelloPlugin {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("example/hello".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "hello-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for HelloPlugin {
    const SLOT_NAME: &'static str = "hello-slot";
    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        Ok(div()
            .class("hello-from-plugin")
            .child(text("Hello from a WASM plugin!"))
            .build())
    }
}

plugin! { type: HelloPlugin, slots: [HelloPlugin] }
```

**host/src/main.rs:** Minimal Dioxus 0.7 fullstack app with one route containing
`<PluginSlot name="hello-slot" />`. Use `dioxus::LaunchBuilder::new()` (0.7 API).

---

## Verification

```bash
cargo check --workspace
cargo test --workspace --lib
cargo check -p dioxus-extism-protocol --target wasm32-unknown-unknown

# Build the example plugin to WASM
cargo build -p hello-plugin-plugin --target wasm32-unknown-unknown --release
```

All must pass before marking Phase 1 complete.
