# cctop pane — verification

The pane is the cctop dashboard drawn inside Claude Code's own TUI through the
function-hooks plugin API (early access, `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1`).
Its module lives in `plugin/hooks/pane.tsx`.

Two kinds of checks live here:

- **Automated** checks run in CI and on every story (`npm run typecheck`,
  `npm test`, `claude plugin validate --strict ./plugin`, `cargo test`). The
  headless load check below is automated too, but it needs the `claude` CLI on
  the machine, so it is run by hand and its result recorded.
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
grep -i 'refused' ~/.claude/debug/latest                        # expect no output
grep 'hook failed' ~/.claude/debug/latest                       # expect no output
```

Expected:

- `claude -p` prints `ok` (or similar) and exits 0.
- The log holds one line `hooks module cctop loaded (... ); events: session.start,command.run,ui.render`.
- The log holds `$.command.register (cctop): /cctop-pane listed`.
- No line contains `refused` or `hook failed`.

Known benign line: `[WARN] plugin cctop: options requested but its manifest
declares no userConfig; every option reads as absent`. The plugin declares no
`userConfig`, so every option is absent; nothing is refused.

Why the command is `/cctop-pane` and not `/cctop`: on 2.1.269 the engine
reserves `/cctop` for the plugin's own skill (`/cctop:cctop`) and refuses
`$.command.register({ name: "cctop" })` with
`"/cctop" refused: it is the plugin's /cctop:cctop`. This resolves open
question 1 of `tasks/prd-cctop-pane.md`: the native command is `cctop-pane`,
and `/cctop` keeps resolving to the skill, which is the fallback path.

Last automated run: 2026-09-12, Claude Code 2.1.269, by the agent: exit 0, load line present, `/cctop-pane listed`, no `refused` or `hook failed` line.
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

9. **Hotkeys gated by focus.** View-bar hotkeys `1`-`6` act only once the
   pane has focus (`ctrl+x tab`), so they never steal digits from the
   composer. Verified at 110, 144 and 200 columns.
   Setup: pane open and docked, at each of 110/144/200 columns in turn.
   Keys: press `2` while the composer (not the pane) has focus; then press
   `ctrl+x tab` to move focus to the pane; then press `2` again.
   Expected: the first `2` is typed into the composer / does nothing to the
   pane; after `ctrl+x tab` the pane is focused and the second `2` switches
   it to the Tools view.
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

11. **Advisor matches the CLI.** The pane's Advisor view's top row matches
    what the binary itself reports.
    Setup: pane open, `cctop` binary present, a session with at least one
    Advisor recommendation.
    Keys: `/cctop-pane advisor`; separately, in a shell, run
    `cctop query advice --session <id>`.
    Expected: the pane's top Advisor row (headline / evidence) matches
    `advice[0]` of the CLI's JSON output for the same session id.
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
