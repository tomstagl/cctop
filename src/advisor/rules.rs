//! Rules A01–A06. Each is a predicate over `State` with the wording from the
//! PRD's catalog; they fire only on evidence from this session.

use super::{Advice, Rule, Saving};
use crate::transcript::CacheTtl;
use crate::ui::fmt;
use crate::ui::State;

pub fn all() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(CacheMiss),
        Box::new(CacheExpiry),
        Box::new(RunawayResult),
        Box::new(Rereads),
        Box::new(ExploreInMain),
        Box::new(CompactionChurn),
    ]
}

/// Explanation shown for `Enter` on an advice.
pub fn explain(doc_key: &str) -> &'static str {
    match doc_key {
        "A01" => "Prompt caching only works on an unchanged prefix. If any byte before the last cache breakpoint differs between requests, the whole prefix is re-written at the cache-write price (1.25× or 2× input) instead of read at 0.1×. Usual causes: a hook or status line that injects the time, a random id, or a counter; CLAUDE.md edited mid-session; tool schemas that change order.",
        "A02" => "A cache entry lives 5 minutes (or 1 hour on this session, as the API reported). After that, the next request re-writes the entire context. Batching questions or keeping the session busy avoids the cold write; a cold turn on a 400k context costs as much as ~20 warm ones.",
        "A03" => "Every byte of a tool result stays in context for the rest of the session and is re-read (and billed) on every later request. One 4k-token `ls -R` repeated twice is 8k tokens on every subsequent call. Narrow the command (head, grep, Glob) or delegate the exploration to a subagent that returns a summary.",
        "A04" => "Reading a file again re-injects it whole. If the model needs to see it again, a Read with offset/limit on the relevant range costs a fraction; better, ask it to note the lines it needs in its own summary.",
        "A05" => "Long runs of Read/Grep/Glob in the main thread fill the main context with raw files that persist until compaction. An Explore subagent does the same search in its own context and hands back a paragraph.",
        "A06" => "Each compaction spends output tokens on a summary and loses detail the model then re-reads. Two compactions in one session usually cost more than ending the session at a natural boundary with a short hand-off note and starting fresh.",
        _ => "No explanation for this rule yet.",
    }
}

fn recent_turns(state: &State, n: usize) -> Vec<&crate::metrics::Turn> {
    state
        .agg
        .turns
        .iter()
        .filter(|t| t.api_calls > 0)
        .rev()
        .take(n)
        .collect()
}

/// A01 — cache-hit ratio < 60 % over 5 turns with cache writes that are not
/// explained by large tool output.
pub struct CacheMiss;
impl Rule for CacheMiss {
    fn id(&self) -> &'static str {
        "A01"
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let recent = recent_turns(state, 5);
        if recent.len() < 5 {
            return None;
        }
        let mut u = crate::metrics::Usage::default();
        for t in &recent {
            u.add(&t.usage);
        }
        let ratio = u.cache_hit_ratio()?;
        if ratio >= 0.6 {
            return None;
        }
        // Writes that outstrip what tool output could have added are the signal.
        let unexplained = recent
            .iter()
            .filter(|t| t.usage.cache_write() > (t.tool_result_bytes / 4) * 2 + 2_000)
            .count();
        if unexplained < 3 {
            return None;
        }
        let per_turn = u.cache_write() / 5;
        Some(Advice {
            rule: "A01",
            headline: format!("Cache hit ratio {:.0} % over the last 5 turns", ratio * 100.0),
            evidence: format!("{} turns re-wrote cache without matching tool output ({}/turn)", unexplained, fmt::tokens(per_turn)),
            action: "Something in the prefix changes every turn — check hooks or a status line injecting time/random values, or a CLAUDE.md edited mid-session.".into(),
            saving: Saving::Tokens(per_turn * 9 / 10),
            doc_key: "A01",
        })
    }
}

