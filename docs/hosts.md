# Side-by-side hosts

`cctop split [--size N]` resolves the session first, then asks the terminal
multiplexer to open a right-hand pane running `cctop run --session <id>`.
The host spawns the pane process, so cctop is never a child of Claude Code
and quitting it (`q`) cannot signal the session.

When the session isn't running inside a multiplexer, there is no existing
pane to split — the current terminal window belongs to Claude Code itself.
In that case `split` falls back to opening a *new window* in the detected
terminal app (Terminal.app or iTerm2) instead of a pane, so the dashboard is
still one command away rather than requiring the user to manually open a
second terminal.

| Host | Detected by | Command issued | Verified |
|---|---|---|---|
| tmux | `$TMUX` | `tmux split-window -h -l 45% -t $TMUX_PANE cctop run --session <id>` | **yes** — 2026-09-11, tmux 3.x on macOS: 132×40 window → panes 72×40 + 59×40; pane process parent is the tmux server; `q` closed only the cctop pane, the neighbour survived |
| zellij | `$ZELLIJ` | `zellij action new-pane -d right -- cctop run --session <id>` | not yet — zellij is not installed on the dev machine; command shape unit-tested only |
| WezTerm | `$WEZTERM_PANE` | `wezterm cli split-pane --right --percent 45 -- cctop run --session <id>` | not yet — WezTerm is not installed on the dev machine; command shape unit-tested only |
| Kitty | `$KITTY_LISTEN_ON` | `kitten @ launch --location=vsplit cctop run --session <id>` | not yet (needs `allow_remote_control yes`) |
| iTerm2 (inside a session) | `$ITERM_SESSION_ID` | `osascript -e 'tell application "iTerm2" … split vertically with default profile'` | not yet |
| iTerm2 (new window) | `$TERM_PROGRAM=iTerm.app`, no `$ITERM_SESSION_ID` split context | `osascript -e 'tell application "iTerm2" to tell (create window with default profile) … write text'` | not yet |
| Terminal.app (new window) | `$TERM_PROGRAM=Apple_Terminal` | `osascript -e 'tell application "Terminal" to do script "cctop run --session <id>"'` | **yes** — 2026-09-12, macOS Terminal.app: opened a new window running `cctop run --session <id>` from a plain (non-tmux) Claude Code session |
| none | — | prints the manual command, exit code 3 | yes |

Reproduce the tmux check without a live Claude Code session:

```
tmux new-session -d -s t -x 132 -y 40 'sleep 120'
SOCK=$(tmux display-message -p -t t '#{socket_path}')
PANE=$(tmux list-panes -t t -F '#{pane_id}' | head -1)
TMUX="$SOCK,1,0" TMUX_PANE=$PANE cctop split --session fixtures/session-a
tmux list-panes -t t -F '#{pane_index} #{pane_width}x#{pane_height} #{pane_current_command}'
tmux kill-server
```

Minimum useful pane width is 40 columns (panels collapse below ~50); 45 % of
a 132-column window gives 59, which fits the narrow layout.
