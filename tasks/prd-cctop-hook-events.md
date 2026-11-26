# PRD 5: seven more hook events — MCP prompts, settings changes, folders, teammates, skill commands

**Status:** v1.0 · 2026-11-26 — ready for implementation. **Issue:** #29.
**Target:** cctop after v0.9.1; Rust (`hooks.rs`, `install.rs`, `ui/state.rs`, surfaces).
**Depends on:** nothing. Independent of PRD 0.
**Source:** the hook-event table in the Claude Code 2.1.332 binary: each event's `summary`, `description` (input fields, exit-code semantics) and `matcherMetadata`.

> Decisions taken with the user on 2026-11-26:
> 1. Only events where a hook that prints nothing and exits 0 changes nothing are subscribed. Every event in §1's second table is excluded for a stated reason.

---

## 1. Introduction

`cctop install` registers `cctop hook` for the 22 events in `hooks::EVENTS` (`src/hooks.rs:12–35`). `cctop hook` appends a pruned copy of the stdin to `~/.cctop/events/<session>.jsonl`, always exits 0 and prints nothing (`hooks.rs:181–200`). `with_hooks` is idempotent (`hooks.rs:277`), so re-running `cctop install` adds new events without duplicating old ones.

Claude Code 2.1.332 fires 34 hook events. Of the 12 cctop does not register, these 7 are safe for a silent exit-0 observer and useful:

| Event | Input (binary) | Exit 0, no output means | What cctop gets |
|---|---|---|---|
| `Elicitation` | `mcp_server_name`, `message`, `requested_schema` | "use hook response if provided": none provided, so the person is asked as usual | An MCP server is **waiting on the person**, like a permission dialog |
| `ElicitationResult` | `mcp_server_name`, `action`, `content`, `mode`, `elicitation_id` | "use hook response if provided": none provided | The wait's end, and accept / decline / cancel |
| `ConfigChange` | `source` (`user_settings`, `project_settings`, `local_settings`, `policy_settings`, `skills`), `file_path` | "allow the change" | Settings or skills changed mid-session, a cause of prompt-cache misses |
| `CwdChanged` | `old_cwd`, `new_cwd` | completes normally | The session's working directory moved |
| `DirectoryAdded` | `directory`, `source` (`slash_command`, `register_repo_root`) | completes normally | `/add-dir` happened, and when |
| `TeammateIdle` | `teammate_name`, `team_name` | stdout/stderr not shown; idle proceeds | A teammate is going idle: exact, where the team view infers today (`team.rs:281`, `IDLE_MS`) |
| `PreSkillShellCommand` | `tool_name`, `tool_input`, `command_name`, `command_source`, `invoked_by` | "the command runs, unless JSON output denies…": no output, so it runs | A skill ran an inline shell command |

Excluded:

| Event | Why not |
|---|---|
| `WorktreeCreate` | A registered hook **replaces** worktree creation: *"Stdout should contain the absolute path to the created worktree directory."* `cctop hook` prints nothing, so worktree creation would break. |
| `WorktreeRemove` | Same family: the hook performs the removal. |
| `MessageDisplay` | Carries the assistant's text (`delta`), and a hook can replace what is displayed. Prose, and in the display path. |
| `Setup` | Fires only for `init` / `maintenance` triggers; exit-0 stdout goes to Claude as context. No session-health value. |
| `FileChanged` | Needs a `matcher` naming files to watch, which cctop has no reason to choose. |

## 2. Goals

- The waiting indicator covers MCP prompts, not just permission dialogs and questions.
- A cache miss that follows a settings or skills change says so.
- The team view knows exactly when a teammate goes idle.
- People who installed hooks before this release are told to re-run `cctop install`.
- No new free text is kept: elicitation messages, schemas and answers are reduced to counts.

## 3. User stories

### US-001: Register the seven events, safely
**Description:** As a person using cctop, I want `cctop install` to subscribe to the seven safe events and never to the excluded ones, so that cctop sees more without changing Claude Code's behaviour.

**Acceptance criteria:**
- [ ] `hooks::EVENTS` gains the seven events in §1's first table.
- [ ] A new `hooks::NEVER: &[&str]` lists `WorktreeCreate`, `WorktreeRemove`, `MessageDisplay`, `Setup`, `FileChanged`, each with a one-line comment giving the reason from §1. A test asserts `EVENTS ∩ NEVER = ∅`.
- [ ] `without_hooks` (uninstall) removes cctop's entries from all 29 events. It also removes them from any `NEVER` event in case an older or hand-edited install put them there (test).
- [ ] The `install` diff preview shows the new events; `cctop install --yes` on a settings file with the old 22 adds exactly 7 groups (test on a fixture settings JSON).

### US-002: Prune the new payloads to facts
**Description:** As a developer, I want each new event spooled without free text, so that the no-prose rule holds.

