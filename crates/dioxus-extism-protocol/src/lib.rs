//! Shared protocol types for `dioxus-extism`.
//!
//! Every message crossing the WASM boundary uses the types defined here.
//! This crate has no dependency on `extism`, `dioxus`, or any platform-specific
//! code, so it compiles for both `wasm32-unknown-unknown` (plugins) and native
//! host targets.
//!
//! All public enums are `#[non_exhaustive]` — new variants may be added in minor
//! versions without a major semver bump.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Monotonically increasing. Bumped whenever `PluginView`, `ViewElement`,
/// `NodeSelector`, or any protocol type gains new variants or fields
/// that old clients cannot handle correctly.
pub const PROTOCOL_VERSION: u32 = 1;

// ── Core identifiers ────────────────────────────────────────────────────────

/// `"org/plugin-name"` — globally unique plugin identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct PluginId(pub String);

/// Opaque handler reference embedded in a `PluginView` event handler.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HandlerId(pub String);

/// Session identifier (maps to a user session, HTTP request, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

/// A URL path pattern with `:param` segments, e.g. `"/product/:id"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RoutePattern(pub String);

impl RoutePattern {
    /// Returns `true` if `path` matches this pattern.
    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        self.extract_params(path).is_some()
    }

    /// Extracts named parameters from `path` if it matches, or `None`.
    #[must_use]
    pub fn extract_params(&self, path: &str) -> Option<HashMap<String, String>> {
        let pattern_segs: Vec<&str> = self.0.split('/').collect();
        let path_segs: Vec<&str> = path.split('/').collect();

        if pattern_segs.len() != path_segs.len() {
            return None;
        }

        let mut params = HashMap::new();
        for (pat, seg) in pattern_segs.iter().zip(path_segs.iter()) {
            if let Some(name) = pat.strip_prefix(':') {
                params.insert(name.to_owned(), (*seg).to_owned());
            } else if *pat != *seg {
                return None;
            }
        }
        Some(params)
    }
}

// ── Client capabilities ──────────────────────────────────────────────────────

/// Sent by the client with every server function call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClientCapabilities {
    /// `PROTOCOL_VERSION` the client was compiled against.
    pub protocol_version: u32,
    /// Host app's own version integer.
    pub app_version: u32,
    /// `HostComponent` names this client has registered and can render.
    pub registered_host_components: Vec<String>,
}

impl ClientCapabilities {
    /// `ClientCapabilities` for use in SSR contexts where there is no real client.
    ///
    /// Uses the current host protocol version and declares no host components,
    /// so plugins that require specific client capabilities are treated as incompatible.
    #[must_use]
    pub const fn default_ssr() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            app_version: 0,
            registered_host_components: vec![],
        }
    }
}

/// Provided as context by `PluginBootProvider` when loaded plugins require
/// a newer client than what is connected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateRequired {
    pub current_protocol: u32,
    pub required_protocol: u32,
    pub current_app: u32,
    pub required_app: u32,
    /// Which specific plugins triggered the requirement.
    pub blocking_plugins: Vec<PluginId>,
}

/// Per-plugin client requirements, embedded in `OverrideMap`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginClientRequirement {
    pub min_protocol_version: u32,
    pub min_app_version: u32,
    /// `HostComponent` names this plugin references.
    pub required_host_components: Vec<String>,
}

// ── Priority ─────────────────────────────────────────────────────────────────

/// A plugin's suggested position in any ordered sequence.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum PriorityHint {
    /// Must run before all others. For security, auth, rate-limiting.
    First,
    /// Earlier than average, but not exclusively first.
    High,
    /// Default. No strong ordering preference.
    #[default]
    Normal,
    /// Later than average, but not exclusively last.
    Low,
    /// Must run after all others. For analytics, logging, auditing.
    Last,
}

impl PriorityHint {
    /// Maps the hint to a numeric bucket used for ordering.
    #[must_use]
    pub const fn as_numeric(&self) -> i32 {
        match self {
            Self::First => 1000,
            Self::High => 750,
            Self::Normal => 500,
            Self::Low => 250,
            Self::Last => 0,
        }
    }
}

// ── Plugin manifest ──────────────────────────────────────────────────────────

