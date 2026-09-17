# Hand-off: after v0.7.0 (issue #4 shipped) — the live checks, then the spend-attribution PRD

Written 2026-09-17 on branch `issue-4` (PR #9, `Closes #4`), seven commits
over `main`: the six items of the issue in the hand-off's order, then the
release commit. This machine: Claude Code **2.1.274** (it moved from 2.1.273
overnight — the first thing the new checks caught, see below), `cctop 0.6.0`
from the tap and plugin 0.7.0 installed until the release lands. Read this,
then the PR's description (one row per item, with the acceptance criteria
of issue #4 answered), `CLAUDE.md` § "Release checklist" (what this session
ran, in order), `docs/verification/pane.md` §A and items 25–32, and
`tasks/handoff-after-team-costs.md` § "Then" for the work that follows.

## Where things stand

| What | Where |
|---|---|
| Issue #4, items 1–6 | `7489798` fake engine `satisfies` the contract · `b6e1ae7` `cctop pane status` pairs plugin and Claude Code (`KNOWN_INCOMPATIBLE`, `TESTED_WITH`) · `4f00acb` one self-check line at `session.start`, the marker carries `testedWith` / `selfCheck`, every clock reading falls back · `0b285bb` CI `contract` job (`scripts/check-contract.sh`: the §A load check with no model call, the surface diff, the validator, the pins; every push and daily) · `0023db0` `scripts/check-harness-facts.py`, three constants corrected, `make check-facts` · `9e07711` the release checklist in `CLAUDE.md`, the versions table in `plugin/README.md` |
| The release commit | `release: v0.7.0 — the contract watched`: Cargo 0.7.0 and the lock, plugin manifest **0.8.0**, the d.ts regenerated against **2.1.274**, `TESTED_WITH` and `READ_FROM` 2.1.274, the install lines, the site, the versions table's 0.8.0 row, the §A run recorded |
| PR #9 | CI green on the six items against 2.1.273 (the `contract` job's first run on a runner passed); the release commit's run is the one to read before merging |
| Tests | `cargo test` 360 passed, 2 ignored; `npm test` 121; `make check-contract`, `make check-types`, `make check-facts` green on 2.1.274 |

**The checklist ran for real on its first day.** `make check-contract` on
this machine (step 2 of the release checklist) failed twice: Claude Code
had moved to 2.1.274 and the contract surface changed — the script named
32 declarations (`AgentSpawnResult`, `BoxProps`, `CoreEngineInterface`,
`UiPressArgument`, …) and `check-plugin-types` the version pin. The load
check itself passed on 2.1.274 (module loaded, `self-check ok`), so the
pane was never broken. The change is additive: `$.agent.register` and its
`AgentSpec`, `position: "absolute"` with `top`/`left`/`right`/`bottom` on
`Box`, a `Markdown` element, `PressedLink`, `UserMessageFrom`/`UserMessageTask`;
the 49 removed lines are the header and comment rewording. `npm run
typecheck` against the regenerated d.ts named no call site, `make
check-facts` found every probed constant unchanged, so `READ_FROM` and
`TESTED_WITH` moved to 2.1.274 by the mechanical route. Steps 1–4 of the
checklist as written in `CLAUDE.md` were enough; nothing was added to it.

- **`Closes #4`** closes the issue on merge. **Issue #3 stays open** until
  the person records item 25's `Result:` line (the header badge, the
  lights filled) — its log half read clean on 2.1.273 / plugin 0.7.0 on
  2026-09-17 (the run notes at the end of `docs/verification/pane.md`);
  then `gh issue close 3`.
- **The tag** goes on `main`'s tip after the rebase-merge (`git fetch`
  first — the sha changes): `git tag -a v0.7.0 -m "v0.7.0 — the contract
  watched" && git push origin v0.7.0`. The release workflow builds the
  four targets, the GitHub release with `SHA256SUMS` and the tap formula;
  then `brew upgrade cctop && claude plugin marketplace update cctop &&
  claude plugin update cctop@cctop` and a Claude Code restart (the session
  that ran the release cannot survive it).
- **The daily `contract` job** now runs against whatever `@anthropic-ai/claude-code`
  npm resolves as latest. When it goes red the step summary names the
  declarations that moved; the fix is the release checklist, and the
  `plugin/README.md` versions table gets a row when the change forced a
  plugin release.
