use dioxus::prelude::*;
use dioxus_extism_frontend::{
    PluginAwareRouter, PluginBootProvider, SessionProviderRoot, WebSessionProvider,
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
                .join(
                    "target/wasm32-unknown-unknown/release/route_injection_example_plugin.wasm",
                );

            let mut builder = PluginRuntimeBuilder::new();
            if wasm_path.exists() {
                builder = builder.add_plugin(PluginSource::File(wasm_path));
                tracing::info!("route-injection plugin loaded");
            } else {
                tracing::warn!("plugin WASM not found — running without route transforms");
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

#[derive(Debug, Clone, Routable, PartialEq)]
enum Route {
    #[layout(Layout)]
    #[route("/")]
    HomePage,
    #[route("/product/:id")]
    ProductPage { id: String },
}

/// Root app: session + plugin boot wrapping the router.
#[component]
fn App() -> Element {
    rsx! {
        SessionProviderRoot { provider: WebSessionProvider,
            PluginBootProvider {
                Router::<Route> {}
            }
        }
    }
}

/// Route layout: `PluginAwareRouter` checks for transforms on every navigation
/// and either wraps the outlet or renders it bare.
#[component]
fn Layout() -> Element {
    rsx! {
        PluginAwareRouter::<Route> {}
    }
}

/// Home page — no plugin code.
#[component]
fn HomePage() -> Element {
    rsx! {
        div {
            h1 { "Route Injection Example" }
            p { "Navigate to a product page to see the plugin in action." }
            Link { to: Route::ProductPage { id: "42".into() }, "Go to product 42" }
        }
    }
}

/// Product page — zero plugin-related code.
///
/// The plugin wraps this page and injects a banner below it, with this component
/// completely unaware of the plugin's existence.
#[component]
fn ProductPage(id: String) -> Element {
    rsx! {
        div {
            class: "product-page",
            h1 { "Product #{id}" }
            p { "This is the product detail page." }
            p { "The plugin wraps and injects around it without any code changes here." }
            Link { to: Route::HomePage, "← Back to home" }
        }
    }
}
