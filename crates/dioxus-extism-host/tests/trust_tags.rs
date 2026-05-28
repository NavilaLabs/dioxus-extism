/// §5 — Trust tag (Ed25519 signature verification) tests.
///
/// Requires compiled fixtures:
///   cargo build --target wasm32-unknown-unknown --release -p fixture-slot-normal
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};

use dioxus_extism_host::{
    PluginInstallConfig, PluginRuntimeBuilder, PluginRuntimeError, PluginSource,
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

fixture!(SLOT_NORMAL_WASM, "fixture_slot_normal.wasm");

fn src(bytes: &'static [u8]) -> PluginSource {
    PluginSource::Bytes(std::borrow::Cow::Borrowed(bytes))
}

/// Returns (pkcs8_doc, public_key_bytes).
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
async fn unsigned_loads_with_verified_false() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/slot-normal".into());
    let tag = runtime.plugin_trust_tag(&id).await;
    assert!(tag.is_some(), "plugin should exist");
    let tag = tag.unwrap();
    assert!(!tag.verified, "unsigned plugin should have verified == false");
    assert!(tag.signer_key_id.is_none(), "signer_key_id should be None for unsigned");
}

#[tokio::test]
async fn valid_signature_loads_with_verified_true() {
    let (pkcs8, pub_key) = generate_keypair();
    let sig = sign_bytes(&pkcs8, SLOT_NORMAL_WASM);

    let runtime = PluginRuntimeBuilder::new()
        .add_plugin_with_config(
            src(SLOT_NORMAL_WASM),
            PluginInstallConfig { signature: Some(sig), key_id: None, ..Default::default() },
        )
        .with_trust_key("key1", pub_key)
        .build()
        .await
        .expect("build failed");

    let id = PluginId("test/slot-normal".into());
    let tag = runtime.plugin_trust_tag(&id).await.expect("plugin not found");
    assert!(tag.verified, "valid signature should yield verified == true");
    assert_eq!(tag.signer_key_id.as_deref(), Some("key1"));
}

#[tokio::test]
async fn require_signature_rejects_unsigned() {
    let result = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .with_require_signature(true)
        .build()
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::UntrustedPlugin(_))),
        "expected UntrustedPlugin for unsigned plugin with require_signature, got: {:?}", result.as_ref().err()
    );
}

#[tokio::test]
async fn require_signature_rejects_tampered_sig() {
    let (pkcs8, pub_key) = generate_keypair();
    let mut sig = sign_bytes(&pkcs8, SLOT_NORMAL_WASM);
    // Flip a byte to tamper.
    sig[10] ^= 0xFF;

    let result = PluginRuntimeBuilder::new()
        .add_plugin_with_config(
            src(SLOT_NORMAL_WASM),
            PluginInstallConfig { signature: Some(sig), key_id: None, ..Default::default() },
        )
        .with_trust_key("key1", pub_key)
        .with_require_signature(true)
        .build()
        .await;

    assert!(
        matches!(result, Err(PluginRuntimeError::UntrustedPlugin(_))),
        "tampered signature should be rejected, got: {:?}", result.as_ref().err()
    );
}

#[tokio::test]
async fn key_id_hint_selects_correct_key() {
    let (pkcs8_a, pub_a) = generate_keypair();
    let (pkcs8_b, pub_b) = generate_keypair();
    let sig = sign_bytes(&pkcs8_b, SLOT_NORMAL_WASM);

    let runtime = PluginRuntimeBuilder::new()
        .add_plugin_with_config(
            src(SLOT_NORMAL_WASM),
            PluginInstallConfig {
                signature: Some(sig),
                key_id: Some("key-b".into()),
                ..Default::default()
            },
        )
        .with_trust_key("key-a", pub_a)
        .with_trust_key("key-b", pub_b)
        .build()
        .await
        .expect("build failed");

    let _ = pkcs8_a; // suppress unused warning

    let id = PluginId("test/slot-normal".into());
    let tag = runtime.plugin_trust_tag(&id).await.expect("plugin not found");
    assert!(tag.verified);
    assert_eq!(tag.signer_key_id.as_deref(), Some("key-b"));
}

#[tokio::test]
async fn wrong_key_id_hint_fails_even_if_other_key_matches() {
    let (pkcs8_a, pub_a) = generate_keypair();
    let (pkcs8_b, pub_b) = generate_keypair();
    let sig = sign_bytes(&pkcs8_b, SLOT_NORMAL_WASM);

    let result = PluginRuntimeBuilder::new()
        .add_plugin_with_config(
            src(SLOT_NORMAL_WASM),
            PluginInstallConfig {
                signature: Some(sig),
                // Hint says "key-a" but sig was made with key-b.
                key_id: Some("key-a".into()),
                ..Default::default()
            },
        )
        .with_trust_key("key-a", pub_a)
        .with_trust_key("key-b", pub_b)
        .with_require_signature(true)
        .build()
        .await;

    let _ = pkcs8_a;
    let _ = pkcs8_b;

    assert!(
        matches!(result, Err(PluginRuntimeError::UntrustedPlugin(_))),
        "wrong key hint should not fall back to matching key, got: {:?}", result.as_ref().err()
    );
}

#[tokio::test]
async fn all_keys_tried_without_hint() {
    let (pkcs8_a, pub_a) = generate_keypair();
    let (pkcs8_b, pub_b) = generate_keypair();
    let (pkcs8_c, pub_c) = generate_keypair();
    // Sign with the middle key.
    let sig = sign_bytes(&pkcs8_b, SLOT_NORMAL_WASM);

    let runtime = PluginRuntimeBuilder::new()
        .add_plugin_with_config(
            src(SLOT_NORMAL_WASM),
            PluginInstallConfig { signature: Some(sig), key_id: None, ..Default::default() },
        )
        .with_trust_key("key-a", pub_a)
        .with_trust_key("key-b", pub_b)
        .with_trust_key("key-c", pub_c)
        .build()
        .await
        .expect("build failed");

    let _ = (pkcs8_a, pkcs8_c);

    let id = PluginId("test/slot-normal".into());
    let tag = runtime.plugin_trust_tag(&id).await.expect("plugin not found");
    assert!(tag.verified, "should find matching key among all registered keys");
    assert_eq!(tag.signer_key_id.as_deref(), Some("key-b"));
}

#[tokio::test]
async fn plugin_trust_tag_none_for_unknown_id() {
    let runtime = PluginRuntimeBuilder::new()
        .build()
        .await
        .expect("build failed");

    let result = runtime.plugin_trust_tag(&PluginId("ghost/plugin".into())).await;
    assert!(result.is_none(), "unknown plugin id should return None");
}

#[tokio::test]
async fn install_verifies_trust_tag() {
    let (pkcs8, pub_key) = generate_keypair();
    let sig = sign_bytes(&pkcs8, SLOT_NORMAL_WASM);

    let runtime = PluginRuntimeBuilder::new()
        .with_trust_key("install-key", pub_key)
        .build()
        .await
        .expect("build failed");

    let id = runtime
        .install(
            src(SLOT_NORMAL_WASM),
            PluginInstallConfig { signature: Some(sig), key_id: None, ..Default::default() },
        )
        .await
        .expect("install failed");

    let tag = runtime.plugin_trust_tag(&id).await.expect("plugin not found after install");
    assert!(tag.verified, "install should verify trust tag");
    assert_eq!(tag.signer_key_id.as_deref(), Some("install-key"));
}
