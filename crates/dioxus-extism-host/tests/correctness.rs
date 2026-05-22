/// Phase D integration tests — require compiled WASM fixtures.
///
/// Build fixtures before running:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-slot-normal -p fixture-slot-high -p fixture-slot-failing \
///     -p fixture-high-app-version -p fixture-high-protocol-version \
///     -p fixture-hook-continue -p fixture-hook-replace -p fixture-hook-cancel \
///     -p fixture-hook-after-cancel -p fixture-hook-erroring \
///     -p fixture-wrap-a -p fixture-wrap-b -p fixture-wrap-failing \
///     -p fixture-wrap-no-content -p fixture-within-selector \
///     -p fixture-failing-on-load
use std::sync::Arc;

use dioxus_extism_host::{HookOutcome, PluginInstallConfig, PluginRuntimeBuilder, PluginSource};
use dioxus_extism_protocol::{
    ClientCapabilities, PluginId, PluginView, Selector, SessionCtx, SessionId,
    TransformContext, ViewElement, PROTOCOL_VERSION,
};

// ── WASM bytes ─────────────────────────────────────────────────────────────────

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(SLOT_NORMAL_WASM, "fixture_slot_normal.wasm");
fixture!(SLOT_HIGH_WASM, "fixture_slot_high.wasm");
fixture!(SLOT_FAILING_WASM, "fixture_slot_failing.wasm");
fixture!(HIGH_APP_VERSION_WASM, "fixture_high_app_version.wasm");
fixture!(HIGH_PROTOCOL_VERSION_WASM, "fixture_high_protocol_version.wasm");
fixture!(HOOK_CONTINUE_WASM, "fixture_hook_continue.wasm");
fixture!(HOOK_REPLACE_WASM, "fixture_hook_replace.wasm");
fixture!(HOOK_CANCEL_WASM, "fixture_hook_cancel.wasm");
fixture!(HOOK_AFTER_CANCEL_WASM, "fixture_hook_after_cancel.wasm");
fixture!(HOOK_ERRORING_WASM, "fixture_hook_erroring.wasm");
fixture!(WRAP_A_WASM, "fixture_wrap_a.wasm");
fixture!(WRAP_B_WASM, "fixture_wrap_b.wasm");
fixture!(WRAP_FAILING_WASM, "fixture_wrap_failing.wasm");
fixture!(WRAP_NO_CONTENT_WASM, "fixture_wrap_no_content.wasm");
fixture!(WITHIN_SELECTOR_WASM, "fixture_within_selector.wasm");
fixture!(FAILING_ON_LOAD_WASM, "fixture_failing_on_load.wasm");

// ── Helpers ────────────────────────────────────────────────────────────────────

fn session_with_protocol(protocol_version: u32) -> SessionCtx {
    SessionCtx {
        session_id: SessionId("test".into()),
        user_id: None,
        client: ClientCapabilities {
            protocol_version,
            app_version: 0,
            registered_host_components: vec![],
        },
        caller: None,
    }
}

fn session_with_app_version(app_version: u32) -> SessionCtx {
    SessionCtx {
        session_id: SessionId("test".into()),
        user_id: None,
        client: ClientCapabilities {
            protocol_version: PROTOCOL_VERSION,
            app_version,
            registered_host_components: vec![],
        },
        caller: None,
    }
}

fn default_session() -> SessionCtx {
    session_with_protocol(PROTOCOL_VERSION)
}

fn view_contains_text(view: &PluginView, needle: &str) -> bool {
    match view {
        PluginView::Text(t) => t.contains(needle),
        PluginView::Element(e) => e.children.iter().any(|c| view_contains_text(c, needle)),
        PluginView::Fragment(children) => children.iter().any(|c| view_contains_text(c, needle)),
        _ => false,
    }
}

fn make_nested_tree(class: &str, depth: usize) -> PluginView {
    if depth == 0 {
        PluginView::Element(ViewElement {
            tag: "span".into(),
            attrs: vec![("class".into(), dioxus_extism_protocol::AttrValue::String(class.into()))],
            ..Default::default()
        })
    } else {
        PluginView::Element(ViewElement {
            tag: "div".into(),
            children: vec![make_nested_tree(class, depth - 1)],
            ..Default::default()
        })
    }
}

// ── render_slot tests ─────────────────────────────────────────────────────────

