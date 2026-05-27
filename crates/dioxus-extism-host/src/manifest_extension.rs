use dioxus_extism_protocol::PluginId;

/// Error returned by a [`ManifestExtensionHandler`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestExtensionError {
    #[error("validation failed for namespace {namespace}: {message}")]
    ValidationFailed { namespace: String, message: String },
    #[error("on_load failed for namespace {namespace}: {message}")]
    LoadFailed { namespace: String, message: String },
    #[error("on_unload failed for namespace {namespace}: {message}")]
    UnloadFailed { namespace: String, message: String },
}

/// Controls how the runtime behaves when a plugin's `extensions` map contains a
/// namespace for which no handler has been registered.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub enum OnUnknownExtension {
    /// Emit a `tracing::warn!` and continue loading. Default.
    #[default]
    Warn,
    /// Abort the plugin load with an error.
    Error,
    /// Silently ignore.
    Ignore,
}

/// A host-provided handler for one extension namespace in [`PluginManifest::extensions`].
///
/// Register handlers via [`PluginRuntimeBuilder::with_manifest_extension`] or
/// [`PluginRuntime::register_manifest_extension`].
pub trait ManifestExtensionHandler: Send + Sync {
    /// Called before the plugin pool is built. Return an error to abort the load.
    fn validate(
        &self,
        plugin_id: &PluginId,
        value: &serde_json::Value,
    ) -> Result<(), ManifestExtensionError>;

    /// Called after the plugin has been fully inserted into the runtime.
    /// Returning an error causes the plugin to be removed and the load to fail.
    fn on_load(
        &self,
        plugin_id: &PluginId,
        value: &serde_json::Value,
    ) -> Result<(), ManifestExtensionError>;

    /// Called before the plugin is removed from the runtime. Best-effort — errors are logged.
    fn on_unload(&self, plugin_id: &PluginId) -> Result<(), ManifestExtensionError>;
}
