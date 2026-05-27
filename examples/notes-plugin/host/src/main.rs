//! notes-plugin-host — demonstrates `dx_invoke` with a host-owned note store.
//!
//! The plugin calls `get_notes` / `add_note` via `dx_invoke` to read from and
//! write to a shared in-memory store managed here. No plugin code is needed in
//! the host — the plugin slot renders entirely from its own WASM logic.

use dioxus::prelude::*;
use dioxus_extism_frontend::{
    PluginBootProvider, PluginPageOutlet, PluginViewRenderer, SessionProviderRoot,
    WebSessionProvider, use_session_id,
};
use dioxus_extism_protocol::{PluginId, SessionId, SlotContent};
use serde::{Deserialize, Serialize};

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    server_main();

    #[cfg(target_arch = "wasm32")]
    dioxus::launch(App);
}

// ── Note type (shared between server fns and the invocation handlers) ─────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Note {
    id: u64,
    text: String,
}

// ── Server-side setup (native only) ──────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
fn server_main() {
    use std::{
        collections::HashMap,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use dioxus::server::DioxusRouterExt;
    use dioxus_extism_host::{InvocationError, PluginRuntimeBuilder, PluginSource};
    use tokio::sync::RwLock;

    type NoteStore = Arc<RwLock<HashMap<String, Vec<Note>>>>;

    // Register "get_notes": returns all notes for a slug.
    fn register_get_notes(
        builder: PluginRuntimeBuilder,
        store: NoteStore,
    ) -> PluginRuntimeBuilder {
        builder.register_invocation(
            "get_notes",
            None,
            move |args: serde_json::Value, _session| {
                let store = store.clone();
                async move {
                    let slug = string_field(&args, "slug");
                    let notes = store.read().await.get(&slug).cloned().unwrap_or_default();
                    Ok::<Vec<Note>, InvocationError>(notes)
                }
            },
        )
    }

    // Register "add_note": appends a note and returns the updated list.
    fn register_add_note(
        builder: PluginRuntimeBuilder,
        store: NoteStore,
        id_ctr: Arc<AtomicU64>,
    ) -> PluginRuntimeBuilder {
        builder.register_invocation(
            "add_note",
            None,
            move |args: serde_json::Value, _session| {
                let store = store.clone();
                let id_ctr = id_ctr.clone();
                async move {
                    let slug = string_field(&args, "slug");
                    let text = string_field(&args, "text");
                    if text.is_empty() {
                        return Ok(store.read().await.get(&slug).cloned().unwrap_or_default());
                    }
                    let note = Note { id: id_ctr.fetch_add(1, Ordering::Relaxed), text };
                    // Write the note, then release the lock before the final read.
                    store.write().await.entry(slug.clone()).or_default().push(note);
                    Ok::<Vec<Note>, InvocationError>(
                        store.read().await.get(&slug).cloned().unwrap_or_default(),
                    )
                }
            },
        )
    }

    fn string_field(args: &serde_json::Value, field: &str) -> String {
        args.get(field)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    }

    let store: NoteStore = Arc::new(RwLock::new(HashMap::new()));
    let next_id = Arc::new(AtomicU64::new(1));

    let builder = register_get_notes(PluginRuntimeBuilder::new(), store.clone());
    let builder = register_add_note(builder, store, next_id)
        .with_plugin_page_prefix("/p");

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
                .join("target/wasm32-unknown-unknown/release/notes_plugin_plugin.wasm");

            let mut builder = builder;
            if wasm_path.exists() {
                builder = builder.add_plugin(PluginSource::File(wasm_path));
                tracing::info!("notes-plugin loaded");
            } else {
                tracing::warn!("plugin WASM not found — page-notes slot will be empty");
            }

            let runtime = builder.build().await.expect("plugin runtime build");

            let addr = dioxus::cli_config::fullstack_address_or_localhost();
            let router = runtime
                .api_router()
                .await
                .serve_dioxus_application(dioxus::server::ServeConfig::new(), App)
                .layer(axum::Extension(std::sync::Arc::clone(&runtime)));

            tracing::info!("listening on {addr}");
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .unwrap_or_else(|e| {
                    panic!("bind {addr}: {e} — set IP / PORT env vars (e.g. IP=0.0.0.0 PORT=3011)")
                });
            axum::serve(listener, router).await.expect("serve");
        });
}

// ── Server function: slot with pre-populated session state ────────────────────

