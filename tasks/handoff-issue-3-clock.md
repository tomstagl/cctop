# Hand-off: issue #3 — the pane on Claude Code ≥ 2.1.271 (`$.clock.now()` is a Promise)

Written 2026-09-15, `main` at `d04e3ec`, CI green. Issue:
https://github.com/tomstagl/cctop/issues/3 (read it first — it has the root
cause, the bundle diff and every call site; this note adds what the repo
knows that the issue does not).

## Where things stand

- cctop **0.3.0** is released (tag `v0.3.0`, the tap at 0.3.0); plugin
  manifest **0.4.0**. On this machine brew's `cctop` 0.3.0 is on PATH,
  `cctop install --yes` is done, plugin 0.4.0 is installed at `5a28f2e`, and
  `coach = "auto"` is in `~/.config/cctop/config.toml`. The coach round-2
  record is `tasks/plan-cctop-coach.md` §1 (Progress) and
  `tasks/handoff-coach-round-2.md` (status block at the top).
- **Claude Code on this machine is 2.1.272** (`~/.local/share/claude/versions/`
  has 2.1.269–2.1.272). The pane is therefore broken here right now, in every
  session, whether or not it is opened: the first marker write throws
  (`marker write failed: RangeError: Invalid Date`), `~/.cctop/pane/<session>.json`
  is never written, `cctop pane status` says the hooks module is not loaded,
  and `/cctop` falls through to `cctop split`. The TUI (`cctop run`) is
  unaffected — dogfood of the coach continues in a terminal split; the pane
  half of the dogfood (TUI ↔ pane slot agreement, `$.ui.status`, `[1 fill]`)
  is blocked on this issue.
- The checked-in contract is stale: `plugin/.claude/types/claude-code.d.ts`
  line 1 says `// Written by Claude Code 2.1.270.`; `make check-types`
  (`scripts/check-plugin-types.sh`) fails on it against 2.1.272, which is the
  guard that should have caught this. `TESTED_WITH = '2.1.270'`
  (`plugin/hooks/model.ts:18`) is what the header badge shows.

## The bug in one paragraph

2.1.271 turned `$.clock.now()` from a synchronous number into a host event
that resolves a Promise (`clock.now`; `clock.sleep`/`after`/`every` became
host events too, `every` at least every 1 ms; `$.session.usage` gained an
optional `args` — compatible). The plugin does arithmetic and
`new Date(...)` on the return value, so every timestamp is a Promise:
`writeMarker` throws `Invalid Date`, and `requestRender`'s debounce computes
`NaN`, calls `$.clock.after(NaN, …)`, and the engine's validator throws —
under every `apply()`/`dispatch()`, so every hook's `.catch` logs a failure
while `cctop query …` itself runs fine and the model never updates.

## Call sites (from the issue; verified against `main`)

```
plugin/hooks/pane.tsx    90  invalidateNow: invalidatedAt = $.clock.now()
                        135  at: $.clock.now()
                        174  requestRender: waited = $.clock.now() - invalidatedAt   ← the hot one
                        276  pollerEngine: clock: { now: () => $.clock.now(), every }
                        372  usage read: at
                        421  openedAt
                        481  const now = $.clock.now()
                        582  session.start at · 607 turn.start at · 618 turn.complete at
                        642  tool.call startedAt · 653 tool.call at
plugin/hooks/poller.ts   86  writeMarker: heartbeatAt: new Date($.clock.now()).toISOString()
                        112  updateStale
                        167  sync (rotation): startedAt
                        194  round: tick at
                        203  start: startedAt
plugin/hooks/poller.ts   16  type PollerEngine.clock: Pick<…, 'now' | 'every'>  (type follows the d.ts)
tests/pane/harness.ts   319  fakeEngine clock.now: () => now                     (make it resolve a Promise)
tests/pane/poller.test.ts 76, 80  read $.clock.now() synchronously
```

## Suggested shape of the fix (decide the details yourself)

1. **Regenerate the contract first**, so the compiler finds every site:
   `cd plugin && CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p '/plugin-types ./.claude/types'`
   (writes `claude-code.d.ts` for 2.1.272 — ~2 000 lines of diff, the clock
   is the only breaking change the issue found in what the pane uses — and a
   machine-specific `claude-code-mcp.d.ts`, already gitignored). Then
   `npm run typecheck` is the worklist: `number − Promise<number>` is an error.
