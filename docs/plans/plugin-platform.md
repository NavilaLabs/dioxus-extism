# Vollständige Plugin-Erweiterbarkeit für zeitrak via dioxus-extism

## Kontext

zeitrak ist heute eine geschlossene Activity-Tracking-Anwendung mit klarer Onion + CQRS + Event-Sourcing Architektur. `zeitrak-core/src/plugin.rs` enthält nur ein leeres `ZeitrakPlugin`-Trait (id, version, permissions) — eine reine Skelettstruktur ohne tatsächlichen Erweiterungspunkt. Ziel ist es, zeitrak in eine **vollumfänglich erweiterbare Plattform** zu verwandeln, in der Plugins als WASM-Module (via Extism) sowohl **Backend-Logik** (Commands, Queries, Domain-Events, eigene Event-Sourced Aggregates, Projektionen) als auch das **Frontend** (Slots, Route-Transforms, neue Pages) sicher und kontrolliert erweitern können.

`dioxus-extism` ist die Plugin-Runtime (bereits sehr ausgereift: Slots, Transforms, Hooks, Events, Capabilities, Hot-Reload, Pool-basierte Extism-Integration mit Dioxus 0.7). Die Crate wird tiefgreifend erweitert um die für zeitrak benötigten Konzepte (vor allem: Event-Sourced Aggregate-Hosting für Plugins).

**Strategische Leitplanken (vom User bestätigt):**
1. **Hook-Tiefe**: Domain-Event-Bus **plus** Application-Layer Pre/Post-Hooks.
2. **Scope**: Plugins dürfen sowohl Admin- als auch Tenant-Aggregate erweitern, aber Admin-Capabilities sind durch Instance-Admin-Installation und Signatur gesperrt.
3. **Eigene Event-Sourced Aggregate**: Plugins müssen eigene Aggregate definieren können, die im Event Store von zeitrak gehostet werden.
4. **Review**: Eigene Phase mit Checklisten-Report vor Implementierung.
5. **API**: Volle CRUD + flexible Queries + dedizierte Plugin-Storage-API.

---

## Ist-Zustand (Kurz)

> **Update nach Phase-1-Review** (`docs/review/zeitrak-review-2026-05.md`): zeitrak ist deutlich vollständiger als ursprünglich angenommen. Aggregate sind faktisch CRUD-komplett (Activity/Timesheet/TimesheetTag/User/Workspace/WorkspaceRole/Invitation), `Permission` ist bewusst read-only. Onion strikt, Multi-Tenancy compile-time-sicher, JWT gehärtet, ProjectionRunner crash-resilient. Größte echte Lücke: **kein Event-Upcasting/Versionierung** (P0 für Plugin-Events). Phase 2 schrumpft entsprechend stark.

### zeitrak
- **Architektur**: Onion/Ports&Adapters, CQRS, Event Sourcing via `eventually-rs`/`eventually-any`/`eventually-projection`. **Konform**, zeitrak-core ist I/O-frei.
- **Aggregate-Vollständigkeit**: ✅ Activity, Timesheet, TimesheetTag, User, Workspace, WorkspaceRole, Invitation, Permission (read-only). Nur Detail-Lücken (User SoftDelete/Restore, Workspace Delete, einheitliche Filter-DSL).
- **Auth**: JWT HS256 (alg:none/Confusion abgewehrt), `RoleBasedPolicy` mit Admin-Bypass via Workspace-Rolle `"admin"` (hardcoded → P1-Finding).
- **Event-Store**: Idempotent persistiert, Snapshots vorhanden, Checkpoint-basierte Projektoren. **Kein Event-Upcasting** (P0-Finding F1).
- **Plugin-System**: nur `ZeitrakPlugin`-Trait (id/version/permissions) + `PluginRegistry::register/all_permissions` — **keine Runtime, keine Hooks, keine Extism-Integration.**

### dioxus-extism
- Produktionsreif: Pool-basiert (`extism::Pool` + `spawn_blocking`), 6 Crates, 5 Beispiele, 46 Fixtures.
- Vorhanden: `PluginManifest` mit Slots, Transforms, Hooks, ApiRoutes, PageRoutes, Events, `HostCapability`-System, Capability-Enforcement, Hot-Reload, SSR, `dx_state_*` / `dx_emit_event` / `dx_invoke` Host-Functions.
- Fehlend für zeitrak-Use-Case: **Event-Sourced-Aggregate-Hosting**, **Plugin-Storage-Tabellen**, **Trust/Signing**, **rsx!-natives Authoring in Plugins**, **Permission-Capability-Bridge zu Host-Apps wie zeitrak**.

---

## Architektur-Entscheidungen

### A1 — Plugin-Touchpoints liegen außerhalb von `zeitrak-core`
`zeitrak-core` bleibt I/O-frei. Alle Plugin-Hooks (Trait-Definitionen, Event-Bus, Aggregate-Registry) leben in einer neuen Crate **`zeitrak-plugin-host`** (zwischen `zeitrak-infrastructure` und `zeitrak`). Die Onion-Regel bleibt unverletzt: Plugins greifen **niemals** direkt auf Domain-Aggregate zu, sondern ausschließlich über Application-Services und Domain-Events.

### A2 — Plugins haben drei Wirkungsebenen, klar separiert
| Ebene | Mechanik | Beispiel |
|---|---|---|
| **Reaktiv** | Subscribe Domain Events (read-only, async) | "Slack-Notification on `TimesheetStopped`" |
| **Interzeptiv** | Pre/Post-Hooks auf Application-Service Commands (kann `Continue`/`Cancel`/`Replace`) | "Block timesheet stop wenn Beschreibung leer" |
| **Konstruktiv** | Eigene Event-Sourced Aggregate + eigene Projektionen + eigene API-Routes + eigene UI-Slots/Pages | "Custom Aggregate `LeaveRequest` mit eigener UI" |