/// A02 — a prompt gap longer than the observed cache TTL followed by a
/// full-context cache write.
pub struct CacheExpiry;
impl Rule for CacheExpiry {
    fn id(&self) -> &'static str {
        "A02"
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let ttl_ms: i64 = match state.agg.observed_ttl {
            Some(CacheTtl::OneHour) => 60 * 60 * 1000,
            _ => 5 * 60 * 1000,
        };
        let turns: Vec<_> = state.agg.turns.iter().filter(|t| t.api_calls > 0).collect();
        let parse = crate::metrics::cost::parse_ts_ms;
        let mut cold: Option<(usize, i64, u64)> = None;
        for w in turns.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (Some(end), Some(start)) = (
                a.last_at.as_deref().and_then(parse),
                b.started_at.as_deref().and_then(parse),
            ) else {
                continue;
            };
            let gap = start - end;
            let write = b.usage.cache_write();
            if gap > ttl_ms && write as f64 >= b.context_size as f64 * 0.7 && b.context_size > 0 {
                cold = Some((b.number, gap, write));
            }
        }
        let (turn, gap, write) = cold?;
        let ttl_label = if ttl_ms >= 3_600_000 { "1 h" } else { "5 min" };
        Some(Advice {
            rule: "A02",
            headline: format!("Prompt cache expired before turn {turn}"),
            evidence: format!("{} idle > {ttl_label} TTL, then {} re-written", fmt::duration_ms(gap), fmt::tokens(write)),
            action: format!("Batch your questions or keep the session warm; each cold turn re-writes the whole context (TTL {ttl_label} on this session)."),
            saving: Saving::Tokens(write),
            doc_key: "A02",
        })
    }
}

/// A03 — one tool result > 3k tokens, or the same input > 2k twice.
pub struct RunawayResult;
impl Rule for RunawayResult {
    fn id(&self) -> &'static str {
        "A03"
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let calls = &state.tools.calls;
        let mut repeats: std::collections::HashMap<(&str, &str), (usize, u64)> = Default::default();
        for c in calls {
            let e = repeats
                .entry((c.name.as_str(), c.input_summary.as_str()))
                .or_default();
            e.0 += 1;
            e.1 = e.1.max(c.result_tokens_est);
        }
        let single = calls
            .iter()
            .filter(|c| c.result_tokens_est > 3_000)
            .max_by_key(|c| c.result_tokens_est);
        let repeated = repeats
            .iter()
            .filter(|(_, (n, max))| *n >= 2 && *max > 2_000)
            .max_by_key(|(_, (n, max))| *max * *n as u64);
        match (single, repeated) {
            (_, Some(((name, input), (n, max)))) if repeated.is_some() && single.is_none_or(|s| s.result_tokens_est < max * *n as u64) => Some(Advice {
                rule: "A03",
                headline: format!("`{name} {input}` pushed {} tokens into context, {n}×", fmt::tokens(*max)),
                evidence: format!("{} in context on every later request", fmt::tokens(max * *n as u64)),
                action: "Pipe through `head -50`, use Glob/Grep, or ask a subagent for a summary.".into(),
                saving: Saving::Tokens(max * *n as u64),
                doc_key: "A03",
            }),
            (Some(c), _) => Some(Advice {
                rule: "A03",
                headline: format!("`{} {}` pushed {} tokens into context", c.name, c.input_summary, fmt::tokens(c.result_tokens_est)),
                evidence: "one result larger than 3k tokens stays in context for the rest of the session".into(),
                action: "Narrow the command (head, grep, Glob) or delegate to a subagent that returns a summary.".into(),
                saving: Saving::Tokens(c.result_tokens_est),
                doc_key: "A03",
            }),
            _ => None,
        }
    }
}

/// A04 — a file read ≥ 3× with no edit in between.
pub struct Rereads;
impl Rule for Rereads {
    fn id(&self) -> &'static str {
        "A04"
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let f = state
            .files
            .files
            .values()
            .filter(|f| f.reread_warning())
            .max_by_key(|f| f.reads_since_edit)?;
        let per_read: u64 = state
            .tools
            .calls
            .iter()
            .filter(|c| {
                c.name == "Read"
                    && c.input_summary
                        .ends_with(&fmt::clip(&f.path, 30).trim_start_matches('…').to_string())
            })
            .map(|c| c.result_tokens_est)
            .max()
            .unwrap_or(1_000);
        let short = std::path::Path::new(&f.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| f.path.clone());
        Some(Advice {
            rule: "A04",
            headline: format!("{short} was read {}× with no edit in between", f.reads_since_edit),
            evidence: format!("each read re-injects ~{} tokens", fmt::tokens(per_read)),
            action: "Read once with an offset/limit, or ask Claude to keep the relevant lines in its summary.".into(),
            saving: Saving::Tokens(per_read * (f.reads_since_edit as u64 - 1)),
            doc_key: "A04",
        })
    }
}

