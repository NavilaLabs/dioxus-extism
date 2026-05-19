use dioxus::prelude::*;
use dioxus_extism_frontend::{
    PluginBootProvider, PluginSlot, SessionProviderRoot, WebSessionProvider,
};

fn main() {
    dioxus::LaunchBuilder::new().launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        SessionProviderRoot { provider: WebSessionProvider,
            PluginBootProvider {
                div {
                    h1 { "Hello Plugin Example" }
                    PluginSlot { name: "hello-slot" }
                }
            }
        }
    }
}
