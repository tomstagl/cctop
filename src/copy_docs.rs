//! The words and thresholds as two reviewable Markdown tables (mod-pane
//! PRD US-013): `docs/coach-copy.md` (`cctop coach --copy-md`) and
//! `docs/verdicts.md` (`cctop verdicts --md`). Both are generated from the
//! code that says the words, and tests fail when either file is stale, so
//! the person reviewing before a release reads what the person using
//! cctop will read.

use crate::advisor::rules::outcome::ERROR_WORDS;
use crate::advisor::rules::{self, PHRASES};
use crate::verdict::{self, Dir, Threshold, Verdict};

/// What a rule said when it fired on the fixture sessions: the first
/// firing of each rule and variant, its line 1 and evidence as the coach
/// draws them, and every action it offered (by kind and label, the payload
/// of the first firing).
#[derive(Debug, Clone, Default)]
pub struct Fired {
    pub headline: String,
    pub evidence: String,
    pub actions: Vec<(String, String, String)>,
}

/// Where the fixture sessions are: the repository's `fixtures/`, read at
/// run time (nothing is embedded in the binary).
pub fn fixtures_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Fire every rule on the fixture sessions, fed line by line and read at
/// 40 points and at two clocks (live, and ten minutes idle): the same
/// sweep as the rules' own whole-words test. Keyed by `(rule id, variant)`.
pub fn fired_examples() -> std::collections::BTreeMap<(String, String), Fired> {
    use crate::ui::state::{SessionInfo, State};
    let root = fixtures_dir();
    let rules = rules::all();
    let mut out: std::collections::BTreeMap<(String, String), Fired> = Default::default();
    for name in ["a", "b", "c", "d", "e", "r", "s", "t", "w", "w-live"] {
        let path = root.join(format!("session-{name}.jsonl"));
        let Ok(parsed) = crate::transcript::parse_file(&path) else {
            continue;
        };
        let step = (parsed.len() / 40).max(1);
        let mut s = State::new(crate::metrics::Pricing::bundled());
        s.session = SessionInfo::from_fixture(&path);
        for (i, line) in parsed.iter().enumerate() {
            s.apply(line);
            let lines = i + 1;
            if lines % step != 0 && lines != parsed.len() {
                continue;
            }
            for idle_ms in [30_000, 600_000] {
                s.clock_override = true;
                s.now_ms = s.last_line_at_ms.unwrap_or(0) + idle_ms;
                for r in &rules {
                    let Some(a) = r.evaluate(&s) else { continue };
                    let e = out
                        .entry((r.id().to_string(), a.variant.to_string()))
                        .or_insert_with(|| Fired {
                            headline: a.headline.clone(),
                            evidence: a.evidence.clone(),
                            actions: Vec::new(),
                        });
                    for x in &a.actions {
                        let (k, l) = (x.kind.id().to_string(), x.label.clone());
                        if !e.actions.iter().any(|(k2, l2, _)| *k2 == k && *l2 == l) {
                            e.actions.push((k, l, x.payload.clone()));
                        }
                    }
                }
            }
        }
    }
    let fixture_keys: std::collections::BTreeSet<(String, String)> = out.keys().cloned().collect();
    // The witnesses (what the rules' own tests build) fill what the fixtures
    // do not reach; a fixture firing wins when both exist.
    for (_, mut s) in rules::witness::all() {
        if s.now_ms != 0 {
            s.clock_override = true;
        }
        for r in &rules {
            let Some(a) = r.evaluate(&s) else { continue };
            let key = (r.id().to_string(), a.variant.to_string());
            if fixture_keys.contains(&key) {
                continue;
            }
            let e = out.entry(key).or_insert_with(|| Fired {
                headline: a.headline.clone(),
                evidence: a.evidence.clone(),
                actions: Vec::new(),
            });
            for x in &a.actions {
                let (k, l) = (x.kind.id().to_string(), x.label.clone());
                if !e.actions.iter().any(|(k2, l2, _)| *k2 == k && *l2 == l) {
                    e.actions.push((k, l, x.payload.clone()));
                }
            }
        }
    }
    out
}