### A3 — Capability-Bridge zwischen zeitrak und dioxus-extism
zeitrak-Permissions (`activity.create`, …) **und** dioxus-extism `HostCapability` werden vom Plugin-Manifest deklariert. Eine zentrale Mapping-Schicht in `zeitrak-plugin-host` übersetzt Capability-Checks auf zeitrak's `AuthorizationService::require_permission`. Default-Deny: ein Plugin-Aufruf, der keine deklarierte Capability hat, schlägt fehl.

### A4 — Trust-Modell (3 Stufen, zeitrak-Policy)
**Diese Stufen sind reine zeitrak-Policy** und werden in `zeitrak-plugin-host` definiert/durchgesetzt. dioxus-extism kennt nur den opaken Trust-Tag aus der Signatur-Verifikation (siehe Phase 7 Punkt 5).

| Stufe | Installierbar durch | Erlaubte Capabilities (zeitrak-Policy) |
|---|---|---|
| **Tenant-Plugin** | Workspace-Admin | Nur Tenant-Scope, kein Filesystem/Netz |
| **Instance-Plugin** | Instance-Admin (CLI) | Tenant + Admin Lesescope |
| **Signed Instance-Plugin** | Ed25519-Signatur eines konfigurierten Trust-Roots | Vollzugriff inkl. Admin-Write |

Die rohe Signaturverifikation (Ed25519) passiert beim Laden in `dioxus-extism-host` und produziert nur den Trust-Tag; zeitrak-plugin-host mappt diesen Tag plus den Installationskontext auf die obige Tabelle.

### A5 — Generischer Manifest-Extension-Mechanismus in dioxus-extism (host-agnostisch)
dioxus-extism kennt **keine** zeitrak-spezifischen Konzepte wie Event-Sourced-Aggregate. Stattdessen wird dioxus-extism um einen **generischen Host-Extension-Mechanismus** erweitert:

- `PluginManifest` bekommt ein Feld `extensions: BTreeMap<String, serde_json::Value>` (oder typisierte Variante über generics) für host-definierte Sektionen.
- Host-App (zeitrak) registriert beim `PluginRuntime`-Setup einen `ManifestExtensionHandler<T>` pro Extension-Namespace (z.B. `"zeitrak.aggregates"`, `"zeitrak.hooks"`, `"zeitrak.permissions"`).
- Beim Plugin-Load deserialisiert dioxus-extism die rohen JSON-Values und ruft die jeweiligen Handler des Hosts auf — Validation, Registry-Updates, Capability-Granting passieren im Host.
- dioxus-extism dispatcht zudem **arbiträre Plugin-Exports** an Host-Extensions: ein Handler kann sagen "ich brauche bei diesem Plugin den Export `aggregate_apply` als Callback verfügbar" → dioxus-extism stellt eine generische `call_plugin_function`-API bereit, die der Host beliebig nutzt.

Das hält dioxus-extism als reine Plattform und macht es für jeden Host nutzbar (zeitrak, ein CMS, eine IDE, …).

### A6 — Plugin-Eigene Event-Sourced Aggregate (rein zeitrak-spezifisch)
Aufbauend auf A5: zeitrak registriert eine Manifest-Extension `zeitrak.aggregates`. Plugins können dort ihre Aggregate beschreiben:
```toml
[[zeitrak.aggregates]]
name = "leave_request"
events = ["Submitted", "Approved", "Rejected"]
snapshot_every = 50
```
Die komplette Mechanik (Stream-Prefix, Event-Folding via WASM-Export, Projection-Bridge, Snapshot-Strategie) lebt vollständig in `zeitrak-plugin-host`. dioxus-extism stellt nur den Call-Mechanismus für die WASM-Exporte (`aggregate_apply`, `aggregate_handle_command`, `project`) zur Verfügung — semantisch interpretiert werden sie von zeitrak.

zeitrak persistiert die Plugin-Aggregate-Events im selben Event Store unter Stream-Prefix `plugin.<plugin_id>.<aggregate_type>.<id>`.

### A7 — Plugin-Storage-API (Read-Models)
Drei Speicheroptionen für Plugins:
1. **State-KV** (bereits in dioxus-extism: `dx_state_*`, `dx_global_state_*`) — für UI-Session-State.
2. **Plugin-Projection-Tables** — Plugins können eigene Read-Model-Tabellen via Migration deklarieren (`migrations/` im Plugin-Bundle, namespaced als `plugin_<id>__<name>`).
3. **Aggregate-Projection** — automatisch aus eigenem Event-Stream gebaut (siehe A5).

### A8 — Frontend-Erweiterbarkeit (Insertion + Replacement)
dioxus-extism unterstützt heute:
- **Slot-Insertion** via `<PluginSlot name="..." />` (mehrere Plugins können beitragen)
- **Komponenten-Replacement** via `#[overridable]` / `OverridableComponent` — Host markiert einzelne Komponenten explizit als ersetzbar, Plugins liefern via `TransformOp::Replace` mit `Selector::Component(name)` Ersatz.
- **Route-Wrap/Inject** via `RouteTransform` (Decorator-Pattern um existierende Host-Routes).
- **Komplett neue Plugin-Routes** via `PageRouteDeclaration`.

**Lücke**: Route-Transforms haben heute **kein `Replace`** — Plugins können bestehende zeitrak-Routes nicht komplett übernehmen. Phase 7 schließt diese Lücke.

