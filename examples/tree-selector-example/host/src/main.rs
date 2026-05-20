use dioxus::prelude::*;
use dioxus_extism_frontend::{
    PluginBootProvider, PluginSlot, SessionProviderRoot, WebSessionProvider,
};

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    server_main();

    #[cfg(target_arch = "wasm32")]
    dioxus::launch(App);
}

#[cfg(not(target_arch = "wasm32"))]
fn server_main() {
    use std::path::PathBuf;

    use dioxus::server::DioxusRouterExt;
    use dioxus_extism_host::{PluginRuntimeBuilder, PluginSource};

    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(async move {
            let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("host has parent")
                .parent()
                .expect("example has parent")
                .parent()
                .expect("examples has parent")
                .join("target/wasm32-unknown-unknown/release");

            let wasm_a = base.join("tree_selector_example_plugin_a.wasm");
            let wasm_b = base.join("tree_selector_example_plugin_b.wasm");

            let mut builder = PluginRuntimeBuilder::new();
            if wasm_a.exists() {
                builder = builder.add_plugin(PluginSource::File(wasm_a));
                tracing::info!("tree-selector plugin_a loaded");
            } else {
                tracing::warn!("plugin_a WASM not found — activity-feed slot will be empty");
            }
            if wasm_b.exists() {
                builder = builder.add_plugin(PluginSource::File(wasm_b));
                tracing::info!("tree-selector plugin_b loaded");
            } else {
                tracing::warn!("plugin_b WASM not found — Share button will not appear");
            }

            let runtime = builder.build().await.expect("plugin runtime build");

            let addr = dioxus::cli_config::fullstack_address_or_localhost();
            let router = axum::Router::new()
                .serve_dioxus_application(dioxus::server::ServeConfig::new(), App)
                .layer(axum::Extension(runtime));

            tracing::info!("listening on {addr}");
            let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
            axum::serve(listener, router).await.expect("serve");
        });
}

/// Root app — session + plugin boot, then the page.
#[component]
fn App() -> Element {
    rsx! {
        SessionProviderRoot { provider: WebSessionProvider,
            PluginBootProvider {
                FeedPage {}
            }
        }
    }
}

/// Feed page — no plugin-related code.
///
/// Uses `PluginSlot` to render the "activity-feed" slot. Both `plugin_a`
/// (which provides the slot content) and `plugin_b` (which injects into
/// `plugin_a`'s output via a Within transform) are completely invisible here.
#[component]
fn FeedPage() -> Element {
    rsx! {
        div {
            h1 { "Tree Selector Example" }
            p { "The activity feed below is composed from two plugins with zero host involvement." }
            p { "plugin_a provides the feed card. plugin_b injects a Share button inside it." }
            PluginSlot { name: "activity-feed" }
        }
    }
}
