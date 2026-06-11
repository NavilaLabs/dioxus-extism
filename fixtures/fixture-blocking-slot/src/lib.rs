use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureBlockingSlot;

impl DioxusPlugin for FixtureBlockingSlot {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/blocking-slot".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "test-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureBlockingSlot {
    const SLOT_NAME: &'static str = "test-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        // Blocks for 5 minutes — the host timeout must cancel this before it completes.
        std::thread::sleep(std::time::Duration::from_mins(5));
        Ok(PluginView::Text("should_not_return".into()))
    }
}

plugin! { type: FixtureBlockingSlot, slots: [FixtureBlockingSlot] }