Im zeitrak-GUI wird folgender Mix angewandt:
- An strategischen Stellen `<PluginSlot name="..." />` für additive Erweiterung.
- Wichtige Komponenten (z.B. `<TimesheetRow />`, `<ActivityCard />`, `<DashboardWidget />`) werden mit `#[overridable("name")]` markiert, sodass Plugins sie ersetzen können.
- Route-Replace nutzt das neue `TransformOp::Replace` aus Phase 7, um z.B. `/timesheet/:id` komplett von einem Plugin rendern zu lassen.

Strategische Slot-Locations (mindestens): `dashboard.widgets`, `sidebar.entries`, `activity.detail.tabs`, `timesheet.row.actions`, `settings.sections`, `admin.menu`, `command-palette.actions`.

Plugins können zusätzlich Page-Routes registrieren (Prefix `/plugin/<id>/...`).

---

## Phase 1 — Manuelles Review (Vorbedingung)

**Deliverable**: `docs/review/zeitrak-review-2026-05.md` mit folgender Checklist:

1. **Architektur-Konformität**
   - Onion-Dependency-Regel statisch verifizieren (script in `xtask` o.ä., oder `cargo-deny`-Regeln).
   - Kein direkter SQL-Zugriff aus `zeitrak/src/...` außerhalb von `authorization.rs`.
2. **Aggregate-Vollständigkeit-Audit**
   - Pro Aggregate Tabelle: Commands (Create/Update/Delete/Custom), Queries (ById/List/Filter), Events, Projections, API-Endpunkte, Permissions, Tests.
3. **Authentifizierung & Autorisierung**
   - JWT-Validation hardening (alg-Whitelist, kid-Support, Refresh-Token-Flow).
   - Verify: jeder API-Endpoint ruft `require_permission` mit korrektem Scope.
   - Admin-Bypass via Rolle `"admin"` → ggf. ersetzen durch dedizierte `Permission::ADMIN_BYPASS`.
4. **Event-Store-Integrität**
   - Snapshot-Strategie pro Aggregate, Replay-Performance.
   - Event-Versionierung / Upcasting-Strategie.
5. **Multi-Tenancy-Isolation**
   - Statisch: niemand mischt `Pool<ScopeAdmin>` und `Pool<ScopeTenant>` falsch.
   - Runtime: Workspace-ID wird konsistent aus Session extrahiert.
6. **Test-Coverage**
   - Liste fehlender Integration-Tests pro Aggregate; Multi-User-Branch-Status.
7. **Technische Schulden**
   - Multi-User-Branch mit `main` reconciliieren (Invitations, i18n).
   - `desktop`-Branch evaluieren.

Output ist ein Bericht mit priorisierter Fix-Liste. **Phase 2 startet erst, wenn Review-Findings adressiert oder bewusst zurückgestellt sind.**

---

## Phase 2 — Vorbedingungen + gezielte Lückenschließung

> **Geänderter Scope nach Phase 1**: Die ursprünglich angenommene große CRUD-Vervollständigung entfällt — die Aggregate sind bereits ausgebaut. Phase 2 reduziert sich auf die im Review (`docs/review/zeitrak-review-2026-05.md`) identifizierten echten Findings.

### P0 — Vor Plugin-Plattform-Launch zwingend
- **F1: Event-Upcasting/Versionierung** (`zeitrak-core`, `zeitrak-infrastructure-impl`):
  Trait `EventUpcaster` + Dispatch im Load-Pfad der `eventually-any` Repositories. `schema_version` aus dem DDL wird endlich aktiv gelesen. Vorbedingung dafür, dass Plugin-Events versioniert werden können.

### P1 — Während/parallel zur Plugin-Plattform
- **F2**: User-Aggregate `rehydrate_from_state(0, user)`-Bug fixen (`zeitrak-infrastructure-impl/src/sea_query_sqlx/admin/user/repositories.rs:1`).
- **F3**: Admin-Rollenname konfigurierbar (Env `ADMIN_ROLE_NAME`) oder durch `Permission::ADMIN_BYPASS` ersetzen — Voraussetzung für die Plugin-Trust-Stufen aus A4.
- **F4**: Invitation Integration-Tests ergänzen.
- **F5**: Einheitliche Snapshot-Strategie (`snapshot_every` pro Aggregate als Metadatum). Vorbild für Plugin-Aggregate-Snapshots.
- **F6**: Refresh-Token-Flow (oder bewusste Akzeptanz + Dokumentation der 1-h-Hardcut).

### P2 — Bei Bedarf (kann auch post-Plugin-Plattform)
- F7 (User SoftDelete/Restore, dedizierter ChangePassword), F8 (Workspace Delete), F9 (Filter/Page/Sort-DSL in `zeitrak-core/src/shared/query.rs`), F10 (JWT `kid`-Header), F11 (List-Endpunkt-Permissions), F12 (Backend-i18n), F13 (Snapshot/Recovery-Tests).

### Plugin-Storage-API (öffentlich exponiert) — bleibt Phase 2
Neuer Service `PluginStorageService` in `zeitrak-plugin-host`:
- `kv_get/set/delete(plugin_id, scope, key)` — wraps existing `dx_state_*`
- `migrate(plugin_id, sql)` — namespaced migrations (Tabellen-Prefix `plugin_<sanitized_id>__`)
- `query_raw(plugin_id, sql, params)` — nur eigene Tabellen erlaubt (Prefix-Check)

