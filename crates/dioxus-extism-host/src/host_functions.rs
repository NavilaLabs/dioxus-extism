use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Instant,
};

use dioxus_extism_protocol::{ClientCapabilities, EventSource, PluginEvent, PluginId, SessionCtx, SessionId};

use tokio::sync::RwLock;

use crate::runtime::{GlobalStateMap, InvocationRegistry, SessionStateMap, StatePersistenceProvider};
use crate::InvocationError;

// ── Per-call session context (thread-local) ───────────────────────────────────
// Set by call_export before each spawn_blocking call. Host functions read these
// to identify the current session without needing per-instance UserData.

thread_local! {
    static CALL_SESSION_ID: RefCell<SessionId> = const { RefCell::new(SessionId(String::new())) };
    static CALL_CLIENT: RefCell<ClientCapabilities> = const { RefCell::new(ClientCapabilities {
        protocol_version: 0,
        app_version: 0,
        registered_host_components: Vec::new(),
    }) };
}

/// Set the thread-local session context before a blocking plugin call.
pub fn set_call_session(session_id: SessionId, client: ClientCapabilities) {
    CALL_SESSION_ID.with(|c| *c.borrow_mut() = session_id);
    CALL_CLIENT.with(|c| *c.borrow_mut() = client);
}

pub fn get_call_session_id() -> SessionId {
    CALL_SESSION_ID.with(|c| c.borrow().clone())
}

// ── Call context ──────────────────────────────────────────────────────────────

/// Context carried in every host function's `UserData`.
/// One instance per plugin pool; all pool instances share it via `Arc<Mutex<...>>`.
/// Per-call session info (`session_id`, client) lives in thread-locals above.
#[derive(Clone)]
pub struct CallCtx {
    /// The plugin that owns this pool — constant for the pool's lifetime.
    pub caller: PluginId,
    pub session_states: Arc<RwLock<SessionStateMap>>,
    /// Last-access timestamps for each session, updated on every state read and write.
    pub session_last_access: Arc<RwLock<HashMap<SessionId, Instant>>>,
    pub global_states: Arc<RwLock<GlobalStateMap>>,
    pub invocation_registry: Arc<InvocationRegistry>,
    /// Optional persistence backend; flushed after every global-state write.
    pub persistence: Option<Arc<dyn StatePersistenceProvider>>,
    pub granted_invocations: HashSet<String>,
    pub granted_global_read: HashSet<String>,
    pub granted_global_write: HashSet<String>,
    /// Hostnames (without scheme/path/port) the plugin is allowed to contact.
    /// An empty list means Http capability was not requested.
    pub granted_http_hosts: Vec<String>,
    /// Sender half of the shared event bus; plugin calls `dx_emit_event` → sends here.
    pub event_tx: tokio::sync::mpsc::UnboundedSender<(PluginEvent, SessionCtx)>,
}

/// Build all host functions, wiring them to `ctx`.
pub fn make_host_functions(
    user_data: extism::UserData<CallCtx>,
) -> Vec<extism::Function> {
    vec![
        make_state_get(user_data.clone()),
        make_state_set(user_data.clone()),
        make_state_delete(user_data.clone()),
        make_global_state_get(user_data.clone()),
        make_global_state_set(user_data.clone()),
        make_plugin_state_get(user_data.clone()),
        make_emit_event(user_data.clone()),
        make_log(user_data.clone()),
        make_http_fetch(user_data.clone()),
        make_invoke(user_data),
    ]
}

