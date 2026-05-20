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
            let wasm_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("host dir has parent")
                .parent()
                .expect("example dir has parent")
                .parent()
                .expect("examples dir has parent")
                .join("target/wasm32-unknown-unknown/release/hello_plugin_plugin.wasm");

            let mut builder = PluginRuntimeBuilder::new();
            if wasm_path.exists() {
                builder = builder.add_plugin(PluginSource::File(wasm_path));
                tracing::info!("hello-plugin loaded");
            } else {
                tracing::warn!("plugin WASM not found — hello-slot will be empty");
            }

            let runtime = builder.build().await.expect("plugin runtime build");

            let addr = dioxus::cli_config::fullstack_address_or_localhost();
            let router = axum::Router::new()
                .serve_dioxus_application(dioxus::server::ServeConfig::new(), App)
                .layer(axum::Extension(runtime));

            tracing::info!("listening on {addr}");
            let listener = tokio::net::TcpListener::bind(addr).await
                .unwrap_or_else(|e| panic!("bind {addr}: {e} — set IP / PORT env vars (e.g. IP=0.0.0.0 PORT=8081)"));
            axum::serve(listener, router).await.expect("serve");
        });
}

/// Root app — session context + plugin boot, then the page.
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