#[tokio::test]
async fn render_slot_contributions_ordered_priority_descending() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_HIGH_WASM.into()))
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let contents = runtime
        .render_slot("test-slot", &default_session())
        .await
        .expect("render_slot failed");

    assert_eq!(contents.len(), 2, "both plugins must contribute");
    assert!(
        contents[0].priority > contents[1].priority,
        "high-priority first"
    );
    assert_eq!(contents[0].plugin_id, PluginId("test/slot-high".into()));
    assert_eq!(contents[1].plugin_id, PluginId("test/slot-normal".into()));
}

#[tokio::test]
async fn render_slot_call_failed_plugin_incompatible_others_unaffected() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_FAILING_WASM.into()))
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let contents = runtime
        .render_slot("test-slot", &default_session())
        .await
        .expect("render_slot failed");

    let failing = contents
        .iter()
        .find(|c| c.plugin_id == PluginId("test/slot-failing".into()))
        .expect("failing plugin must contribute");
    let normal = contents
        .iter()
        .find(|c| c.plugin_id == PluginId("test/slot-normal".into()))
        .expect("normal plugin must contribute");

    assert!(
        matches!(failing.view, PluginView::Incompatible { .. }),
        "failing plugin must produce Incompatible"
    );
    assert!(
        !matches!(normal.view, PluginView::Incompatible { .. }),
        "normal plugin must not produce Incompatible"
    );
}

#[tokio::test]
async fn render_slot_disabled_plugin_contributes_incompatible_not_gap() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/slot-normal".into());
    runtime.disable_plugin(&id).await.expect("disable_plugin failed");

    let contents = runtime
        .render_slot("test-slot", &default_session())
        .await
        .expect("render_slot failed");

    assert_eq!(contents.len(), 1, "disabled plugin still contributes (as Incompatible)");
    assert!(
        matches!(contents[0].view, PluginView::Incompatible { .. }),
        "disabled plugin produces Incompatible"
    );
}

#[tokio::test]
async fn render_slot_min_protocol_version_exceeds_client_yields_incompatible() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let old_session = session_with_protocol(0);
    let contents = runtime
        .render_slot("test-slot", &old_session)
        .await
        .expect("render_slot failed");

    assert_eq!(contents.len(), 1);
    assert!(
        matches!(contents[0].view, PluginView::Incompatible { .. }),
        "old client must receive Incompatible for min_protocol_version check"
    );
}

#[tokio::test]
async fn render_slot_slot_name_not_in_registry_returns_empty() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let contents = runtime
        .render_slot("unregistered-slot", &default_session())
        .await
        .expect("render_slot on unregistered slot must succeed");

    assert!(contents.is_empty(), "unregistered slot must return empty");
}

// ── run_hook tests ────────────────────────────────────────────────────────────

#[tokio::test]
async fn run_hook_continue_replace_cancel_chain_stops_at_cancel() {
    let after_cancel_cfg = PluginInstallConfig {
        overrides: [("test-hook".into(), -1)].into_iter().collect(),
        ..Default::default()
    };
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HOOK_CONTINUE_WASM.into()))
        .add_plugin(PluginSource::Bytes(HOOK_REPLACE_WASM.into()))
        .add_plugin(PluginSource::Bytes(HOOK_CANCEL_WASM.into()))
        .add_plugin_with_config(
            PluginSource::Bytes(HOOK_AFTER_CANCEL_WASM.into()),
            after_cancel_cfg,
        )
        .build()
        .await
        .expect("build failed");

    let outcome = runtime
        .run_hook("test-hook", serde_json::json!({"x": 1}), &default_session())
        .await
        .expect("run_hook failed");

    assert!(
        matches!(&outcome,
            HookOutcome::Cancelled { by, reason }
            if *by == PluginId("test/hook-cancel".into()) && reason == "test-cancel"
        ),
        "expected Cancelled by hook-cancel, got {outcome:?}"
    );

    let count = runtime
        .global_state_json(
            &PluginId("test/hook-after-cancel".into()),
            "after_cancel_count",
        )
        .await
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(count, 0, "plugin after cancel must never be invoked");
}

