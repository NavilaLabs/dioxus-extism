use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{plugin, transform_export};

struct FixtureRouteReplace;

impl DioxusPlugin for FixtureRouteReplace {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/route-replace".into()),
            version: "0.1.0".into(),
            min_protocol_version: PROTOCOL_VERSION,
            transforms: vec![TransformDeclaration {
                selector: Selector::Route(RoutePattern("/replace/:id".into())),
                transform_fn: "transform_replace_route".into(),
                op: TransformOp::RouteReplace,
                priority_hint: PriorityHint::High,
            }],
            ..Default::default()
        }
    }
}

impl TransformProvider for FixtureRouteReplace {
    fn transform(input: TransformInput, _ctx: &PluginCtx) -> Result<TransformOutput, PdkError> {
        let id = input.context.route_params.get("id").cloned().unwrap_or_default();
        Ok(TransformOutput {
            view: div().child(text(&format!("replaced-{id}"))).build(),
        })
    }
}

plugin! { type: FixtureRouteReplace }
transform_export!(FixtureRouteReplace, transform_replace_route);
