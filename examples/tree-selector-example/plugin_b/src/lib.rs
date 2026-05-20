#![allow(clippy::unnecessary_wraps)]

use dioxus_extism_pdk::prelude::*;
use extism_pdk::{FnResult, Json, plugin_fn};

/// `plugin_b`: Within transform that inserts a "Share" button after the
/// `data-plugin-slot="feed-actions"` node inside the "activity-feed" slot.
///
/// `plugin_a` has zero knowledge of `plugin_b`.
struct PluginB;

impl DioxusPlugin for PluginB {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("example/tree-selector-b".into()),
            version: "0.1.0".into(),
            transforms: vec![TransformDeclaration {
                selector: Selector::Within {
                    outer: Box::new(Selector::Slot("activity-feed".into())),
                    inner: NodeSelector::DataAttr(
                        "data-plugin-slot".into(),
                        "feed-actions".into(),
                    ),
                },
                transform_fn: "inject_share_button".into(),
                op: TransformOp::InsertAfter,
                priority_hint: PriorityHint::Normal,
            }],
            ..Default::default()
        }
    }
}

#[plugin_fn]
pub fn manifest() -> FnResult<Json<PluginManifest>> {
    Ok(Json(PluginB::manifest()))
}

/// Injects a "Share" button after the feed-actions node.
/// The `InsertAfter` op means original is `None` — we just return what to insert.
#[plugin_fn]
pub fn inject_share_button(_input: Json<TransformInput>) -> FnResult<Json<TransformOutput>> {
    let view = div()
        .class("plugin-b-share-action")
        .child(text("🔗 Share — injected by plugin_b"))
        .build();
    Ok(Json(TransformOutput { view }))
}
