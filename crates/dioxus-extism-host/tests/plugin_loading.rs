use dioxus_extism_host::{PluginRuntimeBuilder, PluginSource};
use dioxus_extism_protocol::{
    ClientCapabilities, PluginId, SessionCtx, SessionId, PROTOCOL_VERSION,
};

/// hello-plugin WASM built for wasm32-unknown-unknown release.
const HELLO_WASM: &[u8] = include_bytes!(
    "../../target/wasm32-unknown-unknown/release/hello_plugin_plugin.wasm"
);

fn test_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("test-session".into()),
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
async fn loads_plugin_and_reads_manifest() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HELLO_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let map = runtime.override_map().await;
    assert_eq!(map.version, 0, "fresh runtime should have version 0");
    assert!(
        map.overridden_components.is_empty(),
        "hello plugin doesn't override any components"
    );
}

#[tokio::test]
async fn plugin_appears_in_slot_registry() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HELLO_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let session = test_session();
    let contents = runtime
        .render_slot("hello-slot", &session)
        .await
        .expect("render_slot failed");

    assert_eq!(contents.len(), 1, "hello plugin should contribute one slot item");
    assert_eq!(
        contents[0].plugin_id,
        PluginId("example/hello".into()),
        "plugin id should match manifest"
    );
}

#[tokio::test]
async fn slot_render_returns_plugin_view() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HELLO_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let session = test_session();
    let contents = runtime
        .render_slot("hello-slot", &session)
        .await
        .expect("render_slot failed");

    assert_eq!(contents.len(), 1);
    match &contents[0].view {
        dioxus_extism_protocol::PluginView::Element(el) => {
            assert_eq!(el.tag, "div", "hello plugin renders a div");
        }
        other => panic!("expected Element, got {other:?}"),
    }
}

#[tokio::test]
async fn unknown_slot_returns_empty() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HELLO_WASM.into()))
        .build()
        .await
        .expect("build failed");

    let session = test_session();
    let contents = runtime
        .render_slot("nonexistent-slot", &session)
        .await
        .expect("render_slot should succeed even for unknown slots");

    assert!(
        contents.is_empty(),
        "no plugin contributes to 'nonexistent-slot'"
    );
}

#[tokio::test]
async fn hook_with_no_handlers_passes_through() {
    use dioxus_extism_host::HookOutcome;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestCtx {
        value: i32,
    }

    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let session = test_session();
    let ctx = TestCtx { value: 42 };
    let outcome = runtime
        .run_hook("test_hook", ctx.clone(), &session)
        .await
        .expect("run_hook failed");

    match outcome {
        HookOutcome::Passed(result) => assert_eq!(result, ctx),
        HookOutcome::Cancelled { by, reason } => {
            panic!("hook cancelled by {by:?}: {reason}");
        }
    }
}
