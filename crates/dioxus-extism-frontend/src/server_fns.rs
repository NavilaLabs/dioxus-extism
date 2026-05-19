use dioxus::prelude::*;
use dioxus_extism_protocol::{
    ClientCapabilities, ComponentResolution, OverrideMap, SessionId, SlotContent,
};

/// Fetch the current `OverrideMap` at boot time.
#[server]
pub async fn get_override_map(
    _caps: ClientCapabilities,
) -> Result<OverrideMap, ServerFnError> {
    Ok(OverrideMap::default())
}

/// Fetch slot contributions for a named slot.
#[server]
pub async fn get_slot_content(
    _slot: String,
    _session_id: SessionId,
    _caps: ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError> {
    Ok(vec![])
}

/// Resolve plugin transforms for a named component.
///
/// Returns `None` if no transforms are registered for `component_name`.
/// The `PluginRuntime` must be added to the Axum router via `PluginRuntimeExt::with_plugin_runtime`
/// before calling this function. If it is not present, an error is returned and the component
/// renders its fallback.
#[server]
pub async fn get_component_resolution(
    component_name: String,
    props: serde_json::Value,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<Option<ComponentResolution>, ServerFnError> {
    use std::sync::Arc;

    use axum::extract::Extension;
    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Extension(runtime): Extension<Arc<PluginRuntime>> =
        dioxus::fullstack::FullstackContext::extract()
            .await
            .map_err(|e| ServerFnError::new(format!("PluginRuntime not in request extensions — did you call with_plugin_runtime? ({e})")))?;

    let session = SessionCtx {
        session_id,
        user_id: None,
        client: caps,
        caller: None,
    };

    runtime
        .resolve_component(&component_name, props, &session)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}
