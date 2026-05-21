use dioxus::prelude::*;
use dioxus_extism_protocol::{
    ClientCapabilities, ComponentResolution, OverrideMap, PageRouteOutput, RouteTransforms,
    SessionId, SlotContent, ViewUpdate,
};

/// Fetch the current `OverrideMap` at boot time.
#[server]
pub async fn get_override_map(
    caps: ClientCapabilities,
) -> Result<OverrideMap, ServerFnError> {
    use std::sync::Arc;

    use dioxus_extism_host::PluginRuntime;

    let _ = caps;
    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        tracing::warn!("get_override_map: PluginRuntime not found in request extensions — add .layer(axum::Extension(runtime)) to your router");
        return Ok(OverrideMap::default());
    };
    Ok(runtime.override_map().await)
}

/// Fetch slot contributions for a named slot.
#[server]
pub async fn get_slot_content(
    slot: String,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError> {
    use std::sync::Arc;

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        tracing::warn!("get_slot_content: PluginRuntime not found in request extensions — add .layer(axum::Extension(runtime)) to your router");
        return Ok(vec![]);
    };
    let session = SessionCtx { session_id, user_id: None, client: caps, caller: None };
    runtime
        .render_slot(&slot, &session)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
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

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        tracing::warn!("get_route_transforms: PluginRuntime not found in request extensions — add .layer(axum::Extension(runtime)) to your router");
        return Err(ServerFnError::new(
            "PluginRuntime not in request extensions — add .layer(axum::Extension(runtime)) to your router",
        ));
    };

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

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::PluginId;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        tracing::warn!("get_plugin_state: PluginRuntime not found in request extensions");
        return Ok(None);
    };
    let pid = PluginId(plugin_id);
    Ok(runtime.get_plugin_state(&pid, &key, &session_id).await)
}

/// Route an interaction event to the owning plugin and return the updated view.
///
/// Called by `PluginViewRenderer` when the user interacts with an interactive element
/// (button click, input change, etc.) embedded in a plugin view.
#[server]
pub async fn handle_plugin_interaction(
    plugin_id: String,
    handler_id: String,
    event_data: serde_json::Value,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<ViewUpdate, ServerFnError> {
    use std::sync::Arc;

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::{HandlerId, PluginId as PId, SessionCtx};

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        tracing::warn!("handle_plugin_interaction: PluginRuntime not found in request extensions — add .layer(axum::Extension(runtime)) to your router");
        return Err(ServerFnError::new(
            "PluginRuntime not in request extensions — add .layer(axum::Extension(runtime)) to your router",
        ));
    };

    let session = SessionCtx { session_id, user_id: None, client: caps, caller: None };
    let pid = PId(plugin_id);
    let hid = HandlerId(handler_id);

    runtime
        .handle_interaction(&pid, &hid, event_data, &session)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
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

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        tracing::warn!("get_component_resolution: PluginRuntime not found in request extensions — add .layer(axum::Extension(runtime)) to your router");
        return Err(ServerFnError::new(
            "PluginRuntime not in request extensions — add .layer(axum::Extension(runtime)) to your router",
        ));
    };

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

/// Render a plugin-declared page route.
///
/// `relative_path` is the path after the host's configured prefix, e.g. `"/notes"`.
/// Returns `None` if no plugin has claimed that path (caller should render a 404).
#[server]
pub async fn get_plugin_page(
    relative_path: String,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<Option<PageRouteOutput>, ServerFnError> {
    use std::sync::Arc;

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        tracing::warn!("get_plugin_page: PluginRuntime not found in request extensions — add .layer(axum::Extension(runtime)) to your router");
        return Ok(None);
    };

    let session = SessionCtx { session_id, user_id: None, client: caps, caller: None };
    runtime
        .render_page_route(&relative_path, &session)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}
