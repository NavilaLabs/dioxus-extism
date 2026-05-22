#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureCapInvokeA;

impl DioxusPlugin for FixtureCapInvokeA {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/cap-invoke-a".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "slot-a".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![HostCapability::Invoke {
                names: vec!["get_notes".into()],
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureCapInvokeA {
    const SLOT_NAME: &'static str = "slot-a";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        let args =
            serde_json::to_string(&serde_json::json!({})).map_err(PdkError::Json)?;
        // get_notes is granted — should succeed.
        let _ = unsafe { host_fns::dx_invoke("get_notes", args.clone()) };
        // add_note is NOT granted — should be denied.
        let _ = unsafe { host_fns::dx_invoke("add_note", args) };
        Ok(PluginView::Text("a".into()))
    }
}

plugin! { type: FixtureCapInvokeA, slots: [FixtureCapInvokeA] }
