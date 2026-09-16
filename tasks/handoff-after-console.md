# Hand-off: after Console (dashboard v2) — push and PR, the schema-2 release, the live checks

Written 2026-09-16, branch `console` at `f09a921`, **18 commits over `main`**
(`c9abc17`, the v0.5.0 release commit — itself **not pushed and not
tagged**), CI not yet run on any of it, nothing released since v0.4.0.
Read this, then `tasks/prd-cctop-dashboard-v2.md` v2.0 §3.6, §5, §6, §7 and
`tasks/plan-dashboard-v2.md` (the contract this branch implements; its §7
list of wrong claims is extended below), and `docs/verification/pane.md`
items 25–32.

## Where things stand

- **The Console redesign is implemented, Parts A, B and C**, one commit per
  plan step except B1+B2 (one commit, see below). On `console`:

  | Step | Commit | What it holds |
  |---|---|---|
  | — | `d40646e`…`3c1e515` | The ten docs-only commits from `origin/claude/hopeful-johnson-9h0lo8` (the PRD, the plan, `tasks/design-dashboard-v2/`, `tasks/handoff-console.md`), cherry-picked; that branch forked from `210ae47` and can go |
  | A1 | `8b486b2` | `cost::Ledger` keyed by the new `CostState.start_time`, summed; `LEDGER_COVERS = 1.10` estimate floor; `baseline.rs` reads the ledger; `harness_facts::cost_state::PER_PROCESS`; two tests; registry `cost` / `cost_combined` rows |
  | A2 | `4cf8c4d` | A response after `turn_duration` reopens the turn (`Turn::reopened`, `Aggregate::close_open_turn`); fixture E (`fixtures/session-e.jsonl`, 66 lines of the screenshot session, `anonymise-transcript.py --lines`); panel 4 `phase … · turn elapsed …` and `no call running`; row 4 detail `turn elapsed` |
  | A3 | `97227de` | `ContextView.velocity: Option` (`—/turn`); `API-equivalent` (row 2 beside the figure, panel 2 at the line's end); `re-reads N ⚠`, `8 stale files`; the rework light's `number` is `N open` / `N unchecked` / `ok` / `—`; the state line's `4c +145` is `4 calls` in place and `+145 ctx` last; `api_calls` / `tool_calls` pinned as distinct counters |
  | A4 | `c8096c7` | `src/series.rs` (OKLab port pinned to `series-ramp.mjs`); `Theme::series(n)` from `source_bg` / `source_accent` + `caps`; `stacked_bar` alternates on drawn segments; panel 1's bar by agency in `series(3)`; the rendered-buffer test; the encoding note and the guide sentence |
  | B0 | `8e1f277` | `scripts/build-site.sh` exits non-zero on zero matches (`must_sub` / `must_search`) |
  | B1+B2 | `0148e87` | Schema 2: `cells[6]`, `act`, `bodies[8]`, `Slice`, `Tone` s0/s1/s2; `src/ui/dashboard.rs` rewritten as Console; `State.console_body`; keys `1`–`6`, `a`, `0`, Esc, Enter (the body's panel), ask on `A`; the pane's `overview.tsx` rewritten (keyed Boxes of plain Buttons, `Model.body`, FR-16 schema line); the harness draws plain Buttons as the engine does; the row-identity test at 54 / 67 / 85; `docs/query.md`; live checks 28–31 |
  | B3 | `e7d82f3` | The rule line `0: home  ·  ? keys`, `?` expands to `Body::keys` + `GLOBAL_KEYS`; no dashboard footer; the pane's `? keys` Button and `PANE_KEYS`; live check 32 |
  | B4 | `f09a921` | README mockup from the renderer at 85 × 24, the bodies table, the site template §2, the plugin README, the guide, Console regexes in `build-site.sh`, the demo assets regenerated locally (context body), `demo.py` finds Chrome / ffmpeg in Playwright's cache, tape keys |

- **Tests:** `cargo test` 355 passed, 2 ignored (345 on `main`); `npm test`
  118 (122 on `main`: the tile and block-digit tests are gone, the Console
  and row-identity tests are in); `claude plugin validate --strict ./plugin`
  and `make check-types` green (the d.ts is still 2.1.273; `READ_FROM`
  still 2.1.270).
- **Two decisions the author owns were taken as the PRD §6.2 defaults**
  because the person said to continue: `0` *and* Esc as the way home; six
  cells. `BODY_KEYS` / `BODY_IDS` / `BODY_PANELS` in `src/dashboard.rs`
  change either.
- **Two deviations from the PRD's shape**, both for the reason the plan gave
  for width — `snapshot(state, engine)` has no caller to tell it what a
  surface has open, and the TUI and the pane can hold different bodies: the
  object carries all eight `bodies`, and `Cell.active` is not on the wire.
- **The engine draws a plain Button as `1: label`** (colon, space) — the
  prototype's `1:ctx` was wrong about the engine — so the TUI draws `1: ctx`
  and the pane's rows equal it; `tests/pane/render.ts` now draws `plain`
  Buttons that way (the view bar tests lost their `[ ]`).
- **Known limits, deliberate:** the coach card is 52 cells (`coach::WIDTH`),
  so on the *coach card* the spelled run cuts its last token with `…` when
  `silent` and `▸ steer window` are both present; on `cctop query`, a coach
  fire's `at` in the events body is the evaluate-once time (the TUI logs it
  when it fired); the pane's cells carry no threshold colour (a Button has
  `dimColor` only) — the digit is the engine's accent, the open cell green
  Text; the pane's inline strip keeps one engine-side line (`context 396k /
  1.0M (40 %) · 5h 42 %, resets in …`) beyond PRD §4.3's "act line and the
  strip", because it is the pane's own reading with no binary behind it.
- **Live checks pending, all the person's:** 25–27 (from before) and
  28–32 (Console: plain Buttons without a hotkey, the hover scope over a
  spaces-only Button, six bare-digit hotkeys, the real `bodyColumns` at
  35 / 54 / 67 / 85, `? keys`). Never write a `Result:` line.
- The PRD's status line still reads "proposed, nothing implemented"; this
  file records the shipping, as the repo does.

## First: push, PR, CI

Nothing on this machine has left it. In order: `git push origin main`
(the v0.5.0 commit `c9abc17`), wait for CI, tag `v0.5.0 — teammate spend`
and push the tag (the recipe in `tasks/handoff-after-team-costs.md` §
"First"); then `git push -u origin console`, `gh pr create --draft`, `gh
run list --branch console`. CI's clippy is newer than the local one
(`tasks/handoff-after-v0.4.0.md` records a fix for exactly that) and the
pane job runs `npm test` on Node 22; Linux's inotify reports `Access`
events (`tail.rs`) — nothing here touched the watchers. Rebase-merge as
before and cite shas as "on the branch" until they land.

## Then: the schema-2 release — v0.6.0 and plugin 0.7.0 together

The one release where `Cargo.toml`, `plugin.json` and the tap must not
drift (plan §5): the pane's FR-16 line tells an older binary "cctop 0.6.0
or newer needed", `docs/query.md` says schema 1 ended before 0.6.0, and
`docs/verification/pane.md` items 28–32 name plugin 0.7.0. So: `Cargo.toml`
0.6.0 and `cargo build` for the lock, `plugin/.claude-plugin/plugin.json`
0.7.0, the install lines in both READMEs, `docs/mcp.md` (unchanged: no
dashboard MCP tool), `make site`, the §A headless load check recorded, then
the tag after the merge. Then `brew upgrade cctop && claude plugin update
cctop@cctop` and the live checks 25–32, the person's `Result:` lines taken
down verbatim.

## Then: what the plan and the PRD got wrong about the code

Add these to `tasks/plan-dashboard-v2.md` §7 when the branch lands, beside
its own list:

| Claim | Verdict |
|---|---|
| "a near-zero `total_cost_usd` on a session with real usage" (§3.2, A1) | **Not on this machine** (130 ledgers). The mechanism is *two processes appending to one transcript*, each writing its own running total (`ab339470`: $29.04 then $7.15 over ≈$36); a do-nothing process writes `$0` — the screenshot. `startTime` names the process |
| "A bug exists only if … then it is in `src/phase.rs`" (A2) | **Wrong file.** `Turn.duration_ms` stayed `Some(205)` after Claude Code re-drove the prompt following `/login`; fixed in `usage.rs` |
| "`4c +145` spelled out" fits the state line (A3) | **The coach card is 52 cells** and the plan did not carry it; spelled in place it cut `▸ steer window` to `▸ ste…` |
| "The context bar paints its five slices with accent / ok / warn / crit / dim" (§3.3, A4) | **Never in code**: panel 1 drew every slice in `fg` since `38d317c`; measured from the *Before* mockup |
| Baseline "292 passed" | `78f8a2f`'s count; `main` had 345 |
| B1 green on its own, B2 after | **B1 cannot build alone** — the TUI reads the object |
| `Cell.active`, `body: Body` singular (US-102) | The object cannot know what a surface has open; all eight bodies travel |
| The prototype's `1:ctx` | The engine draws `1: ctx` |
| `viewOfDigit` and the ledger's unfold (§3.5) | Gone with the ledger; the pane's other views stay on the view bar, as the non-goals say |

## Working practices that held

Edits by small substitutions; `make check`, `npm run typecheck && npm test`,
`make check-types` and `claude plugin validate --strict ./plugin` before
every commit; one commit per plan step with the trailer `Co-Authored-By:
Claude Opus 5 <noreply@anthropic.com>`; commit subjects are prose
(`Metrics: …`, `Coach: …`, `Theme: …`, `Dashboard: …`, `Site: …`, `Docs: …`).
Regenerate, never hand-edit: `cctop metrics --md > docs/metrics.md && cctop
metrics --readme README.md` in the same commit as any registry text;
`INSTA_UPDATE=always cargo test` then strip `assertion_line:` and delete
`*.snap.new`; `scripts/pane-fixtures.sh` for A, `… fixtures/session-b.jsonl
b` for B and the coach moments, `… -c c`, `… -d d`, in the same commit as
any change to `coach.rs` or `dashboard.rs`; `make site` after the README or
the template; `python3 scripts/demo.py` (release binary) for the assets;
`python3 scripts/anonymise-transcript.py <src> <dst> --max-str 2000
[--lines N]` for a fixture. Fixtures A–E render character-identical unless a
story says otherwise; the row-identity test (`tests/pane/overview.test.ts`,
fixture B at 54 / 67 / 85 against `src/snapshots/…fixture_b_dashboard_*.snap`)
is the proof the two surfaces agree — when the object changes, re-accept
the Rust snapshots *before* regenerating the pane fixtures, or it compares
new rows with old ones. Never run `cctop run` or `claude` in the foreground
of a tool call (`cctop run --once … --keys 1,Enter` and `claude -p … --max-turns
1` are fine). The validator follows `$` only into top-level functions of
`pane.tsx`; the pane has four colours and no hex; never write a `Result:`
line.

## Hand-off prompt

```
Continue cctop after the Console redesign per @tasks/handoff-after-console.md
(read it, then tasks/prd-cctop-dashboard-v2.md §3.6, §5, §6, §7,
tasks/plan-dashboard-v2.md, and docs/verification/pane.md items 25–32).
Branch console is at f09a921, 18 commits over main (c9abc17, the unpushed,
untagged v0.5.0 release commit); Parts A, B and C are implemented and green
(cargo test 355, npm test 118); nothing is pushed and there is no PR.
First push main and stop for me to tag v0.5.0; then push console, open a
draft PR, watch gh run list --branch console and fix what CI finds without
touching the layout. Then prepare the schema-2 release commit — Cargo.toml
0.6.0 and the lock, plugin manifest 0.7.0 (they move together), the install
lines, make site, check-types green, the headless load check recorded in
docs/verification/pane.md §A — and stop for me to tag; then walk me through
live checks 25–32 and take my Result: lines down verbatim. Then add the
hand-off's "what the plan got wrong" table to tasks/plan-dashboard-v2.md §7
and mark the PRD's status line shipped. Do not reopen the PRD's decisions
box or the two defaults the hand-off records (0 and Esc home, six cells);
US-107 stays deferred. Read-only rule and validator constraints as in
CLAUDE.md; regenerate, never hand-edit; never write a Result: line.
```