/// Returned by every plugin's `manifest` export.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    pub id: PluginId,
    pub version: String,
    /// Minimum `PROTOCOL_VERSION` the connecting client must have.
    pub min_protocol_version: u32,
    /// Minimum host app version this plugin requires.
    pub min_app_version: u32,
    /// `HostComponent` names this plugin references in its `PluginView` output.
    pub required_host_components: Vec<String>,
    pub state_scope: StateScope,
    pub slots: Vec<SlotRegistration>,
    pub hooks: Vec<HookRegistration>,
    pub event_subscriptions: Vec<String>,
    pub transforms: Vec<TransformDeclaration>,
    pub host_capabilities: Vec<HostCapability>,
    /// Inbound HTTP API routes this plugin wants to handle.
    #[serde(default)]
    pub api_routes: Vec<ApiRouteDeclaration>,
    /// New view pages this plugin provides (served under the host's catch-all prefix).
    #[serde(default)]
    pub page_routes: Vec<PageRouteDeclaration>,
    /// Host-defined manifest extensions. Each key is a namespace (e.g. `"my-host.feature-x"`);
    /// the value is opaque JSON owned entirely by the host.
    #[serde(default)]
    pub extensions: BTreeMap<String, serde_json::Value>,
}

/// State scope declared by a plugin in its manifest.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[non_exhaustive]
pub enum StateScope {
    #[default]
    PerSession,
    Global,
    Hybrid {
        global_keys: Vec<String>,
        session_keys: Vec<String>,
    },
}

/// Declares one slot this plugin contributes to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotRegistration {
    pub name: String,
    pub priority_hint: PriorityHint,
}

/// Declares one hook this plugin intercepts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRegistration {
    pub hook_name: String,
    pub priority_hint: PriorityHint,
}

/// A host capability the plugin requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum HostCapability {
    Http { allowed_hosts: Vec<String> },
    GlobalStateRead { keys: Vec<String> },
    GlobalStateWrite { keys: Vec<String> },
    ReadPluginState { plugin_id: PluginId, keys: Vec<String> },
    /// Request permission to call named host-side invocations.
    Invoke { names: Vec<String> },
    /// A host-defined capability class. The `namespace` identifies the type of
    /// capability; `value` is opaque JSON interpreted entirely by the host's
    /// registered [`CapabilityCheckFn`].
    ///
    /// If no check is registered for `namespace` at load time, the capability is
    /// **denied** by default.
    Custom {
        namespace: String,
        value: serde_json::Value,
    },
}

// ── Selectors ────────────────────────────────────────────────────────────────

/// Addresses a point in the rendered layer that a transform can target.
///
/// The three layers map to increasing dynamism — see the architecture doc §1.7.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Selector {
    // ── Layer 1 ──────────────────────────────────────────────────────────────
    /// A named `#[overridable]` or `OverridableComponent` boundary.
    Component(String),
    /// A `PluginSlot` by name — targets the slot's contribution list.
    Slot(String),
    /// An element carrying `data-plugin-slot="value"` inside any `PluginView` tree.
    DataPluginSlot(String),

    // ── Layer 2 ──────────────────────────────────────────────────────────────
    /// Targets the rendered output of a route matching this pattern.
    Route(RoutePattern),

    // ── Layer 3 ──────────────────────────────────────────────────────────────
    /// Selects nodes within the `PluginView` tree produced by `outer`.
    Within {
        outer: Box<Self>,
        inner: NodeSelector,
    },

    // ── Composition ──────────────────────────────────────────────────────────
    /// Applies to any selector in the list that matches.
    Any(Vec<Self>),
}

/// Selects specific nodes within a `PluginView` tree.
///
/// Default traversal is shallow (direct children of the outer selection only).
/// Wrap any selector in [`NodeSelector::Recursive`] to match at any depth.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NodeSelector {
    /// Matches any element with this HTML tag.
    Tag(String),
    /// Matches any element whose class list contains this class.
    HasClass(String),
    /// Matches any `HostComponent` reference with this name.
    HostComponent(String),
    /// Matches any element whose `name` field equals this string.
    Name(String),
    /// Matches any element carrying `data-<key>="<value>"`.
    DataAttr(String, String),
    /// Matches the first child of the outer selection.
    First,
    /// Matches the last child of the outer selection.
    Last,
    /// Matches the child at this 0-based index.
    Index(usize),
    /// Both inner selectors must match.
    And(Box<Self>, Box<Self>),
    /// Either inner selector must match.
    Or(Box<Self>, Box<Self>),
    /// Apply the inner selector recursively at any depth in the tree.
    Recursive(Box<Self>),
}

