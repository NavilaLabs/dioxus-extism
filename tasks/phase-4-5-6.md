# Phase 4 — Layer 2: Route Injection via PluginAwareRouter

**Prerequisite:** Phase 3 stop condition confirmed.

**Goal:** `PluginAwareRouter<R>` lets plugins inject UI before, after, or wrapping any
route's output. Multiple Wrap transforms compose via the sequential pipeline.

**Stop condition:** `cargo check --workspace && cargo test --workspace --lib` passes.
The `route-injection-example` starts and renders correctly.

---

## Step 1 — render_route_transforms

### Write tests using TestRuntime (from Phase 2):

```rust
#[test]
fn inject_after_appends_view() {
    // Requires a plugin compiled to WASM that declares InjectAfter for "/test"
    // Build such a plugin as part of this example or as a test fixture
}

#[test]
fn wrap_pipeline_folds_sequential() {
    // Two Wrap plugins: verify higher-priority one receives seed,
    // lower-priority receives higher-priority's output as original
}
```

### Implement — follow architecture section 4.9 exactly:

```rust
pub async fn render_route_transforms(
    &self,
    path: &str,
    session: &SessionCtx,
) -> Result<RouteTransforms, PluginRuntimeError> {
    let all_entries = {
        let regs = self.registries.read().await;
        regs.transforms.for_route(path)
    };

    // Partition by op
    let mut inject_before_entries = vec![];
    let mut wrap_entries = vec![];
    let mut inject_after_entries = vec![];
    for entry in all_entries {
        match entry.op {
            TransformOp::InjectBefore => inject_before_entries.push(entry),
            TransformOp::Wrap         => wrap_entries.push(entry),
            TransformOp::InjectAfter  => inject_after_entries.push(entry),
            _ => {}
        }
    }

    // InjectBefore — error-isolated
    let before = self.run_inject_transforms(&inject_before_entries, path, session).await;

    // Wrap — sequential pipeline fold
    let wrap = if wrap_entries.is_empty() {
        None
    } else {
        let mut current = PluginView::HostComponent(HostComponentRef {
            name: "__content__".into(),
            ..Default::default()
        });
        for entry in &wrap_entries {
            let pool = {
                let plugins = self.plugins.read().await;
                match plugins.get(&entry.plugin_id) {
                    Some(p) if p.enabled.load(Ordering::Relaxed) => p.pool.clone(),
                    _ => continue,  // skip disabled/missing
                }
            };
            let params = RoutePattern(entry.route_pattern.clone())
                .extract_params(path)
                .unwrap_or_default();
            let input = TransformInput {
                original: Some(current.clone()),
                context: TransformContext {
                    route_params: params,
                    client: session.client.clone(),
                    ..Default::default()
                },
                session: session.clone(),
            };
            match call_export::<Json<TransformInput>, Json<TransformOutput>>(
                pool, entry.transform_fn.clone(), Json(input),
            ).await {
                Ok(Json(out)) => {
                    // Warn in debug builds if __content__ is absent
                    #[cfg(debug_assertions)]
                    if !view_contains_content_placeholder(&out.view) {
                        tracing::warn!(
                            plugin = %entry.plugin_id.0,
                            "Wrap transform '{}' omitted original_content(); chain cut",
                            entry.transform_fn,
                        );
                    }
                    current = out.view;
                }
                Err(e) => {
                    // Error isolation: keep current unchanged, log and continue
                    tracing::warn!(plugin = %entry.plugin_id.0, error = %e, "wrap transform failed, skipping");
                }
            }
        }
        Some(current)
    };

    // InjectAfter — error-isolated
    let after = self.run_inject_transforms(&inject_after_entries, path, session).await;

    Ok(RouteTransforms { before, wrap, after })
}
```

---

## Step 2 — PluginAwareRouter<R>

```rust
#[component]
pub fn PluginAwareRouter<R: dioxus_router::prelude::Routable + Clone>() -> Element
where
    <R as std::str::FromStr>::Err: std::fmt::Display,
{
    let path = use_current_path();
    let session_id = use_session_id();
    let client_caps = use_context::<ClientCapabilities>();
    let override_map = use_context::<ReadOnlySignal<OverrideMap>>();

    let has_transforms = override_map.read()
        .route_patterns
        .iter()
        .any(|p| p.matches(&path));

    let transforms = use_resource(move || {
        let path = path.clone();
        let sid = session_id.read().clone();
        let caps = client_caps.clone();
        async move {
            if has_transforms {
                get_route_transforms(path, sid, caps).await
            } else {
                Ok(RouteTransforms::empty())
            }
        }
    });

    match transforms.read().as_ref() {
        Some(Ok(t)) if t.has_wrap() => rsx! {
            for view in &t.before {
                PluginViewRenderer { view: view.clone(), session_id }
            }
            PluginViewRenderer {
                view: t.wrap.clone().unwrap(),
                session_id,
                content_slot: rsx! { dioxus_router::prelude::Outlet::<R> {} },
            }
            for view in &t.after {
                PluginViewRenderer { view: view.clone(), session_id }
            }
        },
        Some(Ok(t)) => rsx! {
            for view in &t.before { PluginViewRenderer { view: view.clone(), session_id } }
            dioxus_router::prelude::Outlet::<R> {}
            for view in &t.after { PluginViewRenderer { view: view.clone(), session_id } }
        },
        _ => rsx! { dioxus_router::prelude::Outlet::<R> {} },
    }
}
```

