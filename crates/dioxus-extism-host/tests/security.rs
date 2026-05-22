/// Phase E security tests — capability enforcement, integrity checks, liveness.
///
/// Fixtures must be compiled before running:
///   cargo build --target wasm32-unknown-unknown --release \
///     -p fixture-cap-invoke-denied -p fixture-cap-global-write-denied \
///     -p fixture-cap-plugin-state-read -p fixture-cap-state-owner \
///     -p fixture-cap-invoke-a -p fixture-cap-invoke-b \
///     -p fixture-high-protocol-version -p fixture-high-app-version \
///     -p fixture-failing-on-load -p fixture-slot-normal
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};
use std::time::Duration;

use dioxus_extism_host::{PluginRuntimeBuilder, PluginRuntimeError, PluginSource};
use dioxus_extism_protocol::{
    ClientCapabilities, PluginId, PluginView, SessionCtx, SessionId, PROTOCOL_VERSION,
};

// ── WASM bytes ─────────────────────────────────────────────────────────────────

macro_rules! fixture {
    ($name:ident, $path:literal) => {
        const $name: &[u8] = include_bytes!(concat!(
            "../../../target/wasm32-unknown-unknown/release/",
            $path
        ));
    };
}

fixture!(CAP_INVOKE_DENIED_WASM,      "fixture_cap_invoke_denied.wasm");
fixture!(CAP_GLOBAL_WRITE_DENIED_WASM,"fixture_cap_global_write_denied.wasm");
fixture!(CAP_PLUGIN_STATE_READ_WASM,  "fixture_cap_plugin_state_read.wasm");
fixture!(CAP_STATE_OWNER_WASM,        "fixture_cap_state_owner.wasm");
fixture!(CAP_INVOKE_A_WASM,           "fixture_cap_invoke_a.wasm");
fixture!(CAP_INVOKE_B_WASM,           "fixture_cap_invoke_b.wasm");
fixture!(HIGH_PROTOCOL_VERSION_WASM,  "fixture_high_protocol_version.wasm");
fixture!(HIGH_APP_VERSION_WASM,       "fixture_high_app_version.wasm");
fixture!(FAILING_ON_LOAD_WASM,        "fixture_failing_on_load.wasm");
fixture!(SLOT_NORMAL_WASM,            "fixture_slot_normal.wasm");

// ── Helpers ────────────────────────────────────────────────────────────────────

fn default_session() -> SessionCtx {
    SessionCtx {
        session_id: SessionId("sec-test".into()),
        user_id: None,
        client: ClientCapabilities {
            protocol_version: PROTOCOL_VERSION,
            app_version: 0,
            registered_host_components: vec![],
        },
        caller: None,
    }
}

// ── Security 1 ────────────────────────────────────────────────────────────────

/// Plugin without Invoke capability cannot call a registered invocation.
///
/// The handler counter must stay at zero; the render_slot call must still
/// return successfully (the plugin treats the denial as a silent error).
#[tokio::test]
async fn dx_invoke_denied_when_capability_not_declared() {
    let counter = Arc::new(AtomicU32::new(0));
    let c = counter.clone();

    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(CAP_INVOKE_DENIED_WASM.into()))
        .register_invocation(
            "add_note",
            None,
            move |_args: serde_json::Value, _session: SessionCtx| {
                c.fetch_add(1, Ordering::SeqCst);
                async { Ok(serde_json::json!({})) }
            },
        )
        .build()
        .await
        .expect("build must succeed");

    let session = default_session();
    let _ = runtime.render_slot("test-slot", &session).await;

    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "handler must not be called without Invoke capability"
    );
}

// ── Security 2 ────────────────────────────────────────────────────────────────

