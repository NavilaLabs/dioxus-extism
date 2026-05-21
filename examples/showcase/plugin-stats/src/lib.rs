//! showcase/plugin-stats — demonstrates:
//!
//! - Slot provider (`post-stats`): per-post view count + like/dislike buttons.
//! - Named API route: `GET /api/stats`.
//! - Plugin page route: `/stats` → full statistics dashboard.
//! - Event subscriber: reacts to `comment_posted` from the comments plugin.
//! - Hook handler: `hook_post_viewed` increments per-post view counts.
//! - Route transform: injects a "trending" banner before the home page (`/`).
//! - Global state: view counts, comment counts, reactions — keyed by post slug.
//! - `on_load` lifecycle: logs plugin initialisation.
//! - `on_interaction`: like/dislike buttons update reaction counts live.
//! - `dx_invoke`: calls `get_posts` to list posts in the dashboard.
//! - `dx_plugin_state_get`: reads the comments plugin's data for totals.

#![allow(unsafe_code)]

use std::collections::HashMap;

use dioxus_extism_pdk::host_fns;
use dioxus_extism_pdk::prelude::*;
use dioxus_extism_pdk::{api_route_fn, plugin};
use extism_pdk::{FnResult, Json, plugin_fn};
use serde::{Deserialize, Serialize};

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Reactions {
    likes: u64,
    dislikes: u64,
}

/// Minimal post info returned by `get_posts` invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostInfo {
    slug: String,
    title: String,
}

// ── Global state keys ─────────────────────────────────────────────────────────

const VIEWS_KEY: &str = "views";
const COMMENT_COUNTS_KEY: &str = "comment_counts";
const REACTIONS_KEY: &str = "reactions";

// ── Plugin struct ─────────────────────────────────────────────────────────────

struct StatsPlugin;

impl DioxusPlugin for StatsPlugin {
    fn manifest() -> PluginManifest {
        PluginManifest {
            id: PluginId("showcase/stats".into()),
            version: "0.1.0".into(),
            slots: vec![SlotRegistration {
                name: "post-stats".into(),
                priority_hint: PriorityHint::Normal,
            }],
            host_capabilities: vec![
                HostCapability::Invoke {
                    names: vec!["get_posts".into(), "get_post".into()],
                },
                HostCapability::GlobalStateRead {
                    keys: vec![VIEWS_KEY.into(), COMMENT_COUNTS_KEY.into(), REACTIONS_KEY.into()],
                },
                HostCapability::GlobalStateWrite {
                    keys: vec![VIEWS_KEY.into(), COMMENT_COUNTS_KEY.into(), REACTIONS_KEY.into()],
                },
            ],
            event_subscriptions: vec!["comment_posted".into()],
            hooks: vec![HookRegistration {
                hook_name: "post_viewed".into(),
                priority_hint: PriorityHint::Normal,
            }],
            transforms: vec![TransformDeclaration {
                selector: Selector::Route(RoutePattern("/".into())),
                op: TransformOp::InjectBefore,
                transform_fn: "transform_home_banner".into(),
                priority_hint: PriorityHint::Normal,
            }],
            api_routes: vec![ApiRouteDeclaration::get("/api/stats", "api_stats_get")],
            page_routes: vec![PageRouteDeclaration {
                path: "/stats".into(),
                title: Some("Statistics Dashboard".into()),
                render_fn: "render_stats_dashboard".into(),
                bypass_layout: false,
            }],
            ..Default::default()
        }
    }
}

impl SlotProvider for StatsPlugin {
    const SLOT_NAME: &'static str = "post-stats";

    fn render(_ctx: &PluginCtx) -> Result<PluginView, PdkError> {
        let slug = read_session_str("current_slug")?;
        let views = read_count_map(VIEWS_KEY, &slug)?;
        let comments = read_count_map(COMMENT_COUNTS_KEY, &slug)?;
        let reactions = read_reactions_for(&slug)?;
        Ok(build_stats_view(&slug, views, comments, &reactions))
    }
}

impl OnLoad for StatsPlugin {
    fn on_load(_ctx: &PluginCtx) -> Result<(), PdkError> {
        unsafe { host_fns::dx_log("info", "stats plugin loaded") }
            .map_err(|e| PdkError::HostFn(e.to_string()))?;
        Ok(())
    }
}

// ── Exports ───────────────────────────────────────────────────────────────────

plugin! { type: StatsPlugin, slots: [StatsPlugin] }

/// Called once after pool init (on_load lifecycle).
#[plugin_fn]
pub fn on_load(Json(session): Json<SessionCtx>) -> FnResult<()> {
    let ctx = PluginCtx::from_session(session);
    StatsPlugin::on_load(&ctx).map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    Ok(())
}

