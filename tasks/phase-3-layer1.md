# Phase 3 — Layer 1 Full: #[overridable] and Component Overrides

**Prerequisite:** Phase 2 stop condition confirmed.

**Goal:** `#[overridable]` macro expands correctly, `OverridableComponent` uses the
`OverrideMap` fast path, `resolve_component` works end-to-end, and the
`slot-example` demonstrates both named slots and component overrides.

**Stop condition:** `cargo check --workspace && cargo test --workspace --lib` passes.
The `slot-example` starts and renders correctly.

---

## Step 1 — TransformRegistry

### Write tests first in `crates/dioxus-extism-host/tests/transform_registry.rs`:

```rust
use dioxus_extism_host::*;
use dioxus_extism_protocol::*;

fn entry(plugin: &str, priority: i32, op: TransformOp) -> TransformEntry {
    TransformEntry {
        plugin_id: PluginId(plugin.into()),
        transform_fn: "fn".into(),
        op,
        priority,
    }
}

#[test]
fn component_transforms_sorted_priority_desc() {
    let mut reg = TransformRegistry::default();
    for p in [100, 750, 500] {
        reg.insert_component("Hero", entry("p", p, TransformOp::WrapNode));
    }
    let results = reg.for_component("Hero");
    assert_eq!(results[0].priority, 750);
    assert_eq!(results[1].priority, 500);
    assert_eq!(results[2].priority, 100);
}

#[test]
fn unknown_component_returns_empty() {
    let reg = TransformRegistry::default();
    assert!(reg.for_component("Unknown").is_empty());
}

#[test]
fn route_matches_pattern() {
    let mut reg = TransformRegistry::default();
    reg.insert_route(
        RoutePattern("/product/:id".into()),
        entry("p", 500, TransformOp::InjectAfter),
    );
    assert_eq!(reg.for_route("/product/42").len(), 1);
    assert!(reg.for_route("/other").is_empty());
}
```

### Implement `TransformRegistry`:

```rust
#[derive(Default)]
pub(crate) struct TransformRegistry {
    by_component: std::collections::HashMap<String, Vec<TransformEntry>>,
    by_slot:      std::collections::HashMap<String, Vec<TransformEntry>>,
    by_route:     Vec<(RoutePattern, TransformEntry)>,
    by_data_slot: std::collections::HashMap<String, Vec<TransformEntry>>,
    within:       Vec<(Selector, NodeSelector, TransformEntry)>,
}

impl TransformRegistry {
    pub fn insert_component(&mut self, name: &str, entry: TransformEntry) {
        let v = self.by_component.entry(name.to_owned()).or_default();
        v.push(entry);
        v.sort_by_key(|e| std::cmp::Reverse(e.priority));
    }
    // ... insert_slot, insert_route, insert_data_slot, insert_within
    pub fn for_component(&self, name: &str) -> Vec<TransformEntry> { /* ... */ }
    pub fn for_slot(&self, name: &str) -> Vec<TransformEntry> { /* ... */ }
    pub fn for_route(&self, path: &str) -> Vec<TransformEntry> { /* ... */ }
    pub fn within_for(&self, outer: &Selector) -> Vec<(NodeSelector, TransformEntry)> { /* ... */ }
    pub fn all_component_names(&self) -> std::collections::HashSet<String> { /* ... */ }
    pub fn all_slot_names(&self) -> std::collections::HashSet<String> { /* ... */ }
}
```

---

## Step 2 — resolve_component

### Write test:

```rust
#[tokio::test]
async fn resolve_component_returns_none_for_unknown() {
    let runtime = PluginRuntimeBuilder::new().build().await.unwrap();
    let session = MockSession::new().as_ctx();
    let result = runtime.resolve_component("Unknown", serde_json::json!({}), &session).await.unwrap();
    assert!(result.is_none());
}
```

### Implement with error isolation:

```rust
pub async fn resolve_component(
    &self,
    component_name: &str,
    props: serde_json::Value,
    session: &SessionCtx,
) -> Result<Option<ComponentResolution>, PluginRuntimeError> {
    let entries = {
        let regs = self.registries.read().await;
        regs.transforms.for_component(component_name)
    };
    if entries.is_empty() { return Ok(None); }

    let mut before = vec![];
    let mut replacement = None;
    let mut after = vec![];

    for entry in entries {
        let pool = {
            let plugins = self.plugins.read().await;
            match plugins.get(&entry.plugin_id) {
                Some(p) if p.enabled.load(Ordering::Relaxed)
                        && self.is_compatible(p, &session.client) => p.pool.clone(),
                _ => continue,
            }
        };

        let context = TransformContext {
            component_props: Some(props.clone()),
            client: session.client.clone(),
            ..Default::default()
        };
        let input = TransformInput { original: None, context, session: session.clone() };

        match call_export::<Json<TransformInput>, Json<TransformOutput>>(
            pool, entry.transform_fn.clone(), Json(input),
        ).await {
            Ok(Json(output)) => match entry.op {
                TransformOp::InjectBefore => before.push(output.view),
                TransformOp::InjectAfter  => after.push(output.view),
                TransformOp::WrapNode | TransformOp::Replace => replacement = Some(output.view),
                _ => {}
            },
            Err(e) => {
                tracing::warn!(plugin = %entry.plugin_id.0, component = component_name, error = %e, "skipped");
            }
        }
    }

    Ok(Some(ComponentResolution { before, replacement, after }))
}
```

---

## Step 3 — OverrideMap population and fast path

`build_registries()` must set:
- `override_map.overridden_components`: names with any registered transforms
- `override_map.plugin_requirements`: per-plugin min versions + required components
- `override_map.required_protocol_version`: max across all plugins
- `override_map.required_app_version`: max across all plugins

`OverridableComponent` local check in frontend (zero network overhead when no transforms):

```rust
let override_map = use_context::<ReadOnlySignal<OverrideMap>>();
if !override_map.read().overridden_components.contains(&name) {
    return fallback;  // no server call
}
// proceed with use_resource call to resolve_component server fn
```

---

## Step 4 — `#[overridable]` proc macro

Read CLAUDE.md proc-macro pitfall first. Then:

### Add trybuild test:

```toml
# dioxus-extism-macros/dev-dependencies
trybuild = { workspace = true }
```

Create `tests/overridable/pass/basic.rs`:
```rust
use dioxus_extism_macros::overridable;
use dioxus::prelude::*;

#[overridable]
#[component]
fn MyComponent(title: String, count: i64) -> Element {
    rsx! { div { "{title}: {count}" } }
}

fn main() {}
```

Test:
```rust
#[test]
fn compile_tests() {
    let t = trybuild::TestCases::new();
    t.pass("tests/overridable/pass/*.rs");
    t.compile_fail("tests/overridable/fail/*.rs");
}
```

### Implement the macro:

Parse the function with `syn::ItemFn`. Extract name, parameters, body. Generate:

```rust
// Generated output (using quote!):
#[dioxus::prelude::component]
fn MyComponent(title: String, count: i64) -> dioxus::prelude::Element
where
    String: serde::Serialize,
    i64: serde::Serialize,
{
    let __props = serde_json::json!({
        "title": title,
        "count": count,
    });
    dioxus::prelude::rsx! {
        dioxus_extism_frontend::OverridableComponent {
            name: "MyComponent",
            props: __props,
            fallback: dioxus::prelude::rsx! { /* original body */ },
        }
    }
}
```

Edge cases to handle with `compile_error!`:
- Parameters using `impl Trait` syntax (cannot add Serialize bound)
- Emit clear message: "Parameter `foo: impl Foo` cannot be used with #[overridable].
  Use a concrete type or wrap in a serialisable struct."

---

## Step 5 — plugin! macro (transform support)

Extend `plugin!` to generate `transform_*` exports:

```rust
plugin! {
    type: MyPlugin,
    slots: [MyPlugin],
    transforms: [
        wrap_hero => MyPlugin::wrap_hero,
    ],
}
```

Generates:
```rust
#[extism_pdk::plugin_fn]
pub fn transform_wrap_hero(
    input: extism_pdk::Json<TransformInput>,
) -> extism_pdk::FnResult<extism_pdk::Json<TransformOutput>> {
    let ctx = PluginCtx::from_input(&input.0);
    MyPlugin::wrap_hero(input.0, &ctx)
        .map(extism_pdk::Json)
        .map_err(Into::into)
}
```

---

## Step 6 — slot-example

```
examples/slot-example/
├── plugin/   <- SlotProvider for "sidebar" + WrapNode transform on "ProductHero"
└── host/     <- #[overridable] on ProductHero, <PluginSlot name="sidebar" />
```

Verify:
- Slot renders plugin content
- ProductHero wrapped by plugin
- When no plugin registers for a component, fallback renders (zero network call)
- OverridableComponent fast path: set `overridden_components` to empty in test to
  confirm fallback renders immediately without `use_resource` firing

---

## Verification

```bash
cargo check --workspace
cargo test --workspace --lib
cargo test -p dioxus-extism-macros  # includes trybuild tests
cargo clippy --workspace -- -D warnings
```

Start `slot-example` host and visually verify plugin slot and component override.
