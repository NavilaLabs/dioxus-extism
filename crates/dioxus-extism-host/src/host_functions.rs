use std::{
    cell::RefCell,
    collections::HashSet,
    sync::{Arc, Mutex},
};

use dioxus_extism_protocol::{ClientCapabilities, PluginId, SessionId};

use tokio::sync::RwLock;

use crate::runtime::{GlobalStateMap, InvocationRegistry, SessionStateMap};
use crate::InvocationError;

// ── Per-call session context (thread-local) ───────────────────────────────────
// Set by call_export before each spawn_blocking call. Host functions read these
// to identify the current session without needing per-instance UserData.

thread_local! {
    static CALL_SESSION_ID: RefCell<SessionId> = RefCell::new(SessionId(String::new()));
    static CALL_CLIENT: RefCell<ClientCapabilities> = RefCell::new(ClientCapabilities {
        protocol_version: 0,
        app_version: 0,
        registered_host_components: Vec::new(),
    });
}

/// Set the thread-local session context before a blocking plugin call.
pub(crate) fn set_call_session(session_id: SessionId, client: ClientCapabilities) {
    CALL_SESSION_ID.with(|c| *c.borrow_mut() = session_id);
    CALL_CLIENT.with(|c| *c.borrow_mut() = client);
}

pub(crate) fn get_call_session_id() -> SessionId {
    CALL_SESSION_ID.with(|c| c.borrow().clone())
}

// ── Call context ──────────────────────────────────────────────────────────────

/// Context carried in every host function's `UserData`.
/// One instance per plugin pool; all pool instances share it via `Arc<Mutex<...>>`.
/// Per-call session info (session_id, client) lives in thread-locals above.
#[derive(Clone)]
pub(crate) struct CallCtx {
    /// The plugin that owns this pool — constant for the pool's lifetime.
    pub caller: PluginId,
    pub session_states: Arc<RwLock<SessionStateMap>>,
    pub global_states: Arc<RwLock<GlobalStateMap>>,
    pub invocation_registry: Arc<InvocationRegistry>,
    pub granted_invocations: HashSet<String>,
    pub granted_global_read: HashSet<String>,
    pub granted_global_write: HashSet<String>,
}

/// Build all host functions, wiring them to `ctx`.
pub(crate) fn make_host_functions(
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
            let (session_states, caller) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (ctx.session_states.clone(), ctx.caller.clone())
            };
            let session_id = get_call_session_id();
            let handle = tokio::runtime::Handle::current();
            let value = handle.block_on(async {
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
            let (session_states, caller) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (ctx.session_states.clone(), ctx.caller.clone())
            };
            let session_id = get_call_session_id();
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
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
            let (session_states, caller) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (ctx.session_states.clone(), ctx.caller.clone())
            };
            let session_id = get_call_session_id();
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
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
                    "capability denied: GlobalStateRead({key}) not granted to {:?}",
                    caller
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
            let (global_states, caller, granted) = {
                let ctx = arc.lock().map_err(|_| anyhow::anyhow!("CallCtx mutex poisoned"))?;
                (
                    ctx.global_states.clone(),
                    ctx.caller.clone(),
                    ctx.granted_global_write.clone(),
                )
            };
            if !granted.is_empty() && !granted.contains(&key) {
                return Err(anyhow::anyhow!(
                    "capability denied: GlobalStateWrite({key}) not granted to {:?}",
                    caller
                ));
            }
            let handle = tokio::runtime::Handle::current();
            handle.block_on(async {
                let mut states = global_states.write().await;
                states.entry(caller).or_default().insert(key, value);
            });
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
              _user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            let raw: String = plugin.memory_get_val(&inputs[0])?;
            // Parse and log the event; full routing wired via emit_event() on PluginRuntime.
            let event: serde_json::Value = serde_json::from_str(&raw)?;
            tracing::debug!("dx_emit_event: {event}");
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
              _inputs: &[extism::Val],
              outputs: &mut [extism::Val],
              _user_data: extism::UserData<CallCtx>|
              -> Result<(), extism::Error> {
            tracing::debug!("dx_http_fetch: not yet implemented");
            write_json_output(plugin, outputs, &serde_json::Value::Null)
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
                    "capability denied: Invoke({name}) not granted to {:?}",
                    caller
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