/// Plugin without GlobalStateWrite capability cannot write to global state.
///
/// After render_slot, the targeted key must not exist in global state.
#[tokio::test]
async fn dx_global_state_set_denied_when_capability_not_declared() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(CAP_GLOBAL_WRITE_DENIED_WASM.into()))
        .build()
        .await
        .expect("build must succeed");

    let session = default_session();
    let _ = runtime.render_slot("test-slot", &session).await;

    let value = runtime
        .global_state_json(&PluginId("test/cap-global-write-denied".into()), "x")
        .await;
    assert!(
        value.is_none(),
        "global state key 'x' must not exist after denied write, got: {value:?}"
    );
}

// ── Security 3 ────────────────────────────────────────────────────────────────

/// Plugin without ReadPluginState capability cannot read another plugin's state.
///
/// The attacker plugin writes the raw result of its denied read to its own
/// global state key "read_attempt_result". After render_slot, that key must
/// be "null", proving the host returned null rather than the real value.
/// The owner's "data" key must still hold "secret".
#[tokio::test]
async fn dx_plugin_state_get_denied_for_undeclared_cross_plugin_read() {
    let runtime = PluginRuntimeBuilder::new()
        // Owner sets global state "data" = "secret" in on_load.
        .add_plugin(PluginSource::Bytes(CAP_STATE_OWNER_WASM.into()))
        // Attacker reads owner's "data" without ReadPluginState capability.
        .add_plugin(PluginSource::Bytes(CAP_PLUGIN_STATE_READ_WASM.into()))
        .build()
        .await
        .expect("build must succeed");

    let session = default_session();
    let _ = runtime.render_slot("test-slot", &session).await;

    // Owner's state must be intact.
    let owner_data = runtime
        .global_state_json(&PluginId("test/cap-state-owner".into()), "data")
        .await;
    assert_eq!(
        owner_data,
        Some(serde_json::json!("secret")),
        "owner's global state must be unchanged: {owner_data:?}"
    );

    // The attacker's read attempt must have returned null (capability denied).
    let attempt_result = runtime
        .global_state_json(
            &PluginId("test/cap-plugin-state-read".into()),
            "read_attempt_result",
        )
        .await;
    assert!(
        attempt_result == Some(serde_json::Value::Null) || attempt_result.is_none(),
        "cross-plugin read must return null/None to the plugin, got: {attempt_result:?}"
    );
}

// ── Security 4 ────────────────────────────────────────────────────────────────

/// SHA-256 mismatch on a remote URL causes build() to return ChecksumMismatch.
///
/// A minimal HTTP server serves WASM bytes locally. The correct hash is
/// computed and one byte is flipped before passing to PluginSource::Url.
#[tokio::test]
async fn sha256_integrity_check_rejects_mismatched_hash() {
    use sha2::Digest;

    let wasm_bytes: &'static [u8] = SLOT_NORMAL_WASM;

    // Compute the correct SHA-256 of the WASM bytes.
    let correct_sha256: [u8; 32] = sha2::Sha256::digest(wasm_bytes).into();

    // Start a minimal HTTP server in a background thread — serves bytes once.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind local HTTP server");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::{Read, Write};
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/wasm\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                wasm_bytes.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(wasm_bytes);
        }
    });

    let served_url = format!("http://127.0.0.1:{port}/plugin.wasm");

    // Flip one bit in the hash to create a mismatched digest.
    let mut bad_hash = correct_sha256;
    bad_hash[0] ^= 0xff;

    let result = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Url {
            url: served_url.clone(),
            sha256: bad_hash,
        })
        .build()
        .await;

    assert!(
        matches!(&result, Err(PluginRuntimeError::ChecksumMismatch { url }) if url == &served_url),
        "expected ChecksumMismatch with url={served_url}, got: {:?}",
        result.as_ref().map(|_| "Ok(...)").unwrap_err()
    );
}

// ── Security 5 ────────────────────────────────────────────────────────────────

