use dioxus_extism_protocol::{
    AttrValue, BoundEventHandler, DomEvent, HandlerId, HostComponentRef, PluginView, ViewElement,
};

/// Fluent builder for `PluginView::Element`.
pub struct ViewBuilder(ViewElement);

impl ViewBuilder {
    /// Start a new element with the given HTML tag.
    pub fn new(tag: impl Into<String>) -> Self {
        Self(ViewElement {
            tag: tag.into(),
            ..Default::default()
        })
    }

    /// Set a string attribute.
    #[must_use]
    pub fn attr(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.attrs.push((name.into(), AttrValue::String(value.into())));
        self
    }

    /// Set a boolean attribute (e.g. `disabled`, `hidden`).
    #[must_use]
    pub fn bool_attr(mut self, name: impl Into<String>, value: bool) -> Self {
        self.0.attrs.push((name.into(), AttrValue::Bool(value)));
        self
    }

    /// Set a numeric attribute.
    #[must_use]
    pub fn num_attr(mut self, name: impl Into<String>, value: f64) -> Self {
        self.0.attrs.push((name.into(), AttrValue::Number(value)));
        self
    }

    /// Set the `class` attribute.
    #[must_use]
    pub fn class(self, value: impl Into<String>) -> Self {
        self.attr("class", value)
    }

    /// Set the `id` attribute.
    #[must_use]
    pub fn id(self, value: impl Into<String>) -> Self {
        self.attr("id", value)
    }

    /// Set the `style` attribute.
    #[must_use]
    pub fn style(self, value: impl Into<String>) -> Self {
        self.attr("style", value)
    }

    /// Set the `href` attribute (for `<a>` elements).
    #[must_use]
    pub fn href(self, value: impl Into<String>) -> Self {
        self.attr("href", value)
    }

    /// Set the `src` attribute (for `<img>`, `<script>`, etc.).
    #[must_use]
    pub fn src(self, value: impl Into<String>) -> Self {
        self.attr("src", value)
    }

    /// Set the `alt` attribute (for `<img>` elements).
    #[must_use]
    pub fn alt(self, value: impl Into<String>) -> Self {
        self.attr("alt", value)
    }

    /// Set the `type` attribute (for `<input>`, `<button>`, etc.).
    #[must_use]
    pub fn ty(self, value: impl Into<String>) -> Self {
        self.attr("type", value)
    }

    /// Set the `value` attribute (for `<input>` elements).
    #[must_use]
    pub fn value(self, v: impl Into<String>) -> Self {
        self.attr("value", v)
    }

    /// Set the `placeholder` attribute (for `<input>` elements).
    #[must_use]
    pub fn placeholder(self, value: impl Into<String>) -> Self {
        self.attr("placeholder", value)
    }

    /// Set the `role` attribute.
    #[must_use]
    pub fn role(self, value: impl Into<String>) -> Self {
        self.attr("role", value)
    }

    /// Set the `disabled` attribute.
    #[must_use]
    pub fn disabled(self, value: bool) -> Self {
        self.bool_attr("disabled", value)
    }

    /// Set the `hidden` attribute.
    #[must_use]
    pub fn hidden(self, value: bool) -> Self {
        self.bool_attr("hidden", value)
    }

    /// Set a `data-*` attribute. Prepends `data-` automatically.
    #[must_use]
    pub fn data(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.attr(format!("data-{}", name.into()), value)
    }

    /// Mark this element as a `data-plugin-slot` target, making it addressable
    /// by `Selector::DataPluginSlot` tree transforms.
    #[must_use]
    pub fn plugin_slot(self, slot_name: impl Into<String>) -> Self {
        self.attr("data-plugin-slot", slot_name)
    }

