use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use dioxus_extism_protocol::{
    ClientCapabilities, ComponentResolution, HandlerId, HookCall, HookResult, HostCapability,
    HostComponentRef, OverrideMap, PluginEvent, PluginId, PluginManifest, PluginView, PriorityHint,
    RoutePattern, RouteTransforms, SessionCtx, SessionId, SlotContent, TransformContext,
    TransformInput, TransformOp, TransformOutput, ViewUpdate, PROTOCOL_VERSION,
};
use extism::convert::Json;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::error::{InvocationError, PersistenceError, PluginRuntimeError};
use crate::host_functions::{self, CallCtx, make_host_functions};

// ── State type aliases ───────────────────────────────────────────────────────

pub type PluginState = HashMap<String, serde_json::Value>;
/// Global (cross-session) state, keyed by plugin.
pub type GlobalStateMap = HashMap<PluginId, PluginState>;
/// Per-session state, keyed by session then plugin.
pub type SessionStateMap = HashMap<SessionId, HashMap<PluginId, PluginState>>;

// ── Plugin source ────────────────────────────────────────────────────────────

/// Where to load plugin WASM bytes from.
pub enum PluginSource {
    /// Local WASM file.
    File(PathBuf),
    /// Remote WASM binary — SHA-256 checksum is mandatory.
    Url {
        url: String,
        sha256: [u8; 32],
    },
    /// In-memory bytes (e.g. `include_bytes!`).
    Bytes(std::borrow::Cow<'static, [u8]>),
}

// ── Install config ────────────────────────────────────────────────────────────

/// Per-plugin installer configuration.
#[derive(Debug, Default)]
pub struct PluginInstallConfig {
    pub base_priority: Option<i32>,
    pub overrides: HashMap<String, i32>,
    pub pool_size: Option<usize>,
    pub max_fuel: Option<u64>,
    pub max_call_duration: Option<Duration>,
}

impl PluginInstallConfig {
    /// Resolve the effective priority for a contribution.
    /// Order: per-name override > `base_priority` > hint.
    #[must_use] 
    pub fn resolve(&self, name: &str, hint: &PriorityHint) -> i32 {
        self.overrides
            .get(name)
            .copied()
            .or(self.base_priority)
            .unwrap_or_else(|| hint.as_numeric())
    }
}

// ── Loaded plugin ─────────────────────────────────────────────────────────────

pub struct LoadedPlugin {
    pub(crate) manifest: PluginManifest,
    pub(crate) pool: extism::Pool,
    pub(crate) enabled: AtomicBool,
    pub(crate) config: PluginInstallConfig,
    /// Shared context for host function callbacks. Updated before each call.
    pub(crate) ctx_arc: Arc<std::sync::Mutex<CallCtx>>,
}

// ── Registries ────────────────────────────────────────────────────────────────

pub struct SlotRegistry(pub(crate) HashMap<String, Vec<(i32, PluginId)>>);
pub struct HookRegistry(pub(crate) HashMap<String, Vec<(i32, PluginId)>>);

/// A resolved transform entry, ready for render-time dispatch.
#[derive(Debug, Clone)]
pub struct TransformEntry {
    pub plugin_id: PluginId,
    /// Plugin export name to call at render time.
    pub transform_fn: String,
    pub op: TransformOp,
    /// Resolved priority (higher = called first).
    pub priority: i32,
    /// The route pattern string, set only for `Selector::Route` entries.
    pub route_pattern: Option<String>,
}

/// Indexes resolved `TransformEntry` values by selector type for efficient render-time lookup.
#[derive(Debug, Default)]
pub struct TransformRegistry {
    components: HashMap<String, Vec<TransformEntry>>,
    slots: HashMap<String, Vec<TransformEntry>>,
    data_slots: HashMap<String, Vec<TransformEntry>>,
    routes: Vec<(RoutePattern, TransformEntry)>,
    /// Within-transforms: `(outer_selector, inner_node_selector, entry)`, sorted priority-desc.
    within: Vec<(dioxus_extism_protocol::Selector, dioxus_extism_protocol::NodeSelector, TransformEntry)>,
}

impl TransformRegistry {
    /// Register a component transform, maintaining priority-descending order.
    pub fn insert_component(&mut self, name: impl Into<String>, entry: TransformEntry) {
        let v = self.components.entry(name.into()).or_default();
        v.push(entry);
        v.sort_by_key(|e| std::cmp::Reverse(e.priority));
    }

    /// Register a slot transform, maintaining priority-descending order.
    pub fn insert_slot(&mut self, name: impl Into<String>, entry: TransformEntry) {
        let v = self.slots.entry(name.into()).or_default();
        v.push(entry);
        v.sort_by_key(|e| std::cmp::Reverse(e.priority));
    }

    /// Register a data-plugin-slot transform, maintaining priority-descending order.
    pub fn insert_data_slot(&mut self, name: impl Into<String>, entry: TransformEntry) {
        let v = self.data_slots.entry(name.into()).or_default();
        v.push(entry);
        v.sort_by_key(|e| std::cmp::Reverse(e.priority));
    }

    /// Register a route transform, maintaining priority-descending order.
    pub fn insert_route(&mut self, pattern: RoutePattern, mut entry: TransformEntry) {
        entry.route_pattern = Some(pattern.0.clone());
        self.routes.push((pattern, entry));
        self.routes.sort_by_key(|e| std::cmp::Reverse(e.1.priority));
    }

    /// Returns component transforms in priority-descending order, or empty if none.
    #[must_use] 
    pub fn for_component(&self, name: &str) -> Vec<TransformEntry> {
        self.components.get(name).cloned().unwrap_or_default()
    }

    /// Returns slot transforms in priority-descending order, or empty if none.
    #[must_use] 
    pub fn for_slot(&self, name: &str) -> Vec<TransformEntry> {
        self.slots.get(name).cloned().unwrap_or_default()
    }

    /// Returns data-plugin-slot transforms in priority-descending order, or empty if none.
    #[must_use] 
    pub fn for_data_slot(&self, name: &str) -> Vec<TransformEntry> {
        self.data_slots.get(name).cloned().unwrap_or_default()
    }

    /// Returns all route transforms whose pattern matches `path`, in priority-descending order.
    #[must_use] 
    pub fn for_route(&self, path: &str) -> Vec<TransformEntry> {
        self.routes
            .iter()
            .filter(|(pat, _)| pat.matches(path))
            .map(|(_, e)| e.clone())
            .collect()
    }

    /// Register a within-transform, maintaining priority-descending order.
    pub fn insert_within(
        &mut self,
        outer: dioxus_extism_protocol::Selector,
        inner: dioxus_extism_protocol::NodeSelector,
        entry: TransformEntry,
    ) {
        self.within.push((outer, inner, entry));
        self.within.sort_by_key(|e| std::cmp::Reverse(e.2.priority));
    }

    /// Returns `(NodeSelector, TransformEntry)` pairs for all within-transforms whose
    /// outer selector matches `outer`, in priority-descending order.
    #[must_use]
    pub fn within_for_outer(
        &self,
        outer: &dioxus_extism_protocol::Selector,
    ) -> Vec<(dioxus_extism_protocol::NodeSelector, TransformEntry)> {
        self.within
            .iter()
            .filter(|(o, _, _)| selectors_equal(o, outer))
            .map(|(_, inner, entry)| (inner.clone(), entry.clone()))
            .collect()
    }

    /// All component names that have at least one registered transform.
    pub fn all_component_names(&self) -> HashSet<&str> {
        self.components.keys().map(String::as_str).collect()
    }

