//! `dioxus-extism` — thin re-export crate.
//!
//! Enable features: `host`, `frontend`, `pdk`, `test`.

pub use dioxus_extism_protocol as protocol;

#[cfg(feature = "host")]
pub use dioxus_extism_host as host;

#[cfg(feature = "frontend")]
pub use dioxus_extism_frontend as frontend;

#[cfg(feature = "pdk")]
pub use dioxus_extism_pdk as pdk;

#[cfg(feature = "test")]
pub use dioxus_extism_test as test_utils;
