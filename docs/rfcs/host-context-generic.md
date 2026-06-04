# RFC: Generic Host Context (`HostCtx`)

**Status**: Proposal
**Date**: 2026-05-28
**Scope**: `dioxus-extism-protocol`, `dioxus-extism-host`, `dioxus-extism-frontend`
**Supersedes (partial)**: §3, §4 of `host-agnostic-extensions.md` (capability checks and route-replace policy callbacks)

---

## 1. Motivation

dioxus-extism is host-agnostic by design. Today, the host can extend the runtime via:

- Custom manifest extensions (§1 of the host-agnostic RFC),
- Generic `call_plugin` dispatch (§2),
- Custom capability classes (§3),
- Route-replace transforms with a policy callback (§4),
- Opaque trust tags (§5).

At several runtime decision points dioxus-extism asks the host to make a choice:

- Should this plugin be allowed to replace this route?
- Should this plugin's custom capability succeed for this call?
- How should this command-hook respond to this command?

Today these callbacks receive only **what dioxus-extism knows**: plugin id, route pattern, capability namespace+value, command payload. They receive **nothing the host knows** about the current call: who initiated it, in what tenant, with what privileges, at what time, from what IP, under which feature flag — all opaque to dioxus-extism.

This restricts host policies to static `(plugin_id, name) → bool` mappings. Anything dynamic — "this user, in this context, may not invoke this plugin" — is impossible without ugly workarounds (task-locals, ambient globals, doubled book-keeping).

`SessionCtx` already exists in dioxus-extism and carries runtime-level metadata (session id, optional user id, granted client capabilities, caller plugin id). It is intentionally minimal and dioxus-extism owns its shape. It is **not** the right place to add arbitrary host state.

This RFC introduces a **generic, opaque host context** that flows through every runtime decision point at which dioxus-extism asks the host for a verdict. It is fully owned by the host, fully opaque to dioxus-extism, and has zero meaning inside the library.

---

## 2. Non-Goals

- dioxus-extism **must not** gain any concept of users, roles, permissions, workspaces, tenants, authentication, sessions-beyond-`SessionCtx`, or any other host-specific identity model.
- dioxus-extism **must not** look inside the host context. The library never reads, serialises, logs, or compares `HostCtx` values.
- `HostCtx` is **not** a replacement for `SessionCtx`. The two coexist and carry orthogonal information (§5).
- Plugin lifecycle operations (`install`, `uninstall`, `enable`, `disable`) do **not** participate in this RFC. They are called by the host into the runtime; the host has already authorised them before calling. dioxus-extism makes no policy decision there.

---

## 3. Design

### 3.1 A single generic parameter

`PluginRuntime` becomes generic over an opaque host-context type:

```rust
pub struct PluginRuntime<HostCtx = ()> { /* ... */ }
```

The default `HostCtx = ()` preserves the current public API for hosts that have no per-call context.

Constraints on `HostCtx`:

```rust
HostCtx: Send + Sync + 'static
```

dioxus-extism imposes no further constraints. The host is free to use any type — a struct, an enum, an `Arc<T>`, a unit type, a tuple, anything. It is opaque.

### 3.2 Where the context flows

dioxus-extism propagates `&HostCtx` to **every callback the host registers that represents a runtime decision**:

| Decision point | Today | After this RFC |
|---|---|---|
| Route-replace policy | `Fn(&PluginId, &str) -> bool` | `Fn(&PluginId, &str, &CallContext<HostCtx>) -> bool` |
| Custom capability check | `Fn(&PluginId, &Value) -> Result<(), String>` | `Fn(&PluginId, &Value, &CallContext<HostCtx>) -> Result<(), String>` |
| Command-hook handler | `Fn(&Command) -> HookResult` | `Fn(&Command, &CallContext<HostCtx>) -> HookResult` |
| Slot resolver (optional) | iterator over registrations | iterator filtered by `Fn(&PluginId, &str, &CallContext<HostCtx>) -> bool` |
| Component-override resolver (optional) | static lookup | optional filter `Fn(&PluginId, &str, &CallContext<HostCtx>) -> bool` |

The four cases marked "optional" are deferred to a follow-up if real demand surfaces — see §10.

### 3.3 Where the context does **not** flow

