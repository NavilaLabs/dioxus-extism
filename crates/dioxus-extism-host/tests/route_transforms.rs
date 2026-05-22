use dioxus_extism_host::{HookOutcome, PluginRuntimeBuilder};
use dioxus_extism_protocol::{
    ClientCapabilities, PluginView, Selector, SessionCtx, SessionId, TransformContext,
    PROTOCOL_VERSION,
};

fn test_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("test".into()),
        user_id: None,
        client: ClientCapabilities {
            protocol_version: PROTOCOL_VERSION,
            app_version: 0,
            registered_host_components: vec![],
        },
        caller: None,
    }
}

#[tokio::test]
async fn empty_runtime_returns_empty_transforms() {
    let runtime = PluginRuntimeBuilder::new().build().await.expect("runtime build");
    let result = runtime
        .render_route_transforms("/product/42", &test_session())
        .await
        .expect("render_route_transforms");
    assert!(result.is_empty());
}

#[tokio::test]
async fn unmatched_path_returns_empty_transforms() {
    let runtime = PluginRuntimeBuilder::new().build().await.expect("runtime build");
    let result = runtime
        .render_route_transforms("/no/plugin/here", &test_session())
        .await
        .expect("render_route_transforms");
    assert!(!result.has_wrap());
    assert!(result.before.is_empty());
    assert!(result.after.is_empty());
}

#[tokio::test]
async fn run_hook_no_handlers_returns_passed_with_original_context() {
    let runtime = PluginRuntimeBuilder::new().build().await.expect("runtime build");
    let ctx = serde_json::json!({"value": 42});
    let outcome = runtime
        .run_hook("unregistered-hook", ctx.clone(), &test_session())
        .await
        .expect("run_hook failed");
    assert!(
        matches!(&outcome, HookOutcome::Passed(c) if *c == ctx),
        "expected Passed with original context, got {outcome:?}"
    );
}

#[tokio::test]
async fn apply_tree_transforms_no_within_returns_view_unchanged() {
    let runtime = PluginRuntimeBuilder::new().build().await.expect("runtime build");
    let original_view = PluginView::Text("unchanged".into());
    let context = TransformContext::default();
    let result = runtime
        .apply_tree_transforms(
            &Selector::Slot("test-slot".into()),
            original_view.clone(),
            context,
            &test_session(),
        )
        .await
        .expect("apply_tree_transforms failed");
    assert_eq!(result, original_view);
}

/// Integration tests requiring compiled WASM fixtures are marked #[ignore].
/// Build the fixture with:
///   cargo build --target wasm32-unknown-unknown -p route-injection-example-plugin
/// then remove the #[ignore] annotation to run these.
#[tokio::test]
#[ignore = "requires compiled WASM test fixture"]
async fn inject_after_appends_view() {
    // Load a plugin that declares InjectAfter for "/product/:id"
    // and verify the after list has exactly one entry.
    todo!()
}

#[tokio::test]
#[ignore = "requires compiled WASM test fixture"]
async fn wrap_pipeline_folds_sequential() {
    // Load two Wrap plugins for the same route (different priorities).
    // The higher-priority plugin receives the seed (HostComponent "__content__").
    // The lower-priority plugin receives the first plugin's full output as original.
    // Verify the final wrap view contains the first plugin's output nested inside the second.
    todo!()
}
