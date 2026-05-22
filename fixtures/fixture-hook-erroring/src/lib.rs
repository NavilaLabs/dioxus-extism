use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{hook_export, plugin};

struct FixtureHookErroring;

impl DioxusPlugin for FixtureHookErroring {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/hook-erroring".into()),
            version: "0.1.0".into(),
            hooks: vec![HookRegistration {
                hook_name: "test-hook".into(),
                priority_hint: PriorityHint::High,
            }],
            ..Default::default()
        }
    }
}

impl HookHandler for FixtureHookErroring {
    const HOOK_NAME: &'static str = "test-hook";

    fn handle(_call: HookCall, _ctx: &PluginCtx) -> Result<HookResult, PdkError> {
        Err(PdkError::Custom("hook error".into()))
    }
}

plugin! { type: FixtureHookErroring }
hook_export!(FixtureHookErroring, hook_test_hook);
