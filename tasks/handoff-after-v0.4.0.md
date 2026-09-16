# Hand-off: after v0.4.0 — the team-costs PRD

Written 2026-09-16, `main` at `cf77392`, CI green, **v0.4.0 released**
(four targets, `SHA256SUMS`, the tap formula at 0.4.0, `cargo publish
--dry-run` green), plugin manifest **0.5.0**, the function-hooks contract at
Claude Code **2.1.273**, which is also what this machine runs. Read this,
then `tasks/prd-cctop-team-costs.md` in full, then two earlier hand-offs
for the parts they own: `tasks/handoff-team-costs.md` § "The shape the team
PRD lands in" (the code the collector plugs into) and
`tasks/handoff-agent-costs.md` § "Fixtures C and D — where the sources are"
(fixture D's sources on this machine) and § "Decisions the person has
already taken".

## Where things stand

- **The agent-costs PRD shipped.** PR #6 was rebase-merged (so its shas
  changed: `2fef773` is the code review's findings, `2a1a1a8` the contract,
  `876d41e` the release) and tagged `v0.4.0`. It holds: the combined cost
  (`CostTracker::combined`, `Cost.source`, the ledger's moment
  `authoritative_at_ms`) on Panel 2, dashboard row 2, `cctop report` and
  `cost_combined` in `cctop query` (`cost` stays main-only for one
  release); the agents view under Panel 6 (`ui/agents_view.rs`, fed by
  `agent_ledger::{rows, totals, workflow_groups}`); `cctop query agents`,
  the `cctop_agents` MCP tool, dashboard row 6 by waste; the
  `<task-notification>` parser (`transcript::task_notification`,
  `State.agent_links`); fixture C; and A48 `agents-waste` (36 rules). The
  branch `agent-costs` still exists locally and on origin with the
  pre-rebase shas; nothing cites them, so it can go.
- **The contract is at 2.1.273** (`2a1a1a8`): the regenerated d.ts adds a
  `vscode` render surface and rewords Button/Svg/surface docs; nothing the
  pane calls changed and it never narrows on `e.surface`. `TESTED_WITH =
  '2.1.273'`, the header badge says so, and the headless load check on
  2.1.273 is recorded in `docs/verification/pane.md` §A. `make check-types`
  is green here and still *warns*: `harness_facts::READ_FROM = "2.1.270"` —
  the binary's constants (autocompact buffer, `/usage` weights, `/context`
  thresholds, the first-seen map) are unverified against 2.1.271–2.1.273.
  Issue #4 item 5 is the script that would make that bump mechanical.
- **This machine is not on the release yet.** brew's `cctop` is 0.3.1 (the
  tap has 0.4.0), the installed plugin is 0.4.1 (the cache has 0.4.1 as its
  newest). Sessions started today run 0.4.1 against binary 0.3.1; the pane's
  new columns show `—` on that pair by design.
- **Live checks pending, all the person's:** `docs/verification/pane.md`
  item 26 (the pane's Agents view columns and the combined cost — the
  acceptance test of 0.4.0's pane half) and item 25 (issue #3's clock fix on
  ≥ 2.1.271; #3 stays open until it passes). Issue #4 (CI never runs against
  a real Claude Code) is open with its six-item order in
  `tasks/handoff-after-v0.3.1.md`.
- **Coach dogfood** (`tasks/handoff-coach-round-2.md`): the TUI half runs;
  the pane half waits on item 25; the `coach-stats` review is due around
  2026-09-29.
- A cosmetic race, still there: the first marker's `version` is `null` in
  a one-turn headless run (`readVersion` and the first `writeMarker` run
  concurrently); live markers carry it. Issue #4 item 3 owns it.

## First: get this machine onto the release (optional, but the dogfood needs it)

The session can run these; the restart is the person's.

```
brew update && brew upgrade cctop && cctop --version          # cctop 0.4.0
claude plugin marketplace update cctop && claude plugin update cctop@cctop   # plugin 0.5.0
```

