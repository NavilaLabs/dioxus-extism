use dioxus::prelude::*;
use dioxus_extism_macros::overridable;

#[overridable]
#[component]
fn BadComponent(items: impl Iterator<Item = String>) -> Element {
    rsx! { div { "bad" } }
}

fn main() {}
