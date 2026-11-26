# cctop mod pane: design spec

**Date:** 2026-11-26 · **Status:** draft for approval
**Companion PRD:** `tasks/prd-cctop-mod-pane.md` (PRD 9). The PRD says what and why; this spec says how.
**Design canvas:** https://claude.ai/artifact/13BthbakcasCgCQk8Zed7B. Sources in `tasks/design-mod-pane/*.dc.html`: POCs 1–8, where POC 6, 7 and 8 are the target overview.
**Builds on:** PRD 0 (#24, contract 2.1.332), PRD 1 (#25), 2 (#26), 3 (#27), 4 (#28), 5 (#29), 6 (#30), 7 (#31).

---

## 1. Intent

The cctop pane inside Claude Code becomes readable and actionable. Advice gets room, is written in plain words, and comes with a ready fix. Numbers become charts with a verdict ("Good / Watch / Act" plus one Do or Don't). Charts are smooth pictures where the terminal can show them (Ghostty, kitty, iTerm2), and cell charts everywhere else (Terminal.app, VS Code, tmux without passthrough). The TUI shows the same words and the same cell charts.

The person who asked put it this way across the review rounds:
- "a user won't understand A38"
- "make here something where a user can click on… the prompt to fix it"
- "which learning can I take out of this chart? give always some comment"
- "make sure that press to open really works"
- "use the proper font settings in the real version"

## 2. Goals, non-goals, anti-goals

**Goals**
- G1. The coach card shows the full advice text, a plain-language name, evidence, and one clickable fix prompt that goes into the prompt box.
- G2. Every queue row and every light opens in place; context opens into the breakdown and residency.
- G3. The overview (POC 6) shows context, spend, cost per turn, prompt cache and tool calls, each with a verdict, in two columns at ≥ 100 pane columns and stacked below that.
- G4. Charts render as pictures (the `Image` element) where the terminal supports them, and as cells (the `Raster` element) elsewhere. Both versions come from one renderer.
- G5. An act dialog (POC 5) for every nudge: what changes, the exact text or rule, and buttons per action kind.
- G6. Small rows placed in the transcript where something happened (POC 4): a NOW nudge at the turn's end, and a located cache miss where it occurred.
- G7. Everything PRDs 1–7 add is visible where it belongs (§7).

**Non-goals (v1)**
- The band above the prompt, the cell beside the prompt and the status-line hook (POC 4 extras the review did not keep).
- Pictures in the TUI: the TUI gets the cell version only.
- Desktop-app `Svg`, and themes or backdrops.
- Jumping the transcript to a specific tool call from an evidence row: the API has no such call. Evidence stays text with its turn and call number.

**Anti-goals** (fail even if it works)
- Anything sent on the person's behalf, or any action on the session. Fill puts text in the prompt box and the person presses Enter; stopping a turn stays the person's Esc.
- Internal shorthand on screen: no rule ids as labels, no "t11".
- Text inside chart pictures. It wouldn't match the terminal font.
- The pane and the TUI disagreeing on a word or a number.
- A model writing any verdict or advice: every word comes from fixed rules over structural facts.

## 3. Constraints (from the 2.1.332 mod API, read 2026-11-26)

| Fact | Consequence |
|---|---|
| `$.wasm.compile(source)` compiles a `.wasm` named by a string literal (≤ 64 MiB) and works with wasm-bindgen glue. Nothing compiles synchronously. No memory cap locally, so set `maximum`. Shared memory is refused. | The renderer is a Rust crate built for `wasm32-unknown-unknown`, compiled once at `session.start`, with a fixed memory maximum. |
| `Image { key, source, columns, rows, alt }`. `source` is a base64 PNG (≤ 2 MiB decoded), base64 RGBA (≤ 2048×2048), a `{file}` or a `{shm}`. The terminal draws it via kitty graphics (kitty, Ghostty), iTerm2 inline images (3.5+, fullscreen), or tmux passthrough (bytes only); otherwise it draws `alt`. | The pane sends **base64 PNG bytes** (works under tmux too). Each chart PNG must stay under 2 MiB: the renderer encodes at 2× an assumed 8×16 px cell. |
| `Raster { key, columns, rows, cells }`: base64 of `[codePoint, fg, bg]` u32 triplets per cell, colours `0x00RRGGBB`. | The cell version uses ▀ with fg = upper pixel and bg = lower pixel: two pixels per row. |
| `$.ui.blit({ requestId, key, cells \| source })` repaints a mounted Raster or swaps an Image without a redraw. It is **denied when the Image is drawing its alt** ("no placeholder images"). | Image support is detected at runtime by a probe blit (§5.3), with environment detection as the first guess. |
| Text: the terminal has one font and one size (`size` is ignored) and draws `semibold`/`bold` as bold. | Typography is colour plus bold only. Verdict boxes are square background fills. |
| Hover: keyed `Box` hover styles; no code runs on hover. `ui.pointer` covers the backdrop only. | Per-turn readouts are precomputed and revealed by keyed hover areas laid over each chart's columns. Clicking the turn number is the fallback where pointer movement isn't reported. |
| `$.prompt.fill`, `$.ui.copy`, `$.ui.open({ focus, closeOnEscape, holdToasts, rows })`, `$.ui.place({ id })` plus `ui.render` `Placed`, `$.ui.view({ agentId })`, `$.ui.notify`. `$.turn.abort` exists but is not used (§5.5). | Fill, copy, act dialog, placed rows, jump-to-agent and notify all exist. |
| The validator follows `$` only into top-level functions of `pane.tsx`; `$.env.get("X")` must be spelled literally; no local named `h`/`Fragment`. | New calls live in top-level functions or behind the existing facade pattern (`pollerEngine($)`). |
| `plugin/` ships exactly what the marketplace ships; today no build output lives there. | `plugin/hooks/charts.wasm` becomes the one committed build artefact: reproducible via `make wasm`, with a test that fails when it's stale (§6.4). CLAUDE.md gets updated. |

## 4. Approaches considered

| | Approach | Trade-off | Decision |
|---|---|---|---|
| A | The `cctop` binary renders chart PNGs; the pane shows them | One renderer in the binary; the pane stays thin. Needs a process call per chart. | Not chosen |
| **B** | **The pane renders charts itself, in WebAssembly** | Charts redraw in the pane with no process call. The wasm is a shipped artefact and leans on an early-access noun (`$.wasm`). | **Chosen by the user, 2026-11-26** |
| C | No pictures in the terminal; cells everywhere | Cheapest, but loses POC 7's look | Not chosen |

**How B avoids duplicate chart logic.** The renderer is one Rust crate, `crates/cctop-charts`. It compiles to wasm32 for the pane and links natively into the `cctop` binary, where the TUI uses its cell output. Both versions are therefore drawn by the same code from the same chart specs.

## 5. Design

### 5.1 One object, two renderings

```
transcript / hooks / status / otel  →  ui::State  →  overview.rs (NEW) / coach.rs / dashboard.rs
                                                        │  cctop query overview|coach   (JSON: words + chart specs)
                                   ┌────────────────────┴────────────────────┐
                        TUI (ratatui)                                  pane (mod)
            cctop_charts::render_cells(spec, w, h)          charts.wasm: render_png(spec, w, h, theme)
                                                                         or render_cells(spec, w, h)
```

- **`src/overview.rs` (new)** builds the overview object: one entry per chart, each with `{id, title, figure, verdict{level, text, todo}, spec, readouts[], legend[]}`. `spec` is data (series, markers, forecast, thresholds, colours by role), never geometry.
- **`src/verdict.rs` (new)** holds the verdict rules, one function per chart. Thresholds are constants in `harness_facts.rs`-style tables; every text is fixed with holes for numbers. Each verdict gets a registry id in `metrics/registry.rs`.
- **`coach.rs`** gains `name` (plain language) per rule, plus `body`, `fix_prompt`, `actions[]` and `foot`, all full length. The 52-cell cut stays only for the one-line forms (`l0/l1/l2`).
- **`crates/cctop-charts`** (new workspace member; `std`, because the `png` crate needs it):
  - `render_cells(spec, cols, rows) -> Vec<Cell>`
  - `render_rgba(spec, px_w, px_h, theme) -> Vec<u8>`, using tiny-skia for antialiased paths
  - `encode_png(rgba) -> Vec<u8>` with the `png` crate
  - wasm exports `alloc`, `free` and `render(kind, spec_json_ptr, len, cols, rows, scale) -> ptr`, returning a length-prefixed buffer
  - chart kinds: `area_forecast` (context), `cumulative_forecast` (spend), `bars` (cost per turn), `pills` (cache), `heat` (tool calls), `stacked_bar` (composition), `progress` (cache countdown)

### 5.2 Layout (pane)

- **Overview** (POC 6/7):
  - Header row: view bar plus badges (PRD 2 workspace facts).
  - Context, full width: title, verdict, composition bar, area chart with forecast and auto-compact line, labels.
  - Two columns: Spend | Cost per turn.
  - Two columns: Prompt cache (countdown, per-turn cells, "How to avoid each rebuild") | Tool calls per turn (heat map).
  - Nudge pinned at the bottom, footer explaining $.
  - Below 100 body columns each two-column row stacks. Below 60, charts drop to Raster one row high (sparkline form).
- **Coach** (POC 1): the NOW card on top (plain name, headline, body, evidence, fix-prompt card, buttons per action kind), then "Up next" rows that open in place, then the lights that open in place (context → breakdown + residency; cache, 5h, rework → plain detail), then the footer.
- **Act dialog** (POC 5): pressing a nudge or `a` opens `$.ui.open({ id: 'cctop-act', focus: true, closeOnEscape: true, holdToasts: true, rows })` with the list on the left and the detail on the right. Buttons by action kind: key → the step "press Esc to stop the turn" shown first, then fill the draft; prompt and slash → fill the text; allow rule → copy the rule, open settings.json, fill `/permissions`; setting → copy the setting line, open settings.json; read → got it, mute. cctop never edits a settings file.
- **Placed rows** (POC 4): `$.ui.place({ id: 'nudge-<fireId>' })` at the turn end for NOW nudges, drawn as a bordered card with buttons. `$.ui.place({ id: 'miss-<turn>' })` for a located cache miss, drawn as one dim row. Display only.
- **Long sessions:** per-turn charts (cost per turn, cache cells, tool heat map) show the last N turns that fit at 2 cells per turn, labelled "last N turns", with the whole-session figure in the header. Context and spend use the whole session, with points bucketed to the chart's width. Readouts exist only for the turns shown, so verdict wording names turns by number.
- **Typography:** the terminal font, colour and bold only. Charts are placed on whole cells and carry no text; labels are cells beside them. The README documents recommended settings: Ghostty `font-family = JetBrains Mono`, `font-size = 13`, `adjust-cell-height = 23%`; Terminal.app SF Mono Regular 13, spacing 1, antialias on, bright-bold off.

### 5.3 Picture or cells

1. **First guess from the environment:** `TERM_PROGRAM`, `KITTY_WINDOW_ID`, `TMUX` and `ITERM_SESSION_ID`, read with literal `$.env.get` calls. ghostty, kitty, iTerm.app or WezTerm → try pictures; Apple_Terminal or vscode → cells.
2. **Probe:** the first time a chart is drawn as an `Image`, the pane blits the same source once. A `{ deny }` whose reason names the alt marks the session "cells".
3. **Override:** `$.env.get("CCTOP_PANE_CHARTS")` = `image` | `cells` | `auto`, plus `~/.cctop/config.toml` `[pane] charts = …` read through `cctop query config`.
4. The decision is stored in module state and in the marker (`charts: "image" | "cells"`); `cctop pane status` prints it.

### 5.4 Data flow and cadence

- The poller already runs `cctop query <verb>`. New verb: `overview`. `coach` gains its fields.
- The pane hashes each chart's `spec`. Only changed specs are re-rendered: one wasm call each, then a `$.ui.blit` of the keyed Image or Raster. A width change triggers a full redraw.
- Render budget: under 30 ms per chart in wasm, measured in tests on the fixture specs. Charts render off the render hook (in the poll handler), never inside `ui.render`.

### 5.5 Actions (read-only kept)

| Action | Call | Note |
|---|---|---|
| Fill a fix prompt or command | `$.prompt.fill({ text })` | The existing `coachActions.fill`; never sends |
| Stop the turn | none: the card says "press Esc to stop the turn" | Decided 2026-11-26: no `$.turn.abort`. The read-only rule and `prd-cctop-pane.md`'s forbidden list stay as they are, and Esc leaves the normal interruption marker cctop counts. |
| Copy a rule | `$.ui.copy({ text })` | Puts the text on the clipboard of the surface; result says `isCopied` or why not |
| Open settings.json | `$.process.run(['open', path])` on macOS / `xdg-open` | Opens the file in the person's editor; cctop never writes it |
| Snooze / mute | `cctop query coach --snooze <rule>` | Existing |
| Jump to agent | `$.ui.view({ agentId })` | PRD 6 |
| Remind at reset | `$.ui.notify(…, { at, untilBack: true })` | PRD 6, opt-in |

### 5.6 Words: names, fix prompts, evidence, verdict thresholds

- **Evidence and fix prompts never quote tool output.** As today, A38 keeps command shapes and error classes. Each class maps to a fixed label and a fixed fix sentence. `gh` "no pull requests found" → class `no_pr` → "there is no PR for this branch yet" → "open it with gh pr create --fill". git non-fast-forward → `rejected_non_ff` → "the remote has commits you don't" → "run git pull --rebase, then push". The no-prose rule in CLAUDE.md is unchanged.
- **Copy and thresholds:** the builder drafts every rule's `name`, `body`, `fix_prompt`, `actions[]` and `foot`, plus the five charts' Good/Watch/Act thresholds and texts. The drafts are grounded in `tasks/research-coach/`, the coach PRD §5 and the existing rule headlines. They go in two reviewable tables, `docs/coach-copy.md` and `docs/verdicts.md`, generated from the code like `docs/metrics.md`. The user reviews both before the release (US-013).

### 5.7 The TUI

- **Overview:** dashboard body 1 is redrawn from the overview object, with `render_cells` charts and wrapped verdicts, replacing today's overview body. The other bodies are unchanged.
- **Coach view:** shows `name`, `body` and `fix_prompt` wrapped. The fix prompt is delivered by the TUI's existing `a` flow (clipboard, or socket send as today, `docs/socket.md`).
- **Pane-only:** the act dialog, stop and placed rows. The TUI can't drive the prompt box.

## 6. Build, test, ship

### 6.1 Crate and wasm
- The repository becomes a Cargo workspace (`members = [".", "crates/cctop-charts"]`).
- `make wasm` runs `cargo build -p cctop-charts --target wasm32-unknown-unknown --release` and `wasm-opt -Oz` when available, then copies the result to `plugin/hooks/charts.wasm`. Size target: under 600 KB.
- Memory maximum: 64 MiB, set in the module.

### 6.2 Tests
- **Rust:** golden tests per chart kind. `render_cells` output is compared as text snapshots (insta); `render_rgba` is compared by hash at 1× on fixture specs. Verdict rules get one unit test per level per chart. `overview.rs` is snapshotted on fixtures B and C (C from PRD 1).
- **Pane:** `tests/pane/` loads the built wasm in node (`WebAssembly.compile` of the file) and asserts PNG magic bytes and size under 2 MiB per chart. It also checks Raster cells decode, the picture-or-cells decision table, hover areas present per column, and act-dialog actions per kind.
- **Row identity:** the TUI's cell charts and the pane's Raster cells for fixture B are identical (same `render_cells`).

### 6.3 Live checks (`docs/verification/pane.md`, `Result: pending`)
- 45: Ghostty shows pictures.
- 46: Terminal.app shows cells.
- 47: tmux with and without passthrough.
- 48: the fill → Enter flow.
- 49: the Esc hint appears first on a key-kind card, and Esc then stops the turn with its marker.
- 50: the act dialog keys (Tab, arrows, Esc).
- 51: placed rows survive scrolling but not `--resume`.
- 52: the hover readouts.

### 6.4 The committed artefact
- `plugin/hooks/charts.wasm` is committed.
- Freshness is checked on **inputs, not bytes**: `plugin/hooks/charts.wasm.inputs` holds the SHA-256 over the crate's sources, `Cargo.lock` and the rustc version that built it. `tests/wasm_fresh.rs` recomputes the source and lock part and fails with "run make wasm" when it differs. No byte-identical build is required, so no toolchain pin and no change to CI's clippy.
- CI installs the wasm32 target and runs `make wasm` only to check that the build succeeds.
- CLAUDE.md's "no build output under plugin/" gains the one exception.

## 7. What PRDs 1–7 add to this pane

| PRD | Shows up as |
|---|---|
| 0 | Hard prerequisite: `$.wasm`, `ui.place` and `ui.view` are 2.1.332 nouns; the contract must be 2.1.332 |
| 1 | Cache chart: each red cell's readout gives the located cause. "How to avoid each rebuild" maps causes to habits (`tools_changed` → connect MCP early; `PrefixRewritten` → steer forward; `ParamsChanged(effort)` → set effort at start or after /clear; `Expired` → reply within the TTL). Also the A01 text and the placed miss row. |
| 2 | Header badges: repo, worktree, `+N dirs`, output style, agent, remote. The spend limit in USD on the limits light. A `>200k` marker on cost per turn. The shim's tool/system deltas as the fallback cause. |
| 3 | The composition bar and the context light's detail come from the `/context` capture, labelled "per Claude Code, Ns ago"; the context verdict reads the categories (MCP share, prefix share) |
| 4 | Thinking drops as markers on the cache strip, with readout "thinking dropped: N blocks". `budget_usd` as a budget line on the spend chart, with verdict "halts in ~N min". Lineage as "from <id8> · clear" in the header and a chain total in spend's readout. |
| 5 | The elicitation wait as a NOW card ("MCP <server> is asking you"). A settings change marks the next cache miss ("after settings change"). A skill's shell commands as their own heat-map row. TeammateIdle in the agents view. The amber hooks badge when events are missing. |
| 6 | `session.info` transcript path for the poller and data-rule badges. Agent limits in the agents view header. `ui.view` on agent rows and in the act dialog. Opt-in `ui.notify` for NOW and resets. `session.stop` ends the running state at once. |
| 7 | Permission waits split by who decided, in the coach's permission card and the A09 count. Retries in cost-per-turn readouts ("2 retries, +14 s"). Hook overhead as a verdict on the tool-calls chart when it exceeds 500 ms per turn. |

Each row degrades cleanly when its PRD has not landed: the field is absent and the view omits it. The order is free except for PRD 0.

## 8. Left to the builder
- Exact palette tokens per theme: reuse `src/theme.rs` roles; the pane's palette comes from the query, as today.
- tiny-skia stroke widths and curve smoothing. Rule: never smooth across a step, and never invent peaks between data points.
- Readout wording beyond the examples, within the anti-goals.
- How hover areas are laid out when columns are narrower than 2 cells: fall back to one area per two turns.

## 9. Calls made without the user (check these)
1. The renderer is one Rust crate compiled to wasm and also linked natively, so B carries no duplicate chart logic.
2. Base64 PNG over `{file}`, so tmux passthrough works and no files are written.
3. The probe blit as the source of truth for image support.
4. The TUI gets cell charts only.
5. Band, beside-prompt and status line left out of v1.
6. A committed `charts.wasm` with a freshness test.
7. (Decided with the user: no stop button, an Esc hint instead.)
8. No tool output is quoted: fixed labels per error class (§5.6).
9. The builder drafts copy and thresholds; the user reviews two generated tables (§5.6).
10. Long sessions: per-turn charts show the last N turns that fit (§5.2).
11. The TUI's overview body is replaced; the act dialog, stop and placed rows are pane-only (§5.7).
12. Freshness is checked by an inputs hash, not a byte-identical wasm (§6.4).