fn actions_cell(f: &Fired) -> String {
    f.actions
        .iter()
        .map(|(k, l, p)| {
            if p.trim().is_empty() {
                format!("{k}: {}", cell(l))
            } else {
                format!("{k}: {} {}", cell(l), code_cell(p))
            }
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

/// A table cell: pipes escaped, line breaks folded.
fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn code_cell(s: &str) -> String {
    format!("`{}`", cell(s))
}

/// `docs/coach-copy.md`.
pub fn coach_copy_md() -> String {
    let mut out = String::new();
    out.push_str("# The coach's words\n\n");
    out.push_str("Generated by `cctop coach --copy-md > docs/coach-copy.md` from the code that says these words; a test fails when this file is stale. Review every cell: this is what a person reads. `{holes}` are filled from facts only (counts, sizes, prices, durations, fixed labels, a file's basename), never from what the person or the model wrote. This file covers each rule's words, the small phrases the rules choose in code, and an example of each rule's headline, evidence line and actions taken from rules that fired on the fixture sessions (the dynamic parts there are examples); the overview's verdict sentences are in `docs/verdicts.md`.\n\n");

    out.push_str("## Rules\n\n");
    out.push_str("One row per rule and variant. A rule with variants says a different whole sentence for each case; the variant column names the case. The fix prompt is a ready prompt that is put in the box, never sent. Body, fix prompt and foot are the rule's words, as written. **Example headline, example evidence and actions are what the rule said when it fired on the fixture sessions** (`fixtures/session-*.jsonl`, the first firing of each variant): numbers, file names and tool names in them are examples, the sentence shapes are the code's. Actions are listed by kind and label, with the payload of that example; `read` means nothing to type. A variant that none of the fixtures reaches says `not fired in tests`.\n\n");
    out.push_str("| Rule | Name | Class · family | Variant | Body | Fix prompt | Example headline | Example evidence | Actions | Foot |\n");
    out.push_str("|---|---|---|---|---|---|---|---|---|---|\n");
    let fired = fired_examples();
    for rule in rules::all() {
        for w in rule.words() {
            let f = fired.get(&(rule.id().to_string(), w.variant.to_string()));
            let (h, e, a) = match f {
                Some(f) => (cell(&f.headline), cell(&f.evidence), actions_cell(f)),
                None => {
                    let n = "not fired in tests".to_string();
                    (n.clone(), n.clone(), n)
                }
            };
            out.push_str(&format!(
                "| {} | {} | {} · {} | {} | {} | {} | {} | {} | {} | {} |\n",
                rule.id(),
                cell(rule.name()),
                rule.urgency().label(),
                rule.family(),
                if w.variant.is_empty() {
                    "default".to_string()
                } else {
                    w.variant.to_string()
                },
                cell(w.body),
                w.fix_prompt.map(cell).unwrap_or_else(|| "—".into()),
                h,
                e,
                a,
                cell(w.foot),
            ));
        }
    }
    out.push_str("\n## Phrases chosen in code\n\n");
    out.push_str("Small fixed phrases a rule picks by case and puts into a `{hole}` above. One row per phrase.\n\n");
    out.push_str("| Rule | Slot | Case | Phrase |\n|---|---|---|---|\n");
    for p in PHRASES {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            p.rule,
            p.slot,
            p.key,
            cell(p.text)
        ));
    }

    out.push_str("\n## Error classes (A38)\n\n");
    out.push_str("A failed call is named by its class, never by its output. The body lists the labels in rank order; the fix prompt joins the fixes.\n\n");
    out.push_str("| Class | Rank | Label | Fix |\n|---|---|---|---|\n");
    for w in ERROR_WORDS {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            w.id,
            w.rank,
            cell(w.label),
            cell(w.fix)
        ));
    }
    out
}

fn num(v: f64) -> String {
    format!("{v}")
}

