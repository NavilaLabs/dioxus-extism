use dioxus_extism_host::PluginRuntimeBuilder;
use dioxus_extism_protocol::{ClientCapabilities, SessionCtx, SessionId, PROTOCOL_VERSION};

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
async fn resolve_component_returns_none_for_unknown() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("runtime build failed");
    let result = runtime
        .resolve_component("Unknown", serde_json::json!({}), &test_session())
        .await
        .expect("resolve_component failed");
    assert!(result.is_none());
}
