use std::collections::BTreeMap;

use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

struct FixtureWithExtension;

impl DioxusPlugin for FixtureWithExtension {
    fn manifest() -> PluginManifest {
        let mut extensions = BTreeMap::new();
        extensions.insert("test.my-feature".into(), serde_json::json!({"level": 2}));
        extensions.insert("test.another-ns".into(), serde_json::json!("hello"));

        PluginManifest {
            id: PluginId("test/with-extension".into()),
            version: "0.1.0".into(),
            min_protocol_version: PROTOCOL_VERSION,
            slots: vec![SlotRegistration {
                name: "ext-slot".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![HostCapability::Custom {
                namespace: "test.cap-a".into(),
                value: serde_json::json!({"tier": 1}),
            }],
            extensions,
            ..Default::default()
        }
    }
}

impl SlotProvider for FixtureWithExtension {
    const SLOT_NAME: &'static str = "ext-slot";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        Ok(PluginView::Text("ext-content".into()))
    }
}

plugin! { type: FixtureWithExtension, slots: [FixtureWithExtension] }
