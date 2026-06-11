#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{hook_export, plugin};

struct FixtureHookAfterCancel;

impl DioxusPlugin for FixtureHookAfterCancel {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/hook-after-cancel".into()),
            version: "0.1.0".into(),
            hooks: vec![HookRegistration {
                hook_name: "test-hook".into(),
                // Priority will be overridden below fixture-hook-cancel via PluginInstallConfig.
                priority_hint: PriorityHint::Last,
            }],
            host_capabilities: vec![
                HostCapability::GlobalStateRead { keys: vec!["after_cancel_count".into()] },
                HostCapability::GlobalStateWrite { keys: vec!["after_cancel_count".into()] },
            ],
            ..Default::default()
        }
    }
}

impl HookHandler for FixtureHookAfterCancel {
    const HOOK_NAME: &'static str = "test-hook";

    fn handle(call: HookCall, _ctx: &PluginCtx) -> Result<HookResult, PdkError> {
        unsafe {
            let raw = host_fns::dx_global_state_get("after_cancel_count")
                .unwrap_or_else(|_| "0".into());
            let count: u64 = serde_json::from_str::<u64>(&raw).unwrap_or(0);
            let encoded =
                serde_json::to_string(&(count + 1)).map_err(PdkError::Json)?;
            host_fns::dx_global_state_set("after_cancel_count", encoded)
                .map_err(|e| PdkError::HostFn(e.to_string()))?;
        }
        Ok(HookResult::Continue { context: call.context })
    }
}

plugin! { type: FixtureHookAfterCancel }
hook_export!(FixtureHookAfterCancel, hook_test_hook);
