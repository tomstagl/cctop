# Claude Code side panels: `/diff` and plugin panes

How Claude Code draws a panel beside the transcript, traced in the 2.1.270
binary (build `97ecbf7`, 2026-09-12), and what it means for the cctop pane.
Nothing here is documented upstream; every statement below was read out of
the bundled JavaScript (`strings` over the Bun-compiled executable, then the
chunk sources), so re-check it after a Claude Code update. Minified names are
quoted only where they help find the code again.

**The one finding that changes cctop's behaviour:** the `/diff` panel and a
plugin pane share the same dock and are mutually exclusive — while the diff
panel is open, an open plugin pane is not drawn at all (§5). Everything the
plugin says to the user about "opened" has to account for that.

## 1. Two panels, one slot

Claude Code's fullscreen renderer has one right-hand slot beside the
transcript. Two things can occupy it:

| Panel | Who draws it | Opened by |
|---|---|---|
| Diff panel | the engine (built in) | `/diff`, or automatically once the session touched a file |
| Plugin pane | a plugin's hooks module, through `ui.render` | `$.ui.open({ id, title })` from a function-hooks plugin |

Both exist only in the **fullscreen renderer**. In the classic renderer
`/diff` opens a modal dialog and a plugin pane draws *inline above the prompt*.

## 2. `/diff` end to end

### 2.1 Command dispatch

`/diff` is a `local-jsx` command (`{ name: "diff", immediate: (args, mode) => …,
thinClientDispatch: "control-request" }`). Its `call` picks one of two paths:

```
ORt(presentation, dispatchedAsImmediate) === "fullscreen"
  → ctx.toggleDiffPanel()      // side panel; the answer is printed as a system line
  else → <DiffDialog>          // the classic modal
```

`ORt` answers `"inline"` when the session is a thin client / remote viewer
(`$t()` = `Wn() || Zn() !== null`) or when the presentation is not
`"fullscreen"`; otherwise `"fullscreen"` when the kill switch is off
(`QZ()` = `!tengu_jazzy_ripple`) or the command was dispatched as immediate.
The answer text is one of `Diff panel shown`, `Diff panel hidden`, or a refusal.

### 2.2 The toggle (`Qmt` in chunk `ymycwj3s`)

```
if replTab === "diff"          → switch back to "convo"           (hidden)
if !QZ()                       → refused: "The diff panel isn't available right now — run /diff again…"
if !isGitRepo(cwd)             → refused: "The diff panel shows git changes — the current directory isn't in a git repository"
if columns < 110               → refused: "Resize your terminal to at least 110 columns to show the diff panel"
else                           → prefetch the diff (≤ 2 s wait), then replTab := "diff"   (shown)
```

Switching (`ne`) sets app state `{ replTab: "diff" | "convo", panelFileView: null }`
and **persists** `diffSidebarOpen: true | false` into `~/.claude.json`
(global config). That key is why a machine that once ran `/diff` keeps the
panel's auto-open armed (§2.4).

### 2.3 Where the toggle lives

`toggleDiffPanel` is a member of the tool-use context built by the REPL
controller; it calls `Qmt` with the live `columns`, `replTab`, the file-history
`trackSequence` and the settings store. `/diff` is therefore purely a UI
state flip — no transcript message, no model turn.

### 2.4 Auto-open

The sidebar component (`age`) arms a 150 ms timer whenever all of these hold:

- `replTab === "convo"` and `fileHistory.trackedFiles.size > 0` (the session
  edited or created a file) and that count changed since the panel was enabled;
- fullscreen (`Ja()`), not a thin client, main conversation focused;
- `Jdn(columns)`: `diffSidebarOpen !== false` in `~/.claude.json`, cwd is a git
  repo, and `columns ≥ 110` when `diffSidebarOpen === true` (the person opened
  it before) else `columns ≥ 144`.

The timer prefetches the diff, marks the open as `auto_open` for telemetry
(`tengu_repl_diff_panel_shown` with a width bucket `under_110 / 110_to_143 /
144_to_199 / 200_plus`) and sets `replTab := "diff"`. Auto-open does *not*
write `diffSidebarOpen`; only a manual toggle does.