### Kritische Dateien (nur die noch nötigen)
- `zeitrak-core/src/event_upcaster.rs` (neu)
- `zeitrak-infrastructure-impl/src/sea_query_sqlx/admin/user/repositories.rs:1` (F2)
- `zeitrak/src/authorization.rs:52` (F3)
- `zeitrak-infrastructure-impl/tests/integration/invitation/...` (F4, neu)
- `zeitrak-core/src/shared/query.rs` (F9, neu, optional)

---

## Phase 2 (Alt) — Volle CRUD-Vervollständigung (entfällt größtenteils)

> Historisch zur Referenz behalten — durch Review überholt. Die folgende Tabelle/Liste wird **nicht mehr 1:1 umgesetzt**; sie dient nur als Checkliste, falls künftig Detail-Commands fehlen.

Ziel: jedes Aggregate hat ein konsistentes Command/Query-Set, sodass jede heute denkbare Plugin-Interaktion möglich ist.

### Pattern (pro Aggregate)
Etabliertes Pattern aus `activity/` und `timesheet/` ist die Vorlage:
- `domain/aggregates.rs`, `domain/events.rs`, `domain/interfaces.rs`
- `application/commands.rs`, `application/queries.rs`, `application/rows.rs`, `application/inputs.rs`
- `zeitrak/src/{admin|tenant}/<aggregate>.rs` mit Service-Funktionen
- API-Endpunkte in `zeitrak-presentation/gui/packages/api/src/<aggregate>/`
- Permissions als Konstanten in `zeitrak-core/src/permissions.rs`
- Tests: Unit (Domain) + Integration (`TestFixture`)

### Minimal-Set pro Aggregate

| Aggregate | Commands | Queries | Events |
|---|---|---|---|
| **User** | Create, UpdateProfile, UpdateEmail (mit Verifikation), ChangePassword, RequestPasswordReset, CompletePasswordReset, SoftDelete, Restore | ById, ByEmail, List(Filter, Page) | Created, ProfileUpdated, EmailChanged, PasswordChanged, SoftDeleted, Restored |
| **Workspace** | Create, UpdateSettings, Delete, AssignUserRole, RevokeUserRole, GrantDirectPermission, RevokeDirectPermission, RemoveUser | ById, ByUser, ListUsers, ListRoles | bereits da + WorkspaceDeleted |
| **WorkspaceRole** | Create, Update, Delete, GrantPermission, RevokePermission | ById, ListByWorkspace, ListPermissions | Created, Updated, Deleted, PermissionGranted, PermissionRevoked |
| **Permission** | Create, Update, Delete, BulkSeed | ById, ByName, List | Created, Updated, Deleted |
| **Invitation** | Create, Accept, Reject, Revoke, Resend | ById, ByToken, ListByWorkspace, ListByEmail | (aus `multi-user`-Branch übernehmen) |
| **Activity** | + Restore, BulkUpdate | + ListWithStats, ListByTag | (vorhanden) + Restored |
| **Timesheet** | + Delete (hard, nur Admin), BulkExport | + ListByDateRange, ListByActivity, AggregateByUser | (vorhanden) |
| **TimesheetTag** | Create, Update, Delete, AssignToTimesheet, RemoveFromTimesheet | ById, ListByWorkspace, ListByTimesheet | Created, Updated, Deleted, Assigned, Removed |

### Erweiterte Query-Endpunkte
Jeder List-Endpoint bekommt einheitliche Query-Parameter (kein generisches "Reflection" — explizit per Aggregate):
- `filter: Json<FilterExpr>` — typed filter DSL (Eq, In, Range, Like) pro Feld
- `sort: Vec<(field, dir)>`
- `page: { cursor: Option<String>, limit: u32 }`

Helper-Modul `zeitrak-core/src/shared/query.rs` definiert `FilterExpr`, `Page`, `Sort` einmalig.

### Permissions vervollständigen
Neue Konstanten in `zeitrak-core/src/permissions.rs`:
```
user.create, user.update, user.delete, user.read_all
workspace.create, workspace.update, workspace.delete
workspace_role.create, workspace_role.update, workspace_role.delete
permission.manage
invitation.create, invitation.accept, invitation.revoke
tag.create, tag.update, tag.delete
timesheet.delete, timesheet.read_all
```

### Plugin-Storage-API (öffentlich exponiert)
Neuer Service `PluginStorageService` in `zeitrak-plugin-host`:
- `kv_get/set/delete(plugin_id, scope, key)` — wraps existing `dx_state_*`
- `migrate(plugin_id, sql)` — namespaced migrations (Tabellen-Prefix `plugin_<sanitized_id>__`)
- `query_raw(plugin_id, sql, params)` — nur eigene Tabellen erlaubt (Prefix-Check)

**Kritische Dateien (Auswahl)**:
- `zeitrak-core/src/admin/user/{domain,application}/*`
- `zeitrak-core/src/admin/workspace/{domain,application}/*`
- `zeitrak-core/src/admin/workspace_role/{domain,application}/*`
- `zeitrak-core/src/admin/invitation/*` (merge aus `multi-user`)
- `zeitrak-core/src/tenant/timesheet_tag/{domain,application}/*`
- `zeitrak-core/src/shared/query.rs` (neu)
- `zeitrak/src/{admin,tenant}/*.rs`
- `zeitrak-presentation/gui/packages/api/src/<aggregate>/mod.rs`
- `zeitrak-infrastructure-impl/src/sea_query_sqlx/{admin,tenant}/<aggregate>/repositories.rs`

---

## Phase 3 — Neue Crate `zeitrak-plugin-host`