#[tokio::test]
async fn run_hook_plugin_err_in_middle_does_not_abort_chain() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HOOK_CONTINUE_WASM.into()))
        .add_plugin(PluginSource::Bytes(HOOK_ERRORING_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let outcome = runtime
        .run_hook("test-hook", serde_json::json!("initial"), &default_session())
        .await
        .expect("run_hook failed");

    assert!(
        matches!(&outcome, HookOutcome::Passed(ctx) if *ctx == serde_json::json!("continued")),
        "expected Passed with 'continued' from fixture-hook-continue, got {outcome:?}"
    );
}

// ── render_route_transforms tests ────────────────────────────────────────────

#[tokio::test]
async fn render_route_transforms_wrap_fold_sequential_pipeline() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(WRAP_A_WASM.into()))
        .add_plugin(PluginSource::Bytes(WRAP_B_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let result = runtime
        .render_route_transforms("/test/42", &default_session())
        .await
        .expect("render_route_transforms failed");

    let wrap = result.wrap.expect("wrap must be present");

    assert!(view_contains_text(&wrap, "marker-a"), "marker-a must be in wrap output");
    assert!(view_contains_text(&wrap, "marker-b"), "marker-b must be in wrap output");

    let received_a = runtime
        .global_state_json(&PluginId("test/wrap-a".into()), "received_original")
        .await
        .expect("wrap-a must have stored received_original");
    assert_eq!(
        received_a["HostComponent"]["name"],
        "__content__",
        "wrap-a must receive __content__ seed as original"
    );
}

#[tokio::test]
async fn render_route_transforms_wrap_plugin_fail_passes_through_unchanged() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(WRAP_A_WASM.into()))
        .add_plugin(PluginSource::Bytes(WRAP_FAILING_WASM.into()))
        .add_plugin(PluginSource::Bytes(WRAP_B_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let result = runtime
        .render_route_transforms("/test/42", &default_session())
        .await
        .expect("render_route_transforms failed");

    let wrap = result.wrap.expect("wrap must survive failing plugin");

    assert!(view_contains_text(&wrap, "marker-a"), "marker-a must survive failing wrap");
    assert!(view_contains_text(&wrap, "marker-b"), "marker-b must survive failing wrap");
    assert!(
        !view_contains_text(&wrap, "marker-failing"),
        "failing plugin must not contribute"
    );
}

#[tokio::test]
async fn render_route_transforms_unmatched_path_returns_empty() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(WRAP_A_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let result = runtime
        .render_route_transforms("/other/path", &default_session())
        .await
        .expect("render_route_transforms failed");

    assert!(result.is_empty(), "unmatched path must return empty RouteTransforms");
}

// ── apply_tree_transforms tests ───────────────────────────────────────────────

#[tokio::test]
async fn apply_tree_transforms_recursive_finds_node_at_depth_3() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(WITHIN_SELECTOR_WASM.into()))
        .build()
        .await
        .expect("build failed");

    // depth=2 means: div(div(span.target))
    let tree = make_nested_tree("target", 2);
    let context = TransformContext::default();

    let result = runtime
        .apply_tree_transforms(
            &Selector::Slot("test-slot".into()),
            tree,
            context,
            &default_session(),
        )
        .await
        .expect("apply_tree_transforms failed");

    assert!(
        view_contains_text(&result, "TRANSFORMED-RECURSIVE"),
        "depth-3 node not reached by Recursive selector"
    );
}

#[tokio::test]
async fn apply_tree_transforms_shallow_does_not_descend_past_direct_children() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(WITHIN_SELECTOR_WASM.into()))
        .build()
        .await
        .expect("build failed");

    // span.shallow-target at depth 1 inside a div: div(span.shallow-target)
    // The shallow selector tests direct children of the outer node.
    // In apply_tree_transforms, the outer = Selector::Slot("test-slot").
    // The within_for_outer lookup finds the shallow selector.
    // traverse_and_apply with NodeSelector::HasClass("shallow-target") without Recursive
    // should only check direct children of the root node.
    // A tree where the shallow-target is at depth 2 (div(div(span.shallow-target)))
    // should NOT be transformed.
    let tree = PluginView::Element(ViewElement {
        tag: "div".into(),
        children: vec![PluginView::Element(ViewElement {
            tag: "div".into(),
            children: vec![PluginView::Element(ViewElement {
                tag: "span".into(),
                attrs: vec![(
                    "class".into(),
                    dioxus_extism_protocol::AttrValue::String("shallow-target".into()),
                )],
                ..Default::default()
            })],
            ..Default::default()
        })],
        ..Default::default()
    });
    let expected = tree.clone();
    let context = TransformContext::default();

    let result = runtime
        .apply_tree_transforms(
            &Selector::Slot("test-slot".into()),
            tree,
            context,
            &default_session(),
        )
        .await
        .expect("apply_tree_transforms failed");

    assert_eq!(result, expected, "shallow selector must leave depth-2 node unchanged");
}

