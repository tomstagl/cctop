# Hand-off: after the team-costs PRD — release, then the spend follow-ups PRD

Written 2026-09-16, `main` at `0428a3b`, CI green, **PR #7 rebase-merged**
(the team-costs PRD, six story commits), **nothing released since v0.4.0**:
`Cargo.toml` is 0.4.0, the plugin manifest 0.5.0, the function-hooks
contract at Claude Code 2.1.273 (what this machine runs). Read this, then
`tasks/prd-cctop-team-costs.md` and `tasks/prd-cctop-agent-costs.md` §10–11
(the follow-ups the next PRD is made of), `docs/teams.md`, and
`tasks/handoff-after-v0.3.1.md` § "Then: issue #4" (its six-item order still
stands).

## Where things stand

- **The team-costs PRD shipped** (`tasks/prd-cctop-team-costs.md`; its
  status line still reads "draft, not implemented" — the repo records
  shipping in hand-offs, as for the agent PRD). The rebase rewrote the
  branch's shas; on `main`:

  | Story | Commit | What it holds |
  |---|---|---|
  | US-001 | `2f603f7` | `harness_facts::first_seen::TEAM_NAME` (≤ 2.1.232) and the `teams` module; `docs/teams.md`; `Line::team` / `Line::session_id` (`agentName`, `teamName`, `sessionId` on user / assistant / system / attachment lines); `Aggregate.team` and `.session_id`; `AgentResult` / `AgentSpawn` `name` + `team_name`; the header names `<member>@<team>` when the attached session is itself a teammate; fixture D via `compose-fixture.py --teammate` / `--team-config` and the anonymiser's `--team`, directory output and `.json` input |
  | US-002 | `83f2008` | `src/team.rs`: `Teammate` (own `Aggregate` + `CostTracker`, `Liveness`, §4.4 waste), `Team` (config → spawns → head scan, merged by name; `read`, `missing`, `looked_in`), `team::load` for the one-shot paths, `TeamWatcher` beside `AgentWatcher`; `State.team`; `load.rs` / `attach.rs`; registry ids `team_cost`, `teammate_*`, `team_waste`; sources D2c and D12 in the PRD table and the legend |
  | US-003 | `1b35c21` | `CostTracker::combined(agents, team)`; `State::cost_combined` / `team_cost` / `team_cost_share`; Panel 2's `team ≈$X (P %, N of M[ read])` via `panels::tokens::team_part` (shared with dashboard row 2 and `cctop report`); `where:` counts the team; `a` toggles it; `summary.team_cost` only with a team; the coach's limits light gains a `team` line |
  | — | `d2d40a9` | The watcher ignores inotify `Access` events (CI: the tailers' own reads rescanned the team every poll on Linux) and never watches a fixture's team-file directory |
  | US-004 | `efaf971` | `agent_ledger::teammate_rows` / `team_totals` / `TeammateRow::status_word`; the agents view's team group (Enter folds it); `query agents` §4.5 (`teammates[]` rows + `team`, only with a team); dashboard row 6 `team ≈$X · N of M read · team wasted ≈$Y`; the pane's group and rows, title `1/3 team`; `tests/pane/fixtures/*-d.json`; `docs/verification/pane.md` item 27 (`Result: pending`) |
  | US-005 | `4de3d95` | The 20-member budget test; the drift warning names the `teams` module; `tail::TEST_TAILERS`, a test-only lock for tests that count live tailers |
  | — | `30cba65`, `0428a3b` | The first hand-off (pre-rebase shas) and the README's team paragraphs |

  Fixtures A, B and C render character-identical (no snapshot moved; their
  pane fixtures regenerate byte-for-byte — the new keys `team_cost`, `team`
  and the header's `team` are omitted without a team, never `null`).
- **Checked on this machine's real sessions:** `cctop query agents
  --session 83f0e9b9` (the lead fixture D came from; its directory is gone)
  finds its one teammate from the transcripts alone with an exact ledger;
  `--session 12f423aa` (the `Workflow`-made team with no `teammate_spawned`
  result) finds the four `rev-*` reviewers. Release binary: 1.5 s on that
  11.6 MB lead, 0.17 s with `--lines 1`, so the team scan costs ≈ 0.1 s.
- **Two team sources sit side by side on purpose.** `State.teammates`
  (`agents::teammates`, the config's member list *including the lead*)
  still feeds Panel 6's `team N …` line and the registry id `teammates`,
  because the PRD's box keeps Panel 6 as it is; `State.team` is the
  collector. Deriving the Panel 6 line from `State.team` needs the person
  to lift that decision (a candidate below).
- **Live checks pending, all the person's:** `docs/verification/pane.md`
  items 25 (issue #3's clock fix), 26 (the pane's Agents view and the
  combined cost) and 27 (the team group and the team's part). Issues #3
  and #4 are open. `make check-types` still *warns*: `harness_facts::
  READ_FROM = "2.1.270"` — the binary's constants are unverified against
  2.1.271–2.1.273 (the team facts were read on 2.1.232–2.1.272
  transcripts, which is a different question).
- **Coach dogfood** (`tasks/handoff-coach-round-2.md`): the TUI half runs;
  the pane half waits on item 25; the `coach-stats` review is due around
  2026-09-29.
- The branch `team-costs` still exists locally and on origin with the
  pre-rebase shas; nothing cites them, so it can go (as can `agent-costs`).

## First: release v0.5.0 (the person tags; the session prepares)

One commit `release: v0.5.0 — teammate spend`, as `handoff-after-v0.4.0.md`
describes: `Cargo.toml` 0.5.0 and `cargo build` for the lock; the plugin
manifest to **0.6.0** (the pane changed: `views/agents.tsx`); the install
lines in both READMEs; `docs/mcp.md`'s version notes (it already says "the
team in 0.5.0"); `make site`; then an annotated tag on `main`'s tip pushed
after the commit lands — the workflow builds four targets, publishes the
release with `SHA256SUMS`, pushes the tap formula and dry-runs `cargo
publish`. Before it: `make check-types` must be green (the d.ts is at
2.1.273 — regenerate only if `claude --version` moved), the headless load
check of `docs/verification/pane.md` §A recorded, `TESTED_WITH` bumped if
so. Then `brew upgrade cctop && claude plugin update cctop@cctop` here, and
walk the person through items 25–27 as `handoff-after-v0.3.1.md` does for
25: run what the session can (`cctop query agents --session <id> | jq
.team` beside the pane's rows, the marker, the debug-log greps), say what to
look at for the rest, and take their `Result:` line down verbatim — never
write one.

## Then: write the spend follow-ups PRD and its plan, and implement them

Both spend PRDs left follow-ups (§11 of each) that are now one coherent
piece of work: the money is complete per surface, but not yet attributed
over time or to turns, and two collectors are one level short. Write
`tasks/prd-cctop-spend-attribution.md` (v1.0, same shape as the two before
it: a decisions box at the top, §3 findings measured on this machine, user
stories with acceptance criteria, FRs, non-goals, budget) and
`tasks/plan-cctop-spend-attribution.md` (phases → commits, like
`plan-cctop-coach.md` §1 keeps), get the person's decisions on the box, then
implement in story order with the practices below. Candidate scope, with
the code each item rests on:

1. **Team burn rate** (team PRD §11, §4.2's `$/h main`). `cost::rates(agg,
   pricing, now)` (`src/metrics/cost.rs`) is trailing-15-minute over
   `agg.turns`; every `Teammate` has its own `agg`, so a team rate is the
   sum of `rates(&m.agg, …)` over live members (`Team::members`) — the
   `main` suffix on Panel 2's headline (`panels/tokens.rs` l6) and the
   registry's `burn_rate` (D2, D9) are what change. Decide whether `$/h`
   becomes combined with `a` on, or a second figure.
2. **Teammates' own subagents** (team PRD §10.2). A teammate's
   `subagents/` sits beside its transcript (`path.with_extension("")`),
   readable with `agents::load`; its `cost-state` already holds them
   (`harness_facts::cost_state::INCLUDES_SUBAGENTS`), so the row's extra is
   `m.cost.agents_after(agents)` — the same rule as the lead's — shown as
   `+ N agents ≈$X` on the teammate row (`agent_ledger::TeammateRow`,
   `agents_view::teammate_line`, the pane's `teammateCells`) and folded
   into `Team::cost`. Watch the budget: 20 members × one `AgentWatcher`.
   Measured 2026-09-16: **none of the 23 teammate transcripts on this
   machine has a `subagents/` directory**, so this item has no fixture
   source yet and may be the one to defer until a teammate launches an
   agent for real.
3. **Turn membership for agents and teammates** (agent PRD §11).
   `tools::AgentSpawn.turn` names the launching turn; attribute an agent's
   spend (and a teammate's, by its spawn turn) to it so `last turn $`,
   the gradient (`State::gradient`) and `$/h` become combined. This is the
   one item that changes `Turn` (`metrics/usage.rs`); the coach's per-turn
   rules read `agg.turns`, so the plan needs a replay check
   (`cctop coach-replay`) that no rule's fire count moves on fixtures A–D.
4. **A `team` LATER coach rule** (team PRD §11) at the `agents-waste`
   threshold: A48 (`advisor/rules/`, id `agents-waste`) reads
   `agent_ledger::totals`; its sibling reads `team_totals(state,
   &teammate_rows(…)).waste_usd`, urgency LATER, `acted` when the agents
   view was opened (`State.agents_view_opens`, as A48). `SessionMode::Team`
   already mutes `/clear`, waiting and cold-resume nudges for team leads —
   check none of them should fire for the *lead* now that its team is
   visible. 36 rules today; the registry, `docs/metrics.md` and the guide
   (`make site`) regenerate.
5. **Folding teammates' ledgers into the baseline** (team PRD §8, §11).
   `baseline.rs` reads finished sessions' `cost-state` per file over the
   last 7 days; a lead's figure excludes its teammates. Folding means a head
   scan for `teamName` over the window's files and adding each teammate's
   ledger to its lead's — decide whether the median should be per *lead*
   session (team folded) or per *session file* (today). The PRD wanted a
   week of combined cost live first; that week starts with the release.
6. **`dup` for agents** (agent PRD §11): measure on the corpus how often
   two agents of the same type and description hash overlap and whether
   the second's result was empty, before deciding a reason exists. Research
   on `~/.claude/projects`, not code, and the person vetoed `dup` for v1.
7. **Panel 6's team line from `State.team`** — only if the person lifts
   "Panel 6 stays as it is" (team PRD box, decision 1). Today the line
   comes from `agents::teammates` (config only, lead included); from
   `State.team` it would survive the directory going and show `N of M
   read`. Small; a decision, not a design.

Two things to find out before writing §3 (both need a real team on this
machine, or a corpus grep):

- **The lead's `/clear`.** The team is named by the lead's session id at
  spawn time (`session-<id8>`); `/clear` gives the lead a new id, so the
  new transcript's `team_name()` no longer matches the teammates' lines,
  and `attach.rs` re-attaches to the new id. Does Claude Code keep the
  team directory under the old name, rename it, or end the team? Look at
  a lead that ran `/clear` with teammates alive (`continued-in` in a
  transcript whose id8 names a `~/.claude/teams/` directory). If the
  directory keeps the old name, the collector needs the *original* lead id
  (the `continued-in` chain backwards) — a fourth discovery step.
- **A member in another `cwd`** (team PRD §10.1): every team seen shared
  the lead's `cwd`; the config path handles another slug
  (`Layout::scan_dirs_with`), the transcript path does not. One such team,
  or leave it as a caveat.

Non-goals to keep: no fleet view, no reading of `inboxes/` or
`<teammate-message>` bodies, no process mapping for teammates, no coach on
a teammate's transcript.

## Issue #4 stays in its own lane

The six items of `handoff-after-v0.3.1.md` § "Then: issue #4" (the fake
engine follows the contract; `cctop pane status` knows the broken pair; one
self-check line in `pane.tsx`; CI with `claude` on the runner; the
`harness_facts` re-verification script and the `READ_FROM` bump; the release
checklist in `CLAUDE.md`) are unchanged and independent of the spend work.
Item 5 is the one this PRD touches: the `teams` module now sits under the
same drift warning (`scripts/check-plugin-types.sh` names it), so the script
that re-verifies the constants should also diff `docs/teams.md`'s keys
against a fresh teammate transcript. Do issue #4 before or after the spend
PRD as the person prefers; the release does not wait for it.

## Working practices that held

Edits by small substitutions; `make check`, `npm run typecheck && npm test`,
`make check-types` and `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin
validate --strict ./plugin` before every commit; one commit per story with
the trailer `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; a draft
PR opened after the first story (CI runs on pull requests and on `main` /
`ralph/**` pushes only), `gh run list --branch <b>` after each push, the
description completed and `gh pr ready` at the end; PRs are rebase-merged,
so cite shas as "on the branch" until they land. Fixtures A, B, C and now D
render character-identical unless a story says otherwise — the insta
snapshots and a `scripts/pane-fixtures.sh` regeneration with `git status`
are the proof. Regenerate, never hand-edit: `cctop metrics --md >
docs/metrics.md && cctop metrics --readme README.md`; `INSTA_UPDATE=always
cargo test`, then strip `assertion_line:` and delete `*.snap.new`;
`scripts/pane-fixtures.sh` for A, `… fixtures/session-b.jsonl b` for B and
the coach moments, `… fixtures/session-c.jsonl c`, `… fixtures/session-d.jsonl
d`; `make site` after a README change; the fixture composer and anonymiser
for any fixture (`fixtures/README.md` has every recipe; the Python scripts
are not executable in git — `python3 scripts/….py`). Tests that count live
tailers or open many at once hold `tail::TEST_TAILERS`. A directory watcher
filters `notify::EventKind::Access(_)` (inotify reports the tailers' own
reads) and never watches a fixture's parent recursively. Never write a
`Result:` line in `docs/verification/pane.md`; never run `cctop run` or
`claude` in the foreground of a tool call (`claude -p … --max-turns 1` is
fine; its debug log is `~/.claude/debug/latest`); read-only rule and
validator constraints as in `CLAUDE.md`. Commit subjects are prose
(`Teams: …`, `Agents: …`, `Coach: …`, `Docs: …`, `Hand-off: …`, `release:
vX.Y.Z — …`).

## Hand-off prompt

```
Continue cctop after the team-costs PRD per @tasks/handoff-after-team-costs.md
(read it, then tasks/prd-cctop-team-costs.md and tasks/prd-cctop-agent-costs.md
§10–11, docs/teams.md, and tasks/handoff-after-v0.3.1.md § "Then: issue #4").
main is at 0428a3b, CI green, PR #7 merged, nothing released since v0.4.0
(Cargo.toml 0.4.0, plugin 0.5.0, Claude Code 2.1.273 on this machine).
First prepare the v0.5.0 release commit (Cargo.toml 0.5.0, the lock, plugin
manifest 0.6.0, install lines, docs/mcp.md notes, make site; check-types
green, the headless load check recorded in docs/verification/pane.md §A)
and stop for me to tag; then walk me through live checks 25–27 and take my
Result: lines down verbatim. Then write tasks/prd-cctop-spend-attribution.md
(v1.0, decisions box, §3 measured on this machine — including what the
lead's /clear does to a team — stories with acceptance criteria) and
tasks/plan-cctop-spend-attribution.md from the hand-off's candidate list,
stop for my decisions on the box, then branch spend-attribution from main
and implement it in story order: one commit per story; make check, npm run
typecheck && npm test, make check-types and claude plugin validate --strict
./plugin before each commit; regenerate docs/metrics.md, the README block,
the insta snapshots and the pane fixtures when the tests say so; draft PR
after the first story, push and check gh run list. Fixtures A, B, C and D
must render character-identical unless a story says otherwise. Do not
reopen the decisions in the two spend PRDs' top boxes. Read-only rule and
validator constraints as in CLAUDE.md.
```
