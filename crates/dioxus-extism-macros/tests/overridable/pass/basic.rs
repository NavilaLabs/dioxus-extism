use dioxus::prelude::*;
use dioxus_extism_macros::overridable;

#[overridable]
#[component]
fn MyComponent(title: String, count: i64) -> Element {
    rsx! { div { "{title}: {count}" } }
}

fn main() {}
