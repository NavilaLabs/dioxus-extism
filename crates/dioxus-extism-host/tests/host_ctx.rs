/// §8 — Non-trivial `HostCtx` integration tests.
///
/// Each test uses a two-field `HostCtx` struct so that closures can
/// demonstrate decisions that depend on BOTH fields — verifying that the
/// context is fully forwarded and readable, not just present in the type.
///
/// Policy/check results are asserted to VARY between calls on the SAME
/// runtime but with DIFFERENT `host_ctx` values.  That is the key
/// property this RFC adds: the library routes the caller's context to
/// every decision callback without inspecting or modifying it.
///
/// Requires compiled fixtures:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-route-replace -p fixture-with-extension
use dioxus_extism_host::{OnUnknownExtension, PluginRuntimeBuilder, PluginRuntimeError, PluginSource};
use dioxus_extism_protocol::{ClientCapabilities, PluginId, SessionCtx, SessionId, PROTOCOL_VERSION};

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(ROUTE_REPLACE_WASM, "fixture_route_replace.wasm");
fixture!(WITH_EXTENSION_WASM, "fixture_with_extension.wasm");

// ── Two-field host context ────────────────────────────────────────────────────

/// Opaque host context with two fields.
///
/// `user_tier` gates access by numeric level; `region` enables region-based
/// overrides.  Policies assert on both to prove each field is reachable.
#[derive(Clone)]
struct HostCtx {
    user_tier: u32,
    region: String,
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn default_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("host-ctx-test".into()),
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

// ── Route-replace policy tests ────────────────────────────────────────────────

/// A policy that reads `user_tier` produces different outcomes when called
/// on the same runtime with contexts that carry different tier values.
#[tokio::test]
async fn policy_ctx_result_varies_with_host_ctx() {
    let runtime = PluginRuntimeBuilder::<HostCtx>::new()
        .add_plugin(src(ROUTE_REPLACE_WASM))
        .with_route_replace_policy_ctx(|_, _, ctx| ctx.host.user_tier >= 2)
        .build()
        .await
        .expect("build failed");

    let session = default_session();

    let low_ctx = HostCtx { user_tier: 1, region: "us-east".into() };
    let blocked = runtime
        .render_route_transforms("/replace/42", &session, &low_ctx)
        .await
        .expect("render failed");
    assert!(
        blocked.replacement.is_none(),
        "policy should block replacement for user_tier < 2, got: {:?}", blocked.replacement
    );

    let high_ctx = HostCtx { user_tier: 3, region: "us-east".into() };
    let allowed = runtime
        .render_route_transforms("/replace/42", &session, &high_ctx)
        .await
        .expect("render failed");
    assert!(
        allowed.replacement.is_some(),
        "policy should allow replacement for user_tier >= 2, got: {:?}", allowed.replacement
    );
}

/// A policy that reads BOTH fields (`user_tier` AND `region`) allows access
/// when either condition is satisfied, demonstrating full context forwarding.
#[tokio::test]
async fn policy_ctx_reads_both_fields() {
    // Allow if user_tier >= 2 OR region == "vip".
    let runtime = PluginRuntimeBuilder::<HostCtx>::new()
        .add_plugin(src(ROUTE_REPLACE_WASM))
        .with_route_replace_policy_ctx(|_, _, ctx| {
            ctx.host.user_tier >= 2 || ctx.host.region == "vip"
        })
        .build()
        .await
        .expect("build failed");

    let session = default_session();

    // Neither condition → blocked.
    let neither = HostCtx { user_tier: 1, region: "us-east".into() };
    let blocked = runtime
        .render_route_transforms("/replace/42", &session, &neither)
        .await
        .expect("render failed");
    assert!(
        blocked.replacement.is_none(),
        "low tier + non-vip should be blocked, got: {:?}", blocked.replacement
    );

    // Only region condition → allowed.
    let vip = HostCtx { user_tier: 1, region: "vip".into() };
    let by_region = runtime
        .render_route_transforms("/replace/42", &session, &vip)
        .await
        .expect("render failed");
    assert!(
        by_region.replacement.is_some(),
        "vip region should override low tier, got: {:?}", by_region.replacement
    );

    // Only tier condition → allowed.
    let tier = HostCtx { user_tier: 5, region: "us-east".into() };
    let by_tier = runtime
        .render_route_transforms("/replace/42", &session, &tier)
        .await
        .expect("render failed");
    assert!(
        by_tier.replacement.is_some(),
        "high tier should override non-vip, got: {:?}", by_tier.replacement
    );
}

// ── Capability-check tests ────────────────────────────────────────────────────

/// `check_custom_capability` returns `Ok`/`Err` based on the `user_tier` in
/// `host_ctx`, not a static compile-time decision.  The same `PluginRuntime`
/// instance produces different results for different contexts.
///
/// The fixture declares `HostCapability::Custom { "test.cap-a", {"tier": 1} }`.
/// The check enforces `host.user_tier >= manifest_required_tier`.
#[tokio::test]
async fn capability_check_ctx_result_varies_with_host_ctx() {
    let runtime = PluginRuntimeBuilder::<HostCtx>::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check_ctx(
            "test.cap-a",
            |_, value, ctx| {
                let required = value["tier"].as_u64().unwrap_or(0) as u32;
                if ctx.host.user_tier >= required {
                    Ok(())
                } else {
                    Err(format!(
                        "user_tier {} < required {}",
                        ctx.host.user_tier, required
                    ))
                }
            },
        )
        .with_on_unknown_extension(OnUnknownExtension::Ignore)
        .build()
        .await
        .expect("build failed");

    let plugin_id = PluginId("test/with-extension".into());
    let session = default_session();

    // user_tier 0 < manifest required (1) → denied.
    let low_ctx = HostCtx { user_tier: 0, region: "us-east".into() };
    let denied = runtime
        .check_custom_capability(&plugin_id, "test.cap-a", &session, &low_ctx)
        .await;
    assert!(
        matches!(denied, Err(PluginRuntimeError::CapabilityDenied { .. })),
        "expected CapabilityDenied for low tier, got: {denied:?}"
    );

    // user_tier 2 >= manifest required (1) → allowed.
    let high_ctx = HostCtx { user_tier: 2, region: "us-east".into() };
    let allowed = runtime
        .check_custom_capability(&plugin_id, "test.cap-a", &session, &high_ctx)
        .await;
    assert!(
        allowed.is_ok(),
        "expected Ok for sufficient tier, got: {allowed:?}"
    );
}

/// A check that reads BOTH `user_tier` AND `region` from `host_ctx` can
/// admit an "admin" region even when the numeric tier is insufficient,
/// proving both fields are accessible inside the callback.
#[tokio::test]
async fn capability_check_ctx_reads_both_fields() {
    // Allow if user_tier >= required OR region == "admin".
    let runtime = PluginRuntimeBuilder::<HostCtx>::new()
        .add_plugin(src(WITH_EXTENSION_WASM))
        .with_capability_check_ctx("test.cap-a", |_, value, ctx| {
            let required = value["tier"].as_u64().unwrap_or(0) as u32;
            if ctx.host.user_tier >= required || ctx.host.region == "admin" {
                Ok(())
            } else {
                Err(format!(
                    "denied: tier={} region={}",
                    ctx.host.user_tier, ctx.host.region
                ))
            }
        })
        .with_on_unknown_extension(OnUnknownExtension::Ignore)
        .build()
        .await
        .expect("build failed");

    let plugin_id = PluginId("test/with-extension".into());
    let session = default_session();

    // Low tier + non-admin: both conditions fail → denied.
    let plain = runtime
        .check_custom_capability(
            &plugin_id,
            "test.cap-a",
            &session,
            &HostCtx { user_tier: 0, region: "us-east".into() },
        )
        .await;
    assert!(
        matches!(plain, Err(PluginRuntimeError::CapabilityDenied { .. })),
        "both conditions fail → expected CapabilityDenied, got: {plain:?}"
    );

    // Low tier + admin region: second condition passes → allowed.
    let admin = runtime
        .check_custom_capability(
            &plugin_id,
            "test.cap-a",
            &session,
            &HostCtx { user_tier: 0, region: "admin".into() },
        )
        .await;
    assert!(
        admin.is_ok(),
        "admin region overrides low tier → expected Ok, got: {admin:?}"
    );

    // High tier + non-admin: first condition passes → allowed.
    let high = runtime
        .check_custom_capability(
            &plugin_id,
            "test.cap-a",
            &session,
            &HostCtx { user_tier: 5, region: "us-east".into() },
        )
        .await;
    assert!(
        high.is_ok(),
        "high tier overrides non-admin region → expected Ok, got: {high:?}"
    );
}
