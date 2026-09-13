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
brew install tomstagl/tap/cctop     # or: cargo install cctop
```

Then the plugin, from a local checkout or the marketplace entry:

```
claude plugin add /path/to/cctop/plugin
# or, once published:
claude plugin add tomstagl/cctop
```

Restart Claude Code (or `/reload-plugins`) and type `/cctop`.

## The pane

With function hooks enabled, `/cctop` docks the dashboard beside the
transcript instead of opening a terminal split. It shows the same six views
as the standalone TUI's panels:

| Key | View | What it shows |
|---|---|---|
| 1 | Overview | Header, Context, Tokens & Cost, Limits, Turn — the TUI's top half |
| 2 | Tools | Calls, errors, p50/p95, tokens pushed into context, per tool |
| 3 | Agents | Subagents, MCP servers, background tasks |
| 4 | Files | Touched files, edits, re-reads |
| 5 | Events | Tool / hook / permission / compaction stream |
| 6 | Advisor | Ranked, evidence-backed recommendations |

`/cctop [view|close]` opens the pane (optionally straight to a view — one of
`overview`, `tools`, `agents`, `files`, `events`, `advisor`), or closes it;
with no argument it toggles. Context, cost and rate limits come from the
engine itself, so the pane is useful with nothing installed; once the `cctop`
binary is found, the rest (tool timings, files, agents, the Advisor) is
filled in from `cctop query`.

## Enabling function hooks

Function hooks are early access: start Claude Code with the flag, then switch
to the surface that draws a docked pane.

```
CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude
```

Then `/tui fullscreen` (the classic renderer draws the pane's short form
inline above the prompt instead of docking it). The first load asks you to
accept the plugin's hooks module once, in `/plugin`.

## Fallback

Without function hooks (or on a build too old for them), `/cctop` falls back
to `cctop split`, splitting the current terminal multiplexer instead of
docking inside Claude Code. If no multiplexer is detected, it prints the
manual command for a second terminal.

## Verification status

Automated (typecheck, tests, `claude plugin validate --strict`, `cargo test`)
runs on every change and is green. Nobody has yet run the checklist below at a
real terminal, so every item is `Result: pending` in
[`docs/verification/pane.md`](../docs/verification/pane.md):

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

See the checklist for the exact setup, keys and expected observation for each.

## What it costs

Nothing rides in the prompt except the two one-line skill entries. Queries are
only run when you ask; the skill never injects metrics unprompted.
