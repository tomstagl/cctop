//! Every number cctop shows is declared here once: what it is, how it is
//! computed, where the data comes from, and when it is only an estimate.
//! `docs/metrics.md` and the README section are generated from this table,
//! and `cctop query` tags values with these ids.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metric {
    /// `snake_case`, stable; used in JSON output and doc anchors.
    pub id: &'static str,
    pub panel: &'static str,
    pub name: &'static str,
    pub unit: &'static str,
    pub formula: &'static str,
    /// Data-source ids from the PRD (`D1`, `D2`, …).
    pub sources: &'static [&'static str],
    pub caveats: &'static str,
    /// When the value is shown with an `est`/`≈` marker; empty if never.
    pub estimate_when: &'static str,
}

macro_rules! metric {
    ($id:ident, $panel:literal, $name:literal, $unit:literal, $formula:literal, [$($src:literal),*], $caveats:literal, $est:literal) => {
        Metric {
            id: stringify!($id),
            panel: $panel,
            name: $name,
            unit: $unit,
            formula: $formula,
            sources: &[$($src),*],
            caveats: $caveats,
            estimate_when: $est,
        }
    };
}

/// Panel order used for grouping in generated docs.
pub const PANELS: &[&str] = &[
    "Header",
    "Context",
    "Tokens & Cost",
    "Limits",
    "Turn",
    "Tools",
    "Agents & MCP",
    "Files",
    "Advisor",
    "Events",
    "Coach",
];

