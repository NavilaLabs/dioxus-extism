# Testing Plan — `dioxus-extism`

Covers general correctness tests and security tests for the workspace. Each entry
describes a test that is missing or only partially covered by the existing files in
`crates/dioxus-extism-protocol/tests/`, `crates/dioxus-extism-host/tests/`,
`crates/dioxus-extism-macros/tests/`, and `crates/dioxus-extism-test/src/lib.rs`.

---

## General Correctness Tests

---

## RoutePattern::matches — empty pattern matches nothing

**Crate:** `dioxus-extism-protocol`
**Requires WASM fixture:** no
**What it tests:** `RoutePattern("")` does not match any path, including `"/"` and `""`.
**Why it matters:** An unguarded empty pattern would match every path, silently routing all route-level transforms to all pages.
**Assertion:**
```rust
assert!(!RoutePattern("".into()).matches("/"));
assert!(!RoutePattern("".into()).matches(""));
assert!(!RoutePattern("".into()).matches("/product/42"));
```

---

## RoutePattern::matches — root pattern does not match non-root path

**Crate:** `dioxus-extism-protocol`
**Requires WASM fixture:** no
**What it tests:** `RoutePattern("/")` matches only `"/"`, not paths with additional segments.
**Why it matters:** A root pattern that matches every path would apply transforms registered for `/` to every route in the application.
**Assertion:**
```rust
assert!(!RoutePattern("/".into()).matches("/product/42"));
assert!(!RoutePattern("/".into()).matches("/a"));
```

---

## RoutePattern::matches — multi-param pattern rejects too-short path

**Crate:** `dioxus-extism-protocol`
**Requires WASM fixture:** no
**What it tests:** `RoutePattern("/shop/:shop/item/:id")` does not match `"/shop/acme"`, which has too few segments to satisfy both parameters.
**Why it matters:** A partial match would silently pass `None` into `extract_params`, corrupting `TransformContext::route_params` for the plugin call.
**Assertion:**
```rust
assert!(!RoutePattern("/shop/:shop/item/:id".into()).matches("/shop/acme"));
assert_eq!(
    RoutePattern("/shop/:shop/item/:id".into()).extract_params("/shop/acme"),
    None
);
```

---

## RoutePattern::matches — trailing slash on pattern only

**Crate:** `dioxus-extism-protocol`
**Requires WASM fixture:** no
**What it tests:** `RoutePattern("/product/:id/")` (trailing slash in pattern) does not match `"/product/42"` (no trailing slash in path).
**Why it matters:** The existing `rejects_trailing_slash` test covers trailing slash on the path side; the pattern side is untested and asymmetric behaviour would cause silent missed matches.
**Assertion:**
```rust
assert!(!RoutePattern("/product/:id/".into()).matches("/product/42"));
```

---

## render_slot — contributions ordered priority-descending

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-high`, `fixture-slot-normal`)
**What it tests:** When two plugins both register for the same slot at different priorities, the returned `Vec<SlotContent>` is sorted so the higher-priority contribution appears first.
**Why it matters:** Priority ordering is a contractual guarantee; silent reordering changes the rendered stack, invalidating any host UI layout that depends on the defined order.
**Assertion:**
```rust
assert_eq!(contents.len(), 2);
assert!(contents[0].priority > contents[1].priority);
assert_eq!(contents[0].plugin_id, PluginId("test/slot-high".into()));
assert_eq!(contents[1].plugin_id, PluginId("test/slot-normal".into()));
```

---

## render_slot — CallFailed plugin contributes Incompatible; others unaffected

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-failing`, `fixture-slot-normal`)
**What it tests:** When one plugin's slot export returns an Extism error, its contribution in the returned `Vec` is `PluginView::Incompatible`; the other plugin's contribution is the correct `PluginView::Element`.
**Why it matters:** Per-plugin error isolation is the central reliability guarantee; one broken plugin must not blank the entire slot or abort the render call.
**Assertion:**
```rust
let failing = contents.iter().find(|c| c.plugin_id == PluginId("test/slot-failing".into())).unwrap();
let normal  = contents.iter().find(|c| c.plugin_id == PluginId("test/slot-normal".into())).unwrap();
assert!(matches!(failing.view, PluginView::Incompatible { .. }));
assert!(matches!(normal.view,  PluginView::Element(_)));
```

---

## render_slot — disabled plugin contributes Incompatible, not a gap

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`)
**What it tests:** After `disable_plugin`, `render_slot` returns exactly one `SlotContent` entry for the disabled plugin with view `PluginView::Incompatible`; the entry is present, not absent.
**Why it matters:** Callers rely on a dense `Vec` to render per-slot update prompts; a gap (absent entry) would prevent the host from rendering a "plugin disabled" placeholder at the correct position.
**Assertion:**
```rust
runtime.disable_plugin(&id).unwrap();
let contents = runtime.render_slot("test-slot", &session).await.unwrap();
assert_eq!(contents.len(), 1);
assert!(matches!(contents[0].view, PluginView::Incompatible { .. }));
```

---

## render_slot — min_protocol_version exceeds client version yields Incompatible

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`)
**What it tests:** When `session.client.protocol_version` is below the plugin's `min_protocol_version`, `render_slot` returns `PluginView::Incompatible` for that plugin without invoking the WASM export.
**Why it matters:** Serving protocol types an older client cannot decode would cause deserialization panics or silent data corruption on the frontend.
**Assertion:**
```rust
// fixture-slot-normal declares min_protocol_version = PROTOCOL_VERSION (= 1).
// Session simulates an older client with protocol_version = 0.
let old_session = MockSession::new().with_protocol_version(0).as_ctx();
let contents = runtime.render_slot("test-slot", &old_session).await.unwrap();
assert_eq!(contents.len(), 1);
assert!(matches!(contents[0].view, PluginView::Incompatible { .. }));
```

---

