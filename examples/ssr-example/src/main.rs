//! SSR example — demonstrates server-side rendering with plugin content.
//!
//! This example shows the correct pattern for rendering a page to HTML on the server
//! when `dioxus-extism` plugins may contribute slot content or route transforms.
//!
//! The key difference from a standard Dioxus SSR render:
//!
//! 1. Call `PluginRuntime::ssr_render_route()` BEFORE rendering to HTML.
//!    This pre-fetches all plugin slot content for the route in a single async pass.
//!
//! 2. Wrap the page component in `SsrPluginDataProvider { output: ssr_data, ... }`.
//!    Child components use `PluginSlotSsr` instead of `PluginSlot` to read from this
//!    pre-fetched data rather than making new server function calls (which do not exist
//!    in a synchronous SSR context).
//!
//! 3. Call `dioxus_ssr::render_element(app_element)` to produce the final HTML string.
//!    This is synchronous — all async plugin calls must complete before this step.

use dioxus::prelude::*;
use dioxus_extism_frontend::{PluginSlotSsr, SsrPluginDataProvider};
use dioxus_extism_host::{PluginRuntimeBuilder, PluginSource};
use dioxus_extism_protocol::{ClientCapabilities, SessionCtx, SessionId, PROTOCOL_VERSION};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // ── 1. Build the plugin runtime ──────────────────────────────────────────
    //
    // In a real application this would load actual WASM plugins. Here we load
    // the hello-plugin binary if it has been compiled; otherwise the runtime
    // starts empty and all slots render nothing.
    let wasm_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("ssr-example dir has parent")
        .join("hello-plugin/plugin")
        .join("../../target/wasm32-unknown-unknown/release/hello_plugin_plugin.wasm");

    let mut builder = PluginRuntimeBuilder::new();
    if wasm_path.exists() {
        builder = builder.add_plugin(PluginSource::File(wasm_path));
        tracing::info!("hello-plugin loaded");
    } else {
        tracing::info!(
            "Plugin WASM not found — running with empty runtime. \
             Build with: cargo build --target wasm32-unknown-unknown -p hello-plugin-plugin --release"
        );
    }
    let runtime = builder.build().await.expect("plugin runtime build");

    // ── 2. Build a minimal session for SSR ───────────────────────────────────
    //
    // SSR sessions don't have a real user — they use a synthetic session ID and
    // ClientCapabilities::default_ssr(). The rendered HTML is then hydrated on
    // the client, which substitutes its real session ID.
    let _client = ClientCapabilities::default_ssr();
    let session = SessionCtx {
        session_id: SessionId("ssr-synthetic".into()),
        user_id: None,
        client: ClientCapabilities {
            protocol_version: PROTOCOL_VERSION,
            app_version: 0,
            registered_host_components: vec![],
        },
        caller: None,
    };

    // ── 3. Pre-fetch all plugin content for the route ────────────────────────
    //
    // This single async call collects slot content, component resolutions, and
    // route transforms for "/" in one pass. The result is a plain data value
    // that can be passed into the synchronous SSR render step below.
    let ssr_data = runtime
        .ssr_render_route("/", &session)
        .await
        .expect("ssr_render_route failed");

    // ── 4. Render to HTML ────────────────────────────────────────────────────
    //
    // Wrap the page component in SsrPluginDataProvider so child components can
    // call PluginSlotSsr and read from the pre-fetched data without network calls.
    let html = dioxus_ssr::render_element(rsx! {
        SsrPluginDataProvider { output: ssr_data,
            SsrPage {}
        }
    });

    println!("<!DOCTYPE html>\n<html><body>{html}</body></html>");
}

/// A simple page that renders a plugin slot.
///
/// Uses `PluginSlotSsr` instead of `PluginSlot` so no server function calls
/// are made during the synchronous SSR render pass.
#[component]
fn SsrPage() -> Element {
    rsx! {
        div { class: "ssr-page",
            h1 { "SSR Example" }
            p { "Content below comes from a plugin slot (empty if no WASM loaded):" }
            div { class: "slot-container",
                PluginSlotSsr { name: "hello-slot" }
            }
        }
    }
}
