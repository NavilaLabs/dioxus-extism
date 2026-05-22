#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureCapInvokeB;

impl DioxusPlugin for FixtureCapInvokeB {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/cap-invoke-b".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "slot-b".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![HostCapability::Invoke {
                names: vec!["add_note".into()],
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureCapInvokeB {
    const SLOT_NAME: &'static str = "slot-b";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        let args =
            serde_json::to_string(&serde_json::json!({})).map_err(PdkError::Json)?;
        // add_note is granted — should succeed.
        let _ = unsafe { host_fns::dx_invoke("add_note", args.clone()) };
        // get_notes is NOT granted — should be denied.
        let _ = unsafe { host_fns::dx_invoke("get_notes", args) };
        Ok(PluginView::Text("b".into()))
    }
}

plugin! { type: FixtureCapInvokeB, slots: [FixtureCapInvokeB] }