## render_slot — slot name not in registry returns Ok(vec![])

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`)
**What it tests:** Calling `render_slot` with a slot name that no loaded plugin has registered for returns `Ok(vec![])` rather than an error.
**Why it matters:** Host code iterates over slot contents; an `Err` result would require the caller to distinguish "no plugins" from "runtime failure", complicating all call sites.
**Assertion:**
```rust
let contents = runtime.render_slot("unregistered-slot", &session).await.unwrap();
assert!(contents.is_empty());
```

---

## run_hook — Continue → Replace → Cancel chain stops at cancelling plugin

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-hook-continue`, `fixture-hook-replace`, `fixture-hook-cancel`, `fixture-hook-after-cancel`)
**What it tests:** A hook chain where plugin 1 returns `Continue`, plugin 2 returns `Replace`, and plugin 3 returns `Cancel` produces `HookOutcome::Cancelled { by: plugin_3_id, .. }`; a fourth plugin registered below plugin 3 is never called.
**Why it matters:** `Cancel` is the mechanism for blocking form submissions, auth checks, and rate-limiting; if the chain continues past `Cancel`, security guards are silently bypassed.
**Assertion:**
```rust
// fixture-hook-after-cancel increments global state "after_cancel_count" when called.
let outcome = runtime.run_hook("test-hook", json!({"x": 1}), &session).await.unwrap();
assert!(matches!(&outcome,
    HookOutcome::Cancelled { by, reason }
    if *by == PluginId("test/hook-cancel".into()) && reason == "test-cancel"
));
// Confirm the plugin after the cancelling one was never invoked:
let count: u32 = runtime.global_state(&PluginId("test/hook-after-cancel".into()), "after_cancel_count")
    .unwrap_or(0);
assert_eq!(count, 0);
```

---

## run_hook — plugin Err in middle of chain does not abort; outcome is Passed

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-hook-continue`, `fixture-hook-erroring`)
**What it tests:** When a plugin's hook export returns an Extism error mid-chain, that plugin is skipped and the chain completes; the final outcome is `HookOutcome::Passed` carrying the last successful context value.
**Why it matters:** A hook chain that aborts on any error is brittle; a single buggy plugin would break every operation that runs that hook, including unrelated business logic.
**Assertion:**
```rust
// fixture-hook-continue (priority First) returns Continue { context: json!("continued") }.
// fixture-hook-erroring (priority High) returns extism_pdk error — is skipped.
// No further plugins; chain ends → Passed.
let outcome = runtime.run_hook("test-hook", json!("initial"), &session).await.unwrap();
assert!(matches!(&outcome, HookOutcome::Passed(ctx) if *ctx == json!("continued")));
```

---

## run_hook — no handlers registered returns Passed with original context

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** `run_hook` on a hook name with no registered handlers returns `HookOutcome::Passed` with the original context value unchanged.
**Why it matters:** Host code calling `run_hook` before any plugin is installed must not receive an error or a corrupted context value; the zero-plugin case is the baseline.
**Assertion:**
```rust
let ctx = json!({"value": 42});
let outcome = runtime.run_hook("unregistered-hook", ctx.clone(), &session).await.unwrap();
assert!(matches!(&outcome, HookOutcome::Passed(c) if *c == ctx));
```

---

## render_route_transforms — Wrap fold: higher-priority plugin receives seed, lower receives prior output

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-wrap-a`, `fixture-wrap-b`)
**What it tests:** In a two-plugin Wrap pipeline, `fixture-wrap-a` (priority High) receives `HostComponent("__content__")` as `TransformInput::original`; `fixture-wrap-b` (priority Low) receives `fixture-wrap-a`'s full output as `original`, not the seed.
**Why it matters:** The sequential pipeline model breaks down if lower-priority plugins receive the seed instead of the accumulated view; they would not be able to see or react to what higher-priority plugins contributed.
**Assertion:**
```rust
// Each fixture records its received `original` in global state under key "received_original".
let result = runtime.render_route_transforms("/test/42", &session).await.unwrap();
let wrap = result.wrap.unwrap();
// fixture-wrap-a must contain "marker-a" text AND original_content somewhere inside:
assert!(view_contains_text(&wrap, "marker-a"), "wrap-a marker missing from fold result");
assert!(view_contains_text(&wrap, "marker-b"), "wrap-b marker missing from fold result");
// Verify wrap-a received __content__ seed (stored in global state by the fixture):
let received_a: serde_json::Value =
    runtime.global_state_json(&PluginId("test/wrap-a".into()), "received_original").unwrap();
assert_eq!(received_a["HostComponent"]["name"], "__content__");
```

---

## render_route_transforms — Wrap plugin call fails: current_view passes through unchanged

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-wrap-a`, `fixture-wrap-failing`, `fixture-wrap-b`)
**What it tests:** When `fixture-wrap-failing` (priority Normal, between High and Low) errors on its transform call, the fold continues with the current accumulated view unchanged; the final result still contains `fixture-wrap-a`'s and `fixture-wrap-b`'s markers.
**Why it matters:** One failing Wrap plugin must not blank the page or abort the fold; the accumulated view from prior plugins must pass through as-is to the next plugin.
**Assertion:**
```rust
let wrap = result.wrap.unwrap();
assert!(view_contains_text(&wrap, "marker-a"), "wrap-a must survive failing wrap");
assert!(view_contains_text(&wrap, "marker-b"), "wrap-b must survive failing wrap");
assert!(!view_contains_text(&wrap, "marker-failing"), "failing plugin must not contribute");
```

---

## render_route_transforms — Wrap plugin output omits __content__: tracing::warn emitted

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-wrap-no-content`)
**What it tests:** When a Wrap plugin returns a view that does not contain `HostComponent("__content__")`, a `tracing::warn!` is emitted and the fold continues using the no-content plugin's output as `current_view`.
**Why it matters:** Silent chain cuts are the hardest failure mode to diagnose; the warning is the only signal to plugin authors that earlier output was discarded.
**Assertion:**
```rust
// Install a tracing subscriber that captures WARN events before the call.
let result = runtime.render_route_transforms("/test/42", &session).await.unwrap();
assert!(result.wrap.is_some(), "fold must not abort on missing __content__");
assert!(
    captured_warnings.iter().any(|w| w.contains("__content__") || w.contains("original_content")),
    "expected a WARN about missing __content__ in wrap output"
);
```

---

