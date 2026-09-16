# Hand-off: after the team-costs PRD — PR #7

Written 2026-09-16, branch `team-costs` at `190c520` (six commits on top of
`main` `75de20d`), CI green on every push, PR #7 open. Nothing is
released: `Cargo.toml` is still 0.4.0 and the plugin manifest 0.5.0.
Read this, then `tasks/prd-cctop-team-costs.md` (its top box holds the
three decisions; nothing in it was reopened) and `docs/teams.md`.

## What shipped, story by story

| Story | Commit | What it holds |
|---|---|---|
| US-001 | `672a9f0` | `harness_facts::first_seen::TEAM_NAME` (≤ 2.1.232) and the `teams` module; `docs/teams.md`; `Line::team` / `Line::session_id` (`agentName`, `teamName`, `sessionId` on user / assistant / system / attachment lines); `Aggregate.team` and `.session_id`; `AgentResult` / `AgentSpawn` `name` + `team_name`; the header names `<member>@<team>` when the attached session is itself a teammate; fixture D via `compose-fixture.py --teammate` / `--team-config` and the anonymiser's `--team`, directory output and `.json` input. |
| US-002 | `c180c58` | `src/team.rs`: `Teammate` (own `Aggregate` + `CostTracker`, `Liveness`, §4.4 waste), `Team` (config → spawns → head scan, merged by name; `read`, `missing`, `looked_in`), `team::load` for the one-shot paths, `TeamWatcher` beside `AgentWatcher`; `State.team`; `load.rs` / `attach.rs` wiring; registry ids `team_cost`, `teammate_*`, `team_waste`; sources D2c and D12 in the PRD table and the legend; docs, README block and site regenerated. `CostTracker` is `Clone`. |
| US-003 | `e17b90b` | `CostTracker::combined(agents, team)`; `State::cost_combined` / `team_cost` / `team_cost_share`; Panel 2's breakdown `team ≈$X (P %, N of M[ read])` via `panels::tokens::team_part` (shared with dashboard row 2 and `cctop report`); `where:` counts the team; `a` toggles it; `summary.team_cost` only with a team; the coach's limits light gains a `team` line. Snapshots `tokens_d_120x30`, `tokens_d_56x20`. |
| — | `b801899` | The watcher ignores inotify `Access` events (CI had the tailers' own reads rescanning the team every poll) and never watches a fixture's team-file directory. |
| US-004 | `ce6a420` | `agent_ledger::teammate_rows` / `team_totals` / `TeammateRow::status_word`; the agents view's team group (`Entry::TeamGroup` / `TeamColumns` / `Teammate`, Enter folds it); `query agents` §4.5 (`teammates[]` rows + `team` object, only with a team); dashboard row 6 `team ≈$X · N of M read · team wasted ≈$Y`; the pane's `views/agents.tsx` group and rows, title `1/3 team`; `tests/pane/fixtures/*-d.json`; snapshots `agents_d_120x30`, `agents_d_56x20`; `docs/verification/pane.md` item 27 (`Result: pending`). |
| US-005 | `190c520` | The 20-member budget test (`twenty_teammates_cost_no_reads_when_quiet_and_drain_an_append_at_once`); the drift warning names the `teams` module; `tail::TEST_TAILERS`, a test-only lock for tests that count live tailers. |

Fixtures A, B and C render character-identical throughout: no existing
snapshot moved, and `scripts/pane-fixtures.sh` for A, B and C regenerates
their JSON byte-for-byte (the new keys — `team_cost`, `team`, the header's
`team` — are omitted when there is no team, not written as `null`).

## Checked on this machine's real sessions (not fixtures)

- `cctop query agents --session 83f0e9b9` (the lead fixture D came from;
  its team directory is gone): `team.source: "transcripts"`, one member,
  `ended`, cost `0.3149` with `source: ledger`; `cost_combined` exact.
- `cctop query agents --session 12f423aa` (the `Workflow`-made team with
  no `teammate_spawned` result): four `rev-*` reviewers found from the
  transcripts alone, all `ended` with ledgers. The release binary takes
  1.5 s on that 11.6 MB lead and 0.17 s with `--lines 1`, so the team
  scan and the four teammate reads cost ≈ 0.1 s.

## Open, in order

1. **Live check 27** (`docs/verification/pane.md`): the pane's team group
   and Panel 2's team part on a real team — the person's, `Result:
   pending`. Items 25 and 26 are still pending too.
2. **Release.** `release: v0.5.0 — teammate spend`: `Cargo.toml` 0.5.0,
   `cargo build` for the lock, plugin manifest 0.6.0 (the pane changed),
   the install lines, `docs/mcp.md`'s version notes (it already says "the
   team in 0.5.0"), `make site`, the annotated tag after the rebase-merge.
   `make check-types` warns that `READ_FROM = "2.1.270"` — the binary's
   constants are unverified against 2.1.271 – 2.1.273; the team facts were
   read on 2.1.232 – 2.1.272 transcripts.
3. **Two team sources sit side by side on purpose.** `State.teammates`
   (`agents::teammates`, the config's member list *including the lead*)
   still feeds Panel 6's `team N …` line and the registry id `teammates`,
   because the PRD's box keeps Panel 6 as it is; `State.team` is the
   collector. A future story could derive the Panel 6 line from
   `State.team` once Panel 6 is allowed to change.
4. **Follow-ups the PRD lists (§11):** a team burn rate, teammates' own
   subagents (`+ N agents ≈$X` on the row), a `team` LATER coach rule at
   the `agents-waste` threshold, folding teammates' ledgers into
   `baseline.rs`. Open question §10.1 (a member in another `cwd`) is
   handled through the config's `cwd` slug and untested on a real team.

## Working practices that held

As `tasks/handoff-after-v0.4.0.md`: edits by small substitutions; `make
check`, `npm run typecheck && npm test`, `make check-types` and `claude
plugin validate --strict ./plugin` before every commit; one commit per
story with the `Co-Authored-By` trailer; push and `gh run list` (CI runs
on pull requests, so the draft PR was opened after US-002). Regenerated,
never hand-edited: `docs/metrics.md` and the README block (`cctop metrics
--md` / `--readme`), the insta snapshots (`INSTA_UPDATE=always cargo test`,
then strip `assertion_line:` and delete `*.snap.new`), the pane fixtures
(`scripts/pane-fixtures.sh …`), `make site` after the README moved. The
Python scripts under `scripts/` are not executable in git: run them as
`python3 scripts/….py`.
