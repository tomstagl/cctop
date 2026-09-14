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
    metric!(cost_by_model, "Tokens & Cost", "Cost by model", "USD", "`cost-state.modelUsage[*].costUSD` plus estimates per model", ["D11", "D9"], "", "≈ when any part is estimated"),
    metric!(burn_rate, "Tokens & Cost", "Burn rate", "USD/h", "Cost of turns active in the trailing 15 minutes, scaled to an hour over the part of the window they cover", ["D2", "D9"], "A turn counts from its start (clamped to the window) to its last line; windows shorter than 1 minute are treated as 1 minute", "≈ (always priced from the table)"),
    metric!(input_rate, "Tokens & Cost", "Input rate", "tokens/min", "Total input tokens of turns started in the trailing 15 minutes ÷ window", ["D2"], "", ""),
    // -- Limits
    metric!(limit_5h, "Limits", "5-hour usage", "%", "`rate_limits.five_hour.used_percentage` from the status line", ["D3"], "Account-wide: other live sessions contribute", ""),
    metric!(limit_7d, "Limits", "7-day usage", "%", "`rate_limits.seven_day.used_percentage` from the status line", ["D3"], "Account-wide", ""),
    metric!(limit_reset, "Limits", "Resets in", "duration", "`resets_at` − now", ["D3"], "", ""),
    metric!(limit_exhaustion, "Limits", "Projected exhaustion", "duration", "Least-squares slope of used_percentage samples over the last 30 min, extrapolated to 100 %", ["D3"], "Needs ≥ 3 samples; rate-limit units are plan-specific, so tokens are not used", "≈ always"),
    // -- Turn
    metric!(turn_duration, "Turn", "Turn duration", "ms", "`turn_duration.durationMs` system line written when the turn ends", ["D2"], "", ""),
    metric!(api_calls, "Turn", "API calls", "count", "Distinct `message.id`s in the turn", ["D2"], "", ""),
    metric!(api_time, "Turn", "API time", "ms", "`cost-state.totalAPIDuration` for the session; per turn, gaps between a user/tool_result line and the next assistant line", ["D11", "D2"], "", "≈ per turn"),
    metric!(retry_time, "Turn", "Retry time", "ms", "`totalAPIDuration − totalAPIDurationWithoutRetries`", ["D11"], "", ""),
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
    metric!(tokens_to_ctx, "Tools", "Tokens → context", "tokens", "Σ len(result text) / 4 per tool", ["D2", "D10"], "Heuristic; exact with OpenTelemetry. Uses the truncated text in the transcript, not offloaded `tool-results/` files", "≈ without OTel"),
    metric!(top_ctx, "Tools", "Top context consumers", "tokens", "The n single results with the largest `tokens_to_ctx`", ["D2"], "", "≈ without OTel"),
    // -- Agents & MCP
    metric!(agent_state, "Agents & MCP", "Agent state", "enum", "running while tool_uses are pending; done when the last response ends with text and no pending tool_use; failed when the last result is an error and nothing followed for 60 s", ["D2a", "D4"], "", ""),
    metric!(agent_tokens, "Agents & MCP", "Agent tokens", "tokens", "Deduplicated usage of the agent's own transcript", ["D2a"], "", ""),
    metric!(mcp_rss, "Agents & MCP", "MCP memory", "bytes", "RSS of the MCP server process", ["D5"], "", ""),
    metric!(mcp_calls, "Agents & MCP", "MCP calls", "count", "Calls of tools named `mcp__<server>__*`", ["D2"], "", ""),
    // -- Files
    metric!(file_touches, "Files", "Touches", "count", "Read / Edit / Write / MultiEdit / NotebookEdit calls per file path, plus Bash `cat` / `sed -n` / `head` / `tail` reads of it", ["D8"], "A read counts when its result arrives", ""),
    metric!(file_lines, "Files", "Lines ±", "lines", "`git diff --numstat` against HEAD at attach time", ["D7"], "Outside a git repo the column is empty", ""),
    metric!(file_rereads, "Files", "Re-reads", "count", "Whole-file reads (Read or a Bash reader) with no Edit/Write in between; ⚠ at ≥ 3", ["D8"], "Ranged reads (offset/limit) and `file_unchanged` results do not count; the counter resets when the file changed under the model (an IDE edit, a stale-read recovery) and at every context boundary", ""),
    // -- Advisor
    metric!(advice_saving, "Advisor", "Estimated saving", "tokens|seconds", "Rule-specific estimate of what following the advice saves per remaining turn", ["D2"], "Ranking key; always an estimate", "≈ always"),
];

/// Look up a metric by id.
pub fn get(id: &str) -> Option<&'static Metric> {
    METRICS.iter().find(|m| m.id == id)
}

/// Markdown reference grouped by panel, as written to `docs/metrics.md`.
pub fn markdown() -> String {
    let mut out = String::from("# cctop metrics\n\nGenerated by `cctop metrics --md` from `src/metrics/registry.rs` — do not edit by hand.\n\nSources: D1 session registry · D2 transcript · D2a subagent transcripts · D3 status-line JSON (shim) · D4 hooks · D5 process tree · D7 git · D8 file tool inputs · D9 price table · D10 OpenTelemetry · D11 `cost-state`.\n");
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
