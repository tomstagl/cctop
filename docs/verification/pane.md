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

## B. Live checks

Collected in US-010. Until then every live item of US-001 and US-008 is listed
here so nothing is silently assumed:

1. `/cctop-pane` appears in the slash-command typeahead with the description
   `Open the cctop dashboard pane` and the hint `[view|close]`.
   Setup: `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --plugin-dir ./plugin`,
   type `/cctop-` and read the menu.
   Claude Code version:
   Result: pending
2. `/cctop-pane` opens a pane whose body reads `cctop`, docked beside the
   transcript in `/tui fullscreen` at 144 columns and at 110 columns, and
   inline above the prompt in `/tui default`.
   Claude Code version:
   Result: pending
3. `/cctop:cctop` (the skill) still resolves and behaves as before the pane
   existed. Record which events the debug log shows for it (`skill.prompt`,
   `command.run`, both) to settle open question 2 of the PRD.
   Claude Code version:
   Result: pending
4. `/reload-plugins` with the pane open reopens it on the same view (US-008:
   `{ open, view }` is restored from `$.store` on `session.start`).
   Setup: `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude --plugin-dir ./plugin`,
   `/tui fullscreen`, `/cctop-pane tools`, then `/reload-plugins`; the pane
   should come back on Tools without a further command.
   Claude Code version:
   Result: pending
5. Claude Code's CPU is under 2 % while idle with the pane open (US-008: the
   poller runs every 10 s idle, redraws are throttled to 4/s).
   Setup: pane open, no turn running; `top -pid $(pgrep -n claude)` for a
   minute and read the `%CPU` column.
   Claude Code version:
   Result: pending
6. The person's close of the pane fires the module's `ui.close` hook (US-008).
   The engine's `hooks module cctop loaded ... events:` debug line names the
   engine events only, so the hook on the op-event `ui.close` is unconfirmed
   headlessly. Setup: pane open, close it with the pane's close key, then read
   `~/.cctop/pane/<session id>.json`: `open` should be `false` with a fresh
   `heartbeatAt`, and the debug log should show no query after the close.
   Claude Code version:
   Result: pending
