mod components;
mod server_fns;
mod session;

pub use components::{
    HostComponentRegistry, OverridableComponent, PluginAwareRouter, PluginBootProvider,
    PluginPageOutlet, PluginSlot, PluginSlotSsr, PluginViewRenderer, SsrPluginDataProvider,
    use_current_path, use_plugin_state,
};
pub use server_fns::{get_plugin_page, handle_plugin_interaction};
#[cfg(not(target_arch = "wasm32"))]
pub use session::MobileSessionProvider;
pub use session::{
    DesktopSessionProvider, SessionProviderRoot, WebSessionProvider, use_session_id,
};

pub use dioxus_extism_protocol::{PROTOCOL_VERSION, RouteTransforms};

/// Convenience alias: the type hosts pass to `use_context_provider` so that
/// `PluginSlot`, `OverridableComponent`, and `PluginAwareRouter` can read it.
///
/// ```ignore
/// use_context_provider(|| HostCtxRef::new(MyHostCtx { /* ... */ }));
/// ```
pub type HostCtxRef<HostCtx> = std::sync::Arc<HostCtx>;