### 2.5 What the panel shows

Three base modes, cycled with `ctrl+x b` while the panel is focused and
persisted as `diffSidebarBaseMode` (session settings):

| mode | meaning |
|---|---|
| `session` (default) | files this session touched (`fileHistory`), diffed against their pre-session state — the "No changes this session" screen when empty |
| `uncommitted` | `git diff` of the working tree |
| `branch` | the branch against its base |

Data comes from `git` in-process (`repl_diff_read` telemetry;
`git_diff_failed` / `git_hunks_failed` / `git_diff_threw` on errors), per-file
hunks capped at 400 changed lines ("… diff truncated (exceeded 400 line
limit)"), binaries and large files listed but not expanded. A thin client asks
the remote engine instead (`sendControlRequest({ subtype: "get_workspace_diff" })`),
falling back to "per-turn changes only" with a notice when the channel is
missing, times out, or the remote build is older.

Other keys: `ctrl+up` / `ctrl+down` (also `meta+…`) scroll the file list from
anywhere; `app:toggleDiffNoiseFilter` (hide tests) and `app:toggleDiffPreSession`
exist but have no default binding.

## 3. When is Claude Code "fullscreen"?

`Ja()` (chunk with `tengu_pewter_brook`), first match wins:

1. `CLAUDE_CODE_SESSION_KIND=bg` → fullscreen
2. screen-reader mode → classic
3. `CLAUDE_CODE_NO_FLICKER=0` or `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN` → classic
4. `CLAUDE_CODE_NO_FLICKER=1` → fullscreen
5. a previous fullscreen crash this version (`fullscreenAutoDisabled`) → classic
6. tmux control mode (`tmux -CC`, iTerm2 integration) → classic (logged)
7. Windows over SSH → classic (logged)
8. **`tui` in settings.json** (`"fullscreen"` / `"default"`; `/tui` writes it) or
   `CLAUDE_CODE_TUI_TRIAL=fullscreen` → as set
9. fresh install (first three starts) → fullscreen
10. GrowthBook `tengu_amber_creek` → fullscreen
11. GrowthBook `tengu_pewter_brook` → its value

On this machine (2026-09-14) `tui: "fullscreen"` is set in
`~/.claude/settings.json`, so step 8 decides; `tengu_pewter_brook` is `false`
in the cache and would otherwise have chosen the classic renderer.

## 4. Layout arithmetic

The fullscreen shell (`BN`) takes `sidebar` and `sidebarWidth`; the transcript
gets `max(1, columns − sidebarWidth)`. The REPL computes:

```
diffWidth = replTab === "diff" && canShowDiff ? p3(columns) : 0
canShowDiff = QZ() && fullscreen && !thinClient && mainFocused && columns ≥ 110 && gitRepo
p3(columns) = min(floor(columns × 0.45), 90, columns − 70)     // 162 cols → 72

dockWidth = fullscreen && mainFocused && diffWidth === 0 ? dockColumns(columns) : 0
dockColumns(columns) =
  columns < 110                → 0
  pluginPanes.dockColumns set  → min(columns − 70, max(24, dockColumns))
  else                         → p3(columns)                     // same default as /diff

sidebarWidth = diffWidth + (a plugin pane is open ? dockWidth : 0)
```

Constants: `DOCK_MIN_COLUMNS = 24`, `DOCK_GRIP_COLUMNS = 1` (the drag handle),
`DOCK_KEY_COLUMNS = 4` (keyboard resize step), `BORDER_COLUMNS = 4`,
`TAB_MIN_COLUMNS = 12` (tab strip when several panes are open). The 110 / 144
thresholds are shared constants (`x8`, `dBe` in chunk `x05j5gpv`).

## 5. Plugin panes (function hooks)

### 5.1 API surface

A plugin's hooks module (`hooks/hooks.json` → `"modules": ["./pane.tsx"]`,
`export const register = (on, options) => …`) gets `$` with, among others:

- `$.ui.open({ id, title?, focus? })` — `id` is 1–64 of `[A-Za-z0-9_-]`, at most
  8 panes per plugin, `focus` must be `true` or absent; opening an open id
  retitles it.
- `$.ui.close({ id })` — raises `ui.close` with `origin` `plugin` / `person` /
  `unload`.
- `on("ui.render", { component: "Pane" }, ($, e, next) => tree)` — `e.requestId`
  is the pane id; `e.props` = `{ title, isFocused, bodyColumns, placement:
  "dock" | "inline", scroll: { offset, bodyRows } }`; `e.viewport` =
  `{ columns, rows }` of the whole screen.
- `$.ui.invalidate("ui.render")` to redraw; `$.ui.toast`, `$.ui.status`,
  `$.ui.log` for messages.

`plugin/.claude/types/claude-code.d.ts` (written by `/plugin-types`) is the
full contract; 2.1.269 and 2.1.270 differ only in the header line.

### 5.2 The pane store

`panes: { open[], unplaced[], asked[], shownId, focusedId, focusRequest,
placements, closing[] }`. On `$.ui.open` the engine decides whether the pane is
*placed* now (`isPlacedAtOpen`):

```
placed = askedByPerson || columns === undefined || columns ≥ (hasAsked ? 110 : 144)
askedByPerson = the open ran inside a command.run hook whose origin is the person
```

So a pane opened from the plugin's own slash command (`/cctop-pane`) is placed
at once at any width; one opened from `skill.prompt`, a timer or `session.start`
waits in `unplaced` below 144 columns (110 once the plugin has asked before)
and is offered later. `placements` counts the surfaces currently able to seat a
pane.

### 5.3 Dock vs inline

- `PaneDock` (docked, `placement: "dock"`): rendered inside the same `sidebar`
  slot as the diff panel, with a 1-column drag grip; only when
  `dockWidth > 0` (§4).
- `InlinePanes` (`placement: "inline"`): a band above the prompt, used when
  the renderer is classic **or** fullscreen below 110 columns. Rows come from
  `pluginPanes.inlineRows` or a share of the screen.

Keys while a pane is focused (`ctrl+x tab` from the chat focuses / cycles
panes): `up/down/pageup/pagedown/home/end` scroll, `ctrl+x left|up` grow,
`ctrl+x right|down` shrink, `ctrl+x x` close, `tab`/`shift+tab` move between
buttons and fields, `enter` press, `esc` leave. A resize is persisted to
`pluginPanes: { dockColumns, inlineRows }` in `~/.claude.json`
(`plugin_function_hooks_pane_resize`).

### 5.4 Mutual exclusion with `/diff`

From §4: `dockWidth` is computed only when `diffWidth === 0`, and
`InlinePanes` hides itself whenever the renderer is fullscreen and
`dockColumns(columns) > 0`. Put together, in a fullscreen session at ≥ 110
columns with the diff panel open:

- an open plugin pane is **neither docked nor inline** — it is invisible;
- no `ui.render` for it is requested, so the plugin only notices by the
  absence of renders;
- `/diff` (hide) brings the plugin pane back into the dock at once, and
  `/diff` (show) hides it again.

The engine emits no event for this. There is also no way for a plugin to read
`replTab`; the only observable is whether renders arrive.

### 5.5 Hotkeys: only the band above the prompt

`Button.hotkey` (one digit or lowercase letter) is honoured by exactly one
site, the `AbovePrompt` band (`XRt`): it collects the hotkeys of the Buttons
it drew (`hotkeysOf`) and runs a digit-press hook (`mh`) on the composer —
when the input becomes that single digit it waits 400 ms, then clears the
input and raises `ui.press` for the Button; any further keystroke inside the
400 ms cancels it. While the band is focused (`ctrl+x tab`), digits and
letters press on keydown. A collapsed band (`ctrl+x ctrl+a`) draws no
Buttons, so its hotkeys are off.

