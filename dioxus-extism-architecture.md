# `dioxus-extism` — Architecture Plan

> A first-class crate for extending Dioxus applications with Extism WASM plugins.
> Front and backend handled separately. Plugins can register components, fill slots,
> override host components, intercept behaviour, and communicate via a message bus —
> without requiring the host developer to anticipate every extension point in advance.

---

## Contents

1. [Foundational Decisions](#1-foundational-decisions)
2. [Crate Structure](#2-crate-structure)
3. [Protocol Layer](#3-protocol-layer-dioxus-extism-protocol)
4. [Host Layer](#4-host-layer-dioxus-extism-host)
5. [Frontend Layer](#5-frontend-layer-dioxus-extism-frontend)
6. [PDK Layer](#6-pdk-layer-dioxus-extism-pdk)
7. [Testing Infrastructure](#7-testing-infrastructure-dioxus-extism-test)
8. [Key Flows](#8-key-flows)
9. [Integration Example](#9-integration-example)
10. [Open Design Decisions](#10-open-design-decisions)
11. [Implementation Roadmap](#11-implementation-roadmap)

---

## 1. Foundational Decisions

### 1.1 Server-Rendered Plugin UI

Extism uses `wasmtime` under the hood. `wasmtime` does not run in browsers. Therefore,
plugins run **exclusively on the server** (or on desktop/native targets in-process).

Frontend plugin UI is delivered as a **`PluginView`** — a serialisable tree of virtual
elements — returned by the plugin over a Dioxus server function. The host Dioxus frontend
deserialises and renders it via `PluginViewRenderer`. This approach:

- Works on every Dioxus target (web, desktop, mobile, SSR) without exception.
- Keeps plugins sandboxed: they never touch the DOM directly.
- Means interactions (clicks, inputs) are round-tripped to the server. Acceptable
  for extension points, not for latency-critical primary UX.

### 1.2 State Scope

Plugins declare their state scope in the manifest:

```
Global      — one shared state across all users/sessions
PerSession  — isolated state per session/user
Hybrid      — named keys are explicitly routed to one scope or the other
```

The host enforces this. Plugins access state only through host functions.

### 1.3 Plugin-to-Plugin: Message Bus

Plugins communicate exclusively via the host as a message bus. A plugin emits a named
event with a JSON payload; the host routes it to all registered subscribers. No direct
plugin-to-plugin call graph, which prevents dependency cycles and lets the host
inspect all inter-plugin traffic.

### 1.4 Behaviour Modification Depth

All three levels are supported:

| Level | Mechanism | Example |
|---|---|---|
| **Intercept / Cancel** | Hook chain returns `Cancel` | Block a form submit |
| **Transform** | Hook chain returns `Replace` | Normalise input data before save |
| **Full Override** | Component / tree transform | Replace `<UserAvatar>` entirely |

### 1.5 Wrap Transform Model: Sequential Pipeline

When multiple plugins declare `TransformOp::Wrap` for the same target (route, slot,
or component), they participate as a **sequential pipeline**, not a competition.

Plugins are sorted by priority descending. The runtime folds through them: each plugin
receives the current accumulated view as `TransformInput::original` and returns a new
`PluginView`. The next plugin receives that output as its input. The final fold result
is the composed view the frontend renders.

```
seed = PluginView::HostComponent("__content__")  // the actual route/slot/component output

fold (priority desc):
  plugin_a (priority 10): receives seed    -> returns view_a  (must include original_content())
  plugin_b (priority  5): receives view_a  -> returns view_b  (must include original_content())
  plugin_c (priority  0): receives view_b  -> returns view_c  (must include original_content())

frontend renders view_c; __content__ chains resolve through to the actual output
```

This means later plugins (lower priority) can see and react to what earlier plugins
added — they receive the full accumulated `PluginView` tree, not an opaque hole.

**Plugin author contract:** a Wrap plugin that does not include `original_content()`
in its returned view intentionally cuts the chain. Everything from higher-priority
plugins and the original content is dropped. This is valid behaviour but must be
documented clearly. The host runtime emits a `tracing::warn!` when `__content__` is
absent from a Wrap output in debug builds.

### 1.6 The Dynamic Extension Problem

The naive slot design requires the host developer to manually place `<PluginSlot>`
and `<OverridableComponent>` at every point a plugin might ever want to extend. This
is not dynamic — it is a pre-negotiated contract disguised as a plugin system, and it
breaks the moment a plugin developer wants to extend something the host developer never
anticipated.

The solution is three cooperative layers of increasing dynamism, all shipping in v1.
The host developer's burden at each layer is deliberately minimal.

### 1.7 The Three Extension Layers

**Layer 1 — Named slots and `#[overridable]` components**

The host developer marks components they explicitly expose for extension with one
attribute at the definition site. Every call site of that component automatically
becomes an extension point. `<PluginSlot>` markers in RSX are reserved for places the
host explicitly wants as an injection point by contract.

Host dev burden: one `#[overridable]` attribute per component definition, or one
`<PluginSlot>` per explicit injection point.

**Layer 2 — Route injection via `PluginAwareRouter`**

The host developer swaps `Router::<R>` for `PluginAwareRouter::<R>` once in the
application root. After that, plugins can inject UI before, after, or wrapping any
route's rendered output by declaring a `Selector::Route` in their manifest — without
any further host involvement. Page components are written completely normally with
zero plugin awareness.

Host dev burden: one component swap in the application root.

**Layer 3 — Tree selectors**

Plugins can declare transforms targeting structural points in the rendered layer:
`#[overridable]` component names, `PluginSlot` names, `data-plugin-slot` attributes
on any element (including elements inside other plugins' output), and nodes within
`PluginView` trees contributed by other plugins. This enables plugin-on-plugin
composition with no host involvement whatsoever.

Host dev burden: zero beyond Layers 1 and 2. A `data-plugin-slot` attribute is
available for host devs who want to expose addressable points on plain elements
without making a full component overridable.

**What the host tree boundary means**

The host renders to Dioxus `Element` (compiled RSX). This is opaque — it cannot be
serialised or traversed generically at runtime. Therefore:

- Tree selectors can target `#[overridable]` component boundaries.
- Tree selectors can target `<PluginSlot>` boundaries.
- Tree selectors can target `data-plugin-slot` attributes on elements inside `PluginView` trees.
- Tree selectors can target arbitrary structural nodes inside any `PluginView` tree.
- Tree selectors **cannot** traverse arbitrary host RSX with no named boundary.
  `PluginAwareRouter` is the coarse-grained escape hatch for that case.

This is not a limitation — it is the correct abstraction. Even browser extensions that
modify arbitrary websites rely on the DOM having structure. Here, the structure is
expressed through component names, slot names, routes, and `data-plugin-slot` markers.

---

## 2. Crate Structure

```
dioxus-extism/                       <- workspace root
|-- Cargo.toml
|-- crates/
|   |-- dioxus-extism-protocol/      <- shared types, no server/browser coupling
|   |-- dioxus-extism-host/          <- server-side runtime (depends on extism)
|   |-- dioxus-extism-macros/        <- proc macros: #[overridable], plugin!
|   |-- dioxus-extism-frontend/      <- Dioxus components (depends on dioxus + protocol)
|   |-- dioxus-extism-pdk/           <- plugin dev kit (depends on extism-pdk + protocol)
|   `-- dioxus-extism-test/          <- test utilities (TestRuntime, MockSession, assert_view!)
|-- dioxus-extism/                   <- thin re-export crate (the public face)
|   `-- Cargo.toml  [features: host, frontend, pdk, test]
`-- examples/
    |-- hello-plugin/                <- minimal plugin + host example
    |-- slot-example/                <- Layer 1: named slots and #[overridable]
    |-- route-injection-example/     <- Layer 2: PluginAwareRouter
    |-- tree-selector-example/       <- Layer 3: cross-plugin transforms
    |-- hook-example/                <- server-side hook interception + invocation
    `-- ssr-example/                 <- SSR rendering mode
```

### Dependency graph

```
protocol  <--------------------------------- (no dioxus, no extism; pure serde types)
   ^              ^              ^      ^
  host         frontend         pdk   test
(extism)   (dioxus+macros)  (extism-pdk) (extism)
   ^              ^
   `------+-------'
      dioxus-extism (re-exports with feature flags)

macros <- (proc-macro crate, depended on by frontend + pdk)
test   <- (depends on host + pdk + protocol; dev-only)
```

`protocol` compiles for `wasm32-unknown-unknown`, so frontend and PDK share identical
types across the WASM boundary with full type-checking.

---

## 3. Protocol Layer (`dioxus-extism-protocol`)

Every message crossing the WASM boundary uses these types. All types derive `Debug`,
`Clone`, `Serialize`, `Deserialize`. Public enums are `#[non_exhaustive]`.

### 3.1 Core Identifiers and Protocol Constants

```rust
/// Monotonically increasing. Bumped whenever PluginView, ViewElement,
/// NodeSelector, or any protocol type gains new variants or fields
/// that old clients cannot handle correctly.
/// Plugins declare min_protocol_version in their manifest; the runtime
/// serves PluginView::Incompatible for clients below that threshold.
pub const PROTOCOL_VERSION: u32 = 1;

/// "org/plugin-name" -- globally unique plugin identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PluginId(pub String);

/// Opaque handler reference embedded in a PluginView event handler.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HandlerId(pub String);

/// Session identifier (maps to a user session, HTTP request, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

/// A URL path pattern with :param segments, e.g. "/product/:id".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoutePattern(pub String);

impl RoutePattern {
    pub fn matches(&self, path: &str) -> bool { ... }
    pub fn extract_params(&self, path: &str) -> Option<HashMap<String, String>> { ... }
}

/// Sent by the client with every server function call.
/// Allows the server to check compatibility and allows plugins to produce
/// compatible output for older clients (e.g. skip new HostComponent references).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// PROTOCOL_VERSION the client was compiled against.
    pub protocol_version: u32,
    /// Host app's own version integer. Increment when new HostComponents are
    /// registered or when plugin rendering capabilities change. Distinct from
    /// protocol_version: the app can update its components without the underlying
    /// wire protocol changing.
    pub app_version: u32,
    /// HostComponent names this client has registered and can render.
    /// Plugins use this to know whether a HostComponent reference will resolve.
    pub registered_host_components: Vec<String>,
}

/// Provided as context by PluginBootProvider when loaded plugins require
/// a newer client than what is connected. The host app reads this context
/// to render an update prompt, block plugin sections gracefully, or degrade.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateRequired {
    pub current_protocol: u32,
    pub required_protocol: u32,
    pub current_app: u32,
    pub required_app: u32,
    /// Which specific plugins triggered the requirement.
    pub blocking_plugins: Vec<PluginId>,
}

/// A plugin's suggested position in any ordered sequence (slot, hook, transform).
/// The installer may override this with an absolute value via PluginInstallConfig.
/// Maps to a numeric bucket at runtime: First=1000, High=750, Normal=500, Low=250, Last=0.
/// Ties within a bucket are broken by plugin load order (installer-controlled).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum PriorityHint {
    /// Must run before all others. For security, auth, rate-limiting.
    First,
    /// Earlier than average, but not exclusively first.
    High,
    /// Default. No strong ordering preference.
    #[default]
    Normal,
    /// Later than average, but not exclusively last.
    Low,
    /// Must run after all others. For analytics, logging, auditing.
    Last,
}

impl PriorityHint {
    pub fn as_numeric(&self) -> i32 {
        match self {
            Self::First  => 1000,
            Self::High   =>  750,
            Self::Normal =>  500,
            Self::Low    =>  250,
            Self::Last   =>    0,
        }
    }
}
```

### 3.2 Plugin Manifest

```rust
/// Returned by every plugin's `manifest` export. Declares all capabilities
/// and requirements of the plugin upfront.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    pub id: PluginId,
    pub version: String,

    /// Minimum PROTOCOL_VERSION the connecting client must have.
    /// If the client's protocol_version is below this, the runtime serves
    /// PluginView::Incompatible instead of calling this plugin at all.
    /// Default: 1 (current version).
    pub min_protocol_version: u32,

    /// Minimum host app version this plugin requires.
    /// Plugin authors set this when they reference HostComponents or app
    /// capabilities introduced after the initial host app release.
    /// Default: 0 (no requirement).
    pub min_app_version: u32,

    /// HostComponent names this plugin references in its PluginView output.
    /// The runtime warns at build() time if any are absent from the
    /// registered HostComponentRegistry, helping catch integration errors early.
    pub required_host_components: Vec<String>,

    pub state_scope: StateScope,
    pub slots: Vec<SlotRegistration>,
    pub hooks: Vec<HookRegistration>,
    pub event_subscriptions: Vec<String>,
    pub transforms: Vec<TransformDeclaration>,
    pub host_capabilities: Vec<HostCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum StateScope {
    #[default]
    PerSession,
    Global,
    Hybrid {
        global_keys: Vec<String>,
        session_keys: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotRegistration {
    pub name: String,
    /// The plugin's suggested ordering. The installer may override via PluginInstallConfig.
    pub priority_hint: PriorityHint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRegistration {
    pub hook_name: String,
    /// The plugin's suggested ordering. The installer may override via PluginInstallConfig.
    pub priority_hint: PriorityHint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HostCapability {
    Http { allowed_hosts: Vec<String> },
    GlobalStateRead { keys: Vec<String> },
    GlobalStateWrite { keys: Vec<String> },
    ReadPluginState { plugin_id: PluginId, keys: Vec<String> },
    /// Request permission to call named host-side invocations.
    /// The host grants or denies each name individually at build time.
    /// Plugins that request an invocation name they were not granted
    /// receive CapabilityDenied when they call dx_invoke.
    Invoke { names: Vec<String> },
}
```

### 3.3 Selector -- Addressing Extension Points

The selector identifies *what* a transform targets. All three layers are expressed
in one unified type.

```rust
/// Addresses a point in the rendered layer that a transform can target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Selector {
    // --- Layer 1 ---
    /// A named #[overridable] or OverridableComponent boundary.
    Component(String),
    /// A PluginSlot by name -- targets the slot's contribution list.
    Slot(String),
    /// An element carrying data-plugin-slot="value" (inside any PluginView tree).
    DataPluginSlot(String),

    // --- Layer 2 ---
    /// Targets the rendered output of a route matching this pattern.
    Route(RoutePattern),

    // --- Layer 3 ---
    /// Selects nodes within the PluginView tree produced by an outer selector.
    Within {
        outer: Box<Selector>,
        inner: NodeSelector,
    },

    // --- Composition ---
    /// Applies to any selector in the list that matches.
    Any(Vec<Selector>),
}

/// Selects specific nodes within a PluginView tree.
/// Default traversal is SHALLOW: only direct children of the outer selection
/// are tested. Wrap any selector in Recursive to match at any depth.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NodeSelector {
    /// Matches any element with this HTML tag.
    Tag(String),
    /// Matches any element whose class list contains this class.
    HasClass(String),
    /// Matches any HostComponent reference with this name.
    HostComponent(String),
    /// Matches any ViewElement whose `name` field equals this string.
    Name(String),
    /// Matches any element carrying data-plugin-slot="value".
    DataAttr(String, String),
    /// Matches the first child of the outer selection.
    First,
    /// Matches the last child of the outer selection.
    Last,
    /// Matches child at this index (0-based).
    Index(usize),
    And(Box<NodeSelector>, Box<NodeSelector>),
    Or(Box<NodeSelector>, Box<NodeSelector>),
    /// Opt-in: apply the inner selector recursively at any depth in the tree,
    /// not just direct children. Use with care on large trees.
    Recursive(Box<NodeSelector>),
}
```

### 3.4 Transform Declaration

```rust
/// Declares a dynamic extension: select a point, apply an operation.
/// The runtime calls `transform_fn` (a named plugin export) at render time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformDeclaration {
    pub selector: Selector,
    /// Plugin export name to call at render time to get the plugin's view.
    pub transform_fn: String,
    pub op: TransformOp,
    /// The plugin's suggested ordering. The installer may override via PluginInstallConfig.
    pub priority_hint: PriorityHint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TransformOp {
    // --- Route-level (Layer 2) and slot-level (Layer 1) ---
    /// Render plugin view before the selected output.
    InjectBefore,
    /// Render plugin view after the selected output.
    InjectAfter,
    /// Wrap the selected output. Plugin view may embed
    /// HostComponent("__content__") as a placeholder for the original.
    Wrap,

    // --- Node-level (Layer 3) ---
    /// Replace the selected node entirely with the plugin view.
    Replace,
    /// Wrap the selected node. Plugin view may embed
    /// HostComponent("__target__") as a placeholder for the original node.
    WrapNode,
    /// Insert plugin view before the selected node.
    InsertBefore,
    /// Insert plugin view after the selected node.
    InsertAfter,
    /// Add a CSS class to the selected node (no new view rendered by plugin).
    AddClass(String),
    /// Set an attribute on the selected node (no new view rendered by plugin).
    SetAttr(String, AttrValue),
}
```

### 3.5 Transform I/O

```rust
/// Input to a plugin's transform export function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformInput {
    /// Current content at the selected point, for ops that need it
    /// (Replace, WrapNode, Wrap). None for Inject* / AddClass / SetAttr.
    pub original: Option<PluginView>,
    pub context: TransformContext,
    pub session: SessionCtx,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransformContext {
    pub route_params: HashMap<String, String>,
    pub component_props: Option<serde_json::Value>,
    pub slot_name: Option<String>,
    /// What the calling client supports. Plugins use this to produce
    /// compatible output for older clients — e.g. skip HostComponent
    /// references not present in registered_host_components, or emit
    /// simpler PluginView for clients with a lower protocol_version.
    pub client: ClientCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformOutput {
    pub view: PluginView,
}
```

### 3.6 `PluginView` -- The UI Description Tree

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PluginView {
    Element(ViewElement),
    Text(String),
    /// Render a named host component with forwarded props.
    /// Special reserved names: "__content__" and "__target__"
    /// are transform placeholders resolved by PluginViewRenderer.
    HostComponent(HostComponentRef),
    Fragment(Vec<PluginView>),
    Empty,
    /// Served by the runtime when a plugin's min_protocol_version or
    /// min_app_version exceeds the connecting client's capabilities.
    /// The plugin is never called; the runtime produces this directly.
    /// `fallback` is an optional simple view the client CAN render safely.
    /// `reason` is a human-readable explanation (for debug/logging only).
    Incompatible {
        reason: String,
        fallback: Option<Box<PluginView>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ViewElement {
    pub tag: String,
    /// Stable name for selector targeting (set via `.name()` in the view DSL).
    pub name: Option<String>,
    /// Stable key for keyed view diffing (set via `.key()` in the view DSL).
    /// Forwarded as a Dioxus RSX `key` attribute so Dioxus's own diffing
    /// engine reconciles keyed lists correctly on interaction updates.
    pub key: Option<String>,
    pub attrs: Vec<(String, AttrValue)>,
    pub handlers: Vec<BoundEventHandler>,
    pub children: Vec<PluginView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum AttrValue {
    String(String),
    Bool(bool),
    Number(f64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundEventHandler {
    pub event: DomEvent,
    pub handler_id: HandlerId,
    pub debounce_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DomEvent {
    Click, Input, Change, Submit, Focus, Blur, KeyDown, KeyUp,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostComponentRef {
    pub name: String,
    pub props: serde_json::Value,
    pub children: Vec<PluginView>,
}
```

### 3.7 Hook Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookCall {
    pub hook_name: String,
    pub context: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HookResult {
    Continue { context: serde_json::Value },
    Cancel { reason: String },
    Replace { context: serde_json::Value },
}
```

### 3.8 Event Bus Types

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEvent {
    pub source: EventSource,
    pub name: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventSource {
    Host,
    Plugin(PluginId),
}
```

### 3.9 Slot Content and Interaction Response

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotContent {
    pub plugin_id: PluginId,
    pub priority: i32,
    pub view: PluginView,
}

/// Returned by a plugin's interaction handler. Contains a new view to render
/// and optional events to emit. The view is applied as a keyed diff against the
/// currently rendered PluginView, using the `key` field on ViewElement nodes.
/// Nodes with matching keys are updated in place; nodes without keys whose
/// position or tag changed are replaced. This eliminates full-subtree flicker
/// on interactions that only change a small part of the view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewUpdate {
    /// If Some, the PluginViewRenderer diffs this against the current view.
    /// If None, the current view is left unchanged (pure side-effect interaction).
    pub view: Option<PluginView>,
    pub events: Vec<PluginEvent>,
}
```

### 3.10 Session Context and Override Map

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCtx {
    pub session_id: SessionId,
    pub user_id: Option<String>,
    /// The capabilities of the client that initiated this request.
    /// Passed through to all plugin calls so plugins can produce
    /// compatible output for older clients.
    pub client: ClientCapabilities,
    /// The plugin making this call. Set by the host runtime on every
    /// plugin invocation. Used by host functions to enforce the capability
    /// model — a host function checks this against the granted capabilities
    /// before executing, preventing any plugin from using capabilities it
    /// was not explicitly granted.
    pub caller: Option<PluginId>,
}

/// Served once at app startup, cached in frontend context.
/// Updated via SSE when plugins are hot-reloaded.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OverrideMap {
    /// Monotonically increasing version. Higher = newer.
    pub version: u64,
    pub overridden_components: HashSet<String>,
    pub transformed_slots: HashSet<String>,
    pub route_patterns: Vec<RoutePattern>,
    /// Maximum min_protocol_version across all loaded plugins.
    /// If this exceeds the client's PROTOCOL_VERSION, some plugins will
    /// serve Incompatible views and the client should prompt for an update.
    pub required_protocol_version: u32,
    /// Maximum min_app_version across all loaded plugins.
    pub required_app_version: u32,
    /// Per-plugin requirements for targeted "plugin X needs app v5" messaging.
    pub plugin_requirements: HashMap<PluginId, PluginClientRequirement>,
}

/// Per-plugin client requirements, embedded in OverrideMap.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginClientRequirement {
    pub min_protocol_version: u32,
    pub min_app_version: u32,
    /// HostComponent names this plugin references.
    pub required_host_components: Vec<String>,
}
```

---

## 4. Host Layer (`dioxus-extism-host`)

### 4.1 `PluginRuntime`

```rust
pub struct PluginRuntime {
    plugins: RwLock<IndexMap<PluginId, LoadedPlugin>>,
    global_states: Arc<RwLock<HashMap<PluginId, PluginState>>>,
    session_states: Arc<RwLock<HashMap<SessionId, HashMap<PluginId, PluginState>>>>,
    event_bus: Arc<EventBus>,
    /// All four registries are under one RwLock so hot-reload can rebuild them
    /// atomically in a single write operation without intermediate inconsistent state.
    registries: RwLock<Registries>,
    invocation_registry: Arc<InvocationRegistry>,
    override_map_tx: broadcast::Sender<OverrideMap>,
}

struct Registries {
    slots: SlotRegistry,
    hooks: HookRegistry,
    transforms: TransformRegistry,
    /// Cached OverrideMap, rebuilt alongside the registries.
    override_map: OverrideMap,
}

/// Each plugin is backed by a pool of WASM instances rather than a single one.
/// This is the central answer to the concurrency problem: see note below.
struct LoadedPlugin {
    manifest: PluginManifest,
    instances: Vec<Mutex<extism::Plugin>>,
    next: AtomicUsize,
    /// When false, the runtime returns empty/Incompatible results for this plugin
    /// without calling its WASM code. Toggled by enable_plugin / disable_plugin.
    enabled: AtomicBool,
    config: PluginInstallConfig,
}

impl LoadedPlugin {
    /// Acquire any available instance from the pool. Tries each slot in round-robin
    /// order; if all are locked (all busy), waits on the least-recently-used one.
    fn acquire(&self) -> MutexGuard<extism::Plugin> { ... }
}
```

**Why a pool? The `Sync` problem explained.**

`Sync` in Rust means: "it is safe to share a *reference* to this value across threads simultaneously." `Send` means: "it is safe to *move* this value to another thread."

Extism's `Plugin` is `Send` but not `Sync`. It can be owned by one thread at a time, but two threads cannot hold `&Plugin` at the same moment. This is because the underlying `wasmtime::Store` has internal mutable state that isn't thread-safe to access concurrently.

Extism's `Plugin::call()` is also **synchronous and blocking** — it runs WASM to completion on the calling thread with no async interface. If you call it on an async executor thread (like Tokio's), you block that thread for the duration of the WASM execution, starving other async tasks.

This creates two distinct problems:

1. **Blocking the async runtime.** `Plugin::call()` should be moved off the async executor via `tokio::task::spawn_blocking`. The closure captures the `MutexGuard<Plugin>` (which is `Send` because `Plugin: Send`), runs on a dedicated blocking thread, and the result is `await`ed back on the async side.

2. **Sequential calls within one render.** If a slot has five plugins, and each plugin has a single `Mutex<Plugin>`, all five calls can run concurrently on separate blocking threads — one per plugin. But if two *requests* hit the same plugin at the same time, the second request waits for the first to release the single `Mutex`. A pool of `N` instances per plugin allows `N` concurrent requests to that plugin without waiting.

The correct execution pattern for every plugin call:

```rust
async fn call_plugin<I, O>(
    plugin: &LoadedPlugin,
    export: &str,
    input: I,
) -> Result<O, PluginRuntimeError>
where
    I: Serialize + Send + 'static,
    O: DeserializeOwned + Send + 'static,
{
    // Acquire an instance (may briefly block on the Mutex, not the async runtime).
    // The guard is Send because Plugin: Send, so it can cross spawn_blocking.
    let guard = plugin.acquire();  // fast, in-memory lock

    // Move the guard and input into a blocking thread.
    // Plugin::call() runs here, off the async executor.
    tokio::task::spawn_blocking(move || {
        guard.call::<I, O>(export, input)
            .map_err(PluginRuntimeError::from)
    })
    .await
    .map_err(|join_err| PluginRuntimeError::TaskPanic(join_err.to_string()))?
}
```

**Pool size** is configurable per plugin via `PluginInstallConfig`:

```rust
pub struct PluginInstallConfig {
    pub base_priority: Option<i32>,
    pub overrides: HashMap<String, i32>,
    /// Number of WASM instances to pre-warm for this plugin.
    /// Default: number of logical CPUs (std::thread::available_parallelism()).
    /// Increase for plugins under high concurrent load.
    /// Each instance uses its own WASM linear memory (~a few MB per instance).
    pub pool_size: Option<usize>,
}
```

State isolation is preserved across pool instances: all state reads and writes go through host functions (`dx_state_get`, etc.) which are backed by the `session_states` and `global_states` maps in `PluginRuntime`, not inside WASM memory. Different pool instances of the same plugin share external state correctly because the `SessionCtx` carried in `UserData` scopes all reads and writes.

```rust
struct SlotRegistry(HashMap<String, Vec<(i32, PluginId)>>);
struct HookRegistry(HashMap<String, Vec<(i32, PluginId)>>);
```

### 4.2 `TransformRegistry`

The engine for Layers 2 and 3. All declared transforms are indexed by selector type
for O(1) or O(patterns) lookup at render time.

```rust
struct TransformRegistry {
    /// Selector::Component("name") transforms
    by_component: HashMap<String, Vec<TransformEntry>>,
    /// Selector::Slot("name") transforms
    by_slot: HashMap<String, Vec<TransformEntry>>,
    /// Selector::Route(pattern) transforms -- Vec for pattern matching
    by_route: Vec<(RoutePattern, TransformEntry)>,
    /// Selector::DataPluginSlot("value") transforms
    by_data_slot: HashMap<String, Vec<TransformEntry>>,
    /// Selector::Within { outer, inner } transforms
    within: Vec<(Selector, NodeSelector, TransformEntry)>,
}

#[derive(Clone)]
struct TransformEntry {
    plugin_id: PluginId,
    transform_fn: String,
    op: TransformOp,
    /// Resolved at build time from PriorityHint + PluginInstallConfig overrides.
    /// This is the value actually used for ordering; PriorityHint never leaks past build().
    priority: i32,
}

impl TransformRegistry {
    fn for_route(&self, path: &str) -> Vec<TransformEntry> {
        // match all patterns against path, collect, sort by priority desc
    }
    fn for_slot(&self, name: &str) -> Vec<TransformEntry> { ... }
    fn for_component(&self, name: &str) -> Vec<TransformEntry> { ... }
    fn within_for(&self, outer: &Selector) -> Vec<(NodeSelector, TransformEntry)> { ... }
}
```

### 4.3 `PluginRuntimeBuilder`

```rust
pub struct PluginRuntimeBuilder {
    sources: Vec<(PluginSource, PluginInstallConfig)>,
    extra_host_fns: Vec<extism::Function>,
    wasm_cache_path: Option<PathBuf>,
    invocations: Vec<(String, InvocationHandler)>,
}

pub enum PluginSource {
    File(PathBuf),
    /// Remote WASM binary. The sha256 checksum is mandatory — build() aborts
    /// if the fetched bytes don't match. This prevents silent supply-chain
    /// substitution of a plugin binary via a compromised CDN or URL.
    Url {
        url: String,
        sha256: [u8; 32],
    },
    Bytes(Cow<'static, [u8]>),
}

/// Per-plugin installer configuration.
#[derive(Debug, Default)]
pub struct PluginInstallConfig {
    /// If set, overrides ALL of this plugin's PriorityHints with one numeric value.
    pub base_priority: Option<i32>,
    /// Fine-grained overrides by name (transform_fn name, hook_name, or slot name).
    pub overrides: HashMap<String, i32>,
    /// Number of WASM instances to pre-warm for this plugin.
    /// Default: std::thread::available_parallelism().
    pub pool_size: Option<usize>,

    // --- Resource limits (applied via Wasmtime) ---

    /// Maximum Wasmtime "fuel" units consumed per plugin call.
    /// Fuel is deducted per WASM instruction; when exhausted the call returns
    /// an error instead of looping forever. Prevents CPU-runaway plugins.
    /// Default: 10_000_000 (roughly 1–10ms of typical WASM compute).
    /// Set to None to disable (not recommended in production).
    pub max_fuel: Option<u64>,

    /// Maximum wall-clock duration per plugin call via epoch interruption.
    /// A background thread increments the epoch on a timer; plugin calls
    /// trap when their deadline epoch is exceeded. Complements fuel by
    /// catching blocking I/O or sleep() within WASM.
    /// Default: 100ms.
    pub max_call_duration: Option<Duration>,
}

/// Priority resolution order for any single contribution:
///
///   1. PluginInstallConfig::overrides[name]   — most specific, wins
///   2. PluginInstallConfig::base_priority     — plugin-wide override
///   3. PriorityHint::as_numeric()             — plugin author's suggestion
///
/// Ties at the same resolved numeric value are broken by plugin load order
/// (the order add_plugin* was called). This is deterministic and installer-controlled.
impl PluginInstallConfig {
    pub fn resolve(&self, name: &str, hint: &PriorityHint) -> i32 {
        self.overrides
            .get(name)
            .copied()
            .or(self.base_priority)
            .unwrap_or_else(|| hint.as_numeric())
    }
}

impl PluginRuntimeBuilder {
    pub fn new() -> Self;

    /// Load a plugin, using its PriorityHints as declared.
    pub fn add_plugin(self, source: PluginSource) -> Self;

    /// Load a plugin and override ALL of its priorities with one absolute value.
    /// Every slot, hook, and transform from this plugin gets this priority.
    pub fn add_plugin_with_priority(self, source: PluginSource, priority: i32) -> Self {
        self.add_plugin_with_config(source, PluginInstallConfig {
            base_priority: Some(priority),
            ..Default::default()
        })
    }

    /// Load a plugin with fine-grained per-contribution priority overrides.
    pub fn add_plugin_with_config(
        self,
        source: PluginSource,
        config: PluginInstallConfig,
    ) -> Self;

    pub fn add_host_function(self, f: extism::Function) -> Self;
    pub fn with_wasm_cache(self, path: impl Into<PathBuf>) -> Self;

    /// Configure session state TTL. Sessions not accessed within this duration
    /// are evicted by a background task. Default: 24 hours.
    pub fn with_session_ttl(self, ttl: Duration) -> Self;

    /// Provide a persistence backend for GlobalScope plugin state.
    /// On startup, global state is restored from the backend.
    /// On shutdown (or when global state is written), it is flushed.
    /// Default: in-memory only (state lost on restart).
    pub fn with_state_persistence(
        self,
        provider: impl StatePersistenceProvider,
    ) -> Self;

    /// Register a named invocation (see section 4.4).
    pub fn register_invocation<Args, Ret, Fut>(
        self,
        name: impl Into<String>,
        timeout: Option<Duration>,
        handler: impl Fn(Args, SessionCtx) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        Args: DeserializeOwned + Send + 'static,
        Ret: Serialize + Send + 'static,
        Fut: Future<Output = Result<Ret, InvocationError>> + Send + 'static;

    pub async fn build(self) -> Result<Arc<PluginRuntime>, PluginRuntimeError>;
}

/// Persistence backend for GlobalScope plugin state.
/// Implement this to survive server restarts with persistent global state.
#[async_trait]
pub trait StatePersistenceProvider: Send + Sync + 'static {
    async fn save(
        &self,
        plugin_id: &PluginId,
        state: &HashMap<String, serde_json::Value>,
    ) -> Result<(), PersistenceError>;

    async fn load(
        &self,
        plugin_id: &PluginId,
    ) -> Result<Option<HashMap<String, serde_json::Value>>, PersistenceError>;
}

/// Built-in: serialises global state to a JSON file on each write.
/// Suitable for single-process deployments. For distributed deployments,
/// implement StatePersistenceProvider backed by Redis, a database, etc.
pub struct JsonFilePersistence {
    pub path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum PersistenceError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialisation error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Custom(String),
}
}
```

During `build()`:
1. For each plugin source: if `PluginSource::Url`, fetch the binary and verify SHA-256.
   Abort if checksum fails.
2. Instantiate `pool_size` WASM instances via `extism::PluginBuilder`, configuring
   fuel limits and epoch interruption per `PluginInstallConfig`. Register all host
   functions. Default pool size: `available_parallelism()`.
3. Call `manifest` export on instance[0] to obtain `PluginManifest`.
4. **Protocol version check**: if `manifest.min_protocol_version > PROTOCOL_VERSION`,
   abort — the host runtime is too old to safely load this plugin.
5. **HostComponent warning**: log a `tracing::warn!` for any name in
   `manifest.required_host_components` not present in the registered
   `HostComponentRegistry`. Plugin will produce broken views for those references.
6. For each contribution, resolve effective priority via `PluginInstallConfig::resolve`.
7. `slots` → `SlotRegistry`; `hooks` → `HookRegistry`.
8. `transforms` → `TransformRegistry`.
9. **Capability validation**: `Invoke { names }` entries checked against registered
   invocations. Unknown names → error.
10. Call `on_load` export on each instance (if exported). Failure aborts build.
11. Rebuild `Registries` (including `OverrideMap` at version=0) under write lock.
12. Store `override_map_tx` broadcast sender for hot-reload notifications.

### 4.4 `InvocationRegistry`

The registry that backs `dx_invoke`. Each entry is a type-erased async handler
that the host registered at build time.

```rust
/// Type-erased invocation handler stored in the registry.
type InvocationHandler = Arc<
    dyn Fn(serde_json::Value, SessionCtx)
        -> BoxFuture<'static, Result<serde_json::Value, InvocationError>>
        + Send
        + Sync,
>;

pub struct InvocationRegistry {
    /// Each entry includes the handler and the configured timeout.
    handlers: HashMap<String, (InvocationHandler, Duration)>,
}

impl InvocationRegistry {
    pub(crate) async fn call(
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

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InvocationError {
    #[error("invocation not found: {0}")]
    NotFound(String),
    #[error("argument deserialisation failed: {0}")]
    BadArgs(#[from] serde_json::Error),
    /// Structured error returned by the host handler.
    /// `code` is a stable numeric identifier for the error category;
    /// plugins branch on `code` rather than parsing `message`.
    /// Codes 0–999 are reserved for the framework; 1000+ are user-defined.
    #[error("invocation failed (code {code}): {message}")]
    Failed { code: u32, message: String },
    #[error("invocation timed out after {0:?}")]
    Timeout(Duration),
}

impl From<InvocationError> for PluginRuntimeError {
    fn from(e: InvocationError) -> Self {
        PluginRuntimeError::Invocation(e)
    }
}
```

**Why type-erasure here?** Each invocation handler has a unique `Args` and `Ret`
type. Storing them in a `HashMap` requires erasing those types behind `dyn Fn`.
The `register_invocation` builder method on `PluginRuntimeBuilder` wraps the
concrete handler in a closure that deserialises `serde_json::Value` -> `Args` and
serialises `Ret` -> `serde_json::Value`, so the type boundary is managed at
registration time, not at call time.

### 4.5 Host Functions (exposed to plugins)

```
dx_state_get          (key: &str) -> Json<Option<Value>>
dx_state_set          (key: &str, value: Json<Value>)
dx_state_delete       (key: &str)
dx_global_state_get   (key: &str) -> Json<Option<Value>>      [capability-gated]
dx_global_state_set   (key: &str, value: Json<Value>)         [capability-gated]
dx_plugin_state_get   (plugin_id: &str, key: &str) -> ...     [capability-gated]
dx_emit_event         (event: Json<PluginEvent>)
dx_log                (level: &str, message: &str)
dx_http_fetch         (request: Json<HttpRequest>) -> ...      [capability-gated]
dx_invoke             (name: &str, args: Json<Value>) -> Json<Value>  [capability-gated per name]
```

All receive `UserData` carrying `SessionCtx` for the current call.

`dx_invoke` is capability-gated per invocation name: the plugin must have declared
`HostCapability::Invoke { names: ["the_name"] }` in its manifest and had it granted
at build time. Calling an undeclared or ungranted name returns a `CapabilityDenied`
error to the plugin without invoking any host logic.

### 4.6 Plugin Export Convention

```
manifest                 ->  PluginManifest
on_load                  ->  (SessionCtx) -> ()        [optional] called after pool init
on_unload                ->  ()                        [optional] called before pool drop
on_event                 ->  (PluginEvent, SessionCtx) -> ()
slot_{name}              ->  (SessionCtx) -> PluginView
hook_{name}              ->  (HookCall, SessionCtx) -> HookResult
on_interaction           ->  (HandlerId, Value, SessionCtx) -> ViewUpdate
transform_{fn_name}      ->  (TransformInput) -> TransformOutput
```

`on_load` is called once per pool initialisation (build and reload). If it returns
an error, build/reload fails — the plugin is not loaded. Use it to set default state
values, validate required capabilities, or register external resources.

`on_unload` is called before the instance pool is dropped (unload and reload). Use it
to flush state, release external connections, or emit a final event.

### 4.7 `PluginRuntime` Public API

```rust
impl PluginRuntime {
    // --- Layer 1 ---

    /// Collect slot contributions, apply all slot-level and within-slot
    /// transforms, return sorted result.
    pub async fn render_slot(
        &self,
        slot_name: &str,
        session: &SessionCtx,
    ) -> Result<Vec<SlotContent>, PluginRuntimeError>;

    /// Run hook chain.
    pub async fn run_hook<T>(
        &self,
        hook_name: &str,
        context: T,
        session: &SessionCtx,
    ) -> Result<HookOutcome<T>, PluginRuntimeError>
    where T: Serialize + DeserializeOwned;

    // --- Layers 1 & 3 ---

    /// Resolve transforms for a named component.
    /// Returns None if no transforms registered (fast path for OverridableComponent).
    pub async fn resolve_component(
        &self,
        component_name: &str,
        props: serde_json::Value,
        session: &SessionCtx,
    ) -> Result<Option<ComponentResolution>, PluginRuntimeError>;

    // --- Layer 2 ---

    /// Collect route-level transforms for a URL path.
    pub async fn render_route_transforms(
        &self,
        path: &str,
        session: &SessionCtx,
    ) -> Result<RouteTransforms, PluginRuntimeError>;

    // --- Layer 3 ---

    /// Apply within-transforms to a PluginView tree.
    pub async fn apply_tree_transforms(
        &self,
        outer_selector: &Selector,
        view: PluginView,
        context: TransformContext,
        session: &SessionCtx,
    ) -> Result<PluginView, PluginRuntimeError>;

    // --- Shared ---

    pub async fn handle_interaction(
        &self,
        plugin_id: &PluginId,
        handler_id: &HandlerId,
        event_data: serde_json::Value,
        session: &SessionCtx,
    ) -> Result<ViewUpdate, PluginRuntimeError>;

    pub async fn emit_event(
        &self,
        event: PluginEvent,
        session: &SessionCtx,
    ) -> Result<(), PluginRuntimeError>;

    /// Compute the current OverrideMap for boot-time serving.
    /// Reads from the cached copy inside `registries` — zero recomputation.
    pub fn override_map(&self) -> OverrideMap;

    // --- Plugin lifecycle controls ---

    /// Pause a plugin: stop routing requests to it without destroying its
    /// pool or config. Returns empty/Incompatible results until re-enabled.
    pub fn disable_plugin(&self, id: &PluginId) -> Result<(), PluginRuntimeError>;

    /// Resume a previously disabled plugin.
    pub fn enable_plugin(&self, id: &PluginId) -> Result<(), PluginRuntimeError>;

    // --- Hot-reload ---

    /// Replace a loaded plugin atomically. Calls on_unload on old instances,
    /// loads new pool, calls on_load, rebuilds Registries under write lock,
    /// increments OverrideMap::version, broadcasts via override_map_tx.
    pub async fn reload_plugin(
        &self,
        id: &PluginId,
        source: PluginSource,
        config: PluginInstallConfig,
    ) -> Result<(), PluginRuntimeError>;

    /// Remove a plugin entirely. Calls on_unload, rebuilds registries, broadcasts.
    pub async fn unload_plugin(&self, id: &PluginId) -> Result<(), PluginRuntimeError>;

    /// Subscribe to OverrideMap change notifications (used by SSE endpoint).
    pub fn override_map_updates(&self) -> broadcast::Receiver<OverrideMap>;

    // --- SSR ---

    /// Render all slot content for a route in a single synchronous pass,
    /// for use during server-side rendering. Returns pre-populated SlotContents
    /// that can be embedded directly into the SSR HTML without client round-trips.
    pub async fn ssr_render_route(
        &self,
        path: &str,
        client: &ClientCapabilities,
        session: &SessionCtx,
    ) -> Result<SsrRouteOutput, PluginRuntimeError>;
}

/// Output of a full SSR render pass for one route.
pub struct SsrRouteOutput {
    pub route_transforms: RouteTransforms,
    /// Pre-rendered slot contents keyed by slot name.
    pub slots: HashMap<String, Vec<SlotContent>>,
    /// Pre-rendered component resolutions keyed by component name.
    pub components: HashMap<String, Option<ComponentResolution>>,
}
}

pub enum HookOutcome<T> {
    Passed(T),
    Cancelled { by: PluginId, reason: String },
}

pub struct ComponentResolution {
    pub before: Vec<PluginView>,
    pub replacement: Option<PluginView>,  // WrapNode embeds __target__, Override replaces
    pub after: Vec<PluginView>,
}

pub struct RouteTransforms {
    pub before: Vec<PluginView>,
    /// The composed result of running all Wrap transforms through the pipeline.
    /// Each plugin received the previous plugin's output as `original` and
    /// returned a new view. This is the final fold result.
    /// None if no Wrap transforms were registered for this route.
    pub wrap: Option<PluginView>,
    pub after: Vec<PluginView>,
}

impl RouteTransforms {
    pub fn empty() -> Self {
        Self { before: vec![], wrap: None, after: vec![] }
    }
    pub fn is_empty(&self) -> bool {
        self.before.is_empty() && self.wrap.is_none() && self.after.is_empty()
    }
    pub fn has_wrap(&self) -> bool { self.wrap.is_some() }
}
```

### 4.8 Render Pipeline: `render_slot`

```
render_slot("sidebar", &session):

  0. Compatibility pre-filter:
       For each plugin in SlotRegistry["sidebar"]:
         if plugin.enabled == false -> skip entirely
         if session.client.protocol_version < plugin.manifest.min_protocol_version
            OR session.client.app_version < plugin.manifest.min_app_version:
           -> replace contribution with PluginView::Incompatible { reason, fallback: None }
              (plugin is NOT called; Incompatible is produced by the runtime)
           -> still included in results so host can render an update prompt

  1. Collect direct contributions (SlotRegistry), isolated per plugin:
       plugin_a.slot_sidebar(session_ctx)
         -> Ok(PluginView A)    -- included
       plugin_b.slot_sidebar(session_ctx)
         -> Err(FuelExhausted)  -- replaced with PluginView::Incompatible { reason: "...", fallback }
                                   plugin_a's contribution is unaffected
       sorted by priority desc

  2. Apply Selector::Slot("sidebar") transforms (slot-level), each isolated:
       InjectBefore: each plugin's transform call isolated; failure -> skip that transform
       InjectAfter:  same
       Wrap pipeline: if a Wrap plugin call fails, that pipeline step is skipped and
                      the current accumulated view passes through unchanged

  3. Apply Selector::Within transforms, each node transform isolated:
       Failure on one node -> that node is left as-is; traversal continues

  4. Return final Vec<SlotContent>
```

### 4.9 Render Pipeline: `render_route_transforms`

```
render_route_transforms("/product/42", &session):

  1. TransformRegistry::for_route("/product/42"):
       Match all RoutePattern entries against "/product/42"
       Extract params: { "id": "42" }
       Partition entries into: inject_before, wrap, inject_after (by TransformOp)

  2. InjectBefore transforms (sorted by priority desc):
       For each entry:
         call plugin.transform_{fn}(TransformInput {
             original: None,
             context: TransformContext { route_params: { "id": "42" }, .. },
             session,
         })
       -> collect returned views into before: Vec<PluginView>

  3. Wrap transforms -- sequential pipeline (sorted by priority desc):
       seed = PluginView::HostComponent("__content__")
              // resolves to Outlet::<R> {} on the frontend

       fold:
         plugin_y (priority 10):
           receives TransformInput {
               original: Some(seed),
               context: TransformContext { route_params: { "id": "42" }, .. },
               session,
           }
           -> returns view_y  (wraps seed; plugin_y's view becomes current_view)

         plugin_z (priority 5):
           receives TransformInput {
               original: Some(view_y),  // plugin_y's full output, not the seed
               context: TransformContext { route_params: { "id": "42" }, .. },
               session,
           }
           -> returns view_z  (plugin_z sees everything plugin_y added; can react to it)

       final_wrap = Some(view_z)
       Rust implementation:

         let mut current = PluginView::HostComponent(HostComponentRef {
             name: "__content__".into(), ..Default::default()
         });
         for entry in wrap_entries {  // sorted priority desc
             let output = plugins[&entry.plugin_id]
                 .call::<TransformInput, TransformOutput>(
                     &entry.transform_fn,
                     TransformInput { original: Some(current), context, session },
                 ).await?;
             // Warn in debug builds if __content__ is absent from output.view
             #[cfg(debug_assertions)]
             if !contains_content_placeholder(&output.view) {
                 tracing::warn!(
                     plugin = %entry.plugin_id.0,
                     "Wrap transform '{}' did not include original_content(). \
                      Chain is cut here; earlier plugin output is dropped.",
                     entry.transform_fn,
                 );
             }
             current = output.view;
         }
         let final_wrap = Some(current);

  4. InjectAfter transforms (sorted by priority desc):
       Same as InjectBefore, collect into after: Vec<PluginView>

  5. Return RouteTransforms { before, wrap: final_wrap, after }
```

### 4.10 Axum Integration

```rust
pub trait PluginRuntimeExt {
    fn with_plugin_runtime(self, runtime: Arc<PluginRuntime>) -> Self;
}

impl PluginRuntimeExt for Router { ... }

/// Extractor for use in Axum handlers and Dioxus server functions.
pub struct RuntimeExtractor(pub Arc<PluginRuntime>);
```

### 4.11 Error Type

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PluginRuntimeError {
    #[error("plugin not found: {0:?}")]
    PluginNotFound(PluginId),
    #[error("plugin call failed on {plugin:?}: {source}")]
    CallFailed { plugin: PluginId, #[source] source: extism::Error },
    #[error("protocol error: {0}")]
    Protocol(#[from] serde_json::Error),
    #[error("capability denied: {capability} for {plugin:?}")]
    CapabilityDenied { plugin: PluginId, capability: String },
    #[error("transform conflict at {0}: multiple Replace transforms at same priority")]
    TransformConflict(String),
    #[error("invocation error: {0}")]
    Invocation(#[from] InvocationError),
    #[error("plugin task panicked: {0}")]
    TaskPanic(String),
    #[error("plugin {plugin:?} requires protocol v{required}, host supports v{supported}")]
    ProtocolIncompatible { plugin: PluginId, required: u32, supported: u32 },
    #[error("plugin {plugin:?} fuel exhausted after {fuel} units")]
    FuelExhausted { plugin: PluginId, fuel: u64 },
    #[error("plugin {plugin:?} call exceeded {duration:?}")]
    CallTimeout { plugin: PluginId, duration: Duration },
    #[error("checksum mismatch for {url}: expected {expected}, got {actual}")]
    ChecksumMismatch { url: String, expected: String, actual: String },
    #[error("on_load failed for {plugin:?}: {reason}")]
    LoadHookFailed { plugin: PluginId, reason: String },
    #[error("persistence error: {0}")]
    Persistence(#[from] PersistenceError),
}
```

---

## 5. Frontend Layer (`dioxus-extism-frontend`)

### 5.1 Application Root Components

The host developer's plugin integration surface at the application root:

```rust
/// Must wrap the entire application. Fetches OverrideMap at boot and provides
/// it as context. All plugin-aware components depend on this context.
#[component]
pub fn PluginBootProvider(children: Element) -> Element;

/// Provides HostComponentRegistry context. Plugins can reference host
/// components by name inside PluginView trees.
#[component]
pub fn PluginHostComponentProvider(children: Element) -> Element;

/// Register a host component under a string name.
pub fn register_host_component(
    name: impl Into<String>,
    render: fn(serde_json::Value, Vec<PluginView>) -> Element,
);
```

### 5.2 `PluginAwareRouter` (Layer 2)

Drop-in replacement for `Router::<R>`. Enables route injection.

```rust
/// Enables Layer 2 route injection. The host uses this once at the app root.
/// Page components require zero changes.
#[component]
pub fn PluginAwareRouter<R: Routable>() -> Element {
    let path = use_current_path();
    let session_id = use_session_id();
    let override_map = use_context::<OverrideMap>();

    // Local check (zero network): does any registered pattern match this path?
    let has_transforms = override_map.route_patterns.iter().any(|p| p.matches(&path));

    let transforms = use_server_future(move || {
        let path = path.clone();
        let sid = session_id.read().clone();
        async move {
            if has_transforms {
                get_route_transforms(path, sid).await
            } else {
                Ok(RouteTransforms::empty())
            }
        }
    });

    match transforms.value().read().as_ref() {
        Some(Ok(t)) if t.has_wrap() => rsx! {
            for view in &t.before {
                PluginViewRenderer { view: view.clone(), session_id }
            }
            // __content__ inside wrap_view resolves to Outlet::<R> {}
            PluginViewRenderer {
                view: t.wrap.clone().unwrap(),
                session_id,
                content_slot: rsx! { Outlet::<R> {} },
            }
            for view in &t.after {
                PluginViewRenderer { view: view.clone(), session_id }
            }
        },
        Some(Ok(t)) => rsx! {
            for view in &t.before {
                PluginViewRenderer { view: view.clone(), session_id }
            }
            Outlet::<R> {}
            for view in &t.after {
                PluginViewRenderer { view: view.clone(), session_id }
            }
        },
        _ => rsx! { Outlet::<R> {} },
    }
}
```

### 5.3 `PluginSlot` (Layer 1 + Layer 3 applied server-side)

```rust
/// Renders contributions from all plugins registered for this slot.
/// Layer 3 tree transforms are applied by the runtime before delivery.
#[component]
pub fn PluginSlot(
    name: String,
    /// Rendered while the server call is in flight. Prevents layout shift
    /// in visible slots (header, sidebar, etc.). Defaults to empty.
    #[props(default)] loading: Option<Element>,
    #[props(default)] fallback: Option<Element>,
) -> Element {
    let session_id = use_session_id();
    let client_caps = use_context::<ClientCapabilities>();
    let contents = use_server_future(move || {
        let name = name.clone();
        let sid = session_id.read().clone();
        let caps = client_caps.clone();
        async move { get_slot_content(name, sid, caps).await }
    });

    match contents.value().read().as_ref() {
        None => loading.unwrap_or(rsx! {}),  // in-flight
        Some(Ok(c)) if !c.is_empty() => rsx! {
            for content in c {
                PluginViewRenderer { view: content.view.clone(), session_id }
            }
        },
        _ => fallback.unwrap_or(rsx! {}),
    }
}
```

### 5.4 `PluginViewRenderer` (internal)

Recursively renders a `PluginView` tree. Resolves `__content__` and `__target__`
placeholder names to their respective slot elements.

```rust
#[component]
pub(crate) fn PluginViewRenderer(
    view: PluginView,
    plugin_id: PluginId,
    session_id: ReadOnlySignal<SessionId>,
    /// Provided by PluginAwareRouter for Wrap transforms.
    #[props(default)] content_slot: Option<Element>,
    /// Provided by OverridableComponent for WrapNode transforms.
    #[props(default)] target_slot: Option<Element>,
) -> Element {
    // Keyed reconciliation: track the previously rendered view so interactions
    // that return a partial ViewUpdate only re-render changed nodes.
    // Nodes are matched by their `key` field; unkeyed nodes fall back to
    // position-based matching within their parent (same as Dioxus's default).
    let previous_view: Signal<Option<PluginView>> = use_signal(|| None);

    // On each render, diff incoming `view` against `previous_view`.
    // Only nodes whose key is absent from previous OR whose content changed
    // produce new RSX; stable-key nodes with unchanged content are skipped.
    // This is a simple recursive structural diff, not a full VDOM algorithm.
    use_effect(move || { *previous_view.write() = Some(view.clone()); });

    match view {
        PluginView::Text(s) => rsx! { "{s}" },
        PluginView::Empty => rsx! {},
        PluginView::Incompatible { reason, fallback } => {
            // The server determined this plugin cannot render for this client.
            // Log the reason, render the fallback if provided, or nothing.
            tracing::debug!("plugin incompatible: {reason}");
            match fallback {
                Some(f) => PluginViewRenderer { view: *f, plugin_id, session_id },
                None => rsx! {},
            }
        },
        PluginView::Fragment(children) => rsx! {
            for child in children {
                PluginViewRenderer { view: child, plugin_id, session_id,
                    content_slot, target_slot }
            }
        },
        PluginView::HostComponent(r) if r.name == "__content__" => {
            content_slot.unwrap_or(rsx! {})
        },
        PluginView::HostComponent(r) if r.name == "__target__" => {
            target_slot.unwrap_or(rsx! {})
        },
        PluginView::HostComponent(r) => {
            let registry = use_context::<HostComponentRegistry>();
            registry.render(&r.name, r.props, r.children)
        },
        PluginView::Element(el) => {
            render_plugin_element(el, plugin_id, session_id)
        },
    }
}
```

### 5.5 `OverridableComponent` (Layers 1 & 3)

Checks `OverrideMap` locally first. Server round-trip only when transforms are
actually registered for this component name.

```rust
#[component]
pub fn OverridableComponent(
    name: String,
    props: serde_json::Value,
    fallback: Element,
) -> Element {
    let session_id = use_session_id();
    let override_map = use_context::<OverrideMap>();

    if !override_map.overridden_components.contains(&name) {
        // Fast path: no registered transforms. Zero network overhead.
        return fallback;
    }

    let resolution = use_server_future(move || {
        let name = name.clone();
        let props = props.clone();
        let sid = session_id.read().clone();
        async move { resolve_component(name, props, sid).await }
    });

    match resolution.value().read().as_ref() {
        Some(Ok(Some(r))) => rsx! {
            for view in &r.before {
                PluginViewRenderer { view: view.clone(), session_id }
            }
            if let Some(replacement) = &r.replacement {
                // WrapNode: __target__ inside replacement resolves to fallback
                PluginViewRenderer {
                    view: replacement.clone(),
                    session_id,
                    target_slot: fallback,
                }
            } else {
                { fallback }
            }
            for view in &r.after {
                PluginViewRenderer { view: view.clone(), session_id }
            }
        },
        _ => fallback,
    }
}
```

### 5.6 `#[overridable]` Proc Macro (Layer 1)

Applied at component definition sites. The macro wraps the component body in
`OverridableComponent` without touching `#[component]`. Requires all prop types
to implement `serde::Serialize` (compile-time `where` bound).

```rust
// Before:
#[overridable]
#[component]
fn ProductHero(product_id: i64, title: String) -> Element {
    rsx! { /* original impl */ }
}

// After macro expansion (conceptually):
#[component]
fn ProductHero(product_id: i64, title: String) -> Element {
    // Props must implement Serialize; enforced by the where bound.
    let props = serde_json::json!({
        "product_id": product_id,
        "title": title,
    });
    rsx! {
        OverridableComponent {
            name: "ProductHero",
            props,
            fallback: rsx! { /* original body verbatim */ },
        }
    }
}
```

For components with non-serialisable props (closures, `EventHandler`, `Signal`),
`#[overridable]` cannot be used. `<PluginSlot>` or `PluginAwareRouter` are the
alternatives. The compile error from the `where` bound makes this clear.

### 5.7 `use_plugin_state`

```rust
pub fn use_plugin_state<T>(
    plugin_id: impl Into<String>,
    key: impl Into<String>,
) -> ReadOnlySignal<Option<T>>
where T: DeserializeOwned + Clone + PartialEq + Send + Sync + 'static;
```

### 5.8 `use_current_path`

```rust
/// Returns the current URL path. On web: window.location.pathname.
/// On desktop: Dioxus router internal state.
pub fn use_current_path() -> ReadOnlySignal<String>;
```

### 5.9 Session Identity

Session identity is handled by a `SessionProvider` trait with platform-specific
implementations. The host picks the right one for its target; `use_session_id`
reads from whichever provider is in context.

```rust
pub trait SessionProvider: Send + Sync + 'static {
    fn session_id(&self) -> SessionId;
}

pub fn use_session_id() -> ReadOnlySignal<SessionId>;

#[component]
pub fn SessionProviderRoot<P: SessionProvider>(
    provider: P,
    children: Element,
) -> Element;
```

Built-in implementations:
- `WebSessionProvider` — `HttpOnly` + `SameSite=Strict` cookie; survives page refresh.
- `DesktopSessionProvider` — file in `dirs::data_local_dir()`; fd-locked against races.
- `MobileSessionProvider` — OS keychain/keystore via `keyring` crate; survives reinstall.

### 5.10 `HostComponentRegistry`

```rust
pub struct HostComponentRegistry { ... }

impl HostComponentRegistry {
    pub fn render(&self, name: &str, props: serde_json::Value, children: Vec<PluginView>) -> Element;

    /// Returns all registered component names. Used by PluginBootProvider
    /// to populate ClientCapabilities::registered_host_components.
    pub fn names(&self) -> Vec<String>;
}
```

### 5.11 Server Functions and SSE

```rust
#[server] async fn get_slot_content(
    slot: String, session_id: SessionId, client: ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError>;

#[server] async fn get_route_transforms(
    path: String, session_id: SessionId, client: ClientCapabilities,
) -> Result<RouteTransforms, ServerFnError>;

#[server] async fn resolve_component(
    name: String, props: serde_json::Value,
    session_id: SessionId, client: ClientCapabilities,
) -> Result<Option<ComponentResolution>, ServerFnError>;

#[server] async fn dx_handle_interaction(
    plugin_id: PluginId, handler_id: HandlerId,
    event_data: serde_json::Value, session_id: SessionId,
    client: ClientCapabilities,
) -> Result<ViewUpdate, ServerFnError>;

#[server] async fn get_override_map(client: ClientCapabilities)
    -> Result<OverrideMap, ServerFnError>;
```

**SSE endpoint** — registered by `with_plugin_runtime`:

```
GET /_dioxus_extism/override_map_updates
    -> text/event-stream
    -> each event: data: <json OverrideMap>\n\n
```

```rust
/// Must wrap the entire application. Fetches OverrideMap at boot, provides it
/// as a reactive context signal, provides ClientCapabilities as context,
/// checks compatibility, and maintains SSE connection for hot-reload.
#[component]
pub fn PluginBootProvider(children: Element) -> Element {
    let registry = use_context::<HostComponentRegistry>();
    let caps = ClientCapabilities {
        protocol_version: PROTOCOL_VERSION,
        app_version: env!("APP_VERSION").parse().unwrap_or(0),
        registered_host_components: registry.names(),
    };

    let override_map: Signal<OverrideMap> = use_signal(OverrideMap::default);
    provide_context(caps.clone());

    use_future(move || {
        let caps = caps.clone();
        async move {
            if let Ok(map) = get_override_map(caps).await {
                // Check compatibility and inject AppUpdateRequired context if needed
                if map.required_protocol_version > PROTOCOL_VERSION
                || map.required_app_version > caps.app_version {
                    let blocking: Vec<PluginId> = map.plugin_requirements
                        .iter()
                        .filter(|(_, req)| {
                            req.min_protocol_version > caps.protocol_version
                            || req.min_app_version > caps.app_version
                        })
                        .map(|(id, _)| id.clone())
                        .collect();
                    provide_context(AppUpdateRequired {
                        current_protocol: PROTOCOL_VERSION,
                        required_protocol: map.required_protocol_version,
                        current_app: caps.app_version,
                        required_app: map.required_app_version,
                        blocking_plugins: blocking,
                    });
                }
                *override_map.write() = map;
            }
        }
    });

    // SSE for hot-reload — reconnects with exponential back-off on loss
    use_future(move || async move {
        let mut backoff = Duration::from_secs(1);
        loop {
            let mut es = EventSource::new("/_dioxus_extism/override_map_updates");
            while let Some(event) = es.next().await {
                if let Ok(map) = serde_json::from_str::<OverrideMap>(&event.data) {
                    if map.version > override_map.read().version {
                        *override_map.write() = map;
                    }
                }
                backoff = Duration::from_secs(1); // reset on successful message
            }
            // Connection lost — wait and reconnect
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    });

    provide_context(override_map.read_only());
    children
}
```

### 5.12 SSR Mode

In SSR (server-side rendering), plugin content cannot be loaded via client-side
server functions — it must be pre-populated in the initial HTML response.

```rust
/// SSR-aware wrapper for PluginSlot. When rendering on the server,
/// reads from SsrRouteOutput provided in context by the SSR handler.
/// On the client, falls back to the standard use_server_future behaviour.
#[component]
pub fn PluginSlotSsr(
    name: String,
    #[props(default)] loading: Option<Element>,
    #[props(default)] fallback: Option<Element>,
) -> Element;

/// Provides pre-rendered slot and component data as context for SSR rendering.
/// The SSR handler calls PluginRuntime::ssr_render_route() and wraps the
/// route component in this provider before rendering to HTML.
#[component]
pub fn SsrPluginDataProvider(data: SsrRouteOutput, children: Element) -> Element;
```

SSR handler pattern:

```rust
async fn render_page(State(runtime): State<Arc<PluginRuntime>>, ...) -> Html<String> {
    let client = ClientCapabilities::default_ssr(); // protocol_version=current, app_version=0
    let session = build_ssr_session(&client);
    let ssr_data = runtime.ssr_render_route("/product/42", &client, &session).await?;

    let html = dioxus_ssr::render(rsx! {
        SsrPluginDataProvider { data: ssr_data,
            ProductPage { product_id: 42 }
        }
    });
    Html(html)
}
```

The SSE connection and hot-reload are disabled in SSR mode — `PluginBootProvider`
detects the SSR context and skips the EventSource entirely.

---

## 6. PDK Layer (`dioxus-extism-pdk`)

### 6.1 Core Trait

```rust
pub trait DioxusPlugin {
    fn manifest() -> PluginManifest;
}
```

### 6.2 Extension Traits

```rust
pub trait SlotProvider: DioxusPlugin {
    const SLOT_NAME: &'static str;
    const PRIORITY_HINT: PriorityHint = PriorityHint::Normal;
    fn render(ctx: &PluginCtx) -> Result<PluginView, PdkError>;
}

pub trait HookHandler: DioxusPlugin {
    const HOOK_NAME: &'static str;
    const PRIORITY_HINT: PriorityHint = PriorityHint::Normal;
    type Context: Serialize + DeserializeOwned;
    fn handle(context: Self::Context, ctx: &PluginCtx) -> Result<HookResult, PdkError>;
}

pub trait EventSubscriber: DioxusPlugin {
    fn subscriptions() -> &'static [&'static str];
    fn handle(event: &PluginEvent, ctx: &PluginCtx) -> Result<(), PdkError>;
}

pub trait InteractionHandler: DioxusPlugin {
    fn handle(
        handler_id: &HandlerId,
        event_data: serde_json::Value,
        ctx: &PluginCtx,
    ) -> Result<ViewUpdate, PdkError>;
}

/// Optional. Implement to run initialisation logic when the plugin pool is built.
/// Called once per pool init (build and reload). If this returns Err, loading aborts.
/// Use to: set default state, validate capabilities, register external resources.
pub trait OnLoad: DioxusPlugin {
    fn on_load(ctx: &PluginCtx) -> Result<(), PdkError>;
}

/// Optional. Implement to run cleanup when the pool is dropped (unload and reload).
/// Use to: flush state, release external connections, emit a final event.
pub trait OnUnload: DioxusPlugin {
    fn on_unload() -> Result<(), PdkError>;
}
```

### 6.3 `TransformProvider`

Layers 2 and 3. One method dispatches all named transform functions. The `plugin!`
macro generates the routing.

```rust
/// Implement one associated method per TransformDeclaration in the manifest.
/// The plugin! macro generates dispatch from the export to these methods.
pub trait TransformProvider: DioxusPlugin {
    fn transform(
        fn_name: &str,
        input: TransformInput,
        ctx: &PluginCtx,
    ) -> Result<TransformOutput, PdkError>;
}
```

### 6.4 `PluginCtx`

```rust
pub struct PluginCtx {
    pub state: StateAccessor,
    pub emit: EventEmitter,
    pub invoke: InvocationAccessor,
    pub session: SessionCtx,
    /// What the calling client supports. Use to produce compatible output
    /// for older app versions (e.g. skip HostComponent refs not available
    /// in registered_host_components, or emit simpler views for lower
    /// protocol versions). Available in all plugin call contexts.
    pub client: ClientCapabilities,
}

pub struct StateAccessor { /* calls dx_state_* host fns */ }

impl StateAccessor {
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, PdkError>;
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<(), PdkError>;
    pub fn delete(&self, key: &str) -> Result<(), PdkError>;
    pub fn get_global<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, PdkError>;
    pub fn set_global<T: Serialize>(&self, key: &str, value: &T) -> Result<(), PdkError>;
    pub fn get_plugin<T: DeserializeOwned>(
        &self, plugin_id: &PluginId, key: &str,
    ) -> Result<Option<T>, PdkError>;
}

pub struct EventEmitter;
impl EventEmitter {
    pub fn emit<T: Serialize>(&self, name: &str, payload: &T) -> Result<(), PdkError>;
}

/// Calls named host-side invocations via dx_invoke.
/// The plugin must have declared HostCapability::Invoke { names: [...] }
/// in its manifest for each name it calls here.
pub struct InvocationAccessor;

impl InvocationAccessor {
    /// Call a named host invocation. Serialises `args` to JSON, sends via
    /// dx_invoke, deserialises the JSON response back to `Ret`.
    /// Returns PdkError::CapabilityDenied if the name was not granted.
    pub fn call<Args, Ret>(
        &self,
        name: &str,
        args: &Args,
    ) -> Result<Ret, PdkError>
    where
        Args: Serialize,
        Ret: DeserializeOwned;
}
```

### 6.5 View Builder DSL

```rust
pub fn div() -> ViewBuilder;
pub fn span() -> ViewBuilder;
pub fn p() -> ViewBuilder;
pub fn h1() -> ViewBuilder;  // ... h2, h3
pub fn button() -> ViewBuilder;
pub fn input() -> ViewBuilder;
pub fn label() -> ViewBuilder;
pub fn text(s: impl Into<String>) -> PluginView;
pub fn host(name: impl Into<String>) -> HostComponentBuilder;
pub fn fragment(views: Vec<PluginView>) -> PluginView;

/// Placeholder for Wrap/InjectAfter-style transforms:
/// resolves to the original route or slot content on the frontend.
pub fn original_content() -> PluginView {
    PluginView::HostComponent(HostComponentRef { name: "__content__".into(), .. })
}

/// Placeholder for WrapNode transforms:
/// resolves to the original targeted node on the frontend.
pub fn original_target() -> PluginView {
    PluginView::HostComponent(HostComponentRef { name: "__target__".into(), .. })
}

/// Produce an incompatible view when the client is too old to render
/// a specific HostComponent or feature. Use alongside ctx.client checks.
pub fn incompatible(reason: impl Into<String>, fallback: Option<PluginView>) -> PluginView {
    PluginView::Incompatible {
        reason: reason.into(),
        fallback: fallback.map(Box::new),
    }
}


impl ViewBuilder {
    pub fn class(self, c: impl Into<String>) -> Self;
    pub fn attr(self, key: impl Into<String>, val: impl Into<AttrValue>) -> Self;
    /// Stable name for Layer 3 selector targeting.
    pub fn name(self, n: impl Into<String>) -> Self;
    /// Stable key for future view diffing.
    pub fn key(self, k: impl Into<String>) -> Self;
    /// Adds data-plugin-slot attribute, making this node targetable
    /// by Selector::DataPluginSlot from other plugins (Layer 3).
    pub fn plugin_slot(self, slot_name: impl Into<String>) -> Self {
        self.attr("data-plugin-slot", slot_name)
    }
    pub fn on_click(self, handler_id: impl Into<HandlerId>) -> Self;
    pub fn on_input(self, handler_id: impl Into<HandlerId>) -> Self;
    pub fn on_change(self, handler_id: impl Into<HandlerId>) -> Self;
    pub fn debounce(self, ms: u32) -> Self;
    pub fn child(self, v: impl Into<PluginView>) -> Self;
    pub fn children(self, vs: impl IntoIterator<Item = PluginView>) -> Self;
    pub fn build(self) -> PluginView;
}
```

### 6.6 The `plugin!` Macro

```rust
plugin! {
    type: MyPlugin,

    // Layer 1
    slots: [MyPlugin],
    hooks: [MyPlugin],
    events: [MyPlugin],
    interactions: [MyPlugin],

    // Optional lifecycle
    on_load: MyPlugin,    // omit if not implementing OnLoad
    on_unload: MyPlugin,  // omit if not implementing OnUnload

    // Layers 2 & 3
    transforms: [
        inject_sidebar_widget => MyPlugin::inject_sidebar_widget,
        wrap_product_page     => MyPlugin::wrap_product_page,
        enhance_other_plugin  => MyPlugin::enhance_other_plugin,
    ],
}
```

The macro generates:
- `manifest()` with all fields including `min_protocol_version`, `min_app_version`,
  `required_host_components` from associated constants on the plugin type.
- `slot_{name}()`, `hook_{name}()`, `on_event()`, `on_interaction()` exports.
- `transform_{fn_name}()` per transforms entry.
- `on_load()` export only if `on_load: Type` is declared; calls `OnLoad::on_load`.
- `on_unload()` export only if `on_unload: Type` is declared; calls `OnUnload::on_unload`.

### 6.7 `PdkError`

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PdkError {
    #[error("serialisation error: {0}")]
    Serialise(#[from] serde_json::Error),
    #[error("host function failed: {0}")]
    HostFn(String),
    #[error("capability denied: {0}")]
    CapabilityDenied(String),
    #[error("{0}")]
    Custom(String),
}
```

---

## 7. Testing Infrastructure (`dioxus-extism-test`)

Plugin authors and host app developers need to test plugin behaviour without running
a full Dioxus + Axum server. `dioxus-extism-test` provides an in-process synchronous
runtime, mock helpers, and assertion macros. It is a dev-dependency only — it never
ships in production binaries.

### 7.1 `TestRuntime`

```rust
/// In-process plugin test runtime. Loads plugins from WASM bytes synchronously,
/// provides mock host functions, and calls plugin exports directly.
pub struct TestRuntime {
    inner: Arc<PluginRuntime>,
}

impl TestRuntime {
    /// Build a TestRuntime from one or more WASM files or bytes.
    /// Uses sensible test defaults: generous fuel limits, no epoch timeout,
    /// pool_size=1, in-memory state, no persistence.
    pub fn build(plugins: Vec<PluginSource>) -> Result<Self, PluginRuntimeError>;

    /// Override state values before calling a plugin, simulating an existing session.
    pub fn with_session_state(
        &self,
        plugin_id: &PluginId,
        session: &SessionId,
        state: HashMap<String, serde_json::Value>,
    ) -> &Self;

    /// Override global state values.
    pub fn with_global_state(
        &self,
        plugin_id: &PluginId,
        state: HashMap<String, serde_json::Value>,
    ) -> &Self;

    /// Register a mock invocation handler for testing without real DB/services.
    pub fn mock_invocation<Args, Ret>(
        &self,
        name: impl Into<String>,
        handler: impl Fn(Args, SessionCtx) -> Result<Ret, InvocationError> + 'static,
    ) -> &Self
    where
        Args: DeserializeOwned + 'static,
        Ret: Serialize + 'static;

    /// Call a slot export directly and return the PluginView.
    pub fn call_slot(
        &self,
        plugin_id: &PluginId,
        slot_name: &str,
        session: &MockSession,
    ) -> Result<PluginView, PluginRuntimeError>;

    /// Call a hook export and return the HookResult.
    pub fn call_hook<T: Serialize + DeserializeOwned>(
        &self,
        plugin_id: &PluginId,
        hook_name: &str,
        context: T,
        session: &MockSession,
    ) -> Result<HookResult, PluginRuntimeError>;

    /// Call a transform export directly.
    pub fn call_transform(
        &self,
        plugin_id: &PluginId,
        transform_fn: &str,
        input: TransformInput,
    ) -> Result<TransformOutput, PluginRuntimeError>;

    /// Run the full render_slot pipeline (includes transforms and tree selectors).
    pub fn render_slot(
        &self,
        slot_name: &str,
        session: &MockSession,
    ) -> Result<Vec<SlotContent>, PluginRuntimeError>;

    /// Read session state after a plugin call.
    pub fn session_state<T: DeserializeOwned>(
        &self,
        plugin_id: &PluginId,
        session: &SessionId,
        key: &str,
    ) -> Option<T>;

    /// Read emitted events since the last call.
    pub fn emitted_events(&self) -> Vec<PluginEvent>;
}
```

### 7.2 `MockSession`

```rust
/// Builder for constructing test sessions.
pub struct MockSession {
    pub session_id: SessionId,
    pub user_id: Option<String>,
    pub client: ClientCapabilities,
}

impl MockSession {
    pub fn new() -> Self;
    pub fn with_user(self, user_id: impl Into<String>) -> Self;
    /// Simulate an older client to test backward compatibility paths.
    pub fn with_protocol_version(self, v: u32) -> Self;
    pub fn with_app_version(self, v: u32) -> Self;
    pub fn with_host_components(self, names: Vec<String>) -> Self;
}
```

### 7.3 `assert_view!` and `assert_slot!`

```rust
/// Assert that a PluginView matches a structural pattern.
/// Uses insta-style snapshot matching if the `snapshots` feature is enabled,
/// otherwise falls back to structural equality assertions.
///
/// # Examples
///
/// ```rust
/// let view = runtime.call_slot(&plugin_id, "header-actions", &session)?;
///
/// // Assert the view is a div with class "badge" containing some text
/// assert_view!(view, PluginView::Element(el) => {
///     assert_eq!(el.tag, "div");
///     assert!(el.attrs.iter().any(|(k, v)| k == "class"
///         && matches!(v, AttrValue::String(s) if s.contains("badge"))));
/// });
///
/// // Or use snapshot mode (requires insta feature)
/// assert_view_snapshot!(view, "header_actions_badge");
/// ```
#[macro_export]
macro_rules! assert_view { ... }

/// Assert that a full slot render produces the expected number of contributions
/// and optionally inspect each.
#[macro_export]
macro_rules! assert_slot { ... }
```

### 7.4 Example Plugin Test

```rust
// In my_plugin/tests/slot_test.rs
use dioxus_extism_test::*;

#[test]
fn test_header_slot_renders_badge() {
    let runtime = TestRuntime::build(vec![
        PluginSource::Bytes(include_bytes!("../target/wasm32-unknown-unknown/debug/my_plugin.wasm")),
    ]).expect("test runtime build failed");

    let plugin_id = PluginId("acme/my-plugin".into());
    let session = MockSession::new().with_user("user-42");

    runtime.with_session_state(&plugin_id, &session.session_id, hashmap! {
        "rec_count".into() => json!(7),
    });

    let view = runtime.call_slot(&plugin_id, "header-actions", &session)
        .expect("slot call failed");

    assert_view!(view, PluginView::Element(el) => {
        assert_eq!(el.tag, "div");
        assert!(el.attrs.iter().any(|(k, v)|
            k == "class" && matches!(v, AttrValue::String(s) if s == "recs-badge")
        ));
        assert!(matches!(
            el.children.first(),
            Some(PluginView::Text(t)) if t.contains("7 recommendations")
        ));
    });
}

#[test]
fn test_hook_passes_through() {
    let runtime = TestRuntime::build(vec![...]).unwrap();
    let session = MockSession::new();
    let order = json!({ "category": "electronics", "total": 99 });

    let result = runtime.call_hook(
        &PluginId("acme/my-plugin".into()),
        "before_order_submit",
        order,
        &session,
    ).unwrap();

    assert!(matches!(result, HookResult::Continue { .. }));
}

#[test]
fn test_old_client_gets_fallback() {
    let runtime = TestRuntime::build(vec![...]).unwrap();
    // Simulate a mobile client on app version 1 (before NewChart was added)
    let session = MockSession::new().with_app_version(1);
    let view = runtime.call_slot(
        &PluginId("acme/my-plugin".into()),
        "dashboard-widget",
        &session,
    ).unwrap();

    // Plugin should have detected old client and returned fallback table, not NewChart
    assert!(matches!(view, PluginView::Element(el) if el.tag == "table"));
}
```

---

## 8. Key Flows

### 8.1 Layer 1 -- Slot Rendering with Layer 3 Tree Transforms

```
<PluginSlot name="sidebar" />

  get_slot_content("sidebar", session_id)  [server fn]
    |
    `-- render_slot("sidebar", &session)
          |
          |-- 1. Direct contributions (SlotRegistry):
          |       plugin_a.slot_sidebar(session) -> PluginView A
          |       plugin_b.slot_sidebar(session) -> PluginView B
          |       sorted by priority desc
          |
          |-- 2. Selector::Slot("sidebar") transforms (slot-level):
          |       plugin_c: InjectAfter -> append plugin_c's view
          |       plugin_d: Wrap -> sequential pipeline (see section 1.5):
          |           seed = __content__, plugin_d receives it as original,
          |           returns its view with original_content() embedded inside
          |
          `-- 3. Selector::Within { Slot("sidebar"), HasClass("badge") } transforms:
                  Traverse PluginView A tree, find .badge nodes.
                  plugin_e.transform_enhance_badge(TransformInput { original: Some(node), .. })
                  -> WrapNode: node now wrapped with plugin_e's view,
                     __target__ inside resolves to original .badge node

  -> Final Vec<SlotContent> rendered by PluginViewRenderer
```

### 8.2 Layer 1 -- Hook Chain

```
runtime.run_hook("before_order_submit", order, &session):

  HookRegistry (sorted by priority desc):
    (20, plugin_c) -> Continue { context: order }          unchanged
    (10, plugin_a) -> Replace  { context: modified_order } propagated forward
    ( 0, plugin_b) -> Cancel   { reason: "validation" }   chain stops

  -> HookOutcome::Cancelled { by: plugin_b, reason: "validation" }
```

### 8.3 Layer 2 -- Route Injection with Multiple Wrap Transforms

```
User navigates to /product/42.
Two plugins both declared Wrap for Selector::Route("/product/:id").

PluginAwareRouter:
  path = "/product/42"
  override_map.route_patterns contains "/product/:id" -> has_transforms = true

  get_route_transforms("/product/42", session_id)  [server fn]
    |
    `-- render_route_transforms("/product/42", &session)
          |
          TransformRegistry::for_route("/product/42"):
            entries: [(10, plugin_y, Wrap), (5, plugin_z, Wrap), (0, plugin_x, InjectAfter)]

          Wrap pipeline (priority desc):

            seed = HostComponent("__content__")  // will resolve to Outlet::<R> {}

            Step 1 -- plugin_y (priority 10):
              receives TransformInput { original: Some(seed) }
              returns:
                div.class="analytics-shell"
                  |-- div.class="analytics-header" "Tracking active"
                  `-- original_content()   // <- seed lands here

              current_view = plugin_y's output

            Step 2 -- plugin_z (priority 5):
              receives TransformInput { original: Some(current_view) }
              // plugin_z sees plugin_y's full output, not just the seed
              // it can inspect it, react to "analytics-shell", build around it
              returns:
                div.class="theme-wrapper dark"
                  |-- original_content()   // <- plugin_y's tree lands here
                  `-- div.class="theme-footer" "Powered by plugin_z"

              current_view = plugin_z's output

          final_wrap = Some(plugin_z's output)

          InjectAfter: plugin_x returns a widget view

          RouteTransforms {
            before: [],
            wrap:   Some(plugin_z_view),
            after:  [plugin_x_view],
          }

  Frontend (PluginAwareRouter):
    PluginViewRenderer {
        view: plugin_z_view,
        content_slot: rsx! { Outlet::<R> {} }   // __content__ chains through:
                                                  // plugin_z.__content__ -> plugin_y's tree
                                                  // plugin_y.__content__ -> Outlet::<R> {}
    }
    PluginViewRenderer { view: plugin_x_view }

  Rendered DOM:
    div.theme-wrapper.dark
      |-- div.analytics-shell
      |     |-- div.analytics-header "Tracking active"
      |     `-- [actual ProductPage output from Outlet]
      `-- div.theme-footer "Powered by plugin_z"
    [plugin_x's widget]

  ProductPage writes zero plugin-related code.
  plugin_y and plugin_z coordinated without knowing about each other.
```

### 8.4 Layer 3 -- Plugin-on-Plugin Composition

```
Plugin A contributes to Slot("activity-feed"):
  div.name="feed-root"
    `-- div.plugin_slot("feed-item-actions")
          `-- text "Entry 1"

Plugin B declared:
  TransformDeclaration {
      selector: Selector::Within {
          outer: Selector::Slot("activity-feed"),
          inner: NodeSelector::DataAttr("data-plugin-slot", "feed-item-actions"),
      },
      transform_fn: "inject_feed_actions",
      op: TransformOp::InsertAfter,
      priority_hint: PriorityHint::Normal,
  }

render_slot("activity-feed"):
  Step 1: Plugin A's view collected.
  Step 3: Within transform fires.
    Traverse Plugin A's tree -> finds div with data-plugin-slot="feed-item-actions"
    plugin_b.transform_inject_feed_actions(TransformInput {
        original: None,  // InsertAfter doesn't need original
        context: TransformContext { slot_name: Some("activity-feed"), .. },
    })
    -> Plugin B's action buttons appended after the matched div

Final tree:
  div.name="feed-root"
    |-- div[data-plugin-slot="feed-item-actions"]
    |     `-- text "Entry 1"           <- Plugin A's original content
    `-- [plugin B's action buttons]    <- inserted by Plugin B, zero host involvement
```

### 8.5 `#[overridable]` with WrapNode Transform

```
Host RSX (completely unmodified page):
  rsx! {
      header {}
      main {
          ProductHero { product_id }   <- expands to OverridableComponent via macro
      }
  }

Plugin X manifest:
  TransformDeclaration {
      selector: Selector::Component("ProductHero"),
      transform_fn: "wrap_hero",
      op: TransformOp::WrapNode,
      priority_hint: PriorityHint::Normal,
  }

OverridableComponent("ProductHero"):
  1. override_map.overridden_components.contains("ProductHero") -> true
  2. resolve_component("ProductHero", props, session_id)  [server fn]
       plugin_x.transform_wrap_hero(TransformInput { original: None, props, .. })
       -> PluginView:
           div.class="hero-wrapper"
             |-- div.class="sponsored-label" "Sponsored"
             |-- HostComponent("__target__")   <- placeholder for original
             `-- div.class="cta" "Buy Now"
  3. ComponentResolution { replacement: Some(above_view), .. }
  4. Frontend:
       PluginViewRenderer {
           view: above_view,
           target_slot: rsx! { DefaultProductHero { product_id } }
       }
       -> __target__ renders DefaultProductHero inside plugin_x's wrapper
```

### 8.6 Event Bus

```
Plugin A (inside slot render) calls ctx.emit.emit("cart_updated", &payload)?
  -> dx_emit_event host fn
  -> runtime.emit_event(PluginEvent { source: Plugin(a), name: "cart_updated", .. })
  -> EventBus dispatches to all "cart_updated" subscribers
  -> Plugin B's on_event export called
  -> Plugin B: ctx.state.set("cart_count", &new_count)?
  -> On next PluginSlot render for Plugin B, new state is reflected
```

### 8.7 Host Invocation

```
Plugin calls ctx.invoke.call::<GetProductArgs, Product>("get_product", &args)?

  -> dx_invoke("get_product", json_args) host fn
       |
       Check: plugin manifest declared HostCapability::Invoke { names: ["get_product"] }
       Check: name "get_product" was granted at build time
       -> if denied: return CapabilityDenied to plugin immediately, no handler called
       |
       InvocationRegistry::call("get_product", json_args, session_ctx)
         |
         Type-erased handler fires (registered at startup):
           async |args: GetProductArgs, session| {
               db_pool.get_product(args.product_id, session.user_id).await
           }
         |
         Serialises Result<Product, InvocationError> -> Json<Value>
  -> Json<Value> returned to plugin via dx_invoke
  -> InvocationAccessor deserialises Json<Value> -> Product
  -> plugin receives typed Product value

The DB pool, service layer, or any other host resource is fully accessible
inside the handler because it is regular Rust code on the host — not in WASM.
The plugin only ever sees serialised JSON; the host boundary is never crossed.
```

### 8.8 Interaction Handling

```
User clicks a button inside a plugin-rendered view.
BoundEventHandler { event: Click, handler_id: "increment" } was in the PluginView.
PluginViewRenderer generated: onclick -> dx_handle_interaction(plugin_id, "increment", {})

  -> runtime.handle_interaction(&plugin_id, "increment", {}, &session)
  -> plugin.on_interaction("increment", {}, session_ctx)
  -> Plugin increments state, returns ViewUpdate { view: Some(new_view), events: [] }
  -> Frontend slot signal updated -> PluginViewRenderer re-renders with keyed diff
```

### 8.9 Client Compatibility Check

```
Mobile app (app_version=2, protocol_version=1) starts up.
PluginBootProvider calls get_override_map(ClientCapabilities { protocol_version: 1, app_version: 2, ... })

Server side:
  PluginRuntime has two loaded plugins:
    - analytics (min_app_version=1, min_protocol_version=1)  <- compatible
    - recommendations (min_app_version=3, min_protocol_version=1)  <- incompatible! app_version 2 < 3

  OverrideMap returned:
    required_protocol_version: 1  (max across plugins)
    required_app_version: 3       (max across plugins)
    plugin_requirements: {
      "acme/recommendations": { min_app_version: 3, min_protocol_version: 1 }
    }

PluginBootProvider detects required_app_version (3) > client app_version (2):
  provide_context(AppUpdateRequired {
    current_app: 2, required_app: 3,
    blocking_plugins: [PluginId("acme/recommendations")],
    ..
  })

Host app reads AppUpdateRequired context and renders:
  "The Recommendations plugin requires app version 3 or later. Please update."

Later, when recommendations slot is rendered:
  render_slot("header-actions", &session):
    analytics -> called normally -> Ok(PluginView)
    recommendations -> min_app_version (3) > client app_version (2)
      -> runtime produces PluginView::Incompatible { reason: "requires app v3", fallback: None }
      -> plugin WASM is NOT called; Incompatible is constructed by the runtime

Frontend PluginViewRenderer encounters PluginView::Incompatible:
  -> logs reason at debug level
  -> renders nothing (or fallback if provided)
  -> analytics contribution still renders normally
```

---

## 9. Integration Example

### 9.1 Host Application

The host developer's complete plugin integration surface:

1. `PluginRuntimeBuilder` at startup — including `register_invocation` for any host logic plugins should be able to call.
2. `PluginBootProvider` + `PluginHostComponentProvider` + `PluginAwareRouter` at root.
3. `#[overridable]` on component definitions to explicitly expose.
4. `<PluginSlot>` for explicit named injection points.
5. Optional `.plugin_slot("name")` on elements inside slot content for Layer 3 anchors.

Page components are written completely normally with zero plugin awareness.

```rust
// main.rs
#[tokio::main]
async fn main() {
    // Set up whatever the host app needs — DB pool, services, etc.
    let db = Database::connect(&env::var("DATABASE_URL").unwrap()).await.unwrap();
    let db = Arc::new(db);

    let runtime = PluginRuntimeBuilder::new()
        // Analytics plugin declares PriorityHint::Last — we accept that as-is.
        .add_plugin(PluginSource::File("./plugins/analytics.wasm".into()))
        // Recommendations plugin declares PriorityHint::Normal for its route Wrap,
        // but we want it to run after analytics in the Wrap pipeline.
        // analytics resolves to 0 (Last); we set recommendations to 100 so it runs
        // after analytics (higher priority = runs first in the fold, wraps outermost).
        .add_plugin_with_config(
            PluginSource::File("./plugins/recommendations.wasm".into()),
            PluginInstallConfig {
                base_priority: None,
                overrides: [
                    // Only override the specific Wrap transform; everything else
                    // (slot, hook) stays at the plugin's declared PriorityHint.
                    ("inject_recs_panel".into(), 100),
                ].into(),
            },
        )
        // Expose host-side logic to plugins by name.
        // The closure captures the DB pool (or any other resource) directly.
        // Plugins must declare HostCapability::Invoke { names: ["get_product"] }
        // in their manifest to be granted access.
        .register_invocation("get_product", {
            let db = db.clone();
            move |args: GetProductArgs, session: SessionCtx| {
                let db = db.clone();
                async move {
                    db.products()
                        .find_by_id(args.product_id, session.user_id)
                        .await
                        .map_err(|e| InvocationError::Failed(e.to_string()))
                }
            }
        })
        .register_invocation("create_note", {
            let db = db.clone();
            move |args: CreateNoteArgs, session: SessionCtx| {
                let db = db.clone();
                async move {
                    db.notes()
                        .insert(session.user_id.as_deref(), &args.content)
                        .await
                        .map_err(|e| InvocationError::Failed(e.to_string()))
                }
            }
        })
        .build()
        .await
        .expect("plugin runtime init failed");

    let cfg = dioxus::fullstack::Config::new()
        .with_axum_router(|router| router.with_plugin_runtime(runtime));

    dioxus::launch_with_props(App, AppProps {}, cfg);
}

fn App() -> Element {
    rsx! {
        // The only plugin-aware code in the entire application.
        PluginBootProvider {
            PluginHostComponentProvider {
                { register_host_component("UserBadge", |p, _| rsx! { UserBadge {} }) }
                { register_host_component("ProductCard", |p, _| rsx! { ProductCard {} }) }
                PluginAwareRouter::<Route> {}  // Layer 2 enabled
            }
        }
    }
}

// ProductPage -- zero plugin awareness. Plugins inject via:
// - Layer 1: the header-actions slot
// - Layer 1: ProductHero being #[overridable]
// - Layer 2: any plugin declaring Selector::Route("/product/:id")
// - Layer 3: any plugin targeting named points in slot output
#[component]
fn ProductPage(product_id: i64) -> Element {
    rsx! {
        header {
            PluginSlot { name: "header-actions" }  // Layer 1 explicit point
        }
        main {
            ProductHero { product_id }   // Layer 1: #[overridable] makes this extensible
            ProductDetails { product_id }
            RelatedProducts { product_id }
        }
    }
}

// Layer 1: one attribute makes this component overridable everywhere it is used.
#[overridable]
#[component]
fn ProductHero(product_id: i64) -> Element {
    rsx! {
        section { class: "hero",
            h1 { "Product {product_id}" }
        }
    }
}

// Completely normal component. Only reachable by plugins via Layer 2 (route wrap)
// or if host adds #[overridable] or a PluginSlot inside it.
#[component]
fn ProductDetails(product_id: i64) -> Element {
    rsx! { div { "Details for {product_id}" } }
}

// Server-side hook usage -- same pattern regardless of the layer used for UI.
#[server]
async fn submit_order(order: Order) -> Result<OrderId, ServerFnError> {
    let RuntimeExtractor(runtime) = extract().await?;
    let session = extract_session().await?;

    let outcome = runtime
        .run_hook("before_order_submit", order, &session)
        .await
        .map_err(ServerFnError::from)?;

    match outcome {
        HookOutcome::Passed(order) => {
            let id = do_submit(order).await?;
            runtime.emit_event(
                PluginEvent {
                    source: EventSource::Host,
                    name: "order_submitted".into(),
                    payload: json!({ "order_id": id }),
                },
                &session,
            ).await?;
            Ok(id)
        }
        HookOutcome::Cancelled { by, reason } => {
            Err(ServerFnError::ServerError(format!("{by}: {reason}")))
        }
    }
}
```

### 9.2 A Plugin Using All Three Layers

This plugin was written by a third-party developer with no coordination with the
host developer beyond: (a) the host used `PluginAwareRouter`, and (b) the host
put `#[overridable]` on `ProductHero`.

```rust
use dioxus_extism_pdk::prelude::*;

struct RecommendationsPlugin;

impl DioxusPlugin for RecommendationsPlugin {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("acme/recommendations".into()),
            version: "1.0.0".into(),
            state_scope: StateScope::PerSession,

            // Layer 1: contribute to a named slot
            slots: vec![
                SlotRegistration {
                    name: "header-actions".into(),
                    priority_hint: PriorityHint::Normal,
                },
            ],

            hooks: vec![
                HookRegistration {
                    hook_name: "before_order_submit".into(),
                    // Run before most things but not necessarily first;
                    // security plugins would use PriorityHint::First.
                    priority_hint: PriorityHint::High,
                },
            ],

            event_subscriptions: vec!["order_submitted".into()],

            // Declare which host invocations this plugin needs.
            // The host grants or denies each name at build time.
            host_capabilities: vec![
                HostCapability::Invoke {
                    names: vec!["get_product".into(), "create_note".into()],
                },
            ],

            // Layers 2 & 3: all dynamic extension declared here
            transforms: vec![
                // Layer 2: inject a recommendations panel after the product page
                TransformDeclaration {
                    selector: Selector::Route(RoutePattern("/product/:id".into())),
                    transform_fn: "inject_recs_panel".into(),
                    op: TransformOp::InjectAfter,
                    priority_hint: PriorityHint::Normal,
                },
                // Layer 1+3: wrap the #[overridable] ProductHero
                TransformDeclaration {
                    selector: Selector::Component("ProductHero".into()),
                    transform_fn: "wrap_hero_sponsored".into(),
                    op: TransformOp::WrapNode,
                    priority_hint: PriorityHint::Normal,
                },
                // Layer 3: target the analytics plugin's slot output
                // No host involvement required.
                TransformDeclaration {
                    selector: Selector::Within {
                        outer: Box::new(Selector::Slot("header-actions".into())),
                        inner: NodeSelector::HasClass("analytics-badge".into()),
                    },
                    transform_fn: "enhance_analytics_badge".into(),
                    op: TransformOp::WrapNode,
                    priority_hint: PriorityHint::Low,
                },
            ],

            ..Default::default()
        }
    }
}

// Layer 1: slot contribution
impl SlotProvider for RecommendationsPlugin {
    const SLOT_NAME: &'static str = "header-actions";
    const PRIORITY_HINT: PriorityHint = PriorityHint::Normal;

    fn render(ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        let count: u32 = ctx.state.get("rec_count")?.unwrap_or(0);
        Ok(div()
            .class("recs-badge")
            .child(text(format!("{count} recommendations ready")))
            .build())
    }
}

// Layer 2: inject after the product page (no PluginSlot was placed by host)
fn inject_recs_panel(input: TransformInput, ctx: &PluginCtx) -> Result<TransformOutput, PdkError> {
    let product_id = input.context.route_params.get("id").cloned().unwrap_or_default();

    // Call the host's get_product function directly.
    // This runs real host-side code: DB query, auth check, whatever the host registered.
    // The plugin never sees the DB; it only receives the serialised result.
    let product = ctx.invoke.call::<GetProductArgs, Product>(
        "get_product",
        &GetProductArgs { product_id: product_id.parse().unwrap_or(0) },
    )?;

    let recs: Vec<String> = ctx.state.get("recs")?.unwrap_or_default();
    let view = div()
        .class("recommendations-panel")
        .child(h2().child(text(format!("More like \"{}\"", product.title))).build())
        .children(recs.iter().map(|r|
            host("ProductCard")
                .props(json!({ "id": r, "product_id": &product_id }))
                .build()
        ))
        .build();
    Ok(TransformOutput { view })
}

// Layer 1+3: wrap ProductHero with a sponsored banner
fn wrap_hero_sponsored(input: TransformInput, _ctx: &PluginCtx) -> Result<TransformOutput, PdkError> {
    let view = div()
        .class("sponsored-wrapper")
        .child(div().class("sponsored-label").child(text("Sponsored")).build())
        .child(original_target())  // the original ProductHero renders here
        .build();
    Ok(TransformOutput { view })
}

// Layer 3: wrap another plugin's output -- zero host involvement
fn enhance_analytics_badge(input: TransformInput, _ctx: &PluginCtx) -> Result<TransformOutput, PdkError> {
    let view = div()
        .class("enhanced-badge-wrapper")
        .child(span().class("rec-dot").build())
        .child(original_target())  // the analytics badge renders here
        .build();
    Ok(TransformOutput { view })
}

impl HookHandler for RecommendationsPlugin {
    const HOOK_NAME: &'static str = "before_order_submit";
    type Context = Order;

    fn handle(order: Order, ctx: &PluginCtx) -> Result<HookResult, PdkError> {
        ctx.state.set("last_order_category", &order.category)?;
        Ok(HookResult::Continue { context: serde_json::to_value(order)? })
    }
}

impl EventSubscriber for RecommendationsPlugin {
    fn subscriptions() -> &'static [&'static str] { &["order_submitted"] }

    fn handle(event: &PluginEvent, ctx: &PluginCtx) -> Result<(), PdkError> {
        let count: u32 = ctx.state.get("rec_count")?.unwrap_or(0);
        ctx.state.set("rec_count", &(count + 1))?;
        Ok(())
    }
}

// One macro call wires all exports.
plugin! {
    type: RecommendationsPlugin,
    slots: [RecommendationsPlugin],
    hooks: [RecommendationsPlugin],
    events: [RecommendationsPlugin],
    transforms: [
        inject_recs_panel      => inject_recs_panel,
        wrap_hero_sponsored    => wrap_hero_sponsored,
        enhance_analytics_badge => enhance_analytics_badge,
    ],
}
```

---

## 10. Open Design Decisions

**9.1 Wrap transform model: resolved — sequential pipeline** ✅

Multiple `TransformOp::Wrap` plugins targeting the same route, slot, or component
participate as a sequential fold, not a competition. Sorted by priority descending,
each plugin receives the current accumulated `PluginView` as `TransformInput::original`
and returns a new view. The final fold result is what the frontend renders.

Three models were considered:

- **Demotion** (priority winner takes Wrap; others become InjectBefore): dishonest — a
  plugin declared Wrap and gets a different op silently. Breaks in unpredictable ways
  when a higher-priority plugin is loaded later.
- **Nesting** (each plugin wraps the previous plugin's *placeholder*, building Russian
  dolls): honest but each plugin gets an opaque `__content__` hole, not the actual
  accumulated view. Plugins cannot see or react to what others contributed. Also
  imposes O(n) DOM wrapper elements.
- **Pipeline** (sequential fold, each plugin receives the previous plugin's full output):
  honest, cooperative, zero extra DOM overhead. Later plugins see everything earlier
  ones added and can build in response. The mental model matches `tower::Layer` and
  HTTP middleware, which Rust developers already understand.

Pipeline is the only model where multiple Wrap plugins can genuinely cooperate. The
fold implementation is a simple `for` loop, simpler than nesting (which would require
deferred placeholder resolution).

**Consequences recorded in the implementation:**
- `render_slot` and `render_route_transforms` both fold Wrap transforms sequentially.
- `TransformInput::original` carries the accumulated view for Wrap ops, `None` for
  InjectBefore/After.
- Plugins that omit `original_content()` from their Wrap output cut the chain
  intentionally. `tracing::warn!` fires in debug builds when this occurs.
- `TransformConflict` error remains for `Replace` ops (two plugins both replacing the
  same named component at the same priority is still a genuine conflict), but is no
  longer applicable to Wrap.

**9.2 View diff: resolved — keyed reconciliation in v1** ✅

`ViewUpdate::view` is applied as a keyed diff against the currently rendered
`PluginView`, not a full replacement. `ViewElement::key` is the stable identity
used for matching. Nodes with matching keys update in place; nodes whose key is
absent or whose position changed are replaced. `ViewElement::name` is the stable
identity used for selector targeting (Layer 3). Both fields are used in v1.
`render_plugin_element` forwards `key` as a Dioxus RSX `key` attribute so
Dioxus's own diffing engine handles keyed list reconciliation correctly.

**9.3 Concurrency: resolved — instance pool + spawn_blocking** ✅

Extism's `Plugin` is `Send` but not `Sync`. It can be moved across threads but
not shared by reference concurrently. `Plugin::call()` is synchronous and blocking
— running it on an async executor thread starves the runtime.

Two problems, two solutions applied together:

1. **Runtime starvation** → every plugin call runs inside `tokio::task::spawn_blocking`,
   moving it off the async executor onto a dedicated blocking thread pool.
2. **Sequential calls under concurrent load** → each `LoadedPlugin` holds a pool of
   `N` independently-owned WASM instances (`Vec<Mutex<extism::Plugin>>`). Concurrent
   requests to the same plugin each acquire a different pool instance and run in
   parallel. Default pool size: `available_parallelism()`. Configurable per plugin
   via `PluginInstallConfig::pool_size`.

State isolation is preserved: all state lives in `PluginRuntime`'s `session_states`
and `global_states` maps, not in WASM memory. Multiple instances of the same plugin
share external state correctly because every call carries a `SessionCtx` in `UserData`.

**9.4 NodeSelector depth: resolved — shallow by default, opt-in Recursive** ✅

`Within { outer, inner }` tests `inner` only against direct children of the outer
selection by default. Wrap any `NodeSelector` in `NodeSelector::Recursive(inner)` to
match at any depth. `DataAttr(key, value)` added as a first-class variant alongside
`HasClass`. Both are shallow by default; both can be wrapped in `Recursive`.

**9.5 `#[overridable]` and non-serialisable props** (noted, no change needed)

Components with non-`Serialize` props cannot use `#[overridable]`. The compile error
from the generated `where` bound is the diagnostic. `<PluginSlot>` or
`PluginAwareRouter` are the alternatives. Document this prominently in the PDK guide.

**9.6 Priority model: resolved — hint from plugin, control by installer** ✅

See section 3.2 and 4.3 for full detail.

**9.7 Session identity: resolved — SessionProvider trait, platform impls** ✅

`SessionProvider` is a trait with three built-in implementations:
- `WebSessionProvider`: `HttpOnly` + `SameSite=Strict` cookie, survives page refresh.
- `DesktopSessionProvider`: file in `dirs::data_local_dir()`, fd-locked against races.
- `MobileSessionProvider`: OS keychain/keystore via `keyring` crate; survives reinstall.

Custom implementations can tie session identity to the host app's auth system.
`SessionProviderRoot<P>` installs the provider as context at the application root.

**9.8 Hot-reload: resolved — atomic registry swap + SSE broadcast** ✅

`reload_plugin` and `unload_plugin` on `PluginRuntime`:
1. Load and validate the new WASM source.
2. Under a write lock: swap the `LoadedPlugin`, rebuild all registries atomically,
   increment `OverrideMap::version`.
3. Broadcast the new `OverrideMap` via a `tokio::broadcast` channel.

An SSE endpoint (`GET /_dioxus_extism/override_map_updates`) streams each broadcast
to connected frontends. `PluginBootProvider` subscribes on mount and updates the
reactive `OverrideMap` context signal when a higher-version map arrives. All
components reading from the context re-render automatically. In-flight requests
using old plugin instances complete normally (pool instances are only dropped when
all guards are released).

**9.9 Invocation error surface: resolved — structured error with code** ✅

`InvocationError::Failed { code: u32, message: String }`. Codes 0–999 reserved for
the framework; 1000+ are user-defined. Plugins branch on `code` to handle known
error categories; `message` is for humans. `InvocationError::Timeout(Duration)` is
a distinct variant so plugins can distinguish timeouts from application errors.

**9.10 Invocation timeouts: resolved — per-invocation Duration, default 5s** ✅

`register_invocation` accepts `timeout: Option<Duration>`. `None` uses the framework
default of 5 seconds. `Duration::MAX` disables the timeout. `InvocationRegistry::call`
wraps the handler future in `tokio::time::timeout` and returns
`InvocationError::Timeout` if it fires.

---

## 11. Implementation Roadmap

All three extension layers and all critical features ship in v1.

### Phase 1 -- Protocol and Foundation

- [ ] `dioxus-extism-protocol`: `PROTOCOL_VERSION`, `ClientCapabilities`,
  `AppUpdateRequired`, `PluginClientRequirement`, `PluginManifest` with
  `min_protocol_version` + `min_app_version` + `required_host_components`,
  `PluginView::Incompatible`, `OverrideMap` with version + compatibility fields,
  `SessionCtx` with `client` + `caller`, `TransformContext` with `client`,
  `PriorityHint`, `ViewElement` with `name` + `key`, all other protocol types
- [ ] `dioxus-extism-host`: `PluginRuntime` with `RwLock<Registries>` +
  `broadcast::Sender<OverrideMap>`, `LoadedPlugin` pool + `spawn_blocking` +
  fuel/epoch resource limits + `enabled: AtomicBool` + `config: PluginInstallConfig`,
  `PluginRuntimeBuilder` with `add_plugin` / `add_plugin_with_priority` /
  `add_plugin_with_config` / `with_session_ttl` / `with_state_persistence`,
  `PluginInstallConfig` with `pool_size` + `max_fuel` + `max_call_duration`,
  `PluginSource::Url` with mandatory SHA-256 integrity check,
  `PluginId` in `UserData` (fixes capability enforcement),
  `StatePersistenceProvider` trait + `JsonFilePersistence`,
  session TTL background eviction task,
  host functions for state + logging, `SlotRegistry`, `HookRegistry`
- [ ] `dioxus-extism-frontend`: `PluginBootProvider` (initial fetch + compatibility
  check + `AppUpdateRequired` context + SSE with exponential back-off reconnect),
  `ClientCapabilities` provided as context, `HostComponentRegistry::names()`,
  `SessionProviderRoot` + `WebSessionProvider` + `DesktopSessionProvider` +
  `MobileSessionProvider`, `PluginSlot` with `loading` prop,
  `PluginViewRenderer` with `Incompatible` handling + keyed diffing
- [ ] `dioxus-extism-pdk`: `DioxusPlugin`, `SlotProvider`, `OnLoad`, `OnUnload`,
  `PluginCtx` with `client: ClientCapabilities`, `plugin!` macro (manifest +
  slot + on_load + on_unload), `incompatible()` view DSL helper
- [ ] SSE endpoint + `PluginRuntime::override_map_updates()` broadcast
- [ ] Protocol version check in `build()` + `HostComponent` warning + `on_load` call
- [ ] `hello-plugin` example

### Phase 2 -- Behaviour

- [ ] Hook chain: `run_hook`, `HookRegistry`, `HookResult`, `HookHandler`
- [ ] Event bus: `EventBus`, `emit_event`, `on_event`, `EventSubscriber`
- [ ] Interaction handling: `dx_handle_interaction`, `InteractionHandler`, keyed diff
- [ ] `InvocationRegistry` with per-handler `(InvocationHandler, Duration)`
- [ ] `register_invocation` with `timeout: Option<Duration>`
- [ ] `InvocationError::Failed { code, message }` + `Timeout`
- [ ] `dx_invoke` with `caller: PluginId` capability check
- [ ] `InvocationAccessor` + `ctx.invoke.call()` in PDK
- [ ] Per-plugin error isolation in all render pipelines
- [ ] `dioxus-extism-test`: `TestRuntime`, `MockSession`, `assert_view!`,
  `assert_slot!`, mock invocation support
- [ ] `hook-example` + `invocation-example`

### Phase 3 -- Layer 1 Full (`#[overridable]` and Component Overrides)

- [ ] `TransformRegistry` data structure
- [ ] `TransformOp::Replace` and `WrapNode` for `Selector::Component`
- [ ] `resolve_component` in runtime + server fn
- [ ] `OverridableComponent` with zero-cost fast path
- [ ] `#[overridable]` proc macro in `dioxus-extism-macros`
- [ ] `PluginHostComponentProvider` + `HostComponentRegistry`
- [ ] `TransformProvider` trait + `transforms` in `plugin!`
- [ ] view builder: `original_target()`, `.name()`, `.key()`
- [ ] `slot-example` with `#[overridable]`

### Phase 4 -- Layer 2 (Route Injection)

- [ ] `RoutePattern::matches` + `extract_params`
- [ ] `PluginAwareRouter<R>` with Wrap pipeline fold
- [ ] `TransformOp::InjectBefore`, `InjectAfter`, `Wrap` for `Selector::Route`
- [ ] `PluginViewRenderer`: `content_slot` + `__content__` resolution
- [ ] `route-injection-example`

### Phase 5 -- Layer 3 (Tree Selectors)

- [ ] `NodeSelector::DataAttr` + `NodeSelector::Recursive`
- [ ] `apply_tree_transforms`: shallow + opt-in recursive traversal
- [ ] `Selector::Slot`, `DataPluginSlot`, `Within` dispatch in `render_slot`
- [ ] `TransformOp::InsertBefore`, `InsertAfter`, `AddClass`, `SetAttr`
- [ ] `__target__` resolution in `PluginViewRenderer`
- [ ] view builder: `original_content()`, `.plugin_slot()`
- [ ] `Selector::Any` + `NodeSelector::And` / `Or`
- [ ] `tree-selector-example`

### Phase 6 -- State, Hot-reload, SSR, and Polish

- [ ] Global state with capability gating
- [ ] Cross-plugin state reads (`dx_plugin_state_get`)
- [ ] `use_plugin_state` frontend hook
- [ ] HTTP host function (capability-gated)
- [ ] `enable_plugin` / `disable_plugin` on `PluginRuntime`
- [ ] `reload_plugin` + `unload_plugin`: `on_unload` on old pool, atomic
  `Registries` rebuild, `on_load` on new pool, version increment, SSE broadcast
- [ ] Global state persistence: `JsonFilePersistence` + custom backends
- [ ] Session TTL eviction background task
- [ ] SSR mode: `ssr_render_route`, `PluginSlotSsr`, `SsrPluginDataProvider`,
  `ClientCapabilities::default_ssr()`, `ssr-example`
- [ ] `dioxus-extism` re-export crate with `host`, `frontend`, `pdk`, `test` features
- [ ] `tracing` instrumentation: plugin call durations, pool wait times,
  fuel consumption, Wrap chain cuts, invocation timeouts, compatibility skips
- [ ] Full documentation, guides, and all examples polished
