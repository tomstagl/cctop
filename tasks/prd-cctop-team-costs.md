# PRD: teammate spend — agent teams on Panel 2 and in the agents view

**Status:** v1.0 · 2026-09-15 — draft, not implemented; second of two (the first is `tasks/prd-cctop-agent-costs.md`)
**Target:** cctop ≥ 0.3.1 attached to Claude Code CLI ≥ 2.1.271, on a session that leads an agent team (`~/.claude/teams/<team>/config.json` with `leadSessionId` = the attached session).
**Depends on:** `tasks/prd-cctop-agent-costs.md` (the combined headline, the provenance model, the agents view and its waste columns); `tasks/prd-cctop.md` v1.1 (collectors, `attach.rs`).

> Decisions taken with the user on 2026-09-15:
> 1. Teammate cost joins the combined headline on Panel 2 and gets rows in the agents view; Panel 6 stays as it is.
> 2. The user asked whether this can be **deterministic** and what its **provenance** is: yes, and better than subagents. A teammate is a full Claude Code session with its own transcript and its own `cost-state`, so its cost is Claude Code's own number, not a table estimate. Only the calls newer than the teammate's last `cost-state` are priced, exactly as cctop does for the main session.
> 3. The user asked whether it is **pricey** to run: no. It is the same mechanism as the subagent watcher (one filesystem watch, `Tailer` reading from the last offset per file), for ≤ 20 files. Budget is stated in §8.

---

## 1. Introduction

cctop knows a team exists: `agents::teammates` reads `~/.claude/teams/<team>/config.json` when the attached session leads the team, and Panel 6 prints `team N name (type) · …`. That is all. `Teammate` is a name and an agent type. The teammates' transcripts, which hold every token they spent and Claude Code's own accounting of it, are never opened, so a 20-member team shows as one dim line and $0.

Unlike subagents, teammates are the easy case for provenance. Each teammate is its own `claude` process with its own session id and its own `<projects>/<slug>/<sessionId>.jsonl`, and that file carries `cost-state` lines exactly like the lead's. Reading them with the collectors cctop already has (`tail.rs`, `transcript::Line`, `metrics::Aggregate`, `CostTracker`) gives per-teammate context, usage, cost and turn counts with the same exactness the lead's Panel 2 has.

What is not established yet is how a teammate's `config.json` entry maps to its session file. The members array cctop parses today carries `agentId`, `name` and `agentType`; whether it also carries a session id, and where a teammate's transcript is written (the lead's project slug, or the teammate's own cwd), has to be read from a live team before the collector is written (US-001).

## 2. Goals

- **Exact teammate cost** from each teammate's own `cost-state`, marked `≈` only for the tail after that line, folded into the combined headline of the first PRD.
- **Teammate rows in the agents view** with the same columns as subagents, plus what only a session has: context size and human/machine turns.
- **Same budget** as today: ≤ 2 % CPU idle, ≤ 5 % during a turn, ≤ 50 MB RSS, with 20 teammates.
- **Read-only, no prose**: teammate transcripts are read with the same collectors that already refuse to keep text (`transcript/`), and their inboxes (`~/.claude/teams/<team>/inboxes/`) are never opened.

## 3. Findings

| Fact | Where | Consequence |
|---|---|---|
| `teammates()` returns `{name, agent_type}` per member; matches the team by `leadSessionId` or the directory-name suffix | `agents.rs:329` | Membership is known; identity of the session file is not |
| Panel 6 prints one `team N …` line; `query agents` exports `teammates: [{name, type}]`; registry `teammates` (D12) | `ui/panels/agents.rs:83`, `query.rs:324` | Nothing to extend for cost until a collector exists |
| The lead's own cost, context and turns come from `State::apply` over `transcript::Line` with `Aggregate` + `CostTracker` | `ui/state.rs`, `metrics/` | The same pipeline can be instantiated per teammate transcript; nothing new to parse |
| `attach.rs` rebuilds every collector on a session change; `AgentWatcher` shows the pattern (notify watch + `Tailer` per file, scan on events) | `attach.rs`, `agents.rs:AgentWatcher` | The teammate watcher is a sibling of `AgentWatcher`, not a new mechanism |
| `load.rs` builds a `State` without a UI for `cctop query` | `load.rs` | The query path reads teammate files once, like `agents::load` |
| Coach PRD non-goal: "No fleet or team view beyond rolling teammates' cost into the attached session" | `prd-cctop-coach.md` §9 | This PRD is exactly that roll-in; a fleet view stays out |
| Fixture A and B lead no team; there is no team fixture | `fixtures/` | A team fixture must be composed (§8) before snapshots exist |

