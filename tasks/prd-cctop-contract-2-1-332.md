# PRD 0: contract 2.1.332 — the d.ts without `/plugin-types`, CI green again

**Status:** v1.0 · 2026-11-26 — ready for implementation.
**Target:** cctop v0.10.0 (plugin 0.11.0), contract Claude Code 2.1.332.
**Depends on:** nothing. **Blocks:** `prd-cctop-context-breakdown.md` (PRD 3) and `prd-cctop-pane-apis.md` (PRD 6), which call `$` nouns the 2.1.284 d.ts does not declare.
**Source:** the 2026-11-26 scan of the installed Claude Code 2.1.332 (build `f2426755`, 2026-11-24), measured on the maintainer's machine. Every claim in §1 was reproduced that day.

> Decisions taken with the user on 2026-11-26:
> 1. The contract bump is its own PRD and lands first. PRDs 1, 2, 4, 5 and 7 do not wait for it.
> 2. A GitHub issue tracks the PRD; the PRD is the spec.

---

## 1. Introduction

cctop pins the function-hooks API to one Claude Code version in three places:

- the d.ts at `plugin/.claude/types/claude-code.d.ts` (line 1: `// Written by Claude Code 2.1.284.`)
- `TESTED_WITH` in `plugin/hooks/model.ts:18`
- `READ_FROM` in `src/harness_facts.rs:17`

