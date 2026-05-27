use std::collections::HashMap;

use dioxus::prelude::*;
use dioxus_extism_protocol::{
    AttrValue, ClientCapabilities, DomEvent, HandlerId, HostComponentRef, OverrideMap, PluginId,
    PluginView, RouteTransforms, SessionId, SlotContent, SsrRouteOutput, ViewElement, ViewUpdate,
    PROTOCOL_VERSION,
};

use crate::server_fns::{
    get_component_resolution, get_override_map, get_plugin_page,
    get_plugin_state as server_get_plugin_state, get_route_transforms, get_slot_content,
    handle_plugin_interaction,
};
use crate::session::use_session_id;

// ── Plugin interaction context ────────────────────────────────────────────────

/// Marker type: indicates we are inside a `PluginViewRenderer` subtree.
///
/// The root `PluginViewRenderer` provides this so nested instances know not to
/// override the interaction context that was already established.
#[derive(Clone, Copy)]
struct InsidePluginTree;

/// Context provided by the root `PluginViewRenderer` for interactive child elements.
#[derive(Clone)]
struct PluginInteractionCtx {
    plugin_id: Option<PluginId>,
    session_id: Signal<SessionId>,
    /// Signal that holds the current view for this plugin's contribution.
    /// Interaction handlers write to this to update the displayed view.
    view_signal: Signal<PluginView>,
    caps: ClientCapabilities,
}

// ── HostComponentRegistry ────────────────────────────────────────────────────

type HostComponentFn = Box<dyn Fn(HostComponentRef) -> Element + Send + Sync>;

/// Registry of named Dioxus components that plugins can reference by name.
#[derive(Clone, Default)]
pub struct HostComponentRegistry {
    inner: std::sync::Arc<HashMap<String, std::sync::Arc<HostComponentFn>>>,
}

impl HostComponentRegistry {
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a named host component renderer.
    ///
    /// # Panics
    /// Panics if the registry has already been shared via `Clone`.
    #[must_use]
    pub fn register(
        mut self,
        name: impl Into<String>,
        f: impl Fn(HostComponentRef) -> Element + Send + Sync + 'static,
    ) -> Self {
        std::sync::Arc::get_mut(&mut self.inner)
            .expect("registry not shared yet")
            .insert(name.into(), std::sync::Arc::new(Box::new(f)));
        self
    }

    /// Returns the names of all registered host components.
    #[must_use] 
    pub fn names(&self) -> Vec<String> {
        self.inner.keys().cloned().collect()
    }

    pub(crate) fn render(&self, name: &str, r: HostComponentRef) -> Option<Element> {
        self.inner.get(name).map(|f| f(r))
    }
}

// ── PluginBootProvider ────────────────────────────────────────────────────────

/// Fetches `OverrideMap` at boot, provides it and `ClientCapabilities` in context.
#[component]
pub fn PluginBootProvider(children: Element) -> Element {
    let registry = use_context_or_default::<HostComponentRegistry>();
    let caps = ClientCapabilities {
        protocol_version: PROTOCOL_VERSION,
        app_version: option_env!("APP_VERSION")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        registered_host_components: registry.names(),
    };
    provide_context(caps.clone());

    let mut override_map: Signal<OverrideMap> = use_signal(OverrideMap::default);

    let caps_clone = caps;
    use_resource(move || {
        let caps = caps_clone.clone();
        async move {
            if let Ok(map) = get_override_map(caps).await {
                *override_map.write() = map;
            }
        }
    });

    provide_context(override_map);
    rsx! { {children} }
}

fn use_context_or_default<T: 'static + Clone + Default>() -> T {
    try_use_context::<T>().unwrap_or_default()
}

// ── PluginSlot ────────────────────────────────────────────────────────────────

/// Renders plugin contributions to a named slot.
#[component]
pub fn PluginSlot(
    name: String,
    #[props(default)] loading: Option<Element>,
    #[props(default)] fallback: Option<Element>,
) -> Element {
    let session_id = use_session_id();
    let client_caps = use_context::<ClientCapabilities>();

    let name_clone = name;
    let contents = use_resource(move || {
        let name = name_clone.clone();
        let sid: SessionId = session_id.read().clone();
        let caps = client_caps.clone();
        async move { get_slot_content(name, sid, caps).await }
    });

    match contents.read().as_ref() {
        None => loading.unwrap_or(rsx! {}),
        Some(Ok(c)) if !c.is_empty() => {
            rsx! {
                for content in c.iter().cloned() {
                    PluginViewRenderer {
                        key: "{content.plugin_id.0}",
                        view: content.view,
                        session_id,
                        plugin_id: Some(content.plugin_id),
                    }
                }
            }
        }
        _ => fallback.unwrap_or(rsx! {}),
    }
}

