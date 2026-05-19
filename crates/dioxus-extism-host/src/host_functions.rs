use std::sync::Arc;

use crate::runtime::PluginRuntime;

/// Build all host function stubs exposed to WASM plugins.
pub(crate) fn make_host_functions(_runtime: Arc<PluginRuntime>) -> Vec<extism::Function> {
    vec![
        make_state_get_stub(),
        make_state_set_stub(),
        make_state_delete_stub(),
        make_global_state_get_stub(),
        make_global_state_set_stub(),
        make_plugin_state_get_stub(),
        make_emit_event_stub(),
        make_log_stub(),
        make_http_fetch_stub(),
        make_invoke_stub(),
    ]
}

fn null_output(plugin: &mut extism::CurrentPlugin, outputs: &mut [extism::Val]) -> Result<(), extism::Error> {
    if !outputs.is_empty() {
        let handle = plugin.memory_new("null")?;
        outputs[0] = plugin.memory_to_val(handle);
    }
    Ok(())
}

fn make_state_get_stub() -> extism::Function {
    extism::Function::new(
        "dx_state_get",
        [extism::PTR],
        [extism::PTR],
        extism::UserData::new(()),
        |plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_state_get stub called");
            null_output(plugin, outputs)
        },
    )
}

fn make_state_set_stub() -> extism::Function {
    extism::Function::new(
        "dx_state_set",
        [extism::PTR, extism::PTR],
        [],
        extism::UserData::new(()),
        |_plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         _outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_state_set stub called");
            Ok(())
        },
    )
}

fn make_state_delete_stub() -> extism::Function {
    extism::Function::new(
        "dx_state_delete",
        [extism::PTR],
        [],
        extism::UserData::new(()),
        |_plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         _outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_state_delete stub called");
            Ok(())
        },
    )
}

fn make_global_state_get_stub() -> extism::Function {
    extism::Function::new(
        "dx_global_state_get",
        [extism::PTR],
        [extism::PTR],
        extism::UserData::new(()),
        |plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_global_state_get stub called");
            null_output(plugin, outputs)
        },
    )
}

fn make_global_state_set_stub() -> extism::Function {
    extism::Function::new(
        "dx_global_state_set",
        [extism::PTR, extism::PTR],
        [],
        extism::UserData::new(()),
        |_plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         _outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_global_state_set stub called");
            Ok(())
        },
    )
}

fn make_plugin_state_get_stub() -> extism::Function {
    extism::Function::new(
        "dx_plugin_state_get",
        [extism::PTR, extism::PTR],
        [extism::PTR],
        extism::UserData::new(()),
        |plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_plugin_state_get stub called");
            null_output(plugin, outputs)
        },
    )
}

fn make_emit_event_stub() -> extism::Function {
    extism::Function::new(
        "dx_emit_event",
        [extism::PTR],
        [],
        extism::UserData::new(()),
        |_plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         _outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_emit_event stub called");
            Ok(())
        },
    )
}

fn make_log_stub() -> extism::Function {
    extism::Function::new(
        "dx_log",
        [extism::PTR, extism::PTR],
        [],
        extism::UserData::new(()),
        |_plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         _outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_log stub called");
            Ok(())
        },
    )
}

fn make_http_fetch_stub() -> extism::Function {
    extism::Function::new(
        "dx_http_fetch",
        [extism::PTR],
        [extism::PTR],
        extism::UserData::new(()),
        |plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_http_fetch stub called");
            null_output(plugin, outputs)
        },
    )
}

fn make_invoke_stub() -> extism::Function {
    extism::Function::new(
        "dx_invoke",
        [extism::PTR, extism::PTR],
        [extism::PTR],
        extism::UserData::new(()),
        |plugin: &mut extism::CurrentPlugin,
         _inputs: &[extism::Val],
         outputs: &mut [extism::Val],
         _user_data: extism::UserData<()>| -> Result<(), extism::Error> {
            tracing::debug!("dx_invoke stub called");
            null_output(plugin, outputs)
        },
    )
}
