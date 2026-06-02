# RFC: Host-Agnostic Plugin Extensions

**Status**: Proposal
**Date**: 2026-05-27
**Scope**: `dioxus-extism-protocol`, `dioxus-extism-host`, `dioxus-extism-frontend`

---

## Motivation

dioxus-extism is a general-purpose plugin runtime. Host applications differ in their domain models, security policies, persistence shapes, and routing semantics. To stay general, dioxus-extism must give hosts **hooks** to extend its manifest schema, dispatch arbitrary WASM exports, define their own capability classes, and gate sensitive operations — all without dioxus-extism knowing the host's domain.

This RFC introduces five mechanisms that, together, let any host build a rich plugin vocabulary on top of dioxus-extism without modifying dioxus-extism itself.

The work is internally referred to as "Phase 7" in a host-side initiative driving these requirements; from dioxus-extism's perspective these are standalone library improvements.

---

## Non-Goals

- dioxus-extism gains **no** domain-specific vocabulary (no event sourcing, aggregates, permissions strings, trust tiers, route paths). These belong in the host.
- No changes to the existing Slot / Component-Override / Page-Route / Wrap-Inject mechanisms beyond the route-replace addition in §4.

---

## 1. Generic Manifest Extensions

### Problem
`PluginManifest` today has a fixed set of fields. Hosts that want to let plugins declare host-specific concepts (e.g., a CMS host wanting content-type declarations, an IDE host wanting language-server bindings) must fork the manifest schema.

### Proposal
Add an `extensions` field to `PluginManifest`:

```rust
pub struct PluginManifest {
    // existing fields...
    pub extensions: BTreeMap<String, serde_json::Value>,
}
```

Hosts register handlers per namespace at runtime setup:

```rust
runtime.register_manifest_extension(
    "my-host.feature-x",
    Box::new(MyHostFeatureXHandler { /* ... */ }),
);
```

The handler trait:

```rust
pub trait ManifestExtensionHandler: Send + Sync {
    fn validate(&self, plugin_id: &PluginId, value: &serde_json::Value) -> Result<(), ManifestExtensionError>;
    fn on_load(&self, plugin_id: &PluginId, value: &serde_json::Value, runtime: &PluginRuntime) -> Result<(), ManifestExtensionError>;
    fn on_unload(&self, plugin_id: &PluginId) -> Result<(), ManifestExtensionError>;
}
```

During plugin load, dioxus-extism:
1. Parses the manifest as usual.
2. For each `(namespace, value)` in `extensions`, looks up the registered handler.
3. Calls `validate` then `on_load`. Either may abort the load.
4. Unknown namespaces produce a warning (configurable: `OnUnknownExtension::Warn | Error | Ignore`).

### Why generic JSON values
Forces the host to own its schema. A host that wants strong typing can ship its own helper crate with `serde`-derived types that wrap the JSON value — that helper crate is **outside** dioxus-extism.

---

## 2. Generic Plugin-Function Dispatch

> **Amended by [host-context-generic RFC](host-context-generic.md)** — `call_plugin`
> gains a `host_ctx: &HostCtx` parameter so the host can thread its opaque per-call
> context through the dispatch path.

### Problem
Existing call paths (slot rendering, page rendering, transforms, hooks, events) are baked-in. Hosts can't reach into a plugin to invoke arbitrary exports like `compute_thing`, `validate_payload`, or domain-specific computations.

### Proposal
Add a public method on `PluginRuntime`:

```rust
impl<HostCtx> PluginRuntime<HostCtx> {
    pub async fn call_plugin<I, O>(
        &self,
        plugin_id: &PluginId,
        function_name: &str,
        input: &I,
        session: &SessionCtx,
        host_ctx: &HostCtx,
    ) -> Result<O, PluginRuntimeError>
    where
        I: Serialize,
        O: DeserializeOwned + Send + 'static;
}
```

`session` carries the same capability/session context already used for hooks.
`host_ctx` is the host's opaque per-call context (see host-context-generic RFC).

