use ring::signature::{UnparsedPublicKey, ED25519};

/// Result of verifying a plugin's Ed25519 signature at load time.
///
/// Stored on [`LoadedPlugin`] and accessible via
/// [`PluginRuntime::plugin_trust_tag`]. dioxus-extism records this tag but
/// assigns no policy meaning to it — hosts use it (together with §3 capability
/// checks and §4 route-replace policy) to make their own trust decisions.
#[derive(Debug, Clone)]
pub struct TrustTag {
    /// `true` if the WASM binary's Ed25519 signature was verified against a
    /// configured [`TrustKey`].
    pub verified: bool,
    /// The `key_id` of the key that produced a valid signature, if any.
    pub signer_key_id: Option<String>,
}

/// An Ed25519 public key with an associated identifier used at runtime construction.
///
/// Register via [`PluginRuntimeBuilder::with_trust_key`].
#[derive(Clone)]
pub struct TrustKey {
    /// Arbitrary identifier for this key — used to label [`TrustTag::signer_key_id`].
    pub key_id: String,
    /// Raw Ed25519 public key bytes (32 bytes).
    pub public_key_bytes: Vec<u8>,
}

/// Compute a [`TrustTag`] for `wasm_bytes` by verifying `signature` against the
/// configured `trust_keys`.
///
/// - If `key_id_hint` is `Some`, only the key with that id is tried.
/// - If `key_id_hint` is `None`, all keys are tried in order.
/// - If `signature` is `None`, returns `TrustTag { verified: false, .. }`.
pub(crate) fn compute_trust_tag(
    wasm_bytes: &[u8],
    signature: Option<&[u8]>,
    key_id_hint: Option<&str>,
    trust_keys: &[TrustKey],
) -> TrustTag {
    let Some(sig) = signature else {
        return TrustTag { verified: false, signer_key_id: None };
    };

    let candidates: Vec<&TrustKey> = match key_id_hint {
        Some(hint) => trust_keys.iter().filter(|k| k.key_id == hint).collect(),
        None => trust_keys.iter().collect(),
    };

    for key in candidates {
        let pk = UnparsedPublicKey::new(&ED25519, &key.public_key_bytes);
        if pk.verify(wasm_bytes, sig).is_ok() {
            return TrustTag { verified: true, signer_key_id: Some(key.key_id.clone()) };
        }
    }

    TrustTag { verified: false, signer_key_id: None }
}
