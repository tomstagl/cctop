# Hand-off: after v0.6.0 (Console shipped) — the live checks, then issue #4 or the spend-attribution PRD

Written 2026-09-16, `main` at `d184a2e`, CI green, nothing unpushed.
**v0.6.0 is released** (plugin 0.7.0, dashboard schema 2) on top of
**v0.5.0**, released the same evening. This machine: Claude Code 2.1.273,
`cctop 0.6.0` from the tap, plugin 0.7.0 installed but **not yet loaded** —
a restart of Claude Code applies it. Read this, then
`tasks/handoff-after-console.md` (the per-step table, the known limits, the
two defaults), `docs/verification/pane.md` §A and items 25–32, and
`tasks/handoff-after-team-costs.md` § "Then" and § "Issue #4" for the two
pieces of work that follow.

## Where things stand

| What | Where |
|---|---|
| v0.5.0 | tag on `c9abc17` (`release: v0.5.0 — teammate spend`); release run `35151587592`: four targets, the GitHub release with `SHA256SUMS`, the tap formula, `cargo publish --dry-run` — all green. The person's own tag push never reached origin; the session created and pushed the tag |
| PR #8 | `console` → `main`, 20 commits, rebase-merged 2026-09-16 21:15 UTC. CI green on the **first** run — 355 Rust on macOS and Ubuntu, 118 pane, clippy clean on CI's newer toolchain. Nothing to fix; the console hand-off's warning about CI's clippy did not bite |
| The release commit | `0703bc1` on the branch → `670740b` on `main`: `Cargo.toml` 0.6.0 and the lock, plugin manifest 0.7.0, the install lines in both READMEs, `make site` (only the version line moved — the build outputs were fresh), `make check-types` green, the §A headless load check recorded (exit 0, load line, `/cctop-pane listed`, nothing refused, marker 12 ms apart) |
| v0.6.0 | tag on `670740b` (`v0.6.0 — Console, dashboard schema 2`); release run `35151911480`, all seven jobs green; the tap formula reads `version "0.6.0"` |
| `d184a2e` | `tasks/plan-dashboard-v2.md` §7 carries the nine-row "what the implementation found wrong" table beside its own list; the PRD's status line reads shipped. Neither the decisions box nor the two defaults (`0` *and* Esc home; six cells) was reopened; US-107 stays deferred |

- **Tests:** `cargo test` 355 passed, 2 ignored; `npm test` 118; the
  validator and `make check-types` green (the d.ts and `TESTED_WITH` at
  2.1.273; `READ_FROM` 2.1.270 — the known warning, issue #4's item 5).
- **Part C's evidence**, since the person asked for it in the Part A
  prompt's shape: `cargo test` 355 / `npm test` 118 both at B2 (`0148e87`,
  measured in a scratch worktree) and after B4 — Part C added no test, the
  `?` toggle is asserted inside `headless_render_shows_the_dashboard_and_footer`
  (`src/app.rs`). US-106: the rule line is `Body::keys` + `GLOBAL_KEYS`, the
  three footer strings are two (`coach_view.rs::FOOTER`, `app.rs::draw_footer`),
  `?` expands and collapses, the help overlay's `BINDINGS` lists the Console
  keys — "both appear in the footer" cannot be ticked as written because the
  first criterion removes the footer. US-108: no residue of tiles or the
  nine-row ledger in any named spot; all ten demo assets and `demo.tape`
  moved in `f09a921`; no separate "guide page for the meters" — the guide's
  index opener and header entry describe the cells and bodies, and the plan's
  B4 struck the `coach_*` regeneration.
- **Branches that can go** (the person deletes): `console` (local and
  origin, merged), `origin/claude/hopeful-johnson-9h0lo8`, `origin/agent-costs`,
  `origin/team-costs`.
- The plugin here went **0.4.1 → 0.7.0** directly (0.5.0 and 0.6.0 were never
  installed on this machine). The session that ran the release therefore had
  the 0.4.1 module against the 0.6.0 binary: `cctop pane status` all ✓,
  `cctop query advice` answered `schema: 2`, the old views kept working. Item
  25's "plugin 0.4.1" setup reads as "0.4.1 or newer".
