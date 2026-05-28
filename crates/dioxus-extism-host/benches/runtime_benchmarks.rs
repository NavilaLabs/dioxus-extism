/// Criterion benchmarks for `dioxus-extism-host` hot paths.
///
/// Fixtures must be compiled before running:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-slot-normal -p fixture-slot-high \
///     -p fixture-hook-continue -p fixture-hook-replace \
///     -p fixture-echo-fn
use std::sync::Arc;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use dioxus_extism_host::{PluginRuntimeBuilder, PluginSource, RuntimeMetrics};
use dioxus_extism_protocol::{
    ClientCapabilities, PluginId, SessionCtx, SessionId, PROTOCOL_VERSION,
};
use tokio::runtime::Runtime;

// ── WASM fixture bytes ─────────────────────────────────────────────────────────

macro_rules! fixture_bytes {
    ($path:literal) => {
        include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ))
    };
}

const SLOT_NORMAL: &[u8] = fixture_bytes!("fixture_slot_normal.wasm");
const SLOT_HIGH: &[u8] = fixture_bytes!("fixture_slot_high.wasm");
const HOOK_CONTINUE: &[u8] = fixture_bytes!("fixture_hook_continue.wasm");
const HOOK_REPLACE: &[u8] = fixture_bytes!("fixture_hook_replace.wasm");
const ECHO_FN: &[u8] = fixture_bytes!("fixture_echo_fn.wasm");

// ── Helpers ────────────────────────────────────────────────────────────────────

fn default_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("bench-session".into()),
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

/// Zero-cost `RuntimeMetrics` impl — measures vtable dispatch overhead only.
struct NoopMetrics;

impl RuntimeMetrics for NoopMetrics {
    fn record_call(&self, _: &PluginId, _: &str, _: Duration, _: bool) {}
    fn record_pool_utilization(&self, _: &PluginId, _: usize, _: usize) {}
}

// ── Group 1: runtime build time ───────────────────────────────────────────────

fn bench_runtime_build(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");
    let mut group = c.benchmark_group("runtime_build");

    let cases: &[(&str, &[&[u8]])] = &[
        ("0_plugins", &[]),
        ("1_plugin", &[SLOT_NORMAL]),
        ("2_plugins", &[SLOT_NORMAL, SLOT_HIGH]),
    ];

    for (label, sources) in cases {
        let sources: Vec<&'static [u8]> = sources.to_vec();
        group.bench_function(*label, |b| {
            b.to_async(&rt).iter(|| async {
                let mut builder = PluginRuntimeBuilder::new();
                for s in &sources {
                    builder = builder.add_plugin(src(s));
                }
                builder.build().await.expect("build failed")
            });
        });
    }

    group.finish();
}

// ── Group 2: slot render throughput ──────────────────────────────────────────

fn bench_slot_render(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");

    let runtime_1 = rt.block_on(
        PluginRuntimeBuilder::new()
            .add_plugin(src(SLOT_NORMAL))
            .build(),
    )
    .expect("build 1_plugin runtime");

    let runtime_2 = rt.block_on(
        PluginRuntimeBuilder::new()
            .add_plugin(src(SLOT_NORMAL))
            .add_plugin(PluginSource::Bytes(std::borrow::Cow::Borrowed(SLOT_HIGH)))
            .build(),
    )
    .expect("build 2_plugin runtime");

    let session = default_session();
    let mut group = c.benchmark_group("slot_render");

    group.bench_function("1_plugin", |b| {
        let rt1 = Arc::clone(&runtime_1);
        let s = session.clone();
        b.to_async(&rt).iter(|| {
            let rt1 = Arc::clone(&rt1);
            let s = s.clone();
            async move { rt1.render_slot("test-slot", &s).await.expect("render") }
        });
    });

    group.bench_function("2_plugins", |b| {
        let rt2 = Arc::clone(&runtime_2);
        let s = session.clone();
        b.to_async(&rt).iter(|| {
            let rt2 = Arc::clone(&rt2);
            let s = s.clone();
            async move { rt2.render_slot("test-slot", &s).await.expect("render") }
        });
    });

    group.bench_function("4_concurrent", |b| {
        let rt1 = Arc::clone(&runtime_1);
        let s = session.clone();
        b.to_async(&rt).iter(|| {
            let rt1 = Arc::clone(&rt1);
            let s = s.clone();
            async move {
                let handles: Vec<_> = (0..4)
                    .map(|_| {
                        let rt1 = Arc::clone(&rt1);
                        let s = s.clone();
                        tokio::spawn(async move { rt1.render_slot("test-slot", &s).await })
                    })
                    .collect();
                for h in handles {
                    h.await.expect("spawn").expect("render");
                }
            }
        });
    });

    group.finish();
}

// ── Group 3: hook chain execution ────────────────────────────────────────────

