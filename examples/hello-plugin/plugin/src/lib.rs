use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct HelloPlugin;

impl DioxusPlugin for HelloPlugin {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("example/hello".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "hello-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for HelloPlugin {
    const SLOT_NAME: &'static str = "hello-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        Ok(div()
            .class("hello-from-plugin")
            .child(text("Hello from a WASM plugin!"))
            .build())
    }
}

plugin! { type: HelloPlugin, slots: [HelloPlugin] }