// ── OverridableComponent ──────────────────────────────────────────────────────

/// Wraps a Dioxus component to allow plugin transforms.
///
/// Fast path: if the `OverrideMap` (provided by `PluginBootProvider`) does not list
/// `name` in `overridden_components`, `fallback` renders immediately with zero network
/// overhead. No `use_resource` call and no server function invocation occur.
#[component]
pub fn OverridableComponent(
    name: String,
    props: serde_json::Value,
    fallback: Element,
) -> Element {
    let override_map: Signal<OverrideMap> = use_context::<Signal<OverrideMap>>();
    if !override_map.read().overridden_components.contains(&name) {
        return fallback;
    }
    rsx! {
        OverridableComponentInner { name, props, fallback }
    }
}

/// Inner half of `OverridableComponent` that issues the server call.
///
/// This is a separate component so that `use_resource` is always called in the same
/// hook position — calling it conditionally in the outer component would violate the
/// Dioxus hook ordering rules.
#[component]
fn OverridableComponentInner(
    name: String,
    props: serde_json::Value,
    fallback: Element,
) -> Element {
    let session_id = use_session_id();
    let client_caps = use_context::<ClientCapabilities>();

    let name_clone = name;
    let props_clone = props;
    let resolution = use_resource(move || {
        let n = name_clone.clone();
        let p = props_clone.clone();
        let sid: SessionId = session_id.read().clone();
        let caps = client_caps.clone();
        async move { get_component_resolution(n, p, sid, caps).await }
    });

    match resolution.read().as_ref() {
        None | Some(Err(_) | Ok(None)) => fallback,
        Some(Ok(Some(r))) => {
            let before = r.before.clone();
            let replacement = r.replacement.clone();
            let after = r.after.clone();
            let fallback = fallback;
            rsx! {
                for (i, view) in before.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-b", view, session_id }
                }
                if let Some(repl) = replacement {
                    PluginViewRenderer { view: repl, session_id }
                } else {
                    {fallback}
                }
                for (i, view) in after.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-a", view, session_id }
                }
            }
        }
    }
}

// ── use_current_path ─────────────────────────────────────────────────────────

/// Returns the current URL pathname.
///
/// On wasm32 targets this reads `window.location.pathname` directly.
/// On non-wasm32 targets (desktop, server) it returns `"/"` — route-based
/// transforms are a web-only feature and will not trigger on those targets.
#[must_use] 
pub fn use_current_path() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.location().pathname().ok())
            .unwrap_or_else(|| "/".into())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        "/".into()
    }
}

// ── PluginAwareRouter ─────────────────────────────────────────────────────────

/// Router wrapper that applies plugin route transforms around `Outlet<R>`.
///
/// Fast path: if no route pattern in the `OverrideMap` matches the current path,
/// `Outlet<R>` renders directly without any server call.
/// If patterns match, `PluginAwareRouterInner` issues a server call and applies
/// before/wrap/after views.
#[component]
pub fn PluginAwareRouter<R: Routable + Clone>() -> Element
where
    <R as std::str::FromStr>::Err: std::fmt::Display,
{
    let path = use_current_path();
    let override_map: Signal<OverrideMap> = use_context::<Signal<OverrideMap>>();
    let has_transforms = override_map.read().route_patterns.iter().any(|p| p.matches(&path));

    if has_transforms {
        rsx! {
            PluginAwareRouterInner {
                path,
                outlet: rsx! { Outlet::<R> {} },
            }
        }
    } else {
        rsx! { Outlet::<R> {} }
    }
}