    /// Set the stable selector name (targets `NodeSelector::Name`).
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.0.name = Some(name.into());
        self
    }

    /// Set a stable diff key. Forwarded as a Dioxus RSX `key` attribute.
    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.0.key = Some(key.into());
        self
    }

    /// Append a child view.
    #[must_use]
    pub fn child(mut self, child: PluginView) -> Self {
        self.0.children.push(child);
        self
    }

    /// Append multiple child views.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = PluginView>) -> Self {
        self.0.children.extend(children);
        self
    }

    /// Attach a pre-built event handler.
    #[must_use]
    pub fn on(mut self, handler: BoundEventHandler) -> Self {
        self.0.handlers.push(handler);
        self
    }

    /// Attach a `click` handler.
    #[must_use]
    pub fn on_click(self, handler_id: HandlerId) -> Self {
        self.on(BoundEventHandler { event: DomEvent::Click, handler_id, debounce_ms: None })
    }

    /// Attach an `input` handler.
    #[must_use]
    pub fn on_input(self, handler_id: HandlerId) -> Self {
        self.on(BoundEventHandler { event: DomEvent::Input, handler_id, debounce_ms: None })
    }

    /// Attach a `change` handler.
    #[must_use]
    pub fn on_change(self, handler_id: HandlerId) -> Self {
        self.on(BoundEventHandler { event: DomEvent::Change, handler_id, debounce_ms: None })
    }

    /// Attach a `submit` handler.
    #[must_use]
    pub fn on_submit(self, handler_id: HandlerId) -> Self {
        self.on(BoundEventHandler { event: DomEvent::Submit, handler_id, debounce_ms: None })
    }

    /// Attach a `focus` handler.
    #[must_use]
    pub fn on_focus(self, handler_id: HandlerId) -> Self {
        self.on(BoundEventHandler { event: DomEvent::Focus, handler_id, debounce_ms: None })
    }

    /// Attach a `blur` handler.
    #[must_use]
    pub fn on_blur(self, handler_id: HandlerId) -> Self {
        self.on(BoundEventHandler { event: DomEvent::Blur, handler_id, debounce_ms: None })
    }

    /// Attach a `keydown` handler.
    #[must_use]
    pub fn on_keydown(self, handler_id: HandlerId) -> Self {
        self.on(BoundEventHandler { event: DomEvent::KeyDown, handler_id, debounce_ms: None })
    }

    /// Attach a `keyup` handler.
    #[must_use]
    pub fn on_keyup(self, handler_id: HandlerId) -> Self {
        self.on(BoundEventHandler { event: DomEvent::KeyUp, handler_id, debounce_ms: None })
    }

    /// Set a debounce delay (ms) on the most recently attached handler.
    /// Has no effect if no handler has been attached yet.
    #[must_use]
    pub fn debounce(mut self, ms: u32) -> Self {
        if let Some(last) = self.0.handlers.last_mut() {
            last.debounce_ms = Some(ms);
        }
        self
    }

    /// Finalise into a `PluginView::Element`.
    #[must_use]
    pub fn build(self) -> PluginView {
        PluginView::Element(self.0)
    }
}

// ── Element constructors ──────────────────────────────────────────────────────

/// Start building an element with the given tag.
pub fn element(tag: impl Into<String>) -> ViewBuilder {
    ViewBuilder::new(tag)
}

/// Start building a `<div>`.
#[must_use]
pub fn div() -> ViewBuilder { ViewBuilder::new("div") }

/// Start building a `<span>`.
#[must_use]
pub fn span() -> ViewBuilder { ViewBuilder::new("span") }

/// Start building a `<p>`.
#[must_use]
pub fn p() -> ViewBuilder { ViewBuilder::new("p") }

/// Start building an `<h1>`.
#[must_use]
pub fn h1() -> ViewBuilder { ViewBuilder::new("h1") }

/// Start building an `<h2>`.
#[must_use]
pub fn h2() -> ViewBuilder { ViewBuilder::new("h2") }

/// Start building an `<h3>`.
#[must_use]
pub fn h3() -> ViewBuilder { ViewBuilder::new("h3") }

/// Start building a `<button>`.
#[must_use]
pub fn button() -> ViewBuilder { ViewBuilder::new("button") }

/// Start building an `<input>`.
#[must_use]
pub fn input() -> ViewBuilder { ViewBuilder::new("input") }

/// Start building a `<label>`.
#[must_use]
pub fn label() -> ViewBuilder { ViewBuilder::new("label") }

/// Start building an `<a>`.
#[must_use]
pub fn a() -> ViewBuilder { ViewBuilder::new("a") }

/// Start building an `<img>`.
#[must_use]
pub fn img() -> ViewBuilder { ViewBuilder::new("img") }

/// Start building a `<ul>`.
#[must_use]
pub fn ul() -> ViewBuilder { ViewBuilder::new("ul") }

/// Start building an `<ol>`.
#[must_use]
pub fn ol() -> ViewBuilder { ViewBuilder::new("ol") }

