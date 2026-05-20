/// Test utilities for `dioxus-extism` plugin authors.
///
/// Provides a synchronous `TestRuntime` wrapper, a `MockSession` builder, and
/// the `assert_view!` macro for structural `PluginView` assertions.
use std::sync::Arc;

use dioxus_extism_host::{
    HookOutcome, PluginRuntime, PluginRuntimeBuilder, PluginRuntimeError,
    PluginSource,
};
use dioxus_extism_protocol::{
    ClientCapabilities, HandlerId, PluginEvent, PluginId, PluginView, SessionCtx, SessionId,
    SlotContent, ViewUpdate, PROTOCOL_VERSION,
};

pub use dioxus_extism_host::{PluginInstallConfig, PluginRuntimeExt};
pub use dioxus_extism_protocol;

// ── MockSession ───────────────────────────────────────────────────────────────

/// A builder for `SessionCtx` values used in tests.
#[derive(Debug, Clone)]
pub struct MockSession {
    pub session_id: SessionId,
    pub user_id: Option<String>,
    pub protocol_version: u32,
    pub app_version: u32,
    pub registered_host_components: Vec<String>,
}

impl Default for MockSession {
    fn default() -> Self {
        Self {
            session_id: SessionId("test-session".into()),
            user_id: None,
            protocol_version: PROTOCOL_VERSION,
            app_version: 0,
            registered_host_components: vec![],
        }
    }
}

impl MockSession {
    /// Create a session with default test values.
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the session ID.
    #[must_use]
    pub fn with_session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = SessionId(id.into());
        self
    }

    /// Set a user ID.
    #[must_use]
    pub fn with_user_id(mut self, id: impl Into<String>) -> Self {
        self.user_id = Some(id.into());
        self
    }

    /// Override the reported protocol version.
    #[must_use]
    pub const fn with_protocol_version(mut self, v: u32) -> Self {
        self.protocol_version = v;
        self
    }

    /// Override the reported app version.
    #[must_use]
    pub const fn with_app_version(mut self, v: u32) -> Self {
        self.app_version = v;
        self
    }

    /// Add a registered host component name.
    #[must_use]
    pub fn with_host_component(mut self, name: impl Into<String>) -> Self {
        self.registered_host_components.push(name.into());
        self
    }

    /// Convert to `SessionCtx` for use in runtime calls.
    #[must_use] 
    pub fn as_ctx(&self) -> SessionCtx {
        SessionCtx {
            session_id: self.session_id.clone(),
            user_id: self.user_id.clone(),
            client: ClientCapabilities {
                protocol_version: self.protocol_version,
                app_version: self.app_version,
                registered_host_components: self.registered_host_components.clone(),
            },
            caller: None,
        }
    }
}

// ── TestRuntime ───────────────────────────────────────────────────────────────

/// Synchronous wrapper around `PluginRuntime` for use in test code.
///
/// Owns a `tokio::Runtime` so tests don't need to be async.
pub struct TestRuntime {
    runtime: Arc<PluginRuntime>,
    rt: tokio::runtime::Runtime,
}