pub const METRICS: &[Metric] = &[
    // -- Header
    metric!(session_status, "Header", "Status", "enum", "`status` from the session registry (busy/idle); WAITING when a permission request is pending; ENDED when the pid is gone", ["D1", "D4"], "", ""),
    metric!(turn_number, "Header", "Turn", "count", "Prompts the person wrote so far, one per `promptId` (`promptSource` typed / suggestion_accepted / queued, or `origin.kind` human); interrupts, slash commands, task notifications, teammate messages and the compaction summary are not turns", ["D2"], "A resumed session starts counting at the resume point; before Claude Code 2.1.220 every non-meta text line counts", ""),
    metric!(turn_elapsed, "Header", "Turn elapsed", "ms", "`turn_duration.durationMs` once the turn ended, else now − turn start", ["D2"], "", ""),
    metric!(effort, "Header", "Effort", "enum", "`perTurnEffort` of the latest assistant line when set, else its `effort`, else the status line's `effort.level`; with thinking on/off and fast mode from the status line", ["D2", "D3"], "", ""),
    metric!(plan_tier, "Header", "Plan", "enum", "`oauthAccount.userRateLimitTier` (else `organizationRateLimitTier`) from `~/.claude.json`", ["D12"], "The status line never carries a plan; keys are read, never the account's names", ""),
    metric!(process_cpu, "Header", "CPU", "%", "CPU share of the `claude` process over the last sample interval", ["D5"], "", ""),
    metric!(process_rss, "Header", "Memory", "bytes", "Resident set size of the `claude` process", ["D5"], "", ""),
    // -- Context
    metric!(context_size, "Context", "Context size", "tokens", "`cache_read + cache_write + input` of the turn's last API call — everything the model read", ["D2", "D3"], "The status line's `total_input_tokens` is preferred when the shim is installed", "est when computed from the transcript alone"),
    metric!(context_window, "Context", "Context window", "tokens", "`context_window_size` from the status line, else the model's default window", ["D3"], "", "est without the status-line shim"),
    metric!(context_prefix, "Context", "Fixed prefix", "tokens", "`cache_read + cache_write` of the session's first API call: system prompt, CLAUDE.md, tool schemas", ["D2"], "With a warm cache the first call is a read, so both fields are summed", ""),
    metric!(context_velocity, "Context", "Context velocity", "tokens/turn", "Exponential moving average (α = 1/5) of Δ context size per turn", ["D2"], "Turns that compacted are excluded from the average", ""),
    metric!(turns_until_compaction, "Context", "Turns until autocompact", "turns", "(autocompact threshold − context size) / context velocity", ["D2", "D3"], "Threshold = Claude Code's effective window − 13 000 tokens (967 000 on native-1M models, 187 000 on 200 k windows) until a compaction has been observed for the model, then the observed value is used", "est until a compaction has been observed"),
    metric!(context_anatomy, "Context", "Context anatomy", "tokens", "The stacked bar: prefix (first call's cache read + write) · tool inputs (chars the model wrote / 4) · tool results (`tokens_to_ctx`) · retained thinking · harness (attachments, `rendered` chars / 4) · prose (text blocks / 4) · unattributed (the rest of the size), summed since the last context boundary", ["D2"], "Slices are scaled down together when their estimates overshoot the exact size", "≈ whenever a slice rests on chars / 4 or an attachment fallback"),
    metric!(harness_tokens, "Context", "Harness per turn", "tokens", "Attachment tokens (reminders, injected files, listings) ÷ human turns since the last boundary", ["D2"], "`rendered[].content` since Claude Code 2.1.266; per-subtype ratios before", "≈ before 2.1.266"),
    metric!(context_band, "Context", "Context band", "enum", "ok below threshold − 20 000 · warn inside that band (Claude Code's footer turns to \"Context low\") · blocked at the threshold or window − 3 000; the footer text is Claude Code's own (`N% until auto-compact`, `N% context used` when autocompact is off); precompute armed at 80 % of the window", ["D2", "D3", "D12"], "Overrides come from settings.json and the claude process environment (`CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`, `DISABLE_AUTO_COMPACT`, `autoCompactWindow`, `autoCompactEnabled`)", ""),
    metric!(compactions, "Context", "Compactions", "count", "`system/compact_boundary` lines (exact: trigger, pre/post tokens, duration), or a PreCompact hook", ["D2", "D4"], "API-error lines (`<synthetic>`, zero usage) never count", "Before Claude Code 2.1.263 a ≥ 30 % context drop between turns is taken as a compaction"),
    // -- Tokens & Cost
    metric!(cache_read, "Tokens & Cost", "Cache read", "tokens", "Σ `cache_read_input_tokens` over distinct API responses", ["D2"], "Counted once per `message.id`; Claude Code writes one line per content block", ""),
    metric!(cache_write, "Tokens & Cost", "Cache write", "tokens", "Σ `cache_creation_input_tokens`, split into 5-minute and 1-hour TTL from `cache_creation.ephemeral_*`", ["D2"], "", ""),
    metric!(fresh_input, "Tokens & Cost", "Fresh input", "tokens", "Σ `input_tokens` (uncached)", ["D2"], "", ""),
    metric!(output, "Tokens & Cost", "Output", "tokens", "Σ `output_tokens`", ["D2"], "", ""),
    metric!(thinking, "Tokens & Cost", "Thinking", "tokens", "Σ `output_tokens_details.thinking_tokens` (a subset of output)", ["D2"], "", ""),
    metric!(cache_hit_ratio, "Tokens & Cost", "Cache hit ratio", "ratio", "cache_read / (cache_read + cache_write + fresh_input)", ["D2"], "Green ≥ 0.8, amber ≥ 0.5, red below", ""),
    metric!(cache_ttl, "Tokens & Cost", "Cache TTL", "enum", "`prompt_cache.ttl` from the status line; else 1h if the latest call reports `ephemeral_1h_input_tokens > 0`, else 5m", ["D3", "D2"], "", "≈ without the shim"),
    metric!(cache_warm, "Tokens & Cost", "Cache warm", "bool", "`prompt_cache.warm` from the status line; else whether the last API call is younger than the observed TTL", ["D3", "D2"], "", "≈ without the shim"),
    metric!(cache_expires_in, "Tokens & Cost", "Cache expires in", "ms", "`prompt_cache.expires_at` − now, clock-driven between status rewrites; else last API call + observed TTL − now", ["D3", "D2"], "The status file is rewritten only at expiry, so the countdown runs on cctop's clock; the shim's figure is ignored when the file predates the last assistant line", "≈ without the shim, or when the status file is stale"),
    metric!(cache_recache_if_cold, "Tokens & Cost", "Re-cache if cold", "tokens", "`prompt_cache.recache_tokens_if_cold`: what the next call re-writes if the cache expires first", ["D3"], "", ""),
    metric!(cache_misses, "Tokens & Cost", "Cache misses", "count", "`prompt_cache.misses` with `miss_causes` (model_changed, tools_changed, messages_rewritten, ttl_expired_1h/5m, likely_server_side…) and `expected_rebuilds`", ["D3"], "Claude Code counts a miss when the cache read is < 95 % of the input and ≥ 2 000 tokens were re-processed", ""),
    metric!(cost, "Tokens & Cost", "Cost", "USD", "Claude Code's `cost-state.totalCostUSD` plus a priced estimate of responses newer than that line", ["D11", "D2", "D9"], "Subscription plans have no per-token bill; the figure is the API-equivalent list price", "≈ when any part is estimated"),
    metric!(cost_combined, "Tokens & Cost", "Combined cost", "USD", "The session's whole spend: `cost-state.totalCostUSD` (which already holds the subagents' calls and calls no transcript shows), plus the priced main responses after it, plus the priced subagent calls whose line timestamp is after the ledger's moment (the last timestamped line before the cost-state); with no cost-state, every main and agent call priced. Panel 2's headline, dashboard row 2, `cctop report` and `cctop query`'s `cost_combined`", ["D2", "D2a", "D9", "D11"], "Never a subtraction: `main` and `agents` are not derived from the ledger. A fork's replayed parent message is not priced. `source` says ledger / priced / mixed; `cost` keeps the main-only meaning for one release", "≈ when any priced part is non-zero"),
    metric!(cost_by_model, "Tokens & Cost", "Cost by model", "USD", "`cost-state.modelUsage[*].costUSD` plus estimates per model", ["D11", "D9"], "", "≈ when any part is estimated"),
    metric!(cost_per_call, "Tokens & Cost", "Cost per call", "USD", "context × cache-read price + median output × output price, at the current context; at the cache-write price of the observed TTL when the cache is cold", ["D2", "D3", "D9"], "What the next API call costs, not what the last one did", "≈ (always priced from the table)"),
    metric!(cost_per_turn, "Tokens & Cost", "Cost per turn", "USD", "cost per call × the session's own median calls per turn (turns with ≥ 1 call), also given at 100 k of context; `next 30 calls` = cost per call × 30", ["D2", "D9"], "The median is the session's, never a constant", "≈ (always priced from the table)"),
    metric!(attribution, "Tokens & Cost", "Where the tokens went", "ratio", "Input tokens of API responses by their `attributionSkill` / `attributionPlugin` / `attributionAgent` / `attributionMcpServer` owner, machine-originated turns under `idle`, the subagents' own usage under `agents`; shares of all input tokens", ["D2", "D6"], "A response without an attribution key is the person's own work", ""),
    metric!(agents_cost, "Tokens & Cost", "Agents cost", "USD", "Priced usage of every subagent transcript (incl. `subagents/workflows/**`) and its share of the session's total", ["D2a", "D9"], "", "≈ (priced from the table)"),
    metric!(team_cost, "Tokens & Cost", "Team cost", "USD", "Σ over the teammates of the team this session leads, each from its own transcript: its `cost-state.totalCostUSD` where one exists, plus its priced calls after that line (all of them while it has none); members found from `~/.claude/teams/<team>/config.json`, the lead's `teammate_spawned` results and a scan of the transcripts naming the team (`teamName == session-<lead id8>`). Part of `cost_combined`, toggled with the agents by `a`", ["D12", "D2c", "D11", "D9"], "A teammate's ledger already holds its own subagents and hidden calls; never a subtraction. A member whose transcript is not found adds nothing and marks the sum", "≈ for a teammate's calls after its last cost-state, or all of them while it runs; ≈ and \"N of M read\" when a transcript is missing"),
    metric!(limit_weight, "Tokens & Cost", "Limit weight", "ratio", "`/usage`'s weight of a call: (cached + uncached × 10 + cache-create × 12.5 + output × 50) × tier (fable 10, opus 5, sonnet 3, haiku 1)", ["D2", "D12"], "Why the limit bar moves faster than dollars", ""),
    metric!(behaviour_flags, "Tokens & Cost", "Behaviour flags", "%", "`/usage`'s five flags as shares of the weighted usage: cache_miss (requests with > 100 k uncached tokens), long_context (> 150 k context), subagent_heavy, high_parallel (≥ 4 live sessions), cron (active ≥ 8 h); shown with Claude Code's own tip text at ≥ 10 %", ["D2", "D1", "D12"], "", ""),
    metric!(burn_rate, "Tokens & Cost", "Burn rate", "USD/h", "Cost of turns active in the trailing 15 minutes, scaled to an hour over the part of the window they cover", ["D2", "D9"], "A turn counts from its start (clamped to the window) to its last line; windows shorter than 1 minute are treated as 1 minute", "≈ (always priced from the table)"),
    metric!(input_rate, "Tokens & Cost", "Input rate", "tokens/min", "Total input tokens of turns started in the trailing 15 minutes ÷ window", ["D2"], "", ""),
    // -- Limits
    metric!(limit_5h, "Limits", "5-hour usage", "%", "`rate_limits.five_hour.used_percentage` from the status line", ["D3"], "Account-wide: other live sessions contribute", ""),
    metric!(limit_7d, "Limits", "7-day usage", "%", "`rate_limits.seven_day.used_percentage` from the status line", ["D3"], "Account-wide", ""),
    metric!(limit_reset, "Limits", "Resets in", "duration", "`resets_at` − now", ["D3"], "", ""),
    metric!(limit_hit, "Limits", "Rate limited", "enum", "The newest API-error line with `error: rate_limit` (or status 429): `quotaLimits.rateLimitType`, `resetsAt`, `lowPriorityRetryAfterSeconds` — cleared by the next successful call", ["D2"], "Exact without the shim: Claude Code writes the 429 into the transcript", ""),
    metric!(spend_limit, "Limits", "Spend limit", "%", "`rate_limits.spend_limit.used_percentage` from the status line, for accounts with a monthly limit", ["D3"], "", ""),
    metric!(other_sessions, "Limits", "Other sessions", "list", "Live registry entries other than this one: busy/idle and how long (`statusUpdatedAt`)", ["D1"], "They share the rate limit", ""),
    metric!(limit_exhaustion, "Limits", "Projected exhaustion", "duration", "Least-squares slope of used_percentage samples over the last 30 min, extrapolated to 100 %", ["D3"], "Needs ≥ 3 samples; rate-limit units are plan-specific, so tokens are not used", "≈ always"),
    // -- Turn
    metric!(turn_duration, "Turn", "Turn duration", "ms", "`turn_duration.durationMs` system line written when the turn ends", ["D2"], "", ""),
    metric!(api_calls, "Turn", "API calls", "count", "Distinct `message.id`s in the turn", ["D2"], "", ""),
    metric!(api_time, "Turn", "API time", "ms", "`cost-state.totalAPIDuration` for the session; per turn, gaps between a user/tool_result line and the next assistant line", ["D11", "D2"], "", "≈ per turn"),
    metric!(retry_time, "Turn", "Retry time", "ms", "`totalAPIDuration − totalAPIDurationWithoutRetries`", ["D11"], "", ""),
    metric!(phase, "Turn", "Phase", "enum", "The last call's phase over the last seven calls (`phase.rs`: EXPLORING / IMPLEMENTING / VERIFYING / COMMITTING / PLANNING / DELEGATING / BROWSING / OPS / WAITING) and its run length; WAITING when a permission dialog, an `AskUserQuestion` or a Notification is pending", ["D2", "D4"], "A test-class command is VERIFYING only once its output confirmed a run", ""),
    metric!(last_check, "Turn", "Last check", "enum", "The newest test-class Bash call whose output confirmed a run (`test result:`, `N passed`, `# pass`…), its verdict and age; `edits since` counts Edit/Write calls after it", ["D2"], "", ""),
    metric!(waiting, "Turn", "Waiting on you", "enum", "A pending permission dialog (hook), a running `AskUserQuestion` / `ExitPlanMode`, an `idle_prompt` / `agent_needs_input` notification, or a finished turn whose last text ended with `?`; with the wait's duration", ["D2", "D4"], "", ""),
    metric!(steers, "Turn", "Steers", "count", "Human `queued_command` attachments folded into the turn (absorbed mid-turn); task notifications are machine turns, not steers", ["D2"], "", ""),
    metric!(interrupts, "Turn", "Interrupts", "count", "`[Request interrupted by user…]` lines with `interruptedMessageId`, and the output tokens the cut turns had produced", ["D2"], "", ""),
    metric!(hook_by_command, "Turn", "Hook time by command", "ms", "`stop_hook_summary.hookInfos[].command` and `hook_success` attachments summed per command over the session; `preventedContinuation` marks a blocked stop", ["D2"], "", ""),
    metric!(goal, "Turn", "Goal", "enum", "The last `goal_status` attachment (`/goal`): met, iterations, tokens", ["D2"], "", ""),
    metric!(hook_runs, "Turn", "Hook runs", "count", "Number of `hookInfos` entries in the turn's `stop_hook_summary`", ["D2"], "Only Stop hooks are summarised by Claude Code; other hook events need `cctop install`", ""),
    metric!(hook_ms, "Turn", "Hook time", "ms", "Σ `hookInfos[].durationMs` for the turn", ["D2", "D4"], "", ""),
    metric!(permission_wait, "Turn", "Permission wait", "ms", "PermissionRequest → PostToolUse for the same tool_use_id, minus the tool's median duration", ["D4"], "PreToolUse fires before the prompt, so it cannot bound the wait", "≈ always"),
    metric!(queued_prompts, "Turn", "Queued prompts", "count", "`queue-operation` enqueue − dequeue/remove; popAll resets to 0", ["D2"], "", ""),
    // -- Tools
    metric!(tool_calls, "Tools", "Calls", "count", "`tool_use` blocks per tool name; MCP tools grouped as `mcp:<server>`", ["D2"], "", ""),
    metric!(tool_errors, "Tools", "Errors", "count", "`tool_result` blocks with `is_error` per tool", ["D2"], "", ""),
    metric!(tool_p50, "Tools", "p50 duration", "ms", "Median of tool_use → tool_result durations", ["D2", "D4"], "Transcript timings include any permission wait", "≈ until hook timings replace them"),
    metric!(tool_p95, "Tools", "p95 duration", "ms", "95th percentile (nearest rank) of durations", ["D2", "D4"], "", "≈ until hook timings replace them"),
    metric!(tool_last_call, "Tools", "Last call", "duration", "now − the tool's most recent `tool_use` timestamp", ["D2"], "", ""),
    metric!(tokens_to_ctx, "Tools", "Tokens → context", "tokens", "Σ len(result text) / 4 per tool, plus `w·h/750` per image (1 500 when the size is unknown); cleared results count 0", ["D2", "D10"], "Heuristic; exact with OpenTelemetry. Uses the text in the transcript, not offloaded `tool-results/` files (their size is shown beside it)", "≈ without OTel"),
    metric!(input_tokens, "Tools", "Input → context (IN→CTX)", "tokens", "Characters the model wrote as tool inputs / 4, per tool — they stay in context like results do", ["D2"], "Bash command text is the largest share", "≈ always"),
    metric!(bash_class, "Tools", "Bash by class", "count", "Bash calls by the phase classifier's class: explore / implement / test / build-lint / commit / gitread / ops / wait (`Bash·test` rows)", ["D2"], "", ""),
    metric!(error_class, "Tools", "Error class", "count", "Failed calls by Claude Code's own taxonomy (Command Failed / User Rejected / Edit Failed / File Changed / File Too Large / File Not Found / Other) plus Content Not Found, Timeout, Tool Not Found and Denied (`toolDenialKind`)", ["D2"], "Classified from the result text, in Claude Code's order", ""),
    metric!(top_ctx, "Tools", "Top context consumers", "tokens", "The n single results with the largest `tokens_to_ctx`; ⊘ marks a result cut at a cap (`truncatedByTokenCap`, a persisted spill)", ["D2"], "", "≈ without OTel"),
    metric!(reread_tax, "Tools", "Re-read tax", "USD", "API calls since the result landed × its tokens × the cache-read price: what re-reading it has cost so far", ["D2", "D9"], "", "≈ always"),
    metric!(tool_search_loads, "Tools", "ToolSearch loads", "count", "Deferred tools loaded through `ToolSearch` per MCP server (`matches` of its result); each load rewrites the cached prefix", ["D2"], "", ""),
    // -- Agents & MCP
    metric!(agent_state, "Agents & MCP", "Agent state", "enum", "running while tool_uses are pending; done when the last response ends with text and no pending tool_use; failed when the last result is an error and nothing followed for 60 s", ["D2a", "D4"], "", ""),
    metric!(agent_cost, "Agents & MCP", "Agent cost", "USD", "The agent's own calls priced from the table (a fork's replayed parent message excluded); `—` with the token count on a model the table does not know", ["D2a", "D9"], "", "≈ always"),
    metric!(agent_status, "Agents & MCP", "Agent status", "enum", "`<status>` of the agent's `<task-notification>` (completed / failed / killed), by whichever of Claude Code's three deliveries it came — a user line, a `queue-operation` enqueue or a `queued_command` attachment; absent until it lands", ["D2"], "Claude Code's word beats the 60 s heuristic of `agent_state` where both exist", "never"),
    metric!(agent_returned, "Agents & MCP", "Agent returned", "tokens", "Length of the notification's `<result>` ÷ 4 (a synchronous `Agent` result's content ÷ 4): what came back into the session; absent until a result exists, 0 for an empty one", ["D2"], "A size, never a judgement of the result; the text is not kept", "≈ always"),
    metric!(agent_waste, "Agents & MCP", "Agent waste", "USD", "The agent's priced cost under one reason, tested in order: `failed` (the notification, a workflow-journal `failed` entry, or the 60 s heuristic without a notification), `killed`, `no ret` (completed with an absent or empty result), `idle` (no notification, running, no line for 5 min, nothing in flight — no tool the hook spool saw start and not finish, none the transcript shows unanswered); a finished agent without a notification is not waste, and a notification's `completed` outranks the journal", ["D2", "D2a", "D4", "D9"], "Structural evidence only: statuses, lengths, counts, timings", "≈ always"),
    metric!(agents_waste, "Agents & MCP", "Agents waste", "USD", "Σ `agent_waste` by reason over every subagent, and the agents classified", ["D2", "D2a", "D4", "D9"], "", "≈ always"),
    metric!(agents_cold_starts, "Agents & MCP", "Cold starts", "count", "Agents (forks excluded) whose first call wrote more cache than it read, and the cache-write dollars of those first calls", ["D2a", "D9"], "A cost of the design, not counted as waste", "≈ for the dollars"),
    metric!(agents_return_ratio, "Agents & MCP", "Return ratio", "ratio", "Σ `agent_returned` ÷ Σ agent output tokens: the share of what the agents wrote that came back", ["D2", "D2a"], "", "≈ always"),
    metric!(agent_tokens, "Agents & MCP", "Agent tokens", "tokens", "Deduplicated usage of the agent's own transcript: the last line of each `message.id` (subagent transcripts stream `output_tokens`), without a fork's replayed first message (the parent's launching response, billed in the parent)", ["D2a"], "", ""),
    metric!(mcp_rss, "Agents & MCP", "MCP memory", "bytes", "RSS of the MCP server process", ["D5"], "", ""),
    metric!(mcp_calls, "Agents & MCP", "MCP calls", "count", "Calls of tools named `mcp__<server>__*`", ["D2"], "", ""),
    metric!(agent_workflows, "Agents & MCP", "Workflow runs", "count", "`subagents/workflows/<run>/journal.jsonl`: agents launched, finished (`result`) and `failed` per run; the run's agents are scanned like the top-level ones", ["D2a"], "", ""),
    metric!(agent_depth, "Agents & MCP", "Spawn depth", "count", "Deepest `spawnDepth` among the agents (Claude Code caps it at 3)", ["D2a"], "", ""),
    metric!(teammates, "Agents & MCP", "Teammates", "list", "Members of `~/.claude/teams/<team>/config.json` when this session leads the team", ["D12"], "", ""),
    metric!(teammate_cost, "Agents & MCP", "Teammate cost", "USD", "The teammate's own `cost-state.totalCostUSD` plus its priced calls after that line; all of them priced while it has none; `—` with `no transcript` when its file is not found", ["D2c", "D11", "D9"], "Claude Code's own number once the teammate ended", "≈ while it runs or after its ledger's moment"),
    metric!(teammate_context, "Agents & MCP", "Teammate context", "tokens", "The teammate's current context: the input of its last API call, as Panel 1 computes the lead's", ["D2c"], "", ""),
    metric!(teammate_tokens, "Agents & MCP", "Teammate tokens", "tokens", "Deduplicated usage of the teammate's transcript (one figure per `message.id`), as the lead's Panel 2", ["D2c"], "", ""),
    metric!(teammate_turns, "Agents & MCP", "Teammate turns", "count", "Turns of the teammate's transcript, human / machine: a `<teammate-message>` starts a machine turn", ["D2c"], "", ""),
    metric!(teammate_state, "Agents & MCP", "Teammate state", "enum", "active (`isActive: true` in the team config) · recent (no config; a line in the last 5 min) · ended (its file ends with a `cost-state`, or `isActive: false`) · gone (no ledger, no config, no recent line) · missing (known from the config or a spawn, no transcript found)", ["D12", "D2c"], "No process mapping: liveness is Claude Code's flag or line recency", ""),
    metric!(teammate_waste, "Agents & MCP", "Teammate waste", "USD", "The cost of the teammate's current turn under one reason: `idle` (alive, no API call for 5 min, no tool call unanswered in its transcript) or `errored` (its last response was an API-error line and nothing followed for 60 s)", ["D2c", "D9"], "Structural evidence of its own transcript only; no inbox, no message body", "≈ always"),
    metric!(team_waste, "Agents & MCP", "Team waste", "USD", "Σ `teammate_waste` over the team", ["D2c", "D9"], "", "≈ always"),
    metric!(mcp_auth, "Agents & MCP", "MCP needs auth", "list", "`deferred_tools_delta.needsAuthMcpServers` / `failedMcpServers` from the transcript", ["D2"], "", ""),
    // -- Files
    metric!(file_touches, "Files", "Touches", "count", "Read / Edit / Write / MultiEdit / NotebookEdit calls per file path, plus Bash `cat` / `sed -n` / `head` / `tail` reads of it", ["D8"], "A read counts when its result arrives", ""),
    metric!(file_lines, "Files", "Lines ±", "lines", "`git diff --numstat` against HEAD at attach time", ["D7"], "Outside a git repo the column is empty", ""),
    metric!(uncommitted, "Files", "Uncommitted", "lines", "`git diff --numstat HEAD` (added, removed, files) and the last commit Claude Code summarised (`gitOperation.commit`) with the edits since it", ["D7", "D2"], "", ""),
    metric!(rewind_points, "Files", "Rewind points", "count", "`file-history-snapshot` lines in the current turn (checkpoints `/rewind` can restore) and the Bash writes of the turn no checkpoint covers; per file: the checkpoint version (`file-history-delta`, ⚠ at v8+), IDE edits (`edited_text_file`), stale markers (`staleRecovered`, `staleReadFileStateHint`), edit → re-read → edit churn", ["D2"], "", ""),
    metric!(file_rereads, "Files", "Re-reads", "count", "Whole-file reads (Read or a Bash reader) with no Edit/Write in between; ⚠ at ≥ 3", ["D8"], "Ranged reads (offset/limit) and `file_unchanged` results do not count; the counter resets when the file changed under the model (an IDE edit, a stale-read recovery) and at every context boundary", ""),
    // -- Advisor
    metric!(advice_saving, "Advisor", "Estimated saving", "tokens|seconds", "Rule-specific estimate of what following the advice saves per remaining turn", ["D2"], "Ranking key; always an estimate", "≈ always"),
    // -- Coach (the four lights; the dashboard's tiles enlarge their figures)
    metric!(coach_context, "Coach", "Context light", "percent", "The context size as % of the exact window; ○ below 150k, ◐ from 150k (or ≥ 300k on a 1M window while a turn runs), ● inside the autocompact warn band (effective window − 13 000 − 20 000) or at ≥ 300k with a clean stop available", ["D1", "D3"], "Never a fixed 80 %: a deliberate 1M session sits amber", "≈ when the window is the model default"),
    metric!(coach_cache, "Coach", "Cache light", "minutes|tokens", "Minutes of cache left (`prompt_cache.expires_at`, else last call + observed TTL ≈), or the re-write size when cold; ◐ inside the countdown band (the last 5 min of a 1 h entry, 2 min of a 5 m one), ● when a reply now would save ≥ 50k", ["D1", "D3"], "", "≈ without the status-line shim"),
    metric!(coach_limits, "Coach", "Limits light", "percent", "The 5 h window used; ◐ when the exhaustion fit lands before the reset or ≥ 80 %, ● on a rate-limit or spend-limit error line; `—` without the status line", ["D3", "D1"], "", ""),
    metric!(coach_rework, "Coach", "Rework light", "count", "Open issues: consecutive failed calls of the turn (denials excluded), corrections (interrupts, rejected calls) in the last three turns, blocked calls; else the source edits since the last confirmed test run. ◐ after 10 min or 14 calls unverified, two fails, a PR without a review, an uncommitted tail; ● on a cascade, a denial streak, a correction streak, destructive git on a dirty tree, a commit without a check", ["D1", "D7"], "", ""),
];

