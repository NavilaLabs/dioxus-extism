use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureEchoFn;

impl DioxusPlugin for FixtureEchoFn {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/echo-fn".into()),
            version: "0.1.0".into(),
            min_protocol_version: PROTOCOL_VERSION,
            ..Default::default()
        }
    }
}

plugin! { type: FixtureEchoFn }

/// Reflects its JSON input back unchanged — used to verify `call_plugin` round-trip.
#[extism_pdk::plugin_fn]
pub fn echo_fn(
    input: extism_pdk::Json<serde_json::Value>,
) -> extism_pdk::FnResult<extism_pdk::Json<serde_json::Value>> {
    Ok(extism_pdk::Json(input.0))
}

/// Expects `{"n": <i64>}` and returns `{"result": <n+1>}`.
#[extism_pdk::plugin_fn]
pub fn compute_fn(
    input: extism_pdk::Json<serde_json::Value>,
) -> extism_pdk::FnResult<extism_pdk::Json<serde_json::Value>> {
    let n = input.0["n"].as_i64().unwrap_or(0);
    Ok(extism_pdk::Json(serde_json::json!({"result": n + 1})))
}
