/// §6 — Plugin registry API tests.
///
/// Requires compiled fixtures:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-slot-normal -p fixture-slot-high
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};

use dioxus_extism_host::{
    PluginInstallConfig, PluginRuntimeBuilder, PluginRuntimeError, PluginSource,
};
use dioxus_extism_protocol::{
    ClientCapabilities, PluginId, PluginView, SessionCtx, SessionId, PROTOCOL_VERSION,
};

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(SLOT_NORMAL_WASM, "fixture_slot_normal.wasm");
fixture!(HIGH_PROTOCOL_WASM, "fixture_high_protocol_version.wasm");

fn default_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("registry-test".into()),
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

fn generate_keypair() -> (Vec<u8>, Vec<u8>) {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("keygen failed");
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("from_pkcs8 failed");
    let pub_key = pair.public_key().as_ref().to_vec();
    (pkcs8.as_ref().to_vec(), pub_key)
}

fn sign_bytes(pkcs8: &[u8], data: &[u8]) -> Vec<u8> {
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8).expect("from_pkcs8 failed");
    pair.sign(data).as_ref().to_vec()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_plugins_empty_runtime() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let plugins = runtime.list_plugins().await;
    assert!(plugins.is_empty(), "empty runtime should have no plugins");
}

#[tokio::test]
async fn list_plugins_after_build() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .build()
        .await
        .expect("build failed");

    let plugins = runtime.list_plugins().await;
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, PluginId("test/slot-normal".into()));
    assert!(plugins[0].enabled, "plugin should be enabled after build");
}

#[tokio::test]
async fn install_adds_plugin_to_list() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    assert!(runtime.list_plugins().await.is_empty());

    let id = runtime
        .install(src(SLOT_NORMAL_WASM), PluginInstallConfig::default())
        .await
        .expect("install failed");

    let plugins = runtime.list_plugins().await;
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].id, id);
}

#[tokio::test]
async fn install_same_plugin_twice_returns_error() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    runtime
        .install(src(SLOT_NORMAL_WASM), PluginInstallConfig::default())
        .await
        .expect("first install failed");

    let result = runtime
        .install(src(SLOT_NORMAL_WASM), PluginInstallConfig::default())
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::CapabilityDenied { .. })),
        "second install of same plugin should fail with CapabilityDenied, got: {result:?}"
    );
}

#[tokio::test]
async fn uninstall_removes_plugin() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let id = runtime
        .install(src(SLOT_NORMAL_WASM), PluginInstallConfig::default())
        .await
        .expect("install failed");

    runtime.uninstall(&id).await.expect("uninstall failed");

    let plugins = runtime.list_plugins().await;
    assert!(plugins.is_empty(), "plugin should be removed after uninstall");
}

#[tokio::test]
async fn uninstall_nonexistent_returns_not_found() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let result = runtime.uninstall(&PluginId("ghost/plugin".into())).await;
    assert!(
        matches!(result, Err(PluginRuntimeError::PluginNotFound(_))),
        "expected PluginNotFound, got: {result:?}"
    );
}

#[tokio::test]
async fn disable_makes_slot_return_incompatible() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/slot-normal".into());
    runtime.disable(&id).await.expect("disable failed");

    let session = default_session();
    let contents = runtime.render_slot("test-slot", &session).await.expect("render failed");

    assert!(
        contents.iter().any(|c| matches!(c.view, PluginView::Incompatible { .. })),
        "disabled plugin should produce Incompatible view"
    );
}

#[tokio::test]
async fn enable_restores_slot_content() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/slot-normal".into());
    runtime.disable(&id).await.expect("disable failed");
    runtime.enable(&id).await.expect("enable failed");

    let session = default_session();
    let contents = runtime.render_slot("test-slot", &session).await.expect("render failed");

    assert!(
        contents.iter().all(|c| !matches!(c.view, PluginView::Incompatible { .. })),
        "re-enabled plugin should no longer produce Incompatible"
    );
}

#[tokio::test]
async fn enable_nonexistent_returns_not_found() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let result = runtime.enable(&PluginId("ghost/plugin".into())).await;
    assert!(
        matches!(result, Err(PluginRuntimeError::PluginNotFound(_))),
        "expected PluginNotFound, got: {result:?}"
    );
}

#[tokio::test]
async fn list_plugins_reflects_trust_tag() {
    let (pkcs8, pub_key) = generate_keypair();
    let sig = sign_bytes(&pkcs8, SLOT_NORMAL_WASM);

    let runtime = PluginRuntimeBuilder::new()
        .add_plugin_with_config(
            src(SLOT_NORMAL_WASM),
            PluginInstallConfig { signature: Some(sig), key_id: None, ..Default::default() },
        )
        .with_trust_key("list-key", pub_key)
        .build()
        .await
        .expect("build failed");

    let plugins = runtime.list_plugins().await;
    assert_eq!(plugins.len(), 1);
    assert!(plugins[0].trust_tag.verified, "summary should reflect verified trust tag");
    assert_eq!(plugins[0].trust_tag.signer_key_id.as_deref(), Some("list-key"));
}

#[tokio::test]
async fn install_rejects_future_protocol_version() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let result = runtime
        .install(src(HIGH_PROTOCOL_WASM), PluginInstallConfig::default())
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::ProtocolVersionMismatch { .. })),
        "expected ProtocolVersionMismatch, got: {result:?}"
    );
}
