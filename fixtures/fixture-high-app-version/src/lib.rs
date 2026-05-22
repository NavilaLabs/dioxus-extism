#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureHighAppVersion;

impl DioxusPlugin for FixtureHighAppVersion {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/high-app-version".into()),
            version: "0.1.0".into(),
            min_app_version: 99,
            slots: vec![SlotRegistration {
                name: "test-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![
                HostCapability::GlobalStateRead { keys: vec!["call_count".into()] },
                HostCapability::GlobalStateWrite { keys: vec!["call_count".into()] },
            ],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureHighAppVersion {
    const SLOT_NAME: &'static str = "test-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        unsafe {
            let raw = host_fns::dx_global_state_get("call_count")
                .unwrap_or_else(|_| "0".into());
            let count: u64 = serde_json::from_str::<u64>(&raw).unwrap_or(0);
            let encoded =
                serde_json::to_string(&(count + 1)).map_err(PdkError::Json)?;
            host_fns::dx_global_state_set("call_count", encoded)
                .map_err(|e| PdkError::HostFn(e.to_string()))?;
        }
        Ok(PluginView::Text("high-app".into()))
    }
}

plugin! { type: FixtureHighAppVersion, slots: [FixtureHighAppVersion] }
