/// §4 — RouteReplace transform tests.
///
/// Requires compiled fixtures:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-route-replace -p fixture-wrap-a
use std::sync::Arc;

use dioxus_extism_host::{PluginRuntimeBuilder, PluginSource, RouteTransforms};
use dioxus_extism_protocol::{
    ClientCapabilities, PluginView, SessionCtx, SessionId, PROTOCOL_VERSION,
};

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(ROUTE_REPLACE_WASM, "fixture_route_replace.wasm");

fn default_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("route-test".into()),
        user_id: None,
        client: ClientCapabilities {
            protocol_version: PROTOCOL_VERSION,
            app_version: 0,
            registered_host_components: vec![],
        },
        caller: None,
    }
}

fn src(bytes: &'static [u8]) -> PluginSource {
    PluginSource::Bytes(std::borrow::Cow::Borrowed(bytes))
}

fn view_contains_text(view: &PluginView, needle: &str) -> bool {
    match view {
        PluginView::Text(t) => t.contains(needle),
        PluginView::Element(e) => e.children.iter().any(|c| view_contains_text(c, needle)),
        PluginView::Fragment(children) => children.iter().any(|c| view_contains_text(c, needle)),
        _ => false,
    }
}

#[tokio::test]
async fn route_replace_produces_replacement() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ROUTE_REPLACE_WASM))
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    let result: RouteTransforms = runtime
        .render_route_transforms("/replace/42", &session)
        .await
        .expect("render_route_transforms failed");

    assert!(result.replacement.is_some(), "expected a replacement view");
    let replacement = result.replacement.unwrap();
    assert!(
        view_contains_text(&replacement, "replaced-42"),
        "replacement should contain 'replaced-42', got: {replacement:?}"
    );
}

#[tokio::test]
async fn non_matching_path_no_replacement() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ROUTE_REPLACE_WASM))
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    let result: RouteTransforms = runtime
        .render_route_transforms("/other/path", &session)
        .await
        .expect("render_route_transforms failed");

    assert!(result.replacement.is_none(), "non-matching path should produce no replacement");
}

#[tokio::test]
async fn policy_deny_blocks_replacement() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ROUTE_REPLACE_WASM))
        .with_route_replace_policy(Arc::new(|_, _pattern: &str| false))
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    let result: RouteTransforms = runtime
        .render_route_transforms("/replace/99", &session)
        .await
        .expect("render_route_transforms failed");

    assert!(result.replacement.is_none(), "policy returning false should block replacement");
}

#[tokio::test]
async fn no_policy_allows_by_default() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ROUTE_REPLACE_WASM))
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    let result: RouteTransforms = runtime
        .render_route_transforms("/replace/1", &session)
        .await
        .expect("render_route_transforms failed");

    assert!(result.replacement.is_some(), "no policy means allow by default");
}

#[tokio::test]
async fn policy_registered_at_runtime_takes_effect() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ROUTE_REPLACE_WASM))
        .build()
        .await
        .expect("build failed");

    // Before registering a policy — replacement is allowed.
    let session = default_session();
    let before: RouteTransforms = runtime
        .render_route_transforms("/replace/10", &session)
        .await
        .expect("render_route_transforms failed");
    assert!(before.replacement.is_some(), "should be allowed before policy");

    // Register a deny-all policy at runtime.
    runtime.register_route_replace_policy(Arc::new(|_, _| false)).await;

    let after: RouteTransforms = runtime
        .render_route_transforms("/replace/10", &session)
        .await
        .expect("render_route_transforms failed");
    assert!(after.replacement.is_none(), "policy registered at runtime should deny");
}