| Decision point | Why no `HostCtx` |
|---|---|
| Manifest-extension `validate` | Schema-time check. Runs before any call exists. |
| Manifest-extension `on_load` | Plugin-load-time. Runs once per plugin install/reload, not per call. |
| Plugin lifecycle methods (`install`, `uninstall`, `enable`, `disable`) | Direct host→runtime calls. Host has already authorised; runtime makes no policy decision. |
| Trust verification (Ed25519 signature) | Cryptographic check on the bundle bytes. Context-free. |

### 3.4 The `CallContext` wrapper

Most callbacks need access to both:

1. **dioxus-extism's own per-call metadata** — already represented by `SessionCtx`.
2. **The host's per-call metadata** — the new `HostCtx`.

To avoid forcing every callback to take two parameters, the library exposes a thin wrapper:

```rust
/// Combined per-call context handed to host callbacks.
///
/// `session` is owned and populated by dioxus-extism.
/// `host` is owned and populated by the host application.
#[non_exhaustive]
pub struct CallContext<'a, HostCtx> {
    pub session: &'a SessionCtx,
    pub host: &'a HostCtx,
}
```

Callbacks receive `&CallContext<HostCtx>`. The host can read both fields as needed:

```rust
runtime.register_route_replace_policy(|plugin_id, route, ctx| {
    // dioxus-extism-owned data
    let session_id = &ctx.session.session_id;

    // Host-owned data — only the host knows its shape
    let allowed = host_policy_decide(plugin_id, route, ctx.host);

    allowed
});
```

`CallContext` is `#[non_exhaustive]` so dioxus-extism can later add further runtime-owned fields without breaking hosts.

### 3.5 Type signatures (concrete)

```rust
// in dioxus-extism-host/src/runtime.rs

pub type RouteReplacePolicyFn<HostCtx> =
    dyn Fn(&PluginId, &str, &CallContext<'_, HostCtx>) -> bool + Send + Sync;

pub type CapabilityCheckFn<HostCtx> =
    dyn Fn(&PluginId, &serde_json::Value, &CallContext<'_, HostCtx>)
        -> Result<(), String> + Send + Sync;

pub type CommandHookFn<HostCtx> =
    dyn Fn(&HookCall, &CallContext<'_, HostCtx>) -> HookResult + Send + Sync;
```

These remain `dyn`-objects boxed in `Arc` inside the runtime's RwLocks, exactly as today. The only change is the additional `&CallContext<'_, HostCtx>` parameter.

### 3.6 How the host supplies `HostCtx` at call time

dioxus-extism never constructs a `HostCtx`. The host must thread it in at every call site where the runtime might ask the host for a decision. Two integration patterns:

**Pattern A — Server-side calls (`call_plugin`, hook dispatch from application services)**

The host service layer holds the current request context already. It passes it explicitly to the runtime API:

```rust
runtime.call_plugin(
    plugin_id,
    "render_invoice",
    &payload,
    &session_ctx,
    &host_ctx,   // <-- new parameter
).await?;
```

The runtime stores `&host_ctx` for the duration of the call, threads it into any callback it invokes during that call, and drops it when the call returns. No globals, no ambient state.

**Pattern B — Frontend-side calls (route rendering, slot rendering, component overrides)**

Frontend components fetch the current `HostCtx` from a Dioxus context provider supplied by the host at app startup. dioxus-extism's frontend crate exposes a typed extractor:

```rust
// In the host's frontend root component:
use_context_provider(|| Arc::new(MyHostCtx { /* ... */ }));

// In PluginAwareRouter (provided by dioxus-extism-frontend):
let host_ctx = use_context::<Arc<MyHostCtx>>();
// ... passed to runtime resolver calls
```

The frontend components in dioxus-extism-frontend become generic in the same way the runtime is:

```rust
#[component]
pub fn PluginAwareRouter<HostCtx>(/* ... */) -> Element
where
    HostCtx: 'static,
{ /* ... */ }
```

The host's Dioxus root provides the `Arc<HostCtx>` once; downstream `PluginSlot<HostCtx>`, `OverridableComponent<HostCtx>`, etc. consume it via `use_context`.

### 3.7 Builder ergonomics

The `PluginRuntimeBuilder` is the entry point hosts use today. It also becomes generic, and the type parameter is fixed when the host calls `.build()`:

```rust
let runtime: PluginRuntime<MyHostCtx> = PluginRuntime::<MyHostCtx>::builder()
    .with_route_replace_policy(|plugin_id, route, ctx| {
        // ctx.host is &MyHostCtx
        // ctx.session is &SessionCtx
        true
    })
    .with_capability_check("my-org.feature", |plugin_id, value, ctx| {
        Ok(())
    })
    .build()
    .await?;