A fresh Claude Code (function hooks on, `/tui fullscreen`, ≥ 110 columns)
then runs 0.5.0 on 2.1.273 against binary 0.4.0 — the pair item 26 asks
for. If the person wants to do item 26 now, walk them through it as
`handoff-after-v0.3.1.md` does for item 25: run what the session can
(`cctop query agents --session <id>` beside the pane's rows, the marker,
the debug log greps), say exactly what to look at for the rest, and take
their `Result:` line down verbatim — never write one.

## Then: the team-costs PRD, US-001 → US-005

Branch `team-costs` from `main`. One commit per story; `make check`,
`npm run typecheck && npm test`, `make check-types` and `claude plugin
validate --strict ./plugin` before each; push and `gh run list`. The PRD's
top box holds three decisions the person took (teammates join the combined
headline and the agents view, Panel 6 unchanged; provenance is the
teammate's own `cost-state`, priced only after it; the collector is the
`AgentWatcher` mechanism, budget in §8) — do not reopen them.

What the PRD assumes that the code now has, or does not yet have:

1. **US-001 — facts, fixture D, the two parsers.**
   - `harness_facts.rs` already has `cost_state` and `task_notification`
     modules with `READ_FROM`-style provenance comments; add
     `first_seen::TEAM_NAME` and the `teams` module beside them, and
     `docs/teams.md` (keys only — no names, no prompts).
   - `transcript::Line` metadata gains `agent_name` / `team_name`;
     `Aggregate` records the first pair. `AgentResult` gains `name` and
     `team_name` from `teammate_spawned`; `AgentSpawn` carries them.
     `State::note_agent_spawn` (`ui/state.rs`) deliberately skips a
     `teammate_spawned` result's `<name>@<team>` id today — that is the hook
     the team side reads, so keep the subagent ledger ignoring it.
   - **`--shared-shift` does not exist yet.** `compose-fixture.py` has
     `--agent-killed` (a segment found by predicate) and `copy_agent`
     (the subagent's files copied with that segment's delta — one shift
     across the set already, for `subagents/`). Fixture D needs a sibling of
     `copy_agent` that writes `fixtures/session-d/teammates/<sessionId>.jsonl`
     and a `--teammate` finder on `teamName` / `agentName`; the one id map
     across lead, teammates and `session-d.team.json` is the new part. The
     anonymiser keeps `agentId` verbatim; add `agentName` and `teamName` to
     `KEEP_KEYS` (roles, and the team key must match the config file), and
     `isActive`, `backendType`, `joinedAt`; `cwd` is rewritten like every
     path. Sources: `handoff-agent-costs.md` § "Fixtures C and D". Add D's
     paragraph to `fixtures/README.md`.
2. **US-002 — the collector** (`src/team.rs`, `TeamWatcher`): a sibling of
   `agents.rs::AgentWatcher`; `agents.rs::teammates()` already reads the
   config directory and includes the lead (skip it; a one-member team is no
   team). Discovery order config → spawns → head scan is the PRD's §5; FR-7
   (found from the transcripts alone) is what fixture D tests, since its
   team directory is gone. A teammate's own `cost-state` is a second ledger:
   `Agent.calls` is placed against the *main* ledger's moment by
   `CostTracker::agents_after`; a teammate needs its own moment from its
   own `CostTracker`. `attach.rs` rebuilds every collector on session
   change; `load.rs` is the one-shot path for `cctop query` and the
   fixtures (`session-d/teammates/` found beside the file like A's
   `subagents/`). Registry: `team_cost`, `teammate_*`, `team_waste`, and the
   new source `D2c` in `prd-cctop.md`'s table and `docs/metrics.md`'s
   legend (which also lacks `D12`).