/// Inner half of `PluginAwareRouter` that issues the server call.
///
/// Non-generic: the outer component passes `Outlet<R>` pre-constructed as `outlet`.
/// Kept separate so `use_resource` is always called in the same hook position —
/// calling it conditionally in the outer component would violate Dioxus hook ordering.
#[component]
fn PluginAwareRouterInner(path: String, outlet: Element) -> Element {
    let session_id = use_session_id();
    let client_caps = use_context::<ClientCapabilities>();

    let path_clone = path;
    let transforms: Resource<Result<RouteTransforms, ServerFnError>> = use_resource(move || {
        let p = path_clone.clone();
        let sid: SessionId = session_id.read().clone();
        let caps = client_caps.clone();
        async move { get_route_transforms(p, sid, caps).await }
    });

    match transforms.read().as_ref() {
        Some(Ok(t)) if t.has_wrap() => {
            let before = t.before.clone();
            let wrap = t.wrap.clone().expect("has_wrap is true");
            let after = t.after.clone();
            // When a replacement is present it takes the place of the host outlet inside the wrap.
            let content_slot = if let Some(repl) = t.replacement.clone() {
                rsx! { PluginViewRenderer { view: repl, session_id } }
            } else {
                outlet
            };
            rsx! {
                for (i, view) in before.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-b", view, session_id }
                }
                PluginViewRenderer {
                    view: wrap,
                    session_id,
                    content_slot,
                }
                for (i, view) in after.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-a", view, session_id }
                }
            }
        }
        Some(Ok(t)) if t.has_replacement() => {
            let before = t.before.clone();
            let repl = t.replacement.clone().expect("has_replacement is true");
            let after = t.after.clone();
            rsx! {
                for (i, view) in before.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-b", view, session_id }
                }
                PluginViewRenderer { view: repl, session_id }
                for (i, view) in after.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-a", view, session_id }
                }
            }
        }
        Some(Ok(t)) => {
            let before = t.before.clone();
            let after = t.after.clone();
            rsx! {
                for (i, view) in before.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-b", view, session_id }
                }
                {outlet}
                for (i, view) in after.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-a", view, session_id }
                }
            }
        }
        _ => outlet,
    }
}

// ── PluginPageOutlet ──────────────────────────────────────────────────────────

/// Renders a plugin-declared page for the given relative path.
///
/// Place this inside the component that handles your wildcard route entry
/// (e.g. `#[route("/p/:..segments")]`). Reconstruct the relative path from
/// the route's captured segments and pass it as `relative_path`.
///
/// # Example
/// ```ignore
/// #[component]
/// fn PluginPage(segments: Vec<String>) -> Element {
///     let path = format!("/{}", segments.join("/"));
///     let bypass = use_signal(|| false);
///     rsx! {
///         PluginPageOutlet {
///             relative_path: path,
///             bypass_layout_signal: bypass,
///             not_found: rsx! { p { "Page not found." } },
///         }
///     }
/// }
/// ```
#[component]
pub fn PluginPageOutlet(
    /// Path after the host prefix, e.g. `"/notes"` or `"/notes/42"`.
    relative_path: String,
    /// Element rendered when no plugin has claimed the path (404 case).
    #[props(default)]
    not_found: Option<Element>,
    /// When `Some`, set to the plugin's `bypass_layout` value once the page loads.
    /// Lets the host conditionally skip its layout wrapper.
    #[props(default)]
    bypass_layout_signal: Option<Signal<bool>>,
) -> Element {
    let session_id = use_session_id();
    let client_caps = use_context::<ClientCapabilities>();

    let page = use_resource(move || {
        let path = relative_path.clone();
        let sid: SessionId = session_id.read().clone();
        let caps = client_caps.clone();
        async move { get_plugin_page(path, sid, caps).await }
    });

    match page.read().as_ref() {
        None => rsx! {},
        Some(Err(e)) => rsx! {
            p { class: "plugin-page-error", "Plugin page error: {e}" }
        },
        Some(Ok(None)) => not_found.unwrap_or_else(|| rsx! {}),
        Some(Ok(Some(output))) => {
            if let Some(mut sig) = bypass_layout_signal {
                sig.set(output.bypass_layout);
            }
            rsx! {
                PluginViewRenderer { view: output.view.clone(), session_id }
            }
        }
    }
}

// ── PluginViewRenderer ────────────────────────────────────────────────────────