fn bound(t: &Threshold, v: Option<f64>) -> String {
    match v {
        Some(v) => format!("{} {}", if t.dir == Dir::Above { "≥" } else { "≤" }, num(v)),
        None => "—".into(),
    }
}

struct Row {
    chart: &'static str,
    scenario: &'static str,
    verdict: Verdict,
}

fn row(chart: &'static str, scenario: &'static str, verdict: Verdict) -> Row {
    Row {
        chart,
        scenario,
        verdict,
    }
}

/// One verdict per level and per sentence shape, from made-up facts: the
/// numbers in the sentences are the facts', the sentences are the code's.
fn examples() -> Vec<Row> {
    use verdict::*;
    let mut v = Vec::new();

    let ctx = |size: u64, turns_left: Option<f64>| ContextFacts {
        turns: 30,
        size,
        window: 1_000_000,
        threshold: 967_000,
        turns_left,
        light: Level::Act,
        mcp_tokens: None,
        messages_share: None,
    };
    v.push(row(
        "context",
        "no turn yet",
        context_from(&ContextFacts::default()),
    ));
    v.push(row(
        "context",
        "plenty of room, forecast trusted, near enough to plan",
        context_from(&ctx(250_000, Some(25.0))),
    ));
    v.push(row(
        "context",
        "plenty of room, no forecast yet",
        context_from(&ctx(120_000, None)),
    ));
    v.push(row(
        "context",
        "plenty of room, MCP small, conversation dominates",
        context_from(&ContextFacts {
            mcp_tokens: Some(6_000),
            messages_share: Some(0.9),
            ..ctx(120_000, None)
        }),
    ));
    v.push(row(
        "context",
        "turns left within the watch band",
        context_from(&ctx(500_000, Some(12.0))),
    ));
    v.push(row(
        "context",
        "window large",
        context_from(&ctx(350_000, None)),
    ));
    v.push(row(
        "context",
        "MCP schemas large",
        context_from(&ContextFacts {
            mcp_tokens: Some(40_000),
            ..ctx(120_000, None)
        }),
    ));
    v.push(row(
        "context",
        "inside the last 20k before autocompact",
        context_from(&ctx(950_000, None)),
    ));
    v.push(row(
        "context",
        "few turns left",
        context_from(&ctx(800_000, Some(3.0))),
    ));
    v.push(row(
        "context",
        "past the autocompact threshold",
        context_from(&ctx(970_000, Some(0.0))),
    ));

    let sp = |per_min: Option<f64>| SpendFacts {
        total: Some(18.75),
        per_min,
        ..Default::default()
    };
    v.push(row(
        "spend",
        "no price for the model",
        spend_from(&SpendFacts::default()),
    ));
    v.push(row(
        "spend",
        "nothing spent",
        spend_from(&SpendFacts {
            total: Some(0.0),
            ..Default::default()
        }),
    ));
    v.push(row("spend", "steady rate", spend_from(&sp(Some(0.10)))));
    v.push(row(
        "spend",
        "no spend in the last 15 minutes",
        spend_from(&sp(None)),
    ));
    v.push(row(
        "spend",
        "rate in the watch band, last jump a helper agent",
        spend_from(&SpendFacts {
            jumps: vec![Jump {
                turn: 3,
                usd: 1.85,
                kind: JumpKind::Agent,
            }],
            ..sp(Some(0.5))
        }),
    ));
    v.push(row(
        "spend",
        "rate in the watch band, last jump a cache rebuild",
        spend_from(&SpendFacts {
            jumps: vec![
                Jump {
                    turn: 3,
                    usd: 1.85,
                    kind: JumpKind::Agent,
                },
                Jump {
                    turn: 11,
                    usd: 2.31,
                    kind: JumpKind::Rebuild,
                },
            ],
            ..sp(Some(0.5))
        }),
    ));
    v.push(row(
        "spend",
        "rate in the watch band, no jump",
        spend_from(&sp(Some(0.5))),
    ));
    v.push(row(
        "spend",
        "rate in the act band, no jump",
        spend_from(&sp(Some(1.5))),
    ));
    v.push(row(
        "spend",
        "three jumps",
        spend_from(&SpendFacts {
            jumps: vec![
                Jump {
                    turn: 3,
                    usd: 1.85,
                    kind: JumpKind::Agent,
                },
                Jump {
                    turn: 7,
                    usd: 2.0,
                    kind: JumpKind::Turn,
                },
                Jump {
                    turn: 11,
                    usd: 2.31,
                    kind: JumpKind::Rebuild,
                },
            ],
            ..sp(Some(0.5))
        }),
    ));
    v.push(row(
        "spend",
        "budget in the watch band",
        spend_from(&SpendFacts {
            budget: Some((8.0, 10.0)),
            ..sp(Some(0.1))
        }),
    ));
    v.push(row(
        "spend",
        "budget in the act band, halting soon",
        spend_from(&SpendFacts {
            budget: Some((9.5, 10.0)),
            ..sp(Some(0.1))
        }),
    ));
    v.push(row(
        "spend",
        "account spend limit in the watch band",
        spend_from(&SpendFacts {
            spend_limit: Some((0.85, "spend $425 / $500 · monthly".into())),
            ..sp(Some(0.1))
        }),
    ));

    let costs = |c: &[f64]| CostTurnFacts {
        costs: c.iter().copied().enumerate().collect(),
        context: 450_000,
        ..Default::default()
    };
    v.push(row(
        "cost_per_turn",
        "no priced turn",
        cost_per_turn_from(&CostTurnFacts::default()),
    ));
    v.push(row(
        "cost_per_turn",
        "one turn",
        cost_per_turn_from(&costs(&[0.6])),
    ));
    v.push(row(
        "cost_per_turn",
        "steady turns",
        cost_per_turn_from(&costs(&[0.5, 0.6, 0.55, 0.6, 0.65])),
    ));
    v.push(row(
        "cost_per_turn",
        "latest turns dear, the context re-sent",
        cost_per_turn_from(&costs(&[0.5, 0.6, 0.55, 0.6, 1.74, 2.21])),
    ));
    v.push(row(
        "cost_per_turn",
        "latest turns far dearer, the context re-sent",
        cost_per_turn_from(&costs(&[0.5, 0.6, 0.55, 0.6, 3.1, 3.4])),
    ));
    let mut f = costs(&[0.5, 0.6, 0.55, 0.6, 3.1, 3.4]);
    f.over_200k = true;
    f.agents.insert(4, 2.5);
    f.agents.insert(5, 2.7);
    v.push(row(
        "cost_per_turn",
        "latest turns far dearer, helper agents, requests past 200k",
        cost_per_turn_from(&f),
    ));
    let mut f = costs(&[0.5, 0.6, 0.55, 0.6, 1.74, 2.21]);
    f.rebuilds.insert(4, 1.2);
    f.rebuilds.insert(5, 1.6);
    v.push(row(
        "cost_per_turn",
        "latest turns dear, cache rebuilds",
        cost_per_turn_from(&f),
    ));

    let miss = |kind: &'static str, field: Option<&'static str>, mcp: bool, tokens: u64| Miss {
        turn: 6,
        kind,
        field,
        mcp,
        tokens,
    };
    let cache = |hit: f64, misses: Vec<Miss>, usd: Option<f64>| CacheFacts {
        turns: 12,
        hit: Some(hit),
        misses,
        avoidable_usd: usd,
        ttl_words: "5 minutes",
        shown_from: None,
    };
    v.push(row(
        "cache",
        "first turn",
        cache_from(&CacheFacts {
            turns: 1,
            hit: Some(0.0),
            ..Default::default()
        }),
    ));
    v.push(row(
        "cache",
        "good, no rebuild",
        cache_from(&cache(0.95, vec![], None)),
    ));
    v.push(row(
        "cache",
        "good, one avoidable rebuild",
        cache_from(&cache(
            0.92,
            vec![miss("expired", None, false, 180_000)],
            Some(2.25),
        )),
    ));
    v.push(row(
        "cache",
        "good, one rebuild with no habit",
        cache_from(&cache(
            0.92,
            vec![miss("system_changed", None, false, 90_000)],
            None,
        )),
    ));
    v.push(row(
        "cache",
        "watch, some rebuilds avoidable",
        cache_from(&cache(
            0.7,
            vec![
                miss("params_changed", Some("model"), false, 300_000),
                miss("system_changed", None, false, 90_000),
            ],
            Some(3.75),
        )),
    ));
    v.push(row(
        "cache",
        "act, all rebuilds avoidable",
        cache_from(&cache(
            0.4,
            vec![
                miss("tools_changed", None, true, 200_000),
                miss("prefix_rewritten", None, false, 100_000),
            ],
            Some(3.75),
        )),
    ));
    v.push(row(
        "cache",
        "watch, no located rebuild",
        cache_from(&cache(0.7, vec![], None)),
    ));

    let tools = ToolFacts {
        calls: 40,
        reads: 6,
        edits: 5,
        ..Default::default()
    };
    v.push(row(
        "tool_calls",
        "no call yet",
        tool_calls_from(&ToolFacts::default()),
    ));
    v.push(row("tool_calls", "good", tool_calls_from(&tools)));
    v.push(row(
        "tool_calls",
        "watch, calls failing",
        tool_calls_from(&ToolFacts {
            recent_calls: 10,
            recent_failed: 2,
            ..tools.clone()
        }),
    ));
    v.push(row(
        "tool_calls",
        "act, calls failing",
        tool_calls_from(&ToolFacts {
            recent_calls: 10,
            recent_failed: 4,
            ..tools.clone()
        }),
    ));
    v.push(row(
        "tool_calls",
        "watch, slow hooks",
        tool_calls_from(&ToolFacts {
            hook_ms_per_turn: Some(800),
            ..tools.clone()
        }),
    ));
    v.push(row(
        "tool_calls",
        "act, slow hooks",
        tool_calls_from(&ToolFacts {
            hook_ms_per_turn: Some(2_500),
            ..tools.clone()
        }),
    ));
    v.push(row(
        "tool_calls",
        "watch, one file read whole again and again",
        tool_calls_from(&ToolFacts {
            reread: Some(("main.rs".into(), 4, 38_000)),
            ..tools.clone()
        }),
    ));
    v.push(row(
        "tool_calls",
        "watch, many reads per edit",
        tool_calls_from(&ToolFacts {
            reads: 40,
            edits: 4,
            ..tools.clone()
        }),
    ));
    v
}

