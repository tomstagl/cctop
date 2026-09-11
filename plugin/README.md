# cctop plugin for Claude Code

Two skills:

- **`/cctop`** — opens the dashboard in a right-hand pane beside the current
  session (tmux, zellij, WezTerm, Kitty, iTerm2). Offers `cctop install` once.
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

## What it costs

Nothing rides in the prompt except the two one-line skill entries. Queries are
only run when you ask; the skill never injects metrics unprompted.
