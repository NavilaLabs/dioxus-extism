use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{hook_export, plugin};
use serde_json::json;

struct FixtureHookReplace;

impl DioxusPlugin for FixtureHookReplace {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("test/hook-replace".into()),
            version: "0.1.0".into(),
            hooks: vec![HookRegistration {
                hook_name: "test-hook".into(),
                priority_hint: PriorityHint::Normal,
            }],
            ..Default::default()
        }
    }
}

impl HookHandler for FixtureHookReplace {
    const HOOK_NAME: &'static str = "test-hook";

    fn handle(_call: HookCall, _ctx: &PluginCtx) -> Result<HookResult, PdkError> {
        Ok(HookResult::Replace { context: json!("replaced") })
    }
}

plugin! { type: FixtureHookReplace }
hook_export!(FixtureHookReplace, hook_test_hook);
