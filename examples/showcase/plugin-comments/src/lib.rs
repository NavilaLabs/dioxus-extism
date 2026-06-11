//! showcase/plugin-comments — demonstrates:
//!
//! - Slot provider (`post-comments`): comment list + live draft form.
//! - Named API routes: `GET /api/comments/:slug`, `POST /api/comments`.
//! - Plugin page route: `/comments` → recent comments across all posts.
//! - Global state: persists comments keyed by post slug.
//! - Session state: per-user draft text.
//! - `on_interaction`: live draft update + comment submission.
//! - `dx_emit_event`: emits `comment_posted` when a comment is submitted.
//! - `dx_invoke`: calls `get_post` to verify the target post exists.

#![allow(unsafe_code)]

use std::collections::HashMap;

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{api_route_fn, plugin};
use extism_pdk::{FnResult, Json, plugin_fn};
use serde::{Deserialize, Serialize};

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Comment {
    id: u64,
    text: String,
}

/// Global state key: `HashMap<post_slug, Vec<Comment>>`.
const COMMENTS_KEY: &str = "data";

// ── Plugin struct ─────────────────────────────────────────────────────────────

struct CommentsPlugin;

impl DioxusPlugin for CommentsPlugin {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("showcase/comments".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "post-comments".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![HostCapability::Invoke {
                names: vec!["get_post".into()],
            }],
            api_routes: vec![
                ApiRouteDeclaration::get("/api/comments/:slug", "api_comments_get"),
                ApiRouteDeclaration::post("/api/comments", "api_comments_post"),
            ],
            page_routes: vec![PageRouteDeclaration {
                path: "/comments".into(),
                title: Some("Recent Comments".into()),
                render_fn: "render_recent_comments".into(),
                bypass_layout: false,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for CommentsPlugin {
    const SLOT_NAME: &'static str = "post-comments";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        let slug = read_session_str("current_slug").map_err(|e| PdkError::HostFn(e.to_string()))?;
        let draft = read_session_str("draft").map_err(|e| PdkError::HostFn(e.to_string()))?;
        let comments = read_comments_for(&slug).map_err(|e| PdkError::HostFn(e.to_string()))?;
        Ok(build_comments_view(&slug, &comments, &draft))
    }
}

// ── Exports ───────────────────────────────────────────────────────────────────

plugin! { type: CommentsPlugin, slots: [CommentsPlugin] }

// GET /api/comments/:slug — returns all comments for a post as JSON.
api_route_fn!(api_comments_get, |req: ApiRequest| {
    let slug = req.path_params.get("slug").cloned().unwrap_or_default();
    let comments = read_comments_for(&slug).map_err(|e| PdkError::HostFn(e.to_string()))?;
    Ok(ApiResponse {
        status: 200,
        body: Some(serde_json::to_value(&comments).map_err(PdkError::Json)?),
        ..Default::default()
    })
});

// POST /api/comments — body `{ "slug": "...", "text": "..." }`.
api_route_fn!(api_comments_post, |req: ApiRequest| {
    let body = req.body.as_ref().ok_or_else(|| PdkError::HostFn("missing body".into()))?;
    let slug = body["slug"].as_str().unwrap_or("").to_owned();
    let text = body["text"].as_str().unwrap_or("").to_owned();

    if slug.is_empty() || text.is_empty() {
        return Ok(ApiResponse { status: 400, body: Some(serde_json::json!({"error": "slug and text required"})), ..Default::default() });
    }

    // Verify post exists via host invocation.
    let post_raw = unsafe { host_fns::dx_invoke("get_post", serde_json::json!({"slug": slug}).to_string()) }
        .map_err(|e| PdkError::HostFn(e.to_string()))?;
    let post: Option<serde_json::Value> = serde_json::from_str(&post_raw).map_err(PdkError::Json)?;
    if post.is_none() {
        return Ok(ApiResponse { status: 404, body: Some(serde_json::json!({"error": "post not found"})), ..Default::default() });
    }

    let updated = append_comment(&slug, &text).map_err(|e| PdkError::HostFn(e.to_string()))?;
    emit_comment_posted(&slug, &text).map_err(|e| PdkError::HostFn(e.to_string()))?;
    Ok(ApiResponse {
        status: 201,
        body: Some(serde_json::to_value(&updated).map_err(PdkError::Json)?),
        ..Default::default()
    })
});

/// Plugin page: `/comments` — recent comments across all posts.
#[plugin_fn]
pub fn render_recent_comments(Json(_input): Json<PageRouteInput>) -> FnResult<Json<PluginView>> {
    let all: HashMap<String, Vec<Comment>> = read_global(COMMENTS_KEY)?;
    let mut rows: Vec<PluginView> = vec![
        div()
            .class("comments-page-header")
            .child(element("h1").child(text("Recent Comments")).build())
            .build(),
    ];
    if all.is_empty() {
        rows.push(div().class("comments-empty").child(text("No comments yet.")).build());
    } else {
        for (slug, comments) in &all {
            if comments.is_empty() {
                continue;
            }
            rows.push(
                div()
                    .class("comments-post-group")
                    .child(element("h2").child(text(format!("Post: {slug}"))).build())
                    .children(
                        comments
                            .iter()
                            .map(|c| {
                                div()
                                    .class("comment-item")
                                    .child(text(format!("• {}", c.text)))
                                    .build()
                            }),
                    )
                    .build(),
            );
        }
    }
    Ok(Json(PluginView::Fragment(rows)))
}

/// Handle UI interactions from the comments slot.
#[plugin_fn]
pub fn on_interaction(
    Json((handler_id, event_data, _session)): Json<(HandlerId, serde_json::Value, SessionCtx)>,
) -> FnResult<Json<ViewUpdate>> {
    let update = match handler_id.0.as_str() {
        "update_draft" => {
            let text = event_data["value"].as_str().unwrap_or("").to_owned();
            write_session("draft", &text)
                .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
            ViewUpdate { view: None, events: vec![] }
        }
        "submit_comment" => {
            let draft = read_session_str("draft")?;
            let slug = read_session_str("current_slug")?;
            if draft.is_empty() || slug.is_empty() {
                return Ok(Json(ViewUpdate { view: None, events: vec![] }));
            }
            let comments = append_comment(&slug, &draft)?;
            write_session("draft", &String::new())
                .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
            emit_comment_posted(&slug, &draft)?;
            let new_view = build_comments_view(&slug, &comments, "");
            ViewUpdate { view: Some(new_view), events: vec![] }
        }
        _ => ViewUpdate { view: None, events: vec![] },
    };
    Ok(Json(update))
}

// ── View builder ──────────────────────────────────────────────────────────────

fn build_comments_view(slug: &str, comments: &[Comment], draft: &str) -> PluginView {
    let header = element("h3")
        .class("comments-title")
        .child(text(format!("Comments on \"{slug}\"")))
        .build();

    let list = if comments.is_empty() {
        div()
            .class("comments-empty")
            .child(text("No comments yet — be the first!"))
            .build()
    } else {
        div()
            .class("comments-list")
            .children(
                comments
                    .iter()
                    .map(|c| {
                        div()
                            .class("comment-item")
                            .child(text(format!("• {}", c.text)))
                            .build()
                    }),
            )
            .build()
    };

    let form = div()
        .class("comment-form")
        .child(
            element("input")
                .class("comment-input")
                .attr("placeholder", "Write a comment…")
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
                .class("comment-submit")
                .on(BoundEventHandler {
                    event: DomEvent::Click,
                    handler_id: HandlerId("submit_comment".into()),
                    debounce_ms: None,
                })
                .child(text("Post comment"))
                .build(),
        )
        .build();

    div()
        .class("comments-section")
        .child(header)
        .child(list)
        .child(form)
        .build()
}

// ── State helpers ─────────────────────────────────────────────────────────────

fn read_global<T: Default + for<'de> Deserialize<'de>>(key: &str) -> Result<T, extism_pdk::Error> {
    let raw = unsafe { host_fns::dx_global_state_get(key) }?;
    let opt: Option<T> = serde_json::from_str(&raw)
        .map_err(|e| extism_pdk::Error::msg(format!("parse global {key}: {e}")))?;
    Ok(opt.unwrap_or_default())
}

fn write_global<T: Serialize>(key: &str, value: &T) -> Result<(), extism_pdk::Error> {
    let json = serde_json::to_string(value)
        .map_err(|e| extism_pdk::Error::msg(format!("serialize {key}: {e}")))?;
    unsafe { host_fns::dx_global_state_set(key, json) }?;
    Ok(())
}

fn read_session_str(key: &str) -> Result<String, extism_pdk::Error> {
    let raw = unsafe { host_fns::dx_state_get(key) }?;
    let opt: Option<String> = serde_json::from_str(&raw)
        .map_err(|e| extism_pdk::Error::msg(format!("parse session {key}: {e}")))?;
    Ok(opt.unwrap_or_default())
}

fn write_session<T: Serialize>(key: &str, value: &T) -> Result<(), extism_pdk::Error> {
    let json = serde_json::to_string(value)
        .map_err(|e| extism_pdk::Error::msg(format!("serialize session {key}: {e}")))?;
    unsafe { host_fns::dx_state_set(key, json) }?;
    Ok(())
}

fn read_comments_for(slug: &str) -> Result<Vec<Comment>, extism_pdk::Error> {
    let all: HashMap<String, Vec<Comment>> = read_global(COMMENTS_KEY)?;
    Ok(all.get(slug).cloned().unwrap_or_default())
}

fn append_comment(slug: &str, text: &str) -> Result<Vec<Comment>, extism_pdk::Error> {
    let mut all: HashMap<String, Vec<Comment>> = read_global(COMMENTS_KEY)?;
    let list = all.entry(slug.to_owned()).or_default();
    let next_id = list.iter().map(|c| c.id).max().unwrap_or(0) + 1;
    list.push(Comment { id: next_id, text: text.to_owned() });
    let updated = list.clone();
    write_global(COMMENTS_KEY, &all)?;
    Ok(updated)
}

fn emit_comment_posted(slug: &str, text: &str) -> Result<(), extism_pdk::Error> {
    let payload = serde_json::json!({ "post_slug": slug, "text": text });
    let event = serde_json::json!({ "name": "comment_posted", "payload": payload }).to_string();
    unsafe { host_fns::dx_emit_event(event) }?;
    Ok(())
}