`use_current_path()` implementation:
```rust
pub fn use_current_path() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.location().pathname().ok())
            .unwrap_or_else(|| "/".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // Dioxus 0.7 router hook — verify exact API
        use_route::<String>()
    }
}
```

---

## Step 3 — route-injection-example

```
examples/route-injection-example/
├── plugin/   <- Wrap + InjectAfter for "/product/:id"
└── host/     <- uses PluginAwareRouter<Route>, normal ProductPage (zero plugin code)
```

ProductPage must contain zero plugin-related code. Verify the plugin wraps the page
and injects a widget below, with ProductPage completely unchanged.

---

## Verification

```bash
cargo check --workspace
cargo test --workspace --lib
cargo test -p dioxus-extism-host -- route_transforms
```

---

---

# Phase 5 — Layer 3: Tree Selectors

**Prerequisite:** Phase 4 stop condition confirmed.

**Goal:** Plugins can target and transform specific nodes inside other plugins' rendered
output. All `NodeSelector` variants work, including `Recursive`. The `tree-selector-example`
demonstrates plugin-on-plugin composition.

**Stop condition:** `cargo check --workspace && cargo test --workspace --lib` passes.

---

## Step 1 — NodeSelector tree traversal

### Write tests first in `crates/dioxus-extism-host/tests/tree.rs`:

```rust
fn make_tree() -> PluginView {
    PluginView::Element(ViewElement {
        tag: "div".into(),
        children: vec![
            PluginView::Element(ViewElement {
                tag: "span".into(),
                attrs: vec![("class".into(), AttrValue::String("badge important".into()))],
                name: Some("the-badge".into()),
                children: vec![PluginView::Text("42".into())],
                ..Default::default()
            }),
            PluginView::Element(ViewElement {
                tag: "button".into(),
                attrs: vec![("data-plugin-slot".into(), AttrValue::String("actions".into()))],
                ..Default::default()
            }),
        ],
        ..Default::default()
    })
}

#[test] fn has_class_shallow_match() {
    let tree = make_tree();
    let m = find_shallow_matching(&tree, &NodeSelector::HasClass("badge".into()));
    assert_eq!(m.len(), 1);
}

#[test] fn data_attr_shallow_match() {
    let tree = make_tree();
    let m = find_shallow_matching(&tree, &NodeSelector::DataAttr(
        "data-plugin-slot".into(), "actions".into()
    ));
    assert_eq!(m.len(), 1);
}

#[test] fn name_shallow_match() {
    let tree = make_tree();
    let m = find_shallow_matching(&tree, &NodeSelector::Name("the-badge".into()));
    assert_eq!(m.len(), 1);
}

#[test] fn shallow_does_not_find_deep() {
    // Wrap tree in extra layer — shallow search from outer should not find inner
    let outer = PluginView::Element(ViewElement {
        tag: "section".into(),
        children: vec![make_tree()],
        ..Default::default()
    });
    // Shallow search from outer's direct children (= make_tree() root div)
    // only tests the div, not its descendants
    let m = find_shallow_matching(&outer, &NodeSelector::HasClass("badge".into()));
    assert_eq!(m.len(), 0, "shallow should not descend");
}

#[test] fn recursive_finds_deep() {
    let outer = PluginView::Element(ViewElement {
        tag: "section".into(),
        children: vec![make_tree()],
        ..Default::default()
    });
    let m = find_recursive_matching(
        &outer,
        &NodeSelector::Recursive(Box::new(NodeSelector::HasClass("badge".into())))
    );
    assert_eq!(m.len(), 1);
}
```

### Implement `crates/dioxus-extism-host/src/tree.rs`:

```rust
/// Test direct children only (SHALLOW — the default).
pub fn find_shallow_matching<'a>(
    view: &'a PluginView,
    selector: &NodeSelector,
) -> Vec<&'a PluginView> {
    match view {
        PluginView::Element(el) => el.children.iter()
            .filter(|c| node_matches(c, selector))
            .collect(),
        PluginView::Fragment(children) => children.iter()
            .filter(|c| node_matches(c, selector))
            .collect(),
        _ => vec![],
    }
}

/// Test nodes at any depth.
pub fn find_recursive_matching<'a>(
    view: &'a PluginView,
    selector: &NodeSelector,
) -> Vec<&'a PluginView> {
    let mut results = vec![];
    collect_recursive(view, selector, &mut results);
    results
}

fn collect_recursive<'a>(
    view: &'a PluginView,
    selector: &NodeSelector,
    out: &mut Vec<&'a PluginView>,
) {
    if node_matches(view, selector) { out.push(view); }
    if let PluginView::Element(el) = view {
        for child in &el.children { collect_recursive(child, selector, out); }
    }
}

pub fn node_matches(view: &PluginView, selector: &NodeSelector) -> bool {
    match (view, selector) {
        (PluginView::Element(el), NodeSelector::Tag(t)) => &el.tag == t,
        (PluginView::Element(el), NodeSelector::HasClass(cls)) => {
            el.attrs.iter().any(|(k, v)| k == "class"
                && matches!(v, AttrValue::String(s) if s.split_whitespace().any(|c| c == cls)))
        }
        (PluginView::Element(el), NodeSelector::Name(n)) => el.name.as_deref() == Some(n),
        (PluginView::Element(el), NodeSelector::DataAttr(k, v)) => {
            el.attrs.iter().any(|(ak, av)| ak == k
                && matches!(av, AttrValue::String(s) if s == v))
        }
        (_, NodeSelector::Recursive(inner)) => node_matches(view, inner),
        (_, NodeSelector::And(a, b)) => node_matches(view, a) && node_matches(view, b),
        (_, NodeSelector::Or(a, b)) => node_matches(view, a) || node_matches(view, b),
        (PluginView::HostComponent(r), NodeSelector::HostComponent(n)) => &r.name == n,
        _ => false,
    }
}
```

---

## Step 2 — apply_tree_transforms

Wire into end of `render_slot` (step 3 of slot pipeline). Traverse each SlotContent's
`PluginView`, find matching nodes, call the transform, apply the op. Error isolation:
failed node transform leaves node unchanged, traversal continues.

---

## Step 3 — tree-selector-example

```
examples/tree-selector-example/
├── plugin_a/  <- SlotProvider for "activity-feed" using .plugin_slot("feed-actions")
├── plugin_b/  <- Within transform targeting DataAttr("feed-actions") — InsertAfter
└── host/      <- <PluginSlot name="activity-feed" />, nothing else plugin-related
```

Verify plugin_b's buttons appear in plugin_a's output with zero host involvement.

---

## Verification

```bash
cargo check --workspace
cargo test --workspace --lib
cargo test -p dioxus-extism-host -- tree
```

---

---

# Phase 6 — State, Hot-reload, SSR, and Polish

**Prerequisite:** Phase 5 stop condition confirmed.

**Goal:** All remaining features complete. Full test suite clean. Documentation complete.
All examples run.

**Stop condition:**
```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo doc --workspace --no-deps
cargo check -p dioxus-extism-protocol --target wasm32-unknown-unknown
```
All must pass. All examples must start without errors.

---

## Step 1 — Global state + session TTL

**Global state host functions** (`dx_global_state_get`/`set`):
- Capability check: `HostCapability::GlobalStateRead/Write { keys }` must contain the key.
- Read/write `PluginRuntime::global_states`.

**Session TTL eviction background task** (spawned in `build()`):
```rust
let session_states = Arc::clone(&self.session_states);
let ttl = self.session_ttl;
tokio::spawn(async move {
    let mut interval = tokio::time::interval(ttl / 4);
    loop {
        interval.tick().await;
        let cutoff = std::time::Instant::now() - ttl;
        let mut states = session_states.write().await;
        // Remove sessions not accessed within TTL
        states.session_last_access.retain(|id, last| {
            if *last < cutoff {
                states.data.remove(id);
                false
            } else { true }
        });
    }
});
```

---

## Step 2 — Global state persistence

Implement `JsonFilePersistence`:
```rust
pub struct JsonFilePersistence { pub dir: std::path::PathBuf }

#[async_trait::async_trait]
impl StatePersistenceProvider for JsonFilePersistence {
    async fn save(&self, plugin_id: &PluginId, state: &...) -> Result<(), PersistenceError> {
        let path = self.dir.join(format!("{}.json", plugin_id.0.replace('/', "_")));
        tokio::fs::create_dir_all(&self.dir).await?;
        let json = serde_json::to_string_pretty(state)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }
    async fn load(&self, plugin_id: &PluginId) -> Result<Option<...>, PersistenceError> {
        let path = self.dir.join(format!("{}.json", plugin_id.0.replace('/', "_")));
        match tokio::fs::read_to_string(path).await {
            Ok(s) => Ok(Some(serde_json::from_str(&s)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(PersistenceError::Io(e)),
        }
    }
}
```

