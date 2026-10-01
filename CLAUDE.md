# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

cctop is a read-only, btop-style dashboard for a running Claude Code session. Two deliverables live in one repo:

- **The `cctop` binary** (Rust, ratatui): `src/`. Reads what Claude Code already writes (`~/.claude/sessions`, the transcript `.jsonl`, hook spool, status-line shim, process tree) and shows it as a TUI, prints it as JSON (`cctop query`), or serves it as MCP tools (`cctop mcp`).
- **The Claude Code plugin** (`plugin/`): two skills (`/cctop`, `cctop-insights`) and a function-hooks module (`plugin/hooks/*.ts[x]`) that docks the same dashboard inside Claude Code as a pane. `plugin/` is exactly what the marketplace ships — no `package.json`, tests or build output under it; the TS toolchain lives at the repo root and tests in `tests/pane/`.

## Commands

Rust (this is what CI runs):

```
make check                       # cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo test <name>                # one test / module, e.g. cargo test advisor::rules::outcome
cargo build --release            # target/release/cctop
make check-types                 # d.ts + harness_facts version vs installed `claude` (skips if no claude)
make check-contract              # what CI's `contract` job runs: the §A load check (no model call), the d.ts surface diff, the validator, the pins
make check-facts                 # src/harness_facts.rs constants re-read from the installed Claude Code bundle; then bump READ_FROM
```

Pane (TypeScript, Node ≥ 22, no bun):

```
npm run typecheck
npm test                                                # tsc -p tsconfig.test.json → node --test '.test-build/tests/pane/**/*.test.js'
node --test .test-build/tests/pane/poller.test.js       # one file, after the tsc step above
CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin validate --strict ./plugin
```

Generated artefacts that tests assert are fresh (regenerate, never hand-edit):

```
cctop metrics --md > docs/metrics.md && cctop metrics --readme README.md   # from src/metrics/registry.rs
scripts/pane-fixtures.sh                          # tests/pane/fixtures/<verb>.json from fixtures/session-a.jsonl
scripts/pane-fixtures.sh fixtures/session-b.jsonl b   # the -b and coach-<moment> fixtures — rerun after any change to the coach or dashboard objects
INSTA_UPDATE=always cargo test                    # accept insta snapshots (src/snapshots, src/ui/snapshots); then strip `assertion_line:` and delete *.snap.new
make site                                         # site/ from README marked blocks + docs/metrics.md + themes
```

