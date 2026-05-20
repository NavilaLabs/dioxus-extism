use dioxus_extism_protocol::{AttrValue, NodeSelector, PluginView, ViewElement};

/// Returns all direct children of `view` that match `selector`.
///
/// Position-based selectors (`First`, `Last`, `Index`) are resolved against the
/// parent's child list and never recurse. All other selectors use [`node_matches`].
#[must_use]
pub fn find_shallow_matching<'a>(
    view: &'a PluginView,
    selector: &NodeSelector,
) -> Vec<&'a PluginView> {
    let children: &[PluginView] = match view {
        PluginView::Element(el) => el.children.as_slice(),
        PluginView::Fragment(v) => v.as_slice(),
        _ => return vec![],
    };
    match selector {
        NodeSelector::First => children.first().into_iter().collect(),
        NodeSelector::Last => children.last().into_iter().collect(),
        NodeSelector::Index(i) => children.get(*i).into_iter().collect(),
        _ => children.iter().filter(|c| node_matches(c, selector)).collect(),
    }
}

/// Returns all nodes at any depth in `view` that match `selector`.
///
/// Recursion is explicit here; [`node_matches`] itself never descends into children.
#[must_use]
pub fn find_recursive_matching<'a>(
    view: &'a PluginView,
    selector: &NodeSelector,
) -> Vec<&'a PluginView> {
    let mut out = vec![];
    collect_recursive(view, selector, &mut out);
    out
}

fn collect_recursive<'a>(view: &'a PluginView, selector: &NodeSelector, out: &mut Vec<&'a PluginView>) {
    if node_matches(view, selector) {
        out.push(view);
    }
    match view {
        PluginView::Element(el) => {
            for child in &el.children {
                collect_recursive(child, selector, out);
            }
        }
        PluginView::Fragment(children) => {
            for child in children {
                collect_recursive(child, selector, out);
            }
        }
        _ => {}
    }
}

/// Returns `true` if `view` matches `selector`.
///
/// `NodeSelector::Recursive(inner)` delegates to `inner` without descending — the caller
/// is responsible for recursion. `First`, `Last`, and `Index` always return `false` here
/// because they require parent context; use [`find_shallow_matching`] for those.
#[must_use]
pub fn node_matches(view: &PluginView, selector: &NodeSelector) -> bool {
    match selector {
        NodeSelector::Tag(t) => matches!(view, PluginView::Element(el) if &el.tag == t),
        NodeSelector::HasClass(cls) => {
            if let PluginView::Element(el) = view {
                el.attrs.iter().any(|(k, v)| {
                    k == "class"
                        && matches!(v, AttrValue::String(s) if s.split_whitespace().any(|c| c == cls.as_str()))
                })
            } else {
                false
            }
        }
        NodeSelector::Name(n) => {
            matches!(view, PluginView::Element(el) if el.name.as_deref() == Some(n.as_str()))
        }
        NodeSelector::DataAttr(k, v) => {
            if let PluginView::Element(el) = view {
                el.attrs.iter().any(|(ak, av)| {
                    ak == k && matches!(av, AttrValue::String(s) if s == v)
                })
            } else {
                false
            }
        }
        NodeSelector::HostComponent(n) => {
            matches!(view, PluginView::HostComponent(r) if &r.name == n)
        }
        NodeSelector::And(a, b) => node_matches(view, a) && node_matches(view, b),
        NodeSelector::Or(a, b) => node_matches(view, a) || node_matches(view, b),
        // Delegates to inner without recursing; caller controls depth.
        NodeSelector::Recursive(inner) => node_matches(view, inner),
        _ => false,
    }
}

/// Returns `true` if `selector` is `NodeSelector::Recursive`, signalling that
/// `traverse_and_apply` should descend into non-matching children.
#[must_use]
pub(crate) const fn is_recursive_selector(selector: &NodeSelector) -> bool {
    matches!(selector, NodeSelector::Recursive(_))
}

/// Add a CSS class to the element's `class` attribute (or create it).
/// Non-element views are returned unchanged.
pub(crate) fn add_class_to_view(view: PluginView, cls: String) -> PluginView {
    if let PluginView::Element(mut el) = view {
        if let Some((_, AttrValue::String(existing))) =
            el.attrs.iter_mut().find(|(k, _)| k == "class")
        {
            existing.push(' ');
            existing.push_str(&cls);
        } else {
            el.attrs.push(("class".into(), AttrValue::String(cls)));
        }
        PluginView::Element(el)
    } else {
        view
    }
}

/// Set an attribute on the element (overwrite if already present).
/// Non-element views are returned unchanged.
pub(crate) fn set_attr_on_view(view: PluginView, key: String, value: AttrValue) -> PluginView {
    if let PluginView::Element(mut el) = view {
        if let Some(pair) = el.attrs.iter_mut().find(|(k, _)| k == &key) {
            pair.1 = value;
        } else {
            el.attrs.push((key, value));
        }
        PluginView::Element(el)
    } else {
        view
    }
}

/// Resolve `HostComponent("__target__")` placeholders in `wrapper` with `target`.
pub(crate) fn resolve_target_in_view(wrapper: PluginView, target: PluginView) -> PluginView {
    match wrapper {
        PluginView::HostComponent(r) if r.name == "__target__" => target,
        PluginView::Element(el) => {
            let new_children = el
                .children
                .into_iter()
                .map(|c| resolve_target_in_view(c, target.clone()))
                .collect();
            PluginView::Element(ViewElement { children: new_children, ..el })
        }
        PluginView::Fragment(children) => PluginView::Fragment(
            children
                .into_iter()
                .map(|c| resolve_target_in_view(c, target.clone()))
                .collect(),
        ),
        other => other,
    }
}