/// `docs/verdicts.md`.
pub fn verdicts_md() -> String {
    let mut out = String::new();
    out.push_str("# The overview's verdicts\n\n");
    out.push_str("Generated by `cctop verdicts --md > docs/verdicts.md` from `src/verdict.rs`; a test fails when this file is stale. Review every threshold and every sentence. Each chart's verdict is Good, Watch or Act, one or two sentences and one Do or Don't. The verdict is the worst of its measures.\n\n");

    out.push_str("## Thresholds\n\n");
    out.push_str("One row per threshold. `Watch` and `Act` are the values where the verdict reaches that level (`≥` when the measure gets worse as it grows, `≤` when as it shrinks). A row with only a Watch value and a gate in its measure is not a level of its own: it is the number a sentence or a chart compares against. The source says where the number comes from; `drafted` means it was chosen for this release and is for you to change.\n\n");
    out.push_str(
        "| Chart | Id | Measure | Unit | Watch | Act | Source |\n|---|---|---|---|---|---|---|\n",
    );
    for t in verdict::THRESHOLDS {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            t.chart,
            t.id,
            cell(t.measure),
            t.unit,
            bound(t, t.watch),
            bound(t, t.act),
            cell(t.source)
        ));
    }

    out.push_str("\n## Sentences\n\n");
    out.push_str("One row per level and sentence shape, produced by the code from made-up facts (so the numbers are examples; the sentences are the code's). A Good verdict may have no Do.\n\n");
    out.push_str("| Chart | Level | Case | Text | Do / Don't |\n|---|---|---|---|---|\n");
    for r in examples() {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            r.chart,
            r.verdict.level.label(),
            r.scenario,
            cell(&r.verdict.text),
            r.verdict
                .todo
                .as_deref()
                .map(cell)
                .unwrap_or_else(|| "—".into())
        ));
    }

    out.push_str("\n## Cache habits\n\n");
    out.push_str("The habit that avoids a located cache rebuild, by cause. The cache verdict's Do is the habit of the costliest avoidable rebuild; a cause with no habit is not counted as avoidable.\n\n");
    out.push_str("| Cause | Changed | Habit |\n|---|---|---|\n");
    for (kind, field, mcp, what) in [
        ("tools_changed", None, true, "an MCP tool came or went"),
        (
            "tools_changed",
            None,
            false,
            "a built-in or deferred tool Claude Code loaded itself",
        ),
        (
            "prefix_rewritten",
            None,
            false,
            "an earlier message changed",
        ),
        ("params_changed", Some("effort"), false, "effort"),
        ("params_changed", Some("model"), false, "model"),
        ("params_changed", Some("betas"), false, "request features"),
        ("expired", None, false, "the cache expired"),
        ("system_changed", None, false, "the system prompt"),
    ] {
        out.push_str(&format!(
            "| {kind} | {what} | {} |\n",
            verdict::habit(kind, field, mcp)
                .map(cell)
                .unwrap_or_else(|| "none (not avoidable)".into())
        ));
    }
    out.push_str("\nWith no avoidable rebuild, the cache verdict's Do is `Do: reply within <the entry's lifetime> so the cache stays warm.`\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::Level;

    fn root(p: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(p)
    }

    #[test]
    fn docs_coach_copy_md_is_up_to_date() {
        let on_disk = std::fs::read_to_string(root("docs/coach-copy.md")).unwrap_or_default();
        assert_eq!(
            on_disk,
            coach_copy_md(),
            "docs/coach-copy.md is stale — run `cctop coach --copy-md > docs/coach-copy.md`"
        );
    }

    #[test]
    fn docs_verdicts_md_is_up_to_date() {
        let on_disk = std::fs::read_to_string(root("docs/verdicts.md")).unwrap_or_default();
        assert_eq!(
            on_disk,
            verdicts_md(),
            "docs/verdicts.md is stale — run `cctop verdicts --md > docs/verdicts.md`"
        );
    }

    #[test]
    fn every_phrase_is_used_and_every_level_has_a_sentence() {
        for p in PHRASES {
            assert!(
                [
                    include_str!("advisor/rules/token.rs"),
                    include_str!("advisor/rules/events.rs"),
                    include_str!("advisor/rules/outcome.rs"),
                ]
                .iter()
                .any(|s| s.contains(&format!("\"{}\"", p.slot))
                    && s.contains(&format!("\"{}\"", p.key))),
                "{} {} {} is not used",
                p.rule,
                p.slot,
                p.key
            );
        }
        let rows = examples();
        for chart in ["context", "spend", "cost_per_turn", "cache", "tool_calls"] {
            for level in [Level::Good, Level::Watch, Level::Act] {
                assert!(
                    rows.iter()
                        .any(|r| r.chart == chart && r.verdict.level == level),
                    "{chart} {level:?}"
                );
            }
        }
        assert!(rows.iter().any(|r| r.verdict.level == Level::Act));
    }

    #[test]
    fn no_cell_breaks_the_table() {
        for md in [coach_copy_md(), verdicts_md()] {
            let mut cols = 0;
            for line in md.lines() {
                if line.starts_with('|') {
                    let n = line.replace("\\|", "").matches('|').count();
                    if line.starts_with("|---") || cols == 0 {
                        cols = n;
                    }
                    assert_eq!(n, cols, "{line}");
                } else {
                    cols = 0;
                }
            }
        }
    }
}