The docked `Pane` site (`pce`) never reads `hotkey`: its bindings are
scroll, `tab`/`shift+tab` between focusables, `enter` press, `ctrl+x`
arrows resize, `ctrl+x x` close. A plugin that wants keyboard switching
would have to draw its Buttons in the band too (`on("ui.render", {
component: "AbovePrompt" }, …)`) with the same `onPress`. cctop did until
0.3.x and stopped: the band cost the transcript a row for six digits, so
the view bar now lives in the pane alone (click, or `ctrl+x tab` then
`tab` / `enter`).

### 5.6 Drawing: what the terminal honours

Plugin trees are checked before they draw (`kL`): Box props `borderStyle`
(one of Ink's `single double round bold singleDouble doubleSingle classic
arrow` plus `dashed quote`), `borderColor`, `borderDimColor`,
`backgroundColor`, `display`, and the flex/size/margin/padding set; Text and
Button props `color`, `backgroundColor`, `dimColor`, `bold`, `italic`,
`underline`, `strikethrough`, `inverse`. A colour is "a theme key, a name, or
hex" — theme keys (`text`, `inactive`, `subtle`, `suggestion`, `success`,
`warning`, `error`, `claude`, `rate_limit_fill`, `diffAdded`, …) resolve to
the person's Claude Code theme, which is how the cctop pane follows light and
dark. Limits: 2000 nodes and depth 32 per tree, 10 000 characters per text,
100 000 per tree. A Text may nest Texts for inline styling, so a whole row of
segments is one truncating Text.

### 5.7 Gates

- Function hooks load when `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS` is set (any
  value the env parser reads as true) **or** GrowthBook `tengu_plugin_hooks_modules`
  is on (default off). `claude --debug` logs the source
  (`overridden by the CLAUDE_CODE_ENABLE_FUNCTION_HOOKS environment variable`,
  `from GrowthBook …`, `from the default …`).
- Also off under `--bare`, safe mode, `disableAllHooks`, or when the hooks
  worker (one Bun worker for all plugins) crashed three times in a session.
- The diff panel's kill switch is `tengu_jazzy_ripple` (absent → panel enabled).
- `CLAUDE_CODE_FORCE_SESSION_PERSISTENCE` has nothing to do with panels: it
  only forces transcript writing in a `CLAUDE_CODE_CHILD_SESSION` so
  `--resume` can find it.

## 6. Config keys touched (`~/.claude.json` unless noted)

| key | written by | read for |
|---|---|---|
| `diffSidebarOpen` | manual `/diff` toggle | auto-open eligibility and its width threshold |
| `diffSidebarBaseMode` (session settings) | `ctrl+x b` | which base the panel diffs against |
| `pluginPanes.dockColumns` / `.inlineRows` | pane resize keys / drag | plugin pane size |
| `tui` (`~/.claude/settings.json`) | `/tui` | fullscreen vs classic |
| `fullscreenAutoDisabled` | a fullscreen crash | classic until the next version |

## 7. What this means for cctop

The pane only ever docks when *all* of these hold, and each has a distinct
remedy the plugin and the skill now spell out (`cctop pane status`,
`/cctop-pane`'s reply):

1. Claude Code ≥ 2.1.269 with function hooks on —
   `"CLAUDE_CODE_ENABLE_FUNCTION_HOOKS": "1"` in the `env` block of
   `~/.claude/settings.json`, then restart Claude Code.
2. The installed cctop plugin ships the hooks module (≥ 0.2.0; `claude plugin
   update cctop`, or `claude --plugin-dir <checkout>/plugin` while developing).
3. Fullscreen renderer (`/tui fullscreen`, persisted as `tui` in settings).
4. Terminal ≥ 110 columns (below that the pane is drawn inline above the prompt).
5. The diff panel closed (`/diff` toggles it; it takes the dock otherwise).

The engine tells the plugin about 3 and 4 through `e.props.placement` and
`e.viewport.columns` on the first `ui.render`; it never tells it about 5, so
the plugin infers "hidden" from a missing render after `$.ui.open`.