`call_plugin` is a thin, **generic** wrapper around the existing Extism pool dispatch — it does not interpret the function name or input semantically. The host is free to use it for any export the plugin declares.

This is the building block that lets a host wire its manifest-extension handlers (§1) to actual WASM behavior.

---

## 3. Host-Defined Capability Classes

> **Amended by [host-context-generic RFC](host-context-generic.md)** — the check callback
> now receives a `CallContext<'_, HostCtx>` that carries the host's opaque per-call
> context alongside the session.  See §3.4–§3.5 of that RFC for the updated
> `CapabilityCheckFn<HostCtx>` type alias and both registration paths.

### Problem
`HostCapability` is currently a fixed enum. Hosts can't introduce new capability classes (e.g., a CMS host wants `CmsScope("publish")`; an IDE host wants `LanguageServer("rust-analyzer")`).

### Proposal
Add a variant:

```rust
pub enum HostCapability {
    // existing variants...
    Custom {
        namespace: String,
        value: serde_json::Value,
    },
}
```

The host registers a check callback per namespace. Two forms are available:

```rust
// Legacy two-argument form — no host context, suitable for load-time checks.
runtime.register_capability_check(
    "my-host.scope",
    |plugin_id: &PluginId, value: &serde_json::Value| -> Result<(), String> {
        // Runs at plugin load time and optionally at call time.
        Ok(())
    },
);

// Context-aware form — receives the full CallContext at call time.
// Use this when the decision depends on per-call host state.
runtime.register_capability_check_ctx(
    "my-host.scope",
    |plugin_id: &PluginId, value: &serde_json::Value, ctx: &CallContext<'_, HostCtx>| {
        // ctx.host is the host's opaque context; ctx.session is the current SessionCtx.
        // Host decides whether this plugin, in this call context, may use this capability.
        Ok(())
    },
);
```

Registering via `register_capability_check` (two-argument form) populates both the
load-time and the call-time check maps, so the closure runs during `build()` / `install()`
as well as `check_custom_capability()`.  Registering via `register_capability_check_ctx`
populates only the call-time map; the plugin is admitted at load time and the check fires
only when the host explicitly calls `check_custom_capability`.

When dioxus-extism encounters a `Custom` capability during a call, it dispatches to the
registered check. If no check is registered for the namespace, the default is **deny**.

Existing fixed-enum variants continue to be checked by dioxus-extism's built-in logic.

---

## 4. Route-Level `TransformOp::Replace`

> **Amended by [host-context-generic RFC](host-context-generic.md)** — the policy
> callback now receives a `CallContext<'_, HostCtx>` that carries the host's opaque
> per-call context alongside the session.  See §3.4–§3.5 of that RFC for the updated
> `RouteReplacePolicyFn<HostCtx>` type alias and both registration paths.

### Problem
`TransformOp` for route transforms today supports `Wrap`, `InjectBefore`, `InjectAfter`. There is no way for a plugin to **fully replace** a host route's content — only to wrap or augment it.

### Proposal
Extend `TransformOp`:

```rust
pub enum TransformOp {
    InjectBefore { view: PluginView },
    InjectAfter { view: PluginView },
    Wrap { view: PluginView },
    Replace { view: PluginView },   // <-- new
}
```

`Replace` is honored by `PluginAwareRouter`:
1. If any plugin declared `Replace` for the matched route pattern, the host route's content is **not** rendered.
2. The highest-priority plugin's view is rendered. Ties broken by plugin id (lexicographic).
3. `Wrap`/`Inject` from other plugins continue to apply around the replacement.

### Gating
**dioxus-extism takes no opinion** on whether a given plugin is allowed to replace a given route. Hosts may register an optional policy callback. Two forms are available:

```rust
// Legacy two-argument form — no host context.
runtime.register_route_replace_policy(
    |plugin_id: &PluginId, route: &str| -> bool {
        // Host-side policy. Return false to refuse the replacement.
        true
    },
);

// Context-aware form — receives the full CallContext at call time.
// Use this when the decision depends on per-call host state (e.g. user tier, tenant).
runtime.register_route_replace_policy_ctx(
    |plugin_id: &PluginId, route: &str, ctx: &CallContext<'_, HostCtx>| -> bool {
        // ctx.host is the host's opaque context; ctx.session is the current SessionCtx.
        ctx.host.user_tier >= 2
    },
);
```

Default policy if none registered: **allow**.

Hosts that require trusted plugins for replacement implement that in this callback, using the `TrustTag` from §5 and their own capability checks from §3.

---

## 5. Opaque Trust Tag

### Problem
Hosts want to verify plugin signatures and tie permissions to verification status, but dioxus-extism shouldn't define what "trust" means semantically (a particular signer doesn't map 1:1 to permissions).

### Proposal
Add Ed25519 signature verification to `dioxus-extism-host` with a configurable trust root. The verification result is an **opaque** `TrustTag`:

```rust
pub struct TrustTag {
    pub verified: bool,
    pub signer_key_id: Option<String>,
}
```

`TrustTag` is stored on `LoadedPlugin` and exposed read-only to hosts via `loaded_plugin.trust_tag()`. dioxus-extism **does not** convert tags into permissions, trust tiers, or any further policy meaning — that's the host's job (typically via the capability check callbacks from §3 and the route-replace policy from §4).

### What dioxus-extism does
- Verifies signatures against a configured Ed25519 public key (or set of keys).
- Records `verified: bool` and the key id used (if multiple keys are configured).
- Refuses to load unsigned plugins **only** if the host configures `require_signature: true` at runtime construction.

### What dioxus-extism does not do
- Define trust "levels" or "tiers".
- Tie verification status to any specific capability.
- Validate signer identity beyond key match.

---

## 6. Plugin Registry API (Library-Level)

To support host admin UIs, expose a small registry API on `PluginRuntime`:

```rust
impl PluginRuntime {
    pub fn list_plugins(&self) -> Vec<PluginSummary>;
    pub async fn install(&self, source: PluginSource) -> Result<PluginId, InstallError>;
    pub async fn uninstall(&self, plugin_id: &PluginId) -> Result<(), UninstallError>;
    pub async fn enable(&self, plugin_id: &PluginId) -> Result<(), StateError>;
    pub async fn disable(&self, plugin_id: &PluginId) -> Result<(), StateError>;
}
```

`PluginSource` covers file path, URL, or in-memory bytes (whatever the existing loader supports). State changes integrate with hot-reload.

---

## 7. Observability

- Plugin-call latency metric per `(plugin_id, function_name)`.
- Pool utilization metric per plugin.
- Exposed via the existing metrics-collection point (or a new `RuntimeMetrics` trait the host implements).

---

## Out of Scope (explicitly not in this RFC)

- Host-specific aggregate hosting, event sourcing, projection wiring — these belong in host crates that use §1 and §2.
- Permission-string semantics — hosts use §3 `Custom` capabilities for that.
- Concrete trust policies (signed vs unsigned, signer-to-permission mappings) — hosts use §3 + §5.
- Storage tables or persistence shapes for plugins — hosts use §2 to dispatch storage calls to their own services.

---

## Compatibility

- All additions are additive to the public API. Existing plugins and existing hosts continue to function unchanged.
- The new `extensions` field on `PluginManifest` defaults to empty.
- `TransformOp::Replace` is a new variant on an enum already used by hosts — hosts that match on `TransformOp` will get a non-exhaustive-match warning until they handle `Replace`.

---

## Open Questions

- Should `register_manifest_extension` accept a typed handler via a `serde::Deserialize` bound, or stay JSON-based? Current proposal: JSON-based for maximum flexibility; hosts that want types use their own helper crate.
- Should `call_plugin` enforce per-function timeouts, or inherit the runtime-wide default? Current proposal: inherit, with optional per-call override in `CallCtx`.
- Native `rsx!` authoring inside plugins is desirable but orthogonal — proposed as a separate RFC (`dioxus-extism-rsx`).
