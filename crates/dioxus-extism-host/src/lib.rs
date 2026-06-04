// Phase 1 skeleton — many items are unused until Phase 2 WASM loading.
#![allow(dead_code)]

pub mod bundle;
pub mod dep_graph;
mod error;
mod host_functions;
mod manifest_extension;
mod persistence;
mod runtime;
mod trust;
pub mod tree;

pub use bundle::{BundleId, BundleManifest, BundleSource, BundleTrustGroup};
pub use dep_graph::DepGraph;
pub use host_functions::PluginDispatch;
pub use error::{InstallError, InvocationError, PersistenceError, PluginRuntimeError};
pub use dioxus_extism_protocol::RouteTransforms;
pub use manifest_extension::{ManifestExtensionError, ManifestExtensionHandler, OnUnknownExtension};
pub use trust::{TrustKey, TrustTag};
pub use persistence::JsonFilePersistence;
pub use runtime::{
    CallOutcome, CapabilityCheckFn, CrossPluginAuditSink, CrossPluginCallEvent,
    GlobalStateMap, GrantPolicyFn, HookOutcome, PluginInstallConfig, PluginRuntime,
    PluginRuntimeBuilder, PluginRuntimeExt, PluginSource, PluginSummary, RouteReplacePolicyFn,
    RuntimeMetrics, SessionStateMap, StatePersistenceProvider, TransformEntry, TransformRegistry,
};