/// Build no-op stub host functions for manifest-only plugin loading.
///
/// The manifest export is pure and never calls host functions, but the WASM binary
/// declares them as imports. Extism validates all imports at instantiation time, so
/// stubs are required to satisfy the linker even though they are never invoked.
pub fn make_stub_host_functions() -> Vec<extism::Function> {
    fn null_output(
        plugin: &mut extism::CurrentPlugin,
        _inputs: &[extism::Val],
        outputs: &mut [extism::Val],
        _ud: extism::UserData<()>,
    ) -> Result<(), extism::Error> {
        let handle = plugin.memory_new("null")?;
        outputs[0] = plugin.memory_to_val(handle);
        Ok(())
    }
    #[allow(clippy::unnecessary_wraps)]
    fn no_output(
        _plugin: &mut extism::CurrentPlugin,
        _inputs: &[extism::Val],
        _outputs: &mut [extism::Val],
        _ud: extism::UserData<()>,
    ) -> Result<(), extism::Error> {
        Ok(())
    }
    let ud = extism::UserData::new(());
    vec![
        extism::Function::new("dx_state_get",         [extism::PTR],             [extism::PTR], ud.clone(), null_output),
        extism::Function::new("dx_state_set",         [extism::PTR, extism::PTR],            [], ud.clone(), no_output),
        extism::Function::new("dx_state_delete",      [extism::PTR],                         [], ud.clone(), no_output),
        extism::Function::new("dx_global_state_get",  [extism::PTR],             [extism::PTR], ud.clone(), null_output),
        extism::Function::new("dx_global_state_set",  [extism::PTR, extism::PTR],            [], ud.clone(), no_output),
        extism::Function::new("dx_plugin_state_get",  [extism::PTR, extism::PTR],[extism::PTR], ud.clone(), null_output),
        extism::Function::new("dx_emit_event",        [extism::PTR],                         [], ud.clone(), no_output),
        extism::Function::new("dx_log",               [extism::PTR, extism::PTR],            [], ud.clone(), no_output),
        extism::Function::new("dx_http_fetch",        [extism::PTR],             [extism::PTR], ud.clone(), null_output),
        extism::Function::new("dx_invoke",            [extism::PTR, extism::PTR],[extism::PTR], ud,          null_output),
    ]
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn extract_ctx(user_data: &extism::UserData<CallCtx>) -> Result<Arc<Mutex<CallCtx>>, extism::Error> {
    user_data.get().map_err(|e| anyhow::anyhow!("UserData::get failed: {e}"))
}

fn write_json_output(
    plugin: &mut extism::CurrentPlugin,
    outputs: &mut [extism::Val],
    value: &impl serde::Serialize,
) -> Result<(), extism::Error> {
    let json = serde_json::to_string(value)?;
    let handle = plugin.memory_new(json.as_str())?;
    outputs[0] = plugin.memory_to_val(handle);
    Ok(())
}

// ── dx_state_get ─────────────────────────────────────────────────────────────

fn make_state_get(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_state_get",
        [extism::PTR],
        [extism::PTR],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let key: String = plugin.memory_get_val(&inputs[0])?;
            let arc = extract_ctx(&user_data)?;
            let (session_states, session_last_access, caller) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (ctx.session_states.clone(), ctx.session_last_access.clone(), ctx.caller.clone())
            };
            let session_id = get_call_session_id();
            let handle = tokio::runtime::Handle::current();
            let value = handle.block_on(async {
                session_last_access.write().await.insert(session_id.clone(), Instant::now());
                let states = session_states.read().await;
                states
                    .get(&session_id)
                    .and_then(|s| s.get(&caller))
                    .and_then(|p| p.get(&key))
                    .cloned()
            });
            write_json_output(plugin, outputs, &value)
        },
    )
}

// ── dx_state_set ─────────────────────────────────────────────────────────────

fn make_state_set(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_state_set",
        [extism::PTR, extism::PTR],
        [],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              _outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let key: String = plugin.memory_get_val(&inputs[0])?;
            let raw: String = plugin.memory_get_val(&inputs[1])?;
            let value: serde_json::Value = serde_json::from_str(&raw)?;
            let arc = extract_ctx(&user_data)?;
            let (session_states, session_last_access, caller) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (ctx.session_states.clone(), ctx.session_last_access.clone(), ctx.caller.clone())
            };
            let session_id = get_call_session_id();
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                session_last_access.write().await.insert(session_id.clone(), Instant::now());
                let mut states = session_states.write().await;
                states
                    .entry(session_id)
                    .or_default()
                    .entry(caller)
                    .or_default()
                    .insert(key, value);
            });
            Ok(())
        },
    )
}

// ── dx_state_delete ───────────────────────────────────────────────────────────

fn make_state_delete(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_state_delete",
        [extism::PTR],
        [],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              _outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let key: String = plugin.memory_get_val(&inputs[0])?;
            let arc = extract_ctx(&user_data)?;
            let (session_states, session_last_access, caller) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (ctx.session_states.clone(), ctx.session_last_access.clone(), ctx.caller.clone())
            };
            let session_id = get_call_session_id();
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                session_last_access.write().await.insert(session_id.clone(), Instant::now());
                let mut states = session_states.write().await;
                if let Some(plugin_map) = states
                    .get_mut(&session_id)
                    .and_then(|s| s.get_mut(&caller))
                {
                    plugin_map.remove(&key);
                }
            });
            Ok(())
        },
    )
}

// ── dx_global_state_get ───────────────────────────────────────────────────────