- **Branches that can go** (the person deletes): `issue-4` after the
  merge, `console`, `origin/claude/hopeful-johnson-9h0lo8`,
  `origin/agent-costs`, `origin/team-costs` — all merged, none cited by sha.

## First: live checks 25–32 (the person's; the session assists)

Unchanged from `tasks/handoff-after-v0.6.0.md` § "First": restart Claude
Code so plugin 0.8.0 loads, a second terminal at 162 columns with
`CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --debug`, item 25 first, then
28 → 29 → 30 → 32 on the same docked pane, then 31 at 110 / 132 / 162 / 200.
What 0.8.0 adds to item 25's log half: `grep 'cctop: plugin ' ~/.claude/debug/latest`
gives one line ending `self-check ok`, and `cctop pane status` shows the
`compatibility` line `✓ cctop plugin 0.8.0 tested with Claude Code 2.1.274`
(a `!` if Claude Code has moved again by then — the line says what to do).
Item 31's default ladder is still unread: `~/.claude.json` holds
`pluginPanes.dockColumns` (a persisted `ctrl+x` resize), which replaces the
ladder; remove the key with Claude Code stopped to read the default, or
record which of the two was set. Items 26 and 27 need agents / teammates;
last, or pending. **Take the person's `Result:` lines down verbatim — never
write one.**

## Then: the spend-attribution PRD

`tasks/handoff-after-team-costs.md` § "Then" has the seven candidate items
with the code each rests on, the two things to find out first (the lead's
`/clear` and a member in another `cwd`), and the non-goals. Nothing in
issue #4 touched `cost.rs`, `Turn`, `agent_ledger` or `team.rs`; the
`harness_facts` corrections (`0023db0`) moved the `/context` thresholds
(the 20 000-token output reserve comes off every window: 167 000 at 200 k,
not 187 000) and the `/usage` tier fall-through, which the context and
limits metrics read — the spend PRD's §3 measurements should be taken on
0.7.0, not on older numbers.

Smaller candidates, none asked for: `cctop pane status` reading
`pluginPanes.dockColumns` beside the marker's `bodyColumns`; a fake-engine
render of `plain` Buttons, `hover`, `scope` once live checks 28–30 have
shown their behaviour; the coach card's 52-cell cut when `silent` and
`▸ steer window` meet (`tasks/handoff-after-console.md` § "Known limits").
US-107 stays deferred.

The coach's dogfood week (`tasks/handoff-coach-round-2.md`) continues;
`cctop coach-stats --since 4w --replay ~/.claude/projects` around
2026-09-29 is the review.

## Working practices that held

Everything in `tasks/handoff-after-v0.6.0.md` § "Working practices", plus:
run `make check-contract` **before** the version bumps, not after — it is
the step that tells whether the release is a contract release (regenerate
the d.ts, `TESTED_WITH`, a versions-table row) or a plain one; a release is
one commit on the feature branch so the PR's CI covers it, and the tag goes
on `main`'s tip after the rebase-merge; poll CI in the background
(`gh run watch` with `run_in_background`, or one `gh pr checks --watch`);
`claude -p … --max-turns 1` and `scripts/check-contract.sh` are fine in a
tool call, `cctop run` and `claude` interactive are not; regenerate, never
hand-edit; never write a `Result:` line; read-only rule and validator
constraints as in `CLAUDE.md`.

## Hand-off prompt

```
Continue cctop after the v0.7.0 release per @tasks/handoff-after-v0.7.0.md
(read it, then docs/verification/pane.md §A and items 25–32, and
tasks/handoff-after-team-costs.md § "Then"). PR #9 is merged, v0.7.0
tagged (plugin 0.8.0, contract 2.1.274), this machine on cctop 0.7.0 and
plugin 0.8.0. First walk me through live checks 25–32: run what the
session can (pane status with its compatibility line, the marker, the
debug-log greps including the self-check line, cctop query dashboard,
cctop run --once --size <bodyColumns>x24 beside the pane's rows), say
what to look at for the rest, and take my Result: lines down verbatim;
then gh issue close 3 when item 25 passes. Then write
tasks/prd-cctop-spend-attribution.md and its plan from the hand-off's
candidate list, stop for my decisions on the box, and implement in story
order on a branch from main. Read-only rule and validator constraints as
in CLAUDE.md; regenerate, never hand-edit; never write a Result: line;
poll CI in the background.
```
