# cctop pane — verification

The pane is the cctop dashboard drawn inside Claude Code's own TUI through the
function-hooks plugin API (early access, `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1`).
Its module lives in `plugin/hooks/pane.tsx`.

Two kinds of checks live here:

- **Automated** checks run in CI and on every story (`npm run typecheck`,
  `npm test`, `cargo test`). The headless load check below and everything
  else that needs the `claude` CLI (`claude plugin validate --strict
  ./plugin`, the contract surface against the installed Claude Code, the
  version pins) run in CI's `contract` job — on every push and daily, against
  the latest Claude Code release — through `scripts/check-contract.sh`
  (`make check-contract` here). The job writes §A's result line to its step
  summary; a person copies it into the `Last automated run:` line below when
  recording a run by hand.
- **Live** checks need a person at a real terminal (a docked pane at a given
  width, hotkeys, `/reload-plugins`, CPU, latency, `cctop split` in tmux or
  Apple Terminal). An agent never marks these as passed.

Every item ends in a `Result:` line. `Result: pending` means nobody has run it
yet. A **person** fills it in with `pass` or `fail`, the date and the Claude
Code version they ran it on. Automated tooling must not touch `Result:` lines.

## A. Headless load check (US-001)

Confirms that Claude Code admits the hooks module, that it registers its
command, and that the engine refused nothing the module asked for.

Setup: a checkout of this repository, `claude` 2.1.269 or newer on `PATH`, and
`plugin/.claude/types/claude-code.d.ts` written by the same version (its first
line names it).

Command (from the repository root):

```
CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p --plugin-dir ./plugin --debug --max-turns 1 'say ok'
echo "exit $?"
```

With `-p`, `--debug` writes to a log file, not to stderr. The newest log is
`~/.claude/debug/latest` (a symlink to `~/.claude/debug/<session-id>.txt`).
Grep it:

```
grep -c 'hooks module cctop loaded' ~/.claude/debug/latest     # expect 1
grep -c '/cctop-pane listed' ~/.claude/debug/latest            # expect 1
grep 'cctop: plugin ' ~/.claude/debug/latest                    # expect one line ending "self-check ok"
grep -i 'refused' ~/.claude/debug/latest                        # expect no output
grep 'hook failed' ~/.claude/debug/latest                       # expect no output
```

Expected:

- `claude -p` prints `ok` (or similar) and exits 0.
- The log holds one line `hooks module cctop loaded (... ); events: session.start,command.run,ui.render`.
- The log holds `$.command.register (cctop): /cctop-pane listed`.
- The log holds one `$.ui.log` line `cctop: plugin <version> (hooks contract
  <TESTED_WITH>) loaded; self-check ok` (plugin 0.8.0+): session.start's
  check of the clock, `HOME` and the session id. A `self-check failed: …`
  line names the surface that moved and is the one line to read when a
  Claude Code release breaks the contract (issue #4).
- No line contains `refused` or `hook failed`.
- `~/.cctop/pane/<session>.json` holds `loaded: true`, `open: false`, the
  plugin's `version` (read before the marker since 0.8.0; earlier modules
  wrote `null` here in a `-p` run), `testedWith` and `selfCheck: "ok"`.

The same check without a model call, and so without credentials — what CI
runs (`.github/workflows/ci.yml`, the `contract` job): the prompt is the
native `/plugin-types` command, which fires `session.start` like any run and
writes the contract beside it.

```
CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p --debug --plugin-dir ./plugin '/plugin-types /tmp/types'
```

Known benign line: `[WARN] plugin cctop: options requested but its manifest
declares no userConfig; every option reads as absent`. The plugin declares no
`userConfig`, so every option is absent; nothing is refused.

Why the command is `/cctop-pane` and not `/cctop`: on 2.1.269 the engine
reserves `/cctop` for the plugin's own skill (`/cctop:cctop`) and refuses
`$.command.register({ name: "cctop" })` with
`"/cctop" refused: it is the plugin's /cctop:cctop`. This resolves open
question 1 of `tasks/prd-cctop-pane.md`: the native command is `cctop-pane`,
and `/cctop` keeps resolving to the skill, which is the fallback path.