#[tokio::test]
async fn apply_tree_transforms_and_requires_both_conditions() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(WITHIN_SELECTOR_WASM.into()))
        .build()
        .await
        .expect("build failed");

    fn span_with_class(class: &str) -> PluginView {
        PluginView::Element(ViewElement {
            tag: "span".into(),
            attrs: vec![(
                "class".into(),
                dioxus_extism_protocol::AttrValue::String(class.into()),
            )],
            ..Default::default()
        })
    }

    // Direct children: span.a.b, span.a (only a), span.b (only b).
    // Only span.a.b should match And(HasClass("a"), HasClass("b")).
    let tree = PluginView::Element(ViewElement {
        tag: "div".into(),
        children: vec![
            span_with_class("a b"),
            span_with_class("a"),
            span_with_class("b"),
        ],
        ..Default::default()
    });
    let context = TransformContext::default();

    let result = runtime
        .apply_tree_transforms(
            &Selector::Slot("test-slot".into()),
            tree,
            context,
            &default_session(),
        )
        .await
        .expect("apply_tree_transforms failed");

    fn count_text_nodes(view: &PluginView, needle: &str) -> usize {
        match view {
            PluginView::Text(t) if t.contains(needle) => 1,
            PluginView::Element(e) => {
                e.children.iter().map(|c| count_text_nodes(c, needle)).sum()
            }
            PluginView::Fragment(children) => {
                children.iter().map(|c| count_text_nodes(c, needle)).sum()
            }
            _ => 0,
        }
    }

    assert_eq!(
        count_text_nodes(&result, "TRANSFORMED-AND"),
        1,
        "only the span with both classes must be transformed"
    );
}

#[tokio::test]
async fn apply_tree_transforms_or_matches_either_condition() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(WITHIN_SELECTOR_WASM.into()))
        .build()
        .await
        .expect("build failed");

    fn span_with_class(class: &str) -> PluginView {
        PluginView::Element(ViewElement {
            tag: "span".into(),
            attrs: vec![(
                "class".into(),
                dioxus_extism_protocol::AttrValue::String(class.into()),
            )],
            ..Default::default()
        })
    }

    // Direct children: span.c, span.d, span.c.d, span.other.
    // Three nodes (c, d, c.d) should match Or(HasClass("c"), HasClass("d")).
    let tree = PluginView::Element(ViewElement {
        tag: "div".into(),
        children: vec![
            span_with_class("c"),
            span_with_class("d"),
            span_with_class("c d"),
            span_with_class("other"),
        ],
        ..Default::default()
    });
    let context = TransformContext::default();

    let result = runtime
        .apply_tree_transforms(
            &Selector::Slot("test-slot".into()),
            tree,
            context,
            &default_session(),
        )
        .await
        .expect("apply_tree_transforms failed");

    fn count_text_nodes(view: &PluginView, needle: &str) -> usize {
        match view {
            PluginView::Text(t) if t.contains(needle) => 1,
            PluginView::Element(e) => {
                e.children.iter().map(|c| count_text_nodes(c, needle)).sum()
            }
            PluginView::Fragment(children) => {
                children.iter().map(|c| count_text_nodes(c, needle)).sum()
            }
            _ => 0,
        }
    }

    assert_eq!(
        count_text_nodes(&result, "TRANSFORMED-OR"),
        3,
        "nodes with class c, d, or both must be transformed; 'other' must not"
    );
}

// ── Reload / unload tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn reload_plugin_override_map_version_increments_by_exactly_one() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/slot-normal".into());
    let before = runtime.override_map().await.version;

    let config = PluginInstallConfig::default();
    runtime
        .reload_plugin(&id, PluginSource::Bytes(SLOT_NORMAL_WASM.into()), config)
        .await
        .expect("reload_plugin failed");

    let after = runtime.override_map().await.version;
    assert_eq!(after, before + 1, "version must increment by exactly 1 on reload");
}

#[tokio::test]
async fn reload_plugin_on_unload_called_on_old_pool() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/slot-normal".into());

    let before_count = runtime
        .global_state_json(&id, "unload_count")
        .await
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    runtime
        .reload_plugin(&id, PluginSource::Bytes(SLOT_NORMAL_WASM.into()), PluginInstallConfig::default())
        .await
        .expect("reload_plugin failed");

    let after_count = runtime
        .global_state_json(&id, "unload_count")
        .await
        .and_then(|v| v.as_u64())
        .expect("unload_count must be set after reload");

    assert_eq!(after_count, before_count + 1, "on_unload must increment unload_count by 1");
}