fn make_global_state_get(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_global_state_get",
        [extism::PTR],
        [extism::PTR],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let key: String = plugin.memory_get_val(&inputs[0])?;
            let arc = extract_ctx(&user_data)?;
            let (global_states, caller, granted) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (
                    ctx.global_states.clone(),
                    ctx.caller.clone(),
                    ctx.granted_global_read.clone(),
                )
            };
            if !granted.is_empty() && !granted.contains(&key) {
                return Err(anyhow::anyhow!(
                    "capability denied: GlobalStateRead({key}) not granted to {caller:?}"
                ));
            }
            let handle = tokio::runtime::Handle::current();
            let value = handle.block_on(async {
                let states = global_states.read().await;
                states
                    .get(&caller)
                    .and_then(|p| p.get(&key))
                    .cloned()
            });
            write_json_output(plugin, outputs, &value)
        },
    )
}

// ── dx_global_state_set ───────────────────────────────────────────────────────

fn make_global_state_set(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_global_state_set",
        [extism::PTR, extism::PTR],
        [],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              _outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let key: String = plugin.memory_get_val(&inputs[0])?;
            let raw: String = plugin.memory_get_val(&inputs[1])?;
            let value: serde_json::Value = serde_json::from_str(&raw)?;
            let arc = extract_ctx(&user_data)?;
            let (global_states, caller, granted, persistence) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (
                    ctx.global_states.clone(),
                    ctx.caller.clone(),
                    ctx.granted_global_write.clone(),
                    ctx.persistence.clone(),
                )
            };
            if !granted.is_empty() && !granted.contains(&key) {
                return Err(anyhow::anyhow!(
                    "capability denied: GlobalStateWrite({key}) not granted to {caller:?}"
                ));
            }
            let handle = tokio::runtime::Handle::current();
            let snapshot = handle.block_on(async {
                let mut states = global_states.write().await;
                states.entry(caller.clone()).or_default().insert(key.clone(), value);
                let snap = states.get(&caller).cloned().unwrap_or_default();
                drop(states);
                snap
            });
            // Async persistence flush — best-effort, does not block the host function.
            if let Some(p) = persistence {
                handle.spawn(async move {
                    if let Err(e) = p.save(&caller, &snapshot).await {
                        tracing::warn!(plugin = %caller.0, error = %e, "global state persistence flush failed");
                    }
                });
            }
            Ok(())
        },
    )
}

// ── dx_plugin_state_get ───────────────────────────────────────────────────────

fn make_plugin_state_get(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_plugin_state_get",
        [extism::PTR, extism::PTR],
        [extism::PTR],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let target_id: String = plugin.memory_get_val(&inputs[0])?;
            let key: String = plugin.memory_get_val(&inputs[1])?;
            let arc = extract_ctx(&user_data)?;
            let global_states = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                ctx.global_states.clone()
            };
            let target = PluginId(target_id);
            let handle = tokio::runtime::Handle::current();
            let value = handle.block_on(async {
                let states = global_states.read().await;
                states
                    .get(&target)
                    .and_then(|p| p.get(&key))
                    .cloned()
            });
            write_json_output(plugin, outputs, &value)
        },
    )
}

// ── dx_emit_event ─────────────────────────────────────────────────────────────

fn make_emit_event(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_emit_event",
        [extism::PTR],
        [],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              _outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let raw: String = plugin.memory_get_val(&inputs[0])?;
            let parsed: serde_json::Value = serde_json::from_str(&raw)?;

            let name = parsed["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("dx_emit_event: JSON must have a string 'name' field"))?
                .to_owned();
            let payload = parsed.get("payload").cloned().unwrap_or(serde_json::Value::Null);

            let arc = extract_ctx(&user_data)?;
            let (caller, event_tx) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (ctx.caller.clone(), ctx.event_tx.clone())
            };

            let event = PluginEvent { source: EventSource::Plugin(caller.clone()), name, payload };
            let session_id = get_call_session_id();
            let client = CALL_CLIENT.with(|c| c.borrow().clone());
            let session = SessionCtx { session_id, user_id: None, client, caller: Some(caller) };

            // Fire-and-forget — the dispatch task logs errors independently.
            let _ = event_tx.send((event, session));
            Ok(())
        },
    )
}

// ── dx_log ────────────────────────────────────────────────────────────────────

fn make_log(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_log",
        [extism::PTR, extism::PTR],
        [],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              _outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let level: String = plugin.memory_get_val(&inputs[0])?;
            let message: String = plugin.memory_get_val(&inputs[1])?;
            let arc = extract_ctx(&user_data)?;
            let caller = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                ctx.caller.0.clone()
            };
            match level.as_str() {
                "error" => tracing::error!(plugin = caller, "{message}"),
                "warn" => tracing::warn!(plugin = caller, "{message}"),
                "info" => tracing::info!(plugin = caller, "{message}"),
                _ => tracing::debug!(plugin = caller, "{message}"),
            }
            Ok(())
        },
    )
}