    /// All slot names that have at least one registered transform.
    pub fn all_slot_names(&self) -> HashSet<&str> {
        self.slots.keys().map(String::as_str).collect()
    }
}

pub struct Registries {
    pub(crate) slots: SlotRegistry,
    pub(crate) hooks: HookRegistry,
    pub(crate) transforms: TransformRegistry,
    pub(crate) override_map: OverrideMap,
    pub(crate) api_routes: ApiRegistry,
    pub(crate) page_routes: PageRouteRegistry,
}

// ── Invocation registry ───────────────────────────────────────────────────────

type InvocationHandler = Arc<
    dyn Fn(
            serde_json::Value,
            SessionCtx,
        ) -> BoxFuture<'static, Result<serde_json::Value, InvocationError>>
        + Send
        + Sync,
>;

/// Maps named invocation handlers registered at build time.
pub struct InvocationRegistry {
    pub(crate) handlers: HashMap<String, (InvocationHandler, Duration)>,
}

impl InvocationRegistry {
    fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    #[tracing::instrument(skip(self, args, session), fields(invocation = name))]
    pub(crate) async fn call(
        &self,
        name: &str,
        args: serde_json::Value,
        session: SessionCtx,
    ) -> Result<serde_json::Value, InvocationError> {
        let (handler, timeout) = self
            .handlers
            .get(name)
            .ok_or_else(|| InvocationError::NotFound(name.into()))?;
        tokio::time::timeout(*timeout, handler(args, session))
            .await
            .map_err(|_| InvocationError::Timeout(*timeout))?
    }
}

// ── API route registry ────────────────────────────────────────────────────────

/// A single plugin-declared inbound API route entry stored in the registry.
#[derive(Clone)]
pub struct ApiRouteEntry {
    /// Plugin that owns this route.
    pub plugin_id: PluginId,
    /// WASM export name to call when this route is hit.
    pub handler_fn: String,
    /// Pool for the owning plugin — cheap to clone (Arc internally).
    pub pool: extism::Pool,
}

/// Maps `(method_str, path_str)` to the plugin that handles it.
///
/// Built by `build_registries`; a duplicate key is a startup error.
pub struct ApiRegistry(pub(crate) HashMap<(String, String), ApiRouteEntry>);

// ── Page route registry ───────────────────────────────────────────────────────

/// A single plugin-declared view page route entry.
#[derive(Clone)]
pub struct PageRouteEntry {
    pub plugin_id: PluginId,
    pub handler_fn: String,
    pub pool: extism::Pool,
    pub bypass_layout: bool,
    pub title: Option<String>,
    /// The declared route pattern (`:param` syntax) stored for param extraction.
    pub pattern: RoutePattern,
}

/// Maps relative path patterns to page route entries.
///
/// Lookup iterates entries; first match wins (ordered by declaration / plugin insertion order).
pub struct PageRouteRegistry(pub(crate) Vec<(RoutePattern, PageRouteEntry)>);

impl PageRouteRegistry {
    /// Find the entry whose pattern matches `relative_path` and extract path params.
    pub fn find(&self, relative_path: &str) -> Option<(PageRouteEntry, HashMap<String, String>)> {
        self.0.iter().find_map(|(pat, entry)| {
            pat.extract_params(relative_path)
                .map(|params| (entry.clone(), params))
        })
    }
}

// ── Event bus ─────────────────────────────────────────────────────────────────

pub struct EventBus {
    pub(crate) subscribers: HashMap<String, Vec<(i32, PluginId)>>,
}

impl EventBus {
    fn new() -> Self {
        Self {
            subscribers: HashMap::new(),
        }
    }

    fn build_from_plugins(plugins: &IndexMap<PluginId, LoadedPlugin>) -> Self {
        let mut subscribers: HashMap<String, Vec<(i32, PluginId)>> = HashMap::new();
        for (id, loaded) in plugins {
            let priority = loaded.config.resolve("__event__", &PriorityHint::Normal);
            for event_name in &loaded.manifest.event_subscriptions {
                subscribers
                    .entry(event_name.clone())
                    .or_default()
                    .push((priority, id.clone()));
            }
        }
        for v in subscribers.values_mut() {
            v.sort_by_key(|e| std::cmp::Reverse(e.0));
        }
        Self { subscribers }
    }
}

// ── HookOutcome ───────────────────────────────────────────────────────────────

/// Result of running a hook chain.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum HookOutcome<T> {
    /// All plugins in the chain processed the context without cancelling.
    Passed(T),
    /// A plugin cancelled the hook, aborting the chain.
    Cancelled {
        /// Which plugin cancelled the hook.
        by: PluginId,
        /// Human-readable reason supplied by the plugin.
        reason: String,
    },
}

// ── PluginRuntime ─────────────────────────────────────────────────────────────

/// The central server-side plugin runtime.
pub struct PluginRuntime {
    pub(crate) plugins: RwLock<IndexMap<PluginId, LoadedPlugin>>,
    pub(crate) global_states: Arc<RwLock<GlobalStateMap>>,
    pub(crate) session_states: Arc<RwLock<SessionStateMap>>,
    /// Last time each session's state was read or written. Used for TTL eviction.
    pub(crate) session_last_access: Arc<RwLock<HashMap<SessionId, std::time::Instant>>>,
    pub(crate) event_bus: RwLock<EventBus>,
    pub(crate) registries: RwLock<Registries>,
    pub(crate) invocation_registry: Arc<InvocationRegistry>,
    pub(crate) override_map_tx: broadcast::Sender<OverrideMap>,
    pub(crate) persistence: Option<Arc<dyn StatePersistenceProvider>>,
    /// URL prefix under which plugin page routes are served (set at build time).
    pub(crate) plugin_page_prefix: Option<String>,
    /// Sender half of the event dispatch channel; plugins call dx_emit_event → send here.
    pub(crate) event_tx: mpsc::UnboundedSender<(PluginEvent, SessionCtx)>,
}

impl PluginRuntime {
    /// Returns the cached `OverrideMap` without recomputation.
    pub async fn override_map(&self) -> OverrideMap {
        self.registries.read().await.override_map.clone()
    }

    /// Pre-fetch all plugin contributions for a route in preparation for SSR.
    ///
    /// Calls all async plugin operations up front so the synchronous
    /// `dioxus_ssr::render` pass can run without async context.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if any registry lock fails.
    pub async fn ssr_render_route(
        &self,
        path: &str,
        session: &SessionCtx,
    ) -> Result<dioxus_extism_protocol::SsrRouteOutput, PluginRuntimeError> {
        use dioxus_extism_protocol::{SsrComponentResolution, SsrRouteOutput, SsrRouteTransforms};

        // Route transforms.
        let rt = self.render_route_transforms(path, session).await?;
        let route_transforms = SsrRouteTransforms {
            before: rt.before,
            wrap: rt.wrap,
            after: rt.after,
        };

        // All registered slot names.
        let slot_names: Vec<String> = {
            let regs = self.registries.read().await;
            regs.slots.0.keys().cloned().collect()
        };
        let mut slots = std::collections::HashMap::new();
        for name in &slot_names {
            let content = self.render_slot(name, session).await?;
            slots.insert(name.clone(), content);
        }

        // All registered component names.
        let component_names: Vec<String> = {
            let regs = self.registries.read().await;
            regs.transforms.all_component_names().into_iter().map(str::to_owned).collect()
        };
        let mut components = std::collections::HashMap::new();
        for name in &component_names {
            let resolution = self
                .resolve_component(name, serde_json::Value::Null, session)
                .await?
                .map(|r| SsrComponentResolution {
                    before: r.before,
                    replacement: r.replacement,
                    after: r.after,
                });
            components.insert(name.clone(), resolution);
        }

        Ok(SsrRouteOutput { route_transforms, slots, components })
    }

    /// Read one key from a plugin's session state.
    ///
    /// Returns `None` if the session or key does not exist.
    pub async fn get_plugin_state(
        &self,
        plugin_id: &PluginId,
        key: &str,
        session_id: &SessionId,
    ) -> Option<serde_json::Value> {
        let states = self.session_states.read().await;
        states
            .get(session_id)
            .and_then(|s| s.get(plugin_id))
            .and_then(|p| p.get(key))
            .cloned()
    }

    /// Write a key into a plugin's per-session state.
    ///
    /// Creates the session entry if it does not exist yet.
    /// Use this before `render_slot` to pass per-request context to a plugin.
    pub async fn set_plugin_state(
        &self,
        plugin_id: &PluginId,
        session_id: &SessionId,
        key: impl Into<String>,
        value: serde_json::Value,
    ) {
        {
            let mut states = self.session_states.write().await;
            states
                .entry(session_id.clone())
                .or_default()
                .entry(plugin_id.clone())
                .or_default()
                .insert(key.into(), value);
        }
        self.session_last_access
            .write()
            .await
            .insert(session_id.clone(), std::time::Instant::now());
    }