- The auto-mode classifier denied `git push origin v0.5.0` once ("Create
  Public Surface"); the retry went through, and `gh pr ready` / `gh pr merge
  --rebase` were never denied. If a tag push is refused again, hand the person
  the two `git tag -a … && git push origin <tag>` lines rather than working
  around it.
- `version: null` in the marker after a `-p` one-turn load check is the
  manifest read (`readVersion`, `pane.tsx`) resolving after the single marker
  write; every recorded §A run has it. Not a defect.

**A measurement for item 31**, taken on the session that did the release
(plugin 0.4.1's module, Claude Code 2.1.273, this terminal): `cctop pane
status` read the session's tty at **142 columns** (TIOCGWINSZ on
`/dev/ttysNNN`), and the marker held **`bodyColumns: 71`, `viewportColumns:
70`**. PRD §3.6's ladder — `min(⌊columns × 0.45⌋, 90, columns − 70)` = 63,
then `BORDER_COLUMNS` and the grip — predicts a body near 58 at that width.
The `columns − 70` term alone (72) less one fits 71; the `0.45` term does not
bind here. It is one reading in one terminal, minutes apart from the
heartbeat that wrote it, not a resize test. Item 31 reads `bodyColumns` at
110 / 132 / 162 / 200 and settles it; if the reading holds,
`docs/claude-code-panels.md` §4 and the PRD's §3.6 table are the two places
to correct. The code needs nothing: the width ladder (three cells at 80, the
middle cell form at 28, the act line's tail at 66) is pinned to *body*
columns, not to the terminal.

**Settled 2026-09-16 (the session after this hand-off):** `~/.claude.json`
holds `pluginPanes: { dockColumns: 137 }` — a persisted `ctrl+x` resize
(item 10). `docs/claude-code-panels.md` §4 already has the rule: with that
key set the dock is `min(columns − 70, max(24, dockColumns))`, and the
ladder never runs. 142 → `min(72, 137) = 72`, body 71, viewport 70; this
machine's 221-column markers read body 136 / viewport 84 (`137 − 1`,
`221 − 137`) and, from an earlier render at `dockColumns` 81, body 80 /
viewport 140. Two readings say **`bodyColumns = dock − 1`** (the grip
only; `BORDER_COLUMNS` does not come out of a docked body) and
`viewportColumns = columns − dock`. Whether the default ladder subtracts
the same one column is still item 31's to read — under the override the
four widths predict 39 / 61 / 91 / 129 (the `columns − 70` term binds
until 207); with the key removed, `p3 − 1` predicts 39 / 58 / 71 / 89,
four more than the PRD §3.6 table's `~35 / ~54 / ~67 / ~85`. If that
holds, the PRD table and item 31's expected line move by four and the
width ladder's brackets stay (58 sits at the middle-cell form's 28-per-cell
edge — the one place to look). Item 31's Setup now carries the key and
names the marker as the only source; `cctop pane status` does not read
`pluginPanes` (a candidate addition, not asked for).

## First: live checks 25–32 (the person's; the session assists)

The order that costs the fewest sessions: restart Claude Code so 0.7.0
loads, then a second terminal at **162 columns** with
`CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --debug` in any live repo. Item
25 first (`say ok` with the pane closed, `!cctop pane status`, `/cctop`),
then 28 → 29 → 30 → 32 on the same docked pane, then 31 (resize to 110, 132,
162, 200 and `/cctop` at each — the 142-column reading above is the one
to explain). Items 26 and 27 need a session that launched agents / spawned
teammates; do them last or leave them pending.

What the session can run for each, beside the person's eyes: `cctop pane
status`; `ls -t ~/.cctop/pane | head -1` for the id, then the marker
(`bodyColumns`, `open`, `visibility`) and the debug-log greps of §A —
item 31's parenthetical names a log line `cctop: rendered … columns` that
`pane.tsx` never writes; the marker is the only source of `bodyColumns`,
and that sentence of item 31 wants correcting when the item is run; `cctop query dashboard
--session <id> | jq '.schema, [.cells[].label]'`; `cctop run --once --session
<id> --size <bodyColumns>x24` to lay the TUI's rows beside the pane's (item
28 and 31 say they are the same characters, the footer aside). Say what to
look at for the rest, and **take the person's `Result:` line down verbatim
— never write one.** `docs/verification/pane.md` has carried pending
`Result:` lines since 2026-09-12; leaving them pending is a state, not a
failure.

## Then: issue #4, or the spend-attribution PRD — the person's pick

Both are written up already and neither has moved since:

- **Issue #4** (the function-hooks contract moves and nothing in CI
  notices): the six items of `tasks/handoff-after-v0.3.1.md` § "Then: issue
  #4", restated in `tasks/handoff-after-team-costs.md` § "Issue #4 stays in
  its own lane". Item 5 (the `harness_facts` re-verification script and the
  `READ_FROM` bump to 2.1.273) is the one `make check-types` warns about on
  every run, and now also covers `docs/teams.md`. Console added nothing to
  the contract's surface (`plain` Buttons, `hotkey`, `scope`, `hover`,
  `dimColor` were all in the 2.1.273 d.ts), but live checks 28–30 are the
  first time those props are exercised for real — a fake-engine item that
  draws them (item 1) is where their behaviour gets pinned once the person
  has seen it.
- **The spend-attribution PRD** (`tasks/handoff-after-team-costs.md` §
  "Then"): the seven candidate items with the code each rests on, the two
  things to find out first (the lead's `/clear` and a member in another
  `cwd`), and the non-goals. Nothing in Console touched `cost.rs`'s rates,
  `Turn`, `agent_ledger` or `team.rs` beyond A1's per-process ledger — which
  item 5 there (folding teammates' ledgers into the baseline) now has to
  read through `cost::Ledger`.

Smaller follow-ups from Console, none urgent, all recorded in
`tasks/handoff-after-console.md` § "Known limits, deliberate": the coach
card's 52-cell cut of the spelled run when `silent` and `▸ steer window`
meet; a coach fire's `at` in the events body being the evaluate-once time
on `cctop query`; the pane's cells carrying no threshold colour (an engine
limit — a Button has `dimColor` only); the inline strip's one engine-side
line beyond PRD §4.3. US-107 (`Ctrl-E`, `z`, `/`, `s`, `-`) stays deferred
until something in the dogfood asks for it.

The coach's dogfood week (`tasks/handoff-coach-round-2.md`, running since
v0.3.1 on 2026-09-15) continues underneath all of this; `cctop coach-stats
--since 4w` towards the end of September is the review.

## Working practices that held

Everything in `tasks/handoff-after-console.md` § "Working practices that
held", plus what this day added: **poll CI and the release workflow in the
background** (`run_in_background`, or one `gh run watch`), not in a
foreground `sleep` loop — the coach's A10 fired on this session for a 3:52
foreground wait, and the coach was right; `gh pr create --draft`, `gh pr
ready`, `gh pr merge --rebase` all run from the session; a tag push may be
refused once by the classifier, and the person is the fallback, not a
workaround; a release is one commit on the feature branch so the PR's CI
covers it, and the tag goes on `main`'s tip after the rebase-merge (the
sha changes — `git fetch` first); `brew upgrade cctop && claude plugin
marketplace update cctop && claude plugin update cctop@cctop` afterwards,
and remember the plugin needs a restart that this session cannot survive.
Never run `cctop run` or `claude` in the foreground of a tool call (`cctop
run --once …` and `claude -p … --max-turns 1` are fine); read-only rule
and validator constraints as in `CLAUDE.md`; regenerate, never hand-edit;
never write a `Result:` line.

## Hand-off prompt

```
Continue cctop after the v0.6.0 release per @tasks/handoff-after-v0.6.0.md
(read it, then tasks/handoff-after-console.md, docs/verification/pane.md §A
and items 25–32, and tasks/handoff-after-team-costs.md § "Then" and
§ "Issue #4"). main is at d184a2e, CI green, v0.6.0 released (plugin 0.7.0,
dashboard schema 2), this machine on cctop 0.6.0 and plugin 0.7.0, Claude
Code 2.1.273. First walk me through live checks 25–32: run what the session
can (pane status, the marker and debug-log greps, cctop query dashboard,
cctop run --once --size <bodyColumns>x24 beside the pane's rows), say what
to look at for the rest, and take my Result: lines down verbatim; item 31
carries the 142-column measurement to settle. Then <issue #4 | the
spend-attribution PRD> — my pick. Do not reopen the dashboard-v2 PRD's
decisions box or the two defaults (0 and Esc home, six cells); US-107 stays
deferred. Read-only rule and validator constraints as in CLAUDE.md;
regenerate, never hand-edit; never write a Result: line; poll CI in the
background.
```
