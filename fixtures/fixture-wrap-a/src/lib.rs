#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{plugin, transform_export};

struct FixtureWrapA;

impl DioxusPlugin for FixtureWrapA {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/wrap-a".into()),
            version: "0.1.0".into(),
            transforms: vec![TransformDeclaration {
                selector: Selector::Route(RoutePattern("/test/:id".into())),
                transform_fn: "transform_wrap_route".into(),
                op: TransformOp::Wrap,
                priority_hint: PriorityHint::High,
            }],
            host_capabilities: vec![
                HostCapability::GlobalStateWrite { keys: vec!["received_original".into()] },
            ],
            ..Default::default()
        }
    }
}

impl TransformProvider for FixtureWrapA {
    fn transform(input: TransformInput, _ctx: &PluginCtx) -> Result<TransformOutput, PdkError> {
        if let Some(ref original) = input.original {
            let encoded = serde_json::to_string(original).map_err(PdkError::Json)?;
            unsafe {
                host_fns::dx_global_state_set("received_original", encoded)
                    .map_err(|e| PdkError::HostFn(e.to_string()))?;
            }
        }
        Ok(TransformOutput {
            view: div()
                .child(text("marker-a"))
                .child(original_content())
                .build(),
        })
    }
}

plugin! { type: FixtureWrapA }
transform_export!(FixtureWrapA, transform_wrap_route);
