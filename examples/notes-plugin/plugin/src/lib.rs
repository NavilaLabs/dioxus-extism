//! notes-plugin — demonstrates `dx_invoke` and `on_interaction`.
//!
//! Slot "page-notes": reads `current_page` from session state (set by the host
//! before `render_slot`), calls `get_notes` to fetch notes from the host store,
//! and renders a notes list with a draft input + submit button.
//!
//! Interactions:
//! - `update_draft`: saves the input value to session state on every keystroke.
//! - `submit_note`: reads the saved draft, calls `add_note`, returns a fresh view.

#![allow(unsafe_code)]

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::plugin;
use extism_pdk::{FnResult, Json, plugin_fn};
use serde::{Deserialize, Serialize};

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct Note {
    id: u64,
    text: String,
}

// ── Plugin struct ─────────────────────────────────────────────────────────────

struct NotesPlugin;

impl DioxusPlugin for NotesPlugin {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("example/notes-plugin".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "page-notes".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![HostCapability::Invoke {
                names: vec!["get_notes".into(), "add_note".into()],
            }],
            page_routes: vec![PageRouteDeclaration {
                path: "/notes".into(),
                title: Some("All Notes".into()),
                render_fn: "render_notes_page".into(),
                bypass_layout: false,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for NotesPlugin {
    const SLOT_NAME: &'static str = "page-notes";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        let slug = read_current_page().map_err(|e| PdkError::HostFn(e.to_string()))?;
        let notes = fetch_notes(&slug).map_err(|e| PdkError::HostFn(e.to_string()))?;
        let draft = read_draft().map_err(|e| PdkError::HostFn(e.to_string()))?;
        Ok(build_notes_view(&slug, &notes, &draft))
    }
}

// ── Exports ───────────────────────────────────────────────────────────────────

plugin! { type: NotesPlugin, slots: [NotesPlugin] }

/// Handle UI interactions: `update_draft` and `submit_note`.
#[plugin_fn]
pub fn on_interaction(
    input: Json<(HandlerId, serde_json::Value, SessionCtx)>,
) -> FnResult<Json<ViewUpdate>> {
    let (handler_id, event_data, _session) = input.0;

    let update = match handler_id.0.as_str() {
        "update_draft" => handle_update_draft(&event_data)?,
        "submit_note" => handle_submit_note()?,
        _ => ViewUpdate { view: None, events: vec![] },
    };

    Ok(Json(update))
}

/// Render the `/notes` page route: lists all notes across every slug.
#[plugin_fn]
pub fn render_notes_page(
    Json(input): Json<PageRouteInput>,
) -> FnResult<Json<PluginView>> {
    let _ = input; // no path params needed for the listing page
    let all_notes = fetch_notes("").unwrap_or_default();
    let count = all_notes.len();

    let items: Vec<PluginView> = all_notes
        .iter()
        .map(|n| {
            div()
                .class("plugin-note-item")
                .child(text(format!("• {}", n.text)))
                .build()
        })
        .collect();

    let body = if items.is_empty() {
        div()
            .class("plugin-notes-empty")
            .child(text("No notes yet across any article."))
            .build()
    } else {
        div().class("plugin-notes-list").children(items).build()
    };

    Ok(Json(
        div()
            .class("plugin-notes-page")
            .child(element("h1").child(text(format!("All Notes ({count})"))).build())
            .child(body)
            .build(),
    ))
}

// ── Interaction handlers ──────────────────────────────────────────────────────

fn handle_update_draft(
    event_data: &serde_json::Value,
) -> Result<ViewUpdate, extism_pdk::Error> {
    let text = event_data
        .get("value")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let encoded = serde_json::to_string(&serde_json::Value::String(text))
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    unsafe { host_fns::dx_state_set("draft", encoded) }?;
    Ok(ViewUpdate { view: None, events: vec![] })
}

fn handle_submit_note() -> Result<ViewUpdate, extism_pdk::Error> {
    let draft = read_draft()?;
    if draft.is_empty() {
        return Ok(ViewUpdate { view: None, events: vec![] });
    }

    let slug = read_current_page()?;

    let args = serde_json::to_string(&serde_json::json!({"slug": slug, "text": draft}))
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    let notes_raw = unsafe { host_fns::dx_invoke("add_note", args) }?;
    let notes: Vec<Note> = serde_json::from_str(&notes_raw).unwrap_or_default();

    unsafe { host_fns::dx_state_delete("draft") }?;

    let new_view = build_notes_view(&slug, &notes, "");
    Ok(ViewUpdate { view: Some(new_view), events: vec![] })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_current_page() -> Result<String, extism_pdk::Error> {
    let raw = unsafe { host_fns::dx_state_get("current_page") }?;
    let val: Option<String> = serde_json::from_str(&raw).unwrap_or(None);
    Ok(val.unwrap_or_default())
}

fn read_draft() -> Result<String, extism_pdk::Error> {
    let raw = unsafe { host_fns::dx_state_get("draft") }?;
    let val: Option<String> = serde_json::from_str(&raw).unwrap_or(None);
    Ok(val.unwrap_or_default())
}

fn fetch_notes(slug: &str) -> Result<Vec<Note>, extism_pdk::Error> {
    let args = serde_json::to_string(&serde_json::json!({"slug": slug}))
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    let raw = unsafe { host_fns::dx_invoke("get_notes", args) }?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

// ── View builder ──────────────────────────────────────────────────────────────

fn build_notes_view(slug: &str, notes: &[Note], draft: &str) -> PluginView {
    let notes_items: Vec<PluginView> = notes
        .iter()
        .map(|n| {
            div()
                .class("plugin-note-item")
                .child(text(format!("• {}", n.text)))
                .build()
        })
        .collect();

    let notes_list = div().class("plugin-notes-list").children(notes_items).build();

    let empty_msg = if notes.is_empty() {
        div()
            .class("plugin-notes-empty")
            .child(text(format!("No notes yet for \"{slug}\". Be the first!")))
            .build()
    } else {
        PluginView::Empty
    };

    let form = div()
        .class("plugin-note-form")
        .child(
            element("input")
                .class("plugin-note-input")
                .attr("placeholder", "Write a note…")
                .attr("value", draft)
                .on(BoundEventHandler {
                    event: DomEvent::Input,
                    handler_id: HandlerId("update_draft".into()),
                    debounce_ms: Some(300),
                })
                .build(),
        )
        .child(
            element("button")
                .class("plugin-note-submit")
                .on(BoundEventHandler {
                    event: DomEvent::Click,
                    handler_id: HandlerId("submit_note".into()),
                    debounce_ms: None,
                })
                .child(text("Add note"))
                .build(),
        )
        .build();

    div()
        .class("plugin-notes-section")
        .child(element("h3").class("plugin-notes-title").child(text("Notes")).build())
        .child(empty_msg)
        .child(notes_list)
        .child(form)
        .build()
}
