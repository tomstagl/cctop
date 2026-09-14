# cctop plugin for Claude Code

Two skills, plus a docked pane on builds with function hooks:

- **`/cctop`** — on a build with function hooks enabled, opens the dashboard
  in a pane docked inside Claude Code itself. Otherwise opens it in a
  right-hand pane of the current terminal (tmux, zellij, WezTerm, Kitty,
  iTerm2) via `cctop split`. Offers `cctop install` once.
- **`cctop-insights`** — lets the session answer questions like "why is my
  cache hit ratio low?" or "what is filling my context?" by running
  `cctop query … --json` and reading the numbers.

## Install

The binary first:

```
brew install tomstagl/tap/cctop     # currently v0.2.0; or: cargo install cctop
```

Then the plugin, from a local checkout or the marketplace entry:

```
claude plugin add /path/to/cctop/plugin
# or, once published:
claude plugin add tomstagl/cctop
```

Restart Claude Code (or `/reload-plugins`) and type `/cctop`.

## The pane

With function hooks enabled, `/cctop-pane` docks the dashboard beside the
transcript instead of opening a terminal split. It is drawn the way the
standalone TUI draws its panels — round frames titled `╭2 Tools ─ 257 calls╮`,
gauges (`▇▇▇▁▁▁`) coloured by band, the status pill (`● BUSY`), a context
sparkline once turns have run, dim secondary text — in Claude Code's own
theme colours, so it follows light and dark. It shows the same six views as
the TUI's panels:

| Key | View | What it shows |
|---|---|---|
| 1 | Overview | Header, Context, Tokens & Cost, Limits, Turn — the TUI's top half |
| 2 | Tools | Calls, errors, p50/p95, tokens pushed into context, per tool |
| 3 | Agents | Subagents, MCP servers, background tasks |
| 4 | Files | Touched files, edits, re-reads |
| 5 | Events | Tool / hook / permission / compaction stream |
| 6 | Advisor | Ranked, evidence-backed recommendations |

`/cctop-pane [view|close]` opens the pane (optionally straight to a view — one
of `overview`, `tools`, `agents`, `files`, `events`, `advisor`), or closes it;
with no argument it toggles.

### Switching views by key

While the pane is docked, a one-line bar `cctop 1: Overview  2: Tools …` sits
in the band above the prompt. **Type the digit into the empty prompt and pause
a moment** (about half a second): the view switches and the digit is cleared.
A second character typed within that moment cancels it, so a prompt that
starts with a digit is safe to keep typing. Clicking a bar entry — above the
prompt or at the top of the pane — switches too.

That band is the only place Claude Code honours a Button's `hotkey`; keys
pressed inside the docked pane never reach the plugin, whether or not it is
focused (`ctrl+x tab`). If you would rather keep the digits for yourself,
collapse the band with `ctrl+x ctrl+a` (`[-]`): the bar folds to one line and
the hotkeys are off until you expand it again. Context, cost and rate limits come from the
engine itself, so the pane is useful with nothing installed; once the `cctop`
binary is found, the rest (tool timings, files, agents, the Advisor) is
filled in from `cctop query`.

Every open answers with where the pane went, so the state is never a guess:

| reply | meaning |
|---|---|
| `cctop pane docked beside the transcript (71 columns): …` | in the side dock |
| `cctop pane drawn above the prompt: the terminal is 100 columns wide, 110 or more dock it …` | inline, terminal too narrow |
| `cctop pane drawn above the prompt: /tui fullscreen docks it …` | inline, classic renderer |
| `cctop pane is open but not shown: the /diff panel holds the side dock. Run /diff …` | hidden behind the diff panel |

### `/clear` and the session id

