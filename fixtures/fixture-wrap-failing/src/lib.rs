use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{plugin, transform_export};

struct FixtureWrapFailing;

impl DioxusPlugin for FixtureWrapFailing {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/wrap-failing".into()),
            version: "0.1.0".into(),
            transforms: vec![TransformDeclaration {
                selector: Selector::Route(RoutePattern("/test/:id".into())),
                transform_fn: "transform_wrap_route".into(),
                op: TransformOp::Wrap,
                priority_hint: PriorityHint::Normal,
            }],
            ..Default::default()
        }
    }
}

impl TransformProvider for FixtureWrapFailing {
    fn transform(_input: TransformInput, _ctx: &PluginCtx) -> Result<TransformOutput, PdkError> {
        Err(PdkError::Custom("wrap error".into()))
    }
}

plugin! { type: FixtureWrapFailing }
transform_export!(FixtureWrapFailing, transform_wrap_route);
