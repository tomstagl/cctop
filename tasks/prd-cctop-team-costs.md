# PRD: teammate spend — agent teams on Panel 2 and in the agents view

**Status:** v1.1 · 2026-09-15 — draft, not implemented; second of two (the first is `tasks/prd-cctop-agent-costs.md`). v1.0 left the member-to-transcript mapping as its one blocker; v1.1 read it from the teams on the user's machine (19 team directories, 22 teammate transcripts across five teams, Claude Code 2.1.232 – 2.1.270). What changed is in the second box.
**Target:** cctop ≥ 0.3.1 attached to Claude Code CLI ≥ 2.1.271, on a session that leads an agent team.
**Depends on:** `tasks/prd-cctop-agent-costs.md` v1.1 (the combined headline, the provenance model, the agents view and its waste columns, the shared-shift fixture composer); `tasks/prd-cctop.md` v1.1 (collectors, `attach.rs`).

> Decisions taken with the user on 2026-09-15:
> 1. Teammate cost joins the combined headline on Panel 2 and gets rows in the agents view; Panel 6 stays as it is.
> 2. The user asked whether this can be **deterministic** and what its **provenance** is: yes, and better than subagents. A teammate is a full Claude Code session with its own transcript and its own `cost-state`, so its cost is Claude Code's own number, not a table estimate. Only the calls newer than the teammate's last `cost-state` are priced, exactly as cctop does for the main session.
> 3. The user asked whether it is **pricey** to run: no. It is the same mechanism as the subagent watcher (one filesystem watch, `Tailer` reading from the last offset per file), for ≤ 20 files. Budget is stated in §8.