/// Fetches slot content for `page-notes` after writing `current_page` into the
/// plugin's session state so it knows which article it is rendering for.
#[server]
async fn get_page_notes(
    slug: String,
    session_id: SessionId,
    caps: dioxus_extism_protocol::ClientCapabilities,
) -> Result<Vec<SlotContent>, ServerFnError> {
    use std::sync::Arc;

    use dioxus_extism_host::PluginRuntime;
    use dioxus_extism_protocol::SessionCtx;

    let Some(runtime) = dioxus::fullstack::FullstackContext::current()
        .and_then(|ctx| ctx.extension::<Arc<PluginRuntime>>())
    else {
        tracing::warn!("get_page_notes: PluginRuntime not in extensions");
        return Ok(vec![]);
    };

    let plugin_id = PluginId("example/notes-plugin".into());
    runtime
        .set_plugin_state(
            &plugin_id,
            &session_id,
            "current_page",
            serde_json::Value::String(slug),
        )
        .await;

    let session = SessionCtx { session_id, user_id: None, client: caps, caller: None };
    runtime
        .render_slot("page-notes", &session)
        .await
        .map_err(|e| ServerFnError::new(e.to_string()))
}

// ── Routes ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Routable, PartialEq)]
#[allow(clippy::enum_variant_names)]
enum Route {
    #[route("/")]
    HomePage,
    #[route("/articles/:slug")]
    ArticlePage { slug: String },
    #[route("/p/:..segments")]
    PluginPage { segments: Vec<String> },
}

/// Root app: session context + plugin boot wrapping the router.
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

/// Home page: lists some article links.
#[component]
fn HomePage() -> Element {
    rsx! {
        div {
            h1 { "Notes Plugin Example" }
            p { "Click an article to see the notes plugin in action." }
            ul {
                li { Link { to: Route::ArticlePage { slug: "rust-async".into() }, "Rust Async" } }
                li { Link { to: Route::ArticlePage { slug: "wasm-plugins".into() }, "WASM Plugins" } }
                li { Link { to: Route::ArticlePage { slug: "dioxus-tutorial".into() }, "Dioxus Tutorial" } }
            }
            p {
                Link {
                    to: Route::PluginPage { segments: vec!["notes".into()] },
                    "View all notes (plugin page)"
                }
            }
        }
    }
}

/// Wildcard route: dispatches to a plugin-declared page via `PluginPageOutlet`.
#[component]
fn PluginPage(segments: Vec<String>) -> Element {
    let relative_path = format!("/{}", segments.join("/"));
    rsx! {
        div {
            PluginPageOutlet {
                relative_path,
                not_found: rsx! {
                    p { "No plugin page found at this path." }
                },
            }
            hr {}
            Link { to: Route::HomePage, "← Back to home" }
        }
    }
}

/// Article page: static content + a plugin slot for notes.
#[component]
fn ArticlePage(slug: String) -> Element {
    rsx! {
        div {
            h1 { "Article: {slug}" }
            p { "This is the article body. The plugin slot below is contributed entirely by the WASM plugin." }
            p {
                "The plugin calls "
                code { "dx_invoke(\"get_notes\", {{slug}})" }
                " to read from the host store and "
                code { "dx_invoke(\"add_note\", {{slug, text}})" }
                " to persist new ones."
            }
            hr {}
            NotesSlot { slug }
            hr {}
            Link { to: Route::HomePage, "← Back to home" }
        }
    }
}

// ── NotesSlot component ───────────────────────────────────────────────────────

/// Fetches the "page-notes" slot with the current article slug pre-populated in
/// the plugin's session state so the plugin knows which article it is rendering for.
#[component]
fn NotesSlot(slug: String) -> Element {
    let session_id = use_session_id();
    let client_caps = use_context::<dioxus_extism_protocol::ClientCapabilities>();

    let slug_signal = use_signal(|| slug.clone());
    let contents = use_resource(move || {
        let slug = slug_signal.read().clone();
        let sid: SessionId = session_id.read().clone();
        let caps = client_caps.clone();
        async move { get_page_notes(slug, sid, caps).await }
    });

    match contents.read().as_ref() {
        None => rsx! { p { "Loading notes…" } },
        Some(Ok(c)) if !c.is_empty() => {
            rsx! {
                for content in c.iter().cloned() {
                    PluginViewRenderer {
                        key: "{content.plugin_id.0}",
                        view: content.view,
                        session_id,
                        plugin_id: Some(content.plugin_id),
                    }
                }
            }
        }
        Some(Ok(_)) => rsx! { p { "No plugin content available." } },
        Some(Err(e)) => rsx! { p { "Error loading notes: {e}" } },
    }
}