2. **`await $.clock.now()` everywhere.** A plugin that awaits runs on 2.1.270
   too (awaiting a number is fine), so one build covers both; only the type
   of `PollerEngine.clock.now` needs the Promise form. `writeMarker`,
   `updateStale`, `round`, `sync`, `start` are already async or trivially
   become so.
3. **`requestRender` (`pane.tsx:171`)** is synchronous and runs under every
   event handler and `apply()`. Either make it async (callers
   `void requestRender($)`) with a synchronous `renderPending` flag set before
   the first `await`, so two events in one tick cannot both arm a timer — the
   lifecycle test *"20 model changes in 1 s produce at most 4 invalidates plus
   one trailing"* (`tests/pane/lifecycle.test.ts:71`) is the guard — or stop
   the debounce from depending on the host clock at all (`Date` exists in the
   plugin environment: 2.1.270 implemented `now` as `Date.now()` there, and
   `new Date(ms).toISOString()` already runs in `poller.ts`). If you use
   `Date.now()`, the fake engine's manual clock no longer drives the debounce
   in tests; weigh that before choosing.
4. **Validator constraints still hold** (`pane.tsx:69`, `:272`): `$` may only
   be used inside top-level functions declared in the file that received it —
   never handed to a closure; the poller keeps working through the
   `PollerEngine` slice built in `pane.tsx`. Every hook returns `next(e)`
   unchanged; no `$.process.run` from `tool.call`, `turn.step` or `ui.render`
   (pane PRD FR-10, FR-15). Nothing toolchain-related under `plugin/`.
5. **Tests:** `fakeEngine.clock.now` returns `Promise.resolve(now)` so the
   harness exercises the 2.1.272 contract; adjust the two synchronous reads in
   `poller.test.ts`; keep the manual `clock.tick` for timers. Run
   `npm run typecheck && npm test`, `make check`, `make check-types`, and
   `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin validate --strict ./plugin`
   (CI runs it when `claude` is present).
6. **Versions and docs:** `TESTED_WITH = '2.1.272'`; plugin manifest
   `0.4.1` (the marketplace serves the repo, so `claude plugin marketplace
   update cctop && claude plugin update cctop@cctop` picks it up after the
   push — no cctop binary release is needed for a plugin-only fix);
   `plugin/README.md` (the `pane status` sample line names 2.1.270; add a
   line under "Verification status"); `docs/verification/pane.md` gains a
   live item: on 2.1.272, a session with the pane never opened logs no
   `cctop:` failure, `~/.cctop/pane/<session>.json` exists with `loaded: true`,
   `cctop pane status` shows the module ✓, and `/cctop` fills every light
   within one poll; `tasks/prd-cctop-pane.md` §11 (v1.2 amendment) gets a
   short v1.3 note on the clock contract and the version pin; note it in
   `tasks/plan-cctop-coach.md` §1 only as "the pane half of the dogfood
   resumed on <date>". `make check-types` will still *warn* that
   `src/harness_facts.rs` `READ_FROM = "2.1.270"` is older than 2.1.272 —
   that is a separate re-verification of the binary's constants, not part of
   this fix.
7. **Live check is the user's:** after the push and the plugin update, a
   Claude Code restart; the issue's repro is the acceptance test, plus the
   comment's case (a session where the pane is never opened must be silent
   and must still write the marker). Close #3 with the commit and the version.

## Working practices that held

Edits by small scripted substitutions; `npm run typecheck && npm test` in
the repo root (the TS toolchain lives there, `plugin/` ships without it) and
`make check` before each commit; commit per chunk with the trailers
`Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and
`Claude-Session: <session url>`; push and then `gh run list` — CI's clippy
(1.98) is newer than the local Homebrew rust 1.85, so a lint can be clean
locally and red in CI.

## Hand-off prompt

```
Fix cctop issue #3 per @tasks/handoff-issue-3-clock.md (read it, then the
issue with `gh issue view 3 --comments`). main is at d04e3ec, CI green,
Claude Code 2.1.272 on this machine. Regenerate the function-hooks contract
against 2.1.272 first, await $.clock.now() everywhere (requestRender is the
hot path; keep the burst test honest), make the fake engine's clock a
Promise, bump TESTED_WITH and the plugin manifest to 0.4.1, update the docs
listed in the hand-off, run npm run typecheck, npm test, make check and
make check-types before each commit, push, then tell me exactly what to do
for the live check and the plugin update.
```
