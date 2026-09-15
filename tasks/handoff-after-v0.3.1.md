# Hand-off: after v0.3.1 — the pane's live check, closing #3, then issue #4

Written 2026-09-15, `main` at `f2ae787` (the release commit), CI green,
`v0.3.1` released: four targets, `SHA256SUMS`, the tap formula at 0.3.1,
`cargo publish --dry-run` green. Read this, then `gh issue view 3 --comments`
and `gh issue view 4`.

## Where things stand

- **Issue #3 is fixed, not yet closed.** Claude Code 2.1.271 turned
  `$.clock.now()` into a host event resolving a Promise; plugin 0.4.0 failed
  every hook on 2.1.271/2.1.272. `7f3a85a` (plugin **0.4.1**) regenerates the
  contract against 2.1.272, awaits every reading (`requestRender` is async
  with a `renderPending` flag; the trailing timer invalidates at the gap's
  end by construction; `ui.render` reads the clock once per frame), makes
  the fake engine's clock a Promise, and sets `TESTED_WITH = '2.1.272'`.
  `bd49b92` is the docs (plugin README section "Claude Code 2.1.271 and the
  clock", pane PRD v1.3 note at the end of §11, `docs/verification/pane.md`
  §A run line and live **item 25**). `f2ae787` is the release: Cargo.toml
  0.3.1, the READMEs' install line, the site line. The binary is unchanged
  apart from its version. Verified headless on 2.1.272 (`claude -p
  --plugin-dir ./plugin --debug --max-turns 1 'say ok'`): module loaded, no
  `hook failed` / `refused` / `cctop: … failed` line, the marker written with
  `loaded: true` for a session whose pane was never opened. The live check
  is the acceptance test and is the person's (item 25, `Result: pending`).
- **This machine is not on the release yet.** brew's `cctop` is 0.3.0 (the
  tap has 0.3.1), the installed plugin is 0.4.0 (`5a28f2e`), Claude Code is
  2.1.272. The Claude Code sessions that were running on 2026-09-15 were
  started under 2.1.270 (their markers in `~/.cctop/pane/` say plugin 0.2.2
  and heartbeat happily) — only a fresh session runs 0.4.1 on 2.1.272.
- **Issue #4** (opened 2026-09-15) is the backwards-compatibility follow-up:
  CI has no `claude` on the runner, so `make check-types` and `claude plugin
  validate --strict` are skipped there; nothing in CI runs against a real
  Claude Code, which is how 2.1.271 went unnoticed for two releases.
- `make check-types` still *warns*: `src/harness_facts.rs` `READ_FROM =
  "2.1.270"` is older than 2.1.272 — the binary's constants (autocompact
  buffer, `/usage` weights, `/context` thresholds, the first-seen map) are
  unverified against 2.1.271/2.1.272. Separate from #3.
- The coach dogfood (`tasks/handoff-coach-round-2.md`, status block at the
  top): the TUI half runs (`coach = "auto"` in `~/.config/cctop/config.toml`);
  the pane half (TUI ↔ pane slot agreement, `$.ui.status`, `[1 fill]`; round-2
  item 5) resumes once item 25 passes. The `coach-stats` review is due around
  2026-09-29.
- A cosmetic race seen in the headless run: the first marker's `version` was
  `null` (`readVersion`'s `$.fs.read` of plugin.json and the first
  `writeMarker` run concurrently), so `cctop pane status` may print `cctop
  plugin ?` for a session whose pane was never opened; the next marker write
  (open, tick) carries it. Left alone; it falls under #4's item 3.

## First: get this machine onto the release

The session can run these; the restart is the person's.

```
brew update && brew upgrade cctop && cctop --version          # cctop 0.3.1
claude plugin marketplace update cctop && claude plugin update cctop@cctop
```

Then a fresh Claude Code (function hooks on, `/tui fullscreen`, ≥ 110
columns, `--debug` for the log). `/reload-plugins` in a running session
loads the new module too, but the acceptance test is a fresh session.

## The live check — `docs/verification/pane.md` item 25

With the pane closed: one short prompt (`say ok`). Expected, in order:

1. No `⏺ cctop: …` line in the transcript at start-up.
2. `!cctop pane status` → the hooks-module line ✓ (`(cctop plugin ?)` is the
   race above, not a failure).
3. `cat ~/.cctop/pane/$(ls -t ~/.cctop/pane | head -1)` → `loaded: true`,
   `open: false`, ISO timestamps (no `Invalid Date`).
4. `/cctop` → the pane docks; the header reads `hooks 2.1.272`; every light
   is filled within one poll (≤ 10 s idle, 2 s while a turn runs); nothing
   says `waiting for cctop`.
5. `grep -E 'hook failed|Invalid Date|non-negative' ~/.claude/debug/latest`
   → nothing.

The session can do 2, 3 and 5 itself (`cctop pane status --session <id>`,
the marker, the debug log); 1 and 4 are what the person sees. Record the
person's `Result:` line for item 25 (never write one for them), then:

```
gh issue close 3 --comment "Fixed in 7f3a85a (plugin 0.4.1, released with cctop v0.3.1): every \$.clock.now() awaited, contract regenerated against 2.1.272. Live check on 2.1.272 passed (docs/verification/pane.md item 25)."
```

While the pane is open on 2.1.272, items 11a and 21–24 are cheap to run in
the same session; and round-2 item 5 (the TUI in a split beside the pane:
both name the same nudge; `cctop query coach --line` equals the status line)
is the pane half of the dogfood.

## Then: issue #4, in this order

1. **The fake engine follows the contract** (`tests/pane/harness.ts`). It is
   built with `as unknown as FakeEngine`, so its synchronous `now` compiled
   against the Promise contract. Build the engine object with `satisfies`
   (or type the literal) so a regenerated d.ts that changes a method's type
   fails `npm run typecheck` in the tests too. Cheap; do it first. 2.1.272's
   d.ts also declares an engine-side harness (`mock.clock` / `mock.store` /
   `mock.env`, `describe`, `expect`, `tier`) — read that section before
   deciding whether to keep the hand-written fake.
2. **`cctop pane status` knows the broken pair** (`src/pane.rs`, beside
   `MIN_PLUGIN_VERSION`): a small known-incompatibility table — plugin
   < 0.4.1 on Claude Code ≥ 2.1.271 is ✗ with `claude plugin update
   cctop@cctop` after `→` — and a warning when Claude Code is newer than the
   plugin's `TESTED_WITH` (read it from the installed plugin's `model.ts`, or
   put it in the marker). Binary-side: ships with the next release.
3. **One clear line instead of a failure per hook** (`pane.tsx`): a
   `session.start` self-check of the `$` surfaces the module depends on
   (`typeof await $.clock.now() === 'number'`, `$.ui.resolve`, `$.fs.write`)
   that logs one line naming plugin version, `TESTED_WITH` and the installed
   version, and writes the marker with `loaded: true` before anything that
   can fail (today the first write needs `$.home()`, the clock and
   `$.fs.write` all working). This also settles the `version: null` race:
   read the manifest, then note the session.
4. **CI with `claude` on the runner**: `make check-types`, `/plugin-types`
   into a temp dir diffed against the checked-in d.ts, `claude plugin
   validate --strict ./plugin`, and the headless load check of
   `docs/verification/pane.md` §A with its log greps and the marker
   assertion. On PRs and on a schedule (Claude Code releases daily). Open
   question: the load check's `say ok` needs a model call — a CI credential,
   or find whether `session.start` fires without one (`--max-turns 0`?).
   Check whether `/plugin-types` itself needs auth before designing the job.
5. **`harness_facts.rs` re-verification as a script**: grep the installed
   bundle (`~/.local/share/claude/versions/<v>`) for the constants and print
   the diff, so the `READ_FROM` bump is mechanical; then bump it to 2.1.272
   and make `check-types` fail, not warn, past N releases.
6. **Release checklist** in `CLAUDE.md`: regenerate the d.ts against the
   latest `claude`, run the headless check, bump `TESTED_WITH`, note the
   version range in `plugin/README.md`, then tag.

Each item is one commit; #4 stays open until 4 lands (its acceptance: a
contract change turns CI red within a day, naming the surface).

## Working practices that held

Edits by small substitutions; `npm run typecheck && npm test`, `make check`
and `make check-types` before every commit (the user's rule); `claude plugin
validate --strict ./plugin` after touching `pane.tsx`; commit per chunk with
the trailers `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` and
`Claude-Session: <session url>`; push and `gh run list` (CI's clippy is
newer than the local one). Releases: bump `Cargo.toml`, `cargo check` for
the lock, the install line in both READMEs, `make site`, commit `release:
vX.Y.Z — …`, annotated tag `vX.Y.Z`, push both — the workflow builds, makes
the GitHub release with `SHA256SUMS`, and pushes the tap formula. In the
pane tests the clock is `await $.clock.now()` (the fake resolves at once);
a redraw asked for after a press or a change lands after the clock, so
`await settle()` before counting invalidates. Never write a `Result:` line
in `docs/verification/pane.md`; never run `cctop run` or `claude` in the
foreground of a tool call (`claude -p … --max-turns 1` is fine).

## Hand-off prompt

```
Continue cctop after the v0.3.1 release per @tasks/handoff-after-v0.3.1.md
(read it, then `gh issue view 3 --comments` and `gh issue view 4`). main is
at f2ae787, CI green, v0.3.1 released, Claude Code 2.1.272 on this machine.
First get this machine onto the release (brew upgrade cctop, claude plugin
update cctop@cctop — I restart Claude Code myself), then walk me through the
live check of docs/verification/pane.md item 25 in a fresh session: run the
parts you can (pane status, the marker, the debug log), tell me exactly what
to look at for the rest, take my Result: line down verbatim, and close #3
with the commit and the version. Then work issue #4 in the hand-off's order,
one commit per item; npm run typecheck, npm test, make check and make
check-types before each commit; push and check gh run list. Read-only rule
and validator constraints as in CLAUDE.md.
```