## render_route_transforms — path matches no route pattern returns is_empty()

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-wrap-a`)
**What it tests:** With `fixture-wrap-a` loaded and registered for `"/test/:id"`, calling `render_route_transforms("/other/path", &session)` returns `RouteTransforms` for which `is_empty()` is `true`.
**Why it matters:** Unmatched paths must not accidentally receive transforms intended for a different route pattern.
**Assertion:**
```rust
let result = runtime.render_route_transforms("/other/path", &session).await.unwrap();
assert!(result.is_empty());
```

---

## apply_tree_transforms — NodeSelector::Recursive finds a node at depth 3

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-within-selector`)
**What it tests:** A `Within` transform using `NodeSelector::Recursive(HasClass("target"))` locates and transforms a node that is three levels deep in the `PluginView` tree (root → div → div → span.target).
**Why it matters:** Layer 3 plugin-on-plugin composition requires recursive matching to target deeply nested structures; a silently capped depth would prevent transforms from reaching their intended nodes.
**Assertion:**
```rust
// fixture-within-selector's transform replaces matched nodes with Text("TRANSFORMED-RECURSIVE").
// Build tree: root(div(div(span.target))); depth-3 span.target must be transformed.
let result = runtime.apply_tree_transforms(&outer, tree, context, &session).await.unwrap();
assert!(
    view_contains_text(&result, "TRANSFORMED-RECURSIVE"),
    "depth-3 node not reached by Recursive selector"
);
```

---

## apply_tree_transforms — shallow NodeSelector does not descend past direct children

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-within-selector`)
**What it tests:** A `Within` transform using `HasClass("shallow-target")` (no `Recursive` wrapper) does not match a node that is two levels deep from the outer selection root; only direct children are tested.
**Why it matters:** Shallow-by-default prevents accidental broad matches; the `Recursive` opt-in enforces intentionality and protects against performance surprises on large trees.
**Assertion:**
```rust
// Build tree: root(div(span.shallow-target)); span is at depth 2.
let result = runtime.apply_tree_transforms(&outer, tree.clone(), context, &session).await.unwrap();
assert_eq!(result, tree, "shallow selector must leave depth-2 node unchanged");
```

---

## apply_tree_transforms — NodeSelector::And requires both conditions

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-within-selector`)
**What it tests:** `NodeSelector::And(HasClass("a"), HasClass("b"))` matches only the node that has both classes; a node with only `"a"` and a node with only `"b"` are not matched.
**Why it matters:** Incorrect `And` logic that behaves like `Or` would transform unintended nodes in Layer 3 plugin-on-plugin composition, producing malformed output for the host.
**Assertion:**
```rust
// Tree direct children: span.a.b (both), span.a (only a), span.b (only b).
// Only span.a.b should be transformed.
let result = runtime.apply_tree_transforms(&outer, tree, context, &session).await.unwrap();
assert_eq!(count_text_nodes(&result, "TRANSFORMED-AND"), 1);
```

---

## apply_tree_transforms — NodeSelector::Or matches either condition

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-within-selector`)
**What it tests:** `NodeSelector::Or(HasClass("c"), HasClass("d"))` matches nodes with class `"c"`, class `"d"`, or both; a node with class `"other"` is not matched.
**Why it matters:** An `Or` that silently requires both conditions would miss valid targets, causing plugins to produce incomplete views with no error signal.
**Assertion:**
```rust
// Tree direct children: span.c, span.d, span.c.d, span.other.
// Three nodes (c, d, c.d) should be transformed; "other" must not be.
let result = runtime.apply_tree_transforms(&outer, tree, context, &session).await.unwrap();
assert_eq!(count_text_nodes(&result, "TRANSFORMED-OR"), 3);
```

---

## apply_tree_transforms — no within-transforms registered returns view unchanged

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** `apply_tree_transforms` with a selector for which no `Within` entries are registered in `TransformRegistry` returns the input view unmodified without invoking any plugin.
**Why it matters:** Calling into the TransformRegistry for every overridable component boundary on every render would add needless overhead; the fast path must be correct and side-effect-free.
**Assertion:**
```rust
// Empty runtime — no Within entries.
let result = runtime.apply_tree_transforms(
    &Selector::Slot("test-slot".into()),
    original_view.clone(),
    context,
    &session,
).await.unwrap();
assert_eq!(result, original_view);
```

---

## TransformRegistry::for_route — two overlapping patterns both returned, priority-sorted

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** When two `RoutePattern` entries (`"/products/:id"` and `"/:category/:id"`) both match `"/products/42"`, `for_route` returns both entries sorted priority-descending.
**Why it matters:** Overlapping route patterns are valid; silently dropping one entry would omit a plugin's transform without any error.
**Assertion:**
```rust
reg.insert_route(RoutePattern("/products/:id".into()), entry("p1", 750, TransformOp::Wrap));
reg.insert_route(RoutePattern("/:category/:id".into()), entry("p2", 500, TransformOp::InjectAfter));
let entries = reg.for_route("/products/42");
assert_eq!(entries.len(), 2);
assert!(entries[0].priority >= entries[1].priority);
```

---

## TransformRegistry::insert_within / within_for_outer — round-trip isolation

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** An entry inserted via `insert_within` for `Selector::Slot("sidebar")` is returned by `within_for_outer(&Selector::Slot("sidebar"))` and is absent from `within_for_outer(&Selector::Slot("header"))`.
**Why it matters:** Within-transform lookup must be scoped to its registered outer selector; a match against the wrong outer selector would apply transforms to plugin output they were never meant to modify.
**Assertion:**
```rust
reg.insert_within(Selector::Slot("sidebar".into()), node_sel, entry("p", 500, TransformOp::WrapNode));
assert_eq!(reg.within_for_outer(&Selector::Slot("sidebar".into())).len(), 1);
assert!(reg.within_for_outer(&Selector::Slot("header".into())).is_empty());
```

---

## PluginInstallConfig::resolve — priority tier precedence

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** `resolve` returns the per-name override when set; `base_priority` when no name override exists; `hint.as_numeric()` when neither is set — each tier independently and not blended.
**Why it matters:** Incorrect precedence silently misorders plugins; an installer's fine-grained override would be ignored if `base_priority` incorrectly outranked it.
**Assertion:**
```rust
let cfg = PluginInstallConfig {
    overrides: [("slot_a".into(), 999)].into_iter().collect(),
    base_priority: Some(500),
    ..Default::default()
};
assert_eq!(cfg.resolve("slot_a", &PriorityHint::Normal), 999); // name override wins
assert_eq!(cfg.resolve("slot_b", &PriorityHint::Normal), 500); // base_priority wins
assert_eq!(PluginInstallConfig::default().resolve("slot_c", &PriorityHint::High), 750); // hint used
```

---

## PluginInstallConfig::resolve — tie at equal resolved priority preserves insertion order

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`, `fixture-slot-high` loaded with base_priority overriding to the same value)
**What it tests:** When two plugins both resolve to priority 500 for the same slot, `render_slot` returns them in the order `add_plugin` was called on the builder.
**Why it matters:** Non-determinism under equal priority would make integration tests flaky and break host applications relying on stable ordering between plugins of equivalent precedence.
**Assertion:**
```rust
// fixture-slot-normal added first, fixture-slot-high added second, both forced to priority 500.
let contents = runtime.render_slot("test-slot", &session).await.unwrap();
assert_eq!(contents[0].plugin_id, PluginId("test/slot-normal".into()));
assert_eq!(contents[1].plugin_id, PluginId("test/slot-high".into()));
```