    /// Enable a previously disabled plugin.
    ///
    /// Uses a read lock on `plugins` — no registry rebuild required because
    /// `enabled` is checked at dispatch time.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError::PluginNotFound` if the plugin is not registered.
    #[allow(clippy::significant_drop_tightening)]
    pub fn enable_plugin(&self, id: &PluginId) -> Result<(), PluginRuntimeError> {
        let plugins = self.plugins.blocking_read();
        plugins
            .get(id)
            .ok_or_else(|| PluginRuntimeError::PluginNotFound(id.clone()))?
            .enabled
            .store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Disable a plugin so it is skipped at dispatch time.
    ///
    /// Uses a read lock on `plugins` — no registry rebuild required because
    /// `enabled` is checked at dispatch time.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError::PluginNotFound` if the plugin is not registered.
    #[allow(clippy::significant_drop_tightening)]
    pub fn disable_plugin(&self, id: &PluginId) -> Result<(), PluginRuntimeError> {
        let plugins = self.plugins.blocking_read();
        plugins
            .get(id)
            .ok_or_else(|| PluginRuntimeError::PluginNotFound(id.clone()))?
            .enabled
            .store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Subscribe to `OverrideMap` change notifications (used by the SSE endpoint).
    pub fn override_map_updates(&self) -> broadcast::Receiver<OverrideMap> {
        self.override_map_tx.subscribe()
    }

    /// Collect slot contributions from all registered plugins, with error isolation.
    ///
    /// Plugins that fail to render serve a `PluginView::Incompatible` entry instead
    /// of aborting the entire slot.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if locking the registry fails.
    #[tracing::instrument(skip(self, session), fields(slot = slot_name))]
    pub async fn render_slot(
        &self,
        slot_name: &str,
        session: &SessionCtx,
    ) -> Result<Vec<SlotContent>, PluginRuntimeError> {
        let entries = {
            let regs = self.registries.read().await;
            regs.slots.0.get(slot_name).cloned().unwrap_or_default()
        };

        let mut results = Vec::with_capacity(entries.len());

        for (priority, plugin_id) in entries {
            let (pool, enabled) = {
                let plugins = self.plugins.read().await;
                match plugins.get(&plugin_id) {
                    Some(p) => {
                        let min_prot = p.manifest.min_protocol_version;
                        if min_prot > session.client.protocol_version {
                            results.push(SlotContent {
                                plugin_id: plugin_id.clone(),
                                priority,
                                view: PluginView::Incompatible {
                                    reason: format!(
                                        "plugin requires protocol {min_prot}, client has {}",
                                        session.client.protocol_version
                                    ),
                                    fallback: None,
                                },
                            });
                            continue;
                        }
                        (p.pool.clone(), p.enabled.load(Ordering::Relaxed))
                    }
                    None => continue,
                }
            };

            if !enabled {
                results.push(SlotContent {
                    plugin_id: plugin_id.clone(),
                    priority,
                    view: PluginView::Incompatible {
                        reason: format!("plugin {plugin_id:?} is disabled"),
                        fallback: None,
                    },
                });
                continue;
            }

            match call_export::<SessionCtx, PluginView>(
                pool,
                "slot_render",
                session.clone(),
                session.clone(),
            )
            .await
            {
                Ok(view) => results.push(SlotContent { plugin_id, priority, view }),
                Err(e) => {
                    tracing::warn!(
                        plugin = %plugin_id.0,
                        error = %e,
                        slot = slot_name,
                        "slot render failed, serving Incompatible"
                    );
                    results.push(SlotContent {
                        plugin_id,
                        priority,
                        view: PluginView::Incompatible {
                            reason: format!("render failed: {e}"),
                            fallback: None,
                        },
                    });
                }
            }
        }

        // Step 3: apply Within-transforms to each slot item's view.
        let within_entries = {
            let regs = self.registries.read().await;
            regs.transforms
                .within_for_outer(&dioxus_extism_protocol::Selector::Slot(slot_name.into()))
        };
        if !within_entries.is_empty() {
            let ctx = TransformContext {
                slot_name: Some(slot_name.into()),
                client: session.client.clone(),
                ..Default::default()
            };
            for content in &mut results {
                content.view = self
                    .apply_within_entries(&within_entries, content.view.clone(), ctx.clone(), session)
                    .await;
            }
        }

        Ok(results)
    }

    /// Run the named hook chain, threading context through each plugin in priority order.
    ///
    /// Any plugin that fails is skipped (error isolation); a plugin can cancel the entire
    /// chain by returning `HookResult::Cancel`.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if context serialisation fails.
    #[tracing::instrument(skip(self, context, session), fields(hook = hook_name))]
    pub async fn run_hook<T>(
        &self,
        hook_name: &str,
        context: T,
        session: &SessionCtx,
    ) -> Result<HookOutcome<T>, PluginRuntimeError>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
    {
        let entries = {
            let regs = self.registries.read().await;
            regs.hooks.0.get(hook_name).cloned().unwrap_or_default()
        };

        let mut current = serde_json::to_value(&context)?;

        for (_, plugin_id) in entries {
            let (pool, enabled) = {
                let plugins = self.plugins.read().await;
                match plugins.get(&plugin_id) {
                    Some(p) => (p.pool.clone(), p.enabled.load(Ordering::Relaxed)),
                    None => continue,
                }
            };
            if !enabled {
                continue;
            }

            let hook_call = HookCall {
                hook_name: hook_name.to_owned(),
                context: current.clone(),
            };

            let export = format!("hook_{hook_name}");
            match call_export::<(HookCall, SessionCtx), HookResult>(
                pool,
                export,
                (hook_call, session.clone()),
                session.clone(),
            )
            .await
            {
                Ok(HookResult::Continue { context: c } | HookResult::Replace { context: c }) => current = c,
                Ok(HookResult::Cancel { reason }) => {
                    return Ok(HookOutcome::Cancelled { by: plugin_id, reason });
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        plugin = %plugin_id.0,
                        error = %e,
                        hook = hook_name,
                        "hook call failed, skipping"
                    );
                }
            }
        }

        let final_context: T = serde_json::from_value(current)?;
        Ok(HookOutcome::Passed(final_context))
    }

    /// Route an interaction event to the owning plugin and return the updated view.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if the plugin is not found or the call fails.
    #[tracing::instrument(skip(self, event_data, session), fields(plugin = %plugin_id.0, handler = %handler_id.0))]
    pub async fn handle_interaction(
        &self,
        plugin_id: &PluginId,
        handler_id: &HandlerId,
        event_data: serde_json::Value,
        session: &SessionCtx,
    ) -> Result<ViewUpdate, PluginRuntimeError> {
        let pool = {
            let plugins = self.plugins.read().await;
            match plugins.get(plugin_id) {
                Some(p) if p.enabled.load(Ordering::Relaxed) => p.pool.clone(),
                Some(_) => {
                    return Err(PluginRuntimeError::PluginDisabled(plugin_id.clone()));
                }
                None => {
                    return Err(PluginRuntimeError::PluginNotFound(plugin_id.clone()));
                }
            }
        };

        call_export::<(HandlerId, serde_json::Value, SessionCtx), ViewUpdate>(
            pool,
            "on_interaction",
            (handler_id.clone(), event_data, session.clone()),
            session.clone(),
        )
        .await
    }

    /// Emit an event to all registered subscribers, with error isolation.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if locking the registry fails.
    pub async fn emit_event(
        &self,
        event: PluginEvent,
        session: &SessionCtx,
    ) -> Result<(), PluginRuntimeError> {
        let subscribers = {
            let bus = self.event_bus.read().await;
            bus.subscribers.get(&event.name).cloned().unwrap_or_default()
        };

        for (_, plugin_id) in subscribers {
            let (pool, enabled) = {
                let plugins = self.plugins.read().await;
                match plugins.get(&plugin_id) {
                    Some(p) => (p.pool.clone(), p.enabled.load(Ordering::Relaxed)),
                    None => continue,
                }
            };
            if !enabled {
                continue;
            }

            if let Err(e) = call_export::<(PluginEvent, SessionCtx), ()>(
                pool,
                "on_event",
                (event.clone(), session.clone()),
                session.clone(),
            )
            .await
            {
                tracing::warn!(
                    plugin = %plugin_id.0,
                    error = %e,
                    event = event.name,
                    "event dispatch failed, skipping"
                );
            }
        }

        Ok(())
    }

    /// Resolve all registered transforms for `component_name`, returning `None` if none are
    /// registered, or `Some(ComponentResolution)` otherwise (with per-plugin error isolation).
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if locking the registry fails.
    pub async fn resolve_component(
        &self,
        component_name: &str,
        props: serde_json::Value,
        session: &SessionCtx,
    ) -> Result<Option<ComponentResolution>, PluginRuntimeError> {
        let entries = {
            let regs = self.registries.read().await;
            regs.transforms.for_component(component_name)
        };
        if entries.is_empty() {
            return Ok(None);
        }

        let mut before = vec![];
        let mut replacement = None;
        let mut after = vec![];

        for entry in entries {
            let pool = {
                let plugins = self.plugins.read().await;
                match plugins.get(&entry.plugin_id) {
                    Some(p)
                        if p.enabled.load(Ordering::Relaxed)
                            && Self::is_compatible(p, &session.client) =>
                    {
                        p.pool.clone()
                    }
                    _ => continue,
                }
            };

            let context = TransformContext {
                component_props: Some(props.clone()),
                client: session.client.clone(),
                ..Default::default()
            };
            let input = TransformInput { original: None, context, session: session.clone() };

            match call_export::<TransformInput, TransformOutput>(
                pool,
                entry.transform_fn.clone(),
                input,
                session.clone(),
            )
            .await
            {
                Ok(output) => match entry.op {
                    TransformOp::InjectBefore => before.push(output.view),
                    TransformOp::InjectAfter => after.push(output.view),
                    TransformOp::WrapNode | TransformOp::Replace => {
                        replacement = Some(output.view);
                    }
                    _ => {}
                },
                Err(e) => {
                    tracing::warn!(
                        plugin = %entry.plugin_id.0,
                        component = component_name,
                        error = %e,
                        "transform call failed, skipping"
                    );
                }
            }
        }

        Ok(Some(ComponentResolution { before, replacement, after }))
    }

    /// Resolve all registered route transforms for `path`, returning partitioned results.
    ///
    /// The Wrap partition uses a sequential fold: each plugin receives the previous plugin's
    /// full output as `original`, not the seed. On plugin error, `current_view` stays
    /// unchanged and the fold continues (error isolation). `InjectBefore` and `InjectAfter`
    /// are error-isolated independently.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if locking the registry fails.
    #[tracing::instrument(skip(self, session), fields(path))]
    pub async fn render_route_transforms(
        &self,
        path: &str,
        session: &SessionCtx,
    ) -> Result<RouteTransforms, PluginRuntimeError> {
        let all_entries = {
            let regs = self.registries.read().await;
            regs.transforms.for_route(path)
        };

        let mut inject_before = vec![];
        let mut wrap_entries = vec![];
        let mut inject_after = vec![];

        for entry in all_entries {
            match entry.op {
                TransformOp::InjectBefore => inject_before.push(entry),
                TransformOp::Wrap => wrap_entries.push(entry),
                TransformOp::InjectAfter => inject_after.push(entry),
                _ => {}
            }
        }

        let before = self.run_inject_transforms(&inject_before, path, session).await;

        let wrap = if wrap_entries.is_empty() {
            None
        } else {
            let seed = PluginView::HostComponent(HostComponentRef {
                name: "__content__".into(),
                ..Default::default()
            });
            let mut current = seed;

            for entry in &wrap_entries {
                let pool = {
                    let plugins = self.plugins.read().await;
                    match plugins.get(&entry.plugin_id) {
                        Some(p)
                            if p.enabled.load(Ordering::Relaxed)
                                && Self::is_compatible(p, &session.client) =>
                        {
                            p.pool.clone()
                        }
                        _ => continue,
                    }
                };

                let params = entry
                    .route_pattern
                    .as_deref()
                    .and_then(|pat| RoutePattern(pat.into()).extract_params(path))
                    .unwrap_or_default();

                let input = TransformInput {
                    original: Some(current.clone()),
                    context: TransformContext {
                        route_params: params,
                        client: session.client.clone(),
                        ..Default::default()
                    },
                    session: session.clone(),
                };

                match call_export::<TransformInput, TransformOutput>(
                    pool,
                    entry.transform_fn.clone(),
                    input,
                    session.clone(),
                )
                .await
                {
                    Ok(out) => {
                        #[cfg(debug_assertions)]
                        if !view_contains_content_placeholder(&out.view) {
                            tracing::warn!(
                                plugin = %entry.plugin_id.0,
                                transform_fn = %entry.transform_fn,
                                "Wrap output omits __content__ placeholder; original content cut"
                            );
                        }
                        current = out.view;
                    }
                    Err(e) => {
                        tracing::warn!(
                            plugin = %entry.plugin_id.0,
                            error = %e,
                            "wrap transform failed, keeping current view"
                        );
                    }
                }
            }
            Some(current)
        };

        let after = self.run_inject_transforms(&inject_after, path, session).await;

        Ok(RouteTransforms { before, wrap, after })
    }

    async fn run_inject_transforms(
        &self,
        entries: &[TransformEntry],
        path: &str,
        session: &SessionCtx,
    ) -> Vec<PluginView> {
        let mut views = Vec::with_capacity(entries.len());

        for entry in entries {
            let pool = {
                let plugins = self.plugins.read().await;
                match plugins.get(&entry.plugin_id) {
                    Some(p)
                        if p.enabled.load(Ordering::Relaxed)
                            && Self::is_compatible(p, &session.client) =>
                    {
                        p.pool.clone()
                    }
                    _ => continue,
                }
            };

            let params = entry
                .route_pattern
                .as_deref()
                .and_then(|pat| RoutePattern(pat.into()).extract_params(path))
                .unwrap_or_default();

            let input = TransformInput {
                original: None,
                context: TransformContext {
                    route_params: params,
                    client: session.client.clone(),
                    ..Default::default()
                },
                session: session.clone(),
            };

            match call_export::<TransformInput, TransformOutput>(
                pool,
                entry.transform_fn.clone(),
                input,
                session.clone(),
            )
            .await
            {
                Ok(out) => views.push(out.view),
                Err(e) => {
                    tracing::warn!(
                        plugin = %entry.plugin_id.0,
                        error = %e,
                        "inject transform failed, skipping"
                    );
                }
            }
        }

        views
    }

    /// Apply all `Within` transforms registered for `outer_selector` to `view`.
    ///
    /// Returns the unchanged view if no within-transforms are registered (fast path).
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if locking the registry fails.
    pub async fn apply_tree_transforms(
        &self,
        outer_selector: &dioxus_extism_protocol::Selector,
        view: PluginView,
        context: TransformContext,
        session: &SessionCtx,
    ) -> Result<PluginView, PluginRuntimeError> {
        let entries = {
            let regs = self.registries.read().await;
            regs.transforms.within_for_outer(outer_selector)
        };
        Ok(self.apply_within_entries(&entries, view, context, session).await)
    }

    async fn apply_within_entries(
        &self,
        entries: &[(dioxus_extism_protocol::NodeSelector, TransformEntry)],
        view: PluginView,
        context: TransformContext,
        session: &SessionCtx,
    ) -> PluginView {
        if entries.is_empty() {
            return view;
        }
        let mut current = view;
        for (node_selector, entry) in entries {
            let pool = {
                let plugins = self.plugins.read().await;
                match plugins.get(&entry.plugin_id) {
                    Some(p)
                        if p.enabled.load(Ordering::Relaxed)
                            && Self::is_compatible(p, &session.client) =>
                    {
                        p.pool.clone()
                    }
                    _ => continue,
                }
            };
            current = traverse_and_apply(
                current,
                node_selector.clone(),
                entry.op.clone(),
                entry.transform_fn.clone(),
                pool,
                context.clone(),
                session.clone(),
            )
            .await;
        }
        current
    }

    /// Reload a plugin from a new source, following Pattern 3 from CLAUDE.md.
    ///
    /// Steps (in order):
    /// 1. Build the new `Pool` outside the write lock (WASM compilation is expensive).
    /// 2. Call `on_unload` on the old pool outside the write lock — best effort.
    /// 3. Atomically swap the plugin entry under a single write lock, rebuild registries,
    ///    and bump the override-map version.
    /// 4. Broadcast the new `OverrideMap` after both locks are released.
    ///
    /// # Errors
    /// Returns an error if the source cannot be fetched, the manifest is incompatible,
    /// or pool construction fails.
    #[allow(clippy::too_many_lines, clippy::significant_drop_tightening)]
    pub async fn reload_plugin(
        &self,
        id: &PluginId,
        source: PluginSource,
        config: PluginInstallConfig,
    ) -> Result<(), PluginRuntimeError> {
        // Step 1: build the new pool outside any write lock.
        let new_manifest = load_plugin_manifest(&source).await?;

        if new_manifest.min_protocol_version > PROTOCOL_VERSION {
            return Err(PluginRuntimeError::ProtocolVersionMismatch {
                required: new_manifest.min_protocol_version,
                host: PROTOCOL_VERSION,
            });
        }

        let (session_states, global_states, session_last_access, invocation_registry, persistence) = {
            let plugins = self.plugins.read().await;
            let p = plugins
                .get(id)
                .ok_or_else(|| PluginRuntimeError::PluginNotFound(id.clone()))?;
            let ctx = p.ctx_arc.lock().map_err(|_| PluginRuntimeError::Pool("ctx mutex poisoned".into()))?;
            (
                ctx.session_states.clone(),
                ctx.global_states.clone(),
                ctx.session_last_access.clone(),
                ctx.invocation_registry.clone(),
                ctx.persistence.clone(),
            )
        };

        let mut granted_invocations = HashSet::new();
        let mut granted_global_read = HashSet::new();
        let mut granted_global_write = HashSet::new();
        let mut granted_http_hosts: Vec<String> = Vec::new();
        for cap in &new_manifest.host_capabilities {
            match cap {
                HostCapability::Invoke { names } => {
                    for name in names {
                        granted_invocations.insert(name.clone());
                    }
                }
                HostCapability::GlobalStateRead { keys } => {
                    granted_global_read.extend(keys.iter().cloned());
                }
                HostCapability::GlobalStateWrite { keys } => {
                    granted_global_write.extend(keys.iter().cloned());
                }
                HostCapability::Http { allowed_hosts } => {
                    granted_http_hosts.extend(allowed_hosts.iter().cloned());
                }
                _ => {}
            }
        }

        let new_ctx = CallCtx {
            caller: new_manifest.id.clone(),
            session_states,
            session_last_access,
            global_states,
            invocation_registry: invocation_registry.clone(),
            persistence,
            granted_invocations,
            granted_global_read,
            granted_global_write,
            granted_http_hosts,
            event_tx: self.event_tx.clone(),
        };
        let user_data = extism::UserData::new(new_ctx);
        let new_ctx_arc = user_data
            .get()
            .map_err(|e| PluginRuntimeError::Pool(format!("UserData::get failed: {e}")))?;

        let host_fns = make_host_functions(user_data.clone());

        let pool_size = config
            .pool_size
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, usize::from));
        let bytes = fetch_and_verify(&source).await?;
        let ext_manifest = extism::Manifest::new([extism::Wasm::data(bytes)]);
        let fns = host_fns;
        let new_pool = extism::Pool::new_from_builder(
            move || {
                extism::PluginBuilder::new(ext_manifest.clone())
                    .with_wasi(true)
                    .with_functions(fns.clone())
                    .build()
            },
            extism::PoolBuilder::default().with_max_instances(pool_size),
        );