/// Start building an `<li>`.
#[must_use]
pub fn li() -> ViewBuilder { ViewBuilder::new("li") }

/// Start building a `<form>`.
#[must_use]
pub fn form() -> ViewBuilder { ViewBuilder::new("form") }

/// Start building an `<h4>`.
#[must_use]
pub fn h4() -> ViewBuilder { ViewBuilder::new("h4") }

/// Start building an `<h5>`.
#[must_use]
pub fn h5() -> ViewBuilder { ViewBuilder::new("h5") }

/// Start building an `<h6>`.
#[must_use]
pub fn h6() -> ViewBuilder { ViewBuilder::new("h6") }

/// Start building a `<section>`.
#[must_use]
pub fn section() -> ViewBuilder { ViewBuilder::new("section") }

/// Start building a `<header>`.
#[must_use]
pub fn header() -> ViewBuilder { ViewBuilder::new("header") }

/// Start building a `<footer>`.
#[must_use]
pub fn footer() -> ViewBuilder { ViewBuilder::new("footer") }

/// Start building a `<nav>`.
#[must_use]
pub fn nav() -> ViewBuilder { ViewBuilder::new("nav") }

/// Start building an `<article>`.
#[must_use]
pub fn article() -> ViewBuilder { ViewBuilder::new("article") }

/// Start building an `<aside>`.
#[must_use]
pub fn aside() -> ViewBuilder { ViewBuilder::new("aside") }

/// Start building a `<table>`.
#[must_use]
pub fn table() -> ViewBuilder { ViewBuilder::new("table") }

/// Start building a `<thead>`.
#[must_use]
pub fn thead() -> ViewBuilder { ViewBuilder::new("thead") }

/// Start building a `<tbody>`.
#[must_use]
pub fn tbody() -> ViewBuilder { ViewBuilder::new("tbody") }

/// Start building a `<tr>`.
#[must_use]
pub fn tr() -> ViewBuilder { ViewBuilder::new("tr") }

/// Start building a `<td>`.
#[must_use]
pub fn td() -> ViewBuilder { ViewBuilder::new("td") }

/// Start building a `<th>`.
#[must_use]
pub fn th() -> ViewBuilder { ViewBuilder::new("th") }

/// Start building a `<textarea>`.
#[must_use]
pub fn textarea() -> ViewBuilder { ViewBuilder::new("textarea") }

/// Start building a `<select>`.
#[must_use]
pub fn select() -> ViewBuilder { ViewBuilder::new("select") }

/// Start building an `<option>`.
#[must_use]
pub fn option() -> ViewBuilder { ViewBuilder::new("option") }

// ── Non-element view constructors ────────────────────────────────────────────

/// Wrap a string as a `PluginView::Text` node.
pub fn text(content: impl Into<String>) -> PluginView {
    PluginView::Text(content.into())
}

/// Collect multiple views into a `PluginView::Fragment`.
pub fn fragment(children: impl IntoIterator<Item = PluginView>) -> PluginView {
    PluginView::Fragment(children.into_iter().collect())
}

/// Return `PluginView::Incompatible` with the given reason.
/// Use this when a plugin cannot render for the current client version.
pub fn incompatible(reason: impl Into<String>) -> PluginView {
    PluginView::Incompatible { reason: reason.into(), fallback: None }
}

/// Return `PluginView::Incompatible` with a reason and a simpler fallback view.
pub fn incompatible_with_fallback(reason: impl Into<String>, fallback: PluginView) -> PluginView {
    PluginView::Incompatible {
        reason: reason.into(),
        fallback: Some(Box::new(fallback)),
    }
}

/// Returns a `HostComponent("__content__")` placeholder — used inside `Wrap`
/// transforms to include the original content in the plugin's output.
#[must_use]
pub fn original_content() -> PluginView {
    PluginView::HostComponent(HostComponentRef {
        name: "__content__".into(),
        props: serde_json::Value::Null,
        children: vec![],
    })
}

/// Returns a `HostComponent("__target__")` placeholder — used inside `WrapNode`
/// transforms to include the original node in the plugin's output.
#[must_use]
pub fn original_target() -> PluginView {
    PluginView::HostComponent(HostComponentRef {
        name: "__target__".into(),
        props: serde_json::Value::Null,
        children: vec![],
    })
}

// ── Host component builder ────────────────────────────────────────────────────