#[tokio::test]
async fn unload_plugin_slot_disappears_from_render_slot() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/slot-normal".into());
    let session = default_session();

    let before = runtime.render_slot("test-slot", &session).await.expect("render_slot");
    assert_eq!(before.len(), 1, "plugin must be present before unload");

    runtime.unload_plugin(&id).await.expect("unload_plugin failed");

    let after = runtime.render_slot("test-slot", &session).await.expect("render_slot after unload");
    assert!(after.is_empty(), "slot must be empty after unload");
}

#[tokio::test]
async fn enable_disable_toggle_concurrent_render_no_panic() {
    let runtime = Arc::new(
        PluginRuntimeBuilder::new()
            .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
            .build()
            .await
            .expect("build failed"),
    );

    let id = PluginId("test/slot-normal".into());

    let handles: Vec<_> = (0..10_u32)
        .map(|i| {
            let rt = Arc::clone(&runtime);
            let s = SessionCtx {
                session_id: SessionId(i.to_string()),
                user_id: None,
                client: ClientCapabilities {
                    protocol_version: PROTOCOL_VERSION,
                    app_version: 0,
                    registered_host_components: vec![],
                },
                caller: None,
            };
            tokio::spawn(async move { rt.render_slot("test-slot", &s).await })
        })
        .collect();

    runtime.disable_plugin(&id).await.expect("disable_plugin failed");

    let results = futures::future::join_all(handles).await;
    for r in results {
        let contents = r.expect("task panicked").expect("render_slot failed");
        for c in &contents {
            assert!(
                matches!(c.view, PluginView::Text(_) | PluginView::Element(_) | PluginView::Incompatible { .. }),
                "unexpected view variant: {:?}",
                c.view
            );
        }
    }
}

// ── on_load failure test ──────────────────────────────────────────────────────

#[tokio::test]
async fn on_load_failure_build_returns_err_plugin_not_inserted() {
    let result = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(FAILING_ON_LOAD_WASM.into()))
        .build()
        .await;

    assert!(result.is_err(), "build() must fail when on_load returns an error");
}

// ── ClientCapabilities app version check ─────────────────────────────────────

#[tokio::test]
async fn client_capabilities_high_app_version_yields_incompatible() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HIGH_APP_VERSION_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let session = session_with_app_version(1);
    let contents = runtime
        .render_slot("test-slot", &session)
        .await
        .expect("render_slot failed");

    assert_eq!(contents.len(), 1);
    assert!(
        matches!(contents[0].view, PluginView::Incompatible { .. }),
        "min_app_version=99 plugin must produce Incompatible for app_version=1 client"
    );

    let call_count = runtime
        .global_state_json(&PluginId("test/high-app-version".into()), "call_count")
        .await
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(
        call_count,
        0,
        "slot WASM export must not be called when app version guard fires"
    );
}

// ── PluginInstallConfig tie-breaking ─────────────────────────────────────────

#[tokio::test]
async fn plugin_install_config_tie_at_equal_priority_preserves_insertion_order() {
    let cfg = PluginInstallConfig { base_priority: Some(500), ..Default::default() };
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin_with_config(PluginSource::Bytes(SLOT_NORMAL_WASM.into()), cfg.clone())
        .add_plugin_with_config(PluginSource::Bytes(SLOT_HIGH_WASM.into()), cfg)
        .build()
        .await
        .expect("build failed");

    let contents = runtime
        .render_slot("test-slot", &default_session())
        .await
        .expect("render_slot failed");

    assert_eq!(contents.len(), 2);
    assert_eq!(
        contents[0].plugin_id,
        PluginId("test/slot-normal".into()),
        "first added plugin must appear first when priority is equal"
    );
    assert_eq!(
        contents[1].plugin_id,
        PluginId("test/slot-high".into()),
        "second added plugin must appear second when priority is equal"
    );
}

// ── Protocol version guard (build-time) ───────────────────────────────────────

#[tokio::test]
async fn protocol_version_guard_at_build_time_rejects_future_version_plugin() {
    use dioxus_extism_host::PluginRuntimeError;

    let result = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HIGH_PROTOCOL_VERSION_WASM.into()))
        .build()
        .await;

    assert!(
        matches!(
            &result,
            Err(PluginRuntimeError::ProtocolVersionMismatch { required, host })
            if *required == PROTOCOL_VERSION + 1 && *host == PROTOCOL_VERSION
        ),
        "expected ProtocolVersionMismatch {{ required: {}, host: {} }}, got: {:?}",
        PROTOCOL_VERSION + 1,
        PROTOCOL_VERSION,
        result.as_ref().map(|_| "Ok(...)").unwrap_err()
    );
}
