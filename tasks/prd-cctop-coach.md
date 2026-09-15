# PRD: cctop coach — actionable signals and a real-time coach view

**Status:** v1.3 · 2026-09-15 — implemented (plan Phases 0–7 on `main`, released as cctop 0.3.0); the boxes below record what holds, the notes what does not (v1 reviewed by 64 adversarial verdicts, a four-design judge panel and a completeness critic; see §14)
**Target:** cctop ≥ 0.1.1 attached to Claude Code CLI 2.1.269 on macOS/Linux; TUI first, then the function-hooks pane (`tasks/prd-cctop-pane.md`) and `cctop query` / MCP.
**Depends on:** `tasks/prd-cctop.md` v1.1 (the nine panels, rules A01–A18, metrics registry, query interface); the pane PRD for the second front-end.

> Assumptions taken without pausing (autonomous run):
> A. The person at the keyboard is a solo developer in an interactive session; bot loops (Ralph, workflows, `-p`) get no human-facing nudges.
> B. The coach stays rule-based and read-only: no model calls, no prompt injection, no rewriting of tool inputs. Enforcement ideas are recorded as opt-in follow-ups (§13).
> C. Every number the coach shows must reconcile with what Claude Code itself shows in `/usage`, `/context`, `/cost` and its footer, so the two never disagree.
> D. Numbers in this document come from 81–113 real sessions on this machine (2026-08-12 → 2026-09-13, 7 projects, ~6 300 main-thread API calls) unless marked otherwise; they are this user's habits, not population statistics.
>
> **v1.1 changes:** 31 of 64 candidates were refuted as *nudges* (never on data availability) and survive as metrics, indicators or Events rows; the coach view is the judged "Lights" design (§6); the autocompact threshold is *effective window* − 13 000 (967 k on native-1M models, from the debug log's `effectiveWindow=980000`), not window − 13 000; baselines in §12 use the verdict denominators; §13 closes the TTL question and adds the critic's gaps.
>
> **v1.2 changes (alignment with the implementation plans, 2026-09-14):** the dashboard is direction B of the design canvas (`tasks/plan-dashboard-big-figures.md`): the framed nine-panel grid and `layout::solve` are removed on both surfaces and replaced by four block-digit tiles (the coach's four lights), the nudge line and a borderless nine-row ledger whose digits open a panel full-screen. Consequences folded in below: every §4 panel addition lands in that panel's two-line ledger row and its full-screen view; US-009's view switch is a `State.view` flag checked before the dashboard draw (no `layout::solve` to skip); US-010's pane Overview draws the dashboard object (`cctop query dashboard`), whose tiles *are* the coach's lights, so the two views cannot disagree; §11's view note is superseded. Evidence re-checked on 2.1.270 while planning: `/clear` writes a `continued-in {continuedInSessionId}` line into the old transcript (a boundary marker, added to US-001); the hook spool must never store `UserPromptSubmit.prompt` or `Stop.last_assistant_message` verbatim (FR-12) — `cctop hook` keeps their lengths and a "ends with `?`" flag instead (US-003); the transcript's `cache_miss_reason` carries `type` only on this machine, so `cache_missed_input_tokens` is optional; the local corpus' first-seen map starts at 2.1.231 (the research trail's T38 map covers 2.1.220 →).
>
> **v1.3 changes (reconciled with the shipped implementation, 2026-09-15):** the acceptance boxes are ticked where the code, its tests and `tasks/plan-cctop-coach.md` §1 hold them, and annotated where they do not; §5.3 adds the per-family table US-013 asked for (the Claude Code surface each family complements, and when the coach stays silent); §6.4's retirement rule names the two rules that outlive the next prompt (verify-gap through the following turn's end, review-before-merge for three turns) and the five NOW rules whose TTL is the next prompt rather than the turn's end; US-011 records that `stats-cache.json` carries no `hourCounts` on 2.1.270, so the active hours come from session-meta `message_hours`; the pane PRD gained its v1.2 amendment (`tasks/prd-cctop-pane.md` §11).

---

## 1. Introduction

cctop today answers "what is happening": nine panels of numbers and an Advisor with 18 rules ranked by tokens saved. Two things are missing.

First, cctop ignores most of what Claude Code 2.1.269 already writes to disk. The status line carries Claude Code's own prompt-cache diagnosis (warm/cold, expiry time, tokens to re-cache, the named cause of the last miss). The transcript carries exact compaction records, the API's own `cache_miss_reason` per response, denial kinds, interrupt markers, per-attachment rendered text (the harness reminders that ride on every call), git operation summaries, stale-file hints, per-line skill/agent/MCP attribution and a prompt id per turn. The hook spool cctop already writes contains exact tool durations, the API error that killed a turn, background-task lists and the cold-resume cost estimate. `~/.claude` holds the deterministic per-session rework counters and the LLM-derived friction taxonomy that `/insights` computes, the slash-command history that transcripts lack, checkpoint versions per file and lifetime plugin/skill usage counters. None of this reaches a panel.

Second, the Advisor's rules are all on the token axis and are session-lifetime: one 82-second `find /` outranks a live problem for the rest of the session, three of its causes for cache misses are documented cache-keepers, its compaction detector counts API-error lines as compactions, its MCP rule cannot fire on 2.1.269 because tool search is on by default, and its hand-off rule is a three-hour timer with no notion of a task boundary. Nothing tells the person that they edited three files and ran no check, that they are about to commit two calls after a failing test, that they interrupted a turn eleven calls in, that a `/model` switch now will re-bill 400 k tokens, or that their prompt cache goes cold in four minutes while a question sits unanswered.

This PRD adds (a) the missing data sources, (b) new metrics for the existing panels, (c) 34 new advisor rules on all three levers (token savings, better outcomes, less rework) with fixes to the 18 existing ones, and (d) a second top-level view — the **coach** ("Lights") — that shows four state lights, one nudge and what comes next, in 56 columns, driven by the same rules, with an urgency model that retires nudges when the person acts.

## 2. Goals

- **Tell the person what to do in the next minute**, not what happened: one nudge at a time, with the exact keystroke, slash command, setting or prompt line; retired the moment they act.
- **Cover the outcome and rework axis**, which today has zero rules: verification after edits, commit without a check, correction streaks, interruptions, denials, stale context, plan-before-feature, review-before-PR, waiting-on-you.
- **Make the token axis exact**: cache countdown and named miss causes from Claude Code's own ledger; exact compaction accounting; cost of continuing at the current context; harness overhead; subagent spend; the price of a model or effort switch before it lands.
- **Agree with Claude Code**: same autocompact threshold math, same `/usage` limit-weight formula and behaviour flags, same `/context` suggestion thresholds, same error taxonomy, same footer wording.
- **No nagging**: at most one new nudge per turn, three urgency classes, cooldowns, acted-detection, suppression in bot loops and machine turns.
- **One implementation, three surfaces**: `cctop query coach` feeds the TUI view, the pane Overview and an MCP tool.

## 3. Research findings

Everything below was checked on this machine against Claude Code 2.1.269 (the binary, the generated `claude-code.d.ts`, real transcripts, the status-line payloads the shim already stores, the hook spool, `~/.claude`) and against the official docs and CHANGELOG (2.1.199–2.1.269). Reader ids in brackets point to the research trail in §14.

### 3.1 Where the money goes (81 sessions, priced at `pricing.toml`)

| Fact | Number | Consequence |
|---|---|---|
| Cost split of main-thread spend ($954) | cache reads 66 %, cache writes 24 % (cold rebuilds $130 of it), output 10 % (thinking 2.7 %), fresh input 0 % | The two levers are *what enters context and when* and *whether the cache stays warm*; output tokens are a distant third |
| Re-read multiplier | every token added to a session is re-read on average **106×** (1.55 G token-calls over 14.6 M tokens of final contexts) | A 5 k-token tool result mid-session costs $0.07 median in later reads; advice must be ranked by tokens × remaining calls, not tokens |
| Context-size gradient | calls above 300 k context are 26.5 % of calls but **53.6 % of spend**; a turn at > 600 k costs 18× a turn at < 100 k ($7.32 vs $0.41); the top 5 % of turns (ctx p50 472 k, 30 API calls) are 42 % of billed input | The single highest-value coach line is "cost of continuing at this context" with a checkpoint to `/clear` |
| What context is made of (token-calls) | model-written tool inputs 22.7 % (Bash command text alone 15.7 %), fixed prefix 22.3 %, tool results 17.7 % (Bash 11 %), retained thinking 9.8 %, harness-injected attachments 9.7 %, assistant prose 3.7 %, user prompts 1.0 %, unattributed 12.9 % | cctop shows one "messages" number; three of the top five slices are invisible today |
| Cold calls | 1.8 % of calls, **14.6 % of spend**; causes: TTL expiry after > 1 h gaps 50, session start 36, model switch 10, in-session 20 (7 right after `ToolSearch`, 6 mid-turn listing re-injection) | Every gap > 60 min went cold (49/49), median re-cache 183 k tokens; a keep-alive costs ~5 % of the rebuild it prevents ($0.45 vs $8.72 at 894 k) |
| Cache TTL | 1 h on 12 292 calls, 5 m on 694; this session flipped from 1 h to 5 m mid-way (37 → 75 calls) when the account went onto extra usage at 125 % of the 5-hour limit | The TTL is per request and per plan state, not per session; the coach must read `prompt_cache.ttl` |
| Subagent spend | 13.7 % of total ($152), **0.8 % of subagent output returns** to the main context; this session ran $118 in 41 workflow agents vs $10 in the main thread; cctop listed 0 of them | `agents.rs` scans one directory level; workflow agents live one deeper |
| Exploration | 32.5 % of calls, 28 % of wall time, **51 % of all context growth**; explore turns add p50 11.9 k / p90 93 k; a main-context run of ≥ 8 read-only calls adds p50 48.7 k for good, an Explore agent returns p50 71 tokens | Under auto mode 70 % of calls are Bash and Grep/Glob never appear, so A05 fires in 4 sessions; a Bash-aware run counter fires in 39 |
| Compaction | one real compaction in 81 sessions (567 k → 230 k, 80.7 s, then the session was abandoned); two sessions ran to 921 k / 953 k uncompacted | On 1 M-window models the lever is context growth and `/clear`, not compaction forecasting; three of the four "compactions" cctop's ≥ 30 % drop rule counts are zero-usage API-error lines |
| Model mix | Opus 5 84.7 % of spend; 10 mid-session model switches, all cold (up to 487 k re-cached, $4.63); `/model` is the most used slash command (75 lifetime) | Warn *before* the switch lands |

Sources: [context-cost-anatomy T01–T17], [session-phase-model PH-08–PH-12].

### 3.2 Where rework shows up (same corpus)

| Signal | Number | Detectable how |
|---|---|---|
| Implement episodes never verified before a commit or the end of the session | **165 of 465 (35 %)**; verified ones reach a check in p50 2 calls / 31 s | Bash class of the following calls |
| Commits with no test/build in the previous 10 calls | **175 of 548 (32 %)**; 5 commits right after a failing check | `gitOperation.commit` + Bash class |
| Mid-turn steers absorbed into the running turn | 63 (6× the interrupts) | `queue-operation.reason = absorbed_mid_turn`, `queued_command.origin.kind = human` |
| Interruptions | 11 in 760 turns; 9 within the first 5 tool calls; re-prompt within 5–66 s | user text `[Request interrupted by user…]` + `interruptedMessageId` |
| Permission denials | 76 of 283 tool errors (27 %): auto-mode classifier 43, settings rules 29, user rejected 8, classifier unavailable 3 | `toolDenialKind` on the tool-result line (exact) |
| Same-file churn | 15 files edited ≥ 8× per session (max 76), 37 turn/file cases ≥ 4 edits, 67 edit→re-read→edit sequences | tool inputs, `file-history-delta.backup.version` |
| Serial one-tool round-trips | 78 runs of ≥ 5 single-tool messages; Claude Code itself injected 285 batching reminders | assistant content shape; `batching_reminder_sent` attachment |
| Failure cascades | 40 consecutive failing pairs, 29 blind same-command retries (13 failed again; `gh` 10 of 12) | consecutive `is_error`; exit code text |
| Edit-string failures | **0** "String to replace not found" in 461 edits; the live signal is `staleRecovered` (7) and `staleReadFileStateHint` (24) | `toolUseResult` fields |
| Files edited in the IDE mid-session | 124 `edited_text_file` injections (82 `.md`), ~1.9 k tokens each | attachment |
| Keyword "correction" detection | 186 of 673 prompts hit broad markers, **6** are real corrections; corrections do not follow visible tool errors (1 of 74) | Do not ship keyword rules; use the structural markers above |
| Question-ended turns | 145 of 760 (19 %); after `AskUserQuestion` the wait is p50 213 s but mean 44 min | last text block ends with `?`; pending `AskUserQuestion` |
| Task boundaries | commits are every ~2 turns and the next turn keeps working on the same files 81 % of the time; only `TaskUpdate(completed)` (70 % new files next) and question-ended turns (89 %) mark real boundaries | composite detector, quiet wording |
| Long autonomous runs (> 20 calls) | 17 % of tool turns, 54 % of billed input; end in text 94 % of the time, same error density as short runs | Do not coach "interrupt"; show cost of the run |

Sources: [rework-signals-mining RW-01–RW-21], [session-phase-model PH-04, PH-06, PH-07, PH-13, PH-14].

### 3.3 Data cctop already has on disk but never reads

No new collector, no `cctop install` change, no new Claude Code feature.

| Source | Field(s) ignored today | What it unlocks |
|---|---|---|
| Status-line JSON (`~/.cctop/status/<id>.json`; the shim stores the whole payload) | `prompt_cache.{warm, ttl, expires_at, hit_ratio, misses, expected_rebuilds, recache_tokens_if_cold, miss_recache_tokens, last_miss_cause.causes[], miss_causes{}}` (v2.1.251; causes v2.1.260) | A **cache countdown with the price of going cold**; A01 on real misses with the named cause (`tools_changed`, `system_prompt_changed`, `model_changed`, `messages_rewritten`, `ttl_expired_5m/1h`, `likely_server_side`) |
| Status-line JSON | `effort.level`, `thinking.enabled`, `fast_mode`, `context_window.{current_usage, total_output_tokens, remaining_percentage}`, `exceeds_200k_tokens`, `cost.*`, `rate_limits.spend_limit`, `session_name`, `prompt_id`, `pr.*`, `worktree.*`, `version` | Exact effort/thinking/fast for A08; `prompt_id` joins hooks, status samples and transcript; `plan` is never populated (no such key) — drop the slot |
| Hook spool (`~/.cctop/events/<id>.jsonl`) | `PostToolUse.duration_ms` (excludes permission and hook time; verified present), `PostToolUseFailure.{error, is_interrupt}`, `Stop.{background_tasks[], session_crons[], last_assistant_message}`, `SessionStart.source` + resume fields `{seconds_since_last_response, context_tokens, prompt_cache_likely_expired, estimated_cache_write_usd}`, `Notification.notification_type` (`permission_prompt`, `idle_prompt`, `quota_auto_resume_*`), `effort.level`, `permission_mode`, `prompt_id`, `agent_id/agent_type` on every event | Exact tool durations in one event (drop the `≈`); background tasks (the `~/.claude/tasks` dir cctop scans holds only `.lock`/`.highwatermark`); hard boundaries for the hand-off rule; Claude Code's own cold-resume dollar figure; WAITING in auto mode where `PermissionRequest` never fires (0 in 7 of 8 spools) |
| Transcript `message.diagnostics.cache_miss_reason` (assistant lines, 2.1.220+) | `{type, cache_missed_input_tokens}`; local: `model_changed` 23 (3.04 M tokens), `tools_changed` 22 (1.75 M), `messages_changed` 13 (0.6 M), `previous_message_not_found` 122, `unavailable` 7 | The API's own attribution of every cold call; replaces A01's byte heuristic entirely |
| Transcript `toolUseResult` (parsed as raw JSON, never consumed) | Bash: `interrupted`, `timedOutAfterMs`, `backgroundTaskId`, `persistedOutputPath/Size`, `returnCodeInterpretation`, `staleReadFileStateHint`, `gitOperation{commit,push,pr,branch}` (227 local), `bashEditDiff`; Edit/Write: `structuredPatch`, `staleRecovered`, `userModified`, `originalFile == null` (new file); Read: `type: file_unchanged`, `truncatedByTokenCap`, `numLines/totalLines`, `file.dimensions`; Agent: `usage` (incl. `thinking_tokens`, TTL split), `toolStats`, `resolvedModel`, `totalToolUseCount` | True runaway size ("produced 724 KB, 8 k kept"); suppress A10 when already backgrounded; git milestones without shelling out; exact A14; `file_unchanged` re-reads inject ~30 tokens and must not count for A04; image token estimates |
| Transcript user-line keys | `promptId` (exact turn identity; the current heuristic over-counts prompts by 13 %), `promptSource` (`typed`/`suggestion_accepted`/`queued`/`system`/`sdk`), `origin.kind`, `toolDenialKind`, `userFeedback`, `interruptedMessageId`, `isCompactSummary`, `turnCompanion`, `sourceToolAssistantUUID` | Human turns vs machine turns; denials by kind; interrupts joined to the exact API call; compaction summary excluded from turn counts |
| Transcript assistant-line keys | `perTurnEffort` (2.1.269), `attributionSkill/Plugin/Agent/McpServer`, `isApiErrorMessage` + `error` + `apiErrorStatus` + `quotaLimits{rateLimitType, resetsAt, lowPriorityRetryAfterSeconds}`, `gitBranch`, `apiBlockIndex`, `message.model == "<synthetic>"` with zero usage on error lines | Per-turn attribution like `/usage`; **error lines currently become the "current model" and a fake compaction** in cctop; exact rate-limit state without the shim |
| Transcript line types dropped as `Unknown` | `system/compact_boundary` (`compactMetadata{trigger, preTokens, postTokens, cumulativeDroppedTokens, durationMs, preservedMessages}`), `system/local_command` (captures `/context` stdout with the official category table), `system/scheduled_task_fire`, `pr-link`, `custom-title`, `file-history-delta{trackingPath, backup.version}`, `agent-setting` | Exact compaction accounting; official prefix breakdown when the user runs `/context`; loop wakeups; PR number; per-file checkpoint version as a churn signal |
| Transcript `attachment` subtypes (4 of 45 parsed) and `rendered[].content` (2.1.266+) | `task_reminder{itemCount}` (up to 5.6 k tokens per injection), `edited_text_file`, `hook_success{command, durationMs, exitCode}`, `hook_blocking_error`, `plan_mode`/`plan_mode_exit{planFilePath}`, `goal_status{tokens, iterations, met}`, `queued_command{commandMode, origin}`, `batching_reminder_sent`, `silent_turn_reminder`, `read_truncation_notice`, `auto_mode`/`auto_mode_exit{bashFirst}`, `prompt_snapshot{systemPrompt[], tools[{name, schema}]}`, `instructions{files}`, `nested_memory`, `invoked_skills`, `total_tokens_reminder` (one per tool result, ~26 tokens) | **Harness overhead per turn** (median 2.3 k tokens; 9.7 % of all re-read tokens); hook latency and blocks without `cctop install`; plan-mode state; goal spend; steers vs notifications; Claude Code's own "you are issuing serial single-tool calls" and "you have been silent" signals; the exact system prompt and tool schemas that make up the unattributed 86 % of the prefix |
| Subagent transcripts under `subagents/workflows/wf_*/` | whole directory | 41 workflow agents / $118 invisible in this session |
| `~/.claude/history.jsonl` | `display` (every typed prompt incl. slash commands), `pastedContents` (`+N lines`), `project`, `sessionId`, `timestamp` | The only record of `/clear` (56 lifetime), `/compact` (18), `/model` (75), `/btw` (15), `/rename`, `/goal`, `/loop`, `/effort`; transcripts hold 6 `local_command` lines in 81 files. Paste size for A11 without content |
| `~/.claude/usage-data/session-meta/*.json` (written by `/insights`; deterministic, algorithm recovered from the binary) | `user_interruptions` (marker `[Request interrupted by user`), `user_response_times[]` (2–3600 s filter), `tool_errors`, `tool_error_categories` (ordered substrings: Command Failed / User Rejected / Edit Failed / File Changed / File Too Large / File Not Found / Other), `git_commits/pushes`, `files_modified`, `lines_added/removed` | Claude Code's own rework taxonomy; cctop can compute the same counters live and compare to the person's history. Never display `first_prompt` |
| `~/.claude/usage-data/facets/*.json` (`/insights`, Haiku-derived, stale until re-run; last 2026-08-16) | `friction_counts` (local: wrong_approach 13, tooling_loop 8, buggy_code 6, user_rejected_action 6, excessive_changes 4, …), `outcome`, `session_type`, `user_satisfaction_counts`, `primary_success` | A per-project friction profile at attach time to bias which nudges the coach favours. Never display `underlying_goal`/`brief_summary` |
| `~/.claude.json` | `projects[cwd].last{Cost, Duration, ModelUsage{thinkingTokens}}`, `lastSessionMetrics.hook_duration_ms_{p50,p95,p99}` (Claude Code's own measurement of hook cost: cctop's hooks p99 31 ms), `pluginUsage/skillUsage/toolUsage{usageCount, lastUsedAt, lastUsedNumStartups}`, `numStartups`, `oauthAccount.{userRateLimitTier, hasExtraUsageEnabled}` | "Previous session here: $0.16, 75 s"; plugins enabled but unused for N startups; the plan tier the status line lacks |
| `~/.claude/plugins/plugin-catalog-cache.json` | per-plugin `tokens[model].{always_on, on_invoke}` for 204 official plugins | Exact always-on cost per plugin (median 371, max 8 752 tokens per request) |
| `~/.claude/stats-cache.json`, `plans/<slug>.md` (joined by the transcript `slug`), `file-history/<session>/<hash>@vN`, `teams/session-*/config.json`, `sessions/<pid>.json` (10 of 20 keys unread: `statusUpdatedAt`, `bridgeSessionId`, `nameSource`, `procStart`) | | Personal baselines, plan-exists-before-first-edit, rewind points and churn versions, teammate roll-up, idle-since per other session |

Sources: [transcript-fields T01–T40], [claude-home-files HS-01–HS-20], [cctop-source-audit UNS-01–UNS-14, GAP-01–GAP-08], [docs S01–S03, H01–H14].

### 3.4 Data available only through optional sources cctop does not subscribe to

| Source | What | Cost to enable | What it unlocks |
|---|---|---|---|
| Settings hooks (cctop registers 12 of 33 events) | `PostCompact{compact_summary}`, `PreModelSwitch/PostModelSwitch{from_model, to_model, context_tokens, prompt_cache_warm, cache_ttl, estimated_cache_write_usd}` (v2.1.251), `StopFailure{error ∈ rate_limit, overloaded, max_output_tokens, billing_error, …}`, `PermissionDenied{reason}`, `InstructionsLoaded{file_path, memory_type, load_reason}`, `PostToolBatch`, `UserPromptExpansion{command_name, prompt}` (skill expansion size), `TaskCreated/TaskCompleted`, `ConfigChange`, `FileChanged` | One `cctop install` diff; all observe-only | The priced cost of a model switch **before** it happens; the API error that killed a turn; nested CLAUDE.md loads as the cause of a cache-write spike; tokens per skill invocation; task boundaries |
| Hook `PermissionRequest.permission_suggestions[]` and `PostToolUse.tool_response` for `tool_name == Agent` (currently stripped) | The exact allow-rule Claude Code's dialog proposes; `resolvedModel`, `totalToolUseCount`, `usage` | Keep two small keys when spooling | A09 prints the rule verbatim instead of "add Bash to the allow-list" (unsafe advice as written); A14 exact |
| OTel (`--otlp`, shipped) | `api_request.{cost_usd, query_source ∈ main/compact/<agent>, speed, effort}`, `tool_result.{tool_result_size_bytes, decision_source, error_type}`, `api_error.{status_code, attempt}`, `code_edit_tool.decision{accept/reject, source}`, `active_time.total{user/cli}`; TTFT only on the `claude_code.llm_request` **span** (beta tracing, `/v1/traces`), not on `api_request` events — cctop's TTFT stays empty on a real install | Env block cctop prints; set `OTEL_METRIC_EXPORT_INTERVAL=5000`, `OTEL_LOGS_EXPORT_INTERVAL=1000` | Exact compaction cost, exact result sizes, edit acceptance rate (the ecosystem's headline outcome metric), human-vs-model time |
| Function-hooks module (early access; pane PRD) | `turn.step` per-request `usage` + `stopReason` (`compaction`, `model_context_window_exceeded`, `max_tokens`); `turn.complete.{usage, reason ∈ answer/aborted/refusal/error}`; `tool.check → {decision, rule}`; `prompt.submit{text, turnId, wait, origin.kind}`; `session.compact{trigger incl. precompute, tokensBefore/After}`; `prompt.section`/`prompt.context`/`tool.describe` (the exact fixed prefix by part); `$.session.messages()` (live post-compaction context by tool); `$.settings.read`; `$.fs.ancestors` | Module accepted once in `/plugin`; `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1` | A cache miss toasted at the request that caused it; permission repeats with the deciding rule; paste size before the tokens are spent |
| Function-hooks display channels (zero model cost) | `$.ui.status(text)` (one pinned line under the prompt), `$.ui.toast`, `$.ui.notice(tool_use_id, text)` (inside an open permission dialog), `ui.render` of `PromptHint.hint`, `SessionMode.modes`, `AbovePrompt` (a band of `Button`s with digit hotkeys), `$.prompt.suggest` (dim Tab suggestion), `$.prompt.fill` | Same module | The coach line inside Claude Code's own screen below 110 columns; one-Tab acceptance of `/compact <focus>` |
| `claude agents --json` | `status ∈ busy/waiting/idle`, `waitingFor ∈ permission prompt/input needed/sandbox request/worker request/dialog open` | Documented as the supported external session-state source | Richer WAITING than the PermissionRequest-derived pill |
| `claude --debug-file <path>` | `[PROMPT CACHE BREAK] [source=…, cache read: N, creation: N]` with cause strings (`possible 1h TTL expiry`, `effort changed`, `defer_loading presence flipped`, `message history mutated at index N`, tools added/removed); `autocompact: tokens=… effectiveWindow=…`; `Autocompact is thrashing` | User starts Claude Code with the flag | The break reason when the status line says `unknown` |

Sources: [function-hooks-api E01–E25, A01–A13, S01–S08], [docs H01–H14, O01–O04].

### 3.5 Facts about 2.1.269 that change existing cctop metrics

| cctop assumes | Claude Code does (recovered from the binary or docs) | Consequence |
|---|---|---|
| Autocompact at 80 % of the window until observed | threshold = **effective window − 13 000** (the debug log reports `effectiveWindow=980000` on a 1 M model, so the threshold is 967 000, matching the docs' "~967K"; 187 000 on 200 k windows); `min(effective·pct/100, effective − 13 000)` with `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`; warn band starts 20 000 before; hard block at window − 3 000; precomputed compaction arms at 80 %; `/autocompact`, `autoCompactWindow`, `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `DISABLE_AUTO_COMPACT` override it; the SDK stream carries an `autocompact_state{effective_window, threshold, enforced, source}` frame | `turns_until_compaction` is wrong on every 1 M model; show "% of window used" (Claude Code's own `enforced=false` wording) whenever the effective window is not exact; mirror the three bands and the footer wording |
| Cache TTL is per session | 1 h for the main conversation only on a subscription **within plan usage**; 5 m on usage credits, API keys, and always for subagents/compaction/titles; `promptCacheTtl`, `subagentPromptCacheTtl` override; miss rule: cache read < 95 % of min(prev, cur) input **and** ≥ 2 000 tokens re-processed; closed cause set | A TTL flip to 5 m is itself a signal ("you are on usage credits") |
| A01's cause list (hook injecting time, CLAUDE.md edit, tool order) | Documented invalidators: model switch, effort change (except Fable 5.1 ≥ 2.1.260), fast mode on, non-deferred MCP connect/disconnect, bare tool deny rule, compaction, image-batch eviction, upgrade. **Keepers:** file edits, CLAUDE.md edits mid-session, permission mode, output style, skills, `/recap`, `/rewind`, subagents | Two of A01's three causes are cache-keepers; use `cache_miss_reason` / `last_miss_cause` |
| Compaction = ≥ 30 % context drop | `system/compact_boundary` with exact metadata; `/clear` starts a new session file (v2.1.211); API-error lines carry zero usage | Three of four detected "compactions" are error lines; A06 churn is false |
| A07: MCP schemas ride along in every request | Tool search is on by default since 2.1.221; `deferred_tools_delta` lines are ~34 bytes per tool (names only); the largest local server measures 1 030 tokens, so the > 2 k gate never fires; loading a deferred tool via `ToolSearch` rewrites the whole cached prefix (46 of 109 calls re-cached > 2 k tokens; worst 318 k) | A07 is dead; the live MCP cost is the name listing plus ToolSearch-triggered rebuilds |
| A17: prefix share > 25 % of window | 1 M windows make that 250 k; live prefix 57 k of 1 M; 86 % of it unattributed; the `/context` capture says system prompt 10.2 k, system tools 31.2 k, skills 4.8 k; `prompt_snapshot` gives tool schemas by name | Express prefix as cost per turn; attribute it |
| A11: uncached `input_tokens` per turn > 5 k = a paste | Every tool round-trip bills its new suffix as fresh input | Use the turn's first API call or `pastedContents` |
| A09 keyed on tool name; A15 on identical 30-char args, session-lifetime | `PermissionRequest` never fires in auto/acceptEdits modes; `toolDenialKind` is exact; failures are git/gh plumbing, retried blind 29× | Key denials on kind and command prefix; window the loop rule |
| A18 waits for "a natural boundary" | no boundary detector exists; Stop payload lists background tasks; `TaskUpdate(completed)` and question-ended turns are the only above-baseline boundary signals | Composite, context-graded checkpoint (§5) |
| Rules are session-lifetime, ranked by raw tokens | one incident pins the top slot; Avoids-ranked rules pin forever | Urgency classes, windows, retirement, acted-detection (§6.3) |
| `~/.claude/tasks/session-*/` lists background tasks | holds only `.lock` and `.highwatermark`; the task list is the `task_reminder` attachment and `Stop.background_tasks` | `tasks.rs` would list dotfiles as tasks |
| Bash output truncation | inline cap 30 000 chars (`bashOutputMaxChars` 4 000–128 000), spill to `tool-results/` with `persistedOutputSize`; MCP warn 10 k / spill 25 k tokens; per-message tool-result budget persists over-budget results; `keepRecent=5` tool-result clearing when ≥ 20 000 tokens would be saved (writes a hidden `microcompact_boundary`; never observed locally) | A03 can quote produced vs kept; do not count cleared results as still in context |
| Harness reminders are free | `total_tokens_reminder` after every tool result (padded countdown from 15 M, ~26 tokens), `silent_turn_reminder` after 5 silent turns (max 3), batching tip when a multi-entry tool got one entry, `bash_output_audience_note` per Bash call in auto mode, `task_reminder` every 5 turns with a full list every 5th; all cache-written once and re-read until compaction | 9.7 % of re-read tokens; count as "harness" in the context bar; some are switchable (`CLAUDE_CODE_TOTAL_TOKENS_REMINDER=off`) |
| Per-model pricing | catalog carries `effort_cost_index` per model (Fable 5.1: low .75 / medium .86 / high 1 / xhigh 1.38 / max 1.74; Sonnet 5: .47 / .74 / 1 / 2.41 / 5.59), Fable 5.1 cache read $0.25/M, `/usage` limit weight = (cached + uncached×10 + cacheCreate×12.5 + output×50) × tier (fable 10, opus 5, sonnet 3, haiku 1), behaviour flags at ≥ 10 % (cache_miss > 100 k uncached, long_context > 150 k, subagent_heavy, high_parallel ≥ 4 sessions/5 min, cron ≥ 8 h) | Price effort advice; explain why the limit bar moves faster than dollars; reuse Claude Code's own flag wording |
| `/context` suggestions | per-tool thresholds: ≥ 15 % of window **and** ≥ 10 k tokens (Read 5 %), memory ≥ 5 % / 5 k, context ≥ 80 %; savings factors Bash 50 %, Read/Grep 30 %, WebFetch 40 % | Align A03's thresholds so cctop and `/context` agree |

Sources: [binary-strings BIN-01–BIN-40], [docs C01–C10, E01–E06, L01–L02].

### 3.6 What the ecosystem shows that cctop lacks

Thirty tools were compared (ccusage, Claude-Code-Usage-Monitor, ccflare, claude-code-otel, cc-sessions, claude-mem, sniffly, session-signals, claude-code-stats, claude-cost, burnstop, ccboard, ccstat, tokscale, Trimly, Prompt-sensei, fbl-ai, claude-coach-plugin, HowMuchClaude, Cursor/Copilot/Codex/Cline/Aider dashboards, and Claude Code's own `/usage`, `/insights`, `/stats`, `/context`, `/doctor`, `/skill-doctor`). Nearly all are retrospective cost aggregators; none coaches per turn. cctop's real competition for a coach is Claude Code itself (`prompt_cache` diagnosis, `/usage` behaviour flags with tips, `/doctor`, `/insights` "Suggested CLAUDE.md additions", spinner tips that are product cross-sell rather than efficiency coaching). Gaps worth closing: cache-cold countdown and cause-named misses; interruption rate and categorised tool errors (sniffly's 15 regex classes, session-signals' rephrase storms and failure cascades); edit acceptance rate; steps per prompt; per-turn attribution to skills/agents/MCP; today-across-sessions $ and 5-hour-block projection against a personal P90; human-vs-model-vs-waiting time; $ per commit; repeated correction → CLAUDE.md line; prompt specificity; a hard budget with time-to-cap. Source: [competitor-scan F01–F30].

### 3.7 Practices with an observable signal

Anthropic's docs and engineering posts and Boris Cherny's published workflow converge on one constraint ("the context window fills fast and performance degrades as it fills") and one quality lever ("give Claude a way to verify its work", quoted as 2–3× on result quality). Forty-one practices were catalogued; cctop covers about twelve, all on the token axis. The deterministic signals for the rest are the basis of §5 rules A32–A52. Source: [best-practices-signals P01–P41].

## 4. What to add to the main dashboard

Every item below is a pure function of data in §3.3 or §3.4; the metric registry gets one row per item. With the direction-B dashboard (v1.2) each panel has two homes — its two-line ledger row on the dashboard and its full-screen view behind the digit — and the additions below land in both; the framed-block layouts of the base PRD are gone.

| Panel | Addition | Data | Replaces / fixes |
|---|---|---|---|
| Header | session title (`ai-title`/`custom-title`), PR number (`pr-link`), effective effort (`perTurnEffort` › `effort` › status `effort.level`) with thinking on/off and fast mode, permission mode incl. `plan`, "prev. session here: $X, N min" (`~/.claude.json`) | transcript, status, `~/.claude.json` | `plan` slot (never populated) |
| Context | **stacked bar** prefix / tool inputs / tool results / thinking (retained) / harness / prose; "harness 2.3 k/turn" row; autocompact threshold = window − 13 000 with `Autocompact buffer` and `precompute armed` markers; **exact compactions** from `compact_boundary` (pre → post, seconds, trigger); boundary markers for `/clear`, resume, compact; a defensive parser branch for a future `microcompact_boundary` line and `[Old tool result content cleared]` results (no UI until observed; 0 in 113 transcripts); "files re-read since boundary" | transcript attachments `rendered`, `compact_boundary`, `SessionStart.source`, env of the claude pid | ≥ 30 % drop heuristic; 80 % threshold; `<synthetic>` lines counted as compactions |
| Tokens & Cost | **$ per call at current context** and **$ per turn vs at 100 k**; "cost of the next 30 calls"; cache line: `warm · expires 41:12 · TTL 1h · misses 2 (model_changed 427 k) · expected rebuilds 1`; TTL source; "where the tokens went: skill code-review 31 % · workflow agents 40 % · mcp:chrome 9 %"; idle-triggered turns priced separately; **agents $X (N % of session)** incl. workflow agents; auxiliary spend gap vs `cost-state` at session end; limit weight per turn with Claude Code's five behaviour flags; model-mix counterfactual at session end only | status `prompt_cache`, `attribution*`, `promptSource`, subagent dirs, `cost-state.modelUsage`, pricing + `effort_cost_index` | A01/A02 heuristics; missing agents; `cache_ttl` shown nowhere |
| Limits | `quotaLimits` from 429 lines (exact `resetsAt`, `rateLimitType`, low-priority retry); spend_limit window; personal ceiling from P90 of past 5-hour blocks when `rate_limits` is absent (API-key users); other sessions idle-since (`statusUpdatedAt`) | transcript, status, registry | Limits panel empty without the shim |
| Turn | phase word + run length (§6.2); `edits N ✓ last check` ; steers (human, absorbed mid-turn) vs task notifications; `AskUserQuestion` / question-ended WAITING with elapsed; interruptions this session with wasted output tokens; hook time **by command** (`hookInfos[].command`, `hook_success.durationMs`); `preventedContinuation`; background tasks from `Stop.background_tasks`; `pendingBackgroundAgentCount`; goal progress (`goal_status.tokens/iterations`); exact tool durations from `duration_ms` (drop `≈`) | transcript, spool | permission-only WAITING; `tasks.rs` dir listing; summed hook time |
| Tools | **IN→CTX** column (tokens the model wrote into tool inputs; Bash command text is 35 % of output); error class column (Claude Code's six classes + Content Not Found / Timeout / Tool Not Found); Bash rows split by class (explore / implement / verify / commit / gitread / ops / wait); image tokens (~w·h/750) and offloaded bytes ("produced 724 KB, 8 k kept"); truncated-at-cap marker; context tax on each large result ("re-read ~100× ≈ $0.30") | transcript `toolUseResult`, content shapes | len/4 blind to images and spills; Bash as one row |
| Agents & MCP | workflow agents (`subagents/**`), per agent model/context/calls/output/TTL, failed phases from the workflow journal, forks with inherited context, teammates from `teams/`; MCP servers needing auth (`mcp-needs-auth-cache.json`, `needsAuthMcpServers`); `ToolSearch` loads per server; "N/20 subagents, depth d/3" | subagent dirs, journal, `~/.claude.json .mcpServers` | one-level scan; `mcp_settings.json` expectations |
| Files | edits per turn per file; checkpoint version per file (`file-history-delta`, v8+ flagged); "uncommitted: +412/−87 across 9 files · last commit 42 min / 18 edits ago" (`gitOperation`); re-read column that counts Bash `cat`/`sed -n` and excludes ranged reads and `file_unchanged`; stale markers (`edited_text_file`, `staleReadFileStateHint`, `staleRecovered`); rewind points available since prompt #k and what Bash-made changes they cannot undo | transcript | A04 counting the fix it recommends |
| Events | `API error: rate_limit 429 · resets 22:20 · low-priority retry 20 s`; `cache miss: model_changed 427 k`; `cache miss: tools_changed 126 k (ToolSearch)` on CLI < 2.1.267; `compacted auto 567 k → 230 k in 80.7 s`; denial (kind, reason); interrupt (after N calls, T output tokens); destructive git op; stale-file hint; `/model`, `/effort`, `/clear` from `<command-name>` lines and history.jsonl | transcript, history.jsonl | error lines rendered as `Compact ctx → 0` |
| Prefix inspector | system prompt sections by name and size (`prompt_snapshot`), tool schemas by tool, skills listing vs the 1 % budget (`window × 4 × 0.01` chars), CLAUDE.md/AutoMem from `instructions.files`, `nested_memory` re-injections, plugins with always-on tokens and "unused for N startups", `/context` official table when captured | attachments, catalog cache, `~/.claude.json` | 86 % "other" |
| Ledger | per-turn columns: tool errors, tool calls, prompt chars, harness tokens, cache miss cause, effort, phase mix, human vs machine turn, cost at this context vs at 100 k | in `State` already (UNS-04) plus new parsers | — |
| Session start line (once, dim; only when a `/insights` report for this project is < 30 days old) | "insights 08-16, 40 sessions: wrong_approach 11 · bugs 6 · unchecked commits 32 %" — one line, no habit text; the live counters (interruptions, error classes, response times) are recomputed by cctop per project instead of read from the stale files | `usage-data/*`, live counters | — (C051: refuted as a nudge, kept as a line) |

Turn identity for every per-turn metric moves to `promptId` with `promptSource ∈ {typed, suggestion_accepted, queued}` or `origin.kind == human` as the definition of a human turn (fixes the 13 % over-count; excludes interrupts, slash commands, task notifications, teammate messages, the compaction summary).

## 5. Advisor rules: fixes and additions

### 5.1 Fixes to A01–A18 (calibrated against the corpus)

| Rule | Change |
|---|---|
| A01 Cache miss | Fire on `prompt_cache.misses` (not `expected_rebuilds`) or `diagnostics.cache_miss_reason` with ≥ 20 k missed tokens; name the cause; drop the byte heuristic and the two cache-keeper causes |
| A02 Cache expiry | Becomes preventive: A19 countdown; the post-mortem version retires after 3 turns |
| A03 Runaway result | Thresholds aligned with `/context` (≥ 15 % of window and ≥ 10 k per tool type; single result ≥ 1.8 k tokens = top 5 %); quote `persistedOutputSize` ("produced 724 KB, 8 k kept") and the re-read tax; window to "since last boundary"; exclude cleared results |
| A04 Re-reads | Count Bash `cat`/`sed -n`/`head`/`tail` as reads; exclude ranged reads and `file_unchanged`; reset on `edited_text_file`, `staleReadFileStateHint`, boundary; flag edit→re-read→edit instead |
| A05 Explore in main | Bash-aware read-only run (A25); current run only; reset on Agent spawn |
| A06 Compaction churn | Exact `compact_boundary`; exclude `<synthetic>` lines and `/clear`; "will re-trigger" when post-compaction context ≥ threshold − 20 k; on 1 M models re-target at growth (A23) |
| A07 Idle MCP | Re-base on listing bytes + `ToolSearch` rebuilds (A22) + plugin always-on tokens; "never used since install" from `~/.claude.json` counters |
| A08 Thinking share | Read `perTurnEffort`, status `thinking.enabled`; price with `effort_cost_index`; count retained thinking in context; routine = edit-heavy, not tool-count; recommend `/effort medium s` at a boundary, note the cache cost on non-Fable models |
| A09 Permission waits | Key on `toolDenialKind` and command prefix; print `permission_suggestions[].ruleContent` verbatim; never suggest bare `Bash`; use `Notification.permission_prompt` for WAITING (A45) |
| A10 Long foreground | Exclude `run_in_background`, `backgroundTaskId`, `timedOutAfterMs` (auto-backgrounded); window to last 5 turns |
| A11 Fresh-input spike | Use the turn's first API call `input_tokens` or history.jsonl `+N lines`; never the turn sum |
| A12 Chatty turns | Human turns only (`promptSource`); exclude slash commands and interrupts |
| A13 Rate-limit pacing | Recompute per tick; add `quotaLimits` on 429; Anthropic's own levers (`/model sonnet ~2× runway`, `/effort medium`) |
| A14 Subagent model | Use `resolvedModel` + `toolStats` (search-heavy, zero edits); see workflow agents; Explore agents already default cheaper on 2.1.269 |
| A15 Error loop | Replaced by A38 (consecutive, windowed, exit-code aware) |
| A16 Hook overhead | Name the hook (`hookInfos[].command`, `hook_success`); exclude `cctop hook`; use `next.trace` in the pane |
| A17 Big prefix | Cost per turn (prefix × cache_read × turns) instead of window share; attribute with `prompt_snapshot`/`/context` |
| A18 No hand-off | Replaced by A42 (composite boundary, context-graded) |
| Engine | Every rule gets `urgency ∈ {NOW, NEXT, LATER}`, `since_turn`, `window_turns`, an `acted` predicate and a cooldown; `Saving` distinguishes one-off from per-turn and ranks by tokens × expected remaining calls; dismissals and `first_fired` persist in `~/.cctop/<session>.advisor.json` so `query`, MCP and the pane see the same state |

### 5.2 New rules

Every candidate was checked by an adversarial verifier on two lenses (data availability on this machine; actionability and false-positive risk). **No candidate failed on data.** 31 failed as *live nudges* and survive as metrics, state-line indicators, Events rows or dashboard-only fixes; the table says which. Wording is the ≤ 2 × 52-column coach text with live numbers; "fires/month" is the corpus frequency for this user after the verifier's tightening. Rules marked *slot* may occupy the coach's single nudge slot; *next-row* rules only appear in the "next" row; *indicator* and *event* never nudge.

**Token axis**

| ID | Name | Trigger (verified form) | Action offered | Evidence | Class | Form | P |
|---|---|---|---|---|---|---|---|
| A19 | Cache-warm countdown | `prompt_cache.warm` and `recache_tokens_if_cold ≥ 50 k` and no turn running; 1 h TTL: `expires_at − now ∈ (0, 300 s]` and idle ≥ 60 s; 5 m TTL: `∈ (0, 120 s]`; and (question/permission pending or idle); once per expiry window; fallback `last call + observed TTL` marked ≈, never when the status file is older than the last assistant line; no countdown shown above 300 s | "cache cold in 4:12 · reply now ≈$0.09, later ≈$1.72" / "re-cache 172 k tok · done? /clear + hand-off note"; 5 m: "cache cold in 1:40 (5 m TTL) · reply or lose 172 k" | 55 of 58 gaps > 60 min were cold; realistic catches 2–4/month on 1 h, every turn on 5 m; a keep-alive costs ~5 % of the rebuild | NOW | slot | P1 |
| A20 | Named cache miss | once per distinct assistant `message.id` when `diagnostics.cache_miss_reason` is non-null and ≥ 20 k missed tokens (or `prompt_cache.misses` increments); TTL misses are Events rows only; `tools_changed` absorbs the former A22 with a "CLI < 2.1.267: `claude update`" hint | "cache miss: model → opus-5 · 487 k re-written ≈$4.7" / "a switch re-writes all context — pick model early" | `model_changed` 23 misses / 3.0 M tokens, `tools_changed` 22 / 1.75 M, `messages_changed` 13 / 0.6 M | LATER | slot (model/tools/messages only) + event | P0 |
| A21 | Switch window | warm: `PreModelSwitch` with `prompt_cache_warm` and `context_tokens > 100 k`, source command/picker (never `auto`), or a history.jsonl `/model`|`/fast` row (`/effort` only on non-Fable-5.1) for this session while `prompt_cache.warm` and `recache_tokens_if_cold > 100 k`; cold hint only when the cache is cold and history shows ≥ 3 switches in this project; **no** plan-exit variant (measured warm, 5/5) | "Switch re-reads 134 k uncached (≈$0.80 sonnet 1 h)" / "cache dies in 38 m: /model back, or switch after /clear"; cold: "Cache is cold: /model or /effort costs nothing now" / "pick one and hold it until the next /clear" | 10 of 10 mid-session switches cold; ~1 per 11 sessions; the value over Claude Code's own warm-cache confirm is the priced figure and the undo | NOW / NEXT | slot | P2 (P1 with hooks) |
| ~~A22~~ | ToolSearch re-cache | *refuted as a nudge* (C004): fires during correct work; folded into A20's `tools_changed` variant | — | 46 of 109 ToolSearch calls re-cached > 2 k | — | event | — |
| A23 | Cost of continuing / context reset | every tick: $/call = ctx × cache_read + median output × output price (subscribers: "each call re-reads 420 k · 5 h 62 %"); escalates only at a **clean turn end** (last block text, `stop_reason end_turn`, `Stop.background_tasks` empty when hooks are live) above 300 k on 1 M windows; projection uses the session's own p50 calls per long run (19 here), never a constant; merges with A42 into one "context-reset" family | "ctx 420 k → ≈$0.23/call, ≈$2.6/turn (was ≈$0.4)" / "clean stop: /compact <focus>, or hand-off + /clear" | calls > 300 k are 25.5 % of calls, 51.6 % of spend; realistic saving $110–140/month, concentrated in ~3 sessions | NEXT | slot (next-row below 200 k) | P0 |
| A24 | Harness overhead | *metric only* (C009): Context row from `rendered[].content`/4 on 2.1.266+ split into session-start bundle vs per-turn; `task_reminder` cost computed exactly from `#id. [status] subject` lines; `n/a` before 2.1.266; nudge only for `task_reminder` itemCount ≥ 10 recurring | "harness: start 9.8 k · since 1.1 k/turn" | attachments 9.7 % of re-read tokens; skills re-sends are ~0.1–0.2 k deltas (1 in 46 sessions) | — | metric | P2 |
| A25 | Exploration run (replaces A05) | Bash-aware read-only run on a narrow allowlist (`cat`, `ls`, `rg`/`grep`, `find`, `head`/`tail`, `sed -n`, `git log/show/diff/status`) plus Read/Grep/Glob/WebFetch; ambiguous read+write commands reset the run; counter shown from call 1; nudge at run ≥ 8 **and** ≥ 20 k context added this run (or ≥ 6 with ≥ 25 k); retires when the run ends or an Agent spawns; A03's single-result line surfaces alongside | "EXPLORING ×8 · +31 k ctx this run" / "queue: 'use an Explore subagent for the rest'" | explore is 51 % of context growth; name-based A05 fires in 0 sessions here (Grep/Glob never used); the delegable remainder is typically ~10 k more (p90 ~45 k) | NEXT | slot | P0 |
| A26 | Model-written tool inputs | *display only* (C013): `IN→CTX` per tool; two LATER residues worth a fixture: whole-file **rewrite** (Write/heredoc to a path already written, ≥ 1 k) and **read-back** (cat/Read within 3 calls of writing ≥ 500 tokens) | Tools column | tool inputs are 58 % of output tokens; 218 rewrites and 142 read-backs per 113 sessions | LATER | metric (+ fixture) | P2 |
| A27 | Idle-triggered turns | machine turn = `promptSource system` or `origin.kind task-notification` or content prefix `<task-notification`/`<teammate-message`; loop wakeup = `scheduled_task_fire`; priced at ctx × cache_read; arm-time nudge only when `Stop.session_crons` appears (or history.jsonl `/loop`/`/goal`) with ctx > 150 k | "/loop 5 m on 180 k ctx ≈ 9 M cache-read tok/h idle" / "/clear first, widen the interval, or narrow it" | 62 task-notification turns counted as human today; one `/goal` = 115 k tokens | NEXT | slot (once) + state token | P1 |
| A28 | Same-file churn | *indicator* (C021): state-line token `render.rs ×6` at ≥ 6 same-path edits incl. Bash writes or ≥ 2 edit→re-read→edit on non-doc paths; `file-history-delta` is always v1 and useless; snapshot versions lag one turn | state line | 37 turn/file cases ≥ 4 edits; churn concentrates in `.md`/`.yaml` | — | indicator | P2 |
| A29 | Serial round-trips | *counter* (C022): `batching_reminder_sent` is unconditional on Fable (fires on every post-tool call) and must not be a trigger; the run size since the last assistant text is a state-line figure | "18c +42 k · silent 3:50" | 78 runs ≥ 5 single-tool messages | — | indicator | P2 |
| A30 | Cold resume | primary = live idle gap: `now − last_line_at > observed TTL`, ctx > 100 k, last line is an `end_turn` or attachment, no API call since; secondary = spooled `SessionStart` with `source ∈ {resume, fork}` and `prompt_cache_likely_expired` (0 samples locally); below 100 k a cache-light detail only; session-start cold writes labelled "expected" | "Idle 2h14m · cache cold · next msg re-writes 182 k" / "Done? /clear + hand-off note · else /compact (½)" | 55/58 gaps > 60 min cold, 50 of them above 100 k; the 1 M outliers cost $4–8 each | NEXT (exempt from the away suppression) | slot | P1 |
| A31 | TTL flip | *event* (C027): the informational flip only, attributed in order env (`ps eww`) → `promptCacheTtl` setting → shim `five_hour ≥ 100` → API key → unknown; no blanket "set 1 h" advice | "cache TTL now 5 m (over plan usage)" | 3 genuine flips locally; this session flipped after 37 calls | — | event | P2 |
| ~~A48~~ | `/btw` next time | *refuted* (C063): cannot tell a side question from the task; kept only as a `/btw` mention in A41's waiting text | — | — | — | — | — |

**Outcome and rework axis**

| ID | Name | Trigger (verified form) | Action offered | Evidence | Class | Form | P |
|---|---|---|---|---|---|---|---|
| A32 | Verification gap | first Edit/Write/MultiEdit/NotebookEdit or implement-class Bash on a **non-doc** path starts the timer; `✓` resets only on a test-class Bash (`cargo test`, `pytest`, `npm test`, `go test`, `vitest`, `jest`) whose stdout confirms a run, or a Read of an edited file; amber at 10 min or 14 calls; nudge at turn end; suppressed when no test command is known (degrades to the rework light's "no check yet"); pass/fail from `is_error` | "Edited 3 src files, no test/build run yet" / "queue: 'run cargo test, fix failures' or /goal" | 15 of 37 source-edit turns/month end unverified (41 %), 13 stay so next turn; verified episodes reach a check in p50 2 calls / 31 s | NEXT | slot | P0 |
| A33 | Commit without a check | `gitOperation.commit` (or commit-class Bash) while `edits_since_verify > 0` on code paths since the **last commit** (not "last 10 calls") and no test-class call in that window; once per turn; escalated when the latest test result in the window failed; no `PreToolUse`/Esc path (cctop sees the commit too late) | "Committed a1b2c3d with 3 unchecked src edits" / "queue: 'run cargo test; fixup before push'" | ~7 fires per 91 sessions after suppressions (~2/month), 4 escalated; the 32 % figure counted doc edits | NOW (on the commit result) | slot | P2 |
| A34 | Plan-first | typed prompt ≥ 300 chars or ≥ 2 path tokens with an implementation verb, mode ≠ plan, no plan artefact this session, no edit yet; suppressed for markdown-structured or PRD/spec prompts; retires as soon as `AskUserQuestion`/`EnterPlanMode`/`ExitPlanMode` appears (Claude often self-selects within 30 s) | "Feature-sized prompt (2 files, 640 chars), plan off" / "Esc · /plan (Shift+Tab) · resend — or say 'plan first'" | 11 fires per 459 prompts (2.4 %); unproven local benefit; Claude Code's own spinner tip shows the same advice | NEXT | next-row only | P2 |
| A35 | Verification target | *pending re-simulation* (C034): refuted on the untightened trigger; the tightened form (≥ 60 chars, path/PRD/`?` suppression, suppressed when Claude self-verified in the last 3 turns, requires a known test command) must show ≤ 1-in-5 wrong on the 459 typed prompts before admission | "No check named. Queue: 'then run cargo test, show failures'" | 42 fires/459 before tightening, ~40 % unnecessary | NEXT | next-row only (if admitted) | P3 |
| A36 | Correction streak | structural only: `[Request interrupted` text block (not inside tool results, not in subagents) + short prompt; `absorbed_mid_turn` steer < 200 chars starting with a negation or naming a file edited this turn; top-level `toolDenialKind user-rejected` / `userFeedback`; two within 3–4 human turns on overlapping files; keyword variant never sufficient | "2 corrections in a row on the same files (Esc, then steer)" / "Esc Esc → restore turn 14, restate with the constraint"; evidence row "rewind: 3 checkpoints since #12 · bash files not covered" | ~5 fires/month; 13 interrupts, 8 user-rejected, 5 userFeedback; 595 checkpoints and **0 rewinds** (verified on `/rewind` history rows and `parentUuid` branching) | NOW | slot | P1 |
| A37 | Interruption ledger | *accounting only* (C036): skip `[Request interrupted` lines in turn counting; ledger row "interrupts 3 · ~20 k output tokens cut"; optional factual one-tick line without advice | "Cut after 11 calls, 6.5 k out (Bash) · 3 files checkpointed" | 13 interrupts/month, 9 within 5 calls | — | ledger + event | P2 |
| A38 | Failure cascade (replaces A15) | 3 consecutive `is_error` in the last 10 calls of the turn **excluding** denials (`toolDenialKind` routes to A45) and (≥ 2 share the normalised 40-char command prefix or the first 60 chars of the error text), no Edit/Write between | "3 fails in a row: gh pr view ×2 (8), git push (128)" / "Esc, give the missing fact — or run it, paste 20 lines" | ~14 pure-error runs/month; 29 blind same-prefix retries (`gh` 10 of 12 fail again) | NOW | slot | P1 |
| A39 | Unattended run without a check | *re-timed* (C039): no live "user away during a turn" signal exists in transcripts (all `away_summary` lines follow turn end); fires **after** a finished turn with ≥ 20 calls, ≥ 1 edit, no test-class call, no `goal_status`, no Ralph marker, when the user returns (`away_summary`, `idle_prompt`, registry idle ≥ 3 min); re-open as a live rule once an OS-idle collector exists (§13) | "Last run: 24 calls, 3 files edited, nothing tested." / "Ask for a test run, or /goal <end state> + turn cap." | 24 of 1 002 turns qualify | NEXT | slot | P2 |
| A40 | Review before merge | `gitOperation.pr.action ∈ {created, ready}` or a push, PR not merged, `git diff --numstat <merge-base>..HEAD` ≥ 100 non-doc lines, no review marker (Skill code-review/simplify/security-review, review agent, `attributionSkill`); status-line `pr.review_state` when present; the irreversible step is `gh pr merge`, so "before merge", not "before push" | "PR #142 open: +412/−87 in 9 files, no review pass" / "run /code-review before gh pr merge (fresh context)" | 60 of 81 PRs without a review pass; 385 `gh pr` calls vs 36 Skill uses | NEXT | slot (once per PR) | P1 |
| A41 | Waiting on you | `AskUserQuestion`/`ExitPlanMode` tool_use without result, or `Notification.notification_type ∈ {idle_prompt, permission_prompt, agent_needs_input}`, or registry idle with the last text ending `?`; desktop alert only from the first two after 30 s; cache clock shown only when < 10 min remain | "◆ WAITING 4 m · Claude asked a question (2 options)" / "reply now · cache warm 4:12 · cold restart ≈182 k"; else "reply now · a one-word answer unblocks the task" / "or /btw <question> if you need a detail first" | 55 turns with AskUserQuestion: wait p50 90 s, 18 > 5 min, 5 > 60 min; 145 `?`-ended turns | NOW | slot + notification | P1 |
| A42 | Natural-boundary checkpoint (replaces A18; one family with A23) | fires on `TaskUpdate(completed)`, task list emptied, `TaskCompleted` hook, a done/next prompt, or a `?`-ended turn with no `AskUserQuestion` pending; commits are a tie-breaker only (next turn keeps the same files 81 %); requires `Stop.background_tasks` and `session_crons` empty and no pending agents; suppressed in loops; quiet < 200 k, visible 200–400 k, insistent > 400 k | "checkpoint · ctx 312 k · next task unrelated?" / "hand-off note → /rename → /clear (fresh ≈45 k, 7×)" | task-completed 70 % / question-ended 89 % new files next vs 30 % baseline; > 600 k calls cost $0.50 vs $0.07 below 100 k | NEXT | slot | P0 |
| A43 | Destructive git on a dirty tree | Bash `git reset --hard|checkout -- <path>|checkout .|clean -f|stash` (not `stash list/show/pop`, not `restore --staged`) **and** `git_dirty` was true at the preceding poll or `bashEditDiff.changedFiles > 0`; always an Events row; slot + toast only when dirty | "Claude ran git reset --hard (#41) on a dirty tree" / "rewind can't undo bash: check git reflog/stash list" | 38 destructive forms/month; checkpoints do not cover Bash writes | NOW | event / slot when dirty | P2 |
| A44 | Stale-context collision | *residue* (C046): silent A04 reset on `edited_text_file`/`staleRecovered`; nudge only when `edited_text_file.filename` equals an Edit/Write path in the same turn, once per file per turn; failure nudge after ≥ 2 `staleRecovered`/modified-since-read on one path; plain IDE-edit notices are Events only | "IDE + Claude both edited src/x.rs this turn — say which version wins" | 126 IDE edits/month, 66 % on `.md` the person edits deliberately | NOW | slot (collision only) + event | P2 |
| A45 | Denial streak / allow rule (replaces A09's advice) | 2nd `automode-blocked` result for the same tool + argv0 (+ subcommand) within a turn or 3 turns; settings deny rule on the 1st occurrence, once per prefix per session; `user-rejected` excluded (it belongs to A36/A49); rule text = `permission_suggestions[].ruleContent` when spooled, else `Bash(<argv0> <sub>:*)`; never bare `Bash`, never for protected paths, never for rules already allowed | "Auto mode blocked gh api ×2 this turn (3 in a row pauses it)" / "allow Bash(gh api:*) or approve it"; deny rule: "git reset --hard is denied by your rules" / "tell Claude the alternative — it cannot run this" | 83 denials/month (43 auto-mode, 29 rules, 8 rejected, 3 unavailable); retried with a variant in ~1 of 3 cases | NOW | slot | P0 |
| A46 | Long-context drift | context crosses 150 k / 300 k / 450 k for the first time, the last typed prompt ≥ 150 tokens is ≥ 100 k tokens behind, no `instructions`/`nested_memory` attachment since, window is 1 M (200 k windows re-summarise anyway); no `/clear` branch (A42 owns it) | "Ctx 312 k: your turn-2 constraints are 300 k tokens back" / "Restate the 3 that matter, or add them to CLAUDE.md" | 37 of 109 sessions exceed 150 k with one compaction corpus-wide; Anthropic samples instruction-following in long sessions | NEXT | next-row only | P2 |
| A47 | Turn died | assistant line with `isApiErrorMessage` (never `<synthetic>` alone); branch on `error` + `quotaLimits.rateLimitType`/`resetsAt` (session/weekly → countdown; spend limit → none), auth, prompt-too-long (context light ●), server error; excluded from model, velocity and compaction math; exempt from the first-2-turns and away suppressions (13 of 14 limit errors land on a session's first call) | "Rate limit (session) · resets 22:20 (in 32 min)" / "wait, or /model <other family> to keep working"; "Monthly spend limit hit · nothing will run" / "raise the limit, switch account, or stop" | 18 error lines on disk; today they become the current model and a fake compaction | NOW | slot + toast + notification | P1 |
| ~~A49~~ | Repeated correction → CLAUDE.md | *refuted as a live nudge* (C052); kept for the `cctop-insights` skill only, which may offer a CLAUDE.md line after a yes in the session | — | — | — | skill | P3 |
| ~~A50~~ | Formatter hook | *refuted* (C053): frequency unquantified; only pure formatter pipelines ≥ 3/session with no matching hook; dashboard tip at most | — | — | — | tip | P3 |
| ~~A51~~ | Bug fix without a test | *refuted* (C047): keyword bug classification is weak; if ever revived, classify test edits by content and require a tests directory | — | — | — | — | P3 |
| ~~A52~~ | UI edit without a screenshot | *refuted* (C048): post-turn only, `.tsx/.jsx/.vue/.svelte` with a `package.json` project and a browser MCP connected | — | — | — | — | P3 |

Indicators that are part of the coach view but never nudge: **silent-run liveness** (run size since the last assistant text, `silent_turn_reminder` count when the attachment exists, the heuristic only when it is absent for the model/version; C023), **loop health** in `session_mode = loop` (C058), **steer token** for a single queued human steer (C059; the slot only at ≥ 2 stacked steers), **churn token** (A28).

### 5.3 Per family: what it complements in Claude Code, and when the coach stays silent

Claude Code already has a surface for part of what every family watches; the coach adds the priced figure, the undo or the moment, and goes quiet where Claude Code's own text is on screen. The **silent** column is what the engine applies today (`src/advisor/mod.rs`: the mode suppressions, the first-two-turns and away rules for NEXT/LATER, the `tipsHistory` map `TIP_FOR_FAMILY`, and each rule's own gate). Everywhere: no nudge in `session_mode = machine`, `workflow` or `loop`; NEXT/LATER wait out the first two human turns and stay quiet after > 2 h away (cold-resume excepted); a `team` session mutes context-reset, waiting, cold-resume and post-compaction; a `remote` session mutes waiting.

| Family (rules) | Complements in Claude Code | The coach stays silent when |
|---|---|---|
| cache-miss (A01) | nothing user-facing names the cause — `diagnostics.cache_miss_reason` is written only to the transcript, `prompt_cache.misses` only to the status line | the miss is a TTL expiry (an Events row only), under 20 k tokens, or one of the two cache-keeper causes |
| cache-expiry (A02) | the same `prompt_cache` fields | cache-countdown (A19) already fired for the window; three turns after the cold write |
| runaway-result (A03) | `/context`'s "tool results > 15 % of the window" flag and `<persisted-output>` spills | the result was spilled or cleared, or the tool type is under 15 % / 10 k since the last boundary |
| reread (A04) | the `file_unchanged` result and `read_truncation_notice` Claude Code hands the model | the read is ranged or `file_unchanged`; after an `edited_text_file` / `staleRecovered` / boundary reset |
| explore-delegate (A25) | the `subagent-fanout-nudge` / `ctx:too-many-subagents` spinner tips and Explore's own cheaper default model | one of those tips showed in the last ten startups; the run is under 8 calls or 20 k; an Agent spawned; any write resets the run |
| idle-mcp (A07) | `/mcp`, `/plugin` and the `plugin-disuse-review` tip | the tip showed recently; the server or plugin had a call in the last 20 min |
| thinking (A08) | the `config-thinking-mode` / `tab-toggle-thinking` tips and Claude Code's own effort nudge | either tip showed recently; the turns are not edit-heavy; on Fable the cache-cost line is dropped (effort is not in its prefix) |
| fresh-input (A11) | the `[Pasted text #N +L lines]` placeholder in the composer | the turn's first call carried under 5 k uncached tokens |
| chatty (A12) | nothing | slash commands, interrupts and machine turns are not counted; under 20 human turns an hour |
| subagent-model (A14) | Explore agents already default to a cheaper model (2.1.269) | the agent edited anything, or made fewer than five search-shaped calls |
| prefix-tip (A17) | `/context`'s prefix breakdown | under $0.25 per turn and under 100 k of prefix |
| cache-countdown (A19) | nothing — Claude Code shows no cache clock | more than 300 s remain; a turn is running; under 50 k to re-cache; the desktop notification only with a question or permission pending |
| warm-switch (A21) / cold-switch (A21b) | the warm-cache confirm on `/model` (the coach adds the priced figure and the undo) | the switch source is `auto`; `/effort` on Fable 5.1; the cold hint only after three switches in the project |
| context-reset (A23 / A42) | the context-low footer and the autocompact countdown — the coach never shows a pre-compaction countdown; the `showClearContextOnPlanAccept` dialog | under 300 k on a 1 M window and outside the warn band (next row only); a turn is running; background tasks or a loop; a commit alone; A42 quiet under 200 k |
| loop-armed (A27) | the `loop-command-nudge` tip | the tip showed recently; the context is under 150 k; once per arming |
| cold-resume (A30) | the resume-from-summary dialog and `SessionStart.prompt_cache_likely_expired` | under 100 k (a cache-light detail only); a session-start cold write, labelled expected |
| post-compaction (A06) | the compaction summary and the footer's own countdown | the compacted context sits under threshold − 20 k; `/clear` and `<synthetic>` lines never count |
| permission-wait (A09) | the permission dialog and the `permissions` tip | the tip showed recently; the wait is under the bar; never a bare `Bash` rule |
| long-foreground (A10) | Claude Code's own auto-backgrounding (`timedOutAfterMs`) and `run_in_background` | the call was backgrounded by either side; outside the last five turns |
| rate-limit (A13) | `/usage`, the footer's limit figure and the `quota_auto_resume_*` notifications | the fit lands after the reset; no status line (the light shows `—`) |
| hook-overhead (A16) | nothing (hook timing is in the debug log only) | `cctop hook` itself is excluded; under the per-turn bar |
| verify-gap (A32) | nothing | every edited path is a doc; no test command is known in the session; the last call is test-class; the turn is still running |
| commit-unchecked (A33) | nothing | doc-only commits; once per turn |
| plan-first (A34) | the `plan-mode-for-complex-tasks` spinner tip | next row only; the tip showed recently; markdown-structured or PRD prompts; a plan artefact exists |
| correction-streak (A36) | the `double-esc` / `double-esc-code-restore` tips and `/rewind` | either tip showed recently; the pair is not on the same work; the steer window closed (a question answered, or ten calls into the turn); ten-turn cooldown |
| failure-cascade (A38) | nothing (Claude Code retries silently) | denials (A45 owns them); an Edit or Write between the failures |
| review-before-merge (A40) | the `ultrareview-post-commit` / `ultrareview-awareness` tips and the PR's review state | either tip showed recently; a review marker exists; under 100 non-doc lines; once per PR |
| waiting (A41) | the `idle_prompt` / `permission_prompt` notifications, the OS notification and the `btw-side-question` tip | the tip showed recently; under 30 s; `team` and `remote` sessions |
| destructive-git (A43) | nothing (`/rewind` covers only Claude's own edits) | the tree was clean (an Events row only) |
| stale-collision (A44) | the `edited_text_file` / `staleRecovered` notices Claude Code hands the model | a plain IDE edit with no Claude edit of the same file this turn (an Events row only) |
| denial-streak (A45) | the permission dialog, auto mode's own "three blocks pause it" and the `permissions` tip | the tip showed recently; `user-rejected` (A36's); protected paths; a rule already in `permissions.allow`; never bare `Bash` |
| instruction-drift (A46) | CLAUDE.md re-injection at boundaries | next row only; a 200 k window; an `instructions` / `nested_memory` attachment since the crossing |
| turn-died (A47) | Claude Code's own error line and the `quota_auto_resume_*` notifications | a `<synthetic>` line without `isApiErrorMessage`; exempt from the first-two-turns and away rules, so never silent on a session's first call |

Every slot rule has a fixture that fires it and one that must not, a replay precision over the 113 transcripts (§12), and an `acted` predicate naming the observable completion: a test-class Bash or a Read of an edited file (A32), a new API call (A19), a new transcript file or `SessionStart source=clear` (A23/A42), a successful call of the failing prefix or the rule appearing in `permissions.allow` (A38/A45), a review marker or merge (A40), the next user line (A41), an Agent spawn (A25), `HEAD` change (A33), a `/rewind` history row or `parentUuid` branch (A36).

## 6. The coach view — "Lights"

### 6.1 Principle

Four lights whose glyph (○ quiet, ◐ watch, ● act) can be read from the corner of the eye, each a re-evaluated state tied to one thing the person can change (context, cache warmth, limit pace, unverified or contested work), plus exactly one nudge slot that names the keystroke, slash command or typed line that resets the light and retires by itself the moment the person does it. Lights are states, not messages: they are never rate-limited and cannot nag; the slot is budgeted and ordered.

Four designs were produced independently (traffic-light "Lights", phase-aware coach, cost-first ticker, rework radar) and scored by three judges (a six-hour-a-day Max user who hates nagging, an API-paying team lead, a TUI designer). Lights won every judge (tally 126 vs 119 / 117 / 101) on glanceability and false-positive resistance; the synthesis grafted the runner-ups' best ideas: the phase coach's explore-delegate and plan-first lines, the radar's commit-without-check and "next row shows the promotion condition", the ticker's price form of the context nudge and its cost tape on the dashboard.

### 6.2 Mockups (56 columns; every row measures 56 cells)

Mid-turn: 18 calls into a silent run after editing one file five times, three consecutive `gh`/`git` failures, 412 k context on a 1 M window, three agents running.

```
╭coach ─ session-a ─────────── claude-opus-5 · turn 14 ╮
│ IMPLEMENTING · render.rs ×5 · 18c +42k · silent 3:50 │
├──────────────────────────────────────────────────────┤
│ ◐ context  412k ▇▇▇▇▁▁▁▁▁▁ 41% · ≈$.21/call          │
│ ○ cache    warm 41m (1h)                             │
│ ○ limits   5h 62% · ↻ 2h10 · agents 3 run · 86%      │
│ ● rework   3 fails ▸gh pr view · edits 3 ✓ none 11m  │
├──────────────────────────────────────────────────────┤
│ ▸ 3 fails in a row: gh pr view ×2 (8), git push (128)│
│   Esc, give the missing fact — or run it, paste tail │
│   NOW · fired at call 12 · +2 queued (n)             │
│                                                      │
│ next     verify-gap → turn end · edits 3 ✓ none 11m  │
│ snoozed  cache-miss (4 turns) · prefix tip (session) │
╰──────────────────────────────────────────────────────╯
 c dashboard  Enter act  x snooze  e why  1-4 light  q
```

Waiting on the person:

```
╭coach ─ session-a ─────────── claude-opus-5 · turn 16 ╮
│ ◆ WAITING 4m · Claude asked a question (2 options)   │
├──────────────────────────────────────────────────────┤
│ ○ context  182k ▇▇▁▁▁▁▁▁▁▁ 18% · ≈$.09/call          │
│ ◐ cache    cold in 4:12 (1h)                         │
│ ○ limits   5h 64% · ↻ 1h58                           │
│ ○ rework   edits 0 · ✓ 9m ago · clean · commit 9m    │
├──────────────────────────────────────────────────────┤
│ ▸ reply now · cache warm 4:12 · cold restart ≈182k   │
│   one word is enough · done? /clear + hand-off note  │
│   NOW · since 20:37 · +0 queued                      │
╰──────────────────────────────────────────────────────╯
```

Idle, nothing to act on (absence of colour is the signal; the two dim lifecycle rows show the coach acted and vanished rather than went silent):

```
╭coach ─ session-a ─────────── claude-opus-5 · turn 15 ╮
│ IDLE 4m · cargo test ok 6m ago · committed 3m ago    │
├──────────────────────────────────────────────────────┤
│ ○ context   96k ▇▁▁▁▁▁▁▁▁▁ 10% · ≈$.05/call          │
│ ○ cache    warm 55m (1h)                             │
│ ○ limits   5h 31% · ↻ 3h40                           │
│ ○ rework   edits 0 · ✓ 6m ago · clean · commit 3m    │
├──────────────────────────────────────────────────────┤
│   quiet · nothing to act on · 0 nudges this hour     │
│   20:41 acted   verify-gap → cargo test ran 31s later│
│   20:20 retired plan-first · EnterPlanMode seen      │
│ next     —                                           │
│ snoozed  —                                           │
╰──────────────────────────────────────────────────────╯
 c dashboard  Enter act  x snooze  e why  1-4 light  q
```

Narrow form at ≤ 40 columns: the lights collapse to 2 × 2 glyph+number pairs, the nudge wraps, next/snoozed drop.

```
╭coach ── opus-5 · turn 14 ────────╮
│ IMPLEMENTING · 18c +42k · 3:50   │
│ ◐ ctx 412k ≈$.21  ○ cache 41m    │
│ ○ 5h 62% ↻2h10    ● fails 3 ✓no  │
├──────────────────────────────────┤
│ ▸ 3 fails in a row: gh pr view   │
│   ×2 (8), git push (128)         │
│   Esc, give the missing fact —   │
│   or run it, paste tail          │
╰──────────────────────────────────╯
```

In `session_mode = loop` (Ralph, workflows) the state line reads `LOOP · stop hook re-fed prompt 44× (~210 tok each)` and the slot holds only the loop-health indicator.

### 6.3 What stays on screen

- **State line.** Phase word from tool names only (Edit/Write/implement-class Bash → IMPLEMENTING; read-only calls → EXPLORING; Agent → EXPLORING (delegated); pending `AskUserQuestion`/`ExitPlanMode` or a `Notification` → ◆ WAITING; `end_turn` + ≥ 60 s → IDLE; loop signature → LOOP); a check is quoted literally ("cargo test ok 6 m ago"), never an inferred VERIFYING word (the Bash classifier's verify class agrees with the model's own description only 47–56 %). Then the churn token (one file edited ≥ 6× this turn), the run size since the last assistant text, the silence, `nudged N×` when the harness injected `silent_turn_reminder`, and `▸ steer window` on calls 1–5 of a run (real interrupts cluster at p50 3 calls in).
- **Context light.** Tokens, ▇▁ bar as % of the exact window, ≈$/call (or "re-reads 412 k/call" when `$` is toggled to limit units, the default when `rate_limits` is present). ○ < 150 k; ◐ 150 k–warn band, or ≥ 300 k on 1 M while a turn runs; ● only when a keystroke changes it now (≥ 300 k with a clean stop available) or inside the binary's warn band (effective window − 13 000 − 20 000), never a fixed 80 % — a deliberate 1 M session sits amber, not red all day. Cold: "(cold: next call ≈$2.0)".
- **Cache light.** `warm 41m (1h)` / `cold in 4:12 (1h)` / `cold · 182k re-write` / `—` before the first call, from `prompt_cache.{warm, ttl, expires_at, recache_tokens_if_cold}`, clock-driven because the status file rewrites only at expiry; mm:ss only under 5 min; fallback `last call + observed TTL` marked ≈.
- **Limits light.** `5h 62% · ↻ 2h10`, 7 d in the same cell only ≥ 60 %, `agents 3 run · 86%` while agents run or their share ≥ 25 % (recursive `subagents/**` scan; failed `<synthetic>` spawns counted apart); ◐ when the exhaustion fit lands before the reset or 5 h ≥ 80 %; ● on a rate-limit or spend-limit error line; `—` when the window is absent (never 0). Claude Code's own runway levers appear in the detail (`/model sonnet ~2× runway`, `/effort medium`).
- **Rework light.** Always `edits N ✓ age`; streaks are prepended, never replace it: `3 fails ▸gh pr view`, `2 corrections`, `2 blocked`; `clean · commit 3m` from the git tick. ◐ at > 10 min or ≥ 14 calls since the first source edit without a test-class run, two shared-prefix fails, a PR open without review, or the uncommitted tail; ● on a cascade, denial streak, correction streak, destructive git on a dirty tree, or commit without a check.
- **Slot, next, snoozed.** Exactly one nudge (headline / action / evidence row with class, fire point and queue depth); one `next` row with the highest-ranked queued nudge **and the condition that promotes it**; one `snoozed` row with remaining cooldowns, so silence is visibly deliberate.
- **1–4 detail.** Pressing a light's digit shows its three-line detail in the slot area (context: `next 19c ≈$4.4 · fresh ≈$0.9`, threshold, prefix share, `turn ≈$2.61 ×3.2 7d` once ≥ 3 sessions exist; cache: hit ratio, misses with the last named cause, TTL source; limits: 7 d, other live sessions, exhaustion fit, agents ≈$ / ran / failed; rework: last check command + result, fails and denials with error classes, `uncommitted +412 −87 · 9 files · 18 edits · 42m`, rewind checkpoints since the last prompt).

### 6.4 Nudge model

- **Classes and fixed order.** NOW (act within this turn) › NEXT (act at the next boundary) › LATER (turn-end retrospectives). Within a class the order is fixed and published, not tokens × remaining calls (which buries rework nudges): NOW: turn-died › waiting-on-you › cache-countdown › failure-cascade › denial-streak › correction-streak › commit-without-check › destructive-git › warm-switch-undo › double-steer. NEXT: cold-resume › verify-gap › context-reset › post-compaction › loop-armed › review-before-merge › commit-tail › explore-delegate › cold-switch-hint. LATER: named cache miss › agent-context-heavy › screenshot-heavy › prefix-tip. Plan-first and instruction-drift are next-row only. Promotion happens only when the occupant retires or a higher class arrives; a same-class newcomer waits in `next`.
- **Admission bar.** A rule may take the slot only if its trigger is structural (an `isApiErrorMessage` line, a pending `AskUserQuestion`, a `Notification` type, `is_error ×3` with a shared prefix, a top-level `toolDenialKind`, an interrupt marker, `compact_boundary`, `cache_miss_reason`, git-tick numbers, `session_crons`, `PreModelSwitch` or a history.jsonl `/model` row, `prompt_cache.expires_at`) or its corpus precision is ≥ 4 in 5 in the verdicts. Anything resting on the Bash classifier's verify class uses only the test-class pattern with stdout confirmation until the ~200-call hand-label pass.
- **Phase gate.** verify-gap never while the last call is test-class; context-reset only after a clean `end_turn` with no background work; waiting-on-you and cache-countdown only while idle; cascade, denial, double-steer and destructive-git only in a running turn; LATER rules only at turn end or session start.
- **Retirement.** Each nudge names all three: predicate false (the light it cites dropped), acted (one observable completion, §5.2), hard TTL (NOW at turn end or, for the five turn-end rules — cache-countdown, warm-switch, commit-without-check, waiting-on-you, turn-died — the next human prompt; NEXT at the next human prompt; LATER after three human turns). Two NEXT rules outlive the next prompt because their act is that turn's first calls: verify-gap holds through the end of the turn after it fired (the test run *is* the next turn), review-before-merge for three human turns (the review pass is a turn of its own). `Enter` puts a nudge in an `acting…` state until its acted predicate fires ("acted · cargo test ran 31 s later" in the lifecycle rows) or the TTL expires ("expired"), so a fooled coach is visible. Human turn = `promptSource ∈ {typed, suggestion_accepted, queued}` or `origin.kind = human`; `sdk` counts as machine.
- **Anti-flicker, budget, cooldown, snooze.** The occupant changes only at a human-turn boundary, on a NOW event, or on retirement, never on a render tick. ≤ 1 newly promoted nudge per human turn; ≤ 3 distinct nudges in any 10 human turns; ≤ 1 LATER per 2 human turns. Per-rule cooldown 5 human turns after retirement (10 for correction streak). A nudge that held the slot for 3 human turns unacted self-snoozes for the session (Claude Code's own tip `cooldownSessions` pattern). `x` snoozes 5 turns (a third snooze = rest of session), `X` snoozes for the session; both persist so `query`, the pane and the skill agree.
- **Suppression.** `session_mode = loop` (last two `stop_hook_summary` lines with errors, or a Stop `hook_blocking_error` in the last 3 turns, and no typed prompt since) → loop-health only; `machine` (last prompt `system`/`sdk` or a task notification) → no nudges; `team` (agent-setting lines or a team name) → no `/clear` or reply advice. Subagent transcripts never feed nudges. NEXT/LATER are silent for the first 2 human turns and after > 2 h away (cold-resume excepted); NOW is exempt. Never duplicate Claude Code's own surfaces: no pre-compaction countdown (its footer owns it; A06 gets the "/compact <focus> while warm" wording), no auto-continue echo, no effort-downgrade echo, no warm `/model` confirm echo; a tip Claude Code showed in the last N startups (`tipsHistory`) suppresses the matching coach text.
- **Evidence beside advice.** Every nudge's numbers come from a light or the state line; `e` opens the explain overlay with evidence lines and metric ids, the acted predicate, the corpus rate ("right 10/10 last month · fires ~15×/month"), the priced saving and its assumption, the doc text, and the last three retired nudges with reasons.
- **Audit.** Every fire, acted, expired and snooze is an Events row `kind=coach`; priced events ≥ 20 k tokens or ≥ $0.10 (cache miss with cause, cold write, agent batch, top-5 % turn, model switch, loop fire, API error, compaction) are Events rows `kind=cost` (the ticker's tape, on the dashboard); the Advisor panel shows the coach's primary nudge as its first row so the two views never disagree; the SessionEnd report tallies "nudges: 7 fired, 4 acted, 2 snoozed".

### 6.5 Interaction

`c` toggles Dashboard ↔ Lights (global, persisted, `cctop run --view coach`, headless `--once --keys c`); `Esc` returns. In the view: `Enter` = act (opens the existing ask popup pre-filled with the exact action: a quoted prompt line, a slash command with its focus filled in, an allow-rule string or a settings snippet; `Enter` copies, `S` sends over the messaging socket **only** for prompt-class actions, opt-in per press, never default); `x`/`X` snooze; `e` why; `n`/`N` peek at the next/previous ranked nudge without promoting it; `1`–`4` focus a light's detail (Shift+digit jumps to the matching dashboard overlay); `l` lifecycle log; `$` toggles $ ↔ rate-limit units. Global keys keep working (`?`, `p`, `+`/`-`, `a`, `t`, `L`, `q`). Footer: ` c dashboard  Enter act  x snooze  e why  1-4 light  q`. Glyphs ○ ◐ ● ◆ ▸ ↻ ✓ ▇ ▁ get the same ASCII fallback as the border set.

### 6.6 Surfaces

| Surface | Renders | Source |
|---|---|---|
| TUI (`c`) | everything above; at height < 20 empty rows go first, then `snoozed`, then `next`; the slot never drops below 2 lines | `State` |
| Function-hooks pane (`views/coach.tsx`, `/cctop coach`) | the same object as Box/Text rows with the ○/◐/● glyphs; below 50 body columns the third figure of each light drops; Buttons with hotkeys (armed only while focused): `[1 fill]` = `$.prompt.fill` for prompt/slash kinds (the person presses Enter; never `$.prompt.submit`, `$.command.run`, `$.turn.abort`), `[2 snooze]`, `[3 why]`; engine-native fields override the binary's where both exist (`$.session.usage`, `turn.step` usage, `tool.check` rule, `classic.PreModelSwitch`, `classic.SessionStart` resume fields, `classic.Stop` background tasks, `Spinner.mode`) | `cctop query coach` via `$.process.run`, verb added to the poller |
| `$.ui.status` (one line per plugin; reserved for the coach) and `cctop query coach --line` for tmux/shell bars | L0 (≥ 80 cols) `◐412k ≈$.21 · ○cache 41m · ○5h 62% · ●fails 3 · ▸ Esc, give the missing fact`; L1 the four glyph+number pairs and a `▸` marker; L2 the four glyphs; changes only when a light level or the occupant changes | same |
| Toast (`$.ui.toast` 4 s / TUI footer 3 s) | only on promotion of a NOW-class nudge, once per fire, ≤ 1 per human turn; never for NEXT/LATER, named misses or cold-resume; correction-streak uses `$.ui.log`; `$.ui.notice(tool_use_id, …)` annotates an open permission dialog for denial-streak only | same |
| Desktop notification (opt-in `--notify`) | exactly three cases: waiting-on-you after 30 s (AskUserQuestion / Notification sources only), repeated once at 5 min of cache left; cache countdown at T−5 / T−2 only with a question or permission pending; turn died | `alerts.rs critical()` |
| `cctop query coach` / MCP `cctop_coach` / `cctop-insights` skill | `{state, lights[4]{id, level, number, detail, source, approx}, agents, nudge{id, family, class, line1, line2, evidence, action_text, action_kind, since_turn, retires_on, acted, acting}, next, snoozed[], recent[], suppressed[], session_mode}`; lines truncated to 52 cells so all surfaces render identically; the skill never re-proposes a snoozed nudge, refuses to act from a machine turn, and is the one place that may offer a CLAUDE.md line after a yes | `State` |

## 7. User stories

### US-001: Ingest the transcript fields cctop already parses or drops
**Description:** As a user, I want cctop to read the exact fields Claude Code writes, so that every downstream metric and rule works from evidence instead of heuristics.

**Acceptance Criteria:**
- [x] `transcript.rs` models: `promptId`, `promptSource`, `origin.kind`, `toolDenialKind`, `userFeedback` (length only), `interruptedMessageId`, `isCompactSummary`, `turnCompanion`, `sourceToolAssistantUUID` on user lines; `perTurnEffort`, `attribution{Skill,Plugin,Agent,McpServer}`, `isApiErrorMessage`, `error`, `apiErrorStatus`, `quotaLimits`, `gitBranch`, `message.diagnostics.cache_miss_reason` on assistant lines; `system/compact_boundary.compactMetadata`, `system/local_command` (with `/context` stdout ANSI-stripped and parsed into category rows), `system/scheduled_task_fire`, `pr-link`, `custom-title`, `file-history-delta`, `continued-in` (the `/clear` boundary on 2.1.270)
- [x] `toolUseResult` is consumed per tool: Bash (`interrupted`, `timedOutAfterMs`, `backgroundTaskId`, `persistedOutputPath/Size`, `returnCodeInterpretation`, `staleReadFileStateHint`, `gitOperation`, `bashEditDiff`), Edit/Write (`structuredPatch` line counts, `staleRecovered`, `userModified`, `originalFile == null`), Read (`type`, `truncatedByTokenCap`, `numLines/totalLines`, `file.dimensions`), Agent (`usage`, `toolStats`, `resolvedModel`, `totalToolUseCount`), AskUserQuestion, TaskCreate/TaskUpdate (`statusChange`)
- [x] All 45 attachment subtypes are counted with `rendered[].content` length (2.1.266+) and per-subtype fallbacks (`task_reminder` ≈ 0.28 × json/4); parsed structures for `task_reminder`, `edited_text_file`, `hook_success`, `hook_blocking_error`, `plan_mode(_exit)`, `goal_status`, `queued_command`, `batching_reminder_sent`, `silent_turn_reminder`, `read_truncation_notice`, `auto_mode(_exit)`, `prompt_snapshot`, `instructions`, `nested_memory`, `invoked_skills`
- [x] Turns are grouped by `promptId`; a human turn requires `promptSource ∈ {typed, suggestion_accepted, queued}` or `origin.kind == human`; interrupts, slash commands, `<local-command-stdout>`, task notifications, teammate messages and the compaction summary are not turns (fixture asserts the 13 % over-count is gone)
- [x] Assistant lines with `isApiErrorMessage` or model `<synthetic>` are excluded from model detection, context velocity and compaction detection and become `api_error` events
- [x] Each parser is gated on the transcript `version` per the first-seen map (2.1.220 … 2.1.269); older transcripts fall back to today's behaviour
- [x] Image blocks count as `w·h/750` tokens (dimensions from `toolUseResult.file`) or 1 500 when unknown; `persistedOutputSize` is kept beside `result_tokens_est`
- [x] Bash `input_summary` widens to a bounded 200-char command; a `phase` class (commit / implement / **test** / build-lint / ops / wait / gitread / explore) is computed on ingest by the classifier ported from the research reference (`phases.py`), with `turns.csv`/`segments.csv` as regression data; the test class is confirmed post hoc from stdout (`test result:` / `passed` / `failed`) and downgraded to `unknown` otherwise; ambiguous read+write commands reset exploration runs
- [x] A04 rewrite lands here: Bash `cat`/`sed -n`/`head`/`tail` count as reads, ranged reads and `file_unchanged` results are excluded, counters reset on `edited_text_file`, `staleRecovered` and every boundary
- [x] `cargo test`; clippy clean; a second anonymised fixture (`fixtures/session-b.jsonl`) contains an 8+ call explore run, Edit → cargo test → git commit, an unverified commit, an `AskUserQuestion` followed by a > 1 h gap, a `/clear`, a `TaskUpdate(completed)`, a `<synthetic>` error line, a `compact_boundary`, an interrupt with `interruptedMessageId`, three `toolDenialKind` values and an `edited_text_file`

### US-002: Read the whole status-line payload
**Description:** As a user with the shim installed, I want cctop to use Claude Code's own cache diagnosis, so that cache advice is exact and preventive.

**Acceptance Criteria:**
- [x] `status.rs::Sample` gains `prompt_cache.*` (all 14 keys), `effort.level`, `thinking.enabled`, `fast_mode`, `exceeds_200k_tokens`, `context_window.{current_usage, total_output_tokens, remaining_percentage}`, `rate_limits.spend_limit`, `session_name`, `prompt_id`, `version`, `pr.*`, `worktree.*`; the never-populated `plan` field is removed and the header slot sourced from `~/.claude.json .oauthAccount.userRateLimitTier` when readable
- [x] `State` exposes `cache_expires_at_ms`, `cache_ttl_source`, `recache_tokens_if_cold`, `misses`, `expected_rebuilds`, `last_miss_cause`; the countdown is clock-driven between status rewrites
- [x] Without the shim the countdown falls back to `last_api_call_at + observed TTL` and is marked `≈`
- [x] Snapshot test on a stored real payload (anonymised); `cctop query summary` gains a `cache` object with `metric_id`s

### US-003: Read the hook spool fully and register the missing events
**Description:** As a user who ran `cctop install`, I want exact durations, denial reasons, background tasks and the cold-resume estimate, so that the Turn panel and the coach stop estimating.

**Acceptance Criteria:**
- [x] `apply_hook` reads `duration_ms` (exact durations, `approx` cleared), `is_interrupt`, `error`, `Stop.{background_tasks, session_crons, last_assistant_message}`, `SessionStart.{source, seconds_since_last_response, context_tokens, prompt_cache_likely_expired, estimated_cache_write_usd}`, `Notification.notification_type`, `permission_mode`, `effort.level`, `prompt_id`, `agent_id/agent_type` (subagent events routed to the agent's stats instead of dropped)
- [x] `cctop hook` keeps `permission_suggestions` on `PermissionRequest` and `tool_response` for `tool_name == Agent`; `tool_input` stays stripped except a bounded 200-char `command` summary; `UserPromptSubmit.prompt` and `Stop.last_assistant_message` are reduced to a length and an "ends with `?`" flag before spooling (FR-12 — the spool held prompt text verbatim before v1.2)
- [x] `cctop install` registers `PostCompact`, `PreModelSwitch`, `PostModelSwitch`, `StopFailure`, `PermissionDenied`, `InstructionsLoaded`, `PostToolBatch`, `UserPromptExpansion`, `TaskCreated`, `TaskCompleted` (all observe-only; merged with existing hooks; diff shown; uninstall restores)
- [x] `tasks.rs` no longer lists dotfiles; background tasks come from `Stop.background_tasks` and `turn_duration.pendingBackgroundAgentCount`
- [x] Unit tests per event with real payload shapes (anonymised)

### US-004: Exact context boundaries, threshold math and harness overhead
**Description:** As a user, I want the Context panel to agree with Claude Code's footer and `/context`, so that "until auto-compact" and the breakdown are trustworthy.

**Acceptance Criteria:**
- [x] Threshold = **effective window** − 13 000 (967 000 on native-1M models, fixture-asserted for `claude-fable-5-1` at window 1 000 000), honouring `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `autoCompactWindow`, `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, `autoCompactEnabled`, `DISABLE_AUTO_COMPACT`, `DISABLE_COMPACT` read from settings and the claude process environment (`ps eww` / `/proc/<pid>/environ`); bands ok / warn (threshold − 20 000) / blocked (window − 3 000); "precompute armed" at 80 % of the window; footer wording mirrors Claude Code
- [x] Compactions come from `compact_boundary.compactMetadata` (and `PreCompact`/`PostCompact`); `/clear`, resume, compact and fork are boundary events that reset re-read, exploration and churn counters; `isCompactSummary` lines are not turns
- [x] Context bar is stacked: prefix / tool inputs / tool results / thinking (retained) / harness / prose / unattributed, computed per the anatomy rules; `harness N k/turn` row with the top three subtypes
- [x] Prefix inspector rows from `prompt_snapshot` (system prompt sections, tool schemas by name), `instructions.files`, `nested_memory`, `invoked_skills`, skills listing vs the 1 % budget, plugin always-on tokens from the catalog cache with `pluginUsage.lastUsedNumStartups`; `/context` official table shown verbatim when captured
- [x] `cleared: N tool results (~T tokens)` when a `microcompact_boundary` or `[Old tool result content cleared]` result appears; cleared results leave `tokens_to_ctx`
- [x] Snapshot tests at 60 × 51 and 40 × 24 on fixture B

### US-005: Cost gradient, attribution and agent spend
**Description:** As a user, I want to see what continuing costs at this context and where the tokens went, so that the biggest lever is visible at a glance.

**Acceptance Criteria:**
- [x] Tokens panel shows `$ per call` and `$ per turn` at the current context vs at 100 k, `next 30 calls ≈ $X`, cache line (warm/cold, expiry, TTL and source, misses with cause, expected rebuilds, re-cache tokens), `where the tokens went` (top three of skill / plugin / agent / MCP / idle turns), `agents $X (N %)` including `subagents/workflows/**` and forks, and the `cost-state` gap at session end
- [x] Limit weight per turn and the five `/usage` behaviour flags with Claude Code's tip text when ≥ 10 %
- [x] Tools panel gains `IN→CTX`, error class, Bash rows by phase class, image tokens, offloaded bytes, cap markers, and the re-read tax on the top-ctx line
- [x] Agents panel scans `subagents/**` recursively, registers agents from `SubagentStart` before their file exists, shows workflow journal failures, forks' inherited context, teammates from `teams/`, MCP servers needing auth, `ToolSearch` loads per server, concurrency caps
- [x] Files panel gains edits per turn, checkpoint version, uncommitted-since-last-commit line from `gitOperation`, stale markers, rewind points; re-read column counts Bash reads and excludes ranged reads and `file_unchanged`
- [x] Ledger and `cctop query ledger` gain tool errors, tool calls, prompt chars, harness tokens, miss cause, effort, phase mix, human/machine, cost at 100 k
- [x] Metrics registry rows and `docs/metrics.md` regenerated; CI drift check passes

### US-006: Advisor engine v2 and rule calibration
**Description:** As a user, I want advice that changes when I act and never pins one old incident, so that I keep reading it.

**Acceptance Criteria:**
- [x] `Advice` gains `urgency`, `since_turn`, `window_turns`, `acted: fn(&State) -> bool`, `cooldown_turns`; `Saving` distinguishes one-off from per-turn and ranks by tokens × expected remaining calls (baseline median calls per session minus calls so far, floor 10)
- [x] Engine applies the class order NOW › NEXT › LATER, hard TTLs, per-rule cooldowns, three-strikes self-snooze, the one-new-nudge-per-turn budget, and the suppression list (bot loop, machine turn, subagent, first two turns, away > 2 h); a `session_mode` flag (interactive / loop / workflow / remote) is derived from `hook_blocking_error`, `promptSource`, `entrypoint`, `bridge-session` — *as shipped:* `remote` stays dormant because a Remote-Control prompt carries no transcript marker on 2.1.270 (`promptSource` never says so; `bridge-session` is a registration line present in every session of a user with the bridge on)
- [x] Dismissals and `first_fired` persist in `~/.cctop/<session>.advisor.json`; `cctop advise`, `cctop query advice` and the MCP tool read them
- [x] A01–A18 changed per §5.1; each change has a fixture pair; A15 and A18 are retired in favour of A38 and A42; A05 becomes A25
- [x] Purity test extended: no rule reads prompt text into the advice; prompt-shape features are booleans computed at parse time

### US-007: New rules, token axis (A19–A21, A23, A25, A27, A30 as nudges; A24, A26, A28, A29, A31 as metrics, indicators and events)
**Description:** As a user, I want the cache, context-size, exploration, idle-turn and switch rules of §5.2 in their verified form, so that the largest cost drivers get a nudge before the money is spent and the refuted forms never nag.

**Acceptance Criteria:**
- [x] Each rule implemented as a predicate over `State` with the trigger, wording, action, urgency and `acted` predicate of §5.2; wording fits 2 × 58 columns with live numbers
- [x] A19 uses `prompt_cache.expires_at` when present, marks the fallback `≈`, shows no countdown above 300 s, and fires only with `recache_tokens_if_cold ≥ 50 k`; A21 fires from `PreModelSwitch` when installed (source command/picker only), else from a history.jsonl watcher (tail `~/.claude/history.jsonl` filtered by `sessionId`) while the shim says warm
- [x] A23/A42 form one context-reset family that escalates only at a clean turn end; A25 nudges at run ≥ 8 with ≥ 20 k added, counts only the read-only Bash allowlist, and offers "queue" (half of human steers are absorbed mid-turn as `queued_command`); A27 prices idle turns separately in the Tokens panel; A24/A26/A28/A29/A31 ship as metrics, indicators and events with no slot access
- [ ] Replay precision per rule over the 113 local transcripts is recorded beside the fixture (US-012) — *open (2026-09-15):* `cctop coach-replay` records fires, sessions and the acted-anyway rate per rule (plan §1, Phase 6: 132 local sessions); precision against labels waits for the hand-label pass of US-012
- [x] Fixture pairs per rule; `cctop advise --session fixtures/session-b.jsonl` lists the expected ranked set

### US-008: New rules, outcome and rework axis (A32, A33, A36, A38, A40, A41, A42, A45, A47 as nudges; A34, A46 next-row; A37, A39, A43, A44 as events and post-turn lines)
**Description:** As a user, I want cctop to tell me when work is unverified, when I am correcting in circles, when Claude is blocked or waiting, and when a checkpoint is a good moment to hand off, so that rework drops.

**Acceptance Criteria:**
- [x] `Turn` gains `first_edit_at`, `last_verify_at`, `edits_since_verify`, `files_edited`, `verify_cmd_detected` (last successful verify-class command, else the CLAUDE.md test line), `ended_with_question`, `pending_question`, `interrupt: Option<{after_calls, output_tokens, tool}>`, `steers`, `denials_by_kind`
- [x] A32/A33 suppressed when every edited path is `.md/.txt/.json/.yaml/.toml/.csv` and when no test command is known; A33 windows by the last commit, fires once per turn, has no `PreToolUse` path; A36 uses only structural markers (keyword variant never sufficient) and counts `/rewind` history rows or `parentUuid` branching as acted; A38 excludes denials; A41 sends the desktop notification after 30 s (AskUserQuestion / Notification sources only) and once more at cache − 5 min; A42 never fires on a commit alone; A45 prints `permission_suggestions[].ruleContent` verbatim, skips rules already in `permissions.allow`, and never suggests bare `Bash`; A34/A46 are next-row only; A35 is admitted only if its tightened trigger replays ≤ 1-in-5 wrong on the 459 typed prompts
- [x] A47 events replace today's false `Compact ctx → 0`; A43/A44 are events with a one-line nudge
- [x] Fixture pairs per rule on fixture B; `cctop advise` on the live session shows no rule firing on the empty session

### US-009: The coach view in the TUI ("Lights")
**Description:** As a user, I want a second top-level view with the state line, four lights, one nudge slot and the next/snoozed rows, so that I can glance and act without reading nine panels.

**Acceptance Criteria:**
- [x] `State.view: View::{Dashboard, Coach}` checked at the top of `App::draw`, before the direction-B dashboard draws (`layout::solve` no longer exists); global key `c` toggles; persisted in `~/.config/cctop/config.toml`; `--view coach` flag; added to `BINDINGS` and the help overlay
- [x] Renders the §6.2 layout at 56 × 20 and 60 × 51; at ≤ 40 columns the lights collapse to 2 × 2 pairs and next/snoozed drop; at height < 20 empty rows go first, then `snoozed`, then `next`; the slot never drops below 2 lines; glyph levels carry the state monochrome, colour from the theme roles only, ASCII fallback for ○ ◐ ● ◆ ▸ ↻ ✓ ▇ ▁
- [x] Keys per §6.5: `Enter` act (ask popup pre-filled; `S` socket send only for prompt-class actions, opt-in per press), `x`/`X` snooze, `e` why, `n`/`N` peek, `1`–`4` light detail, `l` lifecycle log, `$` units, `Esc` back
- [x] State line per §6.3: phase word from tool names only, a literal check quote, churn token, run size and silence, `nudged N×`, `▸ steer window`; ◆ WAITING / IDLE / LOOP variants
- [x] Lights compute their levels every tick from `State` and are never rate-limited; the slot changes only at a human-turn boundary, on a NOW event or on retirement
- [x] Headless: `cctop run --once --view coach --keys … --size 56x20` renders for snapshots; snapshot tests on fixture B in six moments (exploring run, unverified edits, failure cascade, waiting on a question, cold resume, idle checkpoint) plus the loop-mode line
- [ ] Dogfood: opened for a working week; every rule in §5.2 marked P0 has fired at least once correctly; false positives logged with `x` and counted (§12). The view ships with A01–A18 first (plan Phase 4); this criterion closes once the P0 rules of US-007/US-008 exist — *open:* dogfood starts with cctop 0.3.0 on 2026-09-15 under `coach = "auto"`; closes after the working week

### US-010: `cctop query coach`, MCP tool and pane wiring
**Description:** As a user of the pane, the skill or another agent, I want the same coach object everywhere, so that there is one implementation.

**Acceptance Criteria:**
- [x] `cctop query coach [--session] [--line] [--snooze <id>]` returns the §6.6 object (state, four lights with `source` and `approx`, agents, nudge with class/family/action_kind/acted/acting, next with its promotion condition, snoozed, recent, suppressed with reasons, session_mode); lines truncated to 52 cells; documented in `docs/query.md`; insta snapshot
- [x] Single-writer rule for `~/.cctop/<session>.advisor.json`: the TUI owns it while running (lock file); CLI/MCP/pane write only when no lock exists
- [x] MCP tool `cctop_coach` added (schema ≤ 120 tokens; total still under the documented cap); `docs/mcp.md` updated
- [x] `cctop-insights` skill maps "what should I do now / next" to `cctop query coach`
- [x] Pane PRD amendment: Overview renders the dashboard object (`cctop query dashboard`, direction B): the state line, the four lights as tiles, the nudge and the nine ledger rows as Buttons; the Coach view renders the coach object; `$.ui.status` shows the one-line form; NOW-class events go to `$.ui.toast`; no injection into the model
- [x] Marker `~/.cctop/pane/<session>.json` unchanged; the pane polls `coach` instead of `advice` every 2 s while a turn runs

### US-011: Cross-session sources and the session-start line
**Description:** As a user, I want cctop to use the analysis Claude Code already did, so that the first prompt of a session is primed by my own history.

**Acceptance Criteria:**
- [x] Reads `usage-data/session-meta/*.json` (never `first_prompt`) and `facets/*.json` (never `underlying_goal`, `brief_summary`, `friction_detail`) at attach and on mtime change; per-project medians; one dim line for the first turn with the date and a `run /insights` hint when older than 30 days
- [x] `baseline.rs` gains per-turn medians, interruption rate, error categories, commit-without-check ratio, model mix; `stats-cache.json` `hourCounts` feed the limit-exhaustion projection window; `~/.claude.json projects[cwd].last*` feed the header — *as shipped:* `stats-cache.json` carries no `hourCounts` on 2.1.270; the active hours come from session-meta `message_hours` (local time) and say whether the projected exhaustion falls in an hour you usually work
- [ ] history.jsonl watcher (US-007) also feeds `/clear` and `/compact` frequency as habit baselines and the `pastedContents` size for A11 — *partly:* the tailer feeds the `/clear` and `/compact` habit counts and collects the paste sizes; A11 reads them from round 2 (plan §1)
- [x] Privacy test: no field listed as never-display can reach any panel, query or report

### US-012: Measurement of the coach
**Description:** As the maintainer, I want to know whether the coach changes behaviour on my own baseline, so that rules that do not earn their slot are demoted before anyone else sees them.

**Acceptance Criteria:**
- [x] `~/.cctop/<session>.advisor.json` records per fire: rule, class, shown_at, surface_visible (TUI view open / pane docked / status line active), human_idle_ms at fire when available, time-to-x, acted completion and delay, expired, snoozed, Claude Code version, model, project, session_mode; "exposed fire" (a surface was visible) is the denominator for every rate
- [ ] `cctop coach-replay <jsonl…>` replays the final rule set over transcripts (the 113 local ones plus fixtures) and prints per rule: fires per session, precision against the verdict labels, and the **acted-anyway-within-TTL** rate (verified episodes reach a check in p50 2 calls with no nudge), so the causal estimate is acted-rate(with coach) − baseline-acted-rate — *partly:* fires per session and the acted-anyway-within-TTL rate are printed; the research verdicts are per candidate (C001–C064), not per fire, so no per-fire precision exists until a ~200-fire hand-label pass
- [ ] `cctop coach-stats [--since 4w]` prints per rule: exposed fires, acted, snoozed, X, time-to-x < 2 s (reflex dismissals), view toggled away within 10 s of a promotion, expired unacted, and the family's outcome metric vs the pre-coach corpus (2026-08-12 → 2026-09-12) as the control row: cache_creation on first calls after 45–65 min gaps (A19); cache_creation after > TTL gaps split by `/clear`-avoidable vs continued (A30); share of calls > 300 k and new transcript files whose predecessor exceeded 300 k (A23/A42); main-context explore-run tokens ≥ 8 calls and Explore spawns (A25); share of source-edit turns with a stdout-confirmed test within 14 calls (A32); blind same-prefix retries (A38); `automode-blocked` count and `permissions.allow` growth (A45); waits > 5 min / > 55 min after `AskUserQuestion` (A41); re-prompts into the same error class (A47); correction pairs and rewind markers (A36); sessions with `session_mode ≠ interactive` excluded — *partly:* every column of the coach's own record is printed; the control row is the control arm's acted-anyway rate (`--coach auto`), not the pre-coach corpus metrics, which are not computed yet (decide after two weeks of records)
- [x] Exposure alternates by session (`cctop run --coach on|off|auto`, assignment logged), stratified by project, model family (the cost gradient differs 4× between Fable 5.1 and Opus 5) and Claude Code version; rare families (cold countdown 2–4/month, warm switch ~1 per 11 sessions, commit-without-check ~2/month, correction streak 3–8/month, post-compaction ~1 per 113 sessions) get "fires correctly ≥ N exposed times" criteria rather than rate targets; frequent families (verify gap, denials, explore, cold write, waiting) get 2-week rate targets, rare ones 8–12 weeks
- [ ] Coach cost is reported in `coach-stats`: socket sends and their tokens, pane `$.prompt.fill` drafts, CPU of the 1-s tick and the 2-s pane poller, git shell-outs, hook overhead p99 from `~/.claude.json lastSessionMetrics` — *partly:* socket sends and their tokens, the tick's CPU, git shell-outs and the hook p99 are reported; the pane's `$.prompt.fill` drafts are not counted yet
- [x] A rule whose false-positive rate (snoozed + expired-unacted ÷ exposed fires, corrected for acted-anyway) exceeds 20 % after N exposed fires is demoted to `next`-row or removed; a rule whose precision collapses on a new Claude Code version is auto-demoted to LATER

### US-013: Alerts, Advisor dismissals and the pane reconciled with the coach
**Description:** As a user, I want one owner per warning, so that the alert engine, the Advisor panel, the coach and the pane never say two different things.

**Acceptance Criteria:**
- [x] Alert-by-alert table implemented: `ContextHigh` re-thresholded to the effective-window warn band (today it fires at 800 k on 1 M models); `CompactionSoon` removed (Claude Code's footer owns it; A06 reworded); `CacheHitLow` replaced by A20; `PermissionWaitLong` replaced by A41; `ToolRunningLong` kept (A10 fixed); one toast budget shared by alerts.rs and the coach
- [x] One dismissal model: snooze with a turn horizon, persisted; the Advisor panel's `x` and the coach's `x`/`X` write the same file; `cctop query advice` gains a schema version and returns the coach's primary nudge first; the pane PRD's verification item "Advisor top item equals `cctop query advice`" is updated accordingly
- [x] User-visible number changes from `promptId` turn identity and the synthetic-line filter (header turn count −20 %, A12 denominator, context history and compaction count, header model, ledger rows, query snapshots, pane Overview) land in one story with regenerated fixtures and snapshots **before** any coach family ships
- [x] Pane PRD amended: `$.ui.status` reserved for the coach line; `coach` added to the poller's known verbs; `/cctop coach` argument; `$.prompt.fill` drafts allowed, `$.prompt.submit` / `$.command.run` / `$.turn.abort` forbidden; per family a "pane-native source" row (`command.run` for `/clear`/`/compact`/`/model`, `config.set` for autoCompact toggles, `session.compact trigger=precompute`, `turn.complete.reason=refusal`, `agent.spawn` pre-spawn, `$.session.messages()`, `prompt.suggest`)
- [x] Per family, the Claude Code surface it complements and the condition under which the coach stays silent (spinner tips via `tipsHistory`, the warm `/model` confirm dialog, the context-low footer, the resume-from-summary dialog, `quota_auto_resume_*` notifications)

## 8. Functional requirements

- FR-1: The coach and all new rules must be pure functions of `State` with no model calls, no network, and no writes outside `~/.cctop` and `~/.config/cctop` (extends FR-2/FR-15 of the base PRD).
- FR-2: Every nudge must name one concrete action (key, slash command, setting, env var, or a line to queue) and one observable completion; the engine must retire it on completion.
- FR-3: At most one primary nudge is visible at a time; the slot occupant changes only at a human-turn boundary, on a NOW event or on retirement; at most one newly promoted nudge per human turn, three distinct nudges per ten human turns, one LATER per two; NOW › NEXT › LATER precedence with the fixed intra-class order of §6.4; per-rule cooldown ≥ 5 human turns; three unacted showings self-snooze.
- FR-3a: A rule may occupy the slot only if its trigger is structural or its replay precision is ≥ 4 in 5; lights are re-evaluated every tick and never rate-limited.
- FR-4: No human-facing nudge may render in a bot-loop, workflow-driven, `-p` or subagent context, or on a machine-originated turn.
- FR-5: Turn identity must be `promptId`; human turns must be defined by `promptSource`/`origin.kind`; interrupts and slash commands must not count as turns.
- FR-6: API-error assistant lines must never set the model, the context size, a compaction, or velocity.
- FR-7: Compactions must be read from `compact_boundary` (or `PreCompact`/`PostCompact`); the ≥ 30 % drop heuristic may only remain as a labelled fallback for transcripts before 2.1.263.
- FR-8: The autocompact threshold, warn band and footer wording must match Claude Code 2.1.269 (effective window − 13 000; warn − 20 000; block window − 3 000; overrides honoured); when the effective window is not exact the coach must show "% of window used" and never "N turns until autocompact".
- FR-9: Cache advice must use `prompt_cache` when the shim is present and must mark any TTL or expiry derived otherwise as `≈`; a miss must be named by `cache_miss_reason` or `last_miss_cause`, never guessed.
- FR-10: Cost per call, per turn and "cost of continuing" must be computed from the current context size and `pricing.toml`, with `effort_cost_index` for effort advice and Claude Code's limit-weight formula for subscribers.
- FR-11: Every metric added in §4 must exist in the registry with source, caveats and `≈` conditions; `docs/metrics.md` must regenerate; CI must fail on drift.
- FR-12: No prompt text may be stored in `State`, shown, or exported; prompt-shape features are booleans and lengths computed at parse time; near-duplicate detection (A49) hashes locally.
- FR-13: Fields marked never-display in §3.3 (`first_prompt`, `underlying_goal`, `brief_summary`, `friction_detail`, `userFeedback` text) must not reach any surface.
- FR-14: `cctop query coach`, the MCP tool and the pane must render the same object; every number must carry `metric_id` and `approx`.
- FR-15: The coach view must render at 56 × 20 and 60 × 51, collapse at ≤ 40 columns, and remain usable at 40 × 24; headless rendering must be snapshot-tested on fixture B.
- FR-17: Every light and number must render `—` (never 0) when its source is absent, and `≈` when estimated; the per-call price must switch to the cold-write price after a TTL flip or a cold gap.
- FR-16: New hook registrations must be observe-only, merged with existing hooks, shown as a diff, and reversible by `cctop uninstall`.

## 9. Non-goals

- No LLM calls in the coach path, including a Haiku classifier for corrections (recorded as open question 4).
- No enforcement: no `updatedToolOutput`, no `PreToolUse` input rewriting, no `turn.abort`, no `$.prompt.submit`; these are documented as a possible opt-in "guard mode" (open question 5).
- No injection of coach text into the model's context (`additionalContext`, `prompt.context`); the pane PRD's non-goal stands.
- No keyword-based "correction detected" rule; keyword features may only lower or raise the weight of a structural signal.
- No fleet or team view beyond rolling teammates' cost into the attached session.
- No Windows support; no changes to Claude Code.
- No attempt to reproduce `/insights` narrative reports; cctop links to `report.html`.

## 10. Design considerations

- **Glanceability over completeness.** The state line and the four lights are the product; the nudge is the one sentence a person reads while typing. Anything that needs a table goes to the dashboard (`c`) or the light's `1`–`4` detail.
- **Quiet by default.** Lights say nothing; the slot is budgeted; the `next` row shows what would fire and why; the idle view shows what the coach acted on and retired so quiet looks like working rather than broken. Retrospectives (LATER) never toast.
- **Same words as Claude Code.** Footer phrases, error taxonomy, behaviour-flag tips, the effort-nudge ratio text and the `/context` suggestion thresholds are reused verbatim so the person never sees two numbers for one thing.
- **Evidence beside advice.** Each nudge cites a number that is on screen in a light or the state line; `e` shows the corpus rate and the acted predicate.
- **Colour only for state.** The glyph carries the level monochrome (○ ◐ ●), colour sits on top; no colour for phases; ASCII fallback for every glyph.
- **Honest estimates.** `≈` where the shim, hooks or OTel are absent; the fallback TTL is labelled.

## 11. Technical considerations

- **Collectors.** New: history.jsonl tailer (filtered by `sessionId`), optional `human_idle_ms` (ioreg / xprintidle / tmux `client_activity`), optional debug-log tail, `usage-data/*` reader, `~/.claude.json` reader (keys only), plugin catalog cache reader, claude-pid environment reader (`ps eww` / procfs), recursive `subagents/**` scan, workflow journal reader. Extended: transcript, status, hooks, tasks (removed), files, agents, prefix.
- **Phase classifier.** Port `classify_bash`/`assign_phases` from the research scratchpad (`phases.py`) into `src/phase.rs` as pure functions with the regression CSVs as fixtures; expose `Call.phase` and a `Phase::current(&[Call]) -> (Word, run_len)` over the last five calls.
- **Engine.** `src/advisor/{mod.rs, rules.rs}` split into `rules/token.rs`, `rules/outcome.rs`, `rules/events.rs`; `Engine` gains the class scheduler, persistence and the nudge log; `query::coach` and `mcp` share `coach::snapshot(&State)`.
- **View.** `src/ui/coach_view.rs` drawn when `State.view == View::Coach`, checked before the direction-B dashboard (`src/ui/dashboard.rs`; `layout::solve` and its modes are deleted with the framed grid); no panel ownership; `BINDINGS` updated for the help test; the ask popup (`src/ask.rs`) reused for `Enter`.
- **Performance.** The attachment ledger, phase classification and per-call pricing are O(1) per line; the recursive agent scan is throttled to the existing 2 s tick; history.jsonl is read from the last offset. Budget unchanged: ≤ 2 % CPU idle, ≤ 5 % during a turn, ≤ 50 MB RSS.
- **Version drift.** Field parsers gate on `version`; the binary constants (effective-window table 967 k / 187 k, 13 000 / 20 000 / 3 000 / 0.8, tier weights, `effort_cost_index`, `/context` thresholds) live in one `harness_facts.rs` with the Claude Code version they were read from; `scripts/check-plugin-types.sh` already compares versions — extend it to warn when `claude --version` is newer than the facts table.
- **Pane.** No new engine calls; the poller swaps `advice` for `coach`; `$.ui.status`/`$.ui.toast` are one-liners in the existing module.
- **Privacy.** History and prompt-shape features are computed in memory; A49 stores 3-gram shingle hashes per project under `~/.cctop/corrections/<slug>.json`, never text.

## 12. Success metrics

Baselines use the verifiers' denominators on the pre-coach corpus (2026-08-12 → 2026-09-12; `session_mode = interactive` only). Targets for frequent families are rates over 2 dogfood weeks; rare families get exposed-fire counts, because a family that fires twice a month cannot show a rate change in two weeks.

| Family | Baseline (verified) | Volume | Target |
|---|---|---|---|
| Verify gap (A32) | 41 % of source-edit turns end unverified (15 of 37/month); 13 stay so next turn | ~15/month | ≤ 20 % within 2 weeks of exposure |
| Denial streak (A45) | 83 denials/month; retried with a variant in ~1 of 3 cases | ~10 streaks/month | `automode-blocked` count −50 %; `permissions.allow` grows with the suggested rules |
| Exploration run (A25) | 46 runs ≥ 8 calls, p50 48.7 k each; 0 name-based A05 fires | ~4/week | main-context explore-run tokens −40 %; Explore spawns up |
| Context reset (A23/A42) | calls > 300 k are 25.5 % of calls and 51.6 % of spend; realistic saving $110–140/month in ~3 sessions | ~3 sessions/month | share of spend above 300 k < 40 %; new transcript files whose predecessor exceeded 300 k |
| Cold resume (A30) | 55 of 58 gaps > 60 min cold, 50 above 100 k | ~4/month | `/clear`-avoidable re-caches −50 % |
| Cache countdown (A19) | ~7 prompts/month land in the 45–65 min window on 1 h; every turn on 5 m | 2–4 catches/month (1 h) | fires correctly ≥ 3 exposed times; zero fires above 300 s remaining |
| Waiting on you (A41) | after `AskUserQuestion` 18 of 55 waits > 5 min, 5 > 60 min | ~15/month | waits > 55 min halved; notification opt-in kept |
| Failure cascade (A38) | ~14 pure-error runs/month; 29 blind same-prefix retries | ~14/month | blind retries −50 % |
| Correction streak (A36) | ~5/month; 595 checkpoints, 0 rewinds | ~5/month | ≥ 1 rewind marker observed after a nudge; fires correctly ≥ 3 exposed times |
| Commit without check (A33) | ~7 fires per 91 sessions, 4 escalated | ~2/month | fires correctly ≥ 3 exposed times |
| Warm switch (A21) | 10 of 10 mid-session switches cold; ~1 per 11 sessions | ~2/month | 0 unwarned warm switches when hooks are installed |
| Turn died (A47) | 18 error lines; 13 of 14 limit errors on a session's first call | irregular | every error line an event, none a compaction or the current model |
| Harness overhead, subagent spend, turn identity | 0 % of `rendered` bytes attributed; 0 of 264 workflow agents listed; 20 % turn over-count | — | 100 % attributed on 2.1.266+; all agents listed; 0 % over-count on 2.1.220+ |
| Coach behaviour | — | — | ≤ 1 promoted nudge per human turn; per-rule false-positive rate ≤ 20 % after N exposed fires (acted-anyway corrected); zero model tokens spent by the coach; the SessionEnd tally and Events audit exist for every fire |
| Agreement with Claude Code | threshold 80 % vs 967 k; 3 false compactions | — | `/context`, `/usage`, the footer and cctop show the same numbers on fixture B and on a live session |

## 13. Open questions

Closed in v1.1: **TTL source** — the shim's `prompt_cache.ttl` and the transcript's `ephemeral_*` fields agree on every call checked (12 311 at 1 h, 965 at 5 m, three genuine flips); use the shim, fall back to the per-call field. **The 567 k compaction** — explained by the effective window: 980 000 − 13 000 = 967 000 is the threshold and the debug log prints `effectiveWindow`; the session's own window must have been lowered (`autoCompactWindow`/env), verifiable from a debug log.

1. **Human presence.** Every "idle" in the design is transcript silence, which is false while a long turn runs. `ioreg -c IOHIDSystem` HIDIdleTime works on this machine, tmux exposes `client_activity`, and Claude Code itself uses terminal focus for the away recap. Add a `human_idle_ms` collector (`—` when unavailable) and re-open A39 as a live rule with it; gate A19/A41 on human idle rather than silence.
2. **Queued-steer delivery.** Half of human queued messages are absorbed mid-turn as `queued_command` attachments (29 of 68), the other half arrive as `promptSource = queued` next-turn lines; wordings that say "queue" are right, wordings that say "lands at turn end" are half right. Re-derive the split on fixture B and phrase every queued action as "queue it; Claude reads it at its next call or at turn end".
3. **Phase precision.** The Bash classifier agrees with the model's own `description` on 74 % of commits, 78 % of explores and 47 % of verifies; hand-label ~200 calls before any rule escalates on the verify word.
4. **Partial summarisation.** `/rewind → Summarize from here / up to here` has no candidate; add a third form to the context-reset family when a contiguous resolved tool-output segment ≥ 50 k sits behind the last N turns.
5. **Plan-accept as a checkpoint** and `showClearContextOnPlanAccept`: treat `ExitPlanMode` accepted as a boundary signal (highest confidence with task-completed and `?`-ended turns) and recognise `SessionStart source=clear` right after it as acted.
6. **`/autocompact <N>` as a persistent action.** Two sessions ran to 921 k / 953 k uncompacted; decide whether the context-reset family offers "/autocompact 400k (persists)" after its second fire in a project, or rejects it because mid-task compaction loses detail.
7. **Refusals and precomputed compaction.** `turn.complete.reason = refusal` and `session.compact trigger = precompute` (function hooks) have no family; decide whether they get a NOW/NEXT entry in the pane.
8. **Workflow-agent limit refusals as a NOW event.** 148 of 148 journal `failed` lines were limit refusals (this session: 174 failed spawns); extend turn-died with an "agents failing on the limit" branch fed by `journal.jsonl` + the agent transcript tail, coalesced per run.
9. **Correction classification by model.** Structural markers give ~100 events/month; a Haiku classifier at turn end using the `/insights` taxonomy would cost one small request per turn. Default: no.
10. **Guard mode.** `updatedToolOutput`, `PreToolUse.updatedInput`, `agent.spawn` model rewrite and `turn.abort` are the only levers that remove tokens or stop waste; a separate opt-in PRD behind `/config` rows, never in the read-only coach.
11. **Image cap.** The per-request image/PDF cap (oldest batch dropped, history reprocessed) is not recovered from the binary; grep it and add an Events row "N/cap images in context" as a named-miss cause.
12. **Debug log as a source.** `~/.claude/debug/*.txt` now exist (from `claude -p` runs) and carry `autocompact: tokens=… effectiveWindow=…`, `[API:timing] first byte after N ms` (the TTFT cctop's OTel receiver cannot get), `hooks module … settled in N ms` and `[PROMPT CACHE BREAK]` causes; verify whether interactive sessions write them without `--debug` and add `cctop attach --debug-log`.
13. **Function-hooks GA.** If the module ships, A19–A21, A41 and A45 gain exact live triggers; the TUI path must not depend on it.
14. **Attachment rendering before 2.1.266** is heuristic and must be labelled `≈`; **`ttft`** exists only on `llm_request` spans (beta tracing) or the debug log — add `/v1/traces` or the debug tail, or drop the metric claim.
15. **Explore-agent return size.** "p50 71 tokens" is a 10-sample `toolUseResult` figure; quote the live run growth and "typically ~10 k more before the run ends" instead, and keep 71 in the explain overlay as unverified.

## 14. Research trail and verification status

The research ran as a multi-agent workflow on 2026-09-12/13 (run `wf_0aa065ff-0a0` across five resumes, scripts `cctop-research-sweep-2…4.js` under the session's `workflows/scripts/`). All stages completed:

- **Eleven readers** (function-hooks API inventory E/A/S; official docs and changelog H/S/O/C/E/L/X; best-practice signals P/D; competitor scan F; cctop source audit ING/UNS/RULE/GAP/TASK/QUERY/LAYOUT/VIEW/ALERT/BASE; transcript fields T; `~/.claude` home state HS; binary strings BIN; rework-signal mining RW; session-phase model PH/SUG; context-cost anatomy T-anatomy).
- **Four candidate generators** (token savings, better outcomes, less rework, coach view) and a **merge** → 64 canonical candidates C001–C064.
- **64 adversarial verdicts**, two lenses each (data availability on this machine; actionability and false-positive risk), the first 15 at high effort, the rest at medium (batched five per agent): 33 survived, 31 were refuted **as live nudges only** — none on data — each with salvage corrections that §5.2 applies.
- **Four coach designs**, **three judges** (tally: Lights 126, rework radar 119, phase coach 117, cost ticker 101; unanimous winner), a **synthesis** that grafted the runner-ups' best ideas, and a **completeness critic** with 40 gap items across data sources, missing levers, over-strict rejections, unbacked design claims, interactions with existing rules and the pane, and measurement — folded into §5–§6, §12 and §13.

All reports, the candidate list, the 64 verdicts, the four designs, the judge scores, the synthesis and the critic are in `tasks/research-coach/` (untracked; they contain project names and session ids from this machine, so review before committing).

**Verified directly by the author on this machine** (jq over all local transcripts, the live status file, the live hook spool, the binary): `diagnostics.cache_miss_reason` types and token sums; `rendered` presence by version; `toolDenialKind` values; interrupt markers with `interruptedMessageId`; `promptId`/`promptSource` distribution; the 1 h → 5 m TTL flip in this session; attachment subtype counts; `gitOperation` results; one `compact_boundary` + one `isCompactSummary`; the API-error lines; `PostToolUse.duration_ms`, `effort`, `permission_mode` and `Stop.background_tasks` in the live spool; the live `prompt_cache` block.

**Known limits of the evidence.** Every number is one user's month (81–113 sessions, seven projects, mostly auto mode on 1 M-window models); the phase classifier's precision is measured against a labeler that shares its regexes; the `SessionStart` resume path (A30 secondary trigger) had zero local samples; microcompaction was never observed; several harness reminders are model- or version-gated. §12 therefore requires a replay over the local corpus and exposure-logged dogfood before any family is called effective.
