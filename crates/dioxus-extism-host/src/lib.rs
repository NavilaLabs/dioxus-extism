// Phase 1 skeleton — many items are unused until Phase 2 WASM loading.
#![allow(dead_code)]

mod error;
mod host_functions;
mod runtime;

pub use error::{InvocationError, PersistenceError, PluginRuntimeError};
pub use runtime::{
    GlobalStateMap, PluginInstallConfig, PluginRuntime, PluginRuntimeBuilder, PluginRuntimeExt,
    PluginSource, SessionStateMap, StatePersistenceProvider,
};
