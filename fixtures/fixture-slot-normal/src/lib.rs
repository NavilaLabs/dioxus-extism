#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{on_unload_export, plugin};

struct FixtureSlotNormal;

impl DioxusPlugin for FixtureSlotNormal {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/slot-normal".into()),
            version: "0.1.0".into(),
            min_protocol_version: PROTOCOL_VERSION,
            slots: vec![SlotRegistration {
                name: "test-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![
                HostCapability::GlobalStateRead { keys: vec!["unload_count".into()] },
                HostCapability::GlobalStateWrite { keys: vec!["unload_count".into()] },
            ],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureSlotNormal {
    const SLOT_NAME: &'static str = "test-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        Ok(PluginView::Text("normal".into()))
    }
}

impl OnUnload for FixtureSlotNormal {
    fn on_unload() -> Result<(), PdkError> {
        unsafe {
            let raw = host_fns::dx_global_state_get("unload_count")
                .unwrap_or_else(|_| "0".into());
            let count: u64 = serde_json::from_str::<u64>(&raw).unwrap_or(0);
            let encoded =
                serde_json::to_string(&(count + 1)).map_err(PdkError::Json)?;
            host_fns::dx_global_state_set("unload_count", encoded)
                .map_err(|e| PdkError::HostFn(e.to_string()))?;
        }
        Ok(())
    }
}

plugin! { type: FixtureSlotNormal, slots: [FixtureSlotNormal] }
on_unload_export!(FixtureSlotNormal);
