//! showcase-host — a blog platform demonstrating every dioxus-extism capability:
//!
//! **Two plugins:**
//! - `showcase/comments` — slot, API routes, page route, global/session state,
//!   interactions, event emission.
//! - `showcase/stats`    — slot, API route, page route, event subscription,
//!   hook handling, route transform, global state, interactions, `on_load`.
//!
//! **To run:**
//! ```
//! # Build both plugins first:
//! cargo build -p showcase-plugin-comments --release --target wasm32-unknown-unknown
//! cargo build -p showcase-plugin-stats    --release --target wasm32-unknown-unknown
//! # Then run the host:
//! cargo run -p showcase-host
//! ```

use dioxus::prelude::*;
use dioxus_extism_frontend::{
    PluginAwareRouter, PluginBootProvider, PluginPageOutlet, PluginViewRenderer,
    SessionProviderRoot, WebSessionProvider, use_session_id,
};
use dioxus_extism_protocol::{ClientCapabilities, PluginId, SessionId, SlotContent};
use serde::{Deserialize, Serialize};

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    server_main();

    #[cfg(target_arch = "wasm32")]
    dioxus::launch(App);
}

// ── Post data model ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub slug: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

fn sample_posts() -> Vec<Post> {
    vec![
        Post {
            slug: "rust-async".into(),
            title: "Rust Async Explained".into(),
            body: "Futures, async/await, and the Tokio runtime — from zero to production.".into(),
            tags: vec!["rust".into(), "async".into()],
        },
        Post {
            slug: "wasm-plugins".into(),
            title: "WASM Plugins with Extism".into(),
            body: "How to run sandboxed WebAssembly plugins safely in a Rust server.".into(),
            tags: vec!["wasm".into(), "plugins".into()],
        },
        Post {
            slug: "dioxus-tutorial".into(),
            title: "Building UIs with Dioxus".into(),
            body: "Full-stack Rust UIs: components, signals, server functions, and routing.".into(),
            tags: vec!["dioxus".into(), "ui".into(), "rust".into()],
        },
    ]
}

// ── Server-side setup (native only) ──────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn server_main() {
    use std::path::PathBuf;
    use std::sync::Arc;

    use dioxus::server::DioxusRouterExt;
    use dioxus_extism_host::{InvocationError, PluginRuntimeBuilder, PluginSource};

    let posts = sample_posts();

    let builder = PluginRuntimeBuilder::new()
        .with_plugin_page_prefix("/p")
        .register_invocation("get_posts", None, {
            let posts = posts.clone();
            move |_args: serde_json::Value, _session| {
                let posts = posts.clone();
                async move { Ok::<Vec<Post>, InvocationError>(posts) }
            }
        })
        .register_invocation("get_post", None, {
            move |args: serde_json::Value, _session| {
                let posts = posts.clone();
                async move {
                    let slug = args["slug"].as_str().unwrap_or("").to_owned();
                    Ok::<Option<Post>, InvocationError>(
                        posts.iter().find(|p| p.slug == slug).cloned(),
                    )
                }
            }
        });

    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(async move {
            let wasm_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent().expect("has parent")
                .parent().expect("has parent")
                .parent().expect("has parent")
                .join("target/wasm32-unknown-unknown/release");

            let comments_wasm = wasm_root.join("showcase_plugin_comments.wasm");
            let stats_wasm    = wasm_root.join("showcase_plugin_stats.wasm");

            let mut builder = builder;
            if comments_wasm.exists() {
                builder = builder.add_plugin(PluginSource::File(comments_wasm));
                tracing::info!("showcase/comments plugin loaded");
            } else {
                tracing::warn!("showcase_plugin_comments.wasm not found — build it first");
            }
            if stats_wasm.exists() {
                builder = builder.add_plugin(PluginSource::File(stats_wasm));
                tracing::info!("showcase/stats plugin loaded");
            } else {
                tracing::warn!("showcase_plugin_stats.wasm not found — build it first");
            }

            let runtime = builder.build().await.expect("plugin runtime");

            let addr = dioxus::cli_config::fullstack_address_or_localhost();
            let router = runtime
                .api_router()
                .await
                .serve_dioxus_application(dioxus::server::ServeConfig::new(), App)
                .layer(axum::Extension(Arc::clone(&runtime)));

            tracing::info!("listening on {addr}");
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
            axum::serve(listener, router).await.expect("serve");
        });
}