---

## reload_plugin — OverrideMap version increments by exactly 1

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`)
**What it tests:** `reload_plugin` increments `OverrideMap::version` by exactly 1; neither 0 (no-op) nor 2+ (double increment).
**Why it matters:** The version field is the signal for SSE-connected frontends to re-fetch the map; an incorrect delta causes stale UI (`+0`) or unnecessary re-renders (`+2`).
**Assertion:**
```rust
let before = runtime.override_map().await.version;
runtime.reload_plugin(&id, source, config).await.unwrap();
let after = runtime.override_map().await.version;
assert_eq!(after, before + 1);
```

---

## reload_plugin — on_unload called on old pool before swap

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`)
**What it tests:** `reload_plugin` invokes the old plugin's `on_unload` export before replacing the pool; confirmed by a counter the fixture increments in global state.
**Why it matters:** `on_unload` is the mechanism for releasing external connections and flushing state; skipping it on reload causes leaks that accumulate across hot-reload cycles.
**Assertion:**
```rust
// fixture-slot-normal's on_unload writes dx_global_state_set("unload_count", count + 1).
let before: u32 = runtime.global_state_json(&id, "unload_count").unwrap_or(json!(0))
    .as_u64().unwrap_or(0) as u32;
runtime.reload_plugin(&id, source, config).await.unwrap();
let after: u32 = runtime.global_state_json(&id, "unload_count").unwrap()
    .as_u64().unwrap() as u32;
assert_eq!(after, before + 1);
```

---

## unload_plugin — slot disappears from render_slot

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`)
**What it tests:** After `unload_plugin`, `render_slot` for the slot the plugin was registered for returns an empty `Vec`.
**Why it matters:** Stale slot registrations that survive unload would serve views from a dropped pool, causing use-after-free in the WASM executor or producing views the host no longer expects.
**Assertion:**
```rust
assert_eq!(runtime.render_slot("test-slot", &session).await.unwrap().len(), 1);
runtime.unload_plugin(&id).await.unwrap();
assert!(runtime.render_slot("test-slot", &session).await.unwrap().is_empty());
```

---

## enable_plugin / disable_plugin — toggle during concurrent render: no panic, Incompatible returned

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`)
**What it tests:** Toggling the `enabled` flag while `render_slot` tasks are concurrently in flight causes no panic; every returned contribution is either `PluginView::Element` or `PluginView::Incompatible`, never an unrecognised variant.
**Why it matters:** The `AtomicBool` toggle must be safe under concurrent access; a race on the flag could deliver a partially-initialised contribution or panic the blocking thread.
**Assertion:**
```rust
let handles: Vec<_> = (0..10).map(|_| {
    let rt = runtime.clone();
    let s = session.clone();
    tokio::spawn(async move { rt.render_slot("test-slot", &s).await })
}).collect();
runtime.disable_plugin(&id).unwrap();
let results = futures::future::join_all(handles).await;
for r in results {
    let contents = r.expect("task panicked").unwrap();
    for c in &contents {
        assert!(
            matches!(c.view, PluginView::Element(_) | PluginView::Incompatible { .. }),
            "unexpected view variant: {:?}", c.view
        );
    }
}
```

---

## InvocationRegistry::call — timeout fires within 2× the configured wall time

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** A handler registered with a 50 ms timeout that sleeps for 10 seconds causes `InvocationRegistry::call` to return `InvocationError::Timeout` within 1 second wall-clock time.
**Why it matters:** A handler that blocks indefinitely would hold a blocking thread and starve the executor for all subsequent plugin calls.
**Assertion:**
```rust
let start = Instant::now();
let result = registry.call("slow", json!({}), session).await;
assert!(start.elapsed() < Duration::from_secs(1), "call took too long: {:?}", start.elapsed());
assert!(matches!(result, Err(InvocationError::Timeout(_))));
```

---

## Session TTL eviction — session accessed within TTL is NOT evicted

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** Session state written and then accessed within the TTL window is still readable after one eviction tick fires.
**Why it matters:** Premature eviction destroys per-user plugin state mid-session, breaking any plugin that stores user-specific data across interactions.
**Assertion:**
```rust
// Write state, advance time by TTL/2, fire eviction tick, read state.
set_session_state(&runtime, &session_id, "key", json!("value")).await;
advance_mock_time(ttl / 2);
fire_eviction_tick(&runtime).await;
let val = read_session_state::<serde_json::Value>(&runtime, &session_id, "key");
assert!(val.is_some(), "session state evicted too early");
```

---

## Session TTL eviction — session not accessed beyond TTL IS evicted

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** Session state not accessed for longer than the TTL duration is gone after an eviction tick; a subsequent read returns `None`.
**Why it matters:** Without TTL eviction, the `session_states` map grows without bound and exhausts heap memory under sustained load.
**Assertion:**
```rust
set_session_state(&runtime, &session_id, "key", json!("value")).await;
advance_mock_time(ttl + Duration::from_secs(1));
fire_eviction_tick(&runtime).await;
let val = read_session_state::<serde_json::Value>(&runtime, &session_id, "key");
assert!(val.is_none(), "session state should have been evicted after TTL");
```

---

## JsonFilePersistence::save — atomic write leaves no temp file

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** After `save()` returns `Ok`, the target file exists and no file with a temporary suffix (e.g., `.tmp`) remains alongside it in the same directory.
**Why it matters:** A non-atomic write leaves a partial file if the process crashes between write and rename; on the next startup `load()` would read corrupted global state.
**Assertion:**
```rust
persistence.save(&plugin_id, &state).await.unwrap();
assert!(path.exists(), "target file must exist after save");
let temp_files: Vec<_> = std::fs::read_dir(path.parent().unwrap())
    .unwrap()
    .filter_map(|e| e.ok())
    .filter(|e| e.file_name().to_string_lossy().contains(".tmp"))
    .collect();
assert!(temp_files.is_empty(), "temp file remains after save: {:?}", temp_files);
```