// ── Transform types ──────────────────────────────────────────────────────────

/// Declares a dynamic extension: select a point, apply an operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformDeclaration {
    pub selector: Selector,
    /// Plugin export name to call at render time.
    pub transform_fn: String,
    pub op: TransformOp,
    pub priority_hint: PriorityHint,
}

/// The operation a transform performs on the selected target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TransformOp {
    // ── Route / slot level (Layers 1 & 2) ────────────────────────────────────
    /// Render the plugin view before the selected output.
    InjectBefore,
    /// Render the plugin view after the selected output.
    InjectAfter,
    /// Wrap the selected output. The plugin view may embed
    /// `HostComponent("__content__")` as a placeholder for the original.
    /// Multiple Wrap plugins form a sequential pipeline (see §1.5).
    Wrap,

    // ── Node level (Layer 3) ──────────────────────────────────────────────────
    /// Replace the selected node entirely with the plugin view.
    Replace,
    /// Wrap the selected node. The plugin view may embed
    /// `HostComponent("__target__")` as a placeholder for the original node.
    WrapNode,
    /// Insert the plugin view before the selected node.
    InsertBefore,
    /// Insert the plugin view after the selected node.
    InsertAfter,
    /// Add a CSS class to the selected node (no new view rendered).
    AddClass(String),
    /// Set an attribute on the selected node (no new view rendered).
    SetAttr(String, AttrValue),
}

/// Input passed to a plugin's transform export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformInput {
    /// Current content at the selected point (for Wrap/Replace/WrapNode ops).
    pub original: Option<PluginView>,
    pub context: TransformContext,
    pub session: SessionCtx,
}

/// Context available to a transform call.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TransformContext {
    pub route_params: HashMap<String, String>,
    pub component_props: Option<serde_json::Value>,
    pub slot_name: Option<String>,
    pub client: ClientCapabilities,
}

/// Output returned by a plugin's transform export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformOutput {
    pub view: PluginView,
}

// ── PluginView — the UI description tree ────────────────────────────────────

/// A serialisable virtual UI tree returned by plugin exports.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[non_exhaustive]
pub enum PluginView {
    Element(ViewElement),
    Text(String),
    /// Render a named host component with forwarded props.
    HostComponent(HostComponentRef),
    Fragment(Vec<Self>),
    #[default]
    Empty,
    /// Served when a plugin's version requirements exceed the client's capabilities.
    Incompatible {
        reason: String,
        fallback: Option<Box<Self>>,
    },
}

/// A virtual element node in a `PluginView` tree.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ViewElement {
    pub tag: String,
    /// Stable name for selector targeting.
    pub name: Option<String>,
    /// Stable key for keyed view diffing.
    pub key: Option<String>,
    pub attrs: Vec<(String, AttrValue)>,
    pub handlers: Vec<BoundEventHandler>,
    pub children: Vec<PluginView>,
}

/// A typed attribute value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub enum AttrValue {
    String(String),
    Bool(bool),
    Number(f64),
}

/// An event handler bound to a DOM event on a `ViewElement`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoundEventHandler {
    pub event: DomEvent,
    pub handler_id: HandlerId,
    pub debounce_ms: Option<u32>,
}

/// DOM events a plugin can listen for.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub enum DomEvent {
    Click,
    Input,
    Change,
    Submit,
    Focus,
    Blur,
    KeyDown,
    KeyUp,
}

/// A reference to a named host component with forwarded props and children.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct HostComponentRef {
    pub name: String,
    pub props: serde_json::Value,
    pub children: Vec<PluginView>,
}

// ── Hook types ───────────────────────────────────────────────────────────────

/// Input to a plugin's hook handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookCall {
    pub hook_name: String,
    pub context: serde_json::Value,
}

/// Result returned by a plugin's hook handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HookResult {
    Continue { context: serde_json::Value },
    Cancel { reason: String },
    Replace { context: serde_json::Value },
}

// ── Event bus types ──────────────────────────────────────────────────────────

/// A plugin-emitted or host-emitted event routed via the event bus.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEvent {
    pub source: EventSource,
    pub name: String,
    pub payload: serde_json::Value,
}

/// The origin of a `PluginEvent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum EventSource {
    Host,
    Plugin(PluginId),
}

// ── Slot content and interaction response ────────────────────────────────────

/// One plugin's contribution to a named slot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SlotContent {
    pub plugin_id: PluginId,
    pub priority: i32,
    pub view: PluginView,
}