// ── dx_http_fetch ─────────────────────────────────────────────────────────────

fn make_http_fetch(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_http_fetch",
        [extism::PTR],
        [extism::PTR],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            use dioxus_extism_protocol::{HttpRequest, HttpResponse};

            let raw: String = plugin.memory_get_val(&inputs[0])?;
            let req: HttpRequest = serde_json::from_str(&raw)?;

            let arc = extract_ctx(&user_data)?;
            let (caller, allowed_hosts) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (ctx.caller.clone(), ctx.granted_http_hosts.clone())
            };

            // Check the URL host against the allowed list.
            let url = &req.url;
            let req_host = url
                .split("://")
                .nth(1)
                .unwrap_or(url)
                .split('/')
                .next()
                .unwrap_or(url)
                .split(':')
                .next()
                .unwrap_or(url);

            if !allowed_hosts.is_empty() && !allowed_hosts.iter().any(|h| h == req_host) {
                return Err(anyhow::anyhow!(
                    "capability denied: Http({req_host}) not in allowed_hosts for {caller:?}"
                ));
            }

            let handle = tokio::runtime::Handle::current();
            let response = handle.block_on(async {
                let client = reqwest::Client::new();
                let method = reqwest::Method::from_bytes(req.method.as_bytes())
                    .unwrap_or(reqwest::Method::GET);
                let mut builder = client.request(method, url);
                for (k, v) in &req.headers {
                    builder = builder.header(k, v);
                }
                if let Some(body) = req.body {
                    builder = builder.body(body);
                }
                let resp = builder.send().await?;
                let status = resp.status().as_u16();
                let mut headers = std::collections::HashMap::new();
                for (k, v) in resp.headers() {
                    if let Ok(v_str) = v.to_str() {
                        headers.insert(k.as_str().to_owned(), v_str.to_owned());
                    }
                }
                let body = resp.text().await?;
                Ok::<HttpResponse, reqwest::Error>(HttpResponse { status, headers, body })
            });

            match response {
                Ok(resp) => write_json_output(plugin, outputs, &resp),
                Err(e) => Err(anyhow::anyhow!("dx_http_fetch failed: {e}")),
            }
        },
    )
}

// ── dx_invoke ─────────────────────────────────────────────────────────────────

fn make_invoke(user_data: extism::UserData<CallCtx>) -> extism::Function {
    extism::Function::new(
        "dx_invoke",
        [extism::PTR, extism::PTR],
        [extism::PTR],
        user_data,
        move |plugin: &mut extism::CurrentPlugin,
              inputs: &[extism::Val],
              outputs: &mut [extism::Val],
              user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let name: String = plugin.memory_get_val(&inputs[0])?;
            let raw_args: String = plugin.memory_get_val(&inputs[1])?;
            let args: serde_json::Value = serde_json::from_str(&raw_args)?;

            let arc = extract_ctx(&user_data)?;
            let (invocation_registry, caller, granted) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (
                    ctx.invocation_registry.clone(),
                    ctx.caller.clone(),
                    ctx.granted_invocations.clone(),
                )
            };

            if !granted.contains(&name) {
                return Err(anyhow::anyhow!(
                    "capability denied: Invoke({name}) not granted to {caller:?}"
                ));
            }

            let session_id = get_call_session_id();
            let client = CALL_CLIENT.with(|c| c.borrow().clone());
            let session = dioxus_extism_protocol::SessionCtx {
                session_id,
                user_id: None,
                client,
                caller: Some(caller),
            };

            let handle = tokio::runtime::Handle::current();
            let result = handle.block_on(invocation_registry.call(&name, args, session));

            match result {
                Ok(value) => write_json_output(plugin, outputs, &value),
                Err(InvocationError::NotFound(n)) => {
                    Err(anyhow::anyhow!("invocation not found: {n}"))
                }
                Err(InvocationError::Timeout(d)) => {
                    Err(anyhow::anyhow!("invocation timed out after {d:?}"))
                }
                Err(InvocationError::Failed { code, message }) => {
                    Err(anyhow::anyhow!("invocation failed (code {code}): {message}"))
                }
                Err(InvocationError::BadArgs(e)) => {
                    Err(anyhow::anyhow!("bad invocation args: {e}"))
                }
            }
        },
    )
}
