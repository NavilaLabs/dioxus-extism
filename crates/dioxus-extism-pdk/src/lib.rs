mod error;
mod view;

/// Re-exported so macros defined in this crate can refer to it via `$crate::extism_pdk`.
#[doc(hidden)]
pub use extism_pdk;

pub use dioxus_extism_protocol::{
    ClientCapabilities, HandlerId, HookCall, HookResult, HostCapability, HostComponentRef,
    PluginEvent, PluginId, PluginManifest, PluginView, PriorityHint, RoutePattern, Selector,
    SessionCtx, SessionId, SlotContent, SlotRegistration, StateScope, TransformDeclaration,
    TransformInput, TransformOp, TransformOutput, ViewUpdate,
};
pub use error::PdkError;
pub use view::{div, element, incompatible, original_content, span, text, ViewBuilder};

/// Prelude for plugin authors — import everything with one `use`.
pub mod prelude {
    pub use crate::{
        ClientCapabilities, DioxusPlugin, EventSubscriber, HandlerId, HookCall, HookHandler,
        HookResult, HostCapability, HostComponentRef, InteractionHandler, OnLoad, OnUnload,
        PdkError, PluginCtx, PluginEvent, PluginId, PluginManifest, PluginView, PriorityHint,
        RoutePattern, Selector, SessionCtx, SessionId, SlotContent, SlotRegistration, SlotProvider,
        StateScope, TransformDeclaration, TransformInput, TransformOp, TransformOutput,
        TransformProvider, ViewUpdate, div, element, incompatible, original_content, span, text,
    };
}

// ── Core traits ───────────────────────────────────────────────────────────────

/// Implemented by every plugin struct to declare its manifest.
pub trait DioxusPlugin {
    fn manifest() -> PluginManifest;
}

/// Implemented by a plugin to contribute content to a named slot.
pub trait SlotProvider: DioxusPlugin {
    const SLOT_NAME: &'static str;
    /// # Errors
    /// Returns `PdkError` if the plugin cannot render the view.
    fn render(ctx: &PluginCtx) -> Result<PluginView, PdkError>;
}

/// Implemented by a plugin to intercept a named hook.
pub trait HookHandler: DioxusPlugin {
    const HOOK_NAME: &'static str;
    /// # Errors
    /// Returns `PdkError` if the hook handler fails.
    fn handle(call: HookCall, ctx: &PluginCtx) -> Result<HookResult, PdkError>;
}

/// Implemented by a plugin to subscribe to named events.
pub trait EventSubscriber: DioxusPlugin {
    /// # Errors
    /// Returns `PdkError` if the event handler fails.
    fn on_event(event: PluginEvent, ctx: &PluginCtx) -> Result<(), PdkError>;
}

/// Implemented by a plugin to handle UI interactions.
pub trait InteractionHandler: DioxusPlugin {
    /// # Errors
    /// Returns `PdkError` if the interaction handler fails.
    fn on_interaction(
        handler_id: HandlerId,
        event_data: serde_json::Value,
        ctx: &PluginCtx,
    ) -> Result<ViewUpdate, PdkError>;
}

/// Optional lifecycle: called once after pool initialisation.
pub trait OnLoad: DioxusPlugin {
    /// # Errors
    /// Returns `PdkError` if initialisation fails.
    fn on_load(ctx: &PluginCtx) -> Result<(), PdkError>;
}

/// Optional lifecycle: called before pool drop.
pub trait OnUnload: DioxusPlugin {
    /// # Errors
    /// Returns `PdkError` if cleanup fails.
    fn on_unload() -> Result<(), PdkError>;
}

/// Implemented to provide route/slot/component transforms.
pub trait TransformProvider: DioxusPlugin {
    /// # Errors
    /// Returns `PdkError` if the transform fails.
    fn transform(input: TransformInput, ctx: &PluginCtx) -> Result<TransformOutput, PdkError>;
}

// ── Plugin context ────────────────────────────────────────────────────────────

/// Runtime context available inside every plugin call.
pub struct PluginCtx {
    pub state: StateAccessor,
    pub emit: EventEmitter,
    pub invoke: InvocationAccessor,
    pub session: SessionCtx,
    pub client: ClientCapabilities,
}

impl PluginCtx {
    /// Construct from the session context received on each call.
    #[must_use] 
    pub fn from_session(session: SessionCtx) -> Self {
        let client = session.client.clone();
        Self {
            state: StateAccessor,
            emit: EventEmitter,
            invoke: InvocationAccessor,
            client,
            session,
        }
    }
}

/// Read/write per-session or global state via host functions.
pub struct StateAccessor;

/// Emit events via the host event bus.
pub struct EventEmitter;

/// Call named host-side invocations.
pub struct InvocationAccessor;

// ── Host function imports ─────────────────────────────────────────────────────

#[allow(unsafe_code)]
pub mod host_fns {
    use extism_pdk::host_fn;

    #[host_fn]
    extern "ExtismHost" {
        pub fn dx_state_get(key: &str) -> String;
        pub fn dx_state_set(key: &str, value: String);
        pub fn dx_state_delete(key: &str);
        pub fn dx_global_state_get(key: &str) -> String;
        pub fn dx_global_state_set(key: &str, value: String);
        pub fn dx_emit_event(event: String);
        pub fn dx_invoke(name: &str, args: String) -> String;
        pub fn dx_log(level: &str, message: &str);
    }
}

/// Macro to wire up WASM exports for a `DioxusPlugin` type.
///
/// Generates `manifest()` and one export per slot provider, named
/// `slot_<N>` where N is the 0-based index. The host matches exports
/// to slot names via the manifest at load time (Phase 2).
///
/// # Example
/// ```ignore
/// plugin! { type: HelloPlugin, slots: [HelloPlugin] }
/// ```
#[macro_export]
macro_rules! plugin {
    (type: $plugin:ty, slots: [$($slot_impl:ty),* $(,)?]) => {
        #[::extism_pdk::plugin_fn]
        pub fn manifest()
            -> ::extism_pdk::FnResult<::extism_pdk::Json<$crate::PluginManifest>>
        {
            Ok(::extism_pdk::Json(
                <$plugin as $crate::DioxusPlugin>::manifest()
            ))
        }

        $crate::__plugin_slots_inner!(0usize, $($slot_impl,)*);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __plugin_slots_inner {
    // base case
    ($n:expr,) => {};
    // recursive: consume first slot type
    ($n:expr, $first:ty, $($rest:ty,)*) => {
        $crate::__slot_fn!($first);
        $crate::__plugin_slots_inner!($n + 1usize, $($rest,)*);
    };
}

/// Generate a WASM export for a single `SlotProvider` implementation.
///
/// The export is named `slot_render` — Phase 1 only supports one slot per plugin.
/// Phase 2+ will generate per-name exports via a proc-macro.
#[doc(hidden)]
#[macro_export]
macro_rules! __slot_fn {
    ($slot_impl:ty) => {
        #[::extism_pdk::plugin_fn]
        pub fn slot_render(
            input: ::extism_pdk::Json<$crate::SessionCtx>,
        ) -> ::extism_pdk::FnResult<::extism_pdk::Json<$crate::PluginView>> {
            let ctx = $crate::PluginCtx::from_session(input.0);
            let view = <$slot_impl as $crate::SlotProvider>::render(&ctx)
                .map_err(|e| ::extism_pdk::Error::msg(e.to_string()))?;
            Ok(::extism_pdk::Json(view))
        }
    };
}
