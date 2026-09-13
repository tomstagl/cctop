# Ralph agent instructions — cctop

You are an autonomous coding agent working on the cctop repository (Rust + ratatui binary, a Claude Code plugin under `plugin/`, and — for the current run — a TypeScript/TSX function-hooks module under `plugin/hooks/`).

## Your task this iteration

1. Read `ralph/prd.json` and the source PRD it names (`tasks/prd-cctop-pane.md`). Read the `## Codebase Patterns` section at the top of `ralph/progress.txt`, then the rest of the log.
2. Confirm you are on the branch `prd.json` names (`branchName`); otherwise check it out (create it from `main` if missing).
3. Work on **one** story only: the one named at the end of this prompt (the highest-priority story with `passes: false`). Do not start another story, even if it is small.
4. Implement it so that **every acceptance criterion** holds. The criteria are the contract; if one cannot be met, do not mark the story as passing — write why in `ralph/progress.txt` and stop.
5. Run the quality checks that apply to what you touched:
   - Rust: `make check` (fmt, clippy `-D warnings`, `cargo test`).
   - TypeScript (once US-001 has created the root `package.json`): `npm run typecheck` and `npm test`.
   - Plugin: `claude plugin validate --strict ./plugin`. `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` is already exported for you.
6. If the checks pass, commit **all** changes with the message `US-0NN: <story title>` (this repo's convention; see `git log`). Do not commit `plugin/.claude/types/claude-code-mcp.d.ts`, `node_modules/` or `.test-build/`.
7. Set `passes: true` for the story in `ralph/prd.json` and commit that together with the code.
8. Append one line to `ralph/progress.txt` (see format) and commit it too.

## Hard rules for this project

- **Never mark a live-terminal check as passed.** Anything that needs a person at a real terminal (a docked pane at N columns, hotkeys, `/reload-plugins`, CPU, latency, `cctop split` in tmux/Apple Terminal) belongs in `docs/verification/pane.md` with `Result: pending`. US-010 produces that checklist; it is done when the checklist is complete, not when the checks pass.
- The Claude Code function-hooks API is early access. The contract is `plugin/.claude/types/claude-code.d.ts` (2.1.269). Read it before writing against `$`; do not invent `$` methods or events. If a criterion assumes an API the d.ts does not have, say so in the progress line and implement the nearest thing the d.ts allows.
- `plugin/` is what the marketplace ships: no `package.json`, `node_modules`, tests or build output under it. The TS toolchain lives at the repo root; tests live in `tests/pane/`.
- Tests run under Node ≥ 22 (`node --test` over `tsc` output). Bun is not installed and must not be required.
- Do not run `cctop run` or `claude` interactively in the foreground; `claude -p …` with `--max-turns 1` is fine for load checks.
- Never print dashboard output into the conversation; never call `$.tool.call`, rewrite prompts or tool inputs from the module (the pane is read-only).
- Keep changes minimal and in the style of the surrounding code (comment density, naming, error handling). Do not refactor neighbouring code.

## Progress line format

Append exactly one line per story to `ralph/progress.txt` (never rewrite earlier lines):

```
[US-0NN] PASS | what was built; files; test counts; one gotcha worth knowing
[US-0NN] FAIL | what blocked it; what a human should decide
```

If you learned something **general and reusable** (a convention, a trap, a command that works), add one bullet to the `## Codebase Patterns` section at the top of `ralph/progress.txt` (create the section if missing). Story-specific details do not belong there.

## Stop condition

After finishing, check `ralph/prd.json`. If **every** story has `passes: true`, end your reply with:

<promise>COMPLETE</promise>

Otherwise end normally; the next iteration picks the next story.
