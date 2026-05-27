/// §2 — Generic plugin-function dispatch tests.
///
/// Requires compiled fixtures:
///   cargo build --target wasm32-unknown-unknown --release -p fixture-echo-fn
use std::sync::Arc;

use dioxus_extism_host::{PluginRuntimeBuilder, PluginRuntimeError, PluginSource};
use dioxus_extism_protocol::{ClientCapabilities, PluginId, SessionCtx, SessionId, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(ECHO_FN_WASM, "fixture_echo_fn.wasm");

fn default_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("dispatch-test".into()),
        user_id: None,
        client: ClientCapabilities {
            protocol_version: PROTOCOL_VERSION,
            app_version: 0,
            registered_host_components: vec![],
        },
        caller: None,
    }
}

fn src(bytes: &'static [u8]) -> PluginSource {
    PluginSource::Bytes(std::borrow::Cow::Borrowed(bytes))
}

fn echo_id() -> PluginId {
    PluginId("test/echo-fn".into())
}

#[tokio::test]
async fn echo_fn_returns_input_unchanged() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ECHO_FN_WASM))
        .build()
        .await
        .expect("build failed");

    let input = serde_json::json!({"key": "value", "n": 42});
    let result: serde_json::Value = runtime
        .call_plugin(&echo_id(), "echo_fn", &input, &default_session())
        .await
        .expect("call_plugin failed");

    assert_eq!(result, input);
}

#[tokio::test]
async fn compute_fn_returns_incremented_value() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ECHO_FN_WASM))
        .build()
        .await
        .expect("build failed");

    let result: serde_json::Value = runtime
        .call_plugin(&echo_id(), "compute_fn", &serde_json::json!({"n": 41}), &default_session())
        .await
        .expect("call_plugin failed");

    assert_eq!(result, serde_json::json!({"result": 42}));
}

#[tokio::test]
async fn plugin_not_found_returns_error() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("empty build failed");

    let result: Result<serde_json::Value, _> = runtime
        .call_plugin(
            &PluginId("nonexistent/plugin".into()),
            "echo_fn",
            &serde_json::json!({}),
            &default_session(),
        )
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::PluginNotFound(_))),
        "expected PluginNotFound, got: {result:?}"
    );
}

#[tokio::test]
async fn disabled_plugin_returns_error() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ECHO_FN_WASM))
        .build()
        .await
        .expect("build failed");

    runtime.disable_plugin(&echo_id()).await.expect("disable failed");

    let result: Result<serde_json::Value, _> = runtime
        .call_plugin(&echo_id(), "echo_fn", &serde_json::json!({}), &default_session())
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::PluginDisabled(_))),
        "expected PluginDisabled, got: {result:?}"
    );
}

#[tokio::test]
async fn unknown_export_returns_call_failed() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ECHO_FN_WASM))
        .build()
        .await
        .expect("build failed");

    let result: Result<serde_json::Value, _> = runtime
        .call_plugin(&echo_id(), "nonexistent_export", &serde_json::json!({}), &default_session())
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::CallFailed { .. })),
        "expected CallFailed for unknown export, got: {result:?}"
    );
}

#[tokio::test]
async fn struct_input_round_trips_via_serde() {
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Payload {
        name: String,
        count: u32,
    }

    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ECHO_FN_WASM))
        .build()
        .await
        .expect("build failed");

    let input = serde_json::json!({"name": "test", "count": 7});
    let result: Payload = runtime
        .call_plugin(&echo_id(), "echo_fn", &input, &default_session())
        .await
        .expect("call_plugin failed");

    assert_eq!(result, Payload { name: "test".into(), count: 7 });
}

#[tokio::test]
async fn concurrent_calls_all_succeed() {
    let runtime = Arc::new(
        PluginRuntimeBuilder::new()
            .add_plugin(src(ECHO_FN_WASM))
            .build()
            .await
            .expect("build failed"),
    );

    let id = echo_id();
    let session = default_session();
    let input = serde_json::json!({"k": "v"});

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let runtime = Arc::clone(&runtime);
            let id = id.clone();
            let session = session.clone();
            let input = serde_json::json!({"k": i});
            tokio::spawn(async move {
                runtime
                    .call_plugin::<_, serde_json::Value>(&id, "echo_fn", &input, &session)
                    .await
            })
        })
        .collect();

    for (i, h) in handles.into_iter().enumerate() {
        let result = h.await.expect("spawn panicked");
        assert!(result.is_ok(), "call {i} failed: {:?}", result.err());
    }
    let _ = input; // suppress unused warning
}
