use dioxus_extism_host::PluginRuntimeBuilder;

#[tokio::test]
async fn empty_runtime_builds() {
    let runtime = PluginRuntimeBuilder::new()
        .with_session_ttl(std::time::Duration::from_secs(3600))
        .build()
        .await;
    assert!(runtime.is_ok(), "empty runtime should build: {:?}", runtime.err());
}
