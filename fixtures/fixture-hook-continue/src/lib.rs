use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{hook_export, plugin};
use serde_json::json;

struct FixtureHookContinue;

impl DioxusPlugin for FixtureHookContinue {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/hook-continue".into()),
            version: "0.1.0".into(),
            hooks: vec![HookRegistration {
                hook_name: "test-hook".into(),
                priority_hint: PriorityHint::First,
            }],
            ..Default::default()
        }
    }
}

impl HookHandler for FixtureHookContinue {
    const HOOK_NAME: &'static str = "test-hook";

    fn handle(_call: HookCall, _ctx: &PluginCtx) -> Result<HookResult, PdkError> {
        Ok(HookResult::Continue { context: json!("continued") })
    }
}

plugin! { type: FixtureHookContinue }
hook_export!(FixtureHookContinue, hook_test_hook);
