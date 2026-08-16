# Architecture (V0)

```
┌─────────────────────────────────────────────────────────────┐
│  Tauri 2 + React (dark Composer UI)                         │
│  Thought / quote-lines / PermissionBar / honest model menus │
└───────────────────────────┬─────────────────────────────────┘
                            │ invoke / events
┌───────────────────────────▼─────────────────────────────────┐
│  agent-daemon                                               │
│  startup banners · fixture replay · CLI demo orchestration  │
└───────┬─────────────────────┬───────────────────┬───────────┘
        │                     │                   │
┌───────▼────────┐   ┌────────▼────────┐   ┌──────▼──────────┐
│ agent-acp      │   │ agent-core      │   │ SQLite journal  │
│ OpenCode ACP   │   │ scheduler       │   │ events.seq PK   │
│ stdio JSON-RPC │   │ worktree/gate   │   │ soft-invalidate │
│ normalize →    │   │ merge/memory    │   │                 │
│ journal + UI   │   │ (no LLM)        │   │                 │
└────────────────┘   └─────────────────┘   └─────────────────┘
```

## Trust boundaries

| Layer | May call LLM? | Notes |
|-------|---------------|-------|
| Planner (YAML paste in V0) | human / later model | Only place model chooses graph shape |
| Scheduler / Gate / Merge | **no** | Exit codes + artifact edges only |
| ACP session | OpenCode only | No TUI scrape; unknown `_meta` ignored |

## Dependency edges

`task_deps.artifacts_json` declares required upstream artifacts. Bootstrap copies them into the downstream worktree. Missing artifact → `MissingArtifact` hard fail (never a silent green edge).

## Process pool key

Default: `(agent=opencode, executable+version, cwd, auth_fingerprint)`.
`model` / `thought_level` stay out of the key unless probing proves spawn-only injection via `OPENCODE_CONFIG_CONTENT` (not `acp --model`).

## Memory layers

- **L0** `l0_rules` — user standing rules, injected verbatim, audited via `rules_injected`
- **L1** `l1_facts_mock` — invalidate sets `invalid_at`, row remains
- **Proposals → L2** — Inbox approval required before `l2_bullets` insert

## Docs vs code

Protocol docs and OpenCode docs may drift. **This repository's code and tests are authoritative for V0 behavior.**
