# Hook Events Implementation Plan (PRD 5)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Subscribe `cctop hook` to seven more hook events that a silent exit-0 observer cannot affect (Elicitation, ElicitationResult, ConfigChange, CwdChanged, DirectoryAdded, TeammateIdle, PreSkillShellCommand), never to the five excluded ones, prune their payloads to facts, show them where they belong, and tell old installs to re-run `cctop install`.

**Architecture:** `hooks::EVENTS` (+ a `NEVER` list) drives install/uninstall; `HookEvent::from_stdin` prunes; `State::apply_hook` turns events into waiting state, events rows, the miss marker, cwd/dirs, team idle and skill-shell counts; `install::hooks_missing` feeds the sources view, `cctop pane status`, the query and the header badge.

**Tech Stack:** Rust (serde_json, insta).

**Spec:** `tasks/prd-cctop-hook-events.md` (issue #29).

## Global Constraints

- Branch `hook-events`, cut from `session-signals` after PRD 4. Commit subjects `Hooks: …`, `Docs: …`, trailer in the body.
- `EVENTS` = the existing 22 + exactly these 7 (29 total). `NEVER` = `WorktreeCreate`, `WorktreeRemove`, `MessageDisplay`, `Setup`, `FileChanged`, each with its one-line reason from PRD §1.
- `cctop hook` still prints nothing and exits 0 for every event.
- No prose spooled: `Elicitation.message` → `message_chars`, `requested_schema` → `schema_fields` (top-level property count); `ElicitationResult.content` → `content_fields` (key count or 0), keep `action`, `mode`, `elicitation_id`; `PreSkillShellCommand.tool_input.command` bounded to `COMMAND_CHARS` (200), rest of `tool_input` dropped; the other four kept as is.
- Events-view rows: `settings changed · <file name only>` (never the path), cwd/dir rows, skill-shell rows with the bounded command.
- Waiting text `waiting on you · MCP <server> asks` + elapsed, at the existing fixed width.
- A01 suffix `· after settings change`, within `coach::WIDTH`; with PRD 1 present (it is: stacked branch) the marker is a field on the located miss.
- Added-dirs count shared with PRD 2's `workspace.added_dirs`: union by path, no double count.
- `Elicitation` live behaviour → `docs/verification/pane.md` item 42, `Result: pending`.
- Insta, generated docs, pane fixtures per CLAUDE.md. Done = `make check` and `npm test` green.

## Rulings made while planning

- **`WaitingKind` stays `Copy`:** add a unit variant `WaitingKind::Elicitation`; the open prompts live in `State.elicitations: BTreeMap<String /* server */, VecDeque<i64 /* since_ms */>>` (FIFO per server). The waiting line names the server with the oldest open prompt.
- **ConfigChange → miss:** the first `LocatedMiss` (PRD 1) or named miss whose timestamp is after the `ConfigChange` and before the next one gets `after_config_change: Option<String /* source */>`.

## Review Focus

1. Two `Elicitation`s from the same server, one `ElicitationResult`: still waiting (one open) (Task 3 test).
2. `ElicitationResult` with no open `Elicitation` (hook installed mid-prompt): ignored, no underflow (Task 3 test).
3. A settings file where the person already has their own hooks on `ConfigChange`: install adds cctop's group beside theirs, uninstall removes only cctop's (Task 1 test).
4. `CwdChanged` to a path outside the project: git/files display follows the new cwd; the session's identity (project dir) does not change (Task 4 test).
5. A `TeammateIdle` for a member name not in the team config: ignored, inference unchanged (Task 4 test).

---

### Task 1: Register the seven events safely; tell old installs (US-001, US-006)

**Files:**
- Modify: `src/hooks.rs` (`EVENTS`, `NEVER`, `without_hooks`), `src/install.rs` (`hooks_missing`, diff preview), `src/ui/sources_view.rs`, `src/pane.rs` (`cctop pane status`), `src/dashboard.rs` (header `hooks` badge amber), `src/query.rs` (`hooks_missing[]`), snapshots, pane fixtures

**Interfaces:**
- Produces: `pub const NEVER: &[&str]`; `pub fn hooks_missing(settings: &serde_json::Value) -> Vec<&'static str>` in `install.rs`; status text `hooks: <n> new events, run cctop install`; `State` gets the missing list from wherever it reads the settings today (find the existing hooks-installed check that drives the `hooks` badge: `grep -rn "hooks" src/dashboard.rs | grep -i badge`).

- [ ] **Step 1: Write the failing tests:** `events_and_never_disjoint`; `install_adds_exactly_seven` (fixture settings JSON with the old 22 → 7 new groups, nothing duplicated); `install_beside_user_hooks` (Review Focus 3); `uninstall_removes_never_events_too`; `hooks_missing_lists_new_events`; header badge amber snapshot when missing; sources view and `cctop pane status` text.
- [ ] **Step 2: Run** `cargo test hooks install` → FAIL. **Step 3: Implement.** **Step 4:** snapshots, pane fixtures; `make check && npm test` green.
- [ ] **Step 5: Commit** — `Hooks: subscribe seven safe events, never the five that change behaviour, and flag old installs`.

### Task 2: Prune the new payloads to facts (US-002)

**Files:**
- Modify: `src/hooks.rs` (`HookEvent::from_stdin`)

**Interfaces:**
- Produces: spooled JSON with the keys in Global Constraints; later tasks read `message_chars`, `schema_fields`, `content_fields`, `action`, `mode`, `elicitation_id`, `mcp_server_name`, `source`, `file_path`, `old_cwd`, `new_cwd`, `directory`, `teammate_name`, `team_name`, `command_name`, `command_source`, `invoked_by`, `tool_input.command`.

- [ ] **Step 1: Write the failing tests**, one per event, each payload carrying a distinctive string (`SECRET-MSG`, `SECRET-SCHEMA`, `SECRET-ANSWER`, a 500-char command) in the dropped/bounded fields → absent from `serde_json::to_string(&event)`, and the counts right.
- [ ] **Step 2: Run** `cargo test hooks` → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS; `make check`.
- [ ] **Step 5: Commit** — `Hooks: the new events are spooled as facts, without their text`.

### Task 3: MCP prompts in the waiting state (US-003)

**Files:**
- Modify: `src/ui/state.rs` (`WaitingKind::Elicitation`, `elicitations`, `apply_hook`), the turn panel (`src/ui/panels/turn.rs`), `src/dashboard.rs` / `src/coach.rs` (waiting line), `src/metrics/registry.rs` (`elicitation_waits`), `src/query.rs`, snapshots, docs, pane fixtures

**Interfaces:**
- Produces: `elicitation_waits { count, total_wait_ms, accept, decline, cancel }` in state and `cctop query dashboard`; cleared by matching `ElicitationResult`, `Stop`, `UserPromptSubmit`.

- [ ] **Step 1: Write the failing tests:** open → waiting text `waiting on you · MCP github asks`; result clears; Review Focus 1 and 2; `Stop` clears all; accounting counts by action; snapshot of the turn panel while waiting.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4:** snapshots, docs regenerated, pane fixtures; `make check && npm test` green.
- [ ] **Step 5: Commit** — `Hooks: an MCP server's question shows as waiting on you`.

### Task 4: Settings changes, folders, teammates, skill commands (US-004, US-005)

**Files:**
- Modify: `src/ui/state.rs` (`apply_hook` for the four), `src/requests.rs` (`LocatedMiss.after_config_change`), `src/metrics/usage.rs` (`CacheMissRecord.after_config_change` for named misses), `src/advisor/rules/token.rs` (A01 suffix), events (`src/events.rs` / `dashboard.rs`), `src/git.rs` / `src/files.rs` (cwd), PRD 2's added-dirs count, `src/team.rs` (hook idle), the tools view (skill shell counts), snapshots, pane fixtures, `docs/verification/pane.md`

**Interfaces:**
- Consumes: Task 2's spooled keys; PRD 1's `LocatedMiss`; PRD 2's `StatusFacts.workspace.added_dirs`.
- Produces: `State::added_dirs_count() -> usize` (union of status `added_dirs` and `DirectoryAdded` paths); `team.rs` member idle from hook time, `IDLE_MS` inference as fallback; tools-view row `skill shell: <name> ×N`.

- [ ] **Step 1: Write the failing tests:** `config_change_marks_next_miss` (synthetic spool + transcript: `ConfigChange` then a `tools_changed` miss → A01 headline ends `· after settings change`, located miss has `after_config_change == Some("project_settings")`); events row `settings changed · settings.json`; `cwd_changed_moves_display` (Review Focus 4); `directory_added_no_double_count` (same path from status and hook → 1); `teammate_idle_exact` + Review Focus 5 + team-view snapshot; `skill_shell_counts`.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4:** snapshots, pane fixtures; `make check && npm test` green.
- [ ] **Step 5:** `docs/verification/pane.md` §B item 42: *"An MCP elicitation with cctop's silent hook installed still shows the person the prompt unchanged"* — Setup: an MCP server that elicits, cctop hooks installed. Expected: the prompt appears and works; cctop shows `waiting on you · MCP <server> asks` until answered. `Result: pending`.
- [ ] **Step 6: Commit** — `Hooks: settings changes mark the next miss; folders, teammate idle and skill commands where they live`.
