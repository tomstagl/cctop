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
brew install tomstagl/tap/cctop     # currently v0.8.0; or: cargo install cctop
```

Then the plugin, from a local checkout or the marketplace entry:

```
claude plugin add /path/to/cctop/plugin
# or, once published:
claude plugin add tomstagl/cctop
```

Restart Claude Code (or `/reload-plugins`) and type `/cctop`.

### Claude Code versions

The pane needs function hooks, which shipped in Claude Code **2.1.269**.
Each plugin release is built against one Claude Code version — the
`TESTED_WITH` the header's `hooks …` light shows — and runs on the releases
between the last contract change and the next one:

| plugin | tested with | runs on |
|---|---|---|
| 0.2.0 – 0.4.0 | 2.1.269 / 2.1.270 | 2.1.269 – 2.1.270 only (`$.clock.now()` became a Promise in 2.1.271, issue #3) |
| 0.4.1 – 0.7.0 | 2.1.272 / 2.1.273 | 2.1.269 and later, as far as CI has seen (the module awaits every host call) |
| 0.8.0 | 2.1.274 | 2.1.269 – 2.1.274 (`$.agent.register`, `position: "absolute"` on `Box`, a `Markdown` element — additive, nothing the pane calls moved) |
| 0.9.0 | 2.1.278 | 2.1.269 and later, as far as CI has seen (an `Image` element, sub-cell pointer coordinates, `$.ui.panes()` / `$.ui.root()`, `$.prompt.read()`, a budget on `next` and four events, and re-typed `$.fs.read` (a bytes form) and `$.prompt.fill`'s result — additive, nothing the pane calls moved); says so at session start (`cctop: plugin 0.9.0 (hooks contract 2.1.278) loaded; self-check ok`) and writes `testedWith` into the marker, which `cctop pane status` pairs with `claude --version` |

Function hooks are early access and change between Claude Code releases.
CI checks the contract against the latest release daily
(`scripts/check-contract.sh`), `cctop pane status` pairs the installed
plugin with `claude --version`, and the module says at session start what
it found (see "When the contract moves"). When a release breaks the pane,
the fix is a plugin release; `claude plugin marketplace update cctop &&
claude plugin update cctop@cctop`, then restart Claude Code.

## The pane

With function hooks enabled, `/cctop-pane` docks the dashboard beside the
transcript instead of opening a terminal split. It is drawn the way the
standalone TUI draws it — Console: the header, six cells, the act line, the
rule line and one body, row for row; round frames titled `╭5 Tools ─ 257
calls╮` in the other views, gauges (`▇▇▇▁▁▁`) coloured by band, dim secondary
text — in Claude Code's own theme colours, so it follows light and dark. The
cells, the act line and `0: home` are Claude Code's own clickable chrome: a
plain Button per part, the digit it draws being the hotkey, the whole cell
lit and pressed as one area. It shows the same views as the TUI:

| View | TUI | What it shows |
|---|---|---|
| Coach | `c` | The 56-column card: the state line, the four lights, the one nudge with `[1 fill]` `[2 snooze]` `[3 why]`, what is next and what is snoozed, the detail frame of the highest light |
| Overview | the dashboard | Console: the header, the six cells (`1`–`6`), the act line (`a`), the rule line (`0` home, `? keys`), the open body in place — every figure the TUI's nine panels carry, one body at a time |
| Tools | 5 | Calls, errors, p50/p95, tokens pushed into context, per tool |
| Agents | 6 | Subagents, MCP servers, background tasks |
| Files | 7 | Touched files, edits, re-reads |
| Events | 8 | Tool / hook / permission / compaction / coach / cost stream |
| Advisor | 9 | The slot's occupant, then what is queued and what is snoozed |

The Coach and Overview views draw `cctop query coach` and `cctop query
dashboard` verbatim — the same objects the TUI draws — so the pane and the
terminal show the same nudge at the same moment. The frames inside the other
views carry the TUI's panel digits (`╭5 Tools ─ 257 calls╮`), the same
numbers the guide and `cctop query` use. `[1 fill]` writes a prompt-class
action into the prompt box (`$.prompt.fill`) and never submits it; the
coach's one-line form sits under the prompt (`$.ui.status`) and changes only
when a light's level or the nudge changes.

`/cctop-pane [view|close]` opens the pane (optionally straight to a view — one
of `coach`, `overview`, `tools`, `agents`, `files`, `events`, `advisor`), or
closes it; with no argument it toggles.

### Switching views

The bar `cctop  Coach  Overview  Tools  Agents  Files  Events  Advisor` is the
pane's first row, the current view drawn inverse. Click another name to switch, or
give the pane the keyboard with `ctrl+x tab`, move with `tab` / `shift+tab`,
press `enter`, and leave with `esc`. `/cctop-pane <view>` switches without
either. Nothing is drawn above the prompt: Claude Code honours a Button's
hotkey only in the band there, and that band cost the transcript a row, so
the pane has no digit hotkeys — a digit typed into the composer is yours.

Context, cost and rate limits come from the engine itself, so the pane is
useful with nothing installed; once the `cctop` binary is found, the rest
(the lights, the nudge, tool timings, files, agents, the Advisor) is filled in
from `cctop query`.

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

### Claude Code 2.1.271 and the clock

Claude Code 2.1.271 turned `$.clock.now()` from a number into a host event
that resolves a Promise. Plugin 0.4.0 did arithmetic on it, so on 2.1.271
and later every hook failed — `marker write failed: RangeError: Invalid
Date` at the start of every session, every light on `waiting for cctop`
once the pane was open, and `cctop pane status` reporting the hooks module
as not loaded (issue #3). Plugin 0.4.1 awaits every reading and runs on
2.1.270 and 2.1.272 alike; the header's `hooks …` light names the Claude
Code version the module's contract was generated from (`2.1.278` for plugin
0.9.0; 2.1.273 added a `vscode` render surface, 2.1.274 `$.agent.register`
and absolute `Box` placement, 2.1.278 an `Image` element, `$.prompt.read()`
and a bytes form of `$.fs.read` — none moved anything the pane calls).
Update with
`claude plugin marketplace update cctop && claude plugin update
cctop@cctop`, then restart Claude Code.

### When the contract moves

Function hooks are early access and change between Claude Code releases
(issue #4). At `session.start` the module checks the surfaces it cannot do
without — the clock resolves a number, `HOME` is set, the session has an id
— and writes one line to the debug log (`claude --debug`,
`~/.claude/debug/latest`) before anything else:

```
cctop: plugin 0.9.0 (hooks contract 2.1.278) loaded; self-check ok
```

When a surface moved, the line reads `self-check failed: <what>` with the
update command, the marker carries the same text under `selfCheck`, and
`cctop pane status` relays it on the hooks-module line. Every clock reading
falls back to the environment's own `Date.now()`, so the pane keeps its
books on a changed clock instead of failing every hook as 0.4.0 did.

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
  ✓ Claude Code 2.1.272 (function hooks need 2.1.269 or newer)
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

Once a plugin with the hooks module is installed, a `compatibility` line
pairs it with the Claude Code that runs it. The plugin version is the one the
session loaded (the marker's) when the module runs, else the installed one;
what it was tested with is the module's `TESTED_WITH` (the header's `hooks
…` light), read from the marker or from the install's `hooks/model.ts`:

```
  ✓ cctop plugin 0.9.0 tested with Claude Code 2.1.278
  ! Claude Code 2.1.280 is newer than cctop plugin 0.9.0 was tested with (2.1.278); function hooks are early access and change between releases → `claude plugin marketplace update cctop && claude plugin update cctop@cctop` when a newer plugin is out, then restart Claude Code; if a hook fails meanwhile, report it with both versions
  ✗ cctop plugin 0.4.0 does not work on Claude Code 2.1.272: 2.1.271 made `$.clock.now()` resolve a Promise and the module did arithmetic on it, so every hook failed (issue #3) → `claude plugin marketplace update cctop && claude plugin update cctop@cctop` (plugin 0.4.1 or newer), then restart Claude Code
```

The ✗ comes from a short table of pairs known to fail (`KNOWN_INCOMPATIBLE`
in `src/pane.rs`), checked before the `TESTED_WITH` comparison; the `!` is
the general case, a Claude Code the module has not been tested against. The
table ships with the cctop binary, so the line is only as current as
`brew upgrade cctop`.

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
On 2026-09-15 the headless load check ran on Claude Code 2.1.272 with plugin
0.4.1 (issue #3): the module loaded, no hook failed, and the marker was
written for a session whose pane was never opened. The `Result:` lines
below are still for a person to fill in:

1. `/cctop-pane` listed in the slash menu with its description
2. pane docks at 144 columns in `/tui fullscreen`
3. pane docks at 110 columns in `/tui fullscreen`
4. pane draws inline in `/tui default`
5. `/cctop-pane` toggles open/closed
6. `/cctop-pane close` closes the pane
7. `/cctop-pane tools` opens straight to the Tools view
8. `/cctop:cctop` (the skill) still resolves, and answers the one-liner when
   the pane is already open
9. the view bar sits in the pane, nothing above the prompt; a click and
   `ctrl+x tab` + `tab` + `enter` switch views
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
25. on Claude Code 2.1.272, a session with the pane never opened logs no
    `cctop:` failure, its marker says `loaded: true`, `cctop pane status`
    shows the module ✓, and `/cctop` fills every light within one poll

See the checklist for the exact setup, keys and expected observation for each.

## What it costs

Nothing rides in the prompt except the two one-line skill entries. Queries are
only run when you ask; the skill never injects metrics unprompted.
