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
    ClientCapabilities, HandlerId, HookCall, HookResult, HostCapability, OverrideMap, PluginEvent,
    PluginId, PluginManifest, PluginView, PriorityHint, SessionCtx, SessionId, SlotContent,
    ViewUpdate, PROTOCOL_VERSION,
};
use extism::convert::Json;
use futures::future::BoxFuture;
use indexmap::IndexMap;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{RwLock, broadcast};

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
    /// Order: per-name override > base_priority > hint.
    pub fn resolve(&self, name: &str, hint: &PriorityHint) -> i32 {
        self.overrides
            .get(name)
            .copied()
            .or(self.base_priority)
            .unwrap_or_else(|| hint.as_numeric())
    }
}

// ── Loaded plugin ─────────────────────────────────────────────────────────────

pub(crate) struct LoadedPlugin {
    pub(crate) manifest: PluginManifest,
    pub(crate) pool: extism::Pool,
    pub(crate) enabled: AtomicBool,
    pub(crate) config: PluginInstallConfig,
    /// Shared context for host function callbacks. Updated before each call.
    pub(crate) ctx_arc: Arc<std::sync::Mutex<CallCtx>>,
}

// ── Registries ────────────────────────────────────────────────────────────────

pub(crate) struct SlotRegistry(pub(crate) HashMap<String, Vec<(i32, PluginId)>>);
pub(crate) struct HookRegistry(pub(crate) HashMap<String, Vec<(i32, PluginId)>>);
pub(crate) struct TransformRegistry;

pub(crate) struct Registries {
    pub(crate) slots: SlotRegistry,
    pub(crate) hooks: HookRegistry,
    pub(crate) transforms: TransformRegistry,
    pub(crate) override_map: OverrideMap,
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
pub(crate) struct InvocationRegistry {
    pub(crate) handlers: HashMap<String, (InvocationHandler, Duration)>,
}

impl InvocationRegistry {
    fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

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

// ── Event bus ─────────────────────────────────────────────────────────────────

pub(crate) struct EventBus {
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
            v.sort_by(|a, b| b.0.cmp(&a.0));
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
    pub(crate) event_bus: RwLock<EventBus>,
    pub(crate) registries: RwLock<Registries>,
    pub(crate) invocation_registry: Arc<InvocationRegistry>,
    pub(crate) override_map_tx: broadcast::Sender<OverrideMap>,
}

impl PluginRuntime {
    /// Returns the cached `OverrideMap` without recomputation.
    pub async fn override_map(&self) -> OverrideMap {
        self.registries.read().await.override_map.clone()
    }

    /// Subscribe to `OverrideMap` change notifications (used by the SSE endpoint).
    pub fn override_map_updates(&self) -> broadcast::Receiver<OverrideMap> {
        self.override_map_tx.subscribe()
    }

    /// Collect slot contributions from all registered plugins, with error isolation.
    ///
    /// Plugins that fail to render serve a `PluginView::Incompatible` entry instead
    /// of aborting the entire slot.
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
                        reason: format!("plugin {:?} is disabled", plugin_id),
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

        Ok(results)
    }

    /// Run the named hook chain, threading context through each plugin in priority order.
    ///
    /// Any plugin that fails is skipped (error isolation); a plugin can cancel the entire
    /// chain by returning `HookResult::Cancel`.
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
                Ok(HookResult::Continue { context: c }) => current = c,
                Ok(HookResult::Replace { context: c }) => current = c,
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

    pub(crate) fn build_registries(plugins: &IndexMap<PluginId, LoadedPlugin>) -> Registries {
        use dioxus_extism_protocol::{PluginClientRequirement, RoutePattern, Selector};
        use std::collections::{HashMap, HashSet};

        let mut slots: HashMap<String, Vec<(i32, PluginId)>> = HashMap::new();
        let mut hooks: HashMap<String, Vec<(i32, PluginId)>> = HashMap::new();
        let mut overridden_components: HashSet<String> = HashSet::new();
        let mut transformed_slots: HashSet<String> = HashSet::new();
        let mut route_patterns: Vec<RoutePattern> = Vec::new();
        let mut required_protocol_version: u32 = 0;
        let mut required_app_version: u32 = 0;
        let mut plugin_requirements: HashMap<PluginId, PluginClientRequirement> = HashMap::new();

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
                match &transform.selector {
                    Selector::Component(name) => {
                        overridden_components.insert(name.clone());
                    }
                    Selector::Slot(name) => {
                        transformed_slots.insert(name.clone());
                    }
                    Selector::Route(pattern) => {
                        route_patterns.push(pattern.clone());
                    }
                    _ => {}
                }
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
            v.sort_by(|a, b| b.0.cmp(&a.0));
        }
        for v in hooks.values_mut() {
            v.sort_by(|a, b| b.0.cmp(&a.0));
        }
        route_patterns.dedup();