/// Look up a metric by id.
pub fn get(id: &str) -> Option<&'static Metric> {
    METRICS.iter().find(|m| m.id == id)
}

/// Markdown reference grouped by panel, as written to `docs/metrics.md`.
pub fn markdown() -> String {
    let mut out = String::from("# cctop metrics\n\nGenerated by `cctop metrics --md` from `src/metrics/registry.rs` — do not edit by hand.\n\nSources: D1 session registry · D2 transcript · D2a subagent transcripts · D2c teammate transcripts (`<projects>/<slug>/<sessionId>.jsonl` matched by `teamName`) · D3 status-line JSON (shim) · D4 hooks · D5 process tree · D7 git · D8 file tool inputs · D9 price table · D10 OpenTelemetry · D11 `cost-state` · D12 Claude Code's own configuration (`~/.claude.json`, settings, the process environment, `~/.claude/teams/<team>/config.json`).\n");
    for panel in PANELS {
        let ms: Vec<&Metric> = METRICS.iter().filter(|m| m.panel == *panel).collect();
        if ms.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {panel}\n\n| Metric | Unit | How it is computed | Sources | Caveats | Estimate |\n|---|---|---|---|---|---|\n"));
        for m in ms {
            out.push_str(&format!(
                "| **{}** <a id=\"{}\"></a> `{}` | {} | {} | {} | {} | {} |\n",
                m.name,
                m.id,
                m.id,
                m.unit,
                m.formula.replace('|', "\\|"),
                m.sources.join(" "),
                if m.caveats.is_empty() {
                    "—"
                } else {
                    m.caveats
                },
                if m.estimate_when.is_empty() {
                    "never"
                } else {
                    m.estimate_when
                },
            ));
        }
    }
    out
}

