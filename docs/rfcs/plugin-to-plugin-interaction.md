# RFC: Plugin-to-Plugin Interaction

**Status**: Proposal
**Date**: 2026-05-28
**Scope**: `dioxus-extism-protocol`, `dioxus-extism-host`, `dioxus-extism-pdk`, `dioxus-extism-frontend`
**Related**: `host-agnostic-extensions.md` (capability system extended here), `host-context-generic.md` (`HostCtx` used in policy callbacks)

---

## 1. Motivation

Today dioxus-extism enforces a strict host-as-mediator model: plugins communicate exclusively through host functions and host-routed events. They cannot invoke each other's exports, cannot read each other's state except via narrow `ReadPluginState` grants, and cannot declare dependencies on one another. This is safe but limiting:

- Plugin compositions like "Plugin A wants to use a calculation from Plugin B" are impossible without folding B's logic into A or into the host.
- Plugin bundles ("install this collection of plugins as a unit, they assume each other") have no first-class support.
- Plugin authors cannot publish reusable building blocks for other plugins to depend on.

This RFC introduces a tightly scoped plugin-to-plugin interaction mechanism: synchronous WASM-to-WASM function calls, with explicit visibility declarations, manifest-level dependency contracts, host-side veto only for optional grants, and bundle support. Plugin authors gain a composable ecosystem; the host retains full control over policy without being forced into a friction-heavy approval workflow.

The host environment that this RFC targets is **opaque to dioxus-extism**: no users, no admins, no UI assumed. dioxus-extism provides the structured mechanism; hosts decide policy. Default behaviour is "if both plugin sides agreed, dioxus-extism grants; otherwise it does not".

---

## 2. Non-Goals

- dioxus-extism **must not** know who installs plugins, whether there's a human in the loop, how grants are persisted, or how consent is gathered. All "user consent" UX lives in the host.
- dioxus-extism **must not** invent its own type system or interface description language. Function signatures remain bytes-based, consistent with Extism's design. SemVer is the practical contract.
- dioxus-extism **must not** support side-by-side multiple versions of the same plugin id at runtime. One version per plugin id.
- dioxus-extism **must not** allow cyclic dependencies. Cycles are rejected at install time.
- dioxus-extism **must not** expose any plugin to introspection of *other* plugins beyond what it explicitly depends on. Plugin B does not learn that Plugin A is rendering it (unless the host tells B via some other channel).

---

## 3. Core Concepts

### 3.1 Function visibility

Plugins declare each exported function's visibility for **plugin-to-plugin** purposes:

- **`private`** (default): callable by the host runtime only. Other plugins cannot reach this function regardless of any grant.
- **`public`**: callable by the host runtime **and** by any other plugin that holds a `CallPlugin` capability for it.

Visibility is host-orthogonal. The host can always call any export (host↔plugin semantics are unchanged). Visibility only gates plugin↔plugin calls.

There is no `restricted` visibility. Plugin B does not enumerate specific caller plugins. The decision of *which* plugin gets the capability is the host's via `GrantPolicyFn` (§5), and even there only for optional dependencies. Plugin B's contract with the world is uniform: "anyone allowed to call this function may call it".

### 3.2 Required vs optional dependencies

Plugin A declares dependencies on other plugins in its manifest. Each dependency item is marked **`required`** or **`optional`**:

- **`required`**: Plugin A cannot function without this. If the dependency is missing, the wrong version, or its target function is not public, **Plugin A's install fails**.
- **`optional`**: Plugin A prefers to have this but degrades gracefully if it's absent. Missing, wrong version, non-public target, or host-policy denial result in the capability being silently absent at runtime — Plugin A handles this through introspection (§6).

### 3.3 Cross-plugin operations

A `CallPlugin` capability grants Plugin A the right to invoke a specific public function on Plugin B. **Function calls are the only primitive.** Reading or writing Plugin B's state, triggering Plugin B's commands, or fetching computed data are all expressed as function calls. The existing `ReadPluginState` capability is deprecated (§9.4).