Release checklist (issue #4 — the contract moves between Claude Code releases; every step is a command above):

1. `claude update`, then `cd plugin && CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p '/plugin-types ./.claude/types'` — regenerate the d.ts against the latest `claude`; `npm run typecheck` names every call site a changed surface touches.
2. `make check-contract` — the §A headless load check (no model call), the surface diff, the validator, the pins; then `make check-facts` and bump `READ_FROM` in `src/harness_facts.rs` (`make check-types` fails past 5 releases behind).
3. `TESTED_WITH` in `plugin/hooks/model.ts` to the version of step 1; the plugin manifest version; the "Claude Code versions" table in `plugin/README.md` (the range the release runs on, and a new row when a contract change forced it); `KNOWN_INCOMPATIBLE` in `src/pane.rs` when a pair is known to fail.
4. `make check && npm test`, the install lines and `make site`, then the release commit and the tag on `main`'s tip after the merge.

Useful for poking at behaviour without a live session: `--session` accepts a fixture path (`cctop query dashboard --session fixtures/session-b.jsonl`), `--lines N` feeds only the first N transcript lines (a point in time), and `CCTOP_FAKE_NOW=<epoch ms>` fixes the clock. For a recording, `scripts/replay-session.py fixtures/session-b.jsonl /tmp/replay.jsonl --from 700 --speed 60` appends the fixture on the real clock and `CCTOP_FIXTURE_LIVE=1 cctop run --session /tmp/replay.jsonl` shows it as a live session (fixture B compacts at line 766: ctx 33 % → 19 %). Never run `cctop run` or `claude` interactively in the foreground of a tool call; `claude -p … --max-turns 1` is fine for load checks, and its debug log is `~/.claude/debug/latest`.

## Architecture

**One state, many surfaces.** Every number comes from the same pipeline, so the TUI, `cctop query`, the MCP tools and the pane never disagree:

```
tail.rs (follow .jsonl) → transcript::Line (typed, tolerant; unknown types → Line::Unknown)
   → ui::State::apply (+ apply_hook / apply_status / apply_otel / apply_claude_home from the other collectors)
   → metrics/ (context, usage, cost, limits — derived on read)
   → advisor::Engine (36 rules, one nudge slot)
   → coach.rs / dashboard.rs (the objects every surface draws verbatim)
   → ui/ (ratatui) · query.rs (JSON) · mcp.rs · plugin/hooks/views (the pane, via `cctop query`)
```

- `load.rs` builds a fully loaded `State` without a UI; `app.rs` owns the interactive loop; `attach.rs` rebuilds every collector when the session changes (the picker, or `/clear` rotating the session id).
- `metrics/registry.rs` declares every metric once (id, unit, formula, sources, caveats, estimate). `docs/metrics.md` and the README block are generated from it and `cctop query` tags values with these ids; a test fails when either is stale.
- `harness_facts.rs` holds Claude Code's constants and the version each transcript field first appeared in, tagged with `READ_FROM`; parsers fall back on older transcripts from it. Bump it when you re-verify against a newer `claude`.
- **The coach** (`advisor/`): rules in `rules/{token,events,outcome}.rs` implement the `Rule` trait (id, urgency class NOW › NEXT › LATER, `evaluate`, `acted`, TTL, cooldown). The engine in `advisor/mod.rs` keeps one nudge in the slot, persists fires/snoozes to `~/.cctop/<session>.advisor.json`, and runs an exposed/control arm. Rules fire only on structural evidence (a tool result, a denial kind, a git operation), never on prompt text. `coach.rs` and `dashboard.rs` cut text at fixed widths so the TUI and the pane show identical characters; `tests/pane` asserts the fixture-B moments row-identical on both sides.
- **The pane** (`plugin/hooks/`): `pane.tsx` registers the hooks and owns module state; `model.ts` is a pure reducer; `poller.ts` runs `cctop query <verb> --session <id>` on a timer; `views/` are pure renderers over the query JSON. The native command is `/cctop-pane` (the engine reserves `/cctop` for the skill). The API contract is `plugin/.claude/types/claude-code.d.ts`, generated by `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p '/plugin-types ./.claude/types'` and stamped with the Claude Code version on line 1; `TESTED_WITH` in `model.ts` is what the header badge shows. Function hooks are early access and change between releases — read the d.ts before writing against `$`, and regenerate it when `claude` moves.

## Rules that are easy to break

- **Read-only, and no prose.** cctop never modifies Claude Code's files or sends anything on its behalf, and never keeps what the person or the model wrote: collectors take lengths, counts and booleans from free text and drop it (`transcript/`, `history.rs`, `insights.rs` document what is deliberately not parsed). Fixture transcripts are anonymised copies of real sessions — regenerate with `scripts/anonymise-transcript.py` / `compose-fixture.py`, never hand-edit their structure.
- **The plugin validator** (`claude plugin validate --strict`) follows `$` only into top-level `function`s of the same file: keep module state in module-scope `let`s, hand helper modules a facade type (a `Pick` of the `$` nouns, built in `pane.tsx` like `pollerEngine($)`), and spell `$.env.get("NAME")` literally in `pane.tsx`. Never name a local `h` or `Fragment` in a `.tsx` file (JSX compiles to the global `h`). Failures in a `.catch` are `next.error`, not `failure`.
- **Live-terminal checks are never marked passed by automation.** Anything that needs a person at a real terminal goes in `docs/verification/pane.md` as `Result: pending`.
- Rustfmt reformats whole files, so `make check` can go red from drift in a file you touched lightly; fold it in rather than leave it.
- Commit subjects are descriptive prose (`Coach: …`, `Docs: …`, `Site: …`, `release: vX.Y.Z — …`); ralph runs use `US-0NN: <story title>`. Do not commit `plugin/.claude/types/claude-code-mcp.d.ts`, `node_modules/` or `.test-build/`.

## Where the plans live

`tasks/prd-cctop.md` (v1.1), `tasks/prd-cctop-pane.md`, `tasks/prd-cctop-coach.md` are the contracts; `tasks/plan-cctop-coach.md` §1 records what shipped in which commit; `tasks/handoff-coach-round-2.md` is the current open-items list. `ralph/` is the autonomous-agent loop (`prd.json` stories, `progress.txt` with a `## Codebase Patterns` section worth reading). `docs/claude-code-panels.md`, `docs/socket.md`, `docs/hosts.md` are traced from the Claude Code binary and are the only documentation of those mechanisms.
