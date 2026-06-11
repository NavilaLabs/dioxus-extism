#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureCapInvokeDenied;

impl DioxusPlugin for FixtureCapInvokeDenied {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/cap-invoke-denied".into()),
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

impl SlotProvider for FixtureCapInvokeDenied {
    const SLOT_NAME: &'static str = "test-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        let args = serde_json::to_string(&serde_json::json!({}))
            .map_err(PdkError::Json)?;
        // Capability not declared — host must deny this call silently.
        let _ = unsafe { host_fns::dx_invoke("add_note", args) };
        Ok(PluginView::Text("attempted".into()))
    }
}

plugin! { type: FixtureCapInvokeDenied, slots: [FixtureCapInvokeDenied] }