/// `GET /api/stats` — aggregated view / comment / reaction counts for all posts.
api_route_fn!(api_stats_get, |_req: ApiRequest| {
    let views: HashMap<String, u64> = read_global(VIEWS_KEY)?;
    let comment_counts: HashMap<String, u64> = read_global(COMMENT_COUNTS_KEY)?;
    let reactions: HashMap<String, Reactions> = read_global(REACTIONS_KEY)?;
    let slugs: std::collections::BTreeSet<_> = views
        .keys()
        .chain(comment_counts.keys())
        .chain(reactions.keys())
        .cloned()
        .collect();
    let rows: Vec<serde_json::Value> = slugs
        .iter()
        .map(|slug| {
            let r = reactions.get(slug).cloned().unwrap_or_default();
            serde_json::json!({
                "slug": slug,
                "views": views.get(slug).copied().unwrap_or(0),
                "comments": comment_counts.get(slug).copied().unwrap_or(0),
                "likes": r.likes,
                "dislikes": r.dislikes,
            })
        })
        .collect();
    Ok(ApiResponse {
        status: 200,
        body: Some(serde_json::Value::Array(rows)),
        ..Default::default()
    })
});

/// Plugin page `/stats` — full stats dashboard.
#[plugin_fn]
pub fn render_stats_dashboard(Json(_input): Json<PageRouteInput>) -> FnResult<Json<PluginView>> {
    let posts_raw = unsafe { host_fns::dx_invoke("get_posts", "{}".to_owned()) }?;
    let posts: Vec<PostInfo> = serde_json::from_str(&posts_raw)
        .map_err(|e| extism_pdk::Error::msg(format!("parse posts: {e}")))?;

    let views: HashMap<String, u64> = read_global(VIEWS_KEY)
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    let comment_counts: HashMap<String, u64> = read_global(COMMENT_COUNTS_KEY)
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    let reactions: HashMap<String, Reactions> = read_global(REACTIONS_KEY)
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;

    let mut rows: Vec<PluginView> = vec![
        div()
            .class("stats-dashboard-header")
            .child(element("h1").child(text("Statistics Dashboard")).build())
            .child(element("p").child(text("Powered by the stats plugin · tracks views, comments and reactions.")).build())
            .build(),
    ];

    if posts.is_empty() {
        rows.push(div().class("stats-empty").child(text("No posts yet.")).build());
    } else {
        let header_row = div()
            .class("stats-table-header")
            .child(span("Post"))
            .child(span("Views"))
            .child(span("Comments"))
            .child(span("👍 Likes"))
            .child(span("👎 Dislikes"))
            .build();
        let mut table_rows = vec![header_row];
        for post in &posts {
            let r = reactions.get(&post.slug).cloned().unwrap_or_default();
            table_rows.push(
                div()
                    .class("stats-table-row")
                    .child(span(&post.title))
                    .child(span(format!("{}", views.get(&post.slug).copied().unwrap_or(0))))
                    .child(span(format!("{}", comment_counts.get(&post.slug).copied().unwrap_or(0))))
                    .child(span(format!("{}", r.likes)))
                    .child(span(format!("{}", r.dislikes)))
                    .build(),
            );
        }
        rows.push(div().class("stats-table").children(table_rows).build());
    }

    Ok(Json(PluginView::Fragment(rows)))
}

/// Event handler: fired when `comment_posted` arrives from the comments plugin.
#[plugin_fn]
pub fn on_event(
    Json((event, _session)): Json<(PluginEvent, SessionCtx)>,
) -> FnResult<()> {
    if event.name == "comment_posted" {
        let slug = event.payload["post_slug"].as_str().unwrap_or("").to_owned();
        if !slug.is_empty() {
            increment_count(COMMENT_COUNTS_KEY, &slug)
                .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
        }
    }
    Ok(())
}

/// Hook handler: called by the host when a post is viewed.
#[plugin_fn]
pub fn hook_post_viewed(
    Json((call, _session)): Json<(HookCall, SessionCtx)>,
) -> FnResult<Json<HookResult>> {
    let slug = call.context["slug"].as_str().unwrap_or("").to_owned();
    if !slug.is_empty() {
        increment_count(VIEWS_KEY, &slug)
            .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    }
    Ok(Json(HookResult::Continue { context: call.context }))
}

/// Route transform: injects a "trending" banner before the home page content.
#[plugin_fn]
pub fn transform_home_banner(Json(_input): Json<TransformInput>) -> FnResult<Json<TransformOutput>> {
    let views: HashMap<String, u64> = read_global(VIEWS_KEY)
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;

    let banner = if let Some((top_slug, top_count)) =
        views.iter().max_by_key(|&(_, v)| v)
    {
        div()
            .class("stats-trending-banner")
            .child(
                element("p")
                    .child(text(format!("🔥 Trending: /{top_slug}  ({top_count} views)")))
                    .build(),
            )
            .build()
    } else {
        div()
            .class("stats-trending-banner stats-trending-banner--empty")
            .child(element("p").child(text("📊 View any post to start tracking statistics.")).build())
            .build()
    };

    Ok(Json(TransformOutput { view: banner }))
}