Last automated run: 2026-09-16 (the v0.6.0 release, Console — schema 2), Claude Code 2.1.273, the contract unchanged (`make check-types` matches it), the module released as plugin 0.7.0, by the agent: exit 0, load line present, `/cctop-pane listed`, no `refused` or `hook failed` line, no `cctop: … failed` line, marker written with `loaded: true`, `open: false`, `loadedAt` and `heartbeatAt` 12 ms apart. Earlier: 2026-09-16 (the v0.5.0 release), Claude Code 2.1.273, the contract unchanged (`make check-types` matches it), the module released as plugin 0.6.0, by the agent: exit 0, load line present, `/cctop-pane listed`, no `refused` or `hook failed` line, no `cctop: … failed` line, marker written with `loaded: true`, `open: false`, `loadedAt` and `heartbeatAt` 25 ms apart. Earlier: 2026-09-16, Claude Code 2.1.273 with the contract regenerated against it (a `vscode` render surface added, nothing the pane calls changed), the module of plugin 0.5.0, by the agent: exit 0, load line present, `/cctop-pane listed`, no `refused` or `hook failed` line, no `cctop: … failed` line, marker written with `loaded: true`, `open: false`, `loadedAt` and `heartbeatAt` 15 ms apart. Earlier: 2026-09-15, Claude Code 2.1.272, plugin 0.4.1 (issue #3), by the agent: exit 0, load line present, `/cctop-pane listed`, no `refused` or `hook failed` line, no `cctop: … failed` line, and `~/.cctop/pane/<session>.json` written with `loaded: true`, `open: false` and ISO timestamps (`loadedAt` and `heartbeatAt` 15 ms apart: two clock round trips) for a session whose pane was never opened — the case of issue #3's comment. Earlier: 2026-09-12, Claude Code 2.1.269, the same result.
Claude Code version:
Result: pending

## B. Live checks (US-010)

Every item below needs a person at a real terminal. Read `/cctop-pane` as the
native command (US-001's resolution of PRD open question 1) and `/cctop`/
`/cctop:cctop` as the plugin's skill, which is the fallback path when function
hooks are off or before its module is accepted.

1. **Slash-menu entry.** `/cctop-pane` appears in the slash-command
   typeahead with the description `Open the cctop dashboard pane` and the
   hint `[view|close]`.
   Setup: `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --plugin-dir ./plugin`.
   Keys: type `/cctop-` and read the menu.
   Expected: one entry, `/cctop-pane`, with that description and hint.
   Claude Code version:
   Result: pending

2. **Docks at 144 columns.** In `/tui fullscreen` at a 144-column terminal,
   `/cctop-pane` docks a pane beside the transcript whose body reads `cctop`.
   Setup: terminal resized to 144 columns; `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1
   claude --plugin-dir ./plugin`; `/tui fullscreen`.
   Keys: `/cctop-pane`.
   Expected: a docked pane appears to the right of the transcript (not
   inline above the prompt).
   Claude Code version:
   Result: pending

3. **Docks at 110 columns.** Same as item 2 but at exactly 110 columns (the
   d.ts's documented minimum dock width).
   Setup: terminal resized to 110 columns; same session setup as item 2.
   Keys: `/cctop-pane`.
   Expected: the pane still docks (does not fall back to inline) at 110
   columns.
   Claude Code version:
   Result: pending

4. **Inline in `/tui default`.** In the classic renderer the pane draws its
   short form inline above the prompt instead of docking.
   Setup: `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --plugin-dir ./plugin`;
   `/tui default`.
   Keys: `/cctop-pane`.
   Expected: the pane's content is drawn inline, above the prompt; no docked
   pane appears.
   Claude Code version:
   Result: pending

5. **Toggle.** `/cctop-pane` with no argument opens the pane, and running it
   again closes it.
   Setup: pane closed; `/tui fullscreen`.
   Keys: `/cctop-pane`, then `/cctop-pane` again.
   Expected: first run opens the pane; second run closes it.
   Claude Code version:
   Result: pending

6. **`close` argument.** `/cctop-pane close` closes an open pane.
   Setup: pane open (`/cctop-pane`).
   Keys: `/cctop-pane close`.
   Expected: the pane closes; running `/cctop-pane close` again while already
   closed is a harmless no-op.
   Claude Code version:
   Result: pending

7. **View argument.** `/cctop-pane tools` opens the pane straight to the
   Tools view without a further keypress.
   Setup: pane closed.
   Keys: `/cctop-pane tools`.
   Expected: the pane opens with Tools (view 2) drawn, view bar shows it
   selected.
   Claude Code version:
   Result: pending

8. **The skill still resolves; the one-liner when the pane is already
   open.** Resolves PRD open questions 1 and 2 together — record which
   event(s) the debug log shows for the skill invocation in both cases.
   Setup (a): pane closed, function hooks enabled.
   Keys (a): `/cctop:cctop`.
   Expected (a): the skill behaves as it did before the pane existed (offers
   `cctop install` / runs `cctop split`, per `plugin/skills/cctop/SKILL.md`);
   note in this file which of `skill.prompt`/`command.run` the debug log
   shows.
   Setup (b): pane open (`/cctop-pane`), function hooks enabled.
   Keys (b): `/cctop:cctop`.
   Expected (b): the model's only reply is the one line `cctop is open in the
   side pane.`; it runs no tool; the debug log lists `skill.prompt` firing.
   Claude Code version:
   Result: pending

9. **The view bar in the pane.** The bar `cctop  Overview  Tools …` is the
   pane's first row, the current view inverse, the others plain Buttons
   without hotkeys; nothing is drawn in the band above the prompt, and a
   digit typed into the empty composer stays there. Verified at 110, 144
   and 200 columns.
   Setup: pane open and docked, at each of 110/144/200 columns in turn.
   Keys: type `2` into the empty composer and wait a second; clear it; click
   `Tools` in the bar; then `ctrl+x tab`, `tab` to `Files`, `enter`, `esc`.
   Expected: the `2` stays in the composer and the pane does not change; the
   click switches to Tools and the bar reads `cctop  Overview  Tools …`
   with `Tools` inverse; the keyboard route switches to Files; the plain
   Buttons draw as their bare label (no `:`, no `[ ]`) — if they draw
   otherwise, `viewBar` in `pane.tsx` must drop `plain` for the `[ Label ]`
   chrome. Nothing appears above the prompt.
   Claude Code version:
   Result: pending

10. **Resize persists to settings.** `ctrl+x` arrows resize the docked pane
    and the engine persists the width to `pluginPanes.dockColumns`.
    Setup: pane open and docked, focused (`ctrl+x tab`).
    Keys: `ctrl+x` left-arrow / right-arrow a few times; then open
    `/config` (or inspect the user's `settings.json`).
    Expected: the pane visibly narrows/widens with each keypress; a
    `pluginPanes.dockColumns` value is present in settings and matches the
    resized width.
    Claude Code version:
    Result: pending

11. **Advisor matches the CLI.** The pane's Advisor view's top row is the
    coach's slot occupant, the same one the binary reports.
    Setup: pane open, `cctop` binary present, a session with at least one
    nudge (the TUI dashboard running in a split makes the occupant the
    running dashboard's, persisted in `~/.cctop/<session>.advisor.json`).
    Keys: `/cctop-pane advisor`; separately, in a shell, run
    `cctop query advice --session <id>`.
    Expected: the pane's top Advisor row (headline / evidence) matches
    `primary` of the CLI's schema-2 JSON for the same session id, and the
    rows below it are `items` in order; with a TUI running beside it, the
    TUI's Advisor row and its dashboard nudge name the same rule.
    Claude Code version:
    Result: pending

11a. **Coach matches the CLI.** The pane's Coach view is the coach object.
    Setup: pane open, `cctop` binary present.
    Keys: `/cctop-pane coach`; separately, in a shell, run
    `cctop query coach --session <id>` and `cctop query coach --line`.
    Expected: the state line, the four light rows and the nudge's two lines
    are the JSON's `state.line`, `lights[].text` and `nudge.line1/line2`
    byte for byte; the status line under the prompt is `--line`'s output
    (L0 at ≥ 80 body columns, L1 below); `[1 fill]` appears only for a
    prompt- or slash-class action and writes it into the prompt box without
    submitting; `[2 snooze]` removes the nudge and `cctop query coach` on
    the same id shows it under `snoozed`.
    Claude Code version:
    Result: pending

12. **Context % is live.** Engine-native values (no `cctop` binary needed)
    update promptly after a turn.
    Setup: pane open on Overview.
    Keys: send any prompt and wait for the response to finish; watch the
    Context percent field.
    Expected: the Context percent reflects the new usage within 1 s of the
    response completing.
    Claude Code version:
    Result: pending

13. **Open latency.** `/cctop-pane` opens fast enough to feel instant.
    Setup: pane closed, fullscreen renderer, function hooks enabled.
    Keys: type `/cctop-pane`, press Enter, and time until the pane's content
    is drawn (a screen recording or a stopwatch is enough precision).
    Expected: under 500 ms from Enter to the pane's body being visible.
    Claude Code version:
    Result: pending

14. **`/reload-plugins` reopens the pane.** US-008's `{ open, view }`
    restore from `$.store` on `session.start`.
    Setup: `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --plugin-dir
    ./plugin`, `/tui fullscreen`, `/cctop-pane tools`.
    Keys: `/reload-plugins`.
    Expected: the pane reopens by itself, still on the Tools view, without a
    further command.
    Claude Code version:
    Result: pending

15. **Idle CPU.** Claude Code's CPU stays low while the pane sits idle
    (US-008: 10 s idle poll cadence, redraws throttled to 4/s).
    Setup: pane open, no turn running.
    Keys: `top -pid $(pgrep -n claude)` for about a minute; read the `%CPU`
    column.
    Expected: under 2 %.
    Claude Code version:
    Result: pending

16. **Turn latency is unaffected.** Loading the hooks module adds no
    perceptible per-turn cost.
    Setup: two runs of 20 turns each from the repository root:
    `for i in $(seq 20); do time claude -p --max-turns 1 'say ok'; done`
    once with `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 --plugin-dir ./plugin` and
    once with neither.
    Keys: none beyond running the loop; record the average wall-clock time
    of each set of 20.
    Expected: the two averages differ by less than 1 %.
    Claude Code version:
    Result: pending

17. **Flag off: fallback still works.** Without function hooks, `/cctop`
    (the skill) still uses `cctop split` exactly as before this feature.
    Setup (a): inside a tmux session, function hooks NOT set (no
    `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS`), `claude --plugin-dir ./plugin`.
    Keys (a): `/cctop`.
    Expected (a): tmux still splits a right-hand pane running the TUI, as it
    did before this feature.
    Setup (b): Apple Terminal (no multiplexer), function hooks NOT set.
    Keys (b): `/cctop`.
    Expected (b): a new Terminal window opens running the TUI (US-011's
    `open -na Terminal <file.command>`), not a new tab.
    Claude Code version:
    Result: pending

18. **Marker file toggles `open`.** `~/.cctop/pane/<session id>.json`
    tracks whether the pane is open, for the skill's short-circuit (US-009)
    and for `cctop split`'s own check.
    Setup: pane closed; note the session id (`$.session.id` / the id in the
    marker filename once created).
    Keys: `/cctop-pane` to open, read the marker file; then close the pane
    with `ctrl+x x` (or `/cctop-pane close`), read the marker file again.
    Expected: after opening, `open: true` with a fresh `heartbeatAt`; after
    closing, `open: false` with a fresh `heartbeatAt`; no further `cctop
    query` calls appear in the debug log after the close.
    Claude Code version:
    Result: pending

19. **`cctop split` short-circuits on an open pane.** Running `cctop split`
    from a shell while the pane is already open for that session does not
    open a second dashboard (US-009's `pane_marker_open`).
    Setup: pane open (`/cctop-pane`, function hooks enabled).
    Keys: from a separate shell, `cctop split --session <id>` (the id from
    the marker file of item 18).
    Expected: prints `cctop pane is already open in this session`, exits 0,
    and opens no terminal split or window.
    Claude Code version:
    Result: pending

20. **No stale marker false-positive.** `/cctop` on a build *without*
    function hooks (or before accepting the plugin's hooks module) still
    falls through to `cctop split` when there is no fresh open marker.
    Setup: no `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS`, no marker file for this
    session (or one with `open: false` / a stale `heartbeatAt`).
    Keys: `/cctop`.
    Expected: the skill's step 3 (`plugin/skills/cctop/SKILL.md`) finds no
    fresh `open: true` marker and falls through to `cctop split` exactly as
    it did before US-009.
    Claude Code version:
    Result: pending

21. **`/diff` hides the pane and says so.** With the cctop pane docked, `/diff`
    takes the dock: the pane disappears and, within a few seconds (the next
    redraw the module asks for), the status line under the prompt reads
    `cctop pane hidden behind the /diff panel: run /diff to show it`.
    Setup: `/tui fullscreen`, ≥ 110 columns, `/cctop-pane` docked.
    Keys: `/diff`; wait up to 12 s.
    Expected: the diff panel shows where the pane was; the status line
    appears; `/diff` again removes the diff panel, the cctop pane is back on
    its previous view and the status line is gone.
    Claude Code version:
    Result: pending

22. **An open while hidden answers "not shown".** With the diff panel showing,
    `/cctop-pane tools` answers `cctop pane on Tools is open but not shown:
    the /diff panel holds the side dock. Run /diff …`, and `/diff` then shows
    the pane on the Tools view.
    Setup: as item 21, diff panel showing.
    Keys: `/cctop-pane tools`, then `/diff`.
    Claude Code version:
    Result: pending

23. **`cctop pane status` inside the session.** `!cctop pane status` (the
    shell prefix) prints every line `✓` and `pane open, docked (N columns)`
    while the pane is docked; without the flag, in a fresh shell, it prints
    the `✗` lines with their actions and exits 2.
    Setup: as item 21.
    Keys: `!cctop pane status`.
    Claude Code version:
    Result: pending

24. **`/clear` rotates the session id under the pane** (issue #2). `/clear`
    starts a new transcript under a new id in the same process and fires no
    `session.start`; the module re-reads the id on every turn and poll and
    follows it.
    Setup: `/tui fullscreen`, ≥ 110 columns, `cctop` binary installed,
    `/cctop-pane` docked, one prompt answered; note the session id in the
    marker filename (`ls -t ~/.cctop/pane | head -1`).
    Keys: `/clear`, then one short prompt (`say ok`); wait for the answer.
    Expected: the header shows `turn 1` and a context of a few k tokens; the
    Tools view fills again within 2 s of the turn; the debug log shows
    `cctop: session <old id> rotated to <new id>: following it` and its
    `cctop query … --session <new id>` lines, and no `no session matches`
    line; `~/.cctop/pane/<old id>.json` reads `open: false`,
    `~/.cctop/pane/<new id>.json` reads `open: true` with a fresh
    `heartbeatAt`; `!cctop pane status` reports the pane open.
    Claude Code version:
    Result: pending

25. **The clock is a Promise on Claude Code ≥ 2.1.271** (issue #3). 2.1.271
    turned `$.clock.now()` into a host event; plugin 0.4.1 awaits every
    reading. A session with the pane never opened must be silent and must
    still write its marker, and an opened pane must fill.
    Setup: Claude Code 2.1.272, plugin 0.4.1 (`claude plugin marketplace
    update cctop && claude plugin update cctop@cctop`, then a restart),
    function hooks on, `cctop` binary installed, `--debug`; note the session
    id (`!cctop pane status` prints it in its first line; `ls -t
    ~/.cctop/pane | head -1` names whichever open pane heartbeat last,
    which is another session's when several are open).
    Keys: one short prompt (`say ok`) with the pane closed; `!cctop pane
    status`; then `/cctop`; wait one poll (≤ 10 s).
    Expected: the transcript shows no `cctop:` line at start-up and the
    debug log no `hook failed`, `Invalid Date` or `takes a non-negative
    number of milliseconds` line; `~/.cctop/pane/<session id>.json` exists
    with `loaded: true`, `open: false` and ISO timestamps; `!cctop pane
    status` shows the hooks-module line ✓; `/cctop` docks the pane, its
    header reads `hooks 2.1.272`, and every light is filled within one poll
    (no `waiting for cctop`).
    Claude Code version: 2.1.273
    Result: pass, 2026-09-16, Claude Code 2.1.273, plugin 0.7.0

26. **The Agents view's columns and the combined cost** (the agent-costs
    PRD). The pane's Agents view lists each subagent as the TUI's agents
    view does — type, model family, time, tokens, `≈$`, `ret`, waste with
    its reason — and the Overview's row 2 prints the combined cost with its
    mark, row 6 `wasted ≈$X` once an agent was killed or came back empty.
    Setup: as item 25; a session that launched two agents, one of them
    stopped with `TaskStop` while it ran (`Explore` something, then stop it
    from the task list), one that returned.
    Keys: `/cctop agents` (or the view bar); then the Overview.
    Expected: two agent rows within one poll of the task notifications;
    the stopped one `✗ … killed` with its dollars, the returned one with a
    `ret` count and `0.00`; below 58 body columns the model and `ret`
    columns are gone and the waste column stays; the Overview's row 2 reads
    `≈$…` with `agents ≈$… (N %)` in its detail and row 6 ends with
    `wasted ≈$…`; `cctop query agents --session <id> | jq .totals` agrees
    with the rows.
    Claude Code version:
    Result: pending

27. **The team group and the team's part of the cost** (the team-costs
    PRD). The pane's Agents view lists the teammates of a team the session
    leads under the subagents — a `team N · M active · ≈$X` group row, then
    one row per member with the TUI's columns: glyph, name, model family,
    time, `ctx`, `tok`, `$` with its mark, human/machine turns, the status
    word — and the Overview's row 2 carries `team ≈$X (P %, N of M)` in its
    detail, row 6 `team ≈$X`.
    Setup: as item 25; a session that spawned two teammates through the
    `Agent` tool (`Agent` with `name` and `team_name`, or a `/team` flow),
    one of which has ended (its transcript ends with a `cost-state`) while
    the other still works.
    Keys: `/cctop agents` (or the view bar); then the Overview; then, in
    the TUI, `6` and Enter, and `a` on Panel 2.
    Expected: the group row within one poll of the second spawn, marked
    `≈` while a teammate works; the ended member `○ … ended` with an exact
    figure (no `≈`, Claude Code's own `totalCostUSD`), the working one
    `● …` with `≈`; below 58 body columns the model and `ctx` columns are
    gone and the status word stays; the Overview's row 2 reads `≈$…` with
    `team ≈$… (N %, 2 of 2)` in its detail and row 6 `team ≈$…`; `cctop
    query agents --session <id> | jq .team` says `source: "config"` while
    `~/.claude/teams/session-<id8>/` exists and `"transcripts"` after the
    team ended and the directory went, with the same members either way;
    the TUI's team rows and the pane's are the same characters at 120
    columns; `a` on Panel 2 takes the team out (`main only`) and back.
    Claude Code version:
    Result: pending

28. **A plain Button with no hotkey draws only its label** (Console,
    PRD dashboard-v2 US-109). Every part of a Console cell after the first
    — the rest of the label, the padding — is a `plain` Button without a
    `hotkey`; the d.ts says such a Button draws the label alone. If the
    engine draws chrome around it, the cells stop being one run of text.
    Setup: as item 25, plugin 0.7.0 (Console), cctop 0.6.0 or newer, a
    docked pane at 162 columns (67 body columns) on a live session.
    Keys: `/cctop`; look at rows 2–4 of the Overview.
    Expected: each cell reads `1: ctx 35% 616k left` as one run — the digit
    in the accent colour, a colon, the label, no brackets and no second
    digit anywhere in the row; the three (two below 80 body columns) cells
    of a row sit at the same column stops as the TUI's at the same width
    (`cctop run --once --session <id> --size 67x24`, the same characters).
    Claude Code version: 2.1.273
    Result: pass, 2026-09-16, Claude Code 2.1.273, plugin 0.7.0

29. **The hover scope lights a Button whose label is only spaces**
    (Console). A cell's padding is a Button of spaces so that the whole
    area presses and lights (FR-13); the d.ts says a `scope` lights every
    element drawn with it while any is hovered, and says nothing about a
    label of spaces.
    Setup: as item 28, a terminal with mouse reporting on.
    Keys: hover the empty tail of a cell (the spaces after `616k left`),
    then the digit, then the label; click the tail.
    Expected: hovering any part inverts the whole cell — digit, label and
    padding together, and nothing of the neighbouring cell; a click on the
    padding swaps the body exactly as a click on the digit does (the rule
    line reads `─── context …`); the act line lights and presses as one
    area too, `a: advisor` included.
    Claude Code version: 2.1.273
    Result: fail, 2026-09-16, Claude Code 2.1.273, plugin 0.7.0 — each Button inverts on its own, not the cell

30. **Six Buttons in one band each claim a bare digit** (Console, FR-15).
    The cells carry `hotkey` `1`–`6`, the act line `a`, home `0`; the
    d.ts says a digit presses from an empty composer and that of two
    Buttons on one hotkey the later wins — it does not say six digits in
    one pane are all honoured.
    Setup: as item 28, the composer empty.
    Keys: `1`, `2`, `3`, `4`, `5`, `6`, `a`, `0` in turn; then type a
    character into the composer and press `3`.
    Expected: each digit swaps the body (the rule line names it and that
    cell alone turns green and bold), `a` opens the advisor body, `0`
    returns to events; with text in the composer the digit goes to the
    composer and nothing swaps; nothing else in Claude Code reacts to the
    digits (no view switch, no dialog).
    Claude Code version:
    Result: pending

31. **The real `bodyColumns` at 35 / 54 / 67 / 85** (Console, PRD
    dashboard-v2 §3.6). The dock arithmetic — `min(floor(columns × 0.45),
    90, columns − 70)`, then the borders and the grip — was traced from the
    binary, not measured on a screen; the width ladder (three cells per
    row at 80, the middle cell form at 28, the act line's tail at 66 and
    its full copy at 72) is pinned to those four widths.
    Setup: as item 28; `~/.cctop/pane/<session>.json` holds `bodyColumns`
    after each render — the marker is the only source (`pane.tsx` writes
    no width line to the debug log; `cctop pane status` prints the
    marker's figure as `docked (N columns)`). A persisted `ctrl+x` resize,
    `pluginPanes.dockColumns` in `~/.claude.json`, replaces the ladder
    with `min(columns − 70, max(24, dockColumns))`
    (`docs/claude-code-panels.md` §4): remove the key with Claude Code
    stopped to read the default, or record which of the two was set.
    Keys: resize the terminal to 110, 132, 162 and 200 columns, `/cctop`
    at each, and read `bodyColumns` from the marker file.
    Expected: `bodyColumns` 35, 54, 67, 85 (±1 for the grip) at the four
    terminal widths; at 35 two short cells per row and no `a: advisor`
    tail, at 54 two short cells and the short act copy, at 67 two middle
    cells, at 85 three wide cells with the full act copy; the pane's rows
    at each width are the TUI's at `--size <bodyColumns>x24`, row for row,
    the footer aside; below 110 columns no dock, the inline strip above the
    prompt: the status line, the engine's usage line, the coach's line, the
    act line.
    Claude Code version:
    Result: pending

32. **`? keys` expands the rule line and collapses it again** (Console,
    PRD dashboard-v2 US-106). Console has no footer; the rule line's right
    half is `0: home  ·  ? keys`, and `?` (the TUI) or a click on `? keys`
    (the pane, a plain Button without a hotkey — the engine takes digits
    and lowercase letters only) turns it into the open body's key map: the
    body's own keys, then the global ones in the TUI, the pane's hotkeys
    and pointer targets in the pane.
    Setup: as item 28.
    Keys: `?` in the TUI (`cctop run`), then `?` again; in the pane, click
    `? keys`, then click the key map.
    Expected: in the TUI the tail reads `0: home  ·  Enter panel  ·  1-6 a
    0 body  ·  Esc home  ·  c coach  ·  A ask  ·  t theme  ·  p pause  ·  L
    sessions  ·  q quit` cut at the width with `…`, and `?` again restores
    `? keys`; the help overlay is not what `?` opens on Console (it is in a
    panel and the coach view); in the pane the click swaps the tail for
    `1-6 a 0 body  ·  click a cell, the act line or home  ·  the view bar
    for the other views` and a click on that text collapses it; the rows
    above and below do not move on either surface.
    Claude Code version:
    Result: pending

### Run notes (automated, 2026-09-14)

An agent drove Claude Code 2.1.270 in a 162×45 tmux window
(`CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --plugin-dir ./plugin --debug`,
`tui: "fullscreen"` in settings) with `tmux send-keys` / `capture-pane`.
Observed, verbatim from the captures:

- items 1, 2, 5: `/cctop-pane` docked the pane beside the transcript on the
  first try; the reply was `cctop pane docked beside the transcript (71
  columns): click a view in its bar to switch (or ctrl+x tab, then tab and enter), ctrl+x x closes it.`
  (72-column dock, 1 for the grip). The debug log had the load line, `/cctop-pane
  listed`, and no `refused` or `hook failed` line.
- item 21: `/diff` answered `Diff panel shown`, the cctop pane vanished, and
  the status line `⚠ cctop: cctop pane hidden behind the /diff panel: run
  /diff to show it` appeared under the prompt within the next poll (≤ 12 s).
- item 22: `/cctop-pane tools` answered `cctop pane on Tools is open but not
  shown: the /diff panel holds the side dock. …`; `/diff` answered `Diff
  panel hidden` and the pane came back on the Tools view with the status
  line cleared.
- item 23: `!cctop pane status` printed seven `✓` lines including `terminal
  162 columns` (read from the session's TTY) and `pane open, docked (71
  columns)`; from a shell without the flag it printed the two `✗` lines with
  their actions and exited 2.

These are the agent's observations, not a person's `Result:` entries.

### Run notes (automated, 2026-09-14, second pass: look & feel, hotkeys)

Same setup at 140×45 (the maintainer's terminal width), `--plugin-dir
./plugin`, plugin 0.2.2:

- The pane draws the TUI's framed panels: `╭cctop ─ claude-opus-5 ─╮` with
  the `○ IDLE  turn 0  —` pill, `╭Context╮╭Tokens & Cost╮` and
  `╭Limits ─ 5h 3 % · 7d 11 %╮╭Turn╮` side by side and closing on one line,
  gauges `▇▁▁▁▁▁▁▁▁▁   11 %` coloured by band; the ANSI capture shows dim
  borders, bold titles and the `success` green from the Claude Code theme.
  No `refused` line in the debug log.
- Item 9 (hotkeys), resolved differently from the checklist's wording: the
  docked pane's keys never reach the plugin (`docs/claude-code-panels.md`
  §5.5). The bar now also sits in the band above the prompt; typing `1`, `5`,
  `2` into the empty composer switched the view each time (debug log:
  `ui.press cctop/overview in AbovePrompt from terminal: settled in 7.2ms`)
  and cleared the digit. After `ctrl+x ctrl+a` the band read `▸ plugin panel
  hidden · ctrl+x ctrl+a or click to show` and a typed `1` stayed in the
  composer; expanding it again re-armed the digits.
- `/reload-plugins` picked up the new module with the pane open; the view
  bar, band and frames came back at once.


### Run notes (automated, 2026-09-14, third pass: the view bar in the pane)

Same setup at 162×45, `--plugin-dir ./plugin --debug`, after the band above
the prompt was removed:

- Item 9 as reworded: the pane's first row read `cctop  Overview  Tools
  Agents  Files  Events  Advisor` with `Overview` inverse (ANSI `7m`); the
  plain Buttons without a hotkey drew as their bare labels. The frames read
  `╭1 Context╮╭2 Tokens & Cost╮`, `╭3 Limits╮╭4 Turn╮` and, on the Tools
  view, `╭5 Tools ─ 0 calls╮`. Nothing was drawn above the prompt.
- `ctrl+x tab`, `tab`, `enter` switched to Tools (debug log: `ui.press
  cctop/tools in Pane from terminal: settled in 1.1ms`) and the inverse
  moved to `Tools`; a `2` typed into the empty composer stayed there and
  switched nothing. No `refused` or `hook failed` line in the debug log.

### Run notes (2026-09-16, the person's own session, plugin 0.7.0)

The person ran Claude Code 2.1.273 (`claude --debug`, `tui: "fullscreen"`, a
221-column terminal, `pluginPanes.dockColumns: 74` in `~/.claude.json`) with
plugin 0.7.0 from the marketplace cache, session `28dc94bc`, and the agent
read the debug log (`~/.claude/debug/28dc94bc-….txt`, 21:46:49 – 21:52:43
UTC) and the marker from inside that session. Observed, verbatim:

- Item 25's log half, on the release build: 0 lines match `hook failed`,
  `Invalid Date` or `takes a non-negative`, and 0 match `refused`. Grep
  with `| grep -v auto-mode`: the auto-mode classifier echoes every Bash
  command into the log, so a grep for the pattern counts itself once. The
  load line reads `hooks module cctop loaded (worker, environment 1, tier
  user); events: session.start,turn.start,turn.complete,session.compact,tool.call,command.run,skill.prompt,ui.close,ui.render`
  — longer than section A's expected text, which predates `turn.*`,
  `session.compact`, `tool.call`, `skill.prompt` and `ui.close`. Then
  `$.command.register (cctop): /cctop-pane listed`, `session.start settled
  in 112.6ms`, `ui.open cctop cctop (unasked, 221 columns): placed` 660 ms
  after the load line (the persisted `{ open, view }` of item 14 reopening
  the pane), `fullscreen boot canary: healthy`. The known benign
  `userConfig` warning is present at 21:46:49.945.
- Item 13's log half: `ui.open … placed` at 21:46:50.945, the first
  `ui.render settled in 34.5ms` at 21:46:51.307; 486 renders in six
  minutes, 481 of them under 20 ms, the slowest 44.9 ms.
- Item 23: `!cctop pane status` printed seven `✓` lines including
  `terminal 221 columns` and `pane open, docked (73 columns)` and exited 0.
- Items 18 and 25's marker half: `{"version":"0.7.0", …, "loaded":true,
  "open":true, "visibility":"visible", "placement":"dock",
  "bodyColumns":73, "viewportColumns":147}` with ISO timestamps;
  `loadedAt` 21:46:50.784, `openedAt` 21:46:50.953 (169 ms apart),
  `heartbeatAt` moving.
- Item 31's arithmetic, a third and a fourth reading: `dockColumns` 74 →
  `min(221 − 70, max(24, 74))` = 74, body 73 (`dock − 1`), viewport 147
  (`columns − dock`); after a `ctrl+x` resize in the same session,
  `dockColumns` 133 → 133, body 132, viewport 88 — the same two rules as
  the hand-off's 137 and 81 readings, four for four. The default ladder is
  still unread (the key was set throughout).
- Items 28–30's pointer half: fourteen `ui.press cctop/… in Pane from
  terminal` lines settled in 0.4–3.8 ms — the view bar (`tools`, `agents`,
  `files`, `events`, `coach`, `overview`), five cells (`cell-context`,
  `cell-cache`, `cell-work`, `cell-tools`, `cell-cost`) and two act lines
  (`act-0-2`, `act-0-0`); twelve `$.ui.status (cctop)` lines, the first
  `○0% ○— ○— ○—`, the next `○5% ○2% ○59m ○ok`.
- A second session (`3113c1cc`) was heartbeating the same 73 / 147 from
  plugin **0.4.1**: it loaded its module at 21:14:31, eleven minutes before
  `installed_plugins.json`'s `lastUpdated` 21:25:53 moved the cache to
  0.7.0, and a hooks module is read once at session start. It had no debug
  log (no `--debug`); a restart is what moves it to 0.7.0.

These are the agent's readings of the log and the marker, not a person's
`Result:` entries; item 25's `Result:` stays pending until the person
confirms the header badge and the filled lights at the terminal.