Event subscriptions across plugins continue to use the existing `dx_emit_event` + `event_subscriptions`-in-manifest pipeline. This RFC does not change event semantics — Plugin B emits, Plugin A subscribes via its manifest as today.

---

## 4. Dependency Resolution

### 4.1 Single global version

At runtime exactly one version of each plugin id is loaded. If Plugin X is installed at version `1.5.0` and a subsequent install of Plugin Y requires `pausenzeiten ^2.0.0`, the install of Y fails with `InstallError::DependencyVersionConflict`. dioxus-extism never loads `pausenzeiten` 1.5 and 2.0 side-by-side.

The host is free to react to this error however it likes (e.g. uninstall the old, retry; surface a chooser; abort). dioxus-extism makes no policy decisions here.

### 4.2 SemVer constraints

Dependency entries declare a SemVer range. The plugin's own version is a single, exact version. At resolution time:

- Plugin B's declared version is parsed as a single SemVer.
- Plugin A's dependency range is parsed as a SemVer constraint expression (e.g. `>=1.2, <2.0`, `^1.5`, `~1.5.0`).
- Match is performed using standard SemVer semantics. Mismatch produces a structured install error.

The implementation uses an existing SemVer crate (e.g. `semver`) rather than a hand-rolled parser.

### 4.3 No cycles

The dependency graph across installed plugins must be acyclic. When a new plugin is installed, dioxus-extism performs a topological check against the existing graph plus the new edges. If any cycle would result, the install fails with `InstallError::CyclicDependency` and a listing of the offending cycle.

This is enforced **statically at install time only**. Within a single call chain control flow can still bounce arbitrarily (A → B → A) — that is constrained by a runtime stack-depth limit (§7.3), not by the dependency graph.

### 4.4 Lifecycle on dependency changes

| Event | Effect on dependent plugins |
|---|---|
| Required dependency uninstalled | All plugins that required it are auto-disabled with `PluginStatus::Disabled { reason: "required dependency '<id>' uninstalled" }`. State and grants are preserved; if the dependency is re-installed at a compatible version, dependent plugins can be re-enabled. |
| Optional dependency uninstalled | Dependent plugins stay active. The corresponding `CallPlugin` capability becomes inert. Calls to the missing target produce `CallError::TargetUnavailable` at runtime. `on_grants_changed` is fired on dependent plugins. |
| Dependency hot-reloaded | dioxus-extism re-validates grants against the reloaded dependency's manifest. Items whose target function is no longer public, or whose version constraint no longer matches, lose their grant. If any required grant is invalidated, the dependent plugin is auto-disabled. For each affected dependent plugin, `on_grants_changed` is fired (if still enabled). |
| Required dependency installed (where previously absent and the dependent was disabled because of it) | Dependent is **not** auto-re-enabled. The host's install flow decides whether to re-enable, since re-enabling may trigger renewed policy checks. |

The host can override these defaults by intercepting install lifecycle events through its existing dioxus-extism integration; the defaults are designed to be safe and predictable.

---

## 5. Grant Model

### 5.1 Two-party agreement

A `CallPlugin` capability for Plugin A targeting `(plugin_id = B, function = f)` is **automatically granted** when all three of the following hold:

1. Plugin A declares `f` in its `requires_plugins[*].functions` for Plugin B.
2. Plugin B's manifest declares `f` as `public` in its exports.
3. Plugin B is loaded at a version compatible with Plugin A's declared range.

No host intervention is required. The two-party agreement (A wants → B exposes) is sufficient.

### 5.2 Host veto for optional items only

When the dependency item is marked **`optional`**, dioxus-extism consults a host-registered `GrantPolicyFn` (if any) and may revoke the grant based on its return. The signature mirrors the HostCtx-aware callbacks from `host-context-generic.md`:

```rust
pub type GrantPolicyFn<HostCtx> =
    dyn Fn(&GrantRequest, &CallContext<'_, HostCtx>) -> GrantDecision
        + Send + Sync;

pub struct GrantRequest {
    pub plugin_id: PluginId,                  // the requesting plugin
    pub items: Vec<GrantRequestItem>,         // only items marked `optional`
}

pub struct GrantRequestItem {
    pub kind: CapabilityKind,                 // CallPlugin
    pub target_plugin: PluginId,
    pub function: String,
    pub satisfiable: bool,                    // is target loaded & function public?
}

#[derive(Default)]
pub struct GrantDecision {
    pub denied: Vec<usize>,                   // indices into request.items
}
```

By default (no policy registered) **all optional grants whose target is satisfiable are granted**. The `GrantPolicyFn` is a pure veto hook: it returns the subset of items to *deny*.

`GrantPolicyFn` is **never** consulted for required items. If a required item is unsatisfiable, the install fails before policy is consulted. If a required item is satisfiable, the grant is automatic.

### 5.3 Grant lifecycle

Grants are computed at install time and re-computed whenever the dependency graph changes (uninstall/install/hot-reload of any plugin in the graph). The recomputation runs `GrantPolicyFn` again for optional items; if the host's policy is stateless this is idempotent.

Hosts can persist grant decisions externally by storing them in their own data layer and replaying them through the policy callback on subsequent recomputations. dioxus-extism keeps no grant history beyond the currently-effective set.

### 5.4 Failure modes summary

| Scenario | Outcome |
|---|---|
| Required, target plugin missing | Install of requesting plugin fails (`DependencyMissing`) |
| Required, target plugin wrong version | Install fails (`DependencyVersionConflict`) |
| Required, target function private | Install fails (`FunctionNotPublic`) |
| Required, all conditions met | Grant auto-issued |
| Optional, target plugin missing | Item silently denied, plugin loads, `denied: [TargetUnavailable]` reported to plugin |
| Optional, target function private | Same as above, `denied: [FunctionNotPublic]` |
| Optional, host policy vetoes | Same as above, `denied: [HostPolicyVeto]` |
| Optional, all conditions met & host doesn't veto | Grant auto-issued |

---

## 6. Plugin-Side API

Plugin A needs to know which of its declared dependencies actually have grants — both required (so it can rely on them) and optional (so it can branch on availability).

### 6.1 `on_load` init context

The `on_load` lifecycle export receives a context struct including grant information:

```rust
pub struct PluginInitContext {
    pub session: SessionCtx,
    pub grants: GrantStatus,
    // existing fields...
}

pub struct GrantStatus {
    pub call_plugin: Vec<CallPluginGrant>,
}

pub struct CallPluginGrant {
    pub target_plugin: PluginId,
    pub function: String,
    pub required: bool,         // mirrors the manifest declaration
    pub granted: bool,          // true if usable, false if denied (optional only — required ones never reach load)
    pub denial_reason: Option<DenialReason>,
}

#[non_exhaustive]
pub enum DenialReason {
    TargetUnavailable,
    TargetVersionMismatch,
    FunctionNotPublic,
    HostPolicyVeto,
}
```

Required items appear with `granted: true` (if the plugin reached `on_load`, all required grants must be in place). Optional items appear with their actual status. Plugin authors typically branch on `granted` during init to register feature-flag-style handlers.

### 6.2 Runtime introspection

For code paths that are reached rarely, or whose grant state may change post-load, plugins query the runtime:

```rust
// in dioxus-extism-pdk
pub fn is_granted(
    kind: CapabilityKind,
    target_plugin: &PluginId,
    function: &str,
) -> bool;
```

Backed by a host function `dx_is_granted` that takes a serialised query and returns a boolean.

### 6.3 Grant change events

When the dependency graph changes and grants are recomputed, dioxus-extism invokes a lifecycle export on each affected plugin:

```rust
// Plugin-side
#[plugin_fn]
pub fn on_grants_changed(input: Json<GrantStatus>) -> FnResult<()> {
    // Plugin reacts to new grant state (e.g. re-register handlers).
    Ok(())
}
```

Defining this export is optional. Plugins that don't define it simply continue running; they may encounter `CallError::TargetUnavailable` on stale calls and should handle that gracefully.

