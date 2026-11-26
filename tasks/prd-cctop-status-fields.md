# PRD 2: the status-line fields cctop drops

**Status:** v1.0 · 2026-11-26 — ready for implementation.
**Target:** cctop after v0.9.1; Rust (status collector, TUI, `cctop query`, MCP) plus the pane's existing renderers.
**Depends on:** nothing. Independent of PRD 0.
**Source:** the status-line payload builder in the Claude Code 2.1.332 binary (`w9r` / `v9r`), and the live tee at `~/.cctop/status/<session>.json` on the maintainer's machine (keys and types only).

> Decisions taken with the user on 2026-11-26:
> 1. Built first, with PRD 1: cheap, and the miss-cause detail feeds the same A01 nudge.

---

## 1. Introduction

`cctop statusline-shim` tees Claude Code's status-line JSON to `~/.cctop/status/<session>.json`. `status::Sample` (`src/status.rs:158–189`) parses it, and `State::apply_status` (`src/ui/state.rs:1104`) keeps what it uses. Unknown fields are ignored. The 2.1.332 payload carries these fields that cctop drops:

| Field | Present when | What it is | Read today? |
|---|---|---|---|
| `prompt_cache.last_miss_cause.tools_added`, `.tools_removed` | the last miss changed the tool list | counts | no (`MissCause` has only `causes`, `status.rs:107`) |
| `prompt_cache.last_miss_cause.system_char_delta` | the last miss changed the system prompt | signed chars | no |
| `prompt_cache.requests`, `cache_write_tokens`, `miss_recache_tokens` | always with `prompt_cache` | counts | parsed, not surfaced |
| `rate_limits.spend_limit.used_usd`, `.limit_usd`, `.period` | provider `gateway` with an overage and a USD figure | dollars, period word | no (`Limit` has `used_percentage`, `resets_at`) |
| `rate_limits.spend_limit.resets_at` | as `spend_limit` | unix seconds | parsed into `Limit`, unused (`state.rs:1148` keeps the % only) |
| `workspace.project_dir`, `.added_dirs[]`, `.git_worktree`, `.repo{host, owner, name}`, `.cloud_dir` | always / when set | paths, repo slug | no (`workspace` not modelled) |
| `output_style.name` | always | style name (`default`, `Concise`, …) | no |
| `agent.name` | the main thread runs as an agent (`--agent`) | name | no |
| `remote.session_id` | the session is remote-controlled | id | no |
| `pr.url`, `pr.kind` | a PR is linked | URL, kind | `pr` kept raw; only `number`, `review_state` read |
| `exceeds_200k_tokens` | always | bool | stored in `StatusFacts` (`state.rs:259`), **never shown** |
| `vim.mode` | vim mode on | `INSERT`/`NORMAL` | no; out of scope (below) |

Claude Code computes these from its own state, so they are exact where cctop otherwise infers. Example: `added_dirs` explains reads outside the project in the files view; `output_style` explains a `querySource` suffix (`repl_main_thread:outputStyle:Concise`, PRD 1).

## 2. Goals

- The cache card and A01 say *how much* the tool list or system prompt changed on the last miss, when only the shim knows (transcripts < 2.1.302, or a miss PRD 1 could not locate).
- The spend limit shows dollars and its reset, not just a percentage, when Claude Code has them.
- The session header shows the workspace facts a person uses to tell sessions apart: repo, worktree, added dirs, output style, agent name.
- The long-context flag is visible where pricing is shown.

## 3. User stories

### US-001: Parse the dropped fields
**Description:** As a developer, I want `status::Sample` to model every field in §1's table except `vim`, so that `apply_status` can use them.

**Acceptance criteria:**
- [ ] `MissCause` gains `tools_added: Option<u64>`, `tools_removed: Option<u64>`, `system_char_delta: Option<i64>`.
- [ ] `Limit` gains `used_usd`, `limit_usd: Option<f64>`, `period: Option<String>`. Five-hour and seven-day deserialise unchanged.
- [ ] `Sample` gains `workspace: Option<Workspace{current_dir, project_dir, added_dirs: Vec<String>, git_worktree: Option<String>, repo: Option<Repo{host, owner, name}>, cloud_dir}>`, `output_style: Option<Named>`, `agent: Option<Named>`, `remote: Option<RemoteInfo{session_id}>`.
- [ ] `Sample::pr_url()`, `pr_kind()` helpers next to `pr_number()`.
- [ ] The `FULL` test payload (`status.rs:335`) is extended with every new field, and a test asserts each parses. A payload missing all of them still parses (test).
- [ ] `cargo test status` passes.

