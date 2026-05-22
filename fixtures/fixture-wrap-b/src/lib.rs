use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{plugin, transform_export};

struct FixtureWrapB;

impl DioxusPlugin for FixtureWrapB {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/wrap-b".into()),
            version: "0.1.0".into(),
            transforms: vec![TransformDeclaration {
                selector: Selector::Route(RoutePattern("/test/:id".into())),
                transform_fn: "transform_wrap_route".into(),
                op: TransformOp::Wrap,
                priority_hint: PriorityHint::Low,
            }],
            ..Default::default()
        }
    }
}

impl TransformProvider for FixtureWrapB {
    fn transform(input: TransformInput, _ctx: &PluginCtx) -> Result<TransformOutput, PdkError> {
        let inner = input.original.unwrap_or_else(original_content);
        Ok(TransformOutput {
            view: div()
                .child(inner)
                .child(text("marker-b"))
                .build(),
        })
    }
}

plugin! { type: FixtureWrapB }
transform_export!(FixtureWrapB, transform_wrap_route);
