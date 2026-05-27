/// §1 — Manifest extensions integration tests.
///
/// Requires compiled fixtures:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-with-extension -p fixture-slot-normal
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use dioxus_extism_host::{
    ManifestExtensionError, ManifestExtensionHandler, OnUnknownExtension, PluginRuntimeBuilder,
    PluginRuntimeError, PluginSource,
};
use dioxus_extism_protocol::{ClientCapabilities, PluginId, SessionCtx, SessionId, PROTOCOL_VERSION};

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(WITH_EXTENSION_WASM, "fixture_with_extension.wasm");

fn default_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("ext-test".into()),
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

// ── Handler helpers ────────────────────────────────────────────────────────────

/// Records every `validate` call as `(namespace, value)`.
struct RecordingHandler {
    validate_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    on_load_calls: Arc<Mutex<Vec<(String, serde_json::Value)>>>,
    on_unload_count: Arc<AtomicUsize>,
    validate_result: Result<(), String>,
    on_load_result: Result<(), String>,
}

impl RecordingHandler {
    fn new() -> (Self, Arc<Mutex<Vec<(String, serde_json::Value)>>>, Arc<Mutex<Vec<(String, serde_json::Value)>>>, Arc<AtomicUsize>) {
        let validate_calls = Arc::new(Mutex::new(vec![]));
        let on_load_calls = Arc::new(Mutex::new(vec![]));
        let on_unload_count = Arc::new(AtomicUsize::new(0));
        let handler = Self {
            validate_calls: Arc::clone(&validate_calls),
            on_load_calls: Arc::clone(&on_load_calls),
            on_unload_count: Arc::clone(&on_unload_count),
            validate_result: Ok(()),
            on_load_result: Ok(()),
        };
        (handler, validate_calls, on_load_calls, on_unload_count)
    }

    fn failing_validate(namespace: &str) -> Self {
        let (mut h, ..) = Self::new();
        h.validate_result = Err(format!("validate rejected for {namespace}"));
        h
    }

    fn failing_on_load() -> Self {
        let (mut h, ..) = Self::new();
        h.on_load_result = Err("on_load rejected".into());
        h
    }
}

impl ManifestExtensionHandler for RecordingHandler {
    fn validate(
        &self,
        _plugin_id: &PluginId,
        value: &serde_json::Value,
    ) -> Result<(), ManifestExtensionError> {
        // Record namespace from the value (we don't have it here; push as-is)
        self.validate_calls.lock().unwrap().push(("recorded".into(), value.clone()));
        self.validate_result.as_ref().map(|_| ()).map_err(|msg| {
            ManifestExtensionError::ValidationFailed {
                namespace: "test.my-feature".into(),
                message: msg.clone(),
            }
        })
    }

    fn on_load(
        &self,
        _plugin_id: &PluginId,
        value: &serde_json::Value,
    ) -> Result<(), ManifestExtensionError> {
        self.on_load_calls.lock().unwrap().push(("recorded".into(), value.clone()));
        self.on_load_result.as_ref().map(|_| ()).map_err(|msg| {
            ManifestExtensionError::LoadFailed {
                namespace: "test.my-feature".into(),
                message: msg.clone(),
            }
        })
    }

    fn on_unload(&self, _plugin_id: &PluginId) -> Result<(), ManifestExtensionError> {
        self.on_unload_count.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn validate_called_at_build_time() {
    let (handler, validate_calls, _, _) = RecordingHandler::new();
    let _runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_manifest_extension("test.my-feature", Arc::new(handler))
        .with_on_unknown_extension(OnUnknownExtension::Ignore)
        .build()
        .await
        .expect("build failed");

    let calls = validate_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "validate should be called once");
    assert_eq!(calls[0].1, serde_json::json!({"level": 2}));
}

#[tokio::test]
async fn on_load_called_after_successful_build() {
    let (handler, validate_calls, on_load_calls, _) = RecordingHandler::new();
    let _runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_manifest_extension("test.my-feature", Arc::new(handler))
        .with_on_unknown_extension(OnUnknownExtension::Ignore)
        .build()
        .await
        .expect("build failed");

    assert_eq!(validate_calls.lock().unwrap().len(), 1, "validate called");
    assert_eq!(on_load_calls.lock().unwrap().len(), 1, "on_load called");
    // validate before on_load is guaranteed by the load pipeline
}

#[tokio::test]
async fn validate_failure_aborts_build() {
    let handler = RecordingHandler::failing_validate("test.my-feature");
    let result = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_manifest_extension("test.my-feature", Arc::new(handler))
        .with_on_unknown_extension(OnUnknownExtension::Ignore)
        .build()
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::ManifestExtension { .. })),
        "expected ManifestExtension error, got: {:?}", result.as_ref().err()
    );
}