> v1.1 — what the check changed (details in §3):
> - **The mapping is known.** A member entry has no session id, but every `user` / `assistant` / `system` / `attachment` line of a teammate's transcript carries `agentName` (the member's name) and `teamName` (the team directory's name, `session-<lead id8>`), from line 4 on. The file is a normal `<projects>/<slug of the member's cwd>/<sessionId>.jsonl`. No fallback order is needed.
> - **The team directory is deleted when the team ends; the transcripts stay.** Three of the five teams with transcripts on this machine have no `~/.claude/teams/<team>/` any more. So `config.json` is the *live* source (membership, `isActive`, `cwd`, `tmuxPaneId`) and the transcripts' `teamName` is the *durable* one; `cctop query` on a finished session and every fixture must work without the directory.
> - **Exactness is conditional.** A teammate writes `cost-state` when it ends or is bridged, like the lead (18 of 22 teammate transcripts have one; the live one does not). While the team works, every teammate is priced (`≈`); the exact figure arrives when a teammate finishes. v1.0's "exact where it counts" is now "exact once it has ended".
> - **Not every team is spawned through the `Agent` tool.** A `Workflow` run made the four `rev-*` reviewers of one session into teammates with no `teammate_spawned` result in the lead's transcript; the `teamName` scan finds them, the spawn results only add names and models when present.
> - **Liveness comes from Claude Code**: `isActive` on a `tmux` member is its own flag; `procs.rs` cannot map a teammate's process (it reads the attached `claude`'s environment only). Solo sessions have a team directory with one member (the lead) and are not teams.
> - Dropped: `cctop doctor` (does not exist; the `team` object of `cctop query agents` reports what was found and where it looked), the `orphaned` waste reason (the whole directory goes, which is an ending, not waste), the three-teammate synthetic fixture (real teams on this machine supply the fixture).

---

## 1. Introduction

cctop knows a team exists: `agents::teammates` reads `~/.claude/teams/<team>/config.json` when the attached session leads the team, and Panel 6 prints `team N name (type) · …`. That is all. `Teammate` is a name and an agent type. The teammates' transcripts, which hold every token they spent and Claude Code's own accounting of it, are never opened, so a 20-member team shows as one dim line and $0.

Unlike subagents, teammates are the easy case for provenance. Each teammate is its own `claude` process with its own session id and its own `<projects>/<slug>/<sessionId>.jsonl`, and that file carries `cost-state` lines exactly like the lead's — written at the same moments (its end, or a bridge). Reading them with the collectors cctop already has (`tail.rs`, `transcript::Line`, `metrics::Aggregate`, `CostTracker`) gives per-teammate context, usage, cost and turn counts with the same rules the lead's Panel 2 has: exact up to the last `cost-state`, priced after it.

The one thing v1.0 could not say — how a member maps to a file — is now read from live and finished teams (§3.2): the transcript names its team and its member on every line.

## 2. Goals

- **Teammate cost** from each teammate's own transcript: its `cost-state` where one exists, priced after it (or throughout, while it runs), folded into the combined headline of the first PRD with the same marks.
- **Teammate rows in the agents view** with the same columns as subagents, plus what only a session has: context size and human/machine turns.
- **Same budget** as today: ≤ 2 % CPU idle, ≤ 5 % during a turn, ≤ 50 MB RSS, with 20 teammates.
- **Read-only, no prose**: teammate transcripts are read with the same collectors that already refuse to keep text (`transcript/`); `agentName` and `teamName` are read to match and kept only as the row's label; inboxes (`~/.claude/teams/<team>/inboxes/`) and `<teammate-message>` bodies are never read.
- **Works after the team is gone**: `cctop query --session` on a finished lead, `cctop report`, the baseline and a fixture all find the teammates without `~/.claude/teams`.

## 3. Findings

### 3.1 What the code does today

| Fact | Where | Consequence |
|---|---|---|
| `teammates()` returns `{name, agent_type}` per member of the team whose `leadSessionId` is the session (or whose directory name ends in the session's first 8 characters); **the lead is one of the members** (`agentType: team-lead`) | `agents.rs` `teammates` | The collector must skip the lead (its transcript is the attached session) and treat a one-member team as no team |
| Panel 6 prints one `team N …` line; `query agents` exports `teammates: [{name, type}]`; registry `teammates` (D12) | `ui/panels/agents.rs`, `query.rs` | Nothing to extend for cost until a collector exists |
| `SessionMode::Team` is already derived from `agg.agent_setting` or a non-empty `teammates` and mutes `/clear`, waiting, cold-resume and post-compaction nudges | `advisor/mod.rs` | Unchanged; the mode's input gains nothing |
| The lead's own cost, context and turns come from `State::apply` over `transcript::Line` with `Aggregate` + `CostTracker` | `ui/state.rs`, `metrics/` | The same pipeline can be instantiated per teammate transcript; nothing new to parse |
| `ToolUseDetail::Agent(AgentResult)` parses `teammate_spawned` results (`status`, `agent_id`, `agent_type`, `resolved_model` from `model`) into `state.tools.agent_spawns`; `name`, `team_name`, `teammate_id`, `tmux_pane_id`, `is_splitpane` are not kept | `transcript/tool_result.rs`, `tools.rs` | The spawn is the lead-side record of a teammate when the `Agent` tool made it; extend the parser with `name` and `team_name` |
| `<teammate-message` lines are machine turns (coach A27); their content is not parsed | `transcript/mod.rs` | Stays so |
| `procenv.rs` reads the attached `claude`'s environment (allowlisted keys); `procs.rs` enumerates the tree under the attached pid | `procenv.rs`, `procs.rs` | A teammate is a sibling process in another tmux pane, not a descendant: no process mapping |
| `attach.rs` rebuilds every collector on a session change; `AgentWatcher` shows the pattern (notify watch + `Tailer` per file, scan on events) | `attach.rs`, `agents.rs` `AgentWatcher` | The teammate watcher is a sibling of `AgentWatcher` |
| `load.rs` builds a `State` without a UI for `cctop query`; `--session <fixture>` finds `<fixture>/subagents` beside the file | `load.rs` | The query path reads teammate files once, like `agents::load`; a fixture's teammates live beside it the same way |
| Coach PRD non-goal: "No fleet or team view beyond rolling teammates' cost into the attached session" | `prd-cctop-coach.md` §9 | This PRD is exactly that roll-in; a fleet view stays out |
| Fixtures A, B and C lead no team; there is no team fixture; the anonymiser shifts timestamps per file | `fixtures/`, `scripts/anonymise-transcript.py` | Fixture D is composed from a real team with one shared shift (§8) |
| There is no `cctop doctor`; the CLI has `run`, `query`, `metrics`, `install`, `uninstall`, `hook`, `statusline-shim`, `split`, `pane`, `mcp`, `advise`, `report`, `coach-replay`, `coach-stats` | `main.rs` | What the collector could not find is reported in `cctop query agents` (`team.missing`, `team.looked_in`) and on the row |

### 3.2 What Claude Code writes (read on this machine)

| Fact | Evidence | Consequence |
|---|---|---|
| **`~/.claude/teams/session-<lead id8>/config.json`**: `{createdAt, leadAgentId, leadSessionId, members[], name}`; a member is `{agentId, agentType, backendType, cwd, joinedAt, name, subscriptions, tmuxPaneId}` plus, on spawned teammates, `color`, `isActive`, `model`, `planModeRequired`, `prompt`; `agentId` is `<name>@<team dir>`; `backendType` is `in-process` for the lead and `tmux` for teammates; `joinedAt` is epoch ms | 19 directories; 16 hold only the lead | No session id anywhere in the member. The lead is a member. A one-member team is a solo session |
| **Every teammate transcript line of type `user`, `assistant`, `system` and `attachment` carries `agentName` and `teamName`**; the first such line is line 4; `entrypoint` is `cli`; the file is `<projects>/<slug of member.cwd>/<sessionId>.jsonl` and its first timestamp is within seconds of `joinedAt` | 22 teammate transcripts, five teams, versions 2.1.232, 2.1.251, 2.1.263, 2.1.269, 2.1.270 | The durable key is `teamName == <team dir name>`; the member is `agentName == member.name`. `first_seen::TEAM_NAME ≤ 2.1.232` |
| **The lead's transcript carries no team key**; when the `Agent` tool spawned the teammate its result is `toolUseResult.status = "teammate_spawned"` with `agent_id`, `agent_type`, `color`, `is_splitpane`, `model`, `name`, `plan_mode_required`, `prompt`, `team_name`, `teammate_id`, `tmux_pane_id`; a team made by a `Workflow` run (four `rev-*` reviewers) has no such result | 18 `teammate_spawned` results in the corpus; the workflow team has none | Spawn results are optional decoration (name, model, type); the `teamName` scan is the source of truth |
| **The team directory is removed when the team ends; the transcripts stay** | Teams `session-12f423aa`, `session-e3346d84`, `session-83f0e9b9` have transcripts (4, 11 and 1 teammates) and no directory | Membership after the fact comes from the transcripts; a finished session's teammates must not vanish from its total |
| **Teammates write `cost-state` like the lead: at their end or a bridge, never periodically** | 18 of 22 teammate transcripts have one (all ended); the live teammate of the current team has none | A running teammate is priced (`≈`); its exact figure lands when it ends. The team's part of the headline is `≈` while anyone works |
| **`isActive`** on a `tmux` member is Claude Code's liveness flag | `true` on the running teammate, `false` on the finished one, absent on the lead | Liveness = `isActive` when the directory exists, else "a line in the last 5 min"; the row says which |
| Teammates share the lead's `cwd` in every team seen; the member's `cwd` is still the field to trust | 22 of 22 | Scan the slug of each member's `cwd` when the config exists; the lead's slug otherwise |
| No `inboxes/` directory existed under any team on this machine | `ls ~/.claude/teams/*/` shows `config.json` only | FR-2 stands regardless |

## 4. Design

### 4.1 Provenance

Applying the model of the first PRD (§4.1 there):

| Part | Source | Mark |
|---|---|---|
| A teammate's cost up to its last `cost-state` | `ledger` (the teammate's own `totalCostUSD`; it includes that teammate's own subagents and hidden calls, exactly like the lead's) | none |
| A teammate's calls newer than that line, or all of them while it has none | `priced` | `≈` |
| A teammate known (from config or a spawn) whose transcript cannot be found | `missing` | the row shows `—` and `no transcript`; the headline is marked `≈` and the breakdown says `team ≈$X (N of M read)` |

The combined headline is therefore `main + agents(≈) + team`, marked `≈` when any part is; a team session's headline is exact only when every teammate has ended and nothing was priced after their ledgers.

### 4.2 Panel 2

Line 6 and the breakdown line of the first PRD gain the team part:

```
 cache hit 97 %  ·  ≈$41.20 ($3.10/h main)  ·  in 12k/min
 main ≈$9.90 · agents ≈$0.15 · team ≈$31.15 (76 %, 5 of 5)
```

- `a` toggles agents *and* team out of the figure; `main only` on line 7 covers both.
- The `where:` attribution on line 10 gains `team` beside `agents` when the team's input tokens are a top-3 share.
- `$/h` stays main-only with its `main` suffix (first PRD); a team burn rate is a follow-up (§11).

### 4.3 The agents view

Teammates appear as a second group under the subagents, with a group row and subtotal, at the same 60 columns:

```
 ─────────────────────────────────────────────────────────
 team 5 · 3 active · ≈$31.15 · wasted ≈$4.10 · 4 of 5 read
   name      model    time   ctx    tok    $     turns
 ● reviewer  opus    41m 10  312k  9.8M ≈12.40  7/3   idle 9m
 ● builder   opus    38m 02  188k  6.1M  ≈8.10  5/2
 ○ tester    sonnet  12m 44   90k  1.2M   1.05  2/1   ended
 ○ writer    haiku    9m 08   44k  0.6M   0.22  1/1   ended
 — planner   —           —     —     —     —    —     no transcript
```

Columns: activity glyph (`●` `isActive`, or a line in the last 5 min when there is no config · `○` ended: a `cost-state` at the end of its file, or `isActive: false` · `—` unknown), teammate name, model family, elapsed since its first line, current context (the teammate's own Panel-1 figure), tokens, cost with its mark (`≈` only where priced), human/machine turns, and a status word (`idle <age>`, `ended`, `gone` when the directory vanished under a still-open lead, `no transcript`). `s` sorts the group by the same keys as the subagent group.

### 4.4 Wasted spend for teammates

The subagent reasons of the first PRD do not transfer directly: a teammate has no task notification. Reasons that are structural for a session:

| Reason | Evidence | Amount |
|---|---|---|
| `idle` | alive (`isActive`, or a recent line) but no API call for ≥ `IDLE_MS` and no pending tool result in its own transcript | its cost since the last human-turn boundary in *its* transcript, labelled `idle <age>` |
| `errored` | its last turn ended in an API-error line (`isApiErrorMessage`, the lead's parser applied to the teammate's transcript) and no turn followed within `FAILED_AFTER_MS` | the cost of that turn |

A teammate's *outcome* is not classified: whether its work reached the lead is a matter of inbox messages cctop does not read (FR-4). v1.0's `orphaned` is gone: the directory disappears for every member at once when the team ends, which the row shows as `gone` or `ended`, not as waste.

### 4.5 Query, dashboard, pane

- `cctop query agents.teammates[*]` gains `session_id`, `path` (or `null`), `model`, `state` (`active` / `ended` / `gone` / `missing`), `elapsed`, `context`, `tokens`, `cost` (`approx`, `source`), `turns` (`{human, machine}`), `waste`; the object gains `team` (`{name, members, read, missing: [names], looked_in: [dirs], cost, waste, source: "config" | "transcripts"}`).
- `cctop query summary` / `dashboard`: `cost_combined` (first PRD) now includes the team; a new `team_cost` value sits beside `agents_cost`.
- Dashboard row 6 appends `team ≈$X` after the agents part, inside `ROW_WIDTH`.
- The pane's Agents view draws the teammate group after the subagent group with the columns of §4.3.

## 5. User stories

### US-001: The facts, the fixture and the parser additions
**Description:** As the implementer, I need the mapping recorded where cctop keeps its facts, a real team as a fixture, and the two parsers that read the team keys.

**Acceptance Criteria:**
- [ ] `docs/teams.md` records §3.2 (keys only, no names or prompts): the config layout, the transcript keys, the spawn result, the lifetime of the directory, when `cost-state` is written. `harness_facts.rs` gains `first_seen::TEAM_NAME` (`≤ 2.1.232`; refine on the corpus) and a `teams` module: `DIR_REMOVED_AT_END: bool = true`, `MEMBER_HAS_SESSION_ID: bool = false`, `LIVENESS_KEY: &str = "isActive"`.
- [ ] `transcript::Line` parsing keeps `agent_name` and `team_name` from a line's top level as two `Option<String>` on the parsed line's metadata (they are labels, not prose; the anonymiser already treats `agentName` as an id); `Aggregate` records the first pair seen (`team_name`, `agent_name`), so a *teammate* attached as the main session shows its team too.
- [ ] `AgentResult` gains `name` and `team_name` (from `teammate_spawned`); `AgentSpawn` carries them.
- [ ] `fixtures/session-d.jsonl` is composed with `scripts/compose-fixture.py --shared-shift` from a real lead on this machine that spawned a teammate through the `Agent` tool (a 718-line lead with a 101-line teammate that ended with a `cost-state`, 2.1.269), with `fixtures/session-d/teammates/<sessionId>.jsonl` beside it. The composer adds one teammate transcript cut before its `cost-state` (priced) and one `teammate_spawned` result whose teammate has no transcript (missing); `fixtures/session-d.team.json` is the anonymised `config.json` of a *live* team on this machine (the current one, with `isActive` on its member), used by the config-path tests; the transcript-path tests run without it. `--session fixtures/session-d.jsonl` finds `session-d/teammates/` beside the file the way A's `subagents/` is found, and the team file beside it.
- [ ] `scripts/anonymise-transcript.py` keeps `agentName` and `teamName` consistent across the lead, the teammates and the team file (one mapping for the set), and keeps `isActive`, `backendType`, `joinedAt`, `cwd` (rewritten like other paths).

### US-002: The teammate collector
**Description:** As every surface, I want each teammate's context, usage, cost and turns from its own transcript, whether or not the team directory still exists.

**Acceptance Criteria:**
- [ ] `src/team.rs`: `Teammate` gains `session_id: Option<String>`, `path: Option<PathBuf>`, `agg: Aggregate`, `cost: CostTracker`, `last_line_at`, `alive: Liveness` (`Active` / `Recent` / `Ended` / `Gone` / `Missing`, with the source), `model`, `waste: Option<Waste>`; `Team { name, source: Config | Transcripts, members, read, missing, looked_in }`.
- [ ] Discovery, in order and merged by name: (1) `~/.claude/teams/<team>/config.json` when present (membership, `cwd`, `isActive`, `joinedAt`; the lead skipped; a one-member team is no team); (2) the lead's `agent_spawns` with `status == teammate_spawned` (name, model, type); (3) a scan of the first 10 lines of every `.jsonl` in the slug of each known `cwd` (the lead's when none is known), newer than the lead's first line, for `teamName == session-<lead id8>` → `agentName`, `sessionId`, path. A name from (1) or (2) with no file from (3) is `missing`; a file from (3) with no entry in (1) or (2) is a member too.
- [ ] `TeamWatcher` mirrors `AgentWatcher`: a notify watch on the team directory (membership and `isActive` changes; its disappearance → `Gone` for every live member, transcripts kept) and on each project directory in `looked_in` (new transcripts); one `Tailer` per member; `poll()` returns changed; the scan of (3) runs on directory events and at most once per 2 s tick.
- [ ] `team::load(session_dir | fixture)` builds the same rows once for `load.rs`; a test loads fixture D both ways and compares.
- [ ] Every teammate line is fed through `transcript::Line::parse` and the existing `Aggregate` / `CostTracker`; no new parsing beyond US-001; no text retained.
- [ ] `attach.rs` rebuilds the watcher on session change; a session that leads no team costs one `read_dir` of `~/.claude/teams` per tick and nothing else (the head scan runs only when a team is known).
- [ ] Registry ids: `team_cost` (Panel 2, D12 + D2c + D11 + D9, `≈ for a teammate's calls after its last cost-state, or all of them while it runs; ≈ and "N of M read" when a transcript is missing`), `teammate_cost`, `teammate_context`, `teammate_tokens`, `teammate_turns`, `teammate_waste`, `team_waste` (Panel 6). A new source `D2c` — teammate transcripts, `<projects>/<slug>/<sessionId>.jsonl` matched by `teamName` — is added to `prd-cctop.md`'s table and `docs/metrics.md`'s legend (which also gains the missing `D12`).

### US-003: Panel 2 and the headline
**Description:** As a user leading a team, I want the session's money to include the team.

**Acceptance Criteria:**
- [ ] `CostTracker::combined` (first PRD) takes the team's costs; the headline, breakdown line and `where:` attribution render as §4.2; `a` toggles the team with the agents.
- [ ] `cost_combined` in `query summary` / `dashboard` includes the team; `team_cost` exported beside `agents_cost`; dashboard row 2's detail, `cctop report` and the coach's limits-light lines follow.
- [ ] Snapshots on fixture D at 120 × 30 and 56 × 20; fixtures A, B and C render character-identical to the first PRD's result (no team, no change).

### US-004: Teammate rows in the agents view and the pane
**Description:** As a user with 20 teammates, I want them in the same ledger as the subagents.

**Acceptance Criteria:**
- [ ] The agents view renders the team group of §4.3 under the subagent group; sort applies within the group; `Enter` on the group row collapses it; the `no transcript` and `gone` rows render.
- [ ] Waste reasons of §4.4 are unit-tested on synthetic transcripts (idle, errored) and on fixture D.
- [ ] `query agents` exports §4.5; `tests/pane/fixtures/agents-d.json` generated by `scripts/pane-fixtures.sh fixtures/session-d.jsonl d`; the pane's Agents view draws the group and its test asserts the columns; dashboard row 6 on fixture D is row-identical on both sides.

### US-005: Budget and drift
**Description:** As a user, I want cctop to stay cheap with 20 teammates and to notice when Claude Code moves the files.

**Acceptance Criteria:**
- [ ] A test builds a 20-member team from fixture D's teammates (copied under a temp dir with rewritten `agentName`s and a config) and asserts that `TeamWatcher::poll` with no new lines does no file reads beyond the directory scans, and that a 1 000-line append across all files is drained in one poll.
- [ ] `cctop query agents` on a team reports `team.read`, `team.missing` and `team.looked_in`; a member known for > 60 s with no transcript is `missing` with the directories it looked in.
- [ ] `make check-types` / `harness_facts` version comparison covers the `teams` module; a newer `claude` than `READ_FROM` prints the existing drift warning.

## 6. Functional requirements

- FR-1: Teammate cost must come from the teammate's own `cost-state` where present and be marked `≈` for the calls after it (or all of them while it runs); a missing transcript must never be priced as zero without the `N of M read` mark on the sum.
- FR-2: Teammate transcripts are read through the same `transcript::Line` parser and collectors as the lead's; `agentName` and `teamName` are the only new fields read and are kept as labels; no other text field is retained, and inbox files under `~/.claude/teams/**/inboxes/` are never opened.
- FR-3: The collector must be a sibling of `AgentWatcher`: one filesystem watch per directory, one `Tailer` per member reading from its last offset, head scans only on events or the 2 s tick, and no head scan at all when no team is known.
- FR-4: Waste for teammates is classified only by the structural evidence of §4.4; no message content, no inbox, no description text.
- FR-5: With no team (including a one-member team directory), every surface renders character-identical to the first PRD's result.
- FR-6: `cctop query` values introduced here carry `metric_id`, `approx` and `source`; the registry, `docs/metrics.md` and the README block regenerate; CI fails on drift.
- FR-7: The team must be found without `~/.claude/teams` — from the transcripts alone — so that a finished session, `cctop report`, the baseline and every fixture see the same members a live session saw.

## 7. Non-goals

- No fleet view: cctop attaches to one session; teammates are read only because that session leads them. Attaching *to* a teammate as the main session is unchanged and shows that teammate as a normal session (with its team's name in the header, from US-001).
- No teammate coach: no nudges are computed on a teammate's transcript; the lead's coach sees only the team's cost and waste totals.
- No team burn rate or turn attribution (§11).
- No reading of `inboxes/`, `<teammate-message>` bodies or the team's task board, even for counts, in this PRD.
- No process mapping for teammates (no tmux queries): liveness is `isActive` or line recency.

## 8. Technical considerations

- **Budget with 20 teammates.** Each `Tailer` holds an offset and a small buffer; `Aggregate` and `CostTracker` per teammate are a few KB plus the turn vector; 20 of them fit inside the existing 50 MB RSS budget with margin. CPU is dominated by parsing appended lines, which the lead already does for its own transcript at the same rate per teammate; 20 busy teammates at once is the worst case and is the US-005 test. If it exceeds 5 % during a turn, the fallback is parsing teammates on the 2 s tick only, never per event. The head scan reads ≤ 10 lines of each candidate file (73 transcripts in this repo's project directory; `teamName` is on line 4 of every teammate file seen) and only for files newer than the lead's start.
- **Fixture composition.** `compose-fixture.py --shared-shift` anonymises a set of transcripts with one time shift so that ordering across lead and teammates survives (the first PRD's §3.2 shows why per-file shifts make ordering questions unanswerable); it also carries one id map across the set so `teamName` / `agentName` still match.
- **Session rotation.** A teammate's `/clear` writes `continued-in` into its old transcript (coach PRD v1.2); the watcher follows it the way `attach.rs` follows the lead's, and the new file carries the same `agentName` / `teamName`.
- **`load.rs` for `cctop query`.** One-shot read of every teammate file found; for a 20-member team this is the same cost as `cctop query` on 20 sessions, bounded by transcript size, and acceptable for a CLI call.
- **The baseline.** `baseline.rs` reads finished sessions' `cost-state` per file; a lead's figure excludes its teammates (their ledgers are their own files). Whether the baseline should fold teammates into the lead is a follow-up (§11), not a change here.

## 9. Design considerations

- **Exact once it has ended.** The team column shows Claude Code's number without a mark for every teammate that finished, and `≈` for the ones still working; the group header carries the worst mark, once. This is the one place cctop's dollars can be authoritative for a whole group of sessions, and the view lets that show without pretending while the work is in flight.
- **Same table, second group.** No new view; the agents view gains a group with two extra columns that only sessions have (`ctx`, `turns`).
- **Say what is missing.** `5 of 5` / `4 of 5` in the breakdown, a `no transcript` row and `gone` after the directory went are the honest states; never a silent zero and never a vanished teammate.

## 10. Open questions

1. **Teammates in other working directories.** Every team seen shares the lead's `cwd`; a member with another `cwd` is handled through config and untested on a real team. Watch for one.
2. **Nested subagents of teammates.** A teammate can launch its own subagents under its own session directory; its `cost-state` already includes them (first PRD §3.2), its priced tail does not. Fold them into the teammate's row (`+ N agents ≈$X`) in a follow-up after the collector is stable.
3. **`teamName` before 2.1.232.** The oldest teammate transcript on this machine is 2.1.232 and has it; the version it first appeared in is unknown and only matters for old sessions read by `cctop query`.

## 11. Follow-ups

- Team burn rate (`$/h` over lead + team) and per-human-turn attribution once the first PRD's turn membership follow-up exists.
- Teammates' own subagents (open question 2).
- A `team` LATER rule in the coach when team waste crosses the same threshold as `agents-waste`.
- Folding teammates' ledgers into the lead's baseline figure (`baseline.rs`), once combined cost has been live for a week.
