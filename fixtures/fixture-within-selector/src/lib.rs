#![allow(clippy::unnecessary_wraps)]

use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;
use extism_pdk::{FnResult, Json, plugin_fn};

struct FixtureWithinSelector;

impl DioxusPlugin for FixtureWithinSelector {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/within-selector".into()),
            version: "0.1.0".into(),
            transforms: vec![
                TransformDeclaration {
                    selector: Selector::Within {
                        outer: Box::new(Selector::Slot("test-slot".into())),
                        inner: NodeSelector::Recursive(Box::new(
                            NodeSelector::HasClass("target".into()),
                        )),
                    },
                    transform_fn: "transform_recursive".into(),
                    op: TransformOp::Replace,
                    priority_hint: PriorityHint::Normal,
                },
                TransformDeclaration {
                    selector: Selector::Within {
                        outer: Box::new(Selector::Slot("test-slot".into())),
                        inner: NodeSelector::HasClass("shallow-target".into()),
                    },
                    transform_fn: "transform_shallow".into(),
                    op: TransformOp::Replace,
                    priority_hint: PriorityHint::Normal,
                },
                TransformDeclaration {
                    selector: Selector::Within {
                        outer: Box::new(Selector::Slot("test-slot".into())),
                        inner: NodeSelector::And(
                            Box::new(NodeSelector::HasClass("a".into())),
                            Box::new(NodeSelector::HasClass("b".into())),
                        ),
                    },
                    transform_fn: "transform_and".into(),
                    op: TransformOp::Replace,
                    priority_hint: PriorityHint::Normal,
                },
                TransformDeclaration {
                    selector: Selector::Within {
                        outer: Box::new(Selector::Slot("test-slot".into())),
                        inner: NodeSelector::Or(
                            Box::new(NodeSelector::HasClass("c".into())),
                            Box::new(NodeSelector::HasClass("d".into())),
                        ),
                    },
                    transform_fn: "transform_or".into(),
                    op: TransformOp::Replace,
                    priority_hint: PriorityHint::Normal,
                },
            ],
            ..Default::default()
        }
    }
}

plugin! { type: FixtureWithinSelector }

#[plugin_fn]
pub fn transform_recursive(_input: Json<TransformInput>) -> FnResult<Json<TransformOutput>> {
    Ok(Json(TransformOutput { view: PluginView::Text("TRANSFORMED-RECURSIVE".into()) }))
}

#[plugin_fn]
pub fn transform_shallow(_input: Json<TransformInput>) -> FnResult<Json<TransformOutput>> {
    Ok(Json(TransformOutput { view: PluginView::Text("TRANSFORMED-SHALLOW".into()) }))
}

#[plugin_fn]
pub fn transform_and(_input: Json<TransformInput>) -> FnResult<Json<TransformOutput>> {
    Ok(Json(TransformOutput { view: PluginView::Text("TRANSFORMED-AND".into()) }))
}

#[plugin_fn]
pub fn transform_or(_input: Json<TransformInput>) -> FnResult<Json<TransformOutput>> {
    Ok(Json(TransformOutput { view: PluginView::Text("TRANSFORMED-OR".into()) }))
}