        // Call on_load on the new pool before registering — failure aborts reload.
        let has_on_load = new_pool
            .function_exists("on_load", Duration::from_secs(5))
            .map_err(|e| PluginRuntimeError::CallFailed { source: e })?;
        if has_on_load {
            let pool_clone = new_pool.clone();
            let init_session = SessionCtx {
                session_id: SessionId("__init__".into()),
                user_id: None,
                client: ClientCapabilities {
                    protocol_version: PROTOCOL_VERSION,
                    app_version: 0,
                    registered_host_components: vec![],
                },
                caller: None,
            };
            let init_clone = init_session.clone();
            tokio::task::spawn_blocking(move || {
                host_functions::set_call_session(init_clone.session_id.clone(), init_clone.client.clone());
                let mut p = pool_clone
                    .get(Duration::from_secs(5))
                    .map_err(|e| PluginRuntimeError::CallFailed { source: e })?
                    .ok_or_else(|| PluginRuntimeError::Pool("timeout on on_load during reload".into()))?;
                p.call::<Json<SessionCtx>, ()>("on_load", Json(init_session))
                    .map_err(|e| PluginRuntimeError::CallFailed { source: e })
            })
            .await
            .map_err(|e| PluginRuntimeError::TaskPanic(e.to_string()))??;
        }

