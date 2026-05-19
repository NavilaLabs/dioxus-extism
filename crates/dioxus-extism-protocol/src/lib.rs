use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Monotonically increasing. Bumped whenever `PluginView`, `ViewElement`,
/// `NodeSelector`, or any protocol type gains new variants or fields
/// that old clients cannot handle correctly.
pub const PROTOCOL_VERSION: u32 = 1;

// ── Core identifiers ────────────────────────────────────────────────────────

/// `"org/plugin-name"` — globally unique plugin identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub fn matches(&self, path: &str) -> bool {
        self.extract_params(path).is_some()
    }

    /// Extracts named parameters from `path` if it matches, or `None`.
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
    pub fn as_numeric(&self) -> i32 {
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
}

impl Default for PluginId {
    fn default() -> Self {
        Self(String::new())
    }
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
}

// ── Selectors ────────────────────────────────────────────────────────────────

/// Addresses a point in the rendered layer that a transform can target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Selector {
    // Layer 1
    Component(String),
    Slot(String),
    DataPluginSlot(String),
    // Layer 2
    Route(RoutePattern),
    // Layer 3
    Within {
        outer: Box<Selector>,
        inner: NodeSelector,
    },
    // Composition
    Any(Vec<Selector>),
}

/// Selects specific nodes within a `PluginView` tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum NodeSelector {
    Tag(String),
    HasClass(String),
    HostComponent(String),
    Name(String),
    DataAttr(String, String),
    First,
    Last,
    Index(usize),
    And(Box<NodeSelector>, Box<NodeSelector>),
    Or(Box<NodeSelector>, Box<NodeSelector>),
    /// Apply inner selector recursively at any depth.
    Recursive(Box<NodeSelector>),
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
    InjectBefore,
    InjectAfter,
    Wrap,
    Replace,
    WrapNode,
    InsertBefore,
    InsertAfter,
    AddClass(String),
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub enum PluginView {
    Element(ViewElement),
    Text(String),
    /// Render a named host component with forwarded props.
    HostComponent(HostComponentRef),
    Fragment(Vec<PluginView>),
    Empty,
    /// Served when a plugin's version requirements exceed the client's capabilities.
    Incompatible {
        reason: String,
        fallback: Option<Box<PluginView>>,
    },
}

impl Default for PluginView {
    fn default() -> Self {
        Self::Empty
    }
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// Output of a full SSR render pass for one route.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsrRouteOutput {
    pub route_transforms: SsrRouteTransforms,
    /// Pre-rendered slot contents keyed by slot name.
    pub slots: HashMap<String, Vec<SlotContent>>,
    /// Pre-rendered component resolutions keyed by component name.
    pub components: HashMap<String, Option<SsrComponentResolution>>,
}

/// SSR equivalent of `RouteTransforms` (serialisable for embedding into SSR HTML).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SsrRouteTransforms {
    pub before: Vec<PluginView>,
    pub wrap: Option<PluginView>,
    pub after: Vec<PluginView>,
}

/// SSR equivalent of `ComponentResolution`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsrComponentResolution {
    pub before: Vec<PluginView>,
    pub replacement: Option<PluginView>,
    pub after: Vec<PluginView>,
}