/// Renders a `PluginView` tree into Dioxus elements.
///
/// The first (root) `PluginViewRenderer` in a given subtree creates a `view_signal`
/// and provides `PluginInteractionCtx` so nested interactive elements (buttons, inputs)
/// can update the view in place without prop drilling.  Subsequent nested calls detect
/// that they are already inside a tree (via `InsidePluginTree` context) and skip
/// creating a new signal or overriding the context.
#[component]
pub fn PluginViewRenderer(
    view: PluginView,
    session_id: Signal<SessionId>,
    #[props(default)] content_slot: Option<Element>,
    #[props(default)] plugin_id: Option<PluginId>,
) -> Element {
    // Always create a view signal so hooks are called unconditionally.
    let view_signal = use_signal(|| view.clone());
    let caps = try_use_context::<ClientCapabilities>().unwrap_or_default();

    // Only the outermost PluginViewRenderer becomes the interaction context provider.
    // Children inherit the root's context instead of creating a new one.
    let already_inside = try_use_context::<InsidePluginTree>().is_some();
    if !already_inside {
        provide_context(InsidePluginTree);
        provide_context(PluginInteractionCtx {
            plugin_id,
            session_id,
            view_signal,
            caps,
        });
    }

    // Root reads from the signal so interactions can update it.
    // Children render their passed `view` directly (they're part of the root's tree).
    let render_view = if already_inside { view } else { view_signal.read().clone() };

    match render_view {
        PluginView::Empty => rsx! {},
        PluginView::Text(t) => rsx! { "{t}" },
        PluginView::Fragment(children) => rsx! {
            for (i, child) in children.into_iter().enumerate() {
                PluginViewRenderer {
                    key: "{i}",
                    view: child,
                    session_id,
                    content_slot: content_slot.clone(),
                }
            }
        },
        PluginView::Element(el) => render_element(el, session_id, content_slot),
        PluginView::HostComponent(r) => render_host_component(r, session_id, content_slot),
        PluginView::Incompatible { reason, .. } => rsx! {
            div {
                class: "plugin-incompatible",
                "Plugin requires a newer version. ({reason})"
            }
        },
        _ => rsx! {},
    }
}

// ── Interactive sub-components ────────────────────────────────────────────────

/// A button rendered by a plugin view that calls `handle_plugin_interaction` on click.
#[component]
fn InteractiveButton(
    class: String,
    id: String,
    handler_id: HandlerId,
    children: Element,
) -> Element {
    let ctx = try_use_context::<PluginInteractionCtx>();
    rsx! {
        button {
            class: "{class}",
            id: "{id}",
            onclick: move |_| {
                let ctx = ctx.clone();
                let hid = handler_id.clone();
                spawn(async move {
                    let Some(mut ctx) = ctx else { return };
                    let Some(pid) = ctx.plugin_id.clone() else { return };
                    let sid = ctx.session_id.read().clone();
                    match handle_plugin_interaction(
                        pid.0,
                        hid.0.clone(),
                        serde_json::Value::Null,
                        sid,
                        ctx.caps.clone(),
                    )
                    .await
                    {
                        Ok(ViewUpdate { view: Some(new_view), .. }) => {
                            *ctx.view_signal.write() = new_view;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!("plugin interaction failed: {e}");
                        }
                    }
                });
            },
            {children}
        }
    }
}