        Registries {
            slots: SlotRegistry(slots),
            hooks: HookRegistry(hooks),
            transforms: TransformRegistry,
            override_map: OverrideMap {
                version: 0,
                overridden_components,
                transformed_slots,
                route_patterns,
                required_protocol_version,
                required_app_version,
                plugin_requirements,
            },
        }
    }
}

// ── Call helper ───────────────────────────────────────────────────────────────

/// Call a WASM plugin export with JSON I/O on a blocking thread.
///
/// Sets the thread-local session context before calling so host function callbacks
/// can read the current session without per-instance UserData.
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
            .get(Duration::from_millis(5000))
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

            use sha2::Digest;
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
    pub fn with_session_ttl(mut self, ttl: Duration) -> Self {
        self.session_ttl = Some(ttl);
        self
    }

    /// Provide a persistence backend for `GlobalScope` plugin state.
    #[must_use]
    pub fn with_state_persistence(self, _provider: impl StatePersistenceProvider) -> Self {
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
    pub async fn build(self) -> Result<Arc<PluginRuntime>, PluginRuntimeError> {
        let global_states: Arc<RwLock<GlobalStateMap>> = Arc::new(RwLock::new(HashMap::new()));
        let session_states: Arc<RwLock<SessionStateMap>> = Arc::new(RwLock::new(HashMap::new()));

        let mut invocation_registry = InvocationRegistry::new();
        for (name, handler, timeout) in self.invocations {
            invocation_registry.handlers.insert(name, (handler, timeout));
        }
        let invocation_registry = Arc::new(invocation_registry);

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
                    _ => {}
                }
            }

            let ctx = CallCtx {
                caller: plugin_manifest.id.clone(),
                session_states: session_states.clone(),
                global_states: global_states.clone(),
                invocation_registry: invocation_registry.clone(),
                granted_invocations,
                granted_global_read,
                granted_global_write,
            };
            let user_data = extism::UserData::new(ctx);
            let ctx_arc = user_data
                .get()
                .map_err(|e| PluginRuntimeError::Pool(format!("UserData::get failed: {e}")))?;

            let mut all_host_fns = make_host_functions(user_data.clone());
            all_host_fns.extend(self.extra_host_fns.clone());

            let pool_size = config
                .pool_size
                .unwrap_or_else(|| std::thread::available_parallelism().map(usize::from).unwrap_or(4));

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
                .function_exists("on_load", Duration::from_millis(5000))
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
                        .get(Duration::from_millis(5000))
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

        let event_bus = EventBus::build_from_plugins(&all_plugins);
        let registries = PluginRuntime::build_registries(&all_plugins);
        let (override_map_tx, _) = broadcast::channel::<OverrideMap>(32);

        Ok(Arc::new(PluginRuntime {
            plugins: RwLock::new(all_plugins),
            global_states,
            session_states,
            event_bus: RwLock::new(event_bus),
            registries: RwLock::new(registries),
            invocation_registry,
            override_map_tx,
        }))
    }
}

/// Read the `manifest` export from a plugin source without registering host functions.
/// Manifest exports are expected to be pure — no host function calls.
async fn load_plugin_manifest(source: &PluginSource) -> Result<PluginManifest, PluginRuntimeError> {
    let bytes = fetch_and_verify(source).await?;
    let ext_manifest = extism::Manifest::new([extism::Wasm::data(bytes)]);
    tokio::task::spawn_blocking(move || {
        let mut plugin = extism::PluginBuilder::new(ext_manifest)
            .with_wasi(true)
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

// ── PluginRuntimeExt for axum::Router ────────────────────────────────────────

/// Extension trait for wiring `PluginRuntime` into an Axum router.
pub trait PluginRuntimeExt {
    fn with_plugin_runtime(self, runtime: Arc<PluginRuntime>) -> Self;
}

impl PluginRuntimeExt for axum::Router {
    fn with_plugin_runtime(self, runtime: Arc<PluginRuntime>) -> Self {
        self.layer(axum::Extension(runtime))
    }
}
