use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureSlotHigh;

impl DioxusPlugin for FixtureSlotHigh {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/slot-high".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "test-slot".into(),
                priority_hint: PriorityHint::High,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureSlotHigh {
    const SLOT_NAME: &'static str = "test-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        Ok(PluginView::Text("high".into()))
    }
}

plugin! { type: FixtureSlotHigh, slots: [FixtureSlotHigh] }