### 6.4 Performing a cross-plugin call

A new PDK helper and a corresponding host function:

```rust
// In dioxus-extism-pdk
pub fn call_plugin<I, O>(
    target: &PluginId,
    function: &str,
    input: &I,
) -> Result<O, CallError>
where
    I: Serialize,
    O: DeserializeOwned;
```

Backed by host function `dx_call_plugin(target, function, input_bytes) -> output_bytes`. The host function:

1. Looks up the calling plugin's `CallPlugin` capability for `(target, function)`.
2. If no grant, returns `CallError::PermissionDenied`.
3. If target plugin is not loaded, returns `CallError::TargetUnavailable`.
4. Increments the call-stack depth counter (§7.3); if over limit, returns `CallError::StackOverflow`.
5. Records an audit event (§8).
6. Acquires a pool instance for the target plugin and invokes the named export with the input bytes.
7. Returns output bytes; serialisation failures surface as `CallError::DeserializationError` with plugin + function context.

The call is synchronous from the perspective of the calling plugin — the WASM call blocks until the target returns or errors.

---

## 7. Runtime Semantics

### 7.1 Call dispatch path

A plugin-to-plugin call is a regular Extism call against the target plugin's pool, mediated by a host function in the calling plugin's environment. There is no direct WASM-to-WASM linking (WASM components / multi-memory features are not used here). dioxus-extism's pool architecture handles the dispatch; each plugin's pool is independent.

Performance-wise this means one round trip from caller → host bridge → target pool acquire → target execution → return. The host bridge step is in-process (no IPC), so the overhead is on the order of a function call plus serialisation, not a full RPC.

### 7.2 Re-entrance

Plugin A may call Plugin B which calls Plugin C which calls back into Plugin A. The runtime supports re-entrance: each call acquires its own pool instance from the target's pool, so concurrent calls into the same plugin from different chains share the pool but not the instance. Plugins should treat their own state as potentially observed mid-handler by another call from the same chain — same considerations as today for the host calling a plugin while a previous call is still in flight.

### 7.3 Stack-depth limit