## 4. Design

### 4.1 Provenance

Applying the model of the first PRD (§4.1 there):

| Part | Source | Mark |
|---|---|---|
| Teammate cost up to its last `cost-state` | `ledger` (the teammate's own `totalCostUSD`) | none |
| Teammate calls newer than that line | `priced` | `≈` |
| A teammate whose transcript cannot be found | `missing` | the row shows `—` and a `no transcript` word; the headline is marked `≈` and the breakdown says `team ≈$X (N of M read)` |

The combined headline is therefore `main + agents(≈) + team`, marked `≈` when any part is; with subagents absent and every teammate's tail empty, a team session's headline can be exact.

### 4.2 Panel 2

Line 6 and the breakdown line of the first PRD gain the team part:

```
 cache hit 97 %  ·  ≈$41.20 ($3.10/h main)  ·  in 12k/min
 main $9.90 · agents ≈$0.15 · team $31.15 (76 %, 5 of 5)
```

- `a` toggles agents *and* team out of the figure; `main only` on line 7 covers both.
- The `where:` attribution on line 10 gains `team` beside `agents` when the team's input tokens are a top-3 share.
- `$/h` stays main-only with its `main` suffix (first PRD); a team burn rate is a follow-up (§11).

### 4.3 The agents view

Teammates appear as a second group under the subagents, with a group row and subtotal, at the same 60 columns:

```
 ─────────────────────────────────────────────────────────
 team 5 · 3 active · $31.15 · wasted ≈$4.10
   name      model    time   ctx    tok    $     turns
 ● reviewer  opus    41m 10  312k  9.8M  12.40  7/3   idle 9m
 ● builder   opus    38m 02  188k  6.1M   8.10  5/2
 ○ tester    sonnet  12m 44   90k  1.2M   1.05  2/1   done
 ○ writer    haiku    9m 08   44k  0.6M   0.22  1/1   done
 — planner   —           —     —     —     —    —     no transcript
```

Columns: activity glyph (● alive process or a line in the last 5 min · ○ ended · — unknown), teammate name, model family, elapsed since its first line, current context (the teammate's own Panel-1 figure), tokens, cost with its mark, human/machine turns, and a status word. `s` sorts the group by the same keys as the subagent group.

### 4.4 Wasted spend for teammates

The subagent reasons of the first PRD do not transfer directly: a teammate has no `Task` result and no journal. Reasons that are structural for a session:

| Reason | Evidence | Amount |
|---|---|---|
| `idle` | alive (process or recent line) but no API call for ≥ `IDLE_MS` and no pending tool result in its own transcript | its cost since the last human turn boundary in *its* transcript, labelled `idle <age>` |
| `errored` | its last turn ended in an API error line (the lead's `api_error` event type, applied to the teammate's transcript) and no turn followed within `FAILED_AFTER_MS` | the cost of that turn |
| `orphaned` | the member is gone from `config.json` (or the team directory is gone) while its transcript still grows | its cost since the removal was observed |

A teammate's *outcome* is not classified: whether its work reached the lead is a matter of inbox messages cctop does not read (FR-4).

### 4.5 Query, dashboard, pane

- `cctop query agents.teammates[*]` gains `session_id`, `model`, `state`, `elapsed`, `context`, `tokens`, `cost` (`approx`, `source`), `turns` (`{human, machine}`), `waste`; the object gains `team` (`{name, members, read, cost, waste}`).
- `cctop query summary` / `dashboard`: `cost_combined` (first PRD) now includes the team; a new `team_cost` value sits beside `agents_cost`.
- Dashboard row 6 appends `team $X` after the agents part, inside its fixed width.
- The pane's Agents view draws the teammate group after the subagent group with the columns of §4.3.

## 5. User stories

### US-001: Read a live team's layout
**Description:** As the implementer, I need the mapping from a `config.json` member to its transcript, so that the collector opens the right files.

**Acceptance Criteria:**
- [ ] On a live team on the user's machine: record the full key set of one member entry in `config.json` (keys only), the location of each teammate's `.jsonl` (project slug, filename), whether a teammate transcript carries a field naming its team or agent (keys only), and whether teammates write `cost-state` on the same schedule as the lead. Findings go into `docs/hosts.md` or a new `docs/teams.md` and into `harness_facts.rs` (`TEAM_MEMBER_SESSION_KEY`, `READ_FROM`).
- [ ] If the member entry carries no session id, document the fallback order: (1) a teammate transcript whose first lines name the team/agent, (2) transcripts in the lead's project directory whose first line is newer than the team's creation and that are not the lead or its subagents, matched by `agentName`; the collector implements the fallback only if (1) is unavailable.
- [ ] A team fixture is composed: `fixtures/session-c.jsonl` (lead), `fixtures/session-c/teammates/<name>.jsonl` (three teammates, one with a `cost-state`, one without, one missing), and `fixtures/session-c.team.json` (the anonymised `config.json`), via `scripts/anonymise-transcript.py` / `compose-fixture.py` extended for team directories; `--session fixtures/session-c.jsonl` finds the team beside the file, the way fixture A's `subagents/` is found.

### US-002: The teammate collector
**Description:** As every surface, I want each teammate's context, usage, cost and turns from its own transcript.

**Acceptance Criteria:**
- [ ] `src/team.rs`: `Teammate` gains `session_id: Option<String>`, `path: Option<PathBuf>`, `agg: Aggregate`, `cost: CostTracker`, `context: ContextView` inputs, `last_line_at`, `alive: bool` (a `claude` process with that session id in `procs`, else a line in the last 5 min), and `waste: Option<Waste>`; `TeamWatcher` mirrors `AgentWatcher` (watch `~/.claude/teams/<team>` for membership changes and the transcript directory for growth; one `Tailer` per member; `poll()` returns changed).
- [ ] `team::load(session_dir | fixture)` builds the same rows once for `load.rs`; a test loads fixture C both ways and compares.
- [ ] Every line is fed through `transcript::Line::parse` and the existing `Aggregate` / `CostTracker`; no new parsing; no text retained (the `transcript/` module's rules apply unchanged).
- [ ] `attach.rs` rebuilds the watcher on session change; a session that leads no team costs one `read_dir` per scan tick and nothing else.
- [ ] Registry ids: `team_cost` (Panel 2, D12 + D11 + D9, `≈ for the tail after each teammate's last cost-state; ≈ and "N of M read" when a transcript is missing`), `teammate_cost`, `teammate_context`, `teammate_tokens`, `teammate_turns`, `teammate_waste`, `team_waste` (Panel 6).

### US-003: Panel 2 and the headline
**Description:** As a user leading a team, I want the session's money to include the team.

**Acceptance Criteria:**
- [ ] `CostTracker::combined` (first PRD) takes the team's costs; the headline, breakdown line and `where:` attribution render as §4.2; `a` toggles the team with the agents.
- [ ] `cost_combined` in `query summary` / `dashboard` includes the team; `team_cost` exported beside `agents_cost`; the header tile follows.
- [ ] Snapshots on fixture C at 60 × 51; fixtures A and B render character-identical to the first PRD's result (no team, no change).

### US-004: Teammate rows in the agents view and the pane
**Description:** As a user with 20 teammates, I want them in the same ledger as the subagents.

**Acceptance Criteria:**
- [ ] The agents view renders the team group of §4.3 under the subagent group; sort applies within the group; `Enter` on the group row collapses it; the `no transcript` row renders for a member without a file.
- [ ] Waste reasons of §4.4 are unit-tested on synthetic transcripts (idle, errored, orphaned) and on fixture C.
- [ ] `query agents` exports §4.5; `tests/pane/fixtures/agents-c.json` generated by `scripts/pane-fixtures.sh fixtures/session-c.jsonl c`; the pane's Agents view draws the group and its test asserts the columns; dashboard row 6 on fixture C is row-identical on both sides.

### US-005: Budget and drift
**Description:** As a user, I want cctop to stay cheap with 20 teammates and to notice when Claude Code moves the files.

**Acceptance Criteria:**
- [ ] A test builds a 20-member team from fixture C's teammates (copied under a temp dir) and asserts that `TeamWatcher::poll` with no new lines does no file reads beyond the directory scan, and that a 1 000-line append across all files is drained in one poll.
- [ ] `cctop doctor` (or the existing self-check) reports `team: N members, M transcripts found` and warns when a member has no transcript for > 60 s after it appeared, naming the directory it looked in.
- [ ] `make check-types` / `harness_facts` version comparison covers `TEAM_MEMBER_SESSION_KEY`; a newer `claude` than `READ_FROM` prints the existing drift warning.

## 6. Functional requirements

- FR-1: Teammate cost must come from the teammate's own `cost-state` where present and be marked `≈` only for the tail after it; a missing transcript must never be priced as zero without the `N of M read` mark on the sum.
- FR-2: Teammate transcripts are read through the same `transcript::Line` parser and collectors as the lead's; no text field is retained, and inbox files under `~/.claude/teams/**/inboxes/` are never opened.
- FR-3: The collector must be a sibling of `AgentWatcher`: one filesystem watch per directory, one `Tailer` per member reading from its last offset, scans only on events or the 2 s tick.
- FR-4: Waste for teammates is classified only by the structural evidence of §4.4; no message content, no inbox, no description text.
- FR-5: With no team, every surface renders character-identical to the first PRD's result.
- FR-6: `cctop query` values introduced here carry `metric_id`, `approx` and `source`; the registry, `docs/metrics.md` and the README block regenerate; CI fails on drift.
- FR-7: The mapping from member to transcript and the Claude Code version it was read on live in `harness_facts.rs`; the collector falls back per US-001 and reports what it could not find rather than guessing.

## 7. Non-goals

- No fleet view: cctop attaches to one session; teammates are read only because that session leads them. Attaching *to* a teammate as the main session is unchanged and shows that teammate as a normal session.
- No teammate coach: no nudges are computed on a teammate's transcript; the lead's coach sees only the team's cost and waste totals.
- No team burn rate or turn attribution (§11).
- No reading of `inboxes/` or the team's task board, even for counts, in this PRD.

## 8. Technical considerations

- **Budget with 20 teammates.** Each `Tailer` holds an offset and a small buffer; `Aggregate` and `CostTracker` per teammate are a few KB plus the turn vector; 20 of them fit inside the existing 50 MB RSS budget with margin. CPU is dominated by parsing appended lines, which the lead already does for its own transcript at the same rate per teammate; 20 busy teammates at once is the worst case and is the US-005 test. If it exceeds 5 % during a turn, the fallback is parsing teammates on the 2 s tick only, never per event.
- **Process liveness.** `procs.rs` already enumerates `claude` processes and their environment; a teammate's session id in `argv`/environment (to be verified in US-001) gives `alive` exactly; otherwise the 5-minute line recency stands in and is marked in the status word.
- **Fixture composition.** `compose-fixture.py` gains a `--teammates` mode that anonymises a set of transcripts with one shared time shift so that ordering across lead and teammates survives (the first PRD's §1 shows why per-file shifts make ordering questions unanswerable).
- **Session rotation.** A teammate's `/clear` writes `continued-in` into its old transcript (coach PRD v1.2); the watcher follows it the way `attach.rs` follows the lead's.
- **`load.rs` for `cctop query`.** One-shot read of every teammate file found; for a 20-member team this is the same cost as `cctop query` on 20 sessions, bounded by transcript size, and acceptable for a CLI call.

## 9. Design considerations

- **Exactness is the point.** The team column shows Claude Code's number without a mark wherever it can; this is the one place cctop's dollars can be authoritative for the whole group, and the view should let that show.
- **Same table, second group.** No new view; the agents view gains a group with two extra columns that only sessions have (`ctx`, `turns`).
- **Say what is missing.** `5 of 5` / `4 of 5` in the breakdown and a `no transcript` row are the honest states; never a silent zero.

## 10. Open questions

1. **Member to session mapping** (US-001) is the only blocker for the collector; everything else is the existing pipeline.
2. **Ended teammates.** When a teammate finishes, does its member entry stay in `config.json`? If it is removed, its spend would vanish from the total with it; the collector should keep read teammates for the life of the attached session and mark them `gone` rather than drop them (proposed; confirm on a live team).
3. **Nested subagents of teammates.** A teammate can launch its own subagents under its own session directory. Fold them into the teammate's row (`+ N agents ≈$X`) or ignore? Proposed: fold, priced, marked `≈`, in a follow-up after the collector is stable.

## 11. Follow-ups

- Team burn rate (`$/h` over lead + team) and per-human-turn attribution once the first PRD's turn membership follow-up exists.
- Teammates' own subagents (open question 3).
- A `team` LATER rule in the coach when team waste crosses the same threshold as `agents-waste`.
