use dioxus_extism_protocol::PluginId;

use crate::manifest_extension::ManifestExtensionError;

/// Runtime errors from plugin operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PluginRuntimeError {
    #[error("plugin not found: {0:?}")]
    PluginNotFound(PluginId),

    #[error("plugin call failed: {source}")]
    CallFailed {
        #[source]
        source: extism::Error,
    },

    #[error("plugin task panicked: {0}")]
    TaskPanic(String),

    #[error("protocol version mismatch: plugin requires {required}, host has {host}")]
    ProtocolVersionMismatch { required: u32, host: u32 },

    #[error("SHA-256 checksum mismatch for plugin at {url}")]
    ChecksumMismatch { url: String },

    #[error("HTTP fetch failed for {url}: {message}")]
    FetchFailed { url: String, message: String },

    #[error("plugin IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialisation error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("plugin pool error: {0}")]
    Pool(String),

    #[error("invocation error: {0}")]
    Invocation(#[from] InvocationError),

    #[error("plugin disabled: {0:?}")]
    PluginDisabled(PluginId),

    #[error("capability denied for plugin {plugin:?}: {capability}")]
    CapabilityDenied { plugin: PluginId, capability: String },

    #[error("persistence error: {0}")]
    Persistence(#[from] PersistenceError),

    #[error("API route conflict: {method} {path} — claimed by {first:?} and {second:?}")]
    ApiRouteConflict {
        method: String,
        path: String,
        first: PluginId,
        second: PluginId,
    },

    #[error("page route conflict: {path} — claimed by {first:?} and {second:?}")]
    PageRouteConflict { path: String, first: PluginId, second: PluginId },

    #[error("manifest extension error for plugin {plugin:?}: {source}")]
    ManifestExtension {
        plugin: PluginId,
        #[source]
        source: ManifestExtensionError,
    },

    #[error("unknown manifest extension namespace {namespace:?} in plugin {plugin:?}")]
    UnknownManifestExtension { plugin: PluginId, namespace: String },

    #[error("plugin {0:?} has no valid Ed25519 signature but require_signature is enabled")]
    UntrustedPlugin(PluginId),
}

/// Errors from `dx_invoke` host function calls.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum InvocationError {
    #[error("invocation not found: {0}")]
    NotFound(String),

    #[error("argument deserialisation failed: {0}")]
    BadArgs(#[from] serde_json::Error),

    /// Structured error returned by the host handler.
    #[error("invocation failed (code {code}): {message}")]
    Failed { code: u32, message: String },

    #[error("invocation timed out after {0:?}")]
    Timeout(std::time::Duration),
}

/// Errors from `StatePersistenceProvider`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PersistenceError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialisation error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Custom(String),
}
