use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::AtomicBool,
    },
    time::Duration,
};

use async_trait::async_trait;
use dioxus_extism_protocol::{
    OverrideMap, PriorityHint, PluginId, PluginManifest, SessionId,
};
use futures::future::BoxFuture;
use indexmap::IndexMap;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{RwLock, broadcast};

use crate::error::{InvocationError, PersistenceError, PluginRuntimeError};

// ── State type aliases ───────────────────────────────────────────────────────

pub type PluginState = HashMap<String, serde_json::Value>;
pub type GlobalStateMap = HashMap<PluginId, PluginState>;
pub type SessionStateMap = HashMap<SessionId, HashMap<PluginId, PluginState>>;

// ── Plugin source ────────────────────────────────────────────────────────────

/// Where to load plugin WASM bytes from.
pub enum PluginSource {
    File(PathBuf),
    /// Remote WASM binary — SHA-256 checksum is mandatory.
    Url {
        url: String,
        sha256: [u8; 32],
    },
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
            dioxus_extism_protocol::SessionCtx,
        ) -> BoxFuture<'static, Result<serde_json::Value, InvocationError>>
        + Send
        + Sync,
>;

pub(crate) struct InvocationRegistry {
    handlers: HashMap<String, (InvocationHandler, Duration)>,
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
        session: dioxus_extism_protocol::SessionCtx,
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

pub(crate) struct EventBus;

// ── PluginRuntime ─────────────────────────────────────────────────────────────

/// The central server-side plugin runtime.
pub struct PluginRuntime {
    pub(crate) plugins: RwLock<IndexMap<PluginId, LoadedPlugin>>,
    pub(crate) global_states: Arc<RwLock<GlobalStateMap>>,
    pub(crate) session_states: Arc<RwLock<SessionStateMap>>,
    pub(crate) _event_bus: Arc<EventBus>,
    pub(crate) registries: RwLock<Registries>,
    pub(crate) _invocation_registry: Arc<InvocationRegistry>,
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

    fn build_registries(plugins: &IndexMap<PluginId, LoadedPlugin>) -> Registries {
        use dioxus_extism_protocol::{RoutePattern, PluginClientRequirement};
        use std::collections::{HashSet, HashMap};

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
                use dioxus_extism_protocol::Selector;
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

        // Sort each registry by priority descending.
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
        // stored for Phase 2
        self
    }

    /// Register a named invocation handler.
    #[must_use]
    pub fn register_invocation<Args, Ret, Fut>(
        mut self,
        name: impl Into<String>,
        timeout: Option<Duration>,
        handler: impl Fn(Args, dioxus_extism_protocol::SessionCtx) -> Fut + Send + Sync + 'static,
    ) -> Self
    where
        Args: DeserializeOwned + Send + 'static,
        Ret: Serialize + Send + 'static,
        Fut: std::future::Future<Output = Result<Ret, InvocationError>> + Send + 'static,
    {
        let handler: InvocationHandler = Arc::new(move |raw_args, session| {
            let result = serde_json::from_value::<Args>(raw_args);
            let fut = match result {
                Ok(args) => {
                    let fut = handler(args, session);
                    Box::pin(async move {
                        fut.await.and_then(|ret| {
                            serde_json::to_value(ret).map_err(InvocationError::BadArgs)
                        })
                    }) as BoxFuture<'static, _>
                }
                Err(e) => Box::pin(async move { Err(InvocationError::BadArgs(e)) }),
            };
            fut
        });
        let timeout = timeout.unwrap_or(Duration::from_secs(30));
        self.invocations.push((name.into(), handler, timeout));
        self
    }

    /// Build the `PluginRuntime`, loading all declared plugins.
    pub async fn build(self) -> Result<Arc<PluginRuntime>, PluginRuntimeError> {
        let (override_map_tx, _) = broadcast::channel::<OverrideMap>(32);

        let mut invocation_registry = InvocationRegistry::new();
        for (name, handler, timeout) in self.invocations {
            invocation_registry.handlers.insert(name, (handler, timeout));
        }

        let runtime = Arc::new(PluginRuntime {
            plugins: RwLock::new(IndexMap::new()),
            global_states: Arc::new(RwLock::new(HashMap::new())),
            session_states: Arc::new(RwLock::new(HashMap::new())),
            _event_bus: Arc::new(EventBus),
            registries: RwLock::new(Registries {
                slots: SlotRegistry(HashMap::new()),
                hooks: HookRegistry(HashMap::new()),
                transforms: TransformRegistry,
                override_map: OverrideMap::default(),
            }),
            _invocation_registry: Arc::new(invocation_registry),
            override_map_tx,
        });

        // Phase 1: no WASM loading yet — sources are reserved for Phase 2.
        // Validate that no sources were provided (Phase 1 skeleton).
        if !self.sources.is_empty() {
            tracing::warn!(
                "PluginRuntimeBuilder: {} plugin source(s) provided but WASM loading \
                 is not implemented in Phase 1 skeleton.",
                self.sources.len()
            );
        }

        Ok(runtime)
    }
}

// ── PluginRuntimeExt for axum::Router ────────────────────────────────────────

/// Extension trait for wiring `PluginRuntime` into an Axum router.
pub trait PluginRuntimeExt {
    fn with_plugin_runtime(self, runtime: Arc<PluginRuntime>) -> Self;
}

impl PluginRuntimeExt for axum::Router {
    fn with_plugin_runtime(self, runtime: Arc<PluginRuntime>) -> Self {
        // Use axum::Extension so the router type doesn't change.
        // Server functions extract it with: extract::<Extension<Arc<PluginRuntime>>>()
        // or via the Dioxus fullstack with_axum_state mechanism.
        self.layer(axum::Extension(runtime))
    }
}