```

For hosts that have no per-call context, `()` is the default and the type can be omitted:

```rust
let runtime: PluginRuntime = PluginRuntime::builder().build().await?;
```

The `_: &CallContext<'_, ()>` parameter in callbacks is still present but trivially ignorable:

```rust
runtime.register_route_replace_policy(|plugin_id, route, _| {
    // policy uses only plugin_id and route
    plugin_id.0.starts_with("acme.")
});
```

---

## 4. What the host context typically holds

dioxus-extism never inspects `HostCtx`, so this section is **illustrative only** — not a contract.

A real host might put any combination of the following into `HostCtx`:

- User identity (id, email, display name).
- Tenant / workspace / organisation id.
- Effective permission set for the caller.
- Role membership.
- Authentication strength (password vs MFA vs API token).
- Request metadata (IP, user-agent, locale, request id).
- Time-of-day / day-of-week (for time-based policies).
- Feature-flag evaluations resolved for the caller.
- A handle to a host service (e.g. `Arc<DatabaseHandle>`) for callbacks that need to query state.
- Absolutely nothing (`()`).

Hosts that do not need per-call context simply use the default `()` and the entire mechanism collapses to the current behaviour.

---

## 5. Relationship to `SessionCtx`

`SessionCtx` and `HostCtx` are intentionally separate. The split is along ownership lines, not capability lines.

| | `SessionCtx` | `HostCtx` |
|---|---|---|
| Defined by | dioxus-extism (`dioxus-extism-protocol`) | The host (any type) |
| Shape | Fixed struct | Opaque generic |
| Populated by | dioxus-extism, from inputs the host supplies (session id, client caps, caller plugin id) | The host, freely |
| Serialised across WASM boundary | Yes — sent into plugins as part of every call | **No** — never crosses into WASM; lives only in host process |
| Versioning concern | Library-wide | Host-internal |

A consequence of the "never crosses into WASM" rule: plugins cannot read `HostCtx` directly. If a host wants to expose host-side context to a plugin, the host must do so explicitly through `client_capabilities`, the capability system, or by passing fields into the plugin's input payload. This keeps plugin code portable across hosts.

The existing `user_id: Option<String>` field on `SessionCtx` is in scope for re-evaluation in a follow-up: ideally `SessionCtx` would carry only host-agnostic identifiers, and any user concept would live in `HostCtx`. This is out of scope for this RFC and tracked in §10.

---

## 6. API changes by crate

### 6.1 `dioxus-extism-protocol`

Add:

```rust
#[non_exhaustive]
pub struct CallContext<'a, HostCtx> {
    pub session: &'a SessionCtx,
    pub host: &'a HostCtx,
}

impl<'a, HostCtx> CallContext<'a, HostCtx> {
    pub fn new(session: &'a SessionCtx, host: &'a HostCtx) -> Self {
        Self { session, host }
    }
}
```

No changes to `SessionCtx` in this RFC.

### 6.2 `dioxus-extism-host`

- `PluginRuntime` becomes `PluginRuntime<HostCtx = ()>`.
- `PluginRuntimeBuilder` becomes `PluginRuntimeBuilder<HostCtx = ()>`.
- `RouteReplacePolicyFn`, `CapabilityCheckFn`, and any new `CommandHookFn` become generic over `HostCtx`.
- `register_route_replace_policy`, `register_capability_check`, and any new `register_*_hook` accept callbacks that take `&CallContext<'_, HostCtx>`.
- `call_plugin` gains a `&HostCtx` parameter (alongside the existing `&SessionCtx`).
- `with_route_replace_policy`, `with_capability_check` builder methods follow suit.

Internal state stays monomorphic where possible. The generic only touches:
- The runtime's policy/check/hook stores (typed by `HostCtx`).
- The call path that invokes them (carries `&HostCtx` through).