fn bench_hook_chain(c: &mut Criterion) {
    let rt = Runtime::new().expect("tokio runtime");

    let runtime_1 = rt
        .block_on(
            PluginRuntimeBuilder::new()
                .add_plugin(src(HOOK_CONTINUE))
                .build(),
        )
        .expect("build 1_handler runtime");

    let runtime_2 = rt
        .block_on(
            PluginRuntimeBuilder::new()
                .add_plugin(src(HOOK_CONTINUE))
                .add_plugin(src(HOOK_REPLACE))
                .build(),
        )
        .expect("build 2_handler runtime");

    let session = default_session();
    let payload = serde_json::json!({"x": 0});
    let mut group = c.benchmark_group("hook_chain");

    group.bench_function("1_handler", |b| {
        let rt1 = Arc::clone(&runtime_1);
        let s = session.clone();
        let p = payload.clone();
        b.to_async(&rt).iter(|| {
            let rt1 = Arc::clone(&rt1);
            let s = s.clone();
            let p = p.clone();
            async move { rt1.run_hook("test-hook", p, &s).await.expect("hook") }
        });
    });

    group.bench_function("2_handlers", |b| {
        let rt2 = Arc::clone(&runtime_2);
        let s = session.clone();
        let p = payload.clone();
        b.to_async(&rt).iter(|| {
            let rt2 = Arc::clone(&rt2);
            let s = s.clone();
            let p = p.clone();
            async move { rt2.run_hook("test-hook", p, &s).await.expect("hook") }
        });
    });

    group.finish();
}

// ── Group 4: call_plugin dispatch ────────────────────────────────────────────

fn bench_call_plugin(c: &mut Criterion) {
    let tokio_rt = Runtime::new().expect("tokio runtime");

    let runtime = tokio_rt
        .block_on(
            PluginRuntimeBuilder::new()
                .add_plugin(src(ECHO_FN))
                .build(),
        )
        .expect("build echo_fn runtime");

    let session = default_session();
    let plugin_id = PluginId("test/echo-fn".into());
    let small = serde_json::json!({"k": "v"});
    let large = serde_json::json!({"data": "x".repeat(4096)});
    let mut group = c.benchmark_group("call_plugin");

    for (label, payload) in [("small_payload", &small), ("large_payload", &large)] {
        let payload = payload.clone();
        group.bench_with_input(BenchmarkId::new("echo_fn", label), &payload, |b, input| {
            let plugin_rt = Arc::clone(&runtime);
            let id = plugin_id.clone();
            let s = session.clone();
            b.to_async(&tokio_rt).iter(|| {
                let plugin_rt = Arc::clone(&plugin_rt);
                let id = id.clone();
                let s = s.clone();
                let input = input.clone();
                async move {
                    plugin_rt
                        .call_plugin::<_, serde_json::Value>(&id, "echo_fn", &input, &s)
                        .await
                        .expect("call_plugin")
                }
            });
        });
    }

    group.finish();
}

// ── Group 5: metrics recording overhead ──────────────────────────────────────

fn bench_metrics_overhead(c: &mut Criterion) {
    let tokio_rt = Runtime::new().expect("tokio runtime");

    let runtime_no_metrics = tokio_rt
        .block_on(
            PluginRuntimeBuilder::new()
                .add_plugin(src(SLOT_NORMAL))
                .build(),
        )
        .expect("build no-metrics runtime");

    let runtime_noop = tokio_rt
        .block_on(
            PluginRuntimeBuilder::new()
                .add_plugin(src(SLOT_NORMAL))
                .with_metrics(NoopMetrics)
                .build(),
        )
        .expect("build noop-metrics runtime");

    let session = default_session();
    let mut group = c.benchmark_group("metrics_overhead");

    group.bench_function("no_metrics", |b| {
        let plugin_rt = Arc::clone(&runtime_no_metrics);
        let s = session.clone();
        b.to_async(&tokio_rt).iter(|| {
            let plugin_rt = Arc::clone(&plugin_rt);
            let s = s.clone();
            async move { plugin_rt.render_slot("test-slot", &s).await.expect("render") }
        });
    });

    group.bench_function("noop_metrics", |b| {
        let plugin_rt = Arc::clone(&runtime_noop);
        let s = session.clone();
        b.to_async(&tokio_rt).iter(|| {
            let plugin_rt = Arc::clone(&plugin_rt);
            let s = s.clone();
            async move { plugin_rt.render_slot("test-slot", &s).await.expect("render") }
        });
    });

    group.finish();
}

// ── Group 6: registry lookup ──────────────────────────────────────────────────

fn bench_registry_lookup(c: &mut Criterion) {
    let tokio_rt = Runtime::new().expect("tokio runtime");

    let runtime_1 = tokio_rt
        .block_on(
            PluginRuntimeBuilder::new()
                .add_plugin(src(SLOT_NORMAL))
                .build(),
        )
        .expect("build 1_plugin runtime");

    let runtime_2 = tokio_rt
        .block_on(
            PluginRuntimeBuilder::new()
                .add_plugin(src(SLOT_NORMAL))
                .add_plugin(src(SLOT_HIGH))
                .build(),
        )
        .expect("build 2_plugin runtime");

    let mut group = c.benchmark_group("registry_lookup");

    group.bench_function("1_plugin", |b| {
        let plugin_rt = Arc::clone(&runtime_1);
        b.to_async(&tokio_rt).iter(|| {
            let plugin_rt = Arc::clone(&plugin_rt);
            async move { plugin_rt.list_plugins().await }
        });
    });

    group.bench_function("2_plugins", |b| {
        let plugin_rt = Arc::clone(&runtime_2);
        b.to_async(&tokio_rt).iter(|| {
            let plugin_rt = Arc::clone(&plugin_rt);
            async move { plugin_rt.list_plugins().await }
        });
    });

    group.finish();
}

// ── Registration ──────────────────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_runtime_build,
    bench_slot_render,
    bench_hook_chain,
    bench_call_plugin,
    bench_metrics_overhead,
    bench_registry_lookup,
);
criterion_main!(benches);