/// Interaction handler: like / dislike buttons from the post-stats slot.
#[plugin_fn]
pub fn on_interaction(
    Json((handler_id, _event_data, _session)): Json<(HandlerId, serde_json::Value, SessionCtx)>,
) -> FnResult<Json<ViewUpdate>> {
    let id = handler_id.0.as_str();

    let (action, slug) = if let Some(rest) = id.strip_prefix("like:") {
        ("like", rest.to_owned())
    } else if let Some(rest) = id.strip_prefix("dislike:") {
        ("dislike", rest.to_owned())
    } else {
        return Ok(Json(ViewUpdate { view: None, events: vec![] }));
    };

    let mut reactions: HashMap<String, Reactions> = read_global(REACTIONS_KEY)
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    let r = reactions.entry(slug.clone()).or_default();
    if action == "like" {
        r.likes += 1;
    } else {
        r.dislikes += 1;
    }
    write_global(REACTIONS_KEY, &reactions)
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;

    let views = read_count_map(VIEWS_KEY, &slug)
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    let comments = read_count_map(COMMENT_COUNTS_KEY, &slug)
        .map_err(|e| extism_pdk::Error::msg(e.to_string()))?;
    let updated_reactions = reactions.get(&slug).cloned().unwrap_or_default();
    let new_view = build_stats_view(&slug, views, comments, &updated_reactions);

    Ok(Json(ViewUpdate { view: Some(new_view), events: vec![] }))
}

// ── View builder ──────────────────────────────────────────────────────────────

fn build_stats_view(slug: &str, views: u64, comments: u64, reactions: &Reactions) -> PluginView {
    div()
        .class("post-stats")
        .child(element("h4").child(text("Post stats")).build())
        .child(
            div()
                .class("post-stats-counts")
                .child(span(format!("👁 {views} views")))
                .child(span(format!("💬 {comments} comments")))
                .build(),
        )
        .child(
            div()
                .class("post-stats-reactions")
                .child(
                    element("button")
                        .class("reaction-btn reaction-btn--like")
                        .on(BoundEventHandler {
                            event: DomEvent::Click,
                            handler_id: HandlerId(format!("like:{slug}")),
                            debounce_ms: None,
                        })
                        .child(text(format!("👍 {} Likes", reactions.likes)))
                        .build(),
                )
                .child(
                    element("button")
                        .class("reaction-btn reaction-btn--dislike")
                        .on(BoundEventHandler {
                            event: DomEvent::Click,
                            handler_id: HandlerId(format!("dislike:{slug}")),
                            debounce_ms: None,
                        })
                        .child(text(format!("👎 {} Dislikes", reactions.dislikes)))
                        .build(),
                )
                .build(),
        )
        .build()
}

fn span(content: impl Into<String>) -> PluginView {
    dioxus_extism_pdk::span().child(text(content.into())).build()
}

// ── State helpers ─────────────────────────────────────────────────────────────

fn read_global<T: Default + for<'de> Deserialize<'de>>(key: &str) -> Result<T, PdkError> {
    let raw = unsafe { host_fns::dx_global_state_get(key) }
        .map_err(|e| PdkError::HostFn(e.to_string()))?;
    let opt: Option<T> = serde_json::from_str(&raw).map_err(PdkError::Json)?;
    Ok(opt.unwrap_or_default())
}

fn write_global<T: Serialize>(key: &str, value: &T) -> Result<(), PdkError> {
    let json = serde_json::to_string(value).map_err(PdkError::Json)?;
    unsafe { host_fns::dx_global_state_set(key, json) }
        .map_err(|e| PdkError::HostFn(e.to_string()))?;
    Ok(())
}

fn read_session_str(key: &str) -> Result<String, PdkError> {
    let raw = unsafe { host_fns::dx_state_get(key) }
        .map_err(|e| PdkError::HostFn(e.to_string()))?;
    let opt: Option<String> = serde_json::from_str(&raw).map_err(PdkError::Json)?;
    Ok(opt.unwrap_or_default())
}

fn read_count_map(key: &str, slug: &str) -> Result<u64, PdkError> {
    let map: HashMap<String, u64> = read_global(key)?;
    Ok(map.get(slug).copied().unwrap_or(0))
}

fn read_reactions_for(slug: &str) -> Result<Reactions, PdkError> {
    let map: HashMap<String, Reactions> = read_global(REACTIONS_KEY)?;
    Ok(map.get(slug).cloned().unwrap_or_default())
}

fn increment_count(key: &str, slug: &str) -> Result<(), PdkError> {
    let mut map: HashMap<String, u64> = read_global(key)?;
    *map.entry(slug.to_owned()).or_insert(0) += 1;
    write_global(key, &map)
}