        // Step 2: call on_unload on the OLD pool outside the write lock — best effort.
        {
            let plugins = self.plugins.read().await;
            if let Some(old) = plugins.get(id) {
                let has_on_unload = old
                    .pool
                    .function_exists("on_unload", Duration::from_secs(5))
                    .unwrap_or(false);
                if has_on_unload {
                    let pool_clone = old.pool.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(Some(mut p)) = pool_clone.get(Duration::from_secs(5)) {
                            let _ = p.call::<(), ()>("on_unload", ());
                        }
                    })
                    .await;
                }
            }
        }

        // Step 3: single write lock — swap, rebuild registries, bump version.
        let new_map = {
            let mut plugins = self.plugins.write().await;
            let mut regs = self.registries.write().await;
            plugins.insert(
                id.clone(),
                LoadedPlugin {
                    manifest: new_manifest,
                    pool: new_pool,
                    enabled: AtomicBool::new(true),
                    config,
                    ctx_arc: new_ctx_arc,
                },
            );
            let mut new_regs =
                Self::build_registries(&plugins, self.plugin_page_prefix.as_deref())?;
            new_regs.override_map.version = regs.override_map.version + 1;
            *regs = new_regs;
            regs.override_map.clone()
        };

        // Step 4: broadcast after both locks are released.
        let _ = self.override_map_tx.send(new_map);
        Ok(())
    }

    /// Unload a plugin, calling its `on_unload` export if present.
    ///
    /// After removal the registries are rebuilt and subscribers are notified.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError::PluginNotFound` if the plugin is not registered.
    #[allow(clippy::significant_drop_tightening)]
    pub async fn unload_plugin(&self, id: &PluginId) -> Result<(), PluginRuntimeError> {
        // Call on_unload outside the write lock — best effort.
        {
            let plugins = self.plugins.read().await;
            if let Some(p) = plugins.get(id) {
                let has_on_unload = p
                    .pool
                    .function_exists("on_unload", Duration::from_secs(5))
                    .unwrap_or(false);
                if has_on_unload {
                    let pool_clone = p.pool.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(Some(mut p)) = pool_clone.get(Duration::from_secs(5)) {
                            let _ = p.call::<(), ()>("on_unload", ());
                        }
                    })
                    .await;
                }
            }
        }

        // Single write lock — remove, rebuild, bump version.
        let new_map = {
            let mut plugins = self.plugins.write().await;
            if plugins.swap_remove(id).is_none() {
                return Err(PluginRuntimeError::PluginNotFound(id.clone()));
            }
            let mut regs = self.registries.write().await;
            let mut new_regs =
                Self::build_registries(&plugins, self.plugin_page_prefix.as_deref())?;
            new_regs.override_map.version = regs.override_map.version + 1;
            *regs = new_regs;
            regs.override_map.clone()
        };

        let _ = self.override_map_tx.send(new_map);
        Ok(())
    }

    const fn is_compatible(plugin: &LoadedPlugin, client: &ClientCapabilities) -> bool {
        plugin.manifest.min_protocol_version <= client.protocol_version
            && plugin.manifest.min_app_version <= client.app_version
    }

    pub(crate) fn build_registries(
        plugins: &IndexMap<PluginId, LoadedPlugin>,
        page_prefix: Option<&str>,
    ) -> Result<Registries, PluginRuntimeError> {
        use dioxus_extism_protocol::{PluginClientRequirement, Selector};

        let mut slots: HashMap<String, Vec<(i32, PluginId)>> = HashMap::new();
        let mut hooks: HashMap<String, Vec<(i32, PluginId)>> = HashMap::new();
        let mut overridden_components: HashSet<String> = HashSet::new();
        let mut transformed_slots: HashSet<String> = HashSet::new();
        let mut route_patterns: Vec<RoutePattern> = Vec::new();
        let mut required_protocol_version: u32 = 0;
        let mut required_app_version: u32 = 0;
        let mut plugin_requirements: HashMap<PluginId, PluginClientRequirement> = HashMap::new();
        let mut transforms = TransformRegistry::default();
        let mut api_route_map: HashMap<(String, String), ApiRouteEntry> = HashMap::new();
        let mut page_route_list: Vec<(RoutePattern, PageRouteEntry)> = Vec::new();

        for (id, loaded) in plugins {
            let manifest = &loaded.manifest;

            for slot_reg in &manifest.slots {
                let priority = loaded.config.resolve(&slot_reg.name, &slot_reg.priority_hint);
                slots
                    .entry(slot_reg.name.clone())
                    .or_default()
                    .push((priority, id.clone()));
            }
            for hook_reg in &manifest.hooks {
                let priority =
                    loaded.config.resolve(&hook_reg.hook_name, &hook_reg.priority_hint);
                hooks
                    .entry(hook_reg.hook_name.clone())
                    .or_default()
                    .push((priority, id.clone()));
            }
            for transform in &manifest.transforms {
                let priority =
                    loaded.config.resolve(&transform.transform_fn, &transform.priority_hint);
                let entry = TransformEntry {
                    plugin_id: id.clone(),
                    transform_fn: transform.transform_fn.clone(),
                    op: transform.op.clone(),
                    priority,
                    route_pattern: None,
                };
                match &transform.selector {
                    Selector::Component(name) => {
                        overridden_components.insert(name.clone());
                        transforms.insert_component(name.clone(), entry);
                    }
                    Selector::Slot(name) => {
                        transformed_slots.insert(name.clone());
                        transforms.insert_slot(name.clone(), entry);
                    }
                    Selector::DataPluginSlot(name) => {
                        transforms.insert_data_slot(name.clone(), entry);
                    }
                    Selector::Route(pattern) => {
                        route_patterns.push(pattern.clone());
                        transforms.insert_route(pattern.clone(), entry);
                    }
                    Selector::Within { outer, inner } => {
                        transforms.insert_within(*outer.clone(), inner.clone(), entry);
                    }
                    _ => {}
                }
            }
            for decl in &manifest.api_routes {
                let key = (decl.method.as_str().to_owned(), decl.path.clone());
                if let Some(existing) = api_route_map.get(&key) {
                    return Err(PluginRuntimeError::ApiRouteConflict {
                        method: decl.method.as_str().to_owned(),
                        path: decl.path.clone(),
                        first: existing.plugin_id.clone(),
                        second: id.clone(),
                    });
                }
                api_route_map.insert(
                    key,
                    ApiRouteEntry {
                        plugin_id: id.clone(),
                        handler_fn: decl.handler_fn.clone(),
                        pool: loaded.pool.clone(),
                    },
                );
            }
            for decl in &manifest.page_routes {
                if page_route_list.iter().any(|(pat, _)| pat.0 == decl.path) {
                    let existing_id = page_route_list
                        .iter()
                        .find(|(pat, _)| pat.0 == decl.path)
                        .map(|(_, e)| e.plugin_id.clone())
                        .unwrap();
                    return Err(PluginRuntimeError::PageRouteConflict {
                        path: decl.path.clone(),
                        first: existing_id,
                        second: id.clone(),
                    });
                }
                let pattern = RoutePattern(decl.path.clone());
                page_route_list.push((
                    pattern.clone(),
                    PageRouteEntry {
                        plugin_id: id.clone(),
                        handler_fn: decl.render_fn.clone(),
                        pool: loaded.pool.clone(),
                        bypass_layout: decl.bypass_layout,
                        title: decl.title.clone(),
                        pattern,
                    },
                ));
            }

            required_protocol_version =
                required_protocol_version.max(manifest.min_protocol_version);
            required_app_version = required_app_version.max(manifest.min_app_version);
            plugin_requirements.insert(
                id.clone(),
                PluginClientRequirement {
                    min_protocol_version: manifest.min_protocol_version,
                    min_app_version: manifest.min_app_version,
                    required_host_components: manifest.required_host_components.clone(),
                },
            );
        }

        for v in slots.values_mut() {
            v.sort_by_key(|e| std::cmp::Reverse(e.0));
        }
        for v in hooks.values_mut() {
            v.sort_by_key(|e| std::cmp::Reverse(e.0));
        }
        route_patterns.dedup();

        Ok(Registries {
            slots: SlotRegistry(slots),
            hooks: HookRegistry(hooks),
            transforms,
            override_map: OverrideMap {
                version: 0,
                overridden_components,
                transformed_slots,
                route_patterns,
                required_protocol_version,
                required_app_version,
                plugin_requirements,
                page_route_prefix: page_prefix.map(|s| s.to_owned()),
            },
            api_routes: ApiRegistry(api_route_map),
            page_routes: PageRouteRegistry(page_route_list),
        })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compare two `Selector` values for structural equality (enough for within-lookup).
///
/// Only handles the selectors that appear as outer targets in practice; returns false
/// for uncommon forms (Any, Within-as-outer) so they never match accidentally.
fn selectors_equal(a: &dioxus_extism_protocol::Selector, b: &dioxus_extism_protocol::Selector) -> bool {
    use dioxus_extism_protocol::Selector;
    match (a, b) {
        (Selector::Slot(x), Selector::Slot(y))
        | (Selector::Component(x), Selector::Component(y))
        | (Selector::DataPluginSlot(x), Selector::DataPluginSlot(y)) => x == y,
        (Selector::Route(x), Selector::Route(y)) => x == y,
        _ => false,
    }
}

/// Recursively apply a within-transform entry to all matching nodes in `view`.
///
/// Uses `Box::pin` for async recursion. On plugin call failure the matched node is
/// left unchanged (error isolation) and traversal continues.
fn traverse_and_apply(
    view: PluginView,
    selector: dioxus_extism_protocol::NodeSelector,
    op: TransformOp,
    transform_fn: String,
    pool: extism::Pool,
    context: TransformContext,
    session: SessionCtx,
) -> futures::future::BoxFuture<'static, PluginView> {
    use dioxus_extism_protocol::ViewElement;
    Box::pin(async move {
        let recursive = crate::tree::is_recursive_selector(&selector);
        let d = WithinDispatch {
            selector: &selector,
            op: &op,
            transform_fn: &transform_fn,
            pool: pool.clone(),
            context: &context,
            session: &session,
            recursive,
        };
        match view {
            PluginView::Element(el) => {
                let len = el.children.len();
                let mut new_children = Vec::with_capacity(len);
                for child in el.children {
                    apply_within_op_to_child(child, &d, &mut new_children).await;
                }
                PluginView::Element(ViewElement { children: new_children, ..el })
            }
            PluginView::Fragment(children) => {
                let mut new_children = Vec::with_capacity(children.len());
                for child in children {
                    apply_within_op_to_child(child, &d, &mut new_children).await;
                }
                PluginView::Fragment(new_children)
            }
            other => other,
        }
    })
}

