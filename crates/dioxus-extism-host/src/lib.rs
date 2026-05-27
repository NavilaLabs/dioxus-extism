// Phase 1 skeleton — many items are unused until Phase 2 WASM loading.
#![allow(dead_code)]

mod error;
mod host_functions;
mod manifest_extension;
mod persistence;
mod runtime;
pub mod tree;

pub use error::{InvocationError, PersistenceError, PluginRuntimeError};
pub use dioxus_extism_protocol::RouteTransforms;
pub use manifest_extension::{ManifestExtensionError, ManifestExtensionHandler, OnUnknownExtension};
pub use persistence::JsonFilePersistence;
pub use runtime::{
    CapabilityCheckFn, GlobalStateMap, HookOutcome, PluginInstallConfig, PluginRuntime,
    PluginRuntimeBuilder, PluginRuntimeExt, PluginSource, SessionStateMap,
    StatePersistenceProvider, TransformEntry, TransformRegistry,
};