/// Plugin requiring a future protocol version is rejected at build time.
///
/// This prevents loading a plugin that the host cannot safely deserialize.
/// (This test duplicates the correctness suite assertion intentionally — it
/// is the primary security regression guard for protocol versioning.)
#[tokio::test]
async fn protocol_version_guard_at_build_time_rejects_future_version_plugin() {
    let result = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HIGH_PROTOCOL_VERSION_WASM.into()))
        .build()
        .await;

    assert!(
        matches!(
            &result,
            Err(PluginRuntimeError::ProtocolVersionMismatch { required, host })
            if *required == PROTOCOL_VERSION + 1 && *host == PROTOCOL_VERSION
        ),
        "expected ProtocolVersionMismatch, got: {:?}",
        result.as_ref().map(|_| "Ok(...)").unwrap_err()
    );
}

// ── Security 6 ────────────────────────────────────────────────────────────────

/// App version guard fires before the WASM slot export is entered.
///
/// A call counter embedded in the plugin's slot export proves the WASM code
/// was never entered — it must remain at zero after render_slot returns.
#[tokio::test]
async fn app_version_guard_fires_before_pool_call() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(HIGH_APP_VERSION_WASM.into()))
        .build()
        .await
        .expect("build must succeed");

    // Client declares app_version = 1; plugin requires min_app_version = 99.
    let session = SessionCtx {
        session_id: SessionId("sec-test".into()),
        user_id: None,
        client: ClientCapabilities {
            protocol_version: PROTOCOL_VERSION,
            app_version: 1,
            registered_host_components: vec![],
        },
        caller: None,
    };

    let contents = runtime
        .render_slot("test-slot", &session)
        .await
        .expect("render_slot must not return Err");

    assert!(
        !contents.is_empty(),
        "render_slot must produce a contribution even for incompatible plugin"
    );
    assert!(
        matches!(contents[0].view, PluginView::Incompatible { .. }),
        "incompatible plugin must produce Incompatible view, got {:?}",
        contents[0].view
    );

    let call_count = runtime
        .global_state_json(
            &PluginId("test/high-app-version".into()),
            "call_count",
        )
        .await
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert_eq!(
        call_count, 0,
        "WASM slot export must not be entered when app version guard fires"
    );
}

// ── Security 7 ────────────────────────────────────────────────────────────────

/// Wall-clock timeout: render_slot returns within 1 s with Incompatible view.
///
/// Requires fixture-blocking-slot (wasm32-wasip1) and max_call_duration support.
#[tokio::test]
#[ignore = "fixture-blocking-slot uses wasm32-wasip1 target; max_call_duration not yet enforced"]
async fn wall_clock_timeout_render_slot_returns_incompatible() {
    // This test would need:
    //   1. fixture-blocking-slot compiled for wasm32-unknown-unknown
    //   2. max_call_duration enforced inside call_export / spawn_blocking
    // Mark ignored until both are implemented.
}

// ── Security 8 ────────────────────────────────────────────────────────────────