/// Bundles the constant parameters for a within-transform dispatch pass.
struct WithinDispatch<'a> {
    selector: &'a dioxus_extism_protocol::NodeSelector,
    op: &'a TransformOp,
    transform_fn: &'a str,
    pool: extism::Pool,
    context: &'a TransformContext,
    session: &'a SessionCtx,
    recursive: bool,
}

/// Handle one child node: match, apply op, or recurse (if recursive selector).
async fn apply_within_op_to_child(
    child: PluginView,
    d: &WithinDispatch<'_>,
    out: &mut Vec<PluginView>,
) {
    use dioxus_extism_protocol::AttrValue;
    let selector = d.selector;
    let op = d.op;
    let transform_fn = d.transform_fn;
    let pool = d.pool.clone();
    let context = d.context;
    let session = d.session;
    let recursive = d.recursive;
    if crate::tree::node_matches(&child, selector) {
        // AddClass / SetAttr modify the node directly; no plugin call needed.
        match op {
            TransformOp::AddClass(cls) => {
                out.push(crate::tree::add_class_to_view(child, cls.clone()));
                return;
            }
            TransformOp::SetAttr(k, v) => {
                out.push(crate::tree::set_attr_on_view(child, k.clone(), AttrValue::String({
                    if let AttrValue::String(s) = v { s.clone() } else { String::new() }
                })));
                return;
            }
            _ => {}
        }

        let original = match op {
            TransformOp::Replace | TransformOp::WrapNode => Some(child.clone()),
            _ => None,
        };
        let input = TransformInput {
            original,
            context: context.clone(),
            session: session.clone(),
        };
        match call_export::<TransformInput, TransformOutput>(
            pool.clone(),
            transform_fn.to_string(),
            input,
            session.clone(),
        )
        .await
        {
            Ok(out_t) => match op {
                TransformOp::InsertBefore => {
                    out.push(out_t.view);
                    out.push(child);
                }
                TransformOp::InsertAfter => {
                    out.push(child);
                    out.push(out_t.view);
                }
                TransformOp::Replace => {
                    out.push(out_t.view);
                }
                TransformOp::WrapNode => {
                    out.push(crate::tree::resolve_target_in_view(out_t.view, child));
                }
                _ => out.push(child),
            },
            Err(e) => {
                tracing::warn!(
                    transform_fn = transform_fn,
                    error = %e,
                    "within-transform call failed, leaving node unchanged"
                );
                out.push(child);
            }
        }
    } else if recursive {
        // Non-matching child — recurse into it.
        let transformed = traverse_and_apply(
            child,
            selector.clone(),
            op.clone(),
            transform_fn.to_string(),
            pool,
            context.clone(),
            session.clone(),
        )
        .await;
        out.push(transformed);
    } else {
        out.push(child);
    }
}


/// Returns `true` if `view` contains a `HostComponent("__content__")` at any depth.
fn view_contains_content_placeholder(view: &PluginView) -> bool {
    match view {
        PluginView::HostComponent(r) if r.name == "__content__" => true,
        PluginView::Element(el) => el.children.iter().any(view_contains_content_placeholder),
        PluginView::Fragment(children) => children.iter().any(view_contains_content_placeholder),
        _ => false,
    }
}

// ── Call helper ───────────────────────────────────────────────────────────────

/// Call a WASM plugin export with JSON I/O on a blocking thread.
///
/// Sets the thread-local session context before calling so host function callbacks
/// can read the current session without per-instance `UserData`.
#[tracing::instrument(skip(pool, input, session), fields(export))]
async fn call_export<I, O>(
    pool: extism::Pool,
    export: impl Into<String>,
    input: I,
    session: SessionCtx,
) -> Result<O, PluginRuntimeError>
where
    I: Serialize + Send + 'static,
    O: DeserializeOwned + Send + 'static,
{
    let export: String = export.into();
    tokio::task::spawn_blocking(move || {
        host_functions::set_call_session(session.session_id.clone(), session.client.clone());
        let mut plugin = pool
            .get(Duration::from_secs(5))
            .map_err(|e| PluginRuntimeError::CallFailed { source: e })?
            .ok_or_else(|| PluginRuntimeError::Pool("timeout waiting for plugin instance".into()))?;
        let Json(result) = plugin
            .call::<Json<I>, Json<O>>(&export, Json(input))
            .map_err(|e| PluginRuntimeError::CallFailed { source: e })?;
        Ok(result)
    })
    .await
    .map_err(|e| PluginRuntimeError::TaskPanic(e.to_string()))?
}

// ── Fetch and verify ──────────────────────────────────────────────────────────

async fn fetch_and_verify(source: &PluginSource) -> Result<Vec<u8>, PluginRuntimeError> {
    match source {
        PluginSource::Bytes(bytes) => Ok(bytes.to_vec()),
        PluginSource::File(path) => std::fs::read(path).map_err(PluginRuntimeError::Io),
        PluginSource::Url { url, sha256 } => {
            use sha2::Digest;
            let bytes = reqwest::get(url.as_str())
                .await
                .map_err(|e| PluginRuntimeError::FetchFailed {
                    url: url.clone(),
                    message: e.to_string(),
                })?
                .bytes()
                .await
                .map_err(|e| PluginRuntimeError::FetchFailed {
                    url: url.clone(),
                    message: e.to_string(),
                })?
                .to_vec();

            let digest: [u8; 32] = sha2::Sha256::digest(&bytes).into();
            if digest != *sha256 {
                return Err(PluginRuntimeError::ChecksumMismatch { url: url.clone() });
            }
            Ok(bytes)
        }
    }
}

// ── StatePersistenceProvider ──────────────────────────────────────────────────

/// Persistence backend for `GlobalScope` plugin state.
#[async_trait]
pub trait StatePersistenceProvider: Send + Sync + 'static {
    async fn save(
        &self,
        plugin_id: &PluginId,
        state: &HashMap<String, serde_json::Value>,
    ) -> Result<(), PersistenceError>;

    async fn load(
        &self,
        plugin_id: &PluginId,
    ) -> Result<Option<HashMap<String, serde_json::Value>>, PersistenceError>;
}

// ── PluginRuntimeBuilder ──────────────────────────────────────────────────────

/// Builder for `PluginRuntime`.
#[derive(Default)]
pub struct PluginRuntimeBuilder {
    sources: Vec<(PluginSource, PluginInstallConfig)>,
    extra_host_fns: Vec<extism::Function>,
    wasm_cache_path: Option<PathBuf>,
    session_ttl: Option<Duration>,
    invocations: Vec<(String, InvocationHandler, Duration)>,
    persistence: Option<Arc<dyn StatePersistenceProvider>>,
    plugin_page_prefix: Option<String>,
}