3. **US-003 — Panel 2.** `CostTracker::combined` takes the team's part;
   `≈` while any teammate runs, `N of M read` when a transcript is missing;
   `a` toggles team with agents. Fixtures A, B, C must render
   character-identical to today — the insta snapshots are the guard.
4. **US-004 — rows.** Teammate rows go through `agent_ledger` so the TUI,
   `query agents`, dashboard row 6 and the pane cannot disagree; the pane's
   `views/agents.tsx` draws from the query JSON and `tests/pane` asserts
   row-identity. `scripts/pane-fixtures.sh fixtures/session-d.jsonl d` makes
   the `-d` fixtures. Waste for teammates is §4.4's structural evidence only.
5. **US-005 — budget and drift.** The 20-member test (no reads with no new
   lines; a 1 000-line append drained in one poll); `team.read` /
   `team.missing` / `team.looked_in` in `query agents`; the `teams` module
   under the `check-types` comparison.

Regenerate, never hand-edit, when the tests say so: `cctop metrics --md >
docs/metrics.md && cctop metrics --readme README.md`; `INSTA_UPDATE=always
cargo test` then strip `assertion_line:` and delete `*.snap.new`;
`scripts/pane-fixtures.sh` for A, `… fixtures/session-b.jsonl b` for B and
the coach moments, `… fixtures/session-c.jsonl c` for C, and D once it
exists; `make site` after a README change.

## Working practices that held

Edits by small substitutions; `make check`, `npm run typecheck && npm test`,
`make check-types` and the validator before every commit; commit per story
with the trailer `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`;
push and `gh run list` (CI's clippy is newer than the local one — `d844c9e`
was such a fix). Commit subjects are prose (`Agents: …`, `Coach: …`,
`Docs: …`, `Hand-off: …`). PRs are rebase-merged. A release is one commit
`release: vX.Y.Z — …`: `Cargo.toml`, `cargo build` for the lock, the plugin
manifest when the pane changed, the install line in both READMEs and
`docs/mcp.md`'s version notes, `make site`, then an annotated tag on
`main`'s tip pushed after the merge — the workflow builds four targets,
publishes the release with `SHA256SUMS`, pushes the tap formula and
dry-runs `cargo publish`. Before a release, `make check-types` must be
green: regenerate the d.ts against the installed `claude` (from a scratch
copy of `plugin/.claude`, diff, then copy in), bump `TESTED_WITH`, run the
headless load check and record it in `docs/verification/pane.md` §A. Never
write a `Result:` line there; never run `cctop run` or `claude` in the
foreground of a tool call (`claude -p … --max-turns 1` is fine); read-only
rule and validator constraints as in `CLAUDE.md`.

## Hand-off prompt

```
Continue cctop after the v0.4.0 release per @tasks/handoff-after-v0.4.0.md
(read it, then tasks/prd-cctop-team-costs.md in full, then the two sections
it names in tasks/handoff-team-costs.md and tasks/handoff-agent-costs.md).
main is at cf77392, CI green, v0.4.0 released, plugin 0.5.0, Claude Code
2.1.273 on this machine. Branch team-costs from main and implement the
team-costs PRD in story order: US-001 (facts, docs/teams.md, the two parser
additions, fixture D via a --teammate mode of compose-fixture.py), US-002
(src/team.rs, TeamWatcher, load.rs, the registry ids and source D2c),
US-003 (Panel 2 and the headline), US-004 (teammate rows through
agent_ledger, query agents, the pane, the -d fixtures), US-005 (the
20-member budget test, team.read/missing/looked_in, drift). One commit per
story; make check, npm run typecheck && npm test, make check-types and
claude plugin validate --strict ./plugin before each commit; regenerate
docs/metrics.md, the README block, the insta snapshots and the pane fixtures
when the tests say so; push and check gh run list. Fixtures A, B and C must
render character-identical throughout. Do not reopen the decisions in the
PRD's top box. Read-only rule and validator constraints as in CLAUDE.md.
```