---

## JsonFilePersistence::load — missing file returns Ok(None)

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** `load()` called with a path that does not exist returns `Ok(None)`, not an `Err`.
**Why it matters:** `load()` is called at startup; if it returns `Err` on a fresh install (no file yet), `build()` fails before any plugin loads.
**Assertion:**
```rust
let persistence = JsonFilePersistence { path: tmp_dir.join("nonexistent.json") };
let result = persistence.load(&PluginId("any/plugin".into())).await;
assert!(matches!(result, Ok(None)));
```

---

## OverridableComponent fast path — resolve_component not called when component absent from OverrideMap

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no
**What it tests:** `resolve_component` called with a name absent from `OverrideMap::overridden_components` returns `None` without performing any `TransformRegistry` lookup (confirmed by an atomic call counter on the registry path).
**Why it matters:** Every `#[overridable]` component boundary calls this function on every render; a registry lookup on the fast path would add a lock acquisition per component per render cycle.
**Assertion:**
```rust
// No plugins registered — overridden_components is empty.
let result = runtime
    .resolve_component("NotOverridden", json!({}), &session)
    .await
    .unwrap();
assert!(result.is_none());
// transform_registry_lookup_count must remain 0 (asserted via internal counter or mock).
assert_eq!(transform_registry_lookup_count.load(Ordering::SeqCst), 0);
```

---

## ClientCapabilities version check — min_app_version=99 with app_version=1 yields Incompatible without panic

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-high-app-version`)
**What it tests:** `render_slot` with `session.client.app_version = 1` against a plugin declaring `min_app_version = 99` returns `PluginView::Incompatible` for that plugin and does not panic.
**Why it matters:** An unhandled version mismatch that panics instead of returning `Incompatible` would crash the server on every request from any client older than the plugin requires.
**Assertion:**
```rust
let session = MockSession::new().with_app_version(1).as_ctx();
let contents = runtime.render_slot("test-slot", &session).await.unwrap();
assert_eq!(contents.len(), 1);
assert!(matches!(contents[0].view, PluginView::Incompatible { .. }));
// The fixture increments global state "call_count" if the WASM export runs.
// It must not have been called:
let call_count: u64 = runtime
    .global_state_json(&PluginId("test/high-app-version".into()), "call_count")
    .and_then(|v| v.as_u64())
    .unwrap_or(0);
assert_eq!(call_count, 0, "slot export must not be invoked when app version guard fires");
```

---

## on_load failure — build() returns Err; plugin not inserted into runtime

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-failing-on-load`)
**What it tests:** A plugin whose `on_load` export returns a non-empty error causes `build()` to return `Err`; no slot content is served for that plugin in any subsequent render call.
**Why it matters:** Allowing a partially-initialised plugin (on_load failed, pool partially created) into the runtime could cause undefined behaviour; the all-or-nothing contract on `build()` must hold.
**Assertion:**
```rust
let result = PluginRuntimeBuilder::new()
    .add_plugin(PluginSource::Bytes(FAILING_ON_LOAD_WASM.into()))
    .build()
    .await;
assert!(result.is_err(), "build() must fail when on_load returns an error");
```

---

## Security Tests

---

## Security 1: dx_invoke denied when capability not declared

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-cap-invoke-denied`)
**What it tests:** A plugin that calls `dx_invoke("add_note", …)` without declaring `HostCapability::Invoke { names: ["add_note"] }` in its manifest never reaches the registered handler; a counter on the handler stays at 0.
**Why it matters:** Capability enforcement is the primary security boundary between plugin code and host-side resources; a bypass allows any loaded plugin to call arbitrary registered invocations regardless of what the host installer granted.
**Assertion:**
```rust
// Register "add_note" handler with an AtomicU32 counter.
let counter = Arc::new(AtomicU32::new(0));
let c = counter.clone();
let runtime = PluginRuntimeBuilder::new()
    .add_plugin(PluginSource::Bytes(CAP_INVOKE_DENIED_WASM.into()))
    .register_invocation("add_note", None, move |_args: serde_json::Value, _session| {
        c.fetch_add(1, Ordering::SeqCst);
        async { Ok(json!({})) }
    })
    .build().await.unwrap();
// Trigger the slot export (which attempts dx_invoke("add_note", ...) internally):
let _ = runtime.render_slot("test-slot", &session).await;
assert_eq!(counter.load(Ordering::SeqCst), 0, "handler must not be called without Invoke capability");
```

---

## Security 2: dx_global_state_set denied when capability not declared

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-cap-global-write-denied`)
**What it tests:** A plugin that calls `dx_global_state_set("x", …)` without `HostCapability::GlobalStateWrite { keys: ["x"] }` cannot write to global state; the key `"x"` is absent for all plugins after the call.
**Why it matters:** Unguarded global state writes allow plugins to inject data visible to every user and every other plugin, enabling persistent cross-plugin data corruption or exfiltration.
**Assertion:**
```rust
// Trigger the slot export (which calls dx_global_state_set("x", ...) internally):
let _ = runtime.render_slot("test-slot", &session).await;
let value = runtime.global_state_json(
    &PluginId("test/cap-global-write-denied".into()), "x"
);
assert!(value.is_none(), "global state key 'x' must not exist after denied write");
```

---

## Security 3: dx_plugin_state_get denied for undeclared cross-plugin read

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-cap-plugin-state-read`, `fixture-cap-state-owner`)
**What it tests:** Plugin A calling `dx_plugin_state_get("test/cap-state-owner", "data")` without `HostCapability::ReadPluginState { plugin_id: "test/cap-state-owner", keys: ["data"] }` receives `None`/error from the host function; Plugin B's state value `"secret"` is unchanged and unreachable.
**Why it matters:** Cross-plugin state reads without authorisation allow a malicious plugin to exfiltrate another plugin's per-user data (session tokens, preferences, cached queries) without the owner's knowledge.
**Assertion:**
```rust
// fixture-cap-state-owner sets state "data" = "secret" in on_load.
// fixture-cap-plugin-state-read emits event "read_result" with whatever dx_plugin_state_get returned.
let events = runtime.emitted_events_for(&PluginId("test/cap-plugin-state-read".into()));
let read_result = events.iter().find(|e| e.name == "read_result")
    .map(|e| e.payload.clone())
    .unwrap_or(json!(null));