Trennt I/O-freie `zeitrak-core` von Plugin-Infrastruktur. Verantwortlich für:
- Plugin-Lifecycle (Discovery → Verify → Load → Init → Subscribe → Shutdown)
- Capability-Bridge zwischen zeitrak-Permissions und dioxus-extism `HostCapability`
- Domain-Event-Bus (Broadcast-Channel über alle Aggregate-Events)
- Application-Service-Hook-Registry
- Plugin-Aggregate-Hosting (siehe A5)
- Plugin-Storage-Service (siehe Phase 2)
- Trust-Store + Signaturprüfung

### Wichtige Module
```
zeitrak-plugin-host/src/
├── lib.rs                  // PluginHost facade
├── lifecycle.rs            // load/reload/unload, calls dioxus-extism PluginRuntime
├── manifest.rs             // Erweitertes Manifest (extends dioxus-extism PluginManifest)
├── trust.rs                // Ed25519 verify, trust-store
├── capabilities.rs         // ZeitrakCapability <-> HostCapability mapping
├── event_bus.rs            // tokio::broadcast<DomainEvent>
├── hooks.rs                // Pre/Post-Command hook dispatch
├── aggregate_host.rs       // Plugin-Aggregate registry + event-store integration
├── projector_bridge.rs     // Brings WASM projections into eventually-projection runner
├── storage.rs              // PluginStorageService
└── api.rs                  // axum router for plugin-contributed HTTP routes
```

### Zeitrak-Plugin-Manifest (saubere Schichtung)
Ein Plugin-Manifest besteht aus **zwei klar getrennten Teilen**:

**Teil A — dioxus-extism Core-Manifest** (host-agnostisch, von dioxus-extism geparst):
```toml
[plugin]
id = "com.acme.leave"
version = "0.1.0"

[trust]
signature = "..."         # optional Ed25519
signer = "..."

[[capabilities]]
type = "Custom"
namespace = "zeitrak.permission"
value = "timesheet.read_all"

[[ui_slots]]
slot = "dashboard.widgets"
priority = 100

[[routes]]
path = "/plugin/leave/dashboard"
```