Storage that has nothing to do with policy callbacks (the plugin registry, override map, trust store, manifest extensions, observability sinks) **stays monomorphic** — they do not see `HostCtx`.

### 6.3 `dioxus-extism-frontend`

- `PluginAwareRouter`, `PluginSlot`, `OverridableComponent` become generic in `HostCtx`.
- Each pulls the `HostCtx` from Dioxus context via `use_context::<Arc<HostCtx>>()`.
- The host installs the context once in its root component with `use_context_provider`.
- Server functions called by these components forward `Arc<HostCtx>` to the host's runtime layer.

The frontend integration is the largest source of generic propagation but is contained to dioxus-extism-frontend's own component types. Plugin authors using the PDK do **not** see `HostCtx` — plugins are unaware of it (it never reaches WASM).

### 6.4 `dioxus-extism-pdk`

**No changes.** Plugins do not see `HostCtx`. Existing plugin code compiles unchanged.

---

## 7. Migration

Existing call sites continue to compile because of the `= ()` default. Concretely:

```rust
// Before — still works
let runtime: PluginRuntime = PluginRuntime::builder().build().await?;
runtime.register_route_replace_policy(Arc::new(|plugin_id, route| {
    true
})).await;
```

The callback signature broadens to take a third `_: &CallContext<'_, ()>` parameter. To keep this backward-compatible, registration is changed to a helper that accepts both arities via an adapter trait:

```rust
trait IntoRouteReplacePolicy<HostCtx> {
    fn into_policy(self) -> Arc<RouteReplacePolicyFn<HostCtx>>;
}

impl<F, HostCtx> IntoRouteReplacePolicy<HostCtx> for F
where
    F: Fn(&PluginId, &str) -> bool + Send + Sync + 'static,
{
    fn into_policy(self) -> Arc<RouteReplacePolicyFn<HostCtx>> {
        Arc::new(move |id, route, _ctx| self(id, route))
    }
}

impl<F, HostCtx> IntoRouteReplacePolicy<HostCtx> for F
where
    F: Fn(&PluginId, &str, &CallContext<'_, HostCtx>) -> bool + Send + Sync + 'static,
{
    fn into_policy(self) -> Arc<RouteReplacePolicyFn<HostCtx>> {
        Arc::new(self)
    }
}
```

If the dual-impl conflict cannot be resolved cleanly with trait coherence, fall back to two distinct registration methods: `register_route_replace_policy` (new, takes `CallContext`) and `register_route_replace_policy_simple` (legacy, ignores context). Either is acceptable; the goal is "existing tests continue to pass without rewriting".

Same pattern applies to `register_capability_check`.

---

## 8. Frontend integration in detail

The frontend crate ships several components that today are monomorphic. After this RFC:

```rust
// dioxus-extism-frontend/src/components.rs

#[component]
pub fn PluginAwareRouter<HostCtx: 'static>(
    routes: Vec<RoutePattern>,
    children: Element,
) -> Element {
    let host_ctx = use_context::<Arc<HostCtx>>();
    // ... pass &host_ctx through to runtime calls
}
```

Hosts using these components must install the `HostCtx` in their root:

```rust
fn App() -> Element {
    use_context_provider(|| Arc::new(MyHostCtx::from_request()));
    rsx! {
        PluginAwareRouter::<MyHostCtx> {
            routes: ROUTES.to_vec(),
            children: rsx! { ... },
        }
    }
}
```

The `Arc` wrapping is intentional: Dioxus context expects `Clone`, and most host context types are cheaper to share than to clone deeply.

A convenience type alias makes this nicer in practice:

```rust
pub type HostCtxRef<HostCtx> = Arc<HostCtx>;
```

For hosts using `HostCtx = ()`, the context provider can be skipped entirely; the components fall back to a no-op default.

### Server-function bridging

Frontend server functions that today look like:

```rust
#[server]
async fn render_slot(slot: String, session_id: SessionId) -> Result<...> {
    let runtime = extract::<State<Arc<PluginRuntime>>>().await?;
    runtime.render_slot(&slot, &session_ctx).await
}
```

Become:

```rust
#[server]
async fn render_slot<HostCtx>(
    slot: String,
    session_id: SessionId,
    host_ctx: HostCtx,  // host-defined, serialisable
) -> Result<...>
where
    HostCtx: Serialize + DeserializeOwned + Send + 'static,
{
    let runtime = extract::<State<Arc<PluginRuntime<HostCtx>>>>().await?;
    runtime.render_slot(&slot, &session_ctx, &host_ctx).await
}
```