**Acceptance criteria:**
- [ ] In `HookEvent::from_stdin` (`hooks.rs:70`):
  - `Elicitation`: `message` → `message_chars`; `requested_schema` → `schema_fields` (count of top-level properties).
  - `ElicitationResult`: `content` → `content_fields` (count of keys, or 0); `action`, `mode`, `elicitation_id` kept.
  - `PreSkillShellCommand`: `tool_input.command` bounded to `COMMAND_CHARS` like Bash; the rest of `tool_input` dropped.
  - `ConfigChange`, `CwdChanged`, `DirectoryAdded`, `TeammateIdle`: kept as is (paths, names, machine tokens).
- [ ] A test per event feeds a payload with distinctive strings in the dropped fields and asserts they are absent from the spooled line.

### US-003: MCP prompts in the waiting state
**Description:** As a person using cctop, I want "waiting on you" to show when an MCP server is asking me something, so that a stalled session is explained.

**Acceptance criteria:**
- [ ] `WaitingKind::Elicitation { server }` (`ui/state.rs:270`) is set by `Elicitation` and cleared by the next `ElicitationResult` for the same server (FIFO per server), or by `Stop` / `UserPromptSubmit`.
- [ ] The turn panel and the dashboard's waiting line read `waiting on you · MCP <server> asks` with the elapsed time, at the existing fixed width. Snapshot.
- [ ] Each completed elicitation adds to `permission_waits`-style accounting: a new metric `elicitation_waits` (count, total wait, accept/decline/cancel counts) in `metrics/registry.rs`; docs regenerated; `cctop query dashboard` carries it.

### US-004: Settings changes mark the next cache miss
**Description:** As a person using cctop, I want a cache miss right after a settings or skills change to say so, so that I know editing settings mid-session costs a re-write.

**Acceptance criteria:**
- [ ] `ConfigChange` adds an events-view row `settings changed · project_settings` (the file name only, not the path).
- [ ] The next cache miss after a `ConfigChange` (by timestamp, before any later request) carries `after_config_change: <source>`. A01 appends `· after settings change`, within width. With PRD 1 merged, this is a field on the located cause; without it, on today's miss record (`metrics/usage.rs:268`).
- [ ] Test: synthetic spool + transcript where a `ConfigChange` precedes a `tools_changed` miss.

### US-005: Folders, teammates, skill commands
**Description:** As a person using cctop, I want the session's folder moves, teammates' idle times and skills' shell commands to show where those things already live.

**Acceptance criteria:**
- [ ] `CwdChanged` updates the session's cwd used by `git.rs` and `files.rs` path display, and adds an events row. `DirectoryAdded` adds an events row and contributes to the added-dirs count (shared with PRD 2's status field; whichever arrives first wins, no double count).
- [ ] `TeammateIdle` sets the member's idle state in `team.rs` exactly, at the hook's time. Inference via `IDLE_MS` remains the fallback when no hook event exists. Team-view snapshot with a hook-idle member.
- [ ] `PreSkillShellCommand` counts per `command_name` (skill) in the tools view as `skill shell: <name> ×N`; the bounded command shows in the events row.
- [ ] `cargo test hooks` and the affected view snapshots pass.

### US-006: Tell old installs to update
**Description:** As a person who installed cctop's hooks earlier, I want to be told that new events exist, so that I re-run `cctop install`.

**Acceptance criteria:**
- [ ] `install::hooks_missing(settings) -> Vec<&str>` lists `EVENTS` without a `cctop hook` entry.
- [ ] The sources view (`s`) and `cctop pane status` show `hooks: 7 new events, run cctop install` when the list is non-empty; `cctop query dashboard` carries `hooks_missing[]`.
- [ ] The dashboard header's `hooks` badge turns amber (not red) when events are missing. Snapshot.

## 4. Functional requirements

- FR-1: `cctop install` must register exactly the 29 events in `EVENTS` and never any in `NEVER`.
- FR-2: The spool must not contain `message`, `requested_schema`, `content` or a skill command beyond `COMMAND_CHARS`.
- FR-3: An open elicitation must show as waiting until its result, stop, or the next prompt.
- FR-4: A cache miss following a `ConfigChange` must say so.
- FR-5: Missing events must be reported in the sources view, `cctop pane status` and the query.

## 5. Non-goals

- Answering elicitations, blocking config changes, or any non-empty hook output.
- The five excluded events.
- Showing settings file contents or diffs.

## 6. Technical considerations

- Each event spawns `cctop hook` (a process start). The seven are rare except `PreSkillShellCommand`, which fires once per inline skill command; cost is the same as `PreToolUse` today.
- `Elicitation` has no id on its input (only `ElicitationResult` has `elicitation_id`), hence FIFO per server.
- **Verify live:** that `Elicitation` with a silent exit-0 hook still shows the person the prompt unchanged. Item 42 in `docs/verification/pane.md`, `Result: pending`. Until it reads passed, the release notes call `Elicitation` observed-only.

## 7. Success metrics

- Zero behaviour change in Claude Code from the new subscriptions (item 42 passes; no bug reports).
- `waiting on you` covers MCP prompts in the first session that has one.

## 8. Open questions

- Should `ConfigChange` with `source: policy_settings` be shown? It is the organisation's, not the person's. v1 shows it like the others.