/// A05 — ≥ 8 consecutive Read/Grep/Glob calls with no Edit/Write.
pub struct ExploreInMain;
impl Rule for ExploreInMain {
    fn id(&self) -> &'static str {
        "A05"
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let mut run = 0usize;
        let mut run_tokens = 0u64;
        let mut best = (0usize, 0u64);
        for c in &state.tools.calls {
            match c.name.as_str() {
                "Read" | "Grep" | "Glob" => {
                    run += 1;
                    run_tokens += c.result_tokens_est;
                    if run > best.0 {
                        best = (run, run_tokens);
                    }
                }
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
                    run = 0;
                    run_tokens = 0;
                }
                _ => {}
            }
        }
        if best.0 < 8 {
            return None;
        }
        Some(Advice {
            rule: "A05",
            headline: format!("{} consecutive Read/Grep/Glob calls in the main context", best.0),
            evidence: format!("{} tokens of file dumps now ride on every request", fmt::tokens(best.1)),
            action: "Delegate discovery to an Explore subagent — it returns a paragraph instead of the files.".into(),
            saving: Saving::Tokens(best.1.saturating_sub(1_000)),
            doc_key: "A05",
        })
    }
}

/// A06 — ≥ 2 compactions, or one projected within 3 turns.
pub struct CompactionChurn;
impl Rule for CompactionChurn {
    fn id(&self) -> &'static str {
        "A06"
    }
    fn evaluate(&self, state: &State) -> Option<Advice> {
        let v = state.context();
        let soon = v.turns_until_compaction().filter(|n| *n <= 3.0);
        if v.compactions.len() < 2 && soon.is_none() {
            return None;
        }
        let evidence = match soon {
            Some(n) => format!(
                "{} compaction(s) so far, next in ~{} turns at +{}/turn",
                v.compactions.len(),
                n.ceil() as u64,
                fmt::tokens(v.velocity as u64)
            ),
            None => format!("{} compactions so far", v.compactions.len()),
        };
        Some(Advice {
            rule: "A06",
            headline: "Compaction is coming again".into(),
            evidence,
            action: "Finish this unit of work, then /clear or start a fresh session with a hand-off note.".into(),
            saving: Saving::Avoids,
            doc_key: "A06",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::Pricing;
    use crate::transcript::Line;
    use crate::ui::state::tests_support::fixture_state;

    fn fires(rule: &dyn Rule, s: &State) -> Option<Advice> {
        rule.evaluate(s)
    }

    fn prompt(ts: &str) -> Line {
        Line::parse(&format!(
            r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"go"}}}}"#
        ))
        .unwrap()
    }
    fn response(id: &str, ts: &str, input: u64, write: u64, read: u64) -> Line {
        Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5","content":[{{"type":"text","text":"ok"}}],"usage":{{"input_tokens":{input},"cache_creation_input_tokens":{write},"cache_read_input_tokens":{read},"output_tokens":10}}}}}}"#)).unwrap()
    }
    fn tool(id: &str, ts: &str, name: &str, input: &str, result_bytes: usize) -> [Line; 2] {
        [
            Line::parse(&format!(r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"m","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{{"{}":"{input}"}}}}],"usage":{{"output_tokens":1}}}}}}"#, if name == "Bash" { "command" } else { "file_path" })).unwrap(),
            Line::parse(&format!(r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{}"}}]}}}}"#, "x".repeat(result_bytes))).unwrap(),
        ]
    }

    #[test]
    fn a01_cache_miss() {
        let mut s = State::new(Pricing::bundled());
        for i in 0..5 {
            s.apply(&prompt("2026-01-01T00:00:00Z"));
            s.apply(&response(
                &format!("m{i}"),
                "2026-01-01T00:00:01Z",
                100,
                50_000,
                10_000,
            ));
        }
        let a = fires(&CacheMiss, &s).expect("fires");
        assert!(
            a.headline.starts_with("Cache hit ratio 17 %"),
            "{}",
            a.headline
        );
        assert!(matches!(a.saving, Saving::Tokens(t) if t > 40_000));
        // Healthy: mostly reads → no advice. And the real fixture (98 % hits) is quiet.
        let mut ok = State::new(Pricing::bundled());
        for i in 0..5 {
            ok.apply(&prompt("2026-01-01T00:00:00Z"));
            ok.apply(&response(
                &format!("m{i}"),
                "2026-01-01T00:00:01Z",
                100,
                1_000,
                100_000,
            ));
        }
        assert!(fires(&CacheMiss, &ok).is_none());
        assert!(fires(&CacheMiss, &fixture_state()).is_none());
    }

    #[test]
    fn a02_cache_expiry() {
        let mut s = State::new(Pricing::bundled());
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&response("m1", "2026-01-01T00:00:05Z", 10, 100_000, 0));
        // 12 minutes later (TTL 5 min): full re-write.
        s.apply(&prompt("2026-01-01T00:12:00Z"));
        s.apply(&response("m2", "2026-01-01T00:12:05Z", 10, 100_000, 0));
        let a = fires(&CacheExpiry, &s).expect("fires");
        assert!(
            a.evidence.contains("11:55 idle > 5 min TTL"),
            "{}",
            a.evidence
        );
        // Warm follow-up within TTL: no advice.
        let mut ok = State::new(Pricing::bundled());
        ok.apply(&prompt("2026-01-01T00:00:00Z"));
        ok.apply(&response("m1", "2026-01-01T00:00:05Z", 10, 100_000, 0));
        ok.apply(&prompt("2026-01-01T00:02:00Z"));
        ok.apply(&response("m2", "2026-01-01T00:02:05Z", 10, 500, 100_000));
        assert!(fires(&CacheExpiry, &ok).is_none());
    }

    #[test]
    fn a03_runaway_result() {
        let mut s = State::new(Pricing::bundled());
        for (i, l) in tool("t1", "2026-01-01T00:00:00Z", "Bash", "ls -R", 20_000)
            .iter()
            .chain(tool("t2", "2026-01-01T00:00:10Z", "Bash", "ls -R", 20_000).iter())
            .enumerate()
        {
            let _ = i;
            s.apply(l);
        }
        let a = fires(&RunawayResult, &s).expect("fires");
        assert!(
            a.headline
                .contains("`Bash ls -R` pushed 5.0k tokens into context, 2×"),
            "{}",
            a.headline
        );
        assert_eq!(a.saving, Saving::Tokens(10_000));
        let mut ok = State::new(Pricing::bundled());
        for l in tool("t1", "2026-01-01T00:00:00Z", "Bash", "ls", 800) {
            ok.apply(&l);
        }
        assert!(fires(&RunawayResult, &ok).is_none());
    }

    #[test]
    fn a04_rereads() {
        let mut s = State::new(Pricing::bundled());
        for i in 0..3 {
            for l in tool(
                &format!("r{i}"),
                "2026-01-01T00:00:00Z",
                "Read",
                "/p/src/render.rs",
                8_000,
            ) {
                s.apply(&l);
            }
        }
        let a = fires(&Rereads, &s).expect("fires");
        assert_eq!(a.headline, "render.rs was read 3× with no edit in between");
        assert_eq!(a.saving, Saving::Tokens(4_000));
        for l in tool("e1", "2026-01-01T00:00:00Z", "Edit", "/p/src/render.rs", 10) {
            s.apply(&l);
        }
        assert!(fires(&Rereads, &s).is_none(), "an edit resets the run");
    }

    #[test]
    fn a05_explore_in_main() {
        let mut s = State::new(Pricing::bundled());
        for i in 0..8 {
            for l in tool(
                &format!("g{i}"),
                "2026-01-01T00:00:00Z",
                "Read",
                &format!("/p/f{i}.rs"),
                4_000,
            ) {
                s.apply(&l);
            }
        }
        let a = fires(&ExploreInMain, &s).expect("fires");
        assert!(a.headline.starts_with("8 consecutive"), "{}", a.headline);
        assert_eq!(a.saving, Saving::Tokens(8_000 - 1_000));
        let mut ok = State::new(Pricing::bundled());
        for i in 0..7 {
            for l in tool(
                &format!("g{i}"),
                "2026-01-01T00:00:00Z",
                "Read",
                &format!("/p/f{i}.rs"),
                4_000,
            ) {
                ok.apply(&l);
            }
        }
        assert!(fires(&ExploreInMain, &ok).is_none());
    }

    #[test]
    fn a06_compaction_churn() {
        let mut s = State::new(Pricing::bundled());
        for (i, ctx) in [100_000u64, 200_000, 60_000, 200_000, 60_000]
            .iter()
            .enumerate()
        {
            s.apply(&prompt("2026-01-01T00:00:00Z"));
            s.apply(&response(
                &format!("c{i}"),
                "2026-01-01T00:00:01Z",
                *ctx,
                0,
                0,
            ));
        }
        let a = fires(&CompactionChurn, &s).expect("fires: two compactions");
        assert_eq!(a.saving, Saving::Avoids);
        assert!(a.evidence.starts_with("2 compaction"), "{}", a.evidence);
        // Projected within 3 turns also fires.
        let mut soon = State::new(Pricing::bundled());
        for (i, ctx) in [700_000u64, 740_000, 780_000].iter().enumerate() {
            soon.apply(&prompt("2026-01-01T00:00:00Z"));
            soon.apply(&response(
                &format!("s{i}"),
                "2026-01-01T00:00:01Z",
                *ctx,
                0,
                0,
            ));
        }
        assert!(fires(&CompactionChurn, &soon).is_some());
        assert!(
            fires(&CompactionChurn, &fixture_state()).is_none(),
            "fixture: ~9 turns away"
        );
        assert!(explain("A06").contains("hand-off"));
    }
}
