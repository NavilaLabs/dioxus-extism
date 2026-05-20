use dioxus::prelude::*;
use dioxus_extism_protocol::{
    ClientCapabilities, ComponentResolution, OverrideMap, PluginId, RouteTransforms, SessionId,
    SlotContent,
};

/// Fetch the current `OverrideMap` at boot time.
#[server]
pub async fn get_override_map(
    caps: ClientCapabilities,
) -> Result<OverrideMap, ServerFnError> {
    use std::sync::Arc;

    use axum::extract::Extension;
    use dioxus_extism_host::PluginRuntime;

    let _ = caps;
    let result: Result<Extension<Arc<PluginRuntime>>, _> =
        dioxus::fullstack::FullstackContext::extract().await;
    match result {
        Ok(Extension(runtime)) => Ok(runtime.override_map().await),
        Err(_) => Ok(OverrideMap::default()),
    }
}

/// Fetch slot contributions for a named slot.
#[server]
pub async fn get_slot_content(
    slot: String,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError> {
    use std::sync::Arc;

    use axum::extract::Extension;
    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let result: Result<Extension<Arc<PluginRuntime>>, _> =
        dioxus::fullstack::FullstackContext::extract().await;
    match result {
        Ok(Extension(runtime)) => {
            let session = SessionCtx { session_id, user_id: None, client: caps, caller: None };
            runtime
                .render_slot(&slot, &session)
                .await
                .map_err(|e| ServerFnError::new(e.to_string()))
        }
        Err(_) => Ok(vec![]),
    }
}

/// Resolve route transforms (before/wrap/after) for the current path.
///
/// Returns `RouteTransforms::empty()` when no plugins are registered for `path`.
/// The `PluginRuntime` must be added to the Axum router via `PluginRuntimeExt::with_plugin_runtime`.
#[server]
pub async fn get_route_transforms(
    path: String,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<RouteTransforms, ServerFnError> {
    use std::sync::Arc;

    use axum::extract::Extension;
    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Extension(runtime): Extension<Arc<PluginRuntime>> =
        dioxus::fullstack::FullstackContext::extract()
            .await
            .map_err(|e| {
                ServerFnError::new(format!(
                    "PluginRuntime not in request extensions — did you call with_plugin_runtime? ({e})"
                ))
            })?;

    let session = SessionCtx { session_id, user_id: None, client: caps, caller: None };

    runtime
        .render_route_transforms(&path, &session)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Read one key from a plugin's session state.
///
/// Returns `None` if the session, plugin, or key does not exist.
#[server]
pub async fn get_plugin_state(
    plugin_id: String,
    key: String,
    session_id: SessionId,
) -> Result<Option<serde_json::Value>, ServerFnError> {
    use std::sync::Arc;

    use axum::extract::Extension;
    use dioxus_extism_host::PluginRuntime;

    let result: Result<Extension<Arc<PluginRuntime>>, _> =
        dioxus::fullstack::FullstackContext::extract().await;
    match result {
        Ok(Extension(runtime)) => {
            let pid = PluginId(plugin_id);
            Ok(runtime.get_plugin_state(&pid, &key, &session_id).await)
        }
        Err(_) => Ok(None),
    }
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