Even with an acyclic dependency *declaration* graph, runtime call chains can exceed reasonable depth (A → B → A's helper export → B → A → ...). dioxus-extism enforces a configurable runtime stack-depth limit (default: `32`) on cross-plugin call chains. Each `dx_call_plugin` increments a per-chain counter; exceeding the limit returns `CallError::StackOverflow` to the caller.

```rust
PluginRuntime::builder()
    .with_cross_plugin_max_depth(32)
    .build();
```

### 7.4 Timeouts

Cross-plugin calls inherit the same timeout configuration as host→plugin calls. There is no separate timeout for cross-plugin chains in this RFC; per-chain budget management is a follow-up if real demand surfaces.

---

## 8. Audit Hook

Every cross-plugin call (allowed or denied) produces an audit event. The host registers a sink:

```rust
pub trait CrossPluginAuditSink: Send + Sync {
    fn record(&self, event: CrossPluginCallEvent);
}

#[non_exhaustive]
pub struct CrossPluginCallEvent {
    pub caller: PluginId,
    pub target: PluginId,
    pub function: String,
    pub outcome: CallOutcome,
    pub timestamp: SystemTime,
}

#[non_exhaustive]
pub enum CallOutcome {
    Allowed { duration: Duration },
    Denied { reason: DenialReason },
    Failed { error_kind: CallErrorKind },
}

runtime.builder().with_audit_sink(Arc::new(MySink));
```

dioxus-extism feeds events to the sink synchronously but **never** awaits the sink. Hosts that want async persistence implement an in-memory queue inside their sink. Hosts that don't care simply don't register a sink — events are dropped silently (no buffering).

Plugins cannot read audit events.

---

## 9. Manifest Schema

### 9.1 Exporting public functions

In `dioxus-extism-protocol::PluginManifest`:

```rust
pub struct PluginManifest {
    // existing fields...
    pub exports: ExportsManifest,
}

#[derive(Default)]
pub struct ExportsManifest {
    pub public: BTreeMap<String, PublicFunctionDecl>,
}

#[non_exhaustive]
pub struct PublicFunctionDecl {
    pub description: Option<String>,
    // future: schema fields (see §11)
}
```

In TOML:

```toml
[exports.public]
get_break_quota = { description = "Returns the daily break quota for a user" }
list_breaks_for_user = {}
```

Functions not listed under `[exports.public]` are private (host-only) by default.

### 9.2 Requiring other plugins

```rust
pub struct PluginManifest {
    // existing fields...
    pub requires_plugins: Vec<PluginDependency>,
}

#[non_exhaustive]
pub struct PluginDependency {
    pub id: PluginId,
    pub version: VersionRange,    // SemVer constraint
    pub required: bool,
    pub functions: Vec<String>,   // functions Plugin A intends to call on this dependency
}
```

In TOML:

```toml
[[requires_plugins]]
id = "com.acme.pausenzeiten"
version = ">=1.2, <2.0"
required = true
functions = ["get_break_quota", "list_breaks_for_user"]

[[requires_plugins]]
id = "com.acme.feiertage"
version = "^1.0"
required = false
functions = ["is_holiday"]
```

### 9.3 Capability variant

Existing `HostCapability` (see `host-agnostic-extensions.md` §3) gains a variant:

```rust
pub enum HostCapability {
    // existing variants...
    CallPlugin {
        target_plugin_id: PluginId,
        allowed_functions: Vec<String>,
    },
}
```

This variant is **not** declared directly by plugin authors in the manifest — it is *derived* by dioxus-extism from `requires_plugins` declarations during install. The capability appears in `LoadedPlugin::granted_capabilities` for inspection and is enforced on every `dx_call_plugin` invocation.

### 9.4 Deprecation of `ReadPluginState`

The existing `HostCapability::ReadPluginState { plugin_id, keys }` is marked `#[deprecated]` in this RFC. Plugins that want to expose state to other plugins should expose accessor functions instead. The deprecation does not remove the existing variant — existing plugins continue to work — but new plugins should use the function-call mechanism.

A future RFC may complete the migration and remove `ReadPluginState`.

---

## 10. Bundles

### 10.1 Bundle artifact

A bundle is a directory or zip-archive containing:

- A `bundle.toml` manifest at the root.
- One subdirectory per contained plugin, each with its own plugin manifest and `.wasm` file (same layout as a standalone plugin).

Example:

```
unternehmens-suite/
├── bundle.toml
├── arbeitsschutz/
│   ├── plugin.toml
│   └── arbeitsschutz.wasm
└── pausenzeiten/
    ├── plugin.toml
    └── pausenzeiten.wasm
```

### 10.2 Bundle manifest

```toml
[bundle]
id = "com.acme.unternehmens-suite"
version = "1.0.0"

[[bundle.plugins]]
id = "com.acme.arbeitsschutz"
path = "arbeitsschutz"

[[bundle.plugins]]
id = "com.acme.pausenzeiten"
path = "pausenzeiten"

[bundle.trust_group]
mutual_call_plugin = true
```

### 10.3 Install semantics

`runtime.install_bundle(source)` performs:

1. Parses `bundle.toml`.
2. Loads each contained plugin's manifest without yet installing.
3. Builds the combined dependency graph including the bundle's plugins plus all currently-loaded plugins. Verifies acyclicity and version compatibility.
4. For each pair of plugins inside the bundle where `trust_group.mutual_call_plugin = true`: dioxus-extism synthesises implicit `requires_plugins` entries (`required: true`, `functions: <all public functions of sibling>`) so siblings have automatic mutual grants for every public function.
5. Installs each plugin atomically. If any install fails, the entire bundle install is rolled back.

The `trust_group` only affects grants among bundle siblings. External plugins requiring a function in a bundled plugin go through the normal grant path; the bundle membership does not grant anything to outsiders.

### 10.4 Bundle uninstall

`runtime.uninstall_bundle(bundle_id)` removes all contained plugins atomically. If any contained plugin has external dependents that would be left disabled, the host's existing uninstall path applies (auto-disable cascade per §4.4).

---

## 11. Function-Signature Contracts

Following Extism's convention, dioxus-extism enforces no typed signature contract on cross-plugin calls. The practical contract is:

- **Static checks at install**: function name exists and is public, dependency version is in range, dependency graph is acyclic.
- **Dynamic checks at call time**: payload deserialisation must succeed in both directions. Mismatch surfaces as `CallError::DeserializationError { plugin, function, source }`.
- **Authorial discipline**: plugin authors who change a function's input or output shape are expected to bump their plugin's major version (per SemVer). Dependent plugins whose version range pins to the old major will continue to use the old plugin; dependents that move to the new major must update their call sites.

A follow-up RFC may introduce optional JSON-Schema declarations under `PublicFunctionDecl`, enabling early validation at install time. Plugins without schemas continue to work under the lose model.

---

## 12. Compatibility & Versioning

- The visibility declaration is **additive**: existing plugins without `[exports.public]` continue to expose all of their exports to the host only (default private). No existing call site breaks.
- `requires_plugins` is new; absence means "depends on no other plugin", which matches current behaviour for all existing plugins.
- The deprecated `ReadPluginState` capability is retained for backward compatibility. Existing plugins using it continue to work.
- The new `CallPlugin` capability is derived (never declared directly); existing capability-check logic in `host-agnostic-extensions.md` §3 already handles unknown variants.
- New PDK helpers (`call_plugin`, `is_granted`, `on_grants_changed`) are additions; the existing PDK API is unchanged.

---

## 13. API Changes by Crate

### 13.1 `dioxus-extism-protocol`

- Add `ExportsManifest`, `PublicFunctionDecl`, `PluginDependency`, `VersionRange`.
- Extend `PluginManifest` with `exports` and `requires_plugins` fields.
- Add `HostCapability::CallPlugin { target_plugin_id, allowed_functions }`.
- Add `GrantRequest`, `GrantRequestItem`, `GrantDecision`, `CapabilityKind`, `DenialReason`.
- Add `GrantStatus`, `CallPluginGrant` to `PluginInitContext` (extend existing init protocol).
- Add `CallError` variants (`PermissionDenied`, `TargetUnavailable`, `TargetVersionMismatch`, `FunctionNotPublic`, `StackOverflow`, `DeserializationError`, `HostPolicyVeto`).
- Add `CrossPluginCallEvent`, `CallOutcome`, `CallErrorKind` for audit.

### 13.2 `dioxus-extism-host`

- Implement install-time dependency resolution: version matching, acyclicity check, satisfiability check.
- Implement `runtime.install_bundle(source)` and `runtime.uninstall_bundle(bundle_id)`.
- Add `GrantPolicyFn<HostCtx>` and `register_grant_policy` / builder method.
- Add `CrossPluginAuditSink` trait and `with_audit_sink` builder method.
- Add new host function `dx_call_plugin` registered with each loaded plugin's pool. Implementation: capability check, target lookup, stack-depth check, audit-event emission, target pool acquire, target export call, response relay.
- Add new host function `dx_is_granted`.
- Wire `on_grants_changed` lifecycle export invocation on dependency-graph changes.
- Extend `PluginInitContext` payload with `GrantStatus` and pass it through to `on_load`.
- Add cross-plugin stack-depth tracking (per-chain counter, threaded through nested `dx_call_plugin` invocations).

### 13.3 `dioxus-extism-pdk`

- Add `call_plugin::<I, O>(target, function, input) -> Result<O, CallError>` helper.
- Add `is_granted(kind, target_plugin, function) -> bool` helper.
- Document `on_grants_changed` as an optional lifecycle export.
- Provide derive support or macro helpers for the `[exports.public]` manifest section so plugin authors can annotate `#[public_fn]` and have the manifest generated, mirroring the existing `#[plugin_fn]` ergonomics.

### 13.4 `dioxus-extism-frontend`

No direct frontend changes for the call mechanism itself (calls happen plugin-side, not frontend-side). However, frontend components that render plugin views may surface grant-related errors if a plugin's UI internally invokes a cross-plugin call that fails. Error UI is unchanged from existing patterns.

---

## 14. Migration

Existing plugins continue to function without modification. Adopting the new mechanism is opt-in per plugin author:

1. To **export** functions for other plugins to call: add `[exports.public]` to the manifest.
2. To **call** another plugin's functions: add `[[requires_plugins]]` to the manifest and use the PDK `call_plugin` helper.
3. To **bundle** multiple plugins: produce a bundle artifact with a `bundle.toml`.

Existing hosts continue to function without modification. Adopting the new policy hook (`GrantPolicyFn`) is opt-in. By default, dioxus-extism grants all satisfiable optional items.

---

## 15. Open Questions & Follow-ups

1. **JSON-Schema for public functions**: Optional install-time signature validation. Tracked separately; not in this RFC.
2. **Removal of `ReadPluginState`**: Deprecated here; removal in a future RFC after a migration window.
3. **Per-chain timeout budgets**: Not addressed here. Current default: each individual call inherits the runtime-wide call timeout.
4. **WIT / WebAssembly Component Model**: Out of scope. dioxus-extism may revisit signature contracts once WIT becomes practical in the Extism runtime.
5. **Granular hot-reload migration**: When a plugin is hot-reloaded with breaking changes to its public exports, dependent plugins are coarsely disabled. A future refinement could let dependents opt into selective re-validation.
6. **Bundle signature inheritance**: When a bundle is signed, should the trust tag (`host-agnostic-extensions.md` §5) be inherited by all contained plugins? Likely yes, but the exact semantics are deferred.

---

## 16. Implementation Plan

Each step is a discrete commit / PR, keeping the workspace green and all existing tests passing.

1. Protocol additions: `ExportsManifest`, `PluginDependency`, `VersionRange`, `CallError`, `DenialReason`. No behaviour change yet.
2. Manifest parsing for `[exports.public]` and `[[requires_plugins]]`. Surfaced in `LoadedPlugin` but not yet enforced.
3. Dependency-graph data structure in `dioxus-extism-host`. Acyclicity check, version match.
4. `HostCapability::CallPlugin` variant + grant derivation at install time. Required-vs-optional handling.
5. `GrantPolicyFn` + default-allow behaviour. Tests with no policy registered and with a policy that vetoes selectively.
6. `dx_call_plugin` host function. Pool dispatch, capability check, stack-depth tracking.
7. `dx_is_granted` host function and PDK helper.
8. `GrantStatus` in `PluginInitContext`. Plugins receive grant state at `on_load`.
9. `on_grants_changed` lifecycle export invocation. Hot-reload triggers re-evaluation and event dispatch.
10. `CrossPluginAuditSink` trait + audit event emission.
11. Bundle artifact format: `bundle.toml` parsing, `install_bundle`, `uninstall_bundle`. Trust-group synthesis.
12. PDK ergonomics: `#[public_fn]` macro or equivalent for manifest generation.
13. Deprecate `ReadPluginState` (warning attribute, doc note).
14. Examples and fixtures: a small `pausenzeiten`-style plugin pair, a bundled version, tests covering install-success, install-fail-on-conflict, optional-degradation, and hot-reload-grant-recompute scenarios.

---

## 17. Summary

Plugin-to-plugin interaction in dioxus-extism is a synchronous, capability-gated, manifest-declared mechanism for calling other plugins' explicitly-public functions. Required and optional dependencies have different semantics: required failures abort install; optional absences are signalled to the plugin which degrades gracefully. The grant model is "two-party agreement by default, host vetoes only for optionals" — minimising friction while keeping policy in the host's hands. Bundles are first-class artifacts with sibling trust groups. Cycles are statically forbidden, runtime depth is bounded, audit hooks expose every call. dioxus-extism contributes the mechanism; the host owns all policy; plugins compose without knowing each other's identities beyond what they explicitly depend on.
