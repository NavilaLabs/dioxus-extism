use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{hook_export, plugin};

struct FixtureHookCancel;

impl DioxusPlugin for FixtureHookCancel {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/hook-cancel".into()),
            version: "0.1.0".into(),
            hooks: vec![HookRegistration {
                hook_name: "test-hook".into(),
                priority_hint: PriorityHint::Last,
            }],
            ..Default::default()
        }
    }
}

impl HookHandler for FixtureHookCancel {
    const HOOK_NAME: &'static str = "test-hook";

    fn handle(_call: HookCall, _ctx: &PluginCtx) -> Result<HookResult, PdkError> {
        Ok(HookResult::Cancel { reason: "test-cancel".into() })
    }
}

plugin! { type: FixtureHookCancel }
hook_export!(FixtureHookCancel, hook_test_hook);
