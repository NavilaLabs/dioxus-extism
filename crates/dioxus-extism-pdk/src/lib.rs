//! Plugin Development Kit for `dioxus-extism`.
//!
//! Provides the traits, macros, and view-builder DSL that plugin authors use to write
//! Extism WASM plugins for Dioxus fullstack applications.
//!
//! # Quick start
//!
//! ```ignore
//! use dioxus_extism_pdk::prelude::*;
//! use dioxus_extism_pdk::plugin;
//!
//! struct MyPlugin;
//!
//! impl DioxusPlugin for MyPlugin {
//!     fn manifest() -> PluginManifest {
//!         PluginManifest {
//!             id: PluginId("my-org/my-plugin".into()),
//!             version: "0.1.0".into(),
//!             slots: vec![SlotRegistration {
//!                 name: "sidebar".into(),
//!                 priority_hint: PriorityHint::Normal,
//!             }],
//!             ..Default::default()
//!         }
//!     }
//! }
//!
//! impl SlotProvider for MyPlugin {
//!     const SLOT_NAME: &'static str = "sidebar";
//!     fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
//!         Ok(div().class("widget").child(text("Hello from plugin!")).build())
//!     }
//! }
//!
//! plugin! { type: MyPlugin, slots: [MyPlugin] }
//! ```

mod error;
mod view;

/// Re-exported so macros defined in this crate can refer to it via `$crate::extism_pdk`.
#[doc(hidden)]
pub use extism_pdk;

pub use dioxus_extism_protocol::{
    ApiRequest, ApiResponse, ApiRouteDeclaration, HttpMethod,
    PageRouteDeclaration, PageRouteInput, PageRouteOutput,
    AttrValue, BoundEventHandler, ClientCapabilities, DomEvent, HandlerId, HookCall, HookRegistration, HookResult,
    HostCapability, HostComponentRef, NodeSelector, PluginEvent, PluginId, PluginManifest,
    PluginView, PriorityHint, RoutePattern, Selector, SessionCtx, SessionId, SlotContent,
    SlotRegistration, StateScope, TransformDeclaration, TransformInput, TransformOp,
    TransformOutput, ViewElement, ViewUpdate,
};
pub use error::PdkError;
pub use view::{
    a, button, div, element, form, fragment, h1, h2, h3, host, img, incompatible,
    incompatible_with_fallback, input, label, li, ol, original_content, original_target,
    p, span, text, ul, HostComponentBuilder, ViewBuilder,
};

/// Prelude for plugin authors — import everything with one `use`.
pub mod prelude {
    pub use crate::{
        ApiRequest, ApiResponse, ApiRouteDeclaration, HttpMethod,
        PageRouteDeclaration, PageRouteInput, PageRouteOutput,
        AttrValue, BoundEventHandler, ClientCapabilities, DioxusPlugin, DomEvent, EventSubscriber,
        HandlerId, HookCall, HookHandler, HookRegistration, HookResult, HostCapability, HostComponentRef,
        InteractionHandler, NodeSelector, OnLoad, OnUnload, PdkError, PluginCtx, PluginEvent,
        PluginId, PluginManifest, PluginView, PriorityHint, RoutePattern, Selector, SessionCtx,
        SessionId, SlotContent, SlotRegistration, SlotProvider, StateScope, TransformDeclaration,
        TransformInput, TransformOp, TransformOutput, TransformProvider, ViewElement, ViewUpdate,
        a, button, div, element, form, fragment, h1, h2, h3, host, img, incompatible,
        incompatible_with_fallback, input, label, li, ol, original_content, original_target,
        p, span, text, ul, HostComponentBuilder, ViewBuilder,
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
        pub fn dx_plugin_state_get(target_plugin_id: &str, key: &str) -> String;
    }
}

