use dioxus_extism_protocol::{AttrValue, BoundEventHandler, HostComponentRef, PluginView, ViewElement};

/// Fluent builder for `PluginView::Element`.
pub struct ViewBuilder(ViewElement);

impl ViewBuilder {
    pub fn new(tag: impl Into<String>) -> Self {
        Self(ViewElement {
            tag: tag.into(),
            ..Default::default()
        })
    }

    #[must_use]
    pub fn attr(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.attrs.push((name.into(), AttrValue::String(value.into())));
        self
    }

    #[must_use]
    pub fn class(self, value: impl Into<String>) -> Self {
        self.attr("class", value)
    }

    #[must_use]
    pub fn id(self, value: impl Into<String>) -> Self {
        self.attr("id", value)
    }

    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.0.name = Some(name.into());
        self
    }

    #[must_use]
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.0.key = Some(key.into());
        self
    }

    #[must_use]
    pub fn child(mut self, child: PluginView) -> Self {
        self.0.children.push(child);
        self
    }

    #[must_use]
    pub fn children(mut self, children: impl IntoIterator<Item = PluginView>) -> Self {
        self.0.children.extend(children);
        self
    }

    #[must_use]
    pub fn on(mut self, handler: BoundEventHandler) -> Self {
        self.0.handlers.push(handler);
        self
    }

    /// Finalise into a `PluginView::Element`.
    pub fn build(self) -> PluginView {
        PluginView::Element(self.0)
    }
}

// ── Convenience constructors ─────────────────────────────────────────────────

pub fn element(tag: impl Into<String>) -> ViewBuilder {
    ViewBuilder::new(tag)
}

pub fn div() -> ViewBuilder {
    ViewBuilder::new("div")
}

pub fn span() -> ViewBuilder {
    ViewBuilder::new("span")
}

pub fn text(content: impl Into<String>) -> PluginView {
    PluginView::Text(content.into())
}

/// Returns `PluginView::Incompatible` with a reason string.
pub fn incompatible(reason: impl Into<String>) -> PluginView {
    PluginView::Incompatible {
        reason: reason.into(),
        fallback: None,
    }
}

/// Returns a `HostComponent("__content__")` placeholder — used inside Wrap transforms
/// to reference the original content.
pub fn original_content() -> PluginView {
    PluginView::HostComponent(HostComponentRef {
        name: "__content__".into(),
        props: serde_json::Value::Null,
        children: vec![],
    })
}
