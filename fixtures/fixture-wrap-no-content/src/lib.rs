use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{plugin, transform_export};

struct FixtureWrapNoContent;

impl DioxusPlugin for FixtureWrapNoContent {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/wrap-no-content".into()),
            version: "0.1.0".into(),
            transforms: vec![TransformDeclaration {
                selector: Selector::Route(RoutePattern("/test/:id".into())),
                transform_fn: "transform_wrap_route".into(),
                op: TransformOp::Wrap,
                priority_hint: PriorityHint::High,
            }],
            ..Default::default()
        }
    }
}

impl TransformProvider for FixtureWrapNoContent {
    fn transform(_input: TransformInput, _ctx: &PluginCtx) -> Result<TransformOutput, PdkError> {
        // Intentionally omits original_content() to trigger the host warning.
        Ok(TransformOutput { view: PluginView::Text("no-content".into()) })
    }
}

plugin! { type: FixtureWrapNoContent }
transform_export!(FixtureWrapNoContent, transform_wrap_route);
