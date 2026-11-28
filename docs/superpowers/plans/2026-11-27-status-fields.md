# Status-Line Fields Implementation Plan (PRD 2)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse and show the status-line fields cctop drops today: the last miss's tool/system deltas, the spend limit in dollars with its period and reset, the workspace facts (repo, worktree, added dirs, output style, agent, remote), the PR's url/kind, and the long-context flag.

**Architecture:** `status::Sample` models the fields; `State::apply_status` keeps them in `CacheState` / `StatusFacts`; the existing surfaces (A01, limits panel and cell, the dashboard header object, the cost panel, `cctop query`) read them. The pane changes only through the dashboard/coach objects it already draws verbatim.

**Tech Stack:** Rust (serde, ratatui, insta).

**Spec:** `tasks/prd-cctop-status-fields.md` (issue #26).

## Global Constraints

- Branch `status-fields`, cut from `request-ledger` after PRD 1 is complete (A01's fallback order needs PRD 1's located cause). Commit subjects `Status: …`, `Coach: …`, `Docs: …`, trailer in the body.
- `vim.mode` is not parsed; `cloud_dir` is parsed, never shown.
- A payload without the new fields changes no snapshot (PRD §7).
- A later sample without a field does not clear a value an earlier sample set — except `spend_limit_*`, which follows the latest sample like `spend_limit_pct` does today.
- Paths: `added_dirs` shown as a count in the header; as paths only in the sources view. `remote.session_id` is a badge, never shown.
- Spend USD appears only for gateway accounts with an overage: no test may assume it is present.
- `coach.rs` / `dashboard.rs` text within their fixed widths; after touching them rerun `scripts/pane-fixtures.sh && scripts/pane-fixtures.sh fixtures/session-b.jsonl b`; `npm test` green with no pane code change.
- Insta: `INSTA_UPDATE=always cargo test`, review each diff, strip `assertion_line:`, delete `*.snap.new`. Generated docs: `cctop metrics --md > docs/metrics.md && cctop metrics --readme README.md`.
- Done = `make check` and `npm test` green.

## Rulings made while planning

- **Header degradation order:** items in the order repo `owner/name`, worktree, `+N dirs`, output style (when not `default`), `agent: <name>`, `remote`; as width shrinks they are dropped **from the right** (remote first, repo last). The PRD's "first-to-last" is read as "the list is ordered by priority".
- **Header object:** `dashboard::Header` gains `workspace: Vec<String>` (the items, in that order, empty when none — skipped in JSON when empty) and `Header.line` appends them with ` · `. The TUI's `ui/dashboard.rs::header` drops items from the right to fit; the pane keeps drawing `line` verbatim.
- **A01 fallback order:** PRD 1 located cause › shim deltas (this PRD) › today's text.

## Review Focus

1. A gateway payload with `spend_limit.used_usd` but no `limit_usd`: show `spend $412 · monthly`, never `/ $0` or a panic (Task 3 test).
2. `added_dirs: []` vs absent: both show nothing (Task 4 test).
3. `system_char_delta` negative: `system −800 chars` with a real minus sign, matching PRD 1's `±` style (Task 2 test).
4. `output_style.name == "default"` and an absent `output_style`: no style item (Task 4 test).
5. A 60-column TUI with every workspace item present: the phase cell stays visible, items drop from the right, no overflow (Task 4 snapshot at 80, plus a 60-column assertion).

---

### Task 1: Parse the dropped fields and keep them in state (US-001, US-002)

**Files:**
- Modify: `src/status.rs` (types, helpers, `FULL` test payload, tests), `src/ui/state.rs` (`CacheState`, `StatusFacts`, `apply_status`, tests)

**Interfaces:**
- Produces:
  - `MissCause { causes, tools_added: Option<u64>, tools_removed: Option<u64>, system_char_delta: Option<i64> }`
  - `Limit { used_percentage, resets_at, used_usd: Option<f64>, limit_usd: Option<f64>, period: Option<String> }`
  - `Workspace { current_dir: Option<String>, project_dir: Option<String>, added_dirs: Vec<String>, git_worktree: Option<String>, repo: Option<Repo>, cloud_dir: Option<String> }`, `Repo { host: Option<String>, owner: Option<String>, name: Option<String> }`, `Named { name: Option<String> }`, `RemoteInfo { session_id: Option<String> }`; `Sample.{workspace, output_style, agent, remote}: Option<…>`; `Sample::pr_url() -> Option<&str>`, `Sample::pr_kind() -> Option<&str>`.
  - `CacheState.{last_miss_tools_added: Option<u64>, last_miss_tools_removed: Option<u64>, last_miss_system_delta: Option<i64>, requests: u64, miss_recache_tokens: u64}`
  - `StatusFacts.{spend_limit_usd: Option<(f64, Option<f64>)> /* used, limit */, spend_limit_period: Option<String>, spend_limit_resets_at_ms: Option<i64>, workspace: Option<status::Workspace>, output_style: Option<String>, agent_name: Option<String>, remote_session_id: Option<String>, pr_url: Option<String>, pr_kind: Option<String>}` (the PRD's `(used, limit)` with `limit` optional — Review Focus 1).

- [ ] **Step 1: Write the failing tests.** Extend `FULL` (`status.rs:335`) with every new field (`last_miss_cause.tools_added: 2, tools_removed: 0, system_char_delta: 1200`, `spend_limit: {used_percentage: 82, resets_at: <epoch>, used_usd: 412.0, limit_usd: 500.0, period: "monthly"}`, a `workspace` with `repo {host: "github.com", owner: "acme", name: "shop"}`, `added_dirs: ["/x/y"]`, `git_worktree: "feat"`, `output_style: {name: "Concise"}`, `agent: {name: "reviewer"}`, `remote: {session_id: "r-1"}`, `pr: {number, url, kind}`) and assert each parses; `minimal_payload_parses` (none of them) still parses. In `state.rs`: `status_fields_survive_a_sparse_sample` (second sample without `workspace` keeps it) and `spend_limit_follows_latest` (second sample without `spend_limit` clears `spend_limit_usd`).
- [ ] **Step 2: Run** `cargo test status` → FAIL.
- [ ] **Step 3: Implement** the types (all `#[serde(default)]`) and `apply_status`.
- [ ] **Step 4: Run** `cargo test status && make check` → PASS, no snapshot changes.
- [ ] **Step 5: Commit** — `Status: parse and keep the fields the status line carries that cctop dropped`.

### Task 2: A01 and the cache card use the shim's deltas (US-003, first bullet)

**Files:**
- Modify: `src/advisor/rules/token.rs`, `src/coach.rs` (the cache light's "last" text if it names the cause), snapshots, `tests/pane/fixtures/*` (regenerated)

**Interfaces:**
- Consumes: Task 1's `CacheState.last_miss_*`; PRD 1's `state.requests.miss_for(id)`.

- [ ] **Step 1: Write the failing tests** in `token.rs`: `a01_uses_shim_deltas_when_unlocated` — a named `tools_changed` miss, no request lines, `cache.last_miss_tools_added = Some(2)`, `removed = Some(0)` → headline contains `tools +2 −0`; `a01_system_delta` with `-800` → `system −800 chars` (Review Focus 3; `+1.2k` style for |Δ| ≥ 1000, matching PRD 1's `short()` formatter — reuse it); `a01_located_beats_shim` — both present → the PRD 1 located text wins.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** the fallback order (Rulings). **Step 4:** `INSTA_UPDATE=always cargo test` (review: only A01 text for states with shim deltas), pane fixtures regenerated, `make check && npm test` green.
- [ ] **Step 5: Commit** — `Coach: A01 names the shim's tool and system deltas when the transcript cannot`.

### Task 3: The spend limit in dollars (US-003, second bullet)

**Files:**
- Modify: `src/ui/panels/limits.rs` (~130), `src/dashboard.rs` (~952), snapshots

**Interfaces:**
- Consumes: `StatusFacts.spend_limit_usd`, `spend_limit_period`, `spend_limit_resets_at_ms`, `spend_limit_pct`.
- Produces: `fn spend_limit_text(facts: &StatusFacts, now_ms: i64) -> Option<String>` in `src/dashboard.rs` (one formatter for both surfaces): `spend $412 / $500 · monthly · resets 3 Dec`; without limit `spend $412 · monthly · resets 3 Dec`; without period/reset those parts dropped; without USD today's `spend limit 82 %`. Dollars rounded to whole dollars; date as `D Mon` in local time (use the codebase's existing date formatter if there is one).

- [ ] **Step 1: Write the failing tests:** `spend_text_full`, `spend_text_no_limit` (Review Focus 1), `spend_text_pct_only` (equals today's), plus a limits-panel render test with and without USD (pattern: the existing `spend limit 40 %` test at `limits.rs:215`).
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; both surfaces call `spend_limit_text`. **Step 4:** snapshots (only states with USD change), `make check && npm test` green (rerun pane fixtures if the dashboard object changed).
- [ ] **Step 5: Commit** — `Status: the spend limit in dollars with its period and reset`.

### Task 4: Workspace facts in the header and sources view; the long-context marker; query keys (US-003, remaining bullets)

**Files:**
- Modify: `src/dashboard.rs` (`Header.workspace`, `header()`), `src/ui/dashboard.rs` (`header` drops items by width), `src/ui/sources_view.rs` (added-dir paths, repo, worktree), the cost panel (`src/ui/panels/tokens.rs` or wherever the model is drawn beside cost — `grep -rn "long ctx\|long_context" src/ui`), `src/query.rs` (summary/dashboard keys), `src/metrics/registry.rs` (`spend_limit_usd`, `workspace_added_dirs`, `output_style`), docs regenerated, snapshots, pane fixtures

**Interfaces:**
- Consumes: Task 1's `StatusFacts`; Task 3's `spend_limit_text`.
- Produces: `dashboard::Header.workspace: Vec<String>` (`#[serde(skip_serializing_if = "Vec::is_empty")]`), items exactly `acme/shop`, `wt feat`, `+1 dirs` (`+1 dir` singular), `style Concise`, `agent: reviewer`, `remote`. `cctop query summary` gains `status.workspace {repo, worktree, added_dirs (count), output_style, agent, remote: bool}`, `status.spend_limit {used_usd, limit_usd, period, resets_at}` and `status.exceeds_200k_tokens`, each value tagged with its registry id via the file's `m(…)` helper; dashboard JSON carries `header.workspace`.

- [ ] **Step 1: Write the failing tests:** `header_workspace_items` (all present → the six items in order; `added_dirs: []` and absent → no dirs item; style `default` and absent → no style item — Review Focus 2, 4); `tui_header_drops_from_right` (60 columns: phase cell intact, `remote` dropped before `acme/shop` — Review Focus 5); snapshots at 80 and 120 columns with all items; `cost_panel_long_context_marker` (`exceeds_200k_tokens = true` → `>200k` beside the model; false → absent); `sources_view_lists_added_dirs` (paths shown there only); `query_summary_status_keys`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.** The `>200k` marker's caveat/tooltip names long-context pricing; `long_context_pct` unchanged. Registry entries with formula and sources in the file's convention.
- [ ] **Step 4:** `INSTA_UPDATE=always cargo test` (review: only snapshots whose state has the new facts change — fixtures A/B without a status sample must not move); regenerate docs; `scripts/pane-fixtures.sh && scripts/pane-fixtures.sh fixtures/session-b.jsonl b`; `make check && npm test` green.
- [ ] **Step 5: Commit** — `Status: workspace facts in the header, the long-context marker, and the new query keys`.