/// Wire up WASM exports for a `DioxusPlugin` type.
///
/// Generates a `manifest()` export and one `slot_render` export per slot provider.
/// For hooks, transforms, events, interactions, and lifecycle hooks use the
/// dedicated standalone macros: [`hook_export!`], [`transform_export!`],
/// [`events_export!`], [`interactions_export!`], [`on_load_export!`], [`on_unload_export!`].
///
/// # Example
/// ```ignore
/// plugin! { type: HelloPlugin, slots: [HelloPlugin] }
///
/// // Optional additional exports (call after plugin!):
/// hook_export!(HelloPlugin, before_save);
/// transform_export!(HelloPlugin, wrap_header);
/// on_load_export!(HelloPlugin);
/// events_export!(HelloPlugin);
/// interactions_export!(HelloPlugin);
/// ```
#[macro_export]
macro_rules! plugin {
    (type: $plugin:ty, slots: [$($slot_impl:ty),* $(,)?] $(,)?) => {
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
    (type: $plugin:ty $(,)?) => {
        #[::extism_pdk::plugin_fn]
        pub fn manifest()
            -> ::extism_pdk::FnResult<::extism_pdk::Json<$crate::PluginManifest>>
        {
            Ok(::extism_pdk::Json(
                <$plugin as $crate::DioxusPlugin>::manifest()
            ))
        }
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

/// Generate a named WASM export that handles an API route.
///
/// The generated function receives `Json<ApiRequest>` and returns `Json<ApiResponse>`.
/// The export name must match the `handler_fn` field in `ApiRouteDeclaration`.
///
/// # Example
/// ```ignore
/// api_route_fn!(handle_get_notes, |req: ApiRequest| {
///     Ok(ApiResponse { status: 200, body: Some(serde_json::json!([])), ..Default::default() })
/// });
/// ```
#[macro_export]
macro_rules! api_route_fn {
    ($export_name:ident, $handler:expr) => {
        #[::extism_pdk::plugin_fn]
        pub fn $export_name(
            input: ::extism_pdk::Json<$crate::ApiRequest>,
        ) -> ::extism_pdk::FnResult<::extism_pdk::Json<$crate::ApiResponse>> {
            let result: Result<$crate::ApiResponse, $crate::PdkError> = ($handler)(input.0);
            let resp = result.map_err(|e| ::extism_pdk::Error::msg(e.to_string()))?;
            Ok(::extism_pdk::Json(resp))
        }
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

// ── Standalone export macros ─────────────────────────────────────────────────

/// Generate a WASM hook export for a [`HookHandler`] implementation.
///
/// Pass the full export function name — by convention, `hook_<hook_name>` to match what
/// the host runtime looks for based on the `HookRegistration::hook_name` in the manifest.
/// The host calls this export with `(HookCall, SessionCtx)` and expects `HookResult`.
///
/// # Example
/// ```ignore
/// hook_export!(MyPlugin, hook_before_save);
/// ```
#[macro_export]
macro_rules! hook_export {
    ($plugin:ty, $fn_name:ident) => {
        #[::extism_pdk::plugin_fn]
        pub fn $fn_name(
            input: ::extism_pdk::Json<($crate::HookCall, $crate::SessionCtx)>,
        ) -> ::extism_pdk::FnResult<::extism_pdk::Json<$crate::HookResult>> {
            let (call, session) = input.0;
            let ctx = $crate::PluginCtx::from_session(session);
            let result = <$plugin as $crate::HookHandler>::handle(call, &ctx)
                .map_err(|e| ::extism_pdk::Error::msg(e.to_string()))?;
            Ok(::extism_pdk::Json(result))
        }
    };
}

/// Generate a WASM transform export for a [`TransformProvider`] implementation.
///
/// Pass the full export function name — by convention, `transform_<fn_name>` to match what
/// the host runtime looks for based on `TransformDeclaration::transform_fn` in the manifest.
/// The host calls this export with [`TransformInput`] and expects [`TransformOutput`].
///
/// # Example
/// ```ignore
/// transform_export!(MyPlugin, transform_wrap_header);
/// ```
#[macro_export]
macro_rules! transform_export {
    ($plugin:ty, $fn_name:ident) => {
        #[::extism_pdk::plugin_fn]
        pub fn $fn_name(
            input: ::extism_pdk::Json<$crate::TransformInput>,
        ) -> ::extism_pdk::FnResult<::extism_pdk::Json<$crate::TransformOutput>> {
            let session = input.0.session.clone();
            let ctx = $crate::PluginCtx::from_session(session);
            let result = <$plugin as $crate::TransformProvider>::transform(input.0, &ctx)
                .map_err(|e| ::extism_pdk::Error::msg(e.to_string()))?;
            Ok(::extism_pdk::Json(result))
        }
    };
}

/// Generate a WASM `on_event` export for an [`EventSubscriber`] implementation.
///
/// The host calls this export with `(PluginEvent, SessionCtx)`.
///
/// # Example
/// ```ignore
/// events_export!(MyPlugin);
/// ```
#[macro_export]
macro_rules! events_export {
    ($plugin:ty) => {
        #[::extism_pdk::plugin_fn]
        pub fn on_event(
            input: ::extism_pdk::Json<($crate::PluginEvent, $crate::SessionCtx)>,
        ) -> ::extism_pdk::FnResult<()> {
            let (event, session) = input.0;
            let ctx = $crate::PluginCtx::from_session(session);
            <$plugin as $crate::EventSubscriber>::on_event(event, &ctx)
                .map_err(|e| ::extism_pdk::Error::msg(e.to_string()))
        }
    };
}

/// Generate a WASM `on_interaction` export for an [`InteractionHandler`] implementation.
///
/// The host calls this export with `(HandlerId, serde_json::Value, SessionCtx)`.
///
/// # Example
/// ```ignore
/// interactions_export!(MyPlugin);
/// ```
#[macro_export]
macro_rules! interactions_export {
    ($plugin:ty) => {
        #[::extism_pdk::plugin_fn]
        pub fn on_interaction(
            input: ::extism_pdk::Json<($crate::HandlerId, ::serde_json::Value, $crate::SessionCtx)>,
        ) -> ::extism_pdk::FnResult<::extism_pdk::Json<$crate::ViewUpdate>> {
            let (handler_id, event_data, session) = input.0;
            let ctx = $crate::PluginCtx::from_session(session);
            let result = <$plugin as $crate::InteractionHandler>::on_interaction(
                handler_id,
                event_data,
                &ctx,
            )
            .map_err(|e| ::extism_pdk::Error::msg(e.to_string()))?;
            Ok(::extism_pdk::Json(result))
        }
    };
}

/// Generate a WASM `on_load` export for an [`OnLoad`] implementation.
///
/// The host calls this once per pool initialisation. If it returns an error,
/// the plugin fails to load.
///
/// # Example
/// ```ignore
/// on_load_export!(MyPlugin);
/// ```
#[macro_export]
macro_rules! on_load_export {
    ($plugin:ty) => {
        #[::extism_pdk::plugin_fn]
        pub fn on_load(
            input: ::extism_pdk::Json<$crate::SessionCtx>,
        ) -> ::extism_pdk::FnResult<()> {
            let ctx = $crate::PluginCtx::from_session(input.0);
            <$plugin as $crate::OnLoad>::on_load(&ctx)
                .map_err(|e| ::extism_pdk::Error::msg(e.to_string()))
        }
    };
}

/// Generate a WASM `on_unload` export for an [`OnUnload`] implementation.
///
/// The host calls this before dropping the instance pool.
///
/// # Example
/// ```ignore
/// on_unload_export!(MyPlugin);
/// ```
#[macro_export]
macro_rules! on_unload_export {
    ($plugin:ty) => {
        #[::extism_pdk::plugin_fn]
        pub fn on_unload() -> ::extism_pdk::FnResult<()> {
            <$plugin as $crate::OnUnload>::on_unload()
                .map_err(|e| ::extism_pdk::Error::msg(e.to_string()))
        }
    };
}
