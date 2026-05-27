# Session Briefing — dioxus-extism

This document briefs a fresh Claude Code session working on `dioxus-extism` for the host-agnostic extensions initiative.

---

## Where we are

A multi-repo plugin-platform initiative needs dioxus-extism to grow five new mechanisms. The RFC is at `docs/rfcs/host-agnostic-extensions.md` — read it first.

**Status**: RFC drafted, no implementation yet.

---

## Hard rule: dioxus-extism is host-agnostic

**Never** introduce vocabulary from a specific host application into this library. Concretely, **do not** add:

- Event sourcing, aggregates, projections.
- Domain event buses.
- Permission strings or trust tiers as fixed enums.
- Authentication / authorization concepts beyond the generic capability + signature mechanisms in the RFC.
- Any references to a particular application's routes, page names, or schema.

If the work needs *any* of those concepts, you are doing the wrong work — that belongs in a host crate, not here. The RFC explicitly lists what is and isn't in scope.

---

## Branch

Feature branch: `claude/admiring-goldberg-mijsy`.

---

## Recommended implementation order

1. ~~**§1 Generic Manifest Extensions**~~ ✓ done
2. ~~**§2 Generic Plugin-Function Dispatch**~~ ✓ done
3. ~~**§3 Host-Defined Capability Classes**~~ ✓ done
4. ~~**§4 Route-Level `TransformOp::RouteReplace`**~~ ✓ done
5. ~~**§5 Opaque Trust Tag**~~ ✓ done
6. ~~**§6 Plugin Registry API**~~ ✓ done
7. ~~**§7 Observability**~~ ✓ done

Each can ship in its own commit/PR (Conventional Commits).

---

## Suggested first CLI prompt

> "Read docs/rfcs/host-agnostic-extensions.md. We'll start with §1 (generic manifest extensions). Before writing code, find how PluginManifest is parsed and loaded today — file paths and key types — then propose the minimal API for `register_manifest_extension` and the handler trait. Show me the proposal first, then we'll implement."

---

## Coordination with the host repo

A separate session (different terminal) works on the host repo that drives these requirements. Cross-repo iteration uses a Cargo path override in the host repo's `Cargo.toml` pointing at this local checkout. No coordination action is needed from this side beyond keeping the RFC's contract stable. If the host repo asks for a change that *would* introduce host-specific vocabulary into dioxus-extism, push back and propose extending the generic mechanism instead.