`/clear` starts a new transcript under a new session id in the same Claude
Code process, and the plugin API fires no `session.start` for it. The pane
reads the id again on every turn and every poll, so after a `/clear` it
follows the new session: the old session's figures are dropped, the context
is re-read from the engine, `cctop query` is run for the new id at once, and
the marker file of the old id says `open: false` while the new id gets its
own. Before 0.3.1 the pane kept the first id and every query then failed with
`no session matches` for the rest of the process (issue #2).

### The `/diff` panel and the pane share one dock

Claude Code's built-in `/diff` panel and a plugin pane occupy the same
right-hand slot, and the diff panel wins: while it shows, an open cctop pane
is drawn nowhere. The plugin notices (no render arrives) and pins a status
line under the prompt — `cctop pane hidden behind the /diff panel: run /diff
to show it` — until `/diff` hides the diff panel again, at which point the
cctop pane reappears where it was. The engine gives a plugin no other signal
for this; the mechanism is written up in
[`docs/claude-code-panels.md`](../docs/claude-code-panels.md).

### `cctop pane status`

When the pane does not appear, `cctop pane status` (run from inside the
session, or with `--session <id>`) prints one line per prerequisite with the
fix after `→`, and exits 0 when the hooks module runs in that session, 2
otherwise:

```
cctop pane status · session ab339470
  ✓ Claude Code 2.1.270 (function hooks need 2.1.269 or newer)
  ✗ function hooks off → add "CLAUDE_CODE_ENABLE_FUNCTION_HOOKS": "1" to the "env" block of ~/.claude/settings.json (or export it in the shell), then restart Claude Code
  ✗ installed cctop plugin 0.1.0 has no hooks module (skills only; 0.2.0 or newer ships it) → `claude plugin update cctop@cctop` (or start with `claude --plugin-dir <checkout>/plugin`), then restart Claude Code
  ✓ fullscreen renderer (tui = "fullscreen" in ~/.claude/settings.json)
  ✓ terminal 162 columns (110 or more dock the pane)
  ! the /diff panel was open when last toggled (diffSidebarOpen in ~/.claude.json); while it shows, it holds the side dock → if it is showing, run /diff to hide it; cctop takes the dock
→ not ready: fix the ✗ lines, restart Claude Code, then run /cctop-pane
```

The `/cctop` skill runs it first and relays the lines verbatim before it
falls back to `cctop split`, so a session without the pane still tells you
exactly why. `--json` prints the same report as data.

## Enabling function hooks

Function hooks are early access. Put the flag where every session sees it —
the `env` block of `~/.claude/settings.json` — and restart Claude Code:

```json
{ "env": { "CLAUDE_CODE_ENABLE_FUNCTION_HOOKS": "1" } }
```

(`CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude` does the same for one start.)
Then `/tui fullscreen` (the classic renderer draws the pane's short form
inline above the prompt instead of docking it) and a terminal of 110 columns
or more. The first load asks you to accept the plugin's hooks module once, in
`/plugin`. The plugin must be 0.2.0 or newer — `claude plugin update
cctop@cctop` — since 0.1.0 shipped skills only.

## Fallback

Without function hooks (or on a build too old for them), `/cctop` falls back
to `cctop split`, splitting the current terminal multiplexer instead of
docking inside Claude Code. If no multiplexer is detected, it prints the
manual command for a second terminal.

## Verification status

Automated (typecheck, tests, `claude plugin validate --strict`, `cargo test`)
runs on every change and is green. On 2026-09-14 an agent drove a real
Claude Code 2.1.270 in a 162×45 tmux window (see the run notes in
[`docs/verification/pane.md`](../docs/verification/pane.md)): the pane docked,
`/diff` hid it and the status line said so, `/diff` again brought it back.
The `Result:` lines below are still for a person to fill in:

1. `/cctop-pane` listed in the slash menu with its description
2. pane docks at 144 columns in `/tui fullscreen`
3. pane docks at 110 columns in `/tui fullscreen`
4. pane draws inline in `/tui default`
5. `/cctop-pane` toggles open/closed
6. `/cctop-pane close` closes the pane
7. `/cctop-pane tools` opens straight to the Tools view
8. `/cctop:cctop` (the skill) still resolves, and answers the one-liner when
   the pane is already open
9. hotkeys `1`-`6` act only after `ctrl+x tab` gives the pane focus
10. `ctrl+x` arrows resize the pane and persist `pluginPanes.dockColumns`
11. the Advisor view's top row matches `cctop query advice --session <id>`
12. Context % updates within 1 s of a response
13. `/cctop-pane` opens in under 500 ms
14. `/reload-plugins` reopens the pane on its previous view
15. Claude Code's idle CPU with the pane open stays under 2 %
16. turn latency with vs. without `--plugin-dir` differs by under 1 %
17. with function hooks off, `/cctop` still splits in tmux and opens a new
    window in Apple Terminal
18. the marker file `~/.cctop/pane/<id>.json` toggles `open` on open/close
19. `cctop split` short-circuits when the pane is already open
20. `/cctop` falls back to `cctop split` when there is no fresh open marker
21. `/diff` over a docked pane hides it and pins the status line; `/diff`
    again restores the pane
22. an open while the diff panel shows answers "open but not shown"
23. `cctop pane status` from inside a session reports every line ✓ once the
    pane is docked, and names the missing prerequisites otherwise
24. `/clear` under an open pane: the next turn's queries run for the new
    session id, the old id's marker says `open: false`, the new id's
    `open: true`, and no `no session matches` line appears in the debug log

See the checklist for the exact setup, keys and expected observation for each.

## What it costs

Nothing rides in the prompt except the two one-line skill entries. Queries are
only run when you ask; the skill never injects metrics unprompted.