// ── Custom server functions ───────────────────────────────────────────────────

/// Fetch a post AND fire the `post_viewed` hook (stats plugin increments view count).
#[server]
async fn get_showcase_post(
    slug: String,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<Option<Post>, ServerFnError> {
    use std::sync::Arc;

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        // No runtime: fall back to in-memory lookup.
        return Ok(sample_posts().into_iter().find(|p| p.slug == slug));
    };

    let session = SessionCtx { session_id: session_id.clone(), user_id: None, client: caps, caller: None };

    // Fire the hook — stats plugin intercepts and increments the view counter.
    let _ = runtime
        .run_hook("post_viewed", serde_json::json!({ "slug": slug }), &session)
        .await;

    Ok(sample_posts().into_iter().find(|p| p.slug == slug))
}

/// Render the `post-comments` slot with `current_slug` pre-set for the comments plugin.
#[server]
async fn get_comments_slot(
    post_slug: String,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError> {
    use std::sync::Arc;

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        return Ok(vec![]);
    };

    let plugin_id = PluginId("showcase/comments".into());
    runtime
        .set_plugin_state(&plugin_id, &session_id, "current_slug", serde_json::json!(post_slug))
        .await;

    let session = SessionCtx { session_id, user_id: None, client: caps, caller: None };
    runtime
        .render_slot("post-comments", &session)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

/// Render the `post-stats` slot with `current_slug` pre-set for the stats plugin.
#[server]
async fn get_stats_slot(
    post_slug: String,
    session_id: SessionId,
    caps: ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError> {
    use std::sync::Arc;

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        return Ok(vec![]);
    };

    let plugin_id = PluginId("showcase/stats".into());
    runtime
        .set_plugin_state(&plugin_id, &session_id, "current_slug", serde_json::json!(post_slug))
        .await;

    let session = SessionCtx { session_id, user_id: None, client: caps, caller: None };
    runtime
        .render_slot("post-stats", &session)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

// ── Routes ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Routable, PartialEq)]
enum Route {
    // All host routes share the AppLayout, which wraps every page with
    // PluginAwareRouter so that route transforms (e.g. stats banner on "/")
    // are applied.  PluginAwareRouter renders Outlet::<Route> internally, so
    // it must be invoked from inside a Router — hence the layout pattern.
    #[layout(AppLayout)]
    #[route("/")]
    HomePage,
    #[route("/posts/:slug")]
    PostPage { slug: String },
    /// Catch-all for plugin-declared pages (under prefix `/p`).
    #[route("/p/:..segments")]
    PluginPage { segments: Vec<String> },
    #[end_layout]
    /// Catch-all 404 — outside `AppLayout` so it renders without plugin transforms.
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

// ── App shell ─────────────────────────────────────────────────────────────────

/// Layout wrapper: applies plugin route transforms then renders the matched page.
///
/// `PluginAwareRouter` renders `Outlet::<Route>` internally, which requires an
/// ancestor `Router<Route>` in the component tree — this layout provides that.
#[component]
fn AppLayout() -> Element {
    rsx! { PluginAwareRouter::<Route> {} }
}

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

// ── Pages ─────────────────────────────────────────────────────────────────────

/// Home page: list of posts.
/// The stats plugin's `transform_home_banner` injects a "trending" banner above.
#[component]
fn HomePage() -> Element {
    let posts = sample_posts();
    rsx! {
        div { class: "home-page",
            h1 { "Showcase Blog" }
            p { "A dioxus-extism demo with two plugins: " em { "Comments" } " and " em { "Stats" } "." }
            ul { class: "post-list",
                for post in posts {
                    li {
                        Link { to: Route::PostPage { slug: post.slug.clone() },
                            strong { "{post.title}" }
                        }
                        " — {post.body}"
                    }
                }
            }
            hr {}
            p {
                "Plugin pages: "
                Link { to: Route::PluginPage { segments: vec!["comments".into()] }, "Recent comments" }
                " · "
                Link { to: Route::PluginPage { segments: vec!["stats".into()] }, "Statistics dashboard" }
            }
            p {
                "Plugin API routes (open in browser/curl): "
                code { "/api/stats" }
                ", "
                code { "/api/comments/rust-async" }
            }
        }
    }
}

/// Post page: post body + stats slot + comments slot.
#[component]
fn PostPage(slug: String) -> Element {
    let session_id = use_session_id();
    let client_caps = use_context::<ClientCapabilities>();

    // Fetch post (also fires post_viewed hook).
    let post = {
        let slug = slug.clone();
        let sid: SessionId = session_id.read().clone();
        let caps = client_caps.clone();
        use_resource(move || {
            let slug = slug.clone();
            let sid = sid.clone();
            let caps = caps.clone();
            async move { get_showcase_post(slug, sid, caps).await }
        })
    };

    // Fetch stats slot with slug pre-set.
    let stats_slot = {
        let slug = slug.clone();
        let sid: SessionId = session_id.read().clone();
        let caps = client_caps.clone();
        use_resource(move || {
            let slug = slug.clone();
            let sid = sid.clone();
            let caps = caps.clone();
            async move { get_stats_slot(slug, sid, caps).await }
        })
    };

    // Fetch comments slot with slug pre-set.
    let comments_slot = {
        let sid: SessionId = session_id.read().clone();
        use_resource(move || {
            let slug = slug.clone();
            let sid = sid.clone();
            let caps = client_caps.clone();
            async move { get_comments_slot(slug, sid, caps).await }
        })
    };

    rsx! {
        div { class: "post-page",
            // Post content
            match post.read().as_ref() {
                None => rsx! { p { "Loading…" } },
                Some(Ok(Some(p))) => rsx! {
                    h1 { "{p.title}" }
                    p { class: "post-body", "{p.body}" }
                    p { class: "post-tags",
                        for tag in &p.tags { span { class: "tag", " #{tag}" } }
                    }
                },
                Some(Ok(None)) => rsx! { p { "Post not found." } },
                Some(Err(e)) => rsx! { p { "Error: {e}" } },
            }

            hr {}

            // Stats slot (view count + reactions from stats plugin)
            h3 { "Post stats" }
            { render_slot_contents(stats_slot.read().as_ref(), session_id) }

            hr {}

            // Comments slot (comment list + form from comments plugin)
            { render_slot_contents(comments_slot.read().as_ref(), session_id) }

            hr {}
            Link { to: Route::HomePage, "← Back to home" }
        }
    }
}

/// 404 page for unrecognised paths.
#[component]
fn NotFound(segments: Vec<String>) -> Element {
    let path = format!("/{}", segments.join("/"));
    rsx! {
        div { class: "not-found",
            h1 { "404 — Page not found" }
            p { "No route matched " code { "{path}" } "." }
            p {
                "Posts live at " code { "/posts/:slug" }
                ", plugin pages at " code { "/p/:path" } "."
            }
            Link { to: Route::HomePage, "← Back to home" }
        }
    }
}

/// Catch-all for plugin-declared pages.
#[component]
fn PluginPage(segments: Vec<String>) -> Element {
    let relative_path = format!("/{}", segments.join("/"));
    rsx! {
        div { class: "plugin-page",
            PluginPageOutlet {
                relative_path,
                not_found: rsx! { p { "No plugin page found at this path." } },
            }
            hr {}
            Link { to: Route::HomePage, "← Back to home" }
        }
    }
}

// ── Slot rendering helper ─────────────────────────────────────────────────────

fn render_slot_contents(
    result: Option<&Result<Vec<SlotContent>, ServerFnError>>,
    session_id: Signal<SessionId>,
) -> Element {
    match result {
        None => rsx! { p { "Loading plugin content…" } },
        Some(Ok(contents)) if !contents.is_empty() => rsx! {
            for content in contents.iter().cloned() {
                PluginViewRenderer {
                    key: "{content.plugin_id.0}",
                    view: content.view,
                    session_id,
                    plugin_id: Some(content.plugin_id),
                }
            }
        },
        Some(Ok(_)) => rsx! { p { class: "no-plugin-content", "(no plugin content)" } },
        Some(Err(e)) => rsx! { p { "Plugin error: {e}" } },
    }
}