impl PluginRuntimeBuilder {
    /// Create an empty builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a plugin using its `PriorityHint`s as declared.
    #[must_use]
    pub fn add_plugin(self, source: PluginSource) -> Self {
        self.add_plugin_with_config(source, PluginInstallConfig::default())
    }

    /// Load a plugin and override ALL of its priorities with one absolute value.
    #[must_use]
    pub fn add_plugin_with_priority(self, source: PluginSource, priority: i32) -> Self {
        self.add_plugin_with_config(
            source,
            PluginInstallConfig {
                base_priority: Some(priority),
                ..Default::default()
            },
        )
    }

    /// Load a plugin with fine-grained per-contribution priority overrides.
    #[must_use]
    pub fn add_plugin_with_config(mut self, source: PluginSource, config: PluginInstallConfig) -> Self {
        self.sources.push((source, config));
        self
    }

    /// Add an additional host function exposed to all plugins.
    #[must_use]
    pub fn add_host_function(mut self, f: extism::Function) -> Self {
        self.extra_host_fns.push(f);
        self
    }

    /// Set a filesystem cache directory for compiled WASM modules.
    #[must_use]
    pub fn with_wasm_cache(mut self, path: impl Into<PathBuf>) -> Self {
        self.wasm_cache_path = Some(path.into());
        self
    }

    /// Configure session state TTL (default: 24 hours).
    #[must_use]
    pub const fn with_session_ttl(mut self, ttl: Duration) -> Self {
        self.session_ttl = Some(ttl);
        self
    }

    /// Set the URL prefix under which plugin-declared page routes are served.
    ///
    /// The host must also add a matching catch-all route to their Dioxus `Route` enum:
    /// ```ignore
    /// #[route("/p/:..segments")]
    /// PluginPage { segments: Vec<String> },
    /// ```
    /// Replace `"/p"` with whatever prefix you choose here.
    #[must_use]
    pub fn with_plugin_page_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.plugin_page_prefix = Some(prefix.into());
        self
    }

    /// Provide a persistence backend for `GlobalScope` plugin state.
    #[must_use]
    pub fn with_state_persistence(mut self, provider: impl StatePersistenceProvider) -> Self {
        self.persistence = Some(Arc::new(provider));
        self
    }

    /// Register a named invocation handler.
    #[must_use]
    pub fn register_invocation<Args, Ret, Fut>(
        mut self,
        name: impl Into<String>,
        timeout: Option<Duration>,
        handler: impl Fn(Args, SessionCtx) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        Args: DeserializeOwned + Send + 'static,
        Ret: Serialize + Send + 'static,
        Fut: std::future::Future<Output = Result<Ret, InvocationError>> + Send + 'static,
    {
        let handler: InvocationHandler = Arc::new(move |raw_args, session| {
            let result = serde_json::from_value::<Args>(raw_args);
            match result {
                Ok(args) => {
                    let fut = handler(args, session);
                    Box::pin(async move {
                        fut.await.and_then(|ret| {
                            serde_json::to_value(ret).map_err(InvocationError::BadArgs)
                        })
                    }) as BoxFuture<'static, _>
                }
                Err(e) => Box::pin(async move { Err(InvocationError::BadArgs(e)) }),
            }
        });
        let timeout = timeout.unwrap_or(Duration::from_secs(30));
        self.invocations.push((name.into(), handler, timeout));
        self
    }

    /// Build the `PluginRuntime`, loading all declared plugins.
    ///
    /// # Errors
    ///
    /// Returns an error if any plugin fails to load, compile, or has an incompatible
    /// protocol version, unregistered invocation capability, or checksum mismatch.
    #[allow(clippy::too_many_lines)]
    pub async fn build(self) -> Result<Arc<PluginRuntime>, PluginRuntimeError> {
        let global_states: Arc<RwLock<GlobalStateMap>> = Arc::new(RwLock::new(HashMap::new()));
        let session_states: Arc<RwLock<SessionStateMap>> = Arc::new(RwLock::new(HashMap::new()));
        let session_last_access: Arc<RwLock<HashMap<SessionId, std::time::Instant>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let mut invocation_registry = InvocationRegistry::new();
        for (name, handler, timeout) in self.invocations {
            invocation_registry.handlers.insert(name, (handler, timeout));
        }
        let invocation_registry = Arc::new(invocation_registry);

        let (event_tx, event_rx) = mpsc::unbounded_channel::<(PluginEvent, SessionCtx)>();

        let mut all_plugins: IndexMap<PluginId, LoadedPlugin> = IndexMap::new();

        for (source, config) in self.sources {
            let plugin_manifest = load_plugin_manifest(&source).await?;

            if plugin_manifest.min_protocol_version > PROTOCOL_VERSION {
                return Err(PluginRuntimeError::ProtocolVersionMismatch {
                    required: plugin_manifest.min_protocol_version,
                    host: PROTOCOL_VERSION,
                });
            }

            let mut granted_invocations = HashSet::new();
            let mut granted_global_read = HashSet::new();
            let mut granted_global_write = HashSet::new();
            let mut granted_http_hosts: Vec<String> = Vec::new();

            for cap in &plugin_manifest.host_capabilities {
                match cap {
                    HostCapability::Invoke { names } => {
                        for name in names {
                            if !invocation_registry.handlers.contains_key(name.as_str()) {
                                return Err(PluginRuntimeError::CapabilityDenied {
                                    plugin: plugin_manifest.id.clone(),
                                    capability: format!("Invoke({name}): not registered"),
                                });
                            }
                            granted_invocations.insert(name.clone());
                        }
                    }
                    HostCapability::GlobalStateRead { keys } => {
                        granted_global_read.extend(keys.iter().cloned());
                    }
                    HostCapability::GlobalStateWrite { keys } => {
                        granted_global_write.extend(keys.iter().cloned());
                    }
                    HostCapability::Http { allowed_hosts } => {
                        granted_http_hosts.extend(allowed_hosts.iter().cloned());
                    }
                    _ => {}
                }
            }

            let ctx = CallCtx {
                caller: plugin_manifest.id.clone(),
                session_states: session_states.clone(),
                session_last_access: session_last_access.clone(),
                global_states: global_states.clone(),
                invocation_registry: invocation_registry.clone(),
                persistence: self.persistence.clone(),
                granted_invocations,
                granted_global_read,
                granted_global_write,
                granted_http_hosts,
                event_tx: event_tx.clone(),
            };
            let user_data = extism::UserData::new(ctx);
            let ctx_arc = user_data
                .get()
                .map_err(|e| PluginRuntimeError::Pool(format!("UserData::get failed: {e}")))?;

            let mut all_host_fns = make_host_functions(user_data.clone());
            all_host_fns.extend(self.extra_host_fns.clone());

            let pool_size = config
                .pool_size
                .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, usize::from));

            let bytes = fetch_and_verify(&source).await?;
            let ext_manifest = extism::Manifest::new([extism::Wasm::data(bytes)]);
            let fns = all_host_fns;

            let pool = extism::Pool::new_from_builder(
                move || {
                    extism::PluginBuilder::new(ext_manifest.clone())
                        .with_wasi(true)
                        .with_functions(fns.clone())
                        .build()
                },
                extism::PoolBuilder::default().with_max_instances(pool_size),
            );

            // Call on_load if exported — failure aborts build.
            let has_on_load = pool
                .function_exists("on_load", Duration::from_secs(5))
                .map_err(|e| PluginRuntimeError::CallFailed { source: e })?;

            if has_on_load {
                let pool_clone = pool.clone();
                let init_session = SessionCtx {
                    session_id: SessionId("__init__".into()),
                    user_id: None,
                    client: ClientCapabilities {
                        protocol_version: PROTOCOL_VERSION,
                        app_version: 0,
                        registered_host_components: vec![],
                    },
                    caller: None,
                };
                let init_session_clone = init_session.clone();
                tokio::task::spawn_blocking(move || {
                    host_functions::set_call_session(
                        init_session_clone.session_id.clone(),
                        init_session_clone.client.clone(),
                    );
                    let mut p = pool_clone
                        .get(Duration::from_secs(5))
                        .map_err(|e| PluginRuntimeError::CallFailed { source: e })?
                        .ok_or_else(|| {
                            PluginRuntimeError::Pool("timeout on on_load".into())
                        })?;
                    p.call::<Json<SessionCtx>, ()>("on_load", Json(init_session))
                        .map_err(|e| PluginRuntimeError::CallFailed { source: e })
                })
                .await
                .map_err(|e| PluginRuntimeError::TaskPanic(e.to_string()))??;
            }

            all_plugins.insert(
                plugin_manifest.id.clone(),
                LoadedPlugin {
                    manifest: plugin_manifest,
                    pool,
                    enabled: AtomicBool::new(true),
                    config,
                    ctx_arc,
                },
            );
        }

        // Restore global state from persistence before returning.
        if let Some(p) = &self.persistence {
            for id in all_plugins.keys() {
                if let Ok(Some(state)) = p.load(id).await {
                    global_states.write().await.insert(id.clone(), state);
                }
            }
        }

        let event_bus = EventBus::build_from_plugins(&all_plugins);
        let registries =
            PluginRuntime::build_registries(&all_plugins, self.plugin_page_prefix.as_deref())?;
        let (override_map_tx, _) = broadcast::channel::<OverrideMap>(32);

        let runtime = Arc::new(PluginRuntime {
            plugins: RwLock::new(all_plugins),
            global_states,
            session_states,
            session_last_access: session_last_access.clone(),
            event_bus: RwLock::new(event_bus),
            registries: RwLock::new(registries),
            invocation_registry,
            override_map_tx,
            persistence: self.persistence,
            plugin_page_prefix: self.plugin_page_prefix,
            event_tx,
        });

        // Event dispatch task: receives plugin-emitted events and fans them out.
        {
            let mut event_rx = event_rx;
            let rt = Arc::clone(&runtime);
            tokio::spawn(async move {
                while let Some((event, session)) = event_rx.recv().await {
                    if let Err(e) = rt.emit_event(event, &session).await {
                        tracing::warn!("event dispatch error: {e}");
                    }
                }
            });
        }

        // Session TTL eviction background task.
        let ttl = self.session_ttl.unwrap_or(Duration::from_hours(24));
        {
            let states = runtime.session_states.clone();
            let last_access = session_last_access;
            tokio::spawn(async move {
                let interval_dur = ttl / 4;
                let mut interval = tokio::time::interval(interval_dur);
                loop {
                    interval.tick().await;
                    let cutoff = std::time::Instant::now().checked_sub(ttl);
                    let Some(cutoff) = cutoff else { continue };
                    let expired: Vec<SessionId> = {
                        let access = last_access.read().await;
                        access
                            .iter()
                            .filter(|&(_, &last)| last < cutoff)
                            .map(|(id, _)| id.clone())
                            .collect()
                    };
                    for id in &expired {
                        states.write().await.remove(id);
                        last_access.write().await.remove(id);
                    }
                }
            });
        }

        Ok(runtime)
    }
}