/// Fluent builder for `PluginView::HostComponent`.
///
/// Allows plugin views to embed host-registered components by name.
/// The host application must register the component via `register_host_component`.
///
/// # Example
/// ```ignore
/// host("UserAvatar")
///     .props(serde_json::json!({ "user_id": 42 }))
///     .build()
/// ```
pub struct HostComponentBuilder(HostComponentRef);

/// Start building a reference to a named host component.
pub fn host(name: impl Into<String>) -> HostComponentBuilder {
    HostComponentBuilder(HostComponentRef {
        name: name.into(),
        props: serde_json::Value::Null,
        children: vec![],
    })
}

impl HostComponentBuilder {
    /// Set the props forwarded to the host component as a JSON value.
    #[must_use]
    pub fn props(mut self, props: serde_json::Value) -> Self {
        self.0.props = props;
        self
    }

    /// Append a child view passed to the host component.
    #[must_use]
    pub fn child(mut self, v: PluginView) -> Self {
        self.0.children.push(v);
        self
    }

    /// Append multiple child views.
    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = PluginView>) -> Self {
        self.0.children.extend(children);
        self
    }

    /// Finalise into a `PluginView::HostComponent`.
    #[must_use]
    pub fn build(self) -> PluginView {
        PluginView::HostComponent(self.0)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dioxus_extism_protocol::PluginView;

    #[test]
    fn builder_basic() {
        let view = div().class("foo").id("bar").build();
        let PluginView::Element(el) = view else { panic!("expected Element") };
        assert_eq!(el.tag, "div");
        assert!(el.attrs.iter().any(|(k, _)| k == "class"));
        assert!(el.attrs.iter().any(|(k, _)| k == "id"));
    }

    #[test]
    fn builder_event_handlers() {
        let hid = HandlerId("h1".into());
        let view = button()
            .on_click(hid.clone())
            .debounce(200)
            .build();
        let PluginView::Element(el) = view else { panic!("expected Element") };
        assert_eq!(el.handlers.len(), 1);
        assert_eq!(el.handlers[0].debounce_ms, Some(200));
    }

    #[test]
    fn builder_plugin_slot() {
        let view = div().plugin_slot("my-slot").build();
        let PluginView::Element(el) = view else { panic!("expected Element") };
        assert!(el.attrs.iter().any(|(k, v)| {
            k == "data-plugin-slot" && matches!(v, AttrValue::String(s) if s == "my-slot")
        }));
    }

    #[test]
    fn fragment_helper() {
        let f = fragment([text("a"), text("b")]);
        let PluginView::Fragment(items) = f else { panic!("expected Fragment") };
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn host_builder() {
        let view = host("MyCard")
            .props(serde_json::json!({ "id": 1 }))
            .build();
        let PluginView::HostComponent(r) = view else { panic!("expected HostComponent") };
        assert_eq!(r.name, "MyCard");
    }

    #[test]
    fn original_content_and_target() {
        let PluginView::HostComponent(r) = original_content() else { panic!() };
        assert_eq!(r.name, "__content__");
        let PluginView::HostComponent(r) = original_target() else { panic!() };
        assert_eq!(r.name, "__target__");
    }

    #[test]
    fn all_element_constructors_have_correct_tags() {
        let pairs: &[(&str, fn() -> ViewBuilder)] = &[
            ("p", p),
            ("h1", h1), ("h2", h2), ("h3", h3),
            ("h4", h4), ("h5", h5), ("h6", h6),
            ("button", button), ("input", input), ("label", label),
            ("a", a), ("img", img),
            ("ul", ul), ("ol", ol), ("li", li),
            ("form", form),
            ("section", section), ("header", header), ("footer", footer),
            ("nav", nav), ("article", article), ("aside", aside),
            ("table", table), ("thead", thead), ("tbody", tbody),
            ("tr", tr), ("td", td), ("th", th),
            ("textarea", textarea), ("select", select), ("option", option),
        ];
        for (tag, ctor) in pairs {
            let PluginView::Element(el) = ctor().build() else {
                panic!("expected Element for {tag}");
            };
            assert_eq!(&el.tag, tag);
        }
    }

    #[test]
    fn debounce_no_handler_is_noop() {
        // debounce with no handler attached should not panic
        let view = div().debounce(100).build();
        let PluginView::Element(el) = view else { panic!() };
        assert!(el.handlers.is_empty());
    }
}