assert!(read_result.is_null() || read_result == json!(null),
    "cross-plugin read must return null/None to the plugin, got: {read_result}");
// Owner's state must be unchanged:
let owner_state = runtime.session_state_json(
    &PluginId("test/cap-state-owner".into()), &session.session_id, "data"
);
assert_eq!(owner_state, Some(json!("secret")));
```

---

## Security 4: SHA-256 integrity check rejects mismatched hash

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** no (test serves known WASM bytes via a local HTTP mock)
**What it tests:** `PluginSource::Url` with a `sha256` that has one byte flipped from the correct hash causes `build()` to return `PluginRuntimeError::ChecksumMismatch` containing the original URL; the plugin is not inserted.
**Why it matters:** Without SHA-256 verification a compromised CDN or redirected URL could silently swap a plugin binary, executing arbitrary code inside the WASM sandbox with the host's full capability grants.
**Assertion:**
```rust
let mut bad_hash = correct_sha256;
bad_hash[0] ^= 0xff;
let result = PluginRuntimeBuilder::new()
    .add_plugin(PluginSource::Url { url: served_url.clone(), sha256: bad_hash })
    .build()
    .await;
assert!(
    matches!(&result, Err(PluginRuntimeError::ChecksumMismatch { url, .. }) if url == &served_url),
    "expected ChecksumMismatch, got: {:?}", result
);
```

---

## Security 5: Protocol version guard at build time rejects future-version plugin

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-high-protocol-version`)
**What it tests:** A plugin whose `manifest()` export returns `min_protocol_version = PROTOCOL_VERSION + 1` causes `build()` to return `PluginRuntimeError::ProtocolIncompatible { required: PROTOCOL_VERSION + 1, supported: PROTOCOL_VERSION, .. }`; no plugin is inserted.
**Why it matters:** Loading a plugin that requires a newer protocol than the host understands would cause undefined deserialization at render time; rejecting at build time is the only safe option.
**Assertion:**
```rust
let result = PluginRuntimeBuilder::new()
    .add_plugin(PluginSource::Bytes(HIGH_PROTOCOL_WASM.into()))
    .build()
    .await;
assert!(
    matches!(&result,
        Err(PluginRuntimeError::ProtocolIncompatible { required, supported, .. })
        if *required == PROTOCOL_VERSION + 1 && *supported == PROTOCOL_VERSION
    ),
    "expected ProtocolIncompatible, got: {:?}", result
);
```

---

## Security 6: App version guard fires before pool call; WASM export never invoked

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-high-app-version`)
**What it tests:** `render_slot` with `client.app_version = 1` against a plugin declaring `min_app_version = 5` produces `PluginView::Incompatible`; a call counter embedded in the plugin's slot export proves the WASM code was never entered.
**Why it matters:** The guard must fire before `pool.get()`, not after; invoking the WASM and discarding the result still burns fuel, consumes a pool instance, and may trigger state side-effects in the plugin.
**Assertion:**
```rust
let session = MockSession::new().with_app_version(1).as_ctx();
let contents = runtime.render_slot("test-slot", &session).await.unwrap();
assert!(matches!(contents[0].view, PluginView::Incompatible { .. }));
let call_count = runtime
    .global_state_json(&PluginId("test/high-app-version".into()), "call_count")
    .and_then(|v| v.as_u64())
    .unwrap_or(0);
assert_eq!(call_count, 0, "WASM slot export must not be entered when app version guard fires");
```

---

## Security 7: Wall-clock timeout — render_slot returns within 1 second; contribution is Incompatible

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-blocking-slot`)
**What it tests:** With `max_call_duration = 200 ms`, `render_slot` against a plugin whose slot export busy-loops for 30 seconds returns within 1 second wall-clock time; the blocked plugin's contribution is `PluginView::Incompatible`.
**Why it matters:** A WASM busy-loop that is never cancelled would pin a blocking thread indefinitely, starving the pool and making the server unresponsive to all subsequent requests.
**Assertion:**
```rust
let start = Instant::now();
let contents = runtime.render_slot("test-slot", &session).await.unwrap();
assert!(
    start.elapsed() < Duration::from_secs(1),
    "render_slot took {:?}, expected < 1s", start.elapsed()
);
assert!(
    matches!(contents[0].view, PluginView::Incompatible { .. }),
    "timed-out plugin must produce Incompatible, got {:?}", contents[0].view
);
```

---

## Security 8: Capability isolation between pool instances — four-way assertion

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-cap-invoke-a`, `fixture-cap-invoke-b`)
**What it tests:** Plugin A (granted `Invoke(["get_notes"])`) cannot call `"add_note"` and Plugin B (granted `Invoke(["add_note"])`) cannot call `"get_notes"`; the reverse (each calling their granted name) succeeds. All four cases confirmed via handler counters in a single test.
**Why it matters:** A shared `UserData` or misrouted `SessionCtx` that leaks capabilities between pool instances of different plugins would allow one plugin to impersonate another's invocation rights.
**Assertion:**
```rust
// After triggering both plugins' slot exports:
assert_eq!(get_notes_granted_counter.load(Ordering::SeqCst),  1, "A→get_notes must succeed");
assert_eq!(add_note_granted_counter.load(Ordering::SeqCst),   1, "B→add_note must succeed");
assert_eq!(add_note_denied_counter.load(Ordering::SeqCst),    0, "A→add_note must be denied");
assert_eq!(get_notes_denied_counter.load(Ordering::SeqCst),   0, "B→get_notes must be denied");
```

---

## Security 9: Deadlock liveness under 20 concurrent slot renders

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-slot-normal`)
**What it tests:** 20 tokio tasks each calling `render_slot` with a distinct `SessionId` on a loaded runtime all complete within 30 seconds; no task panics; all results are `Ok`.
**Why it matters:** The lock acquisition order (plugins → registries → session_states → global_states) must be strictly consistent; any inversion would deadlock under concurrent load and freeze the server.
**Assertion:**
```rust
let handles: Vec<_> = (0..20).map(|i| {
    let rt = runtime.clone();
    let s = SessionCtx { session_id: SessionId(i.to_string()), ..base_session.clone() };
    tokio::spawn(async move { rt.render_slot("test-slot", &s).await })
}).collect();
let results = tokio::time::timeout(
    Duration::from_secs(30),
    futures::future::join_all(handles),
).await.expect("deadlock detected: 20 concurrent render_slot calls did not complete in 30s");
for (i, r) in results.into_iter().enumerate() {
    assert!(r.expect(&format!("task {i} panicked")).is_ok(), "task {i} returned Err");
}
```