/// Read the `manifest` export from a plugin source.
///
/// The manifest function is pure and never calls host functions, but the WASM binary
/// may declare host imports at the module level. Stubs are provided to satisfy the
/// linker without wiring up real state.
async fn load_plugin_manifest(source: &PluginSource) -> Result<PluginManifest, PluginRuntimeError> {
    let bytes = fetch_and_verify(source).await?;
    let ext_manifest = extism::Manifest::new([extism::Wasm::data(bytes)]);
    let stubs = host_functions::make_stub_host_functions();
    tokio::task::spawn_blocking(move || {
        let mut plugin = extism::PluginBuilder::new(ext_manifest)
            .with_wasi(true)
            .with_functions(stubs)
            .build()
            .map_err(|e| PluginRuntimeError::CallFailed { source: e })?;
        let Json(manifest) = plugin
            .call::<(), Json<PluginManifest>>("manifest", ())
            .map_err(|e| PluginRuntimeError::CallFailed { source: e })?;
        Ok(manifest)
    })
    .await
    .map_err(|e| PluginRuntimeError::TaskPanic(e.to_string()))?
}

// ── Plugin API router ────────────────────────────────────────────────────────

impl PluginRuntime {
    /// Build an `axum::Router` containing all inbound HTTP API routes declared by plugins.
    ///
    /// Call this **before** `serve_dioxus_application` — that call installs a fallback
    /// handler which seals the router against further route or merge operations.
    ///
    /// # Note
    /// API route closures capture pool clones at startup time. Plugin hot-reload rebuilds
    /// the internal `ApiRegistry` but cannot update the already-running Axum router.
    /// API routes are therefore static for the lifetime of the server process.
    pub async fn api_router<S>(&self) -> axum::Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        use axum::extract::{Path, Query};
        use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
        use axum::body::Bytes;
        use axum::response::IntoResponse as _;
        use dioxus_extism_protocol::{ApiRequest, ApiResponse};

        let entries: Vec<((String, String), ApiRouteEntry)> = {
            let regs = self.registries.read().await;
            regs.api_routes.0.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        };

        let mut router = axum::Router::<S>::new();

        for ((method_str, path_str), entry) in entries {
            let axum_path = colon_params_to_braces(&path_str);

            let handler = {
                let entry = entry.clone();
                move |
                    Path(path_params): Path<HashMap<String, String>>,
                    Query(query_params): Query<HashMap<String, String>>,
                    headers: HeaderMap,
                    body: Bytes,
                | {
                    let pool = entry.pool.clone();
                    let handler_fn = entry.handler_fn.clone();
                    let plugin_id = entry.plugin_id.clone();
                    async move {
                        let body_json = if body.is_empty() {
                            None
                        } else {
                            serde_json::from_slice::<serde_json::Value>(&body).ok()
                        };
                        let headers_map: HashMap<String, String> = headers
                            .iter()
                            .filter_map(|(k, v)| {
                                v.to_str().ok().map(|v| (k.as_str().to_owned(), v.to_owned()))
                            })
                            .collect();
                        let request = ApiRequest {
                            path_params,
                            query_params,
                            headers: headers_map,
                            body: body_json,
                        };
                        let stub = SessionCtx {
                            session_id: SessionId("__api__".into()),
                            user_id: None,
                            client: ClientCapabilities {
                                protocol_version: PROTOCOL_VERSION,
                                app_version: 0,
                                registered_host_components: vec![],
                            },
                            caller: None,
                        };
                        match call_export::<ApiRequest, ApiResponse>(
                            pool,
                            handler_fn,
                            request,
                            stub,
                        )
                        .await
                        {
                            Ok(resp) => {
                                let status = StatusCode::from_u16(resp.status)
                                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                                let body_val =
                                    resp.body.unwrap_or(serde_json::Value::Null);
                                let mut response =
                                    (status, axum::Json(body_val)).into_response();
                                for (k, v) in resp.headers {
                                    if let (Ok(name), Ok(val)) = (
                                        HeaderName::from_bytes(k.as_bytes()),
                                        HeaderValue::from_str(&v),
                                    ) {
                                        response.headers_mut().insert(name, val);
                                    }
                                }
                                response
                            }
                            Err(e) => {
                                tracing::error!(
                                    plugin = %plugin_id.0,
                                    error = %e,
                                    "plugin API route handler failed"
                                );
                                (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    axum::Json(
                                        serde_json::json!({"error": e.to_string()}),
                                    ),
                                )
                                    .into_response()
                            }
                        }
                    }
                }
            };

            let method_router = match method_str.as_str() {
                "GET"    => axum::routing::get(handler),
                "POST"   => axum::routing::post(handler),
                "PUT"    => axum::routing::put(handler),
                "PATCH"  => axum::routing::patch(handler),
                "DELETE" => axum::routing::delete(handler),
                other => {
                    tracing::warn!("api_router: unknown method {other}, skipping {path_str}");
                    continue;
                }
            };
            router = router.route(&axum_path, method_router);
        }

        router
    }
}

/// Convert `:param` path segments to `{param}` for Axum 0.8 syntax.
///
/// Example: `"/api/notes/:id"` → `"/api/notes/{id}"`
fn colon_params_to_braces(path: &str) -> String {
    path.split('/')
        .map(|seg| {
            seg.strip_prefix(':')
                .map_or_else(|| seg.to_owned(), |name| format!("{{{name}}}"))
        })
        .collect::<Vec<_>>()
        .join("/")
}

// ── Plugin page route rendering ───────────────────────────────────────────────

impl PluginRuntime {
    /// Render a plugin-declared page route, returning `None` if no plugin owns the path.
    ///
    /// `relative_path` is the path **after** the host's configured prefix, e.g. `"/notes"`.
    /// Path parameters are extracted from the declared pattern automatically.
    ///
    /// # Errors
    /// Returns `PluginRuntimeError` if the plugin call fails.
    pub async fn render_page_route(
        &self,
        relative_path: &str,
        session: &SessionCtx,
    ) -> Result<Option<dioxus_extism_protocol::PageRouteOutput>, PluginRuntimeError> {
        use dioxus_extism_protocol::{PageRouteInput, PageRouteOutput};

        let result = {
            let regs = self.registries.read().await;
            regs.page_routes.find(relative_path)
        };

        let Some((entry, path_params)) = result else {
            return Ok(None);
        };

        let input = PageRouteInput {
            path_params,
            query_params: HashMap::new(),
            session: session.clone(),
        };

        let view = call_export::<PageRouteInput, PluginView>(
            entry.pool.clone(),
            entry.handler_fn.clone(),
            input,
            session.clone(),
        )
        .await?;

        Ok(Some(PageRouteOutput {
            view,
            bypass_layout: entry.bypass_layout,
            title: entry.title.clone(),
        }))
    }
}

// ── PluginRuntimeExt for axum::Router ────────────────────────────────────────

/// Extension trait for wiring `PluginRuntime` into an Axum router.
pub trait PluginRuntimeExt {
    /// Add `PluginRuntime` as an Axum layer so server functions can extract it.
    #[must_use]
    fn with_plugin_runtime(self, runtime: Arc<PluginRuntime>) -> Self;
}

impl PluginRuntimeExt for axum::Router {
    fn with_plugin_runtime(self, runtime: Arc<PluginRuntime>) -> Self {
        self.layer(axum::Extension(runtime))
    }
}
