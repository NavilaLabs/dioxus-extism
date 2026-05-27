/// §7 — RuntimeMetrics observability tests.
///
/// Requires compiled fixtures:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-slot-normal -p fixture-slot-failing -p fixture-echo-fn
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dioxus_extism_host::{PluginRuntimeBuilder, PluginSource, RuntimeMetrics};
use dioxus_extism_protocol::{ClientCapabilities, PluginId, SessionCtx, SessionId, PROTOCOL_VERSION};

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(SLOT_NORMAL_WASM, "fixture_slot_normal.wasm");
fixture!(SLOT_HIGH_WASM, "fixture_slot_high.wasm");
fixture!(SLOT_FAILING_WASM, "fixture_slot_failing.wasm");
fixture!(ECHO_FN_WASM, "fixture_echo_fn.wasm");

fn default_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("metrics-test".into()),
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

// ── Capturing metrics impl ────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct CapturingMetrics {
    calls: Arc<Mutex<Vec<(PluginId, String, Duration, bool)>>>,
    utilizations: Arc<Mutex<Vec<(PluginId, usize, usize)>>>,
}

impl RuntimeMetrics for CapturingMetrics {
    fn record_call(
        &self,
        plugin_id: &PluginId,
        function_name: &str,
        elapsed: Duration,
        success: bool,
    ) {
        self.calls
            .lock()
            .unwrap()
            .push((plugin_id.clone(), function_name.to_owned(), elapsed, success));
    }

    fn record_pool_utilization(&self, plugin_id: &PluginId, active: usize, total: usize) {
        self.utilizations
            .lock()
            .unwrap()
            .push((plugin_id.clone(), active, total));
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn record_call_invoked_after_slot_render() {
    let m = CapturingMetrics::default();
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .with_metrics(m.clone())
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    runtime.render_slot("test-slot", &session).await.expect("render failed");

    let calls = m.calls.lock().unwrap();
    assert!(!calls.is_empty(), "record_call should be invoked after slot render");
    let success_calls: Vec<_> = calls.iter().filter(|(_, _, _, ok)| *ok).collect();
    assert!(!success_calls.is_empty(), "at least one successful call should be recorded");
    let all_positive_elapsed = calls.iter().all(|(_, _, d, _)| *d >= Duration::ZERO);
    assert!(all_positive_elapsed, "elapsed duration must be non-negative");
}

#[tokio::test]
async fn record_call_success_false_on_slot_failure() {
    let m = CapturingMetrics::default();
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_FAILING_WASM))
        .with_metrics(m.clone())
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    // render_slot returns Ok but individual slot content may be Incompatible.
    let _ = runtime.render_slot("test-slot", &session).await;

    let calls = m.calls.lock().unwrap();
    assert!(!calls.is_empty(), "record_call should be invoked even on failure");
    let failure_calls: Vec<_> = calls.iter().filter(|(_, _, _, ok)| !*ok).collect();
    assert!(
        !failure_calls.is_empty(),
        "at least one call should be recorded as failed for the failing fixture"
    );
}

#[tokio::test]
async fn record_pool_utilization_invoked() {
    let m = CapturingMetrics::default();
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .with_metrics(m.clone())
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    runtime.render_slot("test-slot", &session).await.expect("render failed");

    let util = m.utilizations.lock().unwrap();
    assert!(!util.is_empty(), "record_pool_utilization should be invoked during slot render");
    assert!(util.iter().all(|(_, active, _)| *active >= 1), "active must be >= 1 when in use");
}

#[tokio::test]
async fn active_never_exceeds_pool_size() {
    let m = CapturingMetrics::default();
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .with_metrics(m.clone())
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    runtime.render_slot("test-slot", &session).await.expect("render failed");

    let util = m.utilizations.lock().unwrap();
    for (_, active, total) in util.iter() {
        assert!(
            active <= total,
            "active ({active}) must never exceed total ({total})"
        );
    }
}

#[tokio::test]
async fn no_panic_without_metrics_provider() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    let result = runtime.render_slot("test-slot", &session).await;
    assert!(result.is_ok(), "render_slot should succeed without metrics, got: {result:?}");
}

#[tokio::test]
async fn record_call_for_call_plugin() {
    let m = CapturingMetrics::default();
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(ECHO_FN_WASM))
        .with_metrics(m.clone())
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    let id = PluginId("test/echo-fn".into());
    let _: serde_json::Value = runtime
        .call_plugin(&id, "echo_fn", &serde_json::json!({"k": "v"}), &session)
        .await
        .expect("call_plugin failed");

    let calls = m.calls.lock().unwrap();
    let echo_calls: Vec<_> = calls.iter().filter(|(_, name, _, _)| name == "echo_fn").collect();
    assert!(!echo_calls.is_empty(), "record_call should be invoked for call_plugin");
    assert!(echo_calls[0].3, "call_plugin echo_fn should succeed");
}

#[tokio::test]
async fn elapsed_duration_is_plausible() {
    let m = CapturingMetrics::default();
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .with_metrics(m.clone())
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    runtime.render_slot("test-slot", &session).await.expect("render failed");

    let calls = m.calls.lock().unwrap();
    for (_, _, elapsed, _) in calls.iter() {
        assert!(
            *elapsed < Duration::from_secs(10),
            "elapsed duration should be < 10s, got: {elapsed:?}"
        );
    }
}

#[tokio::test]
async fn metrics_called_for_each_plugin_in_multi_plugin_slot() {
    let m = CapturingMetrics::default();
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(src(SLOT_NORMAL_WASM))
        .add_plugin(src(SLOT_HIGH_WASM))
        .with_metrics(m.clone())
        .build()
        .await
        .expect("build failed");

    let session = default_session();
    runtime.render_slot("test-slot", &session).await.expect("render failed");

    let calls = m.calls.lock().unwrap();
    let plugin_ids: std::collections::HashSet<_> =
        calls.iter().map(|(id, _, _, _)| id.clone()).collect();
    assert!(
        plugin_ids.len() >= 2,
        "metrics should be recorded for each plugin in the slot, got ids: {plugin_ids:?}"
    );
}