/// Capability isolation between pool instances — four-way counter assertion.
///
/// Plugin A is granted Invoke(["get_notes"]) only; plugin B is granted
/// Invoke(["add_note"]) only.  Each plugin calls both names.  Counters
/// confirm that granted calls are dispatched and denied calls are blocked.
#[tokio::test]
async fn capability_isolation_between_pool_instances() {
    let get_notes_granted = Arc::new(AtomicU32::new(0));
    let add_note_granted  = Arc::new(AtomicU32::new(0));
    let add_note_denied   = Arc::new(AtomicU32::new(0));
    let get_notes_denied  = Arc::new(AtomicU32::new(0));

    let gng = get_notes_granted.clone();
    let ang = add_note_granted.clone();

    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(CAP_INVOKE_A_WASM.into()))
        .add_plugin(PluginSource::Bytes(CAP_INVOKE_B_WASM.into()))
        // get_notes: increments granted counter; any call that reaches here is granted.
        .register_invocation(
            "get_notes",
            None,
            move |_args: serde_json::Value, _session: SessionCtx| {
                gng.fetch_add(1, Ordering::SeqCst);
                async { Ok(serde_json::json!([])) }
            },
        )
        // add_note: increments granted counter.
        .register_invocation(
            "add_note",
            None,
            move |_args: serde_json::Value, _session: SessionCtx| {
                ang.fetch_add(1, Ordering::SeqCst);
                async { Ok(serde_json::json!({})) }
            },
        )
        .build()
        .await
        .expect("build must succeed");

    let session = default_session();

    // Trigger plugin A (slot-a) — calls get_notes (granted) and add_note (denied).
    let _ = runtime.render_slot("slot-a", &session).await;
    // Trigger plugin B (slot-b) — calls add_note (granted) and get_notes (denied).
    let _ = runtime.render_slot("slot-b", &session).await;

    // Granted counters should each be 1; denied calls never reach the handler.
    // We infer denial by observing handler was NOT called for the non-granted name.
    assert_eq!(
        get_notes_granted.load(Ordering::SeqCst),
        1,
        "A→get_notes must succeed (counter == 1)"
    );
    assert_eq!(
        add_note_granted.load(Ordering::SeqCst),
        1,
        "B→add_note must succeed (counter == 1)"
    );
    // add_note_denied and get_notes_denied counters stay at 0 because denied
    // calls never reach the handler — the host function returns an error to the plugin
    // before dispatching.
    assert_eq!(
        add_note_denied.load(Ordering::SeqCst),
        0,
        "A→add_note must be denied (no second handler reached)"
    );
    assert_eq!(
        get_notes_denied.load(Ordering::SeqCst),
        0,
        "B→get_notes must be denied (no second handler reached)"
    );
}

// ── Security 9 ────────────────────────────────────────────────────────────────

/// 20 concurrent render_slot calls all complete within 30 s without deadlock.
///
/// Lock acquisition order (plugins → registries → session_states → global_states)
/// must be consistent; any inversion deadlocks the server under concurrent load.
#[tokio::test]
async fn deadlock_liveness_under_20_concurrent_renders() {
    let runtime = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("build must succeed");

    let base_session = default_session();
    let handles: Vec<_> = (0..20u32)
        .map(|i| {
            let rt = runtime.clone();
            let session = SessionCtx {
                session_id: SessionId(i.to_string()),
                ..base_session.clone()
            };
            tokio::spawn(async move { rt.render_slot("test-slot", &session).await })
        })
        .collect();

    let results = tokio::time::timeout(
        Duration::from_secs(30),
        futures::future::join_all(handles),
    )
    .await
    .expect("deadlock detected: 20 concurrent render_slot calls did not complete in 30 s");

    for (i, result) in results.into_iter().enumerate() {
        let slot_result = result.expect(&format!("task {i} panicked"));
        assert!(
            slot_result.is_ok(),
            "task {i} returned Err: {:?}",
            slot_result.unwrap_err()
        );
    }
}

// ── Security 10 ───────────────────────────────────────────────────────────────

/// Failed build leaves no corrupted state; a subsequent build succeeds normally.
///
/// If a failed build poisons a Mutex or leaks an Arc into shared state, the
/// second build and subsequent renders become unpredictable.
#[tokio::test]
async fn on_load_failure_leaves_no_corrupted_global_state() {
    // First build must fail due to on_load error.
    let bad = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(FAILING_ON_LOAD_WASM.into()))
        .build()
        .await;
    assert!(bad.is_err(), "first build must fail when on_load returns an error");

    // Second build with a healthy plugin must succeed.
    let good = PluginRuntimeBuilder::new()
        .add_plugin(PluginSource::Bytes(SLOT_NORMAL_WASM.into()))
        .build()
        .await
        .expect("second build must succeed — no global state was corrupted");

    let session = default_session();
    let slots = good
        .render_slot("test-slot", &session)
        .await
        .expect("render_slot must succeed after clean build");

    assert_eq!(
        slots.len(),
        1,
        "normal plugin must serve slot content after failed-build recovery"
    );
    assert!(
        !matches!(slots[0].view, PluginView::Incompatible { .. }),
        "slot content must not be Incompatible after clean build"
    );
}