impl TestRuntime {
    /// Build a `TestRuntime` from raw WASM bytes or file/URL sources.
    ///
    /// # Errors
    ///
    /// Returns an error if any plugin fails to load.
    ///
    /// # Panics
    ///
    /// Panics if the tokio runtime cannot be created.
    pub fn build(plugins: Vec<PluginSource>) -> Result<Self, PluginRuntimeError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        let mut builder = PluginRuntimeBuilder::new();
        for source in plugins {
            builder = builder.add_plugin(source);
        }
        let runtime = rt.block_on(builder.build())?;
        Ok(Self { runtime, rt })
    }

    /// Build with a custom `PluginRuntimeBuilder`, allowing invocation registration.
    ///
    /// # Errors
    ///
    /// Returns an error if any plugin fails to load.
    ///
    /// # Panics
    ///
    /// Panics if the tokio runtime cannot be created.
    pub fn build_with(builder: PluginRuntimeBuilder) -> Result<Self, PluginRuntimeError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");
        let runtime = rt.block_on(builder.build())?;
        Ok(Self { runtime, rt })
    }

    /// Render a slot and return all contributions.
    ///
    /// # Errors
    ///
    /// Propagates `PluginRuntimeError` from the runtime.
    pub fn render_slot(
        &self,
        slot_name: &str,
        session: &MockSession,
    ) -> Result<Vec<SlotContent>, PluginRuntimeError> {
        let ctx = session.as_ctx();
        self.rt.block_on(self.runtime.render_slot(slot_name, &ctx))
    }

    /// Render one plugin's slot contribution, failing if the plugin is absent.
    ///
    /// # Errors
    ///
    /// Returns `PluginRuntimeError::PluginNotFound` if the plugin did not contribute.
    pub fn call_slot(
        &self,
        plugin_id: &PluginId,
        slot_name: &str,
        session: &MockSession,
    ) -> Result<PluginView, PluginRuntimeError> {
        let contents = self.render_slot(slot_name, session)?;
        contents
            .into_iter()
            .find(|c| &c.plugin_id == plugin_id)
            .map(|c| c.view)
            .ok_or_else(|| PluginRuntimeError::PluginNotFound(plugin_id.clone()))
    }

    /// Run a hook chain and return the outcome.
    ///
    /// # Errors
    ///
    /// Propagates `PluginRuntimeError` from the runtime.
    pub fn run_hook<T>(
        &self,
        hook_name: &str,
        context: T,
        session: &MockSession,
    ) -> Result<HookOutcome<T>, PluginRuntimeError>
    where
        T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
    {
        let ctx = session.as_ctx();
        self.rt.block_on(self.runtime.run_hook(hook_name, context, &ctx))
    }

    /// Dispatch an interaction event and return the updated view.
    ///
    /// # Errors
    ///
    /// Propagates `PluginRuntimeError` from the runtime.
    pub fn handle_interaction(
        &self,
        plugin_id: &PluginId,
        handler_id: &HandlerId,
        event_data: serde_json::Value,
        session: &MockSession,
    ) -> Result<ViewUpdate, PluginRuntimeError> {
        let ctx = session.as_ctx();
        self.rt
            .block_on(self.runtime.handle_interaction(plugin_id, handler_id, event_data, &ctx))
    }

    /// Emit an event to all registered subscribers.
    ///
    /// # Errors
    ///
    /// Propagates `PluginRuntimeError` from the runtime.
    pub fn emit_event(
        &self,
        event: PluginEvent,
        session: &MockSession,
    ) -> Result<(), PluginRuntimeError> {
        let ctx = session.as_ctx();
        self.rt.block_on(self.runtime.emit_event(event, &ctx))
    }
}

// ── assert_view! ──────────────────────────────────────────────────────────────

/// Assert structural properties of a `PluginView`.
///
/// # Panics
///
/// Panics with a descriptive message if the assertion fails.
///
/// # Examples
///
/// ```ignore
/// assert_view!(view, element("div"));
/// assert_view!(view, text("hello"));
/// assert_view!(view, fragment);
/// assert_view!(view, empty);
/// ```
#[macro_export]
macro_rules! assert_view {
    ($view:expr, element($tag:literal)) => {
        match &$view {
            $crate::dioxus_extism_protocol::PluginView::Element(el) => {
                assert_eq!(
                    el.tag.as_str(), $tag,
                    "expected element <{tag}>, got <{got}>",
                    tag = $tag,
                    got = el.tag
                );
            }
            other => panic!(
                "expected PluginView::Element({tag:?}), got {other:?}",
                tag = $tag,
            ),
        }
    };
    ($view:expr, text($content:literal)) => {
        match &$view {
            $crate::dioxus_extism_protocol::PluginView::Text(t) => {
                assert_eq!(
                    t.as_str(), $content,
                    "expected text {expected:?}, got {got:?}",
                    expected = $content,
                    got = t
                );
            }
            other => panic!("expected PluginView::Text({content:?}), got {other:?}", content = $content),
        }
    };
    ($view:expr, fragment) => {
        match &$view {
            $crate::dioxus_extism_protocol::PluginView::Fragment(_) => {}
            other => panic!("expected PluginView::Fragment, got {other:?}"),
        }
    };
    ($view:expr, empty) => {
        match &$view {
            $crate::dioxus_extism_protocol::PluginView::Empty => {}
            other => panic!("expected PluginView::Empty, got {other:?}"),
        }
    };
    ($view:expr, incompatible) => {
        match &$view {
            $crate::dioxus_extism_protocol::PluginView::Incompatible { .. } => {}
            other => panic!("expected PluginView::Incompatible, got {other:?}"),
        }
    };
}
