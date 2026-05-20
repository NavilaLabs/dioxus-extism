use std::collections::HashMap;

use dioxus::prelude::*;
use dioxus_extism_protocol::{
    AttrValue, ClientCapabilities, HostComponentRef, OverrideMap, PluginView, RouteTransforms,
    SessionId, ViewElement, PROTOCOL_VERSION,
};

use crate::server_fns::{
    get_component_resolution, get_override_map, get_route_transforms, get_slot_content,
};
use crate::session::use_session_id;

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
            rsx! {
                for (i, view) in before.into_iter().enumerate() {
                    PluginViewRenderer { key: "{i}-b", view, session_id }
                }
                PluginViewRenderer {
                    view: wrap,
                    session_id,
                    content_slot: outlet,
                }
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

// ── PluginViewRenderer ────────────────────────────────────────────────────────

/// Renders a `PluginView` tree into Dioxus elements.
#[component]
pub fn PluginViewRenderer(
    view: PluginView,
    session_id: Signal<SessionId>,
    #[props(default)] content_slot: Option<Element>,
) -> Element {
    match view {
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

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn render_element(el: ViewElement, session_id: Signal<SessionId>, content_slot: Option<Element>) -> Element {
    // Build attributes as a string map — Dioxus requires static attribute names,
    // so we emit a data-encoded element for the generic case and handle common
    // tags specially. For Phase 1 we emit a div with class forwarded.
    let class = el
        .attrs
        .iter()
        .find(|(k, _)| k == "class")
        .and_then(|(_, v)| {
            if let AttrValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let id = el
        .attrs
        .iter()
        .find(|(k, _)| k == "id")
        .and_then(|(_, v)| {
            if let AttrValue::String(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let children_views: Vec<PluginView> = el.children;

    match el.tag.as_str() {
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
        _ => rsx! {
            div {
                "data-tag": "{el.tag}",
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