/// Returned by a plugin's interaction handler.
///
/// Contains a new view to render and optional events to emit.
/// The view is applied as a keyed diff against the currently rendered `PluginView`,
/// using the `key` field on `ViewElement` nodes. Nodes with matching keys are updated
/// in place; nodes without keys whose position or tag changed are replaced. This
/// eliminates full-subtree flicker on interactions that only change a small part.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewUpdate {
    /// If `Some`, the `PluginViewRenderer` diffs this against the current view.
    /// If `None`, the current view is left unchanged (pure side-effect interaction).
    pub view: Option<PluginView>,
    pub events: Vec<PluginEvent>,
}

// ── Session context and override map ────────────────────────────────────────

/// Session and caller context threaded through every plugin call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCtx {
    pub session_id: SessionId,
    pub user_id: Option<String>,
    /// Capabilities of the client that initiated this request.
    pub client: ClientCapabilities,
    /// The plugin making this call, set by the host runtime.
    pub caller: Option<PluginId>,
}

impl Default for SessionCtx {
    fn default() -> Self {
        Self {
            session_id: SessionId(String::new()),
            user_id: None,
            client: ClientCapabilities::default(),
            caller: None,
        }
    }
}

/// Served once at app startup, cached in frontend context.
/// Updated via SSE when plugins are hot-reloaded.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OverrideMap {
    /// Monotonically increasing version. Higher = newer.
    pub version: u64,
    pub overridden_components: HashSet<String>,
    pub transformed_slots: HashSet<String>,
    pub route_patterns: Vec<RoutePattern>,
    /// Maximum `min_protocol_version` across all loaded plugins.
    pub required_protocol_version: u32,
    /// Maximum `min_app_version` across all loaded plugins.
    pub required_app_version: u32,
    /// Per-plugin requirements for targeted messaging.
    pub plugin_requirements: HashMap<PluginId, PluginClientRequirement>,
    /// URL prefix under which plugin page routes are served, e.g. `"/p"`.
    /// `None` when no page route prefix is configured.
    #[serde(default)]
    pub page_route_prefix: Option<String>,
}

/// Output of a full SSR render pass for one route.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SsrRouteOutput {
    pub route_transforms: SsrRouteTransforms,
    /// Pre-rendered slot contents keyed by slot name.
    pub slots: HashMap<String, Vec<SlotContent>>,
    /// Pre-rendered component resolutions keyed by component name.
    pub components: HashMap<String, Option<SsrComponentResolution>>,
}

/// SSR equivalent of `RouteTransforms` (serialisable for embedding into SSR HTML).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SsrRouteTransforms {
    pub before: Vec<PluginView>,
    pub wrap: Option<PluginView>,
    pub after: Vec<PluginView>,
}

/// Resolved transforms for one named component at render time.
///
/// Returned by `PluginRuntime::resolve_component` and used as the server function
/// return type so it is available on `wasm32-unknown-unknown`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentResolution {
    /// Views injected before the component's own output.
    pub before: Vec<PluginView>,
    /// If `Some`, replaces the component's own output entirely.
    pub replacement: Option<PluginView>,
    /// Views injected after the component's own output.
    pub after: Vec<PluginView>,
}

/// Route transforms resolved for the current path.
///
/// Returned by `PluginRuntime::render_route_transforms` and used as the server
/// function return type so it is available on `wasm32-unknown-unknown`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteTransforms {
    pub before: Vec<PluginView>,
    pub wrap: Option<PluginView>,
    pub after: Vec<PluginView>,
}

impl RouteTransforms {
    /// Returns an empty `RouteTransforms` with no contributions.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Returns `true` if all three partitions are empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.before.is_empty() && self.wrap.is_none() && self.after.is_empty()
    }

    /// Returns `true` if a Wrap transform was resolved for this path.
    #[must_use]
    pub const fn has_wrap(&self) -> bool {
        self.wrap.is_some()
    }
}

/// SSR equivalent of `ComponentResolution`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SsrComponentResolution {
    pub before: Vec<PluginView>,
    pub replacement: Option<PluginView>,
    pub after: Vec<PluginView>,
}

// ── HTTP fetch types ──────────────────────────────────────────────────────────

/// An outbound HTTP request made by a plugin via `dx_http_fetch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: HashMap<String, String>,
    pub body: Option<String>,
}

/// The response returned to a plugin after `dx_http_fetch`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

