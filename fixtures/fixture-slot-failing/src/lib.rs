use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureSlotFailing;

impl DioxusPlugin for FixtureSlotFailing {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/slot-failing".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "test-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureSlotFailing {
    const SLOT_NAME: &'static str = "test-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        Err(PdkError::Custom("slot error".into()))
    }
}

plugin! { type: FixtureSlotFailing, slots: [FixtureSlotFailing] }