/// An input rendered by a plugin view that fires `handle_plugin_interaction` on each keystroke.
///
/// The event data sent to the plugin is `{"value": "<current input value>"}`.
/// The view is not updated (the plugin stores the draft in session state instead).
#[component]
fn InteractiveInput(
    class: String,
    id: String,
    placeholder: String,
    value: String,
    handler_id: HandlerId,
) -> Element {
    let ctx = try_use_context::<PluginInteractionCtx>();
    rsx! {
        input {
            class: "{class}",
            id: "{id}",
            placeholder: "{placeholder}",
            value: "{value}",
            oninput: move |e| {
                let ctx = ctx.clone();
                let hid = handler_id.clone();
                let val = e.value();
                spawn(async move {
                    let Some(ctx) = ctx else { return };
                    let Some(pid) = ctx.plugin_id.clone() else { return };
                    let sid = ctx.session_id.read().clone();
                    let event_data = serde_json::json!({"value": val});
                    if let Err(e) = handle_plugin_interaction(
                        pid.0,
                        hid.0.clone(),
                        event_data,
                        sid,
                        ctx.caps.clone(),
                    )
                    .await
                    {
                        tracing::error!("plugin input interaction failed: {e}");
                    }
                });
            },
        }
    }
}

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn render_element(el: ViewElement, session_id: Signal<SessionId>, content_slot: Option<Element>) -> Element {
    let ViewElement { tag, attrs, handlers, children, .. } = el;

    let str_attr = |name: &str| -> String {
        attrs
            .iter()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| if let AttrValue::String(s) = v { Some(s.clone()) } else { None })
            .unwrap_or_default()
    };

    let class = str_attr("class");
    let id = str_attr("id");
    let value = str_attr("value");
    let placeholder = str_attr("placeholder");

    let click_handler: Option<HandlerId> = handlers
        .iter()
        .find(|h| matches!(h.event, DomEvent::Click))
        .map(|h| h.handler_id.clone());

    let input_handler: Option<HandlerId> = handlers
        .iter()
        .find(|h| matches!(h.event, DomEvent::Input))
        .map(|h| h.handler_id.clone());

    let children_views = children;

    match tag.as_str() {
        "button" => {
            if let Some(hid) = click_handler {
                rsx! {
                    InteractiveButton {
                        class,
                        id,
                        handler_id: hid,
                        for (i, child) in children_views.into_iter().enumerate() {
                            PluginViewRenderer { key: "{i}", view: child, session_id }
                        }
                    }
                }
            } else {
                rsx! {
                    button {
                        class: "{class}",
                        id: "{id}",
                        for (i, child) in children_views.into_iter().enumerate() {
                            PluginViewRenderer {
                                key: "{i}",
                                view: child,
                                session_id,
                                content_slot: content_slot.clone(),
                            }
                        }
                    }
                }
            }
        }
        "input" => {
            if let Some(hid) = input_handler {
                rsx! {
                    InteractiveInput {
                        class,
                        id,
                        placeholder,
                        value,
                        handler_id: hid,
                    }
                }
            } else {
                rsx! {
                    input {
                        class: "{class}",
                        id: "{id}",
                        placeholder: "{placeholder}",
                        value: "{value}",
                    }
                }
            }
        }
        "div" => rsx! {
            div {
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
        "span" => rsx! {
            span {
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
        "p" => rsx! {
            p {
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
        "h1" => rsx! {
            h1 {
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
        "h2" => rsx! {
            h2 {
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
        "h3" => rsx! {
            h3 {
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
        "ul" => rsx! {
            ul {
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
        "li" => rsx! {
            li {
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
        _ => rsx! {
            div {
                "data-tag": "{tag}",
                class: "{class}",
                id: "{id}",
                for (i, child) in children_views.into_iter().enumerate() {
                    PluginViewRenderer {
                        key: "{i}",
                        view: child,
                        session_id,
                        content_slot: content_slot.clone(),
                    }
                }
            }
        },
    }
}

// ── SSR components ────────────────────────────────────────────────────────────

/// Provides pre-fetched `SsrRouteOutput` as context for SSR rendering.
///
/// Wrap your SSR route handler's render tree with this component after calling
/// `PluginRuntime::ssr_render_route`. Child components can then use `PluginSlotSsr`
/// to render slot content without additional async calls.
#[component]
pub fn SsrPluginDataProvider(output: SsrRouteOutput, children: Element) -> Element {
    provide_context(output);
    rsx! { {children} }
}

/// Renders a named slot from pre-fetched SSR data.
///
/// Must be used inside a `SsrPluginDataProvider`. Falls back to an empty element
/// if the slot has no contributions or the SSR context is not available.
#[component]
pub fn PluginSlotSsr(name: String) -> Element {
    let output = try_use_context::<SsrRouteOutput>();
    let session_id = use_session_id();

    let contents: Vec<SlotContent> = output
        .and_then(|o| o.slots.get(&name).cloned())
        .unwrap_or_default();

    rsx! {
        for content in contents {
            PluginViewRenderer {
                key: "{content.plugin_id.0}",
                view: content.view,
                session_id,
                content_slot: None,
            }
        }
    }
}

// ── use_plugin_state ──────────────────────────────────────────────────────────

/// Subscribe to one key in a plugin's session state.
///
/// Fetches the value via a server function and returns a `ReadOnlySignal<Option<T>>`.
/// The signal is `None` while loading, or if the key is absent or deserialisation fails.
pub fn use_plugin_state<T>(
    plugin_id: impl Into<String>,
    key: impl Into<String>,
) -> ReadSignal<Option<T>>
where
    T: serde::de::DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    let pid = plugin_id.into();
    let key = key.into();
    let session_id = use_session_id();

    let resource = use_resource(move || {
        let pid = pid.clone();
        let key = key.clone();
        let sid = session_id.read().clone();
        async move {
            server_get_plugin_state(pid, key, sid)
                .await
                .ok()
                .flatten()
                .and_then(|v| serde_json::from_value::<T>(v).ok())
        }
    });

    use_memo(move || resource.read().as_ref().and_then(Clone::clone)).into()
}

fn render_host_component(
    r: HostComponentRef,
    session_id: Signal<SessionId>,
    content_slot: Option<Element>,
) -> Element {
    if r.name == "__content__" {
        return content_slot.unwrap_or(rsx! {});
    }

    let registry = try_use_context::<HostComponentRegistry>().unwrap_or_default();

    if let Some(el) = registry.render(&r.name, r.clone()) {
        return el;
    }

    if r.children.is_empty() {
        rsx! {}
    } else {
        rsx! {
            for (i, child) in r.children.into_iter().enumerate() {
                PluginViewRenderer {
                    key: "{i}",
                    view: child,
                    session_id,
                    content_slot: None,
                }
            }
        }
    }
}