---

## Security 10: on_load failure aborts build and leaves no corrupted global state

**Crate:** `dioxus-extism-host`
**Requires WASM fixture:** yes (`fixture-failing-on-load`, `fixture-slot-normal`)
**What it tests:** A `build()` that fails due to `on_load` returning an error does not corrupt any global state; a subsequent `build()` without the failing plugin succeeds and serves slot content normally.
**Why it matters:** If a failed build leaks a partially-initialised `Arc` or poisons a `Mutex`, all future builds and runtime operations on the same process are unpredictable.
**Assertion:**
```rust
let bad = PluginRuntimeBuilder::new()
    .add_plugin(PluginSource::Bytes(FAILING_ON_LOAD_WASM.into()))
    .build().await;
assert!(bad.is_err(), "first build must fail");

let good = PluginRuntimeBuilder::new()
    .add_plugin(PluginSource::Bytes(NORMAL_WASM.into()))
    .build().await
    .expect("second build must succeed — no global state was corrupted");
let slots = good.render_slot("test-slot", &session).await.unwrap();
assert_eq!(slots.len(), 1, "normal plugin must serve slot content after failed-build recovery");
```

---

## Fixture Crates Needed

---

### `fixture-slot-normal`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/slot-normal"`, registers `"test-slot"` at `PriorityHint::Normal`, `min_protocol_version = PROTOCOL_VERSION`, `min_app_version = 0`
- `slot_test_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — returns `PluginView::Text("normal")`
- `on_unload() -> FnResult<()>` — increments global state key `"unload_count"` via `dx_global_state_set`

**Used by:** render_slot priority ordering · render_slot disabled plugin · render_slot min_protocol_version exceeds client · render_slot slot not in registry · reload_plugin version +1 · reload_plugin on_unload called · unload_plugin slot disappears · enable/disable toggle · PluginInstallConfig tie-breaking · Security 9 (deadlock liveness) · Security 10 (clean build after failure)

---

### `fixture-slot-high`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/slot-high"`, registers `"test-slot"` at `PriorityHint::High`
- `slot_test_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — returns `PluginView::Text("high")`

**Used by:** render_slot priority ordering · PluginInstallConfig tie-breaking (as the second plugin, forced to equal priority)

---

### `fixture-slot-failing`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/slot-failing"`, registers `"test-slot"` at `PriorityHint::Normal`
- `slot_test_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — always returns `Err(extism_pdk::Error::msg("slot error"))`

**Used by:** render_slot CallFailed plugin contributes Incompatible

---

### `fixture-high-app-version`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/high-app-version"`, registers `"test-slot"`, `min_app_version = 99`
- `slot_test_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — increments global state `"call_count"` via `dx_global_state_set`, then returns `PluginView::Text("high-app")`

**Used by:** ClientCapabilities version check correctness · Security 6 (app version guard fires before WASM call)

---

### `fixture-high-protocol-version`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/high-protocol-version"`, `min_protocol_version = 2` (i.e., `PROTOCOL_VERSION + 1`); no slot registration needed — the plugin is expected to be rejected at build time

**Used by:** Security 5 (protocol version guard at build time)

---

### `fixture-hook-continue`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/hook-continue"`, registers `"test-hook"` at `PriorityHint::First`
- `hook_test_hook(Json<HookCall>) -> FnResult<Json<HookResult>>` — returns `HookResult::Continue { context: json!("continued") }`

**Used by:** run_hook Continue→Replace→Cancel chain · run_hook plugin Err in middle (as the first, succeeding plugin)

---

### `fixture-hook-replace`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/hook-replace"`, registers `"test-hook"` at `PriorityHint::Normal`
- `hook_test_hook(Json<HookCall>) -> FnResult<Json<HookResult>>` — returns `HookResult::Replace { context: json!("replaced") }`

**Used by:** run_hook Continue→Replace→Cancel chain

---

### `fixture-hook-cancel`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/hook-cancel"`, registers `"test-hook"` at `PriorityHint::Last`
- `hook_test_hook(Json<HookCall>) -> FnResult<Json<HookResult>>` — returns `HookResult::Cancel { reason: "test-cancel".into() }`

**Used by:** run_hook Continue→Replace→Cancel chain (as the cancelling plugin)

---

### `fixture-hook-after-cancel`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/hook-after-cancel"`, registers `"test-hook"` at `PriorityHint::Last` with an overridden numeric priority below `fixture-hook-cancel` (e.g., priority −1 set via `PluginInstallConfig`)
- `hook_test_hook(Json<HookCall>) -> FnResult<Json<HookResult>>` — increments global state `"after_cancel_count"` then returns `HookResult::Continue { context: input.context }`

**Used by:** run_hook Continue→Replace→Cancel chain (confirms chain stops before this plugin)

---

### `fixture-hook-erroring`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/hook-erroring"`, registers `"test-hook"` at `PriorityHint::High` (between First and Normal)
- `hook_test_hook(Json<HookCall>) -> FnResult<Json<HookResult>>` — always returns `Err(extism_pdk::Error::msg("hook error"))`

**Used by:** run_hook plugin Err in middle of chain

---

### `fixture-wrap-a`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/wrap-a"`, declares `TransformDeclaration { selector: Selector::Route(RoutePattern("/test/:id")), transform_fn: "transform_wrap_route", op: TransformOp::Wrap, priority_hint: PriorityHint::High }`
- `transform_wrap_route(Json<TransformInput>) -> FnResult<Json<TransformOutput>>` — records `input.original` in global state `"received_original"` via `dx_global_state_set`; returns a `PluginView::Element` containing `PluginView::Text("marker-a")` and `original_content()`

**Used by:** render_route_transforms Wrap fold · render_route_transforms Wrap plugin call fails (as the surviving high-priority plugin)