In `build()`: restore global state from persistence. In `dx_global_state_set`:
after writing to memory, spawn a task to flush to persistence (non-blocking).

---

## Step 3 — enable_plugin / disable_plugin

```rust
pub fn disable_plugin(&self, id: &PluginId) -> Result<(), PluginRuntimeError> {
    let plugins = self.plugins.blocking_read();
    plugins.get(id)
        .ok_or_else(|| PluginRuntimeError::PluginNotFound(id.clone()))?
        .enabled.store(false, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}
```

No registry rebuild needed — `enabled` is checked at dispatch time.

---

## Step 4 — reload_plugin / unload_plugin

Follow architecture section 4.7 and CLAUDE.md Pattern 3 exactly.

Key: `on_unload` called before write lock, `on_load` called on new pool before
insert, registry rebuild and version bump inside write lock, broadcast after.

Also call `on_unload` in `Drop` for `LoadedPlugin` as a safety net:
```rust
impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        if self.pool.try_get().is_ok() {
            // Try to call on_unload — best effort
        }
    }
}
```

---

## Step 5 — HTTP host function

```rust
fn dx_http_fetch_impl(/* ... */) {
    // 1. Capability check: HostCapability::Http { allowed_hosts }
    // 2. Deserialise HttpRequest from WASM memory
    // 3. Check URL host against allowed_hosts
    // 4. block_on(reqwest::Client::new().execute(request))
    // 5. Serialise HttpResponse to WASM memory
}
```

---

## Step 6 — use_plugin_state frontend hook

```rust
pub fn use_plugin_state<T>(
    plugin_id: impl Into<String>,
    key: impl Into<String>,
) -> ReadOnlySignal<Option<T>>
where T: serde::de::DeserializeOwned + Clone + PartialEq + Send + Sync + 'static {
    let pid = plugin_id.into();
    let key = key.into();
    let session_id = use_session_id();

    use_resource(move || {
        let pid = pid.clone(); let key = key.clone();
        let sid = session_id.read().clone();
        async move { get_plugin_state::<T>(pid, key, sid).await.ok().flatten() }
    })
}
```

---

## Step 7 — SSR mode

Implement `PluginRuntime::ssr_render_route` (async pre-fetch of all slot content
and route transforms, then returns `SsrRouteOutput`).

Implement `PluginSlotSsr` and `SsrPluginDataProvider` in frontend — read from
`SsrRouteOutput` context instead of server function calls.

Add `ssr-example`:
```
examples/ssr-example/
├── plugin/   <- standard slot plugin
└── host/     <- Axum handler calling ssr_render_route + dioxus_ssr::render
```

---

## Step 8 — dioxus-extism re-export crate

```toml
[features]
default  = []
host     = ["dep:dioxus-extism-host"]
frontend = ["dep:dioxus-extism-frontend"]
pdk      = ["dep:dioxus-extism-pdk"]
test     = ["dep:dioxus-extism-test"]
```

```rust
pub use dioxus_extism_protocol as protocol;
#[cfg(feature = "host")]     pub use dioxus_extism_host     as host;
#[cfg(feature = "frontend")] pub use dioxus_extism_frontend as frontend;
#[cfg(feature = "pdk")]      pub use dioxus_extism_pdk      as pdk;
#[cfg(feature = "test")]     pub use dioxus_extism_test     as test;
```

---

## Step 9 — tracing instrumentation

Add `#[tracing::instrument]` to:
- `call_export` — log plugin_id, export name, duration
- `render_slot` — log slot name, contrib count, transform count
- `render_route_transforms` — log path, partition counts
- `run_hook` — log hook name, outcome
- `handle_interaction` — log plugin_id, handler_id
- `InvocationRegistry::call` — log name, duration

Emit `tracing::warn!` for:
- Wrap chain cuts (missing `__content__`)
- Plugin compatibility skips
- Any plugin call error (with error isolation context)

---

## Step 10 — Documentation and final checks

- Every public item must have a `///` doc comment.
- Every example must have `examples/<name>/README.md`.
- `cargo doc --workspace --no-deps` must emit zero warnings.

### Final verification:

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo doc --workspace --no-deps

# Build all example plugins
cargo build --target wasm32-unknown-unknown --release \
  -p hello-plugin-plugin \
  -p slot-example-plugin \
  -p route-injection-example-plugin \
  -p tree-selector-example-plugin \
  -p hook-example-plugin \
  -p invocation-example-plugin

# Start each host example and verify it launches without errors
cargo run -p hello-plugin-host
cargo run -p slot-example-host
```

Phase 6 is complete when all commands pass and all examples launch successfully.
