use dioxus_extism_host::{TransformEntry, TransformRegistry};
use dioxus_extism_protocol::{PluginId, RoutePattern, TransformOp};

fn entry(plugin: &str, priority: i32, op: TransformOp) -> TransformEntry {
    TransformEntry {
        plugin_id: PluginId(plugin.into()),
        transform_fn: "fn".into(),
        op,
        priority,
        route_pattern: None,
    }
}

#[test]
fn component_transforms_sorted_priority_desc() {
    let mut reg = TransformRegistry::default();
    for p in [100, 750, 500] {
        reg.insert_component("Hero", entry("p", p, TransformOp::WrapNode));
    }
    let results = reg.for_component("Hero");
    assert_eq!(results[0].priority, 750);
    assert_eq!(results[1].priority, 500);
    assert_eq!(results[2].priority, 100);
}

#[test]
fn unknown_component_returns_empty() {
    let reg = TransformRegistry::default();
    assert!(reg.for_component("Unknown").is_empty());
}

#[test]
fn route_matches_pattern() {
    let mut reg = TransformRegistry::default();
    reg.insert_route(
        RoutePattern("/product/:id".into()),
        entry("p", 500, TransformOp::InjectAfter),
    );
    assert_eq!(reg.for_route("/product/42").len(), 1);
    assert!(reg.for_route("/other").is_empty());
}

#[test]
fn insert_route_sets_route_pattern() {
    let mut reg = TransformRegistry::default();
    reg.insert_route(
        RoutePattern("/product/:id".into()),
        entry("p", 500, TransformOp::Wrap),
    );
    let entries = reg.for_route("/product/42");
    assert_eq!(entries[0].route_pattern.as_deref(), Some("/product/:id"));
}

#[test]
fn slot_transforms_sorted_priority_desc() {
    let mut reg = TransformRegistry::default();
    reg.insert_slot("sidebar", entry("a", 100, TransformOp::InjectBefore));
    reg.insert_slot("sidebar", entry("b", 900, TransformOp::InjectAfter));
    let results = reg.for_slot("sidebar");
    assert_eq!(results[0].priority, 900);
    assert_eq!(results[1].priority, 100);
}

#[test]
fn all_component_names_returns_registered() {
    let mut reg = TransformRegistry::default();
    reg.insert_component("Hero", entry("p", 500, TransformOp::Replace));
    reg.insert_component("Sidebar", entry("p", 500, TransformOp::Replace));
    let names = reg.all_component_names();
    assert!(names.contains("Hero"));
    assert!(names.contains("Sidebar"));
}

#[test]
fn for_route_two_overlapping_patterns_both_returned_priority_sorted() {
    let mut reg = TransformRegistry::default();
    reg.insert_route(RoutePattern("/products/:id".into()), entry("p1", 750, TransformOp::Wrap));
    reg.insert_route(
        RoutePattern("/:category/:id".into()),
        entry("p2", 500, TransformOp::InjectAfter),
    );
    let entries = reg.for_route("/products/42");
    assert_eq!(entries.len(), 2);
    assert!(entries[0].priority >= entries[1].priority);
}

#[test]
fn insert_within_round_trip_isolation() {
    use dioxus_extism_protocol::{NodeSelector, Selector};

    let mut reg = TransformRegistry::default();
    let node_sel = NodeSelector::HasClass("target".into());
    let e = entry("p", 500, TransformOp::WrapNode);
    reg.insert_within(Selector::Slot("sidebar".into()), node_sel, e);

    assert_eq!(reg.within_for_outer(&Selector::Slot("sidebar".into())).len(), 1);
    assert!(reg.within_for_outer(&Selector::Slot("header".into())).is_empty());
}
