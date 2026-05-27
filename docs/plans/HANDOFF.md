# Plugin Platform — Session Handoff

This document is a self-contained briefing so a fresh Claude Code session (CLI or web) can pick up the plugin-platform initiative without re-reading the entire prior conversation.

---

## TL;DR — What this initiative is

Turn **zeitrak** (closed activity-tracking app, Rust + Dioxus + eventually-rs CQRS/ES) into a **fully extensible platform**, with **dioxus-extism** as the host-agnostic plugin runtime. Plugins ship as WASM modules and can extend:

- **Backend**: commands, queries, domain-event reactions, application-service pre/post hooks, **plugin-authored event-sourced aggregates**, projections, API routes.
- **Frontend**: slot insertion, named-component replacement, route-wrap/inject/replace, completely new plugin pages.

**Strict design constraint**: dioxus-extism stays a **generic** plugin platform with **zero zeitrak vocabulary**. All zeitrak-specific concepts (aggregates, hooks, permissions, trust tiers) live in a new `zeitrak-plugin-host` crate and reach dioxus-extism only through generic extension mechanisms.

---

## Repos

- `NavilaLabs/zeitrak` — feature branch `claude/admiring-goldberg-mijsy`
- `NavilaLabs/dioxus-extism` — feature branch `claude/admiring-goldberg-mijsy`

Both are tracked in the working repo set.

---

## Where we are

**Phase 1 (architecture review) is complete and committed.**

- Deliverable: `docs/review/zeitrak-review-2026-05.md` (pushed to `claude/admiring-goldberg-mijsy`).
- Key insight: zeitrak is **far more complete** than the original plan assumed. 8/8 aggregates are effectively CRUD-complete; only **Invitation lacks dedicated integration tests**. Architecture is strictly onion, multi-tenancy is compile-time safe, JWT is hardened.
- Real prerequisites for the plugin platform shrank to a small set (see Findings below).

**Nothing else is implemented yet.** Phases 2–9 are planned but not started.

---

## The plan in one page

The full plan lives in `docs/plans/plugin-platform.md` (committed alongside this handoff). Phase summaries:

| Phase | Repo | Scope |
|---|---|---|
| 1 | zeitrak | ✅ DONE — architecture review |
| 2 | zeitrak | Tight prerequisite list: F1 (event upcasting, P0), F2–F6 (P1), plus PluginStorageService skeleton |
| 3 | zeitrak | New crate `zeitrak-plugin-host` (lifecycle, capability bridge, hooks, event bus, aggregate hosting, storage, trust) |
| 4 | zeitrak | Domain-event bus + application-service pre/post hooks |
| 5 | zeitrak | Plugin-authored event-sourced aggregates (zeitrak-specific manifest extension, hosted in zeitrak-plugin-host) |
| 6 | zeitrak | GUI slots, `#[overridable]` on key components, route-replace adoption, plugin page catch-all |
| 7 | **dioxus-extism** | **Host-agnostic only**: generic manifest-extension API, generic `call_plugin` dispatch, `HostCapability::Custom`, route-level `TransformOp::Replace`, generic Ed25519 trust-store, registry API. **No zeitrak concepts.** |
| 8 | zeitrak | Security: audit log, quotas, trust enforcement, isolation |
| 9 | zeitrak | Reference plugin `com.acme.leave-requests` demonstrating all three impact tiers |

### Recommended execution order
1. ✅ Phase 1 (review) — done.
2. F1 (event upcasting) — **P0 prerequisite for any plugin events**.
3. Phase 7 (dioxus-extism host-agnostic extensions) in parallel with F2–F6 (zeitrak P1 fixes).
4. Phase 3 (`zeitrak-plugin-host` scaffolding).
5. Phase 4 (event bus + hooks) → first reactive plugin possible.
6. Phase 5 (aggregate hosting) → constructive plugins possible.
7. Phase 6 (frontend slots/overrides/routes).
8. Phase 8 (security hardening).
9. Phase 9 (reference plugin + end-to-end verification).

---

## Phase 1 review — prioritized findings

From `docs/review/zeitrak-review-2026-05.md` §8:

| ID | Prio | Area | Finding | Recommendation |
|---|---|---|---|---|
| F1 | **P0** | Event store | `schema_version` exists in DDL but is never read; no upcasting. | Introduce `EventUpcaster` trait + dispatch in repository load path. |
| F2 | P1 | Event store | `Root::rehydrate_from_state(0, user)` bug forces full replay for user reads. | Pull real version from snapshot repository. |
| F3 | P1 | Auth | Admin role name `"admin"` hardcoded, case-sensitive. | Env var or dedicated `Permission::ADMIN_BYPASS`. |
| F4 | P1 | Aggregate | Invitation lacks integration tests. | Add suite matching other aggregates. |
| F5 | P1 | Aggregate | No unified snapshot strategy. | `snapshot_every` per aggregate as metadata. |
| F6 | P1 | Auth | No refresh-token flow; 1-hour hard cut. | Add refresh endpoint or document. |
| F7–F14 | P2 | various | User SoftDelete, Workspace Delete, FilterExpr DSL, JWT `kid`, list-endpoint perms, backend i18n, recovery tests, remote-branch hygiene. | Defer to post-platform as appropriate. |

---

## Critical design decisions (locked in)

1. **dioxus-extism stays host-agnostic.** No aggregate / event-sourcing / permission concepts in it. Host extends it via:
   - `PluginManifest::extensions: BTreeMap<String, serde_json::Value>` + host-registered `ManifestExtensionHandler` per namespace.
   - Generic `runtime.call_plugin::<I, O>(plugin_id, fn_name, input)` for arbitrary WASM exports.
   - `HostCapability::Custom(namespace, value)` for host-defined capability classes.
   - `TransformOp::Replace` added at the route level; gating is via host-supplied `RouteReplacePolicy` callback — dioxus-extism makes no policy decisions itself.
   - Trust verification produces an opaque `TrustTag`; tiers/policies live in the host.

2. **zeitrak-plugin-host is the boundary.** All zeitrak vocabulary (event-sourced aggregates, hooks, permissions, trust tiers) lives there. zeitrak-core stays I/O-free; the new crate sits between `zeitrak-infrastructure` and the `zeitrak` facade.

3. **Three plugin impact tiers**:
   - *Reactive*: subscribe to domain events (read-only).
   - *Interceptive*: pre/post hooks on application-service commands (can Continue/Cancel/Replace).
   - *Constructive*: own event-sourced aggregates, projections, API routes, UI slots/pages.

4. **Trust model (zeitrak policy, not dioxus-extism)**: Tenant-Plugin / Instance-Plugin / Signed Instance-Plugin. Mapping from opaque TrustTag to tier happens in zeitrak-plugin-host.

5. **Frontend extension mechanisms** (all already exist in dioxus-extism today, except the last one):
   - Slot insertion via `<PluginSlot name=".." />` ✅
   - Component replacement via `#[overridable]` ✅ (requires opt-in by host — zeitrak must annotate `TimesheetRow`, `ActivityCard`, `DashboardWidget`, etc.)
   - Route wrap/inject ✅
   - Full plugin page routes ✅
   - **Route replace (new — Phase 7)** for plugins to take over existing host routes.

---

## Files to read first when resuming

In this priority order:

1. `docs/plans/plugin-platform.md` — full plan with all architectural decisions, manifest examples, file lists.
2. `docs/review/zeitrak-review-2026-05.md` — the Phase 1 review with prioritized findings.
3. `CLAUDE.md` — zeitrak project rules (English-only, Conventional Commits, onion rules, etc.).
4. `zeitrak-core/src/plugin.rs` — current empty skeleton, will be replaced/extended.
5. The CLAUDE.md/AGENTS.md in `~/path/to/dioxus-extism` (for the dioxus-extism side).

---

## Suggested first CLI prompt to resume

> "Read docs/plans/plugin-platform.md and docs/review/zeitrak-review-2026-05.md. Phase 1 is committed. The next step is finding F1 — introducing an `EventUpcaster` trait and dispatching it in the `eventually-any`-backed repository load path so `schema_version` is finally honored. Before writing code, explore how the load path is currently structured in `zeitrak-infrastructure-impl/src/...` and propose a minimal API for the trait that fits the existing patterns. No code yet — show me the plan first."

That gets a fresh session into the work cleanly.

---

## Open items / parked questions

- **Remote-branch hygiene** (F14): `desktop`, `multi-user`, `old`, `refactor`, `refactoring` exist on origin but were never reconciled with `main`. `multi-user` may already contain invitations work that overlaps with what's now on main — needs an audit before any plugin code touches invitation flows.
- **Refresh-token flow** (F6): explicit decision needed whether to add it or accept the 1-hour session as a constraint.
- **Tenant-only constraint on plugin storage**: when plugins access `Pool<ScopeTenant>` directly through PluginStorageService, the API must make it impossible to reach `Pool<ScopeAdmin>` without an explicit trust check. Design pending.

---

*This handoff was generated 2026-05-27 from a web Claude Code session that completed Phase 1 of the plan. The plan file `docs/plans/plugin-platform.md` is the authoritative reference going forward.*