**Teil B — zeitrak-Extension** (lebt unter `[extensions."zeitrak.*"]`, von zeitrak's Manifest-Extension-Handler geparst — siehe A5 / Phase 7 Punkt 1):
```toml
[extensions."zeitrak.app"]
min_version = "0.5"

[extensions."zeitrak.permissions"]
contributed = ["leave.submit", "leave.approve"]

[extensions."zeitrak.events"]
subscriptions = ["TimesheetStopped", "ActivityCreated"]

[extensions."zeitrak.hooks"]
command_hooks = [
  { service = "timesheet", command = "Stop", phase = "Pre" }
]

[[extensions."zeitrak.aggregates"]]
name = "leave_request"
events = ["Submitted", "Approved", "Rejected"]
snapshot_every = 50
```

dioxus-extism sieht Teil B als opaken JSON-Wert und reicht ihn an den von zeitrak registrierten `ManifestExtensionHandler` weiter. Damit kennt dioxus-extism **kein** zeitrak-Vokabular.

### Capability-Bridge
```rust
pub enum ZeitrakCapability {
    ReadPermission(String),     // requires zeitrak permission
    WritePermission(String),
    SubscribeDomainEvent { aggregate: String, event: String },
    HookCommand { service: String, command: String, phase: HookPhase },
    HostAggregate { name: String },
    OwnProjection,
    OwnTables,
    EmitEvent,
    AdminScope,                 // requires Instance-Admin install
}
```
Vor jedem `dx_invoke` / `dx_state_*` Call: Capability prüfen (Default-Deny). Admin-Capabilities zusätzlich gegen Trust-Stufe checken.

---

## Phase 4 — Domain Event Bus + Application Hooks

### Domain-Event-Bus
- `zeitrak-plugin-host::event_bus::EventBus` mit `tokio::sync::broadcast::Sender<DomainEvent>`.
- `DomainEvent` ist ein Enum (oder typed wrapper) das alle Aggregate-Events serialisierbar exponiert. Bevor das Event aus dem `eventually-any` Event-Store committet wird, wird es zusätzlich auf den Bus publiziert (**At-Least-Once nach commit** — Hook hängt sich an `eventually-any` Save).
- Subscriber sind Plugins; Crash eines Plugins blockiert nicht die Domain-Operation.
- Plugins erhalten Events über eine neue WASM-Export-Funktion `on_domain_event(name, payload)` (in PDK ergänzt).

### Application-Service-Hooks
Pro Application-Service-Methode ein Hook-Punkt. Pattern via Macro (`hookable!`) oder explizit:
```rust
pub async fn stop(&self, cmd: StopTimesheet) -> Result<TimesheetRow> {
    let cmd = self.hooks.pre("timesheet.stop", cmd).await?;     // can Cancel/Replace
    let result = self.inner_stop(cmd).await;
    self.hooks.post("timesheet.stop", &result).await;            // notify-only
    result
}
```
- `pre` returns `HookResult::{Continue(cmd), Cancel(reason), Replace(modified)}` (matches dioxus-extism's existing `HookResult`).
- `post` ist fire-and-forget (errors loggen).
- Hooks-Registry ist nach Topic indiziert; Reihenfolge per Priorität im Manifest.

### Kritische Dateien
- `zeitrak-plugin-host/src/event_bus.rs`, `hooks.rs` (neu)
- `eventually-any` Save-Wrapper in `zeitrak-infrastructure-impl/src/sea_query_sqlx/...` — sendet auf den Bus nach erfolgreichem Commit
- `zeitrak/src/{admin,tenant}/*.rs` — Application-Services bekommen Hook-Dispatch (Pre/Post)

---

## Phase 5 — Plugin-Eigene Event-Sourced Aggregates (zeitrak-spezifisch)

**Wichtig**: Dieses gesamte Feature ist eine **zeitrak-Manifest-Extension** und lebt vollständig in `zeitrak-plugin-host`. dioxus-extism muss kein Aggregate-Konzept kennen — es liefert nur den generischen Manifest-Extension-Mechanismus (A5) und den generischen WASM-Call-Dispatch (Phase 7 Punkt 2).

### Konzept
Der Event-Store ist ein **shared resource**. Plugins beschreiben in ihrer **zeitrak-spezifischen Manifest-Extension** `zeitrak.aggregates`, welche Aggregate sie einbringen. zeitrak vergibt ihnen einen reservierten Stream-Prefix.

### Event-Stream-Naming
- Core Aggregates: `tenant.activity.<uuid>`, `admin.user.<uuid>`
- Plugin Aggregates: `plugin.<plugin_id>.<aggregate_type>.<uuid>`

### Funktionsweise
1. Plugin-Manifest enthält Sektion `zeitrak.aggregates` (host-spezifische Extension).
2. zeitrak's registrierter `ManifestExtensionHandler` validiert das Schema und meldet die Aggregate-Typen an `zeitrak-plugin-host::aggregate_host`.
3. Plugin exportiert WASM-Funktionen, die zeitrak per **generischem** `runtime.call_plugin(...)` (Phase 7 Punkt 2) aufruft:
   - `<aggregate>__apply(Json<(state, event)>) -> Json<state>` — pure Folder
   - `<aggregate>__handle_command(Json<(state, command)>) -> Json<(events, error?)>` — Command Handler
4. zeitrak fährt einen **Plugin-Aggregate-Runtime-Wrapper** der das `Aggregate`-Trait von `eventually-rs` implementiert und alle Calls an WASM delegiert.
5. Persistierung läuft durch die existierende `eventually-any` Infrastruktur (kein Sonderpfad).
6. Snapshots: `snapshot_every` aus Manifest steuert die existierende Snapshot-Strategie.

### Plugin-Projektionen
- Plugin exportiert `<projection>__project(Json<(state, event)>) -> Json<state>`.
- `projector_bridge` in `zeitrak-plugin-host` wraps das als `Projector` für den existierenden `eventually-projection`-Runner.
- Read-Tabellen liegen in den Plugin-Migration-Tabellen.

### Plugin-Commands aus dem GUI
Neuer generischer API-Endpoint in zeitrak: `POST /api/plugin/<plugin_id>/aggregate/<type>/<id>/command` mit JSON-Body. Routing erfolgt durch `zeitrak-plugin-host::api`. Permissions: das Plugin deklariert pro Command eine Permission im Manifest.

### Optionale PDK-Helper (zeitrak-spezifisch, nicht in dioxus-extism)
zeitrak liefert eine **eigene** kleine Helper-Crate (z.B. `zeitrak-plugin-sdk`), die Macros wie `zeitrak_aggregate!{ ... }` und `zeitrak_projection!{ ... }` anbietet. Diese generieren die korrekt benannten WASM-Exports und Manifest-Snippets, ohne dioxus-extism's PDK zu belasten.

### Kritische Dateien
- `zeitrak-plugin-host/src/aggregate_host.rs` (neu)
- `zeitrak-plugin-host/src/projector_bridge.rs` (neu)
- `zeitrak-plugin-host/src/manifest_extensions.rs` (neu — definiert die `zeitrak.*` Extensions)
- `zeitrak-plugin-sdk/` (optionale neue Crate für Plugin-Autoren)

---

## Phase 6 — Frontend-Integration

### Slots im zeitrak-GUI
In `zeitrak-presentation/gui/packages/ui/src/` an strategischen Stellen `<PluginSlot name="..." />` einbauen. Strategische Slot-Locations (mindestens):
- `dashboard.widgets` (Home/Dashboard Page)
- `sidebar.entries` (Hauptnavigation, unter den Core-Items)
- `activity.detail.tabs` (Tabs in der Activity-Detail-View)
- `activity.list.toolbar.actions`
- `timesheet.row.actions` (Inline-Actions pro Timesheet-Row)
- `timesheet.detail.sections`
- `settings.sections` (User Settings Page)
- `workspace.settings.sections` (Admin)
- `admin.menu` (nur sichtbar für Workspace-Admins)
- `command-palette.actions` (sofern Command-Palette existiert; sonst optional)

### Komponenten-Overrides (Replacement einzelner Komponenten)
Damit Plugins existierende zeitrak-Komponenten ersetzen können (nicht nur Slots befüllen), werden strategische Komponenten mit dem `#[overridable("name")]`-Macro aus dioxus-extism-frontend ausgezeichnet. Mindestens:
- `TimesheetRow` (`#[overridable("zeitrak.timesheet.row")]`)
- `ActivityCard` (`#[overridable("zeitrak.activity.card")]`)
- `DashboardWidget` (`#[overridable("zeitrak.dashboard.widget")]`)
- `UserAvatar`, `WorkspaceSwitcher`, `Sidebar`, `TopBar`, `BreadcrumbBar`
- Form-Komponenten in Settings (`UserProfileForm`, `WorkspaceSettingsForm`)

Override-Berechtigung wird via Capability gegated (siehe Phase 7 Punkt 3 + Phase 8). Tenant-Plugins dürfen nur Tenant-Komponenten überschreiben.

### Plugin-Routes
Zentraler Catch-All-Route im GUI: `/plugin/:plugin_id/*rest` → ruft `zeitrak-plugin-host` → `dioxus-extism` `render_page`.

### Route-Transforms inkl. Route-Replace
zeitrak-Core-Routes (z.B. `/timesheet/:id`) sind über das existierende Transform-System wrap-/inject-fähig — kein zusätzlicher Code in zeitrak nötig. Zusätzlich nutzt zeitrak den neuen `TransformOp::Replace` aus Phase 7, sodass signierte Plugins ganze Routes übernehmen können (z.B. ein Custom-Timesheet-Editor). Replace ist standardmäßig **nur für signierte Instance-Plugins** freigeschaltet.

### Session-/Permission-Propagation
Vor jedem Plugin-Call wird der `CallCtx` mit aktuellen `ClientCapabilities` befüllt, abgeleitet aus der zeitrak-Session (User-ID, Workspace-ID, Permissions-Set, Admin-Flag). dioxus-extism's Capability-Enforcement greift dann automatisch.

### Kritische Dateien
- `zeitrak-presentation/gui/packages/ui/src/dashboard.rs` (+ andere Hauptviews) — Slots einfügen
- `zeitrak-presentation/gui/packages/ui/src/router.rs` — Catch-All-Route
- `zeitrak-presentation/gui/packages/api/src/plugin/*` (neu) — Server Functions, die `PluginHost` aufrufen

---

## Phase 7 — Erweiterungen in `dioxus-extism` (host-agnostisch)

**Leitprinzip**: dioxus-extism bleibt eine **generische** Plugin-Plattform. Keine zeitrak-spezifischen Konzepte (Aggregate, Event-Sourcing, Permissions-Strings) wandern in dioxus-extism. Stattdessen wird die Plattform so erweitert, dass Hosts ihr **eigenes** Plugin-Vokabular sauber draufsetzen können.

1. **Generische Manifest-Extension-API** (`dioxus-extism-protocol` + `-host`)
   - `PluginManifest` bekommt `extensions: BTreeMap<String, serde_json::Value>` für host-definierte Sektionen.
   - Host registriert pro Namespace einen Handler:
     ```rust
     runtime.register_manifest_extension::<ZeitrakAggregatesExt>("zeitrak.aggregates", handler);
     ```
   - Beim Plugin-Load deserialisiert dioxus-extism den rohen JSON-Wert und ruft den Handler. Validation/Registry-Updates passieren im Host.
   - Im PDK kann der Plugin-Autor host-spezifische Extensions ergänzen — Plugin-Crates können ein optionales Helper-Crate des Hosts nutzen, um typisiert zu schreiben.

2. **Generische Plugin-Function-Dispatch-API** (`dioxus-extism-host`)
   - Erweitert die existierende Mechanik um eine öffentliche `runtime.call_plugin::<I, O>(plugin_id, fn_name, input)`-API, damit Host-Extensions arbiträre WASM-Exports aufrufen können (z.B. zeitrak's `aggregate_apply`, `project`).
   - Damit kann zeitrak Aggregate-Folding, Projektionen, Custom-Commands etc. an die Plugins delegieren, ohne dass dioxus-extism diese Konzepte kennen muss.

3. **Generische Host-Capability-Definition** (`dioxus-extism-host`)
   - Heute ist `HostCapability` ein festes Enum. Wird erweitert um `HostCapability::Custom(String, serde_json::Value)`, sodass Hosts beliebige Capability-Klassen einführen können (z.B. `Custom("zeitrak.permission", "timesheet.read_all")`).
   - Capability-Check-Funktion erlaubt das Host-seitige Hook-in für Custom-Capabilities.

4. **Route-Level `TransformOp::Replace`** (`dioxus-extism-protocol` + `-frontend` + `-host`)
   - Heute kennt `TransformOp` für Route-Transforms nur `Wrap`, `InjectBefore`, `InjectAfter`. Erweitern um **`Replace`**, sodass ein Plugin eine bestehende Host-Route komplett übernehmen kann.
   - `PluginAwareRouter` resolver-Logik: `Replace` short-circuited die Host-Component und rendert ausschließlich die Plugin-View. Wenn mehrere Plugins `Replace` auf dieselbe Route deklarieren → höchste Priorität gewinnt (deterministische Tie-Break: Plugin-ID lexikalisch).
   - **Gating ist generisch**: Vor jedem Replace ruft `PluginAwareRouter` einen optionalen `RouteReplacePolicy`-Callback des Hosts auf (`fn(plugin_id, trust_level, route_pattern) -> bool`). Default-Implementierung lässt alles zu — Hosts können restriktivere Policies einbauen (z.B. nur signierte Plugins, nur bestimmte Routen). dioxus-extism trifft selbst keine Annahmen über "elevated capability" oder "signed plugin".
   - Bestehende `Wrap`/`Inject` bleiben unverändert.

5. **Trust-Store + Signature Verification** (`dioxus-extism-host`)
   - Ed25519, konfigurierbarer Trust-Root. Verifikation erzeugt einen **opaken** `TrustTag` (z.B. `{ verified: bool, signer_key_id: Option<String> }`) — **kein festes Enum** von Trust-Stufen.
   - Trust-Tag wird in `LoadedPlugin` festgehalten und dem Host als Read-Only-Metadatum gereicht. Welche Berechtigungen ein Trust-Tag freischaltet, entscheidet **ausschließlich** der Host über seine Custom-Capabilities und Policies (z.B. den `RouteReplacePolicy` aus Punkt 4).

6. **Native-Dioxus-Authoring in Plugins (Stretch)**
   - Heute baut man PluginView manuell. Ziel: `rsx!`-Macro, das nach `PluginView` lowert. Lieferung sinnvoll als separates Sub-Crate `dioxus-extism-rsx`.

7. **Performance/Observability**
   - Plugin-Call-Latency Metriken (per Plugin und Function-Name), Pool-Auslastung.

8. **Plugin-Registry-API**
   - List/Install/Uninstall/Enable/Disable als Library-API — Hosts (zeitrak) nutzen das für ihre Admin-UI.

**Was nicht in dioxus-extism kommt** (sondern in zeitrak-plugin-host):
- Event-Sourced-Aggregate-Konzept und Stream-Naming
- Domain-Event-Bus-Definition
- zeitrak-Permissions-Mapping
- Plugin-Storage-Tabellen-Konventionen

---

## Phase 8 — Sicherheit & Sandbox

### Layered Defense
1. **WASM-Sandbox** (Extism/wasmtime): kein FS, kein Netz außer durch Host-Functions.
2. **Capability-Default-Deny**: Manifest deklariert; Host prüft pro Call.
3. **Trust-Stufen** (A4): Tenant- vs Instance- vs Signed-Plugin.
4. **Audit-Log**: jeder privilegierte Plugin-Call (Hook, Aggregate-Write, Admin-Read) wird in eine `plugin_audit`-Tabelle geschrieben (Plugin-ID, User, Action, Outcome, Timestamp).
5. **Quotas**: Plugin-Call-Timeout (z.B. 5s Default), max. Pool-Slots pro Plugin, max. Memory per Plugin (Extism `MemoryOptions`).
6. **Isolation pro Workspace**: Plugin-State und Plugin-Tabellen sind workspace-scoped, sofern nicht explizit `GlobalScope` deklariert (und genehmigt).
7. **Plugin-Removal**: Soft-Disable (Events bleiben, keine neuen Calls), Hard-Remove (Trigger Cleanup-Hook, optional Drop Tables).

### Code-Touchpoints
- `zeitrak-plugin-host/src/trust.rs` + `capabilities.rs` + `audit.rs` (neu)
- `dioxus-extism-host/src/runtime.rs` — Quotas & Trust-Level beim Loading

---

## Phase 9 — Beispiel-Plugin + End-to-End-Verifikation

Ein konkretes Plugin `com.acme.leave-requests` bauen, das **alle drei Wirkungsebenen** demonstriert:

- **Konstruktiv**: eigenes Aggregate `leave_request` mit Events `Submitted/Approved/Rejected`, eigene Projection `pending_leaves`, eigener Page-Route `/plugin/leave/dashboard`.
- **Reaktiv**: subscribed `TimesheetStopped` → setzt Vacation-Day-Counter herunter.
- **Interzeptiv**: Pre-Hook auf `timesheet.start` → cancelt mit Begründung, wenn der User im Urlaub ist.
- **UI**: Slot `dashboard.widgets` zeigt "Pending Leave Requests", Slot `sidebar.entries` fügt "Urlaub" Eintrag hinzu.
- **Capabilities**: Tenant-Plugin (keine Admin-Capabilities) → installierbar von Workspace-Admin.

---

## Verifikation

| Was | Wie |
|---|---|
| Phase 1 Review-Findings | `docs/review/zeitrak-review-2026-05.md` Walkthrough mit User |
| Phase 2 CRUD | Pro Aggregate: Integration-Tests via `TestFixture`; manuelle GUI-Smoke-Tests |
| Phase 3-5 Plugin-Host | Unit-Tests in `zeitrak-plugin-host`; Integration-Test mit Fixture-Plugin |
| Phase 6 Frontend | Plugin laden → Slot rendert → Route erreichbar → Permission denied funktioniert |
| Phase 7 dioxus-extism | Bestehende 46 Fixtures laufen weiter; neue Fixtures für Aggregate-Hosting + Signing |
| Phase 8 Security | Negative Tests: Plugin ohne Capability schlägt fehl, unsigniertes Admin-Plugin abgelehnt, Quota-Überschreitung killt Call |
| Phase 9 E2E | `leave-requests`-Plugin durchspielen: Submit → Approve → Hook verhindert Timesheet → Slot zeigt Status |

---

## Aufteilung über die zwei Repos

| Repo | Hauptarbeit |
|---|---|
| **navilalabs/zeitrak** (Branch `claude/admiring-goldberg-mijsy`) | Phase 1 (Review), Phase 2 (CRUD), Phase 3 (`zeitrak-plugin-host` neu), Phase 4 (Hooks/Bus), Phase 5 (Aggregate-Host-Integration), Phase 6 (GUI-Slots), Phase 8 (Audit/Quotas), Phase 9 (Beispiel-Plugin) |
| **navilalabs/dioxus-extism** (Branch `claude/admiring-goldberg-mijsy`) | Phase 7 — **rein host-agnostisch**: Generischer Manifest-Extension-Mechanismus, generischer Plugin-Function-Dispatch, `HostCapability::Custom`, Trust-Store, Quotas/Metrics, optional `rsx!`. **Keine zeitrak-Konzepte.** |

Empfohlene Reihenfolge der Umsetzung:
1. Phase 1 (Review) → User-Approval der Findings
2. Phase 7 (dioxus-extism Protokoll-Erweiterungen) parallel zu Phase 2 (CRUD)
3. Phase 3 (Plugin-Host-Crate-Grundgerüst)
4. Phase 4 (Event-Bus + Hooks) → erstes lauffähiges Plugin möglich (reaktiv)
5. Phase 5 (Aggregate-Hosting) → konstruktive Plugins möglich
6. Phase 6 (Frontend-Slots)
7. Phase 8 (Security-Hardening)
8. Phase 9 (Beispiel-Plugin + Verifikation)
