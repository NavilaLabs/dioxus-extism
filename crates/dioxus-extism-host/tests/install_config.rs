use dioxus_extism_host::PluginInstallConfig;
use dioxus_extism_protocol::PriorityHint;

#[test]
fn resolve_priority_tier_precedence() {
    let cfg = PluginInstallConfig {
        overrides: [("slot_a".into(), 999)].into_iter().collect(),
        base_priority: Some(500),
        ..Default::default()
    };
    // Per-name override wins over base_priority and hint.
    assert_eq!(cfg.resolve("slot_a", &PriorityHint::Normal), 999);
    // base_priority wins over hint when no name override exists.
    assert_eq!(cfg.resolve("slot_b", &PriorityHint::Normal), 500);
    // hint used when neither override nor base_priority is set.
    assert_eq!(
        PluginInstallConfig::default().resolve("slot_c", &PriorityHint::High),
        750
    );
}
