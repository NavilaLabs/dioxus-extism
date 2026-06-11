use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{plugin, PROTOCOL_VERSION};

struct FixtureHighProtocolVersion;

impl DioxusPlugin for FixtureHighProtocolVersion {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/high-protocol-version".into()),
            version: "0.1.0".into(),
            min_protocol_version: PROTOCOL_VERSION + 1,
            ..Default::default()
        }
    }
}

plugin! { type: FixtureHighProtocolVersion }