The release checklist (issue #4, `CLAUDE.md`) regenerates the d.ts with `claude -p '/plugin-types ./.claude/types'`. **That command no longer exists in 2.1.332**: the string `plugin-types` survives in the binary only as a CSP directive name.

The scheduled `contract` CI job has failed on every run `gh run list` shows, the last three being 2026-11-24, 11-25 and 11-26 (run 45761595761). Its checks on 2.1.332:

| Check | Result | Why |
|---|---|---|
| Load check (module loaded, `/cctop-pane` listed, self-check, marker) | **green** | The pane still loads and runs on 2.1.332 |
| `claude -p "/plugin-types …"` exit code | exit 1 | The prompt is no longer a native command, so it goes to the model. Under the temp HOME there are no credentials, hence "Not logged in". |
| Contract surface diff | `/plugin-types wrote no …/claude-code.d.ts` | Nothing writes the file |
| `claude plugin validate --strict ./plugin` | 5 type errors (table below) | The validator now type-checks with `noUncheckedIndexedAccess`, and `Placement` changed |

| File:line | Error | Cause |
|---|---|---|
| `plugin/hooks/model.ts:364` | TS2322 `number \| undefined` → `number` | `sorted[rank]` in `percentile` |
| `plugin/hooks/pane.tsx:148` | TS2322 `"dock" \| "inline" \| "widget"` → `Placement` | 2.1.332 adds `widget` to `e.props.placement`; the model's `Placement` lacks it |
| `plugin/hooks/pane.tsx:636`, `:637` | TS18048 `line` possibly undefined | `lines[lines.length - 1]` |
| `plugin/hooks/pane.tsx:654` | TS2532 object possibly undefined | the same pattern |

**Where the d.ts comes from now (measured):**

| Route | Credentials | Result on 2.1.332 |
|---|---|---|
| The engine writes `.claude-plugin/types/claude-code/index.d.ts` beside a mod on load. The d.ts header promises this. | — | **Not written** by `claude -p --plugin-dir ./plugin`, with either the temp HOME or the real, trusted HOME. The 2.1.284 copy from 2026-09-29 stayed. It is probably interactive-only; unverified, and not usable in CI either way. |
| The bundled `plugin-authoring` skill. Invoking it writes `types/claude-code.d.ts` under `$TMPDIR/claude-<uid>/bundled-skills/<version>/<random>/plugin-authoring/` | **yes** (one model call) | Full file, tool tables included: 29 571 lines, line 1 `// Written by Claude Code 2.1.332.` Under the temp HOME without credentials: "Not logged in", nothing written. |
| The binary. The core d.ts is bundled zstd-compressed as `/$bunfs/root/claude-code.d.ts-138c1d26.txt.zst`; its decompressed text starts `// Claude Code function hooks: the plugin API's TypeScript declarations.` | no | The core surface (23 551 lines, no tool tables). With `// Written by Claude Code <v>.` prepended, its surface equals the skill file's **apart from one trailing newline**. |

So the full file (for typecheck) needs the skill and credentials, run locally at release time. The surface (for CI's drift check) can come from the binary without credentials.

## 2. Goals

- The contract CI job is green on the latest Claude Code, without credentials.
- One local command regenerates the checked-in d.ts from the installed `claude`.
- The checked-in contract, `TESTED_WITH`, `READ_FROM` and the plugin README all name 2.1.332.
- The release checklist in `CLAUDE.md` matches what the tooling does.

## 3. User stories

### US-001: `scripts/extract-dts.py`, the surface without credentials
**Description:** As CI, I want the d.ts core read straight out of the `claude` binary, so that the contract check needs no login and no model call.

**Acceptance criteria:**
- [ ] `scripts/extract-dts.py <claude-binary> <out.d.ts>` (stdlib Python, run with `python3 -I`) scans the binary for zstd frames (magic `28 B5 2F FD`). It decompresses each candidate with the `zstd` CLI (`zstd -dc --no-check`, at most 3 MB input per frame). It writes the first output that starts with `// Claude Code function hooks: the plugin API's TypeScript declarations.`
- [ ] It prepends `// Written by Claude Code <v>.`, `<v>` taken from `<claude-binary> --version`, and ends the file with one trailing newline, so its surface is byte-identical to the skill's.
- [ ] Exit 1 with a clear message when no frame matches, or when the match is under 1000 lines.
- [ ] A test, `scripts/test-extract-dts.sh` or a Rust test that shells out, skipped when `claude` or `zstd` is absent, asserts that line 1 names the installed version and line 2 is the header comment.
- [ ] It resolves the binary through symlinks (`claude` on PATH is a symlink into `~/.local/share/claude/versions/<v>`).

### US-002: `scripts/regen-dts.sh`, the full file for the release checklist
**Description:** As the maintainer, I want one command that replaces the checked-in d.ts with the installed `claude`'s full declarations, tool tables included.

**Acceptance criteria:**
- [ ] `scripts/regen-dts.sh` creates a temp dir `T`, then runs `TMPDIR=$T CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p --max-turns 1 "/plugin-authoring reply with ok and do nothing else" </dev/null`. It copies `T/claude-*/bundled-skills/<v>/*/plugin-authoring/types/claude-code.d.ts` (the newest match) to `plugin/.claude/types/claude-code.d.ts`, then removes `T`.
- [ ] It fails, naming the path it looked in, when no file appears or line 1 does not name `claude --version`'s version.
- [ ] Its header comment says it makes one model call and needs a logged-in `claude`, and names `extract-dts.py` as the credential-free route for the surface alone.

### US-003: `check-contract.sh` on the new sources
**Description:** As the maintainer, I want the contract job green again with the same four checks.

**Acceptance criteria:**
- [ ] Step 1 keeps the load check and its assertions unchanged. The prompt becomes a plain no-op (`"noop"`) and `</dev/null` is added. A non-zero exit whose output contains `Not logged in` passes; any other non-zero exit fails.
- [ ] Step 2 gets the fresh d.ts from `scripts/extract-dts.py "$(command -v claude)" "$work/types/claude-code.d.ts"`. The surface extraction (`tools_marker`) and the 1000-line floor are unchanged.
- [ ] The regenerate hint printed on a surface change names `scripts/regen-dts.sh`.
- [ ] `.github/workflows/ci.yml`'s contract job installs `zstd` if `command -v zstd` fails (ubuntu: `sudo apt-get install -y zstd`).
- [ ] No string `/plugin-types` remains in `scripts/`, `Makefile`, `.github/` or `CLAUDE.md`.

### US-004: The checked-in d.ts moves to 2.1.332
**Description:** As a developer, I want the checked-in contract to be 2.1.332's, so that `npm run typecheck` names every call site that moved.

**Acceptance criteria:**
- [ ] `plugin/.claude/types/claude-code.d.ts` is regenerated with `scripts/regen-dts.sh`; line 1 reads `// Written by Claude Code 2.1.332.`
- [ ] `tsconfig.json` sets `"noUncheckedIndexedAccess": true`. The bundled tsconfig template has it, so local typecheck and `claude plugin validate` agree.
- [ ] `npm run typecheck` is green. The commit message lists every source change it needed.

### US-005: The pane type-checks under the 2.1.332 validator
**Description:** As a developer, I want `claude plugin validate --strict ./plugin` to report zero errors.

**Acceptance criteria:**
- [ ] `percentile` (`model.ts:360`) returns `sorted[rank] ?? 0`.
- [ ] The model's `Placement` type includes `'widget'`. The reducer lays a widget out as `inline`, and `tests/pane/model.test.ts` has a case for a render with `placement: 'widget'`.
- [ ] `pane.tsx:636–654` reads the last line through a guarded local (`const line = lines.at(-1)`), with no non-null assertion.
- [ ] `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude plugin validate --strict ./plugin` prints no `error TS`.
- [ ] `npm test` green.

### US-006: Version pins and docs
**Description:** As the maintainer, I want every pin to name 2.1.332 so that the header badge, `cctop pane status` and the README tell the truth.

**Acceptance criteria:**
- [ ] `TESTED_WITH = '2.1.332'` in `plugin/hooks/model.ts`; plugin manifest version 0.11.0.
- [ ] `make check-facts` was run against 2.1.332. Each constant it flags is re-verified or updated in `src/harness_facts.rs`, `READ_FROM = "2.1.332"`, and `make check-types` is green.
- [ ] `plugin/README.md` "Claude Code versions" gets a row for 0.11.0 / 2.1.332 naming the forced changes (`widget` placement, `noUncheckedIndexedAccess`). The 0.9.0 row's "runs on" range ends at the last version CI saw green (2.1.284 unless the maintainer knows better).
- [ ] `CLAUDE.md`: release checklist step 1, the Commands block and the pane paragraph name `scripts/regen-dts.sh` / `scripts/extract-dts.py` and say why there are two.
- [ ] `.gitignore` adds `plugin/.claude-plugin/types/`, which older engines wrote and which is local-only.
- [ ] `docs/verification/pane.md` gains item 39, *"An interactive session rewrites `plugin/.claude-plugin/types/claude-code/index.d.ts`"* (setup: `claude --plugin-dir ./plugin` in a real terminal; expected: line 1 names the running version). `Result: pending`.
- [ ] `make check && npm test` green; the CI `contract` job green on the PR.

## 4. Functional requirements

- FR-1: `scripts/extract-dts.py` produces the d.ts surface from a `claude` binary without network, login or model call.
- FR-2: `scripts/regen-dts.sh` produces the full d.ts, tool tables included, with one model call.
- FR-3: `scripts/check-contract.sh` has no reference to `/plugin-types` and passes on 2.1.332 without credentials.
- FR-4: `claude plugin validate --strict ./plugin` reports zero type errors on 2.1.332.
- FR-5: `TESTED_WITH`, `READ_FROM`, the d.ts line 1 and the README table all name 2.1.332.

## 5. Non-goals

- Adopting `claude plugin test <dir>` (new in the 2.1.332 header) for `tests/pane/`. The node test runner stays.
- Using any new `$` noun. That is PRDs 3 and 6.
- Relying on the engine writing `.claude-plugin/types/claude-code/index.d.ts`. It did not in `-p` mode (§1).

## 6. Technical considerations

- **CI's binary.** `npm install -g @anthropic-ai/claude-code@latest` must put the Bun-compiled executable on PATH for `extract-dts.py` to find the bundled frame. **To verify in the PR's first CI run.** If the npm package ships a JS bundle instead, the script finds the declarations text in that bundle's files: it searches for the same header line in any file under the package directory. Amend US-001 then.
- The zstd scan takes about 30 s on the 397 MB binary (measured with one `zstd` process per candidate frame, 989 frames). Stop at the first match to keep CI fast. The d.ts frame sat at offset 390 MB, so scanning from the end first is the cheap order.
- `KNOWN_INCOMPATIBLE` in `src/pane.rs`: plugin 0.10.0 still loads on 2.1.332 (load check green), so no entry is added.
- `make check` can go red from rustfmt drift in `harness_facts.rs`; fold it in.

## 7. Success metrics

- The `contract` job is green on its first scheduled run after merge.
- A Claude Code release that moves the surface turns the job red with the moved declarations named, as before.

## 8. Open questions

- Is the engine's write of `.claude-plugin/types/claude-code/index.d.ts` interactive-only? It only matters if it would make US-002's model call unnecessary. The live check goes in `docs/verification/pane.md` as item 39 (`Result: pending`).