#[tokio::test]
async fn on_load_failure_aborts_build() {
    let handler = RecordingHandler::failing_on_load();
    let result = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_manifest_extension("test.my-feature", Arc::new(handler))
        .with_on_unknown_extension(OnUnknownExtension::Ignore)
        .build()
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::ManifestExtension { .. })),
        "expected ManifestExtension error on on_load failure, got: {:?}", result.as_ref().err()
    );
}

#[tokio::test]
async fn on_unload_called_at_unload_time() {
    let (handler, _, _, on_unload_count) = RecordingHandler::new();
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_manifest_extension("test.my-feature", Arc::new(handler))
        .with_on_unknown_extension(OnUnknownExtension::Ignore)
        .build()
        .await
        .expect("build failed");

    let plugin_id = PluginId("test/with-extension".into());
    runtime.unload_plugin(&plugin_id).await.expect("unload failed");

    assert_eq!(on_unload_count.load(Ordering::Relaxed), 1, "on_unload should be called once");
}

#[tokio::test]
async fn unknown_ns_warn_loads_plugin() {
    let result = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_on_unknown_extension(OnUnknownExtension::Warn)
        .build()
        .await;

    assert!(result.is_ok(), "Warn policy should allow unknown namespaces");
    let runtime = result.unwrap();
    let session = default_session();
    let contents = runtime.render_slot("ext-slot", &session).await.expect("render");
    assert!(!contents.is_empty(), "plugin should render");
}

#[tokio::test]
async fn unknown_ns_error_rejects_plugin() {
    let result = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_on_unknown_extension(OnUnknownExtension::Error)
        .build()
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::UnknownManifestExtension { .. })),
        "Error policy should reject unknown namespace, got: {:?}", result.as_ref().err()
    );
}

#[tokio::test]
async fn unknown_ns_ignore_loads_plugin() {
    let result = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_on_unknown_extension(OnUnknownExtension::Ignore)
        .build()
        .await;

    assert!(result.is_ok(), "Ignore policy should silently allow unknown namespaces");
}

#[tokio::test]
async fn register_at_runtime_then_install() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("empty runtime build failed");

    let (handler, _, on_load_calls, _) = RecordingHandler::new();
    runtime
        .register_manifest_extension("test.my-feature", Arc::new(handler))
        .await;
    runtime.register_capability_check("test.cap-a", Arc::new(|_, _| Ok(()))).await;

    let id = runtime
        .install(
            src(WITH_EXTENSION_WASM),
            dioxus_extism_host::PluginInstallConfig::default(),
        )
        .await
        .expect("install failed");

    assert_eq!(id, PluginId("test/with-extension".into()));
    let calls = on_load_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "on_load should be called after install");
}

#[tokio::test]
async fn second_namespace_handler_receives_correct_value() {
    let another_ns_calls: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(vec![]));
    let another_ns_calls_clone = Arc::clone(&another_ns_calls);

    struct AnotherHandler(Arc<Mutex<Vec<serde_json::Value>>>);
    impl ManifestExtensionHandler for AnotherHandler {
        fn validate(&self, _: &PluginId, value: &serde_json::Value) -> Result<(), ManifestExtensionError> {
            self.0.lock().unwrap().push(value.clone());
            Ok(())
        }
        fn on_load(&self, _: &PluginId, _: &serde_json::Value) -> Result<(), ManifestExtensionError> { Ok(()) }
        fn on_unload(&self, _: &PluginId) -> Result<(), ManifestExtensionError> { Ok(()) }
    }

    let _runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check("test.cap-a", Arc::new(|_, _| Ok(())))
        .with_manifest_extension("test.my-feature", Arc::new(RecordingHandler::new().0))
        .with_manifest_extension("test.another-ns", Arc::new(AnotherHandler(another_ns_calls_clone)))
        .build()
        .await
        .expect("build failed");

    let calls = another_ns_calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "validate called for test.another-ns");
    assert_eq!(calls[0], serde_json::json!("hello"), "value matches manifest declaration");
}