pub const README_START: &str = "<!-- metrics:start -->";
pub const README_END: &str = "<!-- metrics:end -->";

/// Replace the marked block in a README with the generated reference
/// (headings demoted one level so they nest under the README's section).
pub fn splice_readme(readme: &str) -> Option<String> {
    let start = readme.find(README_START)? + README_START.len();
    let end = readme.find(README_END)?;
    let body = markdown()
        .lines()
        .skip_while(|l| !l.starts_with("## "))
        .map(|l| {
            if l.starts_with("## ") {
                format!("#{l}")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "{}\n{}\n{}",
        &readme[..start],
        body.trim_end(),
        &readme[end..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::Path;

    #[test]
    fn ids_are_unique_snake_case_and_panels_known() {
        let mut seen = HashSet::new();
        for m in METRICS {
            assert!(seen.insert(m.id), "duplicate id {}", m.id);
            assert!(
                m.id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "{} is not snake_case",
                m.id
            );
            assert!(
                PANELS.contains(&m.panel),
                "{} has unknown panel {}",
                m.id,
                m.panel
            );
            assert!(
                !m.formula.is_empty() && !m.sources.is_empty(),
                "{} incomplete",
                m.id
            );
        }
    }

    #[test]
    fn required_metrics_are_registered() {
        for id in [
            "cache_read",
            "cache_write",
            "fresh_input",
            "output",
            "thinking",
            "cache_hit_ratio",
            "cost",
            "burn_rate",
            "context_size",
            "context_velocity",
            "turns_until_compaction",
            "tool_calls",
            "tool_errors",
            "tool_p50",
            "tool_p95",
            "tokens_to_ctx",
            "hook_ms",
            "turn_duration",
        ] {
            assert!(get(id).is_some(), "{id} missing from registry");
        }
    }

    #[test]
    fn docs_metrics_md_is_up_to_date() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/metrics.md");
        let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(
            on_disk,
            markdown(),
            "docs/metrics.md is stale — run `cctop metrics --md > docs/metrics.md`"
        );
    }

    #[test]
    fn readme_block_is_up_to_date() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let readme = std::fs::read_to_string(&path).unwrap();
        let spliced = splice_readme(&readme).expect("README has metrics markers");
        assert_eq!(
            readme, spliced,
            "README metrics block is stale — run `cctop metrics --readme README.md`"
        );
    }

    #[test]
    fn splice_replaces_only_the_marked_block() {
        let r = "before\n<!-- metrics:start -->\nold\n<!-- metrics:end -->\nafter\n";
        let s = splice_readme(r).unwrap();
        assert!(s.starts_with("before\n<!-- metrics:start -->\n### Header"));
        assert!(s.ends_with("<!-- metrics:end -->\nafter\n"));
        assert!(!s.contains("\nold\n"));
        assert!(splice_readme("no markers").is_none());
    }
}