// ── Plugin API route types ────────────────────────────────────────────────────

/// HTTP method for a plugin-declared inbound API route.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    /// Returns the method as an uppercase ASCII string slice.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

/// One inbound HTTP API route declared by a plugin in its manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRouteDeclaration {
    pub method: HttpMethod,
    /// Full path, e.g. `"/api/notes"` or `"/api/notes/:id"`.
    /// Param segments use `:param` syntax (same as `RoutePattern`).
    pub path: String,
    /// The WASM export name to invoke when this route is hit.
    /// The export must accept `Json<ApiRequest>` and return `Json<ApiResponse>`.
    pub handler_fn: String,
}

impl ApiRouteDeclaration {
    /// Declare a `GET` route handled by `handler_fn`.
    pub fn get(path: impl Into<String>, handler_fn: impl Into<String>) -> Self {
        Self { method: HttpMethod::Get, path: path.into(), handler_fn: handler_fn.into() }
    }
    /// Declare a `POST` route handled by `handler_fn`.
    pub fn post(path: impl Into<String>, handler_fn: impl Into<String>) -> Self {
        Self { method: HttpMethod::Post, path: path.into(), handler_fn: handler_fn.into() }
    }
    /// Declare a `PUT` route handled by `handler_fn`.
    pub fn put(path: impl Into<String>, handler_fn: impl Into<String>) -> Self {
        Self { method: HttpMethod::Put, path: path.into(), handler_fn: handler_fn.into() }
    }
    /// Declare a `PATCH` route handled by `handler_fn`.
    pub fn patch(path: impl Into<String>, handler_fn: impl Into<String>) -> Self {
        Self { method: HttpMethod::Patch, path: path.into(), handler_fn: handler_fn.into() }
    }
    /// Declare a `DELETE` route handled by `handler_fn`.
    pub fn delete(path: impl Into<String>, handler_fn: impl Into<String>) -> Self {
        Self { method: HttpMethod::Delete, path: path.into(), handler_fn: handler_fn.into() }
    }
}

/// Request payload delivered to a plugin's API handler export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequest {
    /// Path parameters extracted by the router, e.g. `{"id": "42"}`.
    pub path_params: HashMap<String, String>,
    /// Query string parameters, e.g. `{"page": "2"}`.
    pub query_params: HashMap<String, String>,
    /// Request headers as lowercase-name → value pairs.
    pub headers: HashMap<String, String>,
    /// JSON body, if present and parseable. `None` for GET/DELETE or empty bodies.
    pub body: Option<serde_json::Value>,
}

/// Response returned by a plugin's API handler export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse {
    /// HTTP status code, e.g. `200`, `404`.
    pub status: u16,
    /// Additional response headers to include.
    pub headers: HashMap<String, String>,
    /// JSON body. Omit or set to `None` for empty responses.
    pub body: Option<serde_json::Value>,
}

impl Default for ApiResponse {
    fn default() -> Self {
        Self { status: 200, headers: HashMap::new(), body: None }
    }
}

// ── Plugin page route types ───────────────────────────────────────────────────

/// One view page route declared by a plugin in its manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRouteDeclaration {
    /// Path relative to the host's catch-all prefix, e.g. `"/notes"` or `"/notes/:id"`.
    /// Uses `:param` syntax for path parameters.
    pub path: String,
    /// Optional page title (host may use for `<title>` or breadcrumbs).
    pub title: Option<String>,
    /// WASM export name: `fn(Json<PageRouteInput>) -> FnResult<Json<PluginView>>`.
    pub render_fn: String,
    /// When `true` the plugin wants a full-page render — the host should skip its normal
    /// layout wrapper. Read from `PageRouteOutput` and act on it in the host component.
    #[serde(default)]
    pub bypass_layout: bool,
}

/// Input delivered to a plugin's page route render export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRouteInput {
    /// Path parameters extracted from the declared pattern, e.g. `{"id": "42"}`.
    pub path_params: HashMap<String, String>,
    /// Query string parameters.
    pub query_params: HashMap<String, String>,
    /// Caller's session.
    pub session: SessionCtx,
}

/// Returned by the `get_plugin_page` server function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageRouteOutput {
    /// The rendered view — pass to `PluginViewRenderer`.
    pub view: PluginView,
    /// Whether the plugin requested full-page rendering (no host layout).
    pub bypass_layout: bool,
    /// Optional page title set by the plugin.
    pub title: Option<String>,
}
