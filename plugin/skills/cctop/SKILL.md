---
name: cctop
description: Open the cctop dashboard in a right-hand pane beside this session. Use when the user types /cctop or asks to see the session dashboard, monitor, or "top" for Claude Code.
user-invocable: true
---

# /cctop

Open the live dashboard next to this session. Everything runs in a separate
pane; **never print dashboard output into the conversation**.

## Steps

1. Check the binary: `command -v cctop`. If missing, stop and tell the user:

   ```
   brew install tomstagl/tap/cctop     # or: cargo install cctop
   ```

2. First run only — if `~/.cctop/installed` does not exist, ask once:
   "cctop can also install a status-line shim and hooks (exact rate limits,
   tool timings, permission waits). Run `cctop install` now? It shows a diff
   of ~/.claude/settings.json and backs the file up."
   - yes → run `cctop install --yes` and report the backup path it printed.
   - no → run `touch ~/.cctop/declined-install` and do not ask again; the
     dashboard works from the transcript alone.
   Skip the question when either marker file exists.

3. Open the pane: `cctop split`. It resolves this session itself (from
   `$CLAUDE_SESSION_ID`, the tmux pane, or the working directory).
   - exit 0 → say "cctop is open in the right-hand pane; press ? there for keys."
   - exit 3 → no tmux/zellij/WezTerm/Kitty/iTerm2 detected: relay the manual
     command it printed (`cctop run --session <id>`) for a second terminal.
   - other → relay the error text.

Do not run `cctop run` in the foreground of this session's shell: it is a
full-screen program and would block the tool call.
