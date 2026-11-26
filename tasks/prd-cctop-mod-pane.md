# PRD 9: the mod pane — readable advice, charts with a verdict, pictures where the terminal can draw them

**Status:** v1.0 · 2026-11-26 — draft for approval.
**Target:** cctop v0.11.x / plugin 0.12.x, contract Claude Code 2.1.332.
**Spec (how):** `docs/superpowers/specs/2026-11-26-cctop-mod-pane-design.md`.
**Design:** https://claude.ai/artifact/13BthbakcasCgCQk8Zed7B, sources in `tasks/design-mod-pane/`. Target screens: POC 1 (coach), POC 5 (act dialog), POC 4 (placed rows only), POC 6 (overview), POC 7 (Ghostty, pictures), POC 8 (Terminal.app, cells).
**Depends on:** PRD 0 (#24), a hard dependency. Integrates PRDs 1–7 (#25–#31) where they surface; each degrades when its PRD has not landed (§7 of the spec).

> Decisions taken with the user on 2026-11-26, in the design review (canvas comments, 20 threads):
> 1. **Plain language everywhere.** Rule ids appear only as a dim detail; turns are "turn 3", never "t3"; the $ figure is explained as "what this session would cost at API prices".
> 2. **The fix is a clickable prompt.** One click puts it in the prompt box; nothing is sent.
> 3. **Rows and lights really open in place.** Context opens into the breakdown and residency.
> 4. **Every chart gives a verdict:** Good / Watch / Act, one sentence, one Do or Don't.
> 5. **Overview:**
>    - Keep: cost per turn and the cache strip (POC 2), the smooth context chart fitted into the POC 2 layout, spend with forecast, and the composition bar with an interpretation.
>    - Drop: POC 2's cell context chart and the 5h-window chart.
>    - Cache misses come with how to prevent each.
> 6. **Keep** the act dialog (POC 5) and the placed transcript rows (POC 4).
> 7. **Both versions:** graphic (pictures) and text (cells), with proper font settings documented.
> 8. **Approach B:** the pane renders charts itself in WebAssembly.
> 9. **Take PRDs 1–7 into account.**
> 10. **No stop button:** a key-kind card shows "press Esc to stop the turn" as its first step. `$.turn.abort` is not used, and the read-only rule is unchanged.

---

## 1. Introduction

Today the pane mirrors the TUI. Every coach string is cut at 52 cells, rule ids stand in for names, and the only chart elements are text bars. The 2.1.332 mod API can draw pictures (`Image`), cell graphics (`Raster`), focused dialogs, rows placed in the transcript, and it runs WebAssembly. This PRD uses them to make the pane readable and actionable, without breaking cctop's two rules: read-only, and one state for every surface.

## 2. Goals

- A person can read every piece of advice in full and act on it with one click, and nothing is sent without them pressing Enter.
- Every chart in the overview says whether things are fine and what to do.
- Smooth charts in Ghostty, kitty and iTerm2; legible cell charts in Terminal.app, VS Code and plain tmux. The same words and numbers everywhere, the TUI included.
- What PRDs 1–7 collect is visible in the pane.

## 3. User stories

### US-001: Plain names and full advice in the coach object
**Description:** As a person reading the coach, I want each piece of advice in plain words with its full explanation and a ready fix, so that I understand it without knowing cctop's rule ids.

**Acceptance criteria:**
- [ ] Each of the 36 rules in `advisor/rules/` declares `name()`: a plain-language name of at most 32 characters, e.g. A38 "Claude is stuck retrying", A13 "usage limit", A09 "permissions", A01 "cache". A test asserts every rule has one and none contains a rule id.
- [ ] The coach object (`coach.rs`, `cctop query coach`) carries per nudge: `name`, `headline`, `body`, `evidence[]`, `fix_prompt` (a template with holes filled from structural facts only, or absent), `actions[]` (kind, label, payload), `foot` (when it retires), all uncut. `line1`/`line2` and the `l0`/`l1`/`l2` one-line forms keep the 52-cell cut.
- [ ] A test feeds a session with a distinctive prompt text and asserts that no string from the person's or the model's text appears in any field (the no-prose rule).
- [ ] Evidence and fix prompts use fixed labels per error class (`no_pr`, `rejected_non_ff`, …), never quoted tool output; a test asserts a fixture's error text does not appear.
- [ ] Action kinds: prompt and slash → fill; allow rule → copy, open settings.json, fill `/permissions`; setting → copy the line, open settings.json; key → stop and fill the draft; advice → got it.
- [ ] The TUI coach view shows `name`, `body` and `fix_prompt` wrapped at its width (snapshot at 56 and 122 columns), and its `a` flow delivers the fix prompt.
- [ ] `scripts/pane-fixtures.sh` is regenerated; `make check` and `npm test` are green.

### US-002: Verdicts and the overview object
**Description:** As a person looking at a chart, I want one line telling me whether it's fine and what to do, so that I learn something from every chart.

**Acceptance criteria:**
- [ ] `src/verdict.rs` has one function per chart (context, spend, cost per turn, cache, tool calls). Each returns `{level: good|watch|act, text, todo?}` from fixed thresholds kept in one table. One unit test per level per chart.
- [ ] `src/overview.rs` builds the overview object: per chart `{id, title, figure, verdict, spec, readouts[], legend[]}`. `spec` is series, markers, forecast and thresholds with role colours, never geometry. The cache chart carries `prevent[]`: one habit per located cause (PRD 1), mapping `tools_changed`, `PrefixRewritten`, `ParamsChanged(effort|model)` and `Expired` to fixed sentences.
- [ ] `cctop query overview --session <fixture>` prints it; `mcp.rs` exposes it; each verdict has a registry id in `metrics/registry.rs`; `docs/metrics.md` and the README block are regenerated.
- [ ] Insta snapshots of the overview JSON for fixtures B and C.

### US-003: The chart renderer crate
**Description:** As a developer, I want one renderer for both versions, so that pictures and cells can never disagree.

**Acceptance criteria:**
- [ ] The repository becomes a Cargo workspace. `crates/cctop-charts` (`std`; the `png` crate needs it) implements the chart kinds `area_forecast`, `cumulative_forecast`, `bars`, `pills`, `heat`, `stacked_bar` and `progress`, each with:
  - `render_cells(spec, cols, rows)`: half-block cells, fg above and bg below
  - `render_rgba(spec, px_w, px_h, theme)`: tiny-skia, antialiased
  - `encode_png`
- [ ] No text is drawn inside any picture (a test scans the spec for label fields being ignored by the renderer).
- [ ] Golden tests: text snapshots of `render_cells` per kind; SHA-256 of `render_rgba` at 1× per kind on fixture specs.
- [ ] A PNG for the widest chart (103×8 cells at 2× of 8×16 px) is under 2 MiB (test).
- [ ] `cargo test -p cctop-charts` and `make check` green.

### US-004: The wasm build, shipped in the plugin
**Description:** As the maintainer, I want `charts.wasm` built reproducibly and checked for freshness, so that the shipped plugin always matches the source.

**Acceptance criteria:**
- [ ] `make wasm` builds `cctop-charts` for `wasm32-unknown-unknown` (release, `wasm-opt -Oz` when present) into `plugin/hooks/charts.wasm`. Size under 600 KB. The memory `maximum` is set to 64 MiB.
- [ ] Exports: `alloc`, `free`, `render(kind, spec_ptr, spec_len, cols, rows, scale, mode) -> ptr` (mode png | cells) returning a length-prefixed buffer.
- [ ] `plugin/hooks/charts.wasm.inputs` records the SHA-256 of the crate sources, `Cargo.lock` and the rustc version. `tests/wasm_fresh.rs` recomputes the sources and lock part and fails with "run make wasm" when it differs. No byte-identical build and no toolchain pin. CI installs the wasm32 target and checks that `make wasm` succeeds.
- [ ] CLAUDE.md's "no build output under plugin/" names `charts.wasm` as the one exception, and the release checklist gains `make wasm`.
- [ ] `claude plugin validate --strict ./plugin` lists the wasm with its size and exports, without error.

### US-005: The overview in the TUI (text version)
**Description:** As a TUI user, I want the same overview, verdicts and cell charts as the pane, so that both surfaces tell me the same thing.

**Acceptance criteria:**
- [ ] The TUI's overview (dashboard body 1) draws the overview object: context full width, then spend | cost per turn, then cache | tool calls, with `cctop_charts::render_cells` for every chart and verdict lines wrapped. Below 100 columns the pairs stack.
- [ ] Insta snapshots at 80, 122 and 190 columns; assertion lines stripped, `*.snap.new` deleted.
- [ ] Row identity: for fixture B, the TUI's chart cells equal the pane's Raster cells (`tests/pane` compares against a fixture the Rust side writes).

### US-006: Picture or cells in the pane
**Description:** As a pane user, I want pictures when my terminal can show them and clean cell charts when it can't, without configuring anything.

**Acceptance criteria:**
- [ ] At `session.start` the pane compiles `./charts.wasm` with `$.wasm.compile` (a literal path, in a top-level function of `pane.tsx`) and instantiates it once.
- [ ] Decision order:
  1. `$.env.get("CCTOP_PANE_CHARTS")` = `image` | `cells`.
  2. `[pane] charts` from `cctop query config`.
  3. The environment: `TERM_PROGRAM` ghostty, iTerm.app or WezTerm, or `KITTY_WINDOW_ID` set → image. Apple_Terminal or vscode → cells. Unknown → image with probe.
  4. A probe blit: a `{ deny }` naming the alt switches to cells.

  The decision is a pure function with a table test.
- [ ] The marker gains `charts`, and `cctop pane status` prints it.
- [ ] If the wasm fails to compile, the pane logs once and draws the cell version from `render_cells` output that the query carries as a fallback field. Nothing breaks.

### US-007: The overview in the pane
**Description:** As a pane user, I want the POC 6 overview, smooth in Ghostty (POC 7) and in cells in Terminal.app (POC 8).

**Acceptance criteria:**
- [ ] `views/overview.tsx` draws the overview object in the layout of spec §5.2. Charts are `Image` (base64 PNG, `alt` = the readout summary) or `Raster`, keyed by chart id and placed on whole cells; labels and verdicts are `Text` beside them.
- [ ] Only charts whose spec hash changed are re-rendered, outside `ui.render`, and swapped with `$.ui.blit`. A test with a fake engine counts the wasm calls.
- [ ] Hover: one keyed `Box` per turn column over each per-turn chart reveals its readout; the turn numbers are `Button`s that set the readout on click.
- [ ] Width: two columns at ≥ 100 body columns, stacked below, sparkline form below 60 (tests at 59, 99, 105).
- [ ] Long sessions: per-turn charts show the last N turns that fit at 2 cells per turn, labelled "last N turns"; context and spend bucket the whole session (test on a 300-turn synthetic session).
- [ ] The footer reads "spent so far $X · what this session would cost at API prices".
- [ ] `docs/verification/pane.md` items 45 (Ghostty pictures), 46 (Terminal.app cells), 47 (tmux with and without passthrough) and 52 (hover), each `Result: pending`.

### US-008: The coach view in the pane
**Description:** As a pane user, I want the POC 1 coach: the full advice, a fix prompt I can click, and rows that open.

**Acceptance criteria:**
- [ ] The NOW card shows: name, age, the rule id dimmed, headline, body, evidence rows, and the fix-prompt card. Clicking the card calls `$.prompt.fill`, and a confirmation line says nothing was sent.
- [ ] Buttons per action kind: key → the step "press Esc to stop the turn" shown first, then fill the draft; slash → fill the command; allow rule → copy the rule (`$.ui.copy`), open settings.json, fill `/permissions`; read → got it. Plus snooze.
- [ ] "Up next" rows and the four lights open in place (state in `model.ts`, reducer tests). The context light opens into the composition (PRD 3) and the residency figures.
- [ ] `docs/verification/pane.md` item 48 (fill → Enter), `Result: pending`.

### US-009: The act dialog
**Description:** As a pane user, I want to work through an action step by step, so that I know exactly what changes before I do it.

**Acceptance criteria:**
- [ ] No call to `$.turn.abort` anywhere (a grep test in `tests/pane/`). Key-kind items show "press Esc to stop the turn" as their first step.
- [ ] `a` or pressing a nudge opens the act dialog: `$.ui.open({ id: 'cctop-act', focus: true, closeOnEscape: true, holdToasts: true, rows })`. The list is on the left; the detail on the right shows what changes (a settings diff as `Code format="diff"` for allow rules), the exact text, and the kind's buttons.
- [ ] Items 49 (the Esc hint shown first; Esc stops the turn with its marker) and 50 (dialog keys) in `docs/verification/pane.md`, `Result: pending`.

### US-010: Rows placed in the transcript
**Description:** As a person reading the conversation, I want cctop's important notes where they happened, so that I see them without opening the pane.

**Acceptance criteria:**
- [ ] A NOW nudge places `nudge-<fireId>` at the turn end, drawn as a bordered card with the headline, one line of body and its two main buttons.
- [ ] A located cache miss (PRD 1) places `miss-<turn>`: one dim row naming the cause and the cost.
- [ ] Display only; nothing is written to the transcript (test: no `session.append` call).
- [ ] Item 51 (placed rows scroll, gone after `--resume`), `Result: pending`.

### US-011: What PRDs 1–7 add
**Description:** As a pane user, I want the new facts from PRDs 1–7 where they belong, so that the redesign shows everything cctop knows.

**Acceptance criteria** (each guarded by presence; absent data hides the element):
- [ ] PRD 1: located cause in cache readouts, `prevent[]`, A01 text, placed miss rows.
- [ ] PRD 2: header badges (repo, worktree, `+N dirs`, output style, agent, remote), spend limit in USD, `>200k` marker.
- [ ] PRD 3: the composition bar and the context light detail, labelled with the capture's age.
- [ ] PRD 4: thinking-drop markers on the cache strip, a budget line and halt ETA on spend, lineage in the header and a chain total.
- [ ] PRD 5: an elicitation NOW card, "after settings change" on misses, a skill-shell heat-map row, the amber hooks badge.
- [ ] PRD 6: the transcript path for the poller, data-rule badges, agent limits, jump to agent, opt-in notify, the running state ending at once on `session.stop`.
- [ ] PRD 7: the permission card split by who decided, retry readouts, a hook-overhead verdict.
- [ ] One snapshot per PRD's contribution, with fixture data present and absent.

### US-013: The words, reviewed
**Description:** As the person who owns cctop's voice, I want to review every rule's name, body and fix prompt and every verdict threshold before release, so that no advice is wrong or unclear.

**Acceptance criteria:**
- [ ] `cctop coach --copy-md > docs/coach-copy.md` and `cctop verdicts --md > docs/verdicts.md` are generated from the code, and a test fails when either is stale. One row per rule (id, plain name, body, fix prompt, actions, foot); one row per chart and level (threshold, text, Do/Don't).
- [ ] The drafts are grounded in `tasks/research-coach/` and the coach PRD §5; each threshold cites its source in a column.
- [ ] The user has reviewed both tables (a PR comment or sign-off) before the release tag.

### US-012: Fonts and docs
**Description:** As a person setting up cctop, I want the recommended terminal settings, so that the pane looks its best.

**Acceptance criteria:**
- [ ] The README and `plugin/README.md` get a "Best look" section:
  - Ghostty: `font-family = JetBrains Mono`, `font-size = 13`, `adjust-cell-height = 23%`.
  - Terminal.app: SF Mono Regular 13 pt, character and line spacing 1, antialias on, bright-bold off.
  - Which terminals show pictures, and `CCTOP_PANE_CHARTS`.
- [ ] `make site` rerun; the plugin README's versions table gets the 0.12.0 row.

## 4. Functional requirements

- FR-1: Every coach rule must have a plain-language name, and the coach object must carry full-length body, fix prompt, actions and foot.
- FR-2: Every overview chart must carry a verdict computed by fixed rules from structural facts.
- FR-3: Charts must render from one Rust crate: cells natively and in wasm, pictures in wasm only.
- FR-4: The pane must choose picture or cells by override, then config, then environment, then probe.
- FR-5: No picture may contain text; labels are terminal cells.
- FR-6: Fill must never send, and the pane must never act on the session (no `$.turn.abort`).
- FR-7: The act dialog must close on Esc and hold toasts while open.
- FR-8: Placed rows must be display only.
- FR-9: Every PRD 1–7 element must hide cleanly when its data is absent.
- FR-10: The TUI and the pane must show identical words, numbers and cell charts for the same session.

## 5. Non-goals

- The band above the prompt, the cell beside the prompt, and the status-line hook.
- Pictures in the TUI.
- Desktop `Svg` and themes.
- Jumping the transcript to a tool call.
- Any model-written text.

## 6. Technical considerations

- `$.wasm` is an early-access noun: if a release breaks it, the cell fallback from the query keeps the pane working (US-006).
- PNG bytes go through `$` as base64 (≤ 2 MiB). Rendering at 2× of an assumed 8×16 px cell keeps the largest chart near 1.6 MiB raw RGBA; PNG compresses it far below. Pictures are encoded as PNG, never RGBA.
- The wasm runs inside Claude Code's process: set the memory maximum, and keep every render bounded (no loops over unbounded input; specs carry at most 512 points).

## 7. Success metrics

- No coach text cut mid-sentence anywhere in the pane.
- In the next design review, the person can say what each chart means and what to do without asking.
- Rendering cost under 30 ms per chart in wasm (test), no visible stutter during a poll (item 52).

## 8. Open questions

- Whether iTerm2 shows pictures outside fullscreen. The probe handles it either way.
