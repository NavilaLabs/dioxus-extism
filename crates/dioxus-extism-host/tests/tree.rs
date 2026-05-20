use dioxus_extism_host::tree::{find_recursive_matching, find_shallow_matching, node_matches};
use dioxus_extism_protocol::{AttrValue, NodeSelector, PluginView, ViewElement};

fn make_tree() -> PluginView {
    PluginView::Element(ViewElement {
        tag: "div".into(),
        children: vec![
            PluginView::Element(ViewElement {
                tag: "span".into(),
                attrs: vec![("class".into(), AttrValue::String("badge important".into()))],
                name: Some("the-badge".into()),
                children: vec![PluginView::Text("42".into())],
                ..Default::default()
            }),
            PluginView::Element(ViewElement {
                tag: "button".into(),
                attrs: vec![("data-plugin-slot".into(), AttrValue::String("actions".into()))],
                ..Default::default()
            }),
        ],
        ..Default::default()
    })
}

#[test]
fn has_class_shallow_match() {
    let tree = make_tree();
    let m = find_shallow_matching(&tree, &NodeSelector::HasClass("badge".into()));
    assert_eq!(m.len(), 1);
}

#[test]
fn data_attr_shallow_match() {
    let tree = make_tree();
    let m = find_shallow_matching(
        &tree,
        &NodeSelector::DataAttr("data-plugin-slot".into(), "actions".into()),
    );
    assert_eq!(m.len(), 1);
}

#[test]
fn name_shallow_match() {
    let tree = make_tree();
    let m = find_shallow_matching(&tree, &NodeSelector::Name("the-badge".into()));
    assert_eq!(m.len(), 1);
}

#[test]
fn shallow_does_not_find_deep() {
    let outer = PluginView::Element(ViewElement {
        tag: "section".into(),
        children: vec![make_tree()],
        ..Default::default()
    });
    // Shallow search from outer's direct children (= make_tree() root div)
    // only tests the div, not its descendants.
    let m = find_shallow_matching(&outer, &NodeSelector::HasClass("badge".into()));
    assert_eq!(m.len(), 0, "shallow must not descend into grandchildren");
}

#[test]
fn recursive_finds_deep() {
    let outer = PluginView::Element(ViewElement {
        tag: "section".into(),
        children: vec![make_tree()],
        ..Default::default()
    });
    let m = find_recursive_matching(
        &outer,
        &NodeSelector::Recursive(Box::new(NodeSelector::HasClass("badge".into()))),
    );
    assert_eq!(m.len(), 1);
}

#[test]
fn node_matches_tag() {
    let el = PluginView::Element(ViewElement {
        tag: "button".into(),
        ..Default::default()
    });
    assert!(node_matches(&el, &NodeSelector::Tag("button".into())));
    assert!(!node_matches(&el, &NodeSelector::Tag("div".into())));
}

#[test]
fn node_matches_recursive_delegates_to_inner() {
    // Recursive(inner) on node_matches must delegate to inner WITHOUT recursing.
    let el = PluginView::Element(ViewElement {
        tag: "span".into(),
        attrs: vec![("class".into(), AttrValue::String("badge".into()))],
        ..Default::default()
    });
    let recursive_sel = NodeSelector::Recursive(Box::new(NodeSelector::HasClass("badge".into())));
    // node_matches should return true: it delegates to inner (HasClass)
    assert!(node_matches(&el, &recursive_sel));

    // A non-matching node should return false through the delegation
    let other = PluginView::Element(ViewElement { tag: "div".into(), ..Default::default() });
    assert!(!node_matches(&other, &recursive_sel));
}

#[test]
fn find_shallow_first_last() {
    let tree = make_tree();
    let first = find_shallow_matching(&tree, &NodeSelector::First);
    let last = find_shallow_matching(&tree, &NodeSelector::Last);
    assert_eq!(first.len(), 1);
    assert_eq!(last.len(), 1);
    // first is the span, last is the button
    assert!(matches!(first[0], PluginView::Element(e) if e.tag == "span"));
    assert!(matches!(last[0], PluginView::Element(e) if e.tag == "button"));
}

#[test]
fn find_shallow_index() {
    let tree = make_tree();
    let at_1 = find_shallow_matching(&tree, &NodeSelector::Index(1));
    assert_eq!(at_1.len(), 1);
    assert!(matches!(at_1[0], PluginView::Element(e) if e.tag == "button"));
    // Out-of-range
    let out_of_range = find_shallow_matching(&tree, &NodeSelector::Index(99));
    assert!(out_of_range.is_empty());
}
