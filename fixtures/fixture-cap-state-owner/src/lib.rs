#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{on_load_export, plugin};

struct FixtureCapStateOwner;

impl DioxusPlugin for FixtureCapStateOwner {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/cap-state-owner".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "other-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![
                HostCapability::GlobalStateWrite { keys: vec!["data".into()] },
            ],
            ..Default::default()
        }
    }
}

impl OnLoad for FixtureCapStateOwner {
    fn on_load(_ctx: &PluginCtx) -> Result<(), PdkError> {
        let encoded = serde_json::to_string(&"secret").map_err(PdkError::Json)?;
        // Write to global state so dx_plugin_state_get can read it.
        unsafe { host_fns::dx_global_state_set("data", encoded) }
            .map_err(|e| PdkError::HostFn(e.to_string()))
    }
}

impl SlotProvider for FixtureCapStateOwner {
    const SLOT_NAME: &'static str = "other-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        Ok(PluginView::Text("owner".into()))
    }
}

plugin! { type: FixtureCapStateOwner, slots: [FixtureCapStateOwner] }
on_load_export!(FixtureCapStateOwner);
