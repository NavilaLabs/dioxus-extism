#![allow(clippy::unnecessary_wraps)]

use dioxus_extism_pdk::prelude::*;
use extism_pdk::{FnResult, Json, plugin_fn};

#[plugin_fn]
pub fn manifest() -> FnResult<Json<PluginManifest>> {
    Ok(Json(PluginManifest {
        id: PluginId("example/route-injection".into()),
        version: "0.1.0".into(),
        transforms: vec![
            TransformDeclaration {
                selector: Selector::Route(RoutePattern("/product/:id".into())),
                transform_fn: "wrap_product_page".into(),
                op: TransformOp::Wrap,
                priority_hint: PriorityHint::Normal,
            },
            TransformDeclaration {
                selector: Selector::Route(RoutePattern("/product/:id".into())),
                transform_fn: "inject_product_banner".into(),
                op: TransformOp::InjectAfter,
                priority_hint: PriorityHint::Normal,
            },
        ],
        ..Default::default()
    }))
}

/// Wraps the product page with a plugin-provided header and footer.
/// The `original_content()` placeholder is resolved to `Outlet<R>` by the frontend.
#[plugin_fn]
pub fn wrap_product_page(input: Json<TransformInput>) -> FnResult<Json<TransformOutput>> {
    let product_id = input
        .0
        .context
        .route_params
        .get("id")
        .cloned()
        .unwrap_or_else(|| "unknown".into());

    let view = div()
        .class("plugin-product-wrapper")
        .child(
            div()
                .class("plugin-product-header")
                .child(text(format!("✨ Enhanced by plugin — product {product_id}")))
                .build(),
        )
        .child(original_content())
        .child(
            div()
                .class("plugin-product-footer")
                .child(text("Plugin: see also our bestsellers"))
                .build(),
        )
        .build();

    Ok(Json(TransformOutput { view }))
}

/// Injects a "related products" widget after the product page.
#[plugin_fn]
pub fn inject_product_banner(_input: Json<TransformInput>) -> FnResult<Json<TransformOutput>> {
    let view = div()
        .class("plugin-injected-banner")
        .child(text("Related products — injected by plugin below the page"))
        .build();

    Ok(Json(TransformOutput { view }))
}
