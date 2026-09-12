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
runs on every change; the live checks a person must run at a real terminal
(a docked pane at a given width, hotkeys, `/reload-plugins`, CPU, `cctop
split` in tmux or Apple Terminal) are tracked in
[`docs/verification/pane.md`](../docs/verification/pane.md) and are `pending`
until someone runs them.

## What it costs

Nothing rides in the prompt except the two one-line skill entries. Queries are
only run when you ask; the skill never injects metrics unprompted.