---

### `fixture-wrap-b`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/wrap-b"`, same route pattern as `fixture-wrap-a`, `op: TransformOp::Wrap`, `priority_hint: PriorityHint::Low`
- `transform_wrap_route(Json<TransformInput>) -> FnResult<Json<TransformOutput>>` — returns a `PluginView::Element` containing `original_content()` and `PluginView::Text("marker-b")`

**Used by:** render_route_transforms Wrap fold · render_route_transforms Wrap plugin call fails (as the surviving low-priority plugin)

---

### `fixture-wrap-failing`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/wrap-failing"`, same route pattern, `op: TransformOp::Wrap`, `priority_hint: PriorityHint::Normal` (between High and Low)
- `transform_wrap_route(Json<TransformInput>) -> FnResult<Json<TransformOutput>>` — always returns `Err(extism_pdk::Error::msg("wrap error"))`

**Used by:** render_route_transforms Wrap plugin call fails

---

### `fixture-wrap-no-content`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/wrap-no-content"`, same route pattern, `op: TransformOp::Wrap`, `priority_hint: PriorityHint::High`
- `transform_wrap_route(Json<TransformInput>) -> FnResult<Json<TransformOutput>>` — returns `PluginView::Text("no-content")` with no `HostComponent("__content__")` anywhere in the tree

**Used by:** render_route_transforms Wrap plugin output omits `__content__` (tracing::warn)

---

### `fixture-within-selector`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/within-selector"`, declares four `Within` transforms for `Selector::Slot("test-slot")` with inner selectors:
  1. `NodeSelector::Recursive(Box::new(HasClass("target")))` → `transform_fn: "transform_recursive"`
  2. `NodeSelector::HasClass("shallow-target")` → `transform_fn: "transform_shallow"`
  3. `NodeSelector::And(Box::new(HasClass("a")), Box::new(HasClass("b")))` → `transform_fn: "transform_and"`
  4. `NodeSelector::Or(Box::new(HasClass("c")), Box::new(HasClass("d")))` → `transform_fn: "transform_or"`
- `transform_recursive(Json<TransformInput>) -> FnResult<Json<TransformOutput>>` — returns `PluginView::Text("TRANSFORMED-RECURSIVE")`
- `transform_shallow(Json<TransformInput>) -> FnResult<Json<TransformOutput>>` — returns `PluginView::Text("TRANSFORMED-SHALLOW")`
- `transform_and(Json<TransformInput>) -> FnResult<Json<TransformOutput>>` — returns `PluginView::Text("TRANSFORMED-AND")`
- `transform_or(Json<TransformInput>) -> FnResult<Json<TransformOutput>>` — returns `PluginView::Text("TRANSFORMED-OR")`

**Used by:** apply_tree_transforms Recursive depth-3 · apply_tree_transforms shallow no-descend · apply_tree_transforms And · apply_tree_transforms Or

---

### `fixture-failing-on-load`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/failing-on-load"`, no slot registrations
- `on_load(Json<SessionCtx>) -> FnResult<()>` — always returns `Err(extism_pdk::Error::msg("on_load failed intentionally"))`

**Used by:** on_load failure correctness · Security 10 (on_load failure leaves runtime clean)

---

### `fixture-cap-invoke-denied`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/cap-invoke-denied"`, registers `"test-slot"`, `host_capabilities: vec![]` (no Invoke capability declared)
- `slot_test_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — calls `dx_invoke("add_note", json!({}))` (will be denied); returns `PluginView::Text("attempted")` regardless of the result

**Used by:** Security 1 (dx_invoke denied when capability not declared)

---

### `fixture-cap-global-write-denied`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/cap-global-write-denied"`, registers `"test-slot"`, no `GlobalStateWrite` capability declared
- `slot_test_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — calls `dx_global_state_set("x", json!("malicious"))`; returns `PluginView::Text("attempted")`

**Used by:** Security 2 (dx_global_state_set denied)

---

### `fixture-cap-plugin-state-read`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/cap-plugin-state-read"`, registers `"test-slot"`, no `ReadPluginState` capability declared
- `slot_test_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — calls `dx_plugin_state_get("test/cap-state-owner", "data")`; emits event `"read_result"` with the returned value (or `json!(null)` if `None`); returns `PluginView::Text("attempted")`

**Used by:** Security 3 (cross-plugin state read denied)

---

### `fixture-cap-state-owner`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/cap-state-owner"`, registers `"other-slot"`
- `on_load(Json<SessionCtx>) -> FnResult<()>` — calls `dx_state_set("data", json!("secret"))` to initialise the state the attacker plugin tries to read
- `slot_other_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — returns `PluginView::Text("owner")`

**Used by:** Security 3 (as Plugin B whose state must not be readable by Plugin A)

---

### `fixture-cap-invoke-a`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/cap-invoke-a"`, registers `"slot-a"`, declares `HostCapability::Invoke { names: ["get_notes"] }` (only `get_notes` granted; `add_note` not declared)
- `slot_slot_a(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — calls `dx_invoke("get_notes", json!({}))` (granted; should succeed) and `dx_invoke("add_note", json!({}))` (not granted; should be denied); returns `PluginView::Text("a")`

**Used by:** Security 8 (capability isolation between pool instances)

---

### `fixture-cap-invoke-b`

**Target:** `wasm32-unknown-unknown`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/cap-invoke-b"`, registers `"slot-b"`, declares `HostCapability::Invoke { names: ["add_note"] }` (only `add_note` granted; `get_notes` not declared)
- `slot_slot_b(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — calls `dx_invoke("add_note", json!({}))` (granted) and `dx_invoke("get_notes", json!({}))` (not granted; should be denied); returns `PluginView::Text("b")`

**Used by:** Security 8 (capability isolation between pool instances)

---

### `fixture-blocking-slot`

**Target:** `wasm32-wasip1`
**Required exports:**
- `manifest() -> Json<PluginManifest>` — id `"test/blocking-slot"`, registers `"test-slot"` at `PriorityHint::Normal`
- `slot_test_slot(Json<SessionCtx>) -> FnResult<Json<PluginView>>` — busy-loops indefinitely: `loop { std::hint::spin_loop(); }` (or `std::thread::sleep(Duration::from_secs(30))` if WASI sleep is available)

**Used by:** Security 7 (wall-clock timeout)