### US-002: Keep them in state
**Description:** As a developer, I want `apply_status` to carry the new facts into `State`, so that surfaces read one place.

**Acceptance criteria:**
- [ ] `CacheState` gains `last_miss_tools_added`, `last_miss_tools_removed`, `last_miss_system_delta`, `requests`, `miss_recache_tokens`.
- [ ] `StatusFacts` gains `spend_limit_usd: Option<(f64 used, f64 limit)>`, `spend_limit_period`, `spend_limit_resets_at_ms`, `workspace` (as parsed), `output_style`, `agent_name`, `remote_session_id`, `pr_url`, `pr_kind`.
- [ ] A later sample without a field does not clear a value an earlier sample set, except `spend_limit_*`, which follows the latest sample as `spend_limit_pct` does today (`state.rs:1148`). Tests for both.

### US-003: Show them
**Description:** As a person using cctop, I want the new facts where I already look, so that I need no new view.

**Acceptance criteria:**
- [ ] **Cache card / A01:** when the last miss has `tools_added`/`tools_removed`/`system_char_delta` and PRD 1 gave no located cause for it, A01's `{what}` reads `tools +2 −0` / `system +1.2k chars`. Snapshot updated.
- [ ] **Limits panel** (`ui/panels/limits.rs:130`) and the dashboard limits cell (`dashboard.rs:952`): `spend $412 / $500 · monthly · resets 3 Dec` when USD is present, else today's %. Snapshot test with and without USD.
- [ ] **Session header / sources view:** repo as `owner/name`, worktree name, `+N dirs` when `added_dirs` is non-empty, output style when not `default`, `agent: <name>` when set, `remote` badge when `remote.session_id` is set. Each item is dropped first-to-last as width shrinks. Snapshot at 80 and 120 columns.
- [ ] **Cost panel:** a `>200k` marker beside the model when `exceeds_200k_tokens` is true. The tooltip/caveat names long-context pricing; the existing `long_context_pct` flag (`metrics/cost.rs:575`) is unchanged.
- [ ] `cctop query dashboard` and `cctop query summary` carry the new keys, each tagged with a registry id. `metrics/registry.rs` gains `spend_limit_usd`, `workspace_added_dirs`, `output_style`; docs regenerated; the freshness test passes.
- [ ] `coach.rs` / `dashboard.rs` text stays within the fixed widths; `scripts/pane-fixtures.sh fixtures/session-b.jsonl b` rerun; `npm test` green. The pane shows the same characters with no pane code change, because it renders the dashboard object verbatim.
- [ ] `make check` green.

## 4. Functional requirements

- FR-1: cctop must parse every field in §1's table except `vim`, ignoring absent ones.
- FR-2: The spend-limit cell must show used / limit in USD, the period and the reset when the payload carries them.
- FR-3: A01 must use the shim's tool/system deltas when no transcript-located cause exists.
- FR-4: The session header must show repo, worktree, added-dir count, non-default output style, agent name and remote state, degrading by width.
- FR-5: `exceeds_200k_tokens` must be visible next to the model in the cost panel.

## 5. Non-goals

- `vim.mode`: not a session-health fact.
- `cloud_dir`: parsed, not shown in v1.
- Anything that needs the shim to change. The shim already tees the whole payload.

## 6. Technical considerations

- `spend_limit` USD appears only for provider `gateway` with an overage and a USD-currency spend figure (binary: `be()==="gateway"&&xt.overage`). Most accounts never see it, so tests must not assume it.
- Paths in `workspace` are the person's own folders. cctop already shows `cwd`. Show `added_dirs` as a count in the header and as paths only in the sources view (`s`).
- `remote.session_id` is shown as a badge, never the id itself.

## 7. Success metrics

- Every field in §1's table except `vim` and `cloud_dir` is visible in at least one surface.
- No snapshot changes for a payload without the new fields.

## 8. Open questions

- None.
