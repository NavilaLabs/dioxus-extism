/// §3 — Host-defined capability classes tests.
///
/// Requires compiled fixtures:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-with-extension -p fixture-slot-normal
use std::sync::{Arc, Mutex};

use dioxus_extism_host::{
    ManifestExtensionError, ManifestExtensionHandler, PluginInstallConfig, PluginRuntimeBuilder,
    PluginRuntimeError, PluginSource,
};
use dioxus_extism_protocol::PluginId;

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(WITH_EXTENSION_WASM, "fixture_with_extension.wasm");

fn src(bytes: &'static [u8]) -> PluginSource {
    PluginSource::Bytes(std::borrow::Cow::Borrowed(bytes))
}

#[tokio::test]
async fn capability_allowed_when_check_returns_ok() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_on_unknown_extension(dioxus_extism_host::OnUnknownExtension::Ignore)
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/with-extension".into());
    let result = runtime.check_custom_capability(&id, "test.cap-a").await;
    assert!(result.is_ok(), "check should pass when declared and check returns Ok");
}

#[tokio::test]
async fn capability_denied_at_build_when_check_returns_err() {
    let result = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Err("tier too low".into())))
        .with_on_unknown_extension(dioxus_extism_host::OnUnknownExtension::Ignore)
        .build()
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::CapabilityDenied { .. })),
        "expected CapabilityDenied at build time, got: {:?}", result.as_ref().err()
    );
}

#[tokio::test]
async fn capability_denied_when_no_check_registered() {
    // fixture-with-extension declares HostCapability::Custom { "test.cap-a", ... }
    // Without a registered check, the default policy is deny.
    let result = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_on_unknown_extension(dioxus_extism_host::OnUnknownExtension::Ignore)
        .build()
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::CapabilityDenied { .. })),
        "expected CapabilityDenied with no check registered, got: {:?}", result.as_ref().err()
    );
}

#[tokio::test]
async fn check_function_receives_correct_value() {
    let captured: Arc<Mutex<Option<serde_json::Value>>> = Arc::new(Mutex::new(None));
    let captured_clone = Arc::clone(&captured);

    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check(
            "test.cap-a",
            Arc::new(move |_, value: &serde_json::Value| {
                *captured_clone.lock().unwrap() = Some(value.clone());
                Ok(())
            }),
        )
        .with_on_unknown_extension(dioxus_extism_host::OnUnknownExtension::Ignore)
        .build()
        .await
        .expect("build failed");

    let _ = runtime; // keep alive
    let val = captured.lock().unwrap().clone();
    assert_eq!(
        val,
        Some(serde_json::json!({"tier": 1})),
        "check should receive the value from the manifest"
    );
}

#[tokio::test]
async fn check_custom_capability_plugin_not_found() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let result = runtime
        .check_custom_capability(&PluginId("ghost/plugin".into()), "test.cap-a")
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::PluginNotFound(_))),
        "expected PluginNotFound, got: {result:?}"
    );
}

#[tokio::test]
async fn register_check_at_runtime_then_install() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let called = Arc::new(Mutex::new(false));
    let called_clone = Arc::clone(&called);

    runtime
        .register_capability_check(
            "test.cap-a",
            Arc::new(move |_, _| {
                *called_clone.lock().unwrap() = true;
                Ok(())
            }),
        )
        .await;

    struct NopExtHandler;
    impl ManifestExtensionHandler for NopExtHandler {
        fn validate(&self, _: &PluginId, _: &serde_json::Value) -> Result<(), ManifestExtensionError> { Ok(()) }
        fn on_load(&self, _: &PluginId, _: &serde_json::Value) -> Result<(), ManifestExtensionError> { Ok(()) }
        fn on_unload(&self, _: &PluginId) -> Result<(), ManifestExtensionError> { Ok(()) }
    }

    runtime
        .register_manifest_extension(
            "test.my-feature",
            Arc::new(NopExtHandler),
        )
        .await;

    let _ = runtime
        .install(src(WITH_EXTENSION_WASM), PluginInstallConfig::default())
        .await
        .expect("install failed");

    assert!(*called.lock().unwrap(), "check should be called during install");
}