Hosts can choose to extract `HostCtx` server-side from the request directly (e.g. from cookies / JWTs) rather than transmit it from the client — that's a host decision and out of scope for the library. The library only requires that **some** `&HostCtx` is available at the server boundary.

---

## 9. Compatibility & versioning

- The default `HostCtx = ()` means hosts that don't opt in see no behavioural change.
- All public type signatures that previously did not mention `HostCtx` gain a generic parameter; default values keep existing call sites working.
- Plugin-side code (PDK, plugin manifests, plugin exports) is **unchanged**. Plugins do not see or depend on `HostCtx`.
- This is an additive, source-compatible change with `= ()` defaults. Hosts using non-default `HostCtx` will, however, need to update their `PluginRuntime` type annotations across their service code.

---

## 10. Open questions and follow-ups

1. **Slot- and component-override filtering**: Should the slot resolver and component-override resolver gain a `HostCtx`-aware filter? Today both are static lookups based on plugin manifest declarations. A real demand has not yet surfaced; deferring until it does keeps the API smaller.

2. **`user_id` in `SessionCtx`**: The current `SessionCtx::user_id: Option<String>` is mildly host-flavoured. With `HostCtx` available, this field could be dropped from `SessionCtx` and pushed into `HostCtx` by hosts that want it. This requires a separate RFC and migration window.

3. **Async callbacks**: All policy/check/hook callbacks are currently synchronous (returning `bool` or `Result`). Should they be allowed to be async? An async policy could query a database. dioxus-extism's call path is async so this is mechanically possible. Trade-off: async callbacks complicate signature, encourage slow checks, can introduce reentrancy. Recommendation: stay synchronous; hosts who need async data should pre-compute it into `HostCtx` before entering the runtime call.

4. **`CallContext` field additions**: `CallContext` is `#[non_exhaustive]`. Future additions could include a `now: Instant`, a `tracing_span: &Span`, or other dioxus-extism-owned signals. None are part of this RFC.

5. **Multiple `HostCtx` types per runtime**: Out of scope. A runtime has exactly one `HostCtx` type. Hosts that need per-feature contexts compose them into a single struct.

6. **Backward compatibility with single-arity callbacks**: §7 sketches the adapter-trait approach. If coherence prevents it, use two registration methods. Either path is acceptable.

---

## 11. Implementation plan

A reasonable implementation order (each step a separate commit / PR):

1. Add `CallContext<'_, HostCtx>` to `dioxus-extism-protocol`.
2. Introduce `HostCtx = ()` generic on `PluginRuntime` and `PluginRuntimeBuilder`. Internals compile against `()` initially.
3. Convert `RouteReplacePolicyFn` to take `&CallContext<'_, HostCtx>`. Wire through to call sites in `runtime.rs`.
4. Same for `CapabilityCheckFn`.
5. Add `&HostCtx` parameter to `call_plugin`. Thread through to all callback invocations during that call.
6. Update `dioxus-extism-frontend` components to consume `HostCtx` from Dioxus context.
7. Update existing tests to use `()` explicitly where the generic is now visible.
8. Add new tests exercising a non-trivial `HostCtx` (e.g. a struct with two fields, a policy that reads both, and an assertion that the policy result varies with context).
9. Documentation: update `host-agnostic-extensions.md` to cross-reference this RFC where §3 and §4 are amended.

Each step keeps the workspace compiling and all existing tests passing.

---

## 12. Summary

`HostCtx` is a single opaque generic on `PluginRuntime` that flows through every runtime decision point at which dioxus-extism asks the host for a verdict. It is owned, populated, and interpreted entirely by the host. dioxus-extism never inspects it, never serialises it, never sends it to plugins. The change is additive, source-compatible via `HostCtx = ()` defaults, and contained in scope — plugin-side code is unaffected, internal runtime state that is not policy-related stays monomorphic. The result is that hosts of any shape — applications with users and tenants, applications with no users at all, time-based policies, IP-based policies, feature-flag-based policies, or no policies — can express their decisions cleanly without dioxus-extism encoding any assumption about what a "context" is.
