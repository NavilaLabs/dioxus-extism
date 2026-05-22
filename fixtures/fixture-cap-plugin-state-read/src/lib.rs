#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureCapPluginStateRead;

impl DioxusPlugin for FixtureCapPluginStateRead {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/cap-plugin-state-read".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "test-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            // No ReadPluginState capability declared; GlobalStateWrite for test observability.
            host_capabilities: vec![
                HostCapability::GlobalStateWrite { keys: vec!["read_attempt_result".into()] },
            ],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureCapPluginStateRead {
    const SLOT_NAME: &'static str = "test-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        // Attempt to read another plugin's state — should be denied by the host.
        let result_raw = unsafe {
            host_fns::dx_plugin_state_get("test/cap-state-owner", "data")
                .unwrap_or_else(|_| "null".into())
        };
        // Write the raw result to global state so the test can observe it.
        let _ = unsafe {
            host_fns::dx_global_state_set("read_attempt_result", result_raw)
        };
        Ok(PluginView::Text("attempted".into()))
    }
}

plugin! { type: FixtureCapPluginStateRead, slots: [FixtureCapPluginStateRead] }
