use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;

/// `plugin_a`: slot provider for "activity-feed".
///
/// Renders an activity feed card. The action area is marked with
/// `data-plugin-slot="feed-actions"` so `plugin_b` can inject into it
/// without any code changes here.
struct PluginA;

impl DioxusPlugin for PluginA {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("example/tree-selector-a".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "activity-feed".into(),
                priority_hint: PriorityHint::Normal,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for PluginA {
    const SLOT_NAME: &'static str = "activity-feed";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        Ok(div()
            .class("activity-card")
            .child(
                div()
                    .class("activity-header")
                    .child(text("Latest activity"))
                    .build(),
            )
            .child(
                div()
                    .class("activity-body")
                    .child(text("User alice posted a comment."))
                    .build(),
            )
            .child(
                div()
                    .attr("data-plugin-slot", "feed-actions")
                    .child(text("👍 Like"))
                    .build(),
            )
            .build())
    }
}

plugin! { type: PluginA, slots: [PluginA] }
