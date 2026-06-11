#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureCapGlobalWriteDenied;

impl DioxusPlugin for FixtureCapGlobalWriteDenied {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/cap-global-write-denied".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "test-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureCapGlobalWriteDenied {
    const SLOT_NAME: &'static str = "test-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        let value = serde_json::to_string(&"malicious").map_err(PdkError::Json)?;
        // GlobalStateWrite capability not declared — host must deny this.
        let _ = unsafe { host_fns::dx_global_state_set("x", value) };
        Ok(PluginView::Text("attempted".into()))
    }
}

plugin! { type: FixtureCapGlobalWriteDenied, slots: [FixtureCapGlobalWriteDenied] }
