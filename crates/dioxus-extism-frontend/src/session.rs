use dioxus::prelude::*;
use dioxus_extism_protocol::SessionId;
use uuid::Uuid;

/// Provides a `Signal<SessionId>` in context; use at the application root.
#[component]
pub fn SessionProviderRoot<P: 'static + Clone + PartialEq>(
    provider: P,
    children: Element,
) -> Element {
    let _ = provider;
    let session_id: Signal<SessionId> = use_signal(|| SessionId(Uuid::new_v4().to_string()));
    provide_context(session_id);
    rsx! { {children} }
}

/// Session provider for web (WASM) targets — persists session ID in `localStorage`.
#[derive(Clone, PartialEq, Eq)]
pub struct WebSessionProvider;

/// Session provider for desktop targets — persists session ID on the filesystem.
#[derive(Clone, PartialEq, Eq)]
pub struct DesktopSessionProvider;

/// Read the current `SessionId` from context.
pub fn use_session_id() -> Signal<SessionId> {
    use_context::<Signal<SessionId>>()
}
