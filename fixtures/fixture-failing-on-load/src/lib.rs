use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{on_load_export, plugin};

struct FixtureFailingOnLoad;

impl DioxusPlugin for FixtureFailingOnLoad {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/failing-on-load".into()),
            version: "0.1.0".into(),
            ..Default::default()
        }
    }
}

impl OnLoad for FixtureFailingOnLoad {
    fn on_load(_ctx: &PluginCtx) -> Result<(), PdkError> {
        Err(PdkError::Custom("on_load failed intentionally".into()))
    }
}

plugin! { type: FixtureFailingOnLoad }
on_load_export!(FixtureFailingOnLoad);
