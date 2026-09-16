//! The agents view: a sortable ledger of the subagents with dollars and
//! waste, opened from Panel 6 with Enter (agent PRD §4.3). Rows come from
//! `crate::agent_ledger`, so this view, `cctop query agents` and the pane
//! draw the same figures; workflow runs fold into one group row each.

use std::collections::BTreeSet;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::agent_ledger::{self, AgentRow, Sort, WasteReason, WorkflowGroup};
use crate::agents::State as AgentState;
use crate::metrics::cost::Source;
use crate::ui::fmt;
use crate::ui::panel::Handled;
use crate::ui::State;

/// The panel id that owns the view (Agents & MCP).
pub const OWNER: u8 = 6;

/// Content width the columns are cut to, so the pane can mirror the rows.
pub const WIDTH: usize = 60;

#[derive(Debug, Clone, Default)]
pub struct AgentsUi {
    pub sort: Sort,
    pub ascending: bool,
    pub selected: usize,
    /// Workflow runs whose agents are listed under their group row.
    pub expanded: BTreeSet<String>,
}

pub fn open(state: &mut State) {
    state.overlay = Some(OWNER);
    state.agents_view_opens += 1;
}

/// One line of the list: an agent, or a workflow run's group row.
// Built per frame from the rows and dropped with them; the size skew
// between the variants is not worth a box.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Entry {
    Agent {
        row: AgentRow,
        /// Listed under an expanded group row.
        member: bool,
    },
    Group(WorkflowGroup),
}

/// The visible list: agents outside any workflow in sort order, then one
/// group row per run (its agents beneath it when expanded).
pub fn entries(state: &State, ui: &AgentsUi) -> Vec<Entry> {
    let rows = agent_ledger::rows(state, ui.sort, ui.ascending);
    let groups = agent_ledger::workflow_groups(state, &rows);
    let mut out: Vec<Entry> = rows
        .iter()
        .filter(|r| r.workflow.is_none())
        .map(|r| Entry::Agent {
            row: r.clone(),
            member: false,
        })
        .collect();
    for g in groups {
        let run = g.run.clone();
        out.push(Entry::Group(g));
        if ui.expanded.contains(&run) {
            out.extend(
                rows.iter()
                    .filter(|r| r.workflow.as_deref() == Some(run.as_str()))
                    .map(|r| Entry::Agent {
                        row: r.clone(),
                        member: true,
                    }),
            );
        }
    }
    out
}

/// Keys while the view is open. `Handled::No` on Esc closes it (App).
pub fn handle_key(key: KeyEvent, state: &mut State) -> Handled {
    let list = entries(state, &state.agents_ui);
    let n = list.len();
    let ui = &mut state.agents_ui;
    match key.code {
        KeyCode::Enter => match list.get(ui.selected) {
            Some(Entry::Group(g)) => {
                if !ui.expanded.remove(&g.run) {
                    ui.expanded.insert(g.run.clone());
                }
            }
            _ => return Handled::No,
        },
        KeyCode::Char('s') => {
            ui.sort = ui.sort.next();
            ui.ascending = false;
            ui.selected = 0;
        }
        KeyCode::Char('S') => ui.ascending = !ui.ascending,
        KeyCode::Char('j') | KeyCode::Down => {
            ui.selected = (ui.selected + 1).min(n.saturating_sub(1))
        }
        KeyCode::Char('k') | KeyCode::Up => ui.selected = ui.selected.saturating_sub(1),
        KeyCode::Char('g') => ui.selected = 0,
        KeyCode::Char('G') => ui.selected = n.saturating_sub(1),
        _ => return Handled::No,
    }
    Handled::Yes
}

/// Dollars without the sign, five cells: `0.19`, `12.4`, ` 118`.
fn cents(usd: f64) -> String {
    if usd >= 100.0 {
        format!("{usd:>5.0}")
    } else if usd >= 10.0 {
        format!("{usd:>5.1}")
    } else {
        format!("{usd:>5.2}")
    }
}

/// `1m 02`, `44s`, `12m 40`: elapsed in six cells.
fn elapsed(ms: Option<i64>) -> String {
    match ms {
        None => "—".into(),
        Some(ms) => {
            let s = ms.max(0) / 1000;
            if s < 60 {
                format!("{s}s")
            } else if s < 3600 {
                format!("{}m {:02}", s / 60, s % 60)
            } else {
                format!("{}h {:02}", s / 3600, (s % 3600) / 60)
            }
        }
    }
}

/// The model family: `opus`, `sonnet`, `haiku`, `fable`.
fn family(model: &str) -> String {
    model
        .trim_start_matches("claude-")
        .split('-')
        .next()
        .unwrap_or("")
        .to_string()
}

/// The text of one agent's row, cut to `WIDTH`.
pub fn agent_line(r: &AgentRow, now_ms: i64) -> String {
    let _ = now_ms;
    let glyph = match r.state {
        AgentState::Running => "◐",
        AgentState::Done => "✓",
        AgentState::Failed => "✗",
    };
    let cost = if r.cost.source == Source::Unpriced {
        "    —".to_string()
    } else {
        cents(r.cost.usd)
    };
    let ret = match r.returned_tokens {
        None if r.state == AgentState::Running => "  ...".to_string(),
        None => "    —".to_string(),
        Some(t) => format!("{:>5}", fmt::tokens(t)),
    };
    let waste = match r.waste {
        Some(w) => {
            let reason = match (w.reason, w.idle_ms) {
                (WasteReason::Idle, Some(ms)) => {
                    format!("idle {}", crate::coach::short_duration(ms))
                }
                (reason, _) => reason.label().to_string(),
            };
            let amount = if r.cost.source == Source::Unpriced {
                "    —".to_string()
            } else {
                cents(w.usd)
            };
            format!("{amount} {reason}")
        }
        None => " 0.00".to_string(),
    };
    let inherited = r
        .inherited_context
        .map(|n| format!("  ↰{}", fmt::tokens(n)))
        .unwrap_or_default();
    fmt::clip(
        &format!(
            " {glyph} {:<8} {:<6} {:>6} {:>5} {cost} {ret}  {waste}{inherited}",
            fmt::clip(&r.agent_type, 8),
            fmt::clip(&family(&r.model), 6),
            elapsed(r.elapsed_ms),
            fmt::tokens(r.tokens),
        ),
        WIDTH,
    )
}

/// The text of a workflow run's group row, cut to `WIDTH`.
pub fn group_line(g: &WorkflowGroup, expanded: bool) -> String {
    let mut parts = vec![
        format!("{} launched", g.launched),
        format!("{} done", g.done),
        format!("{} failed", g.failed),
    ];
    if let Some(e) = g.empty_result {
        parts.push(format!("{e} empty"));
    }
    let cost = if g.cost.source == Source::Unpriced {
        "—".to_string()
    } else {
        format!("≈${}", cents(g.cost.usd).trim())
    };
    fmt::clip(
        &format!(
            " {} {:<14} {}  {cost}",
            if expanded { "▾" } else { "wf" },
            fmt::clip(&g.run, 14),
            parts.join(" · ")
        ),
        WIDTH,
    )
}

/// The header line: count, running, spend and waste.
pub fn header_line(t: &agent_ledger::Totals) -> String {
    let spend = if t.cost.source == Source::Unpriced {
        "—".to_string()
    } else {
        format!("≈{}", fmt::usd(t.cost.usd))
    };
    let mut s = format!(" Agents ─ {} · {} running · {spend}", t.agents, t.running);
    if t.waste_usd > 0.0 {
        let share = if t.cost.usd > 0.0 {
            t.waste_usd / t.cost.usd * 100.0
        } else {
            0.0
        };
        s.push_str(&format!(
            " · wasted ≈{} ({share:.0} %)",
            fmt::usd(t.waste_usd)
        ));
    }
    fmt::clip(&s, WIDTH)
}

/// The two footer lines: waste by reason; cold starts and the return ratio.
pub fn footer_lines(t: &agent_ledger::Totals) -> [String; 2] {
    let by: Vec<String> = WasteReason::ALL
        .iter()
        .zip(t.waste_by_reason.iter())
        .map(|(r, usd)| format!("{} {}", r.label(), cents(*usd).trim()))
        .collect();
    let returned = match t.returned_ratio {
        Some(r) => format!("{:.1} % of output returned", r * 100.0),
        None => "no output yet".to_string(),
    };
    [
        fmt::clip(&format!(" waste: {}", by.join(" · ")), WIDTH),
        fmt::clip(
            &format!(
                " cold starts {} (≈{} writes) · {returned}",
                t.cold_starts,
                fmt::usd(t.cold_start_usd)
            ),
            WIDTH,
        ),
    ]
}

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let ui = &state.agents_ui;
    let now = state.clock_ms();
    let rows = agent_ledger::rows(state, ui.sort, ui.ascending);
    let totals = agent_ledger::totals(&rows);
    let list = entries(state, ui);
    let dim = state.theme.dim();
    let block = Block::default().borders(Borders::ALL).title(format!(
        "{}  ↕{}{}  (s/S sort, j/k, Enter expand, Esc back) ",
        header_line(&totals),
        ui.sort.label(),
        if ui.ascending { "↑" } else { "↓" }
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            "   {:<8} {:<6} {:>6} {:>5} {:>5} {:>5}  {}",
            "type", "model", "time", "tok", "≈$", "ret", "waste"
        ),
        dim,
    ))];
    let footer = footer_lines(&totals);
    let body = inner.height.saturating_sub(1 + footer.len() as u16 + 1) as usize;
    let first = ui.selected.saturating_sub(body.saturating_sub(1));
    // Narrower than the columns (the pane at 56): cut with a mark rather
    // than mid-word at the frame.
    let cut = |s: String| fmt::clip(&s, inner.width as usize);
    for (i, e) in list.iter().enumerate().skip(first).take(body.max(1)) {
        let mut line = match e {
            Entry::Agent { row, member } => {
                let text = agent_line(row, now);
                let text = cut(if *member {
                    format!("  {}", text.trim_start())
                } else {
                    text
                });
                let style = match row.state {
                    AgentState::Failed => state.theme.crit(),
                    AgentState::Running => state.theme.accent(),
                    AgentState::Done => Style::default(),
                };
                Line::from(Span::styled(text, style))
            }
            Entry::Group(g) => Line::from(Span::styled(
                cut(group_line(g, ui.expanded.contains(&g.run))),
                if g.failed > 0 {
                    state.theme.crit()
                } else {
                    dim
                },
            )),
        };
        if i == ui.selected {
            line = line.style(Style::default().add_modifier(Modifier::REVERSED));
        }
        lines.push(line);
    }
    if list.is_empty() {
        lines.push(Line::from(Span::styled(" no subagents yet", dim)));
    }
    lines.push(Line::from(Span::styled(
        "─".repeat(inner.width.min(WIDTH as u16) as usize),
        dim,
    )));
    for f in footer {
        lines.push(Line::from(Span::styled(cut(f), dim)));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{Agent, Meta};
    use crate::app::{render_to_string, App};
    use crate::metrics::cost::parse_ts_ms;
    use crate::metrics::Pricing;
    use crate::transcript::{parse_file, Line as TLine};
    use crate::ui::state::SessionInfo;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

    fn fixture_app(name: &str) -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{name}.jsonl"));
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.state = State::new(Pricing::bundled());
        app.state.session = SessionInfo::from_fixture(&path);
        for l in parse_file(&path).unwrap() {
            app.feed(l);
        }
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        app.state
            .merge_agents(&crate::agents::load(&path.with_extension("")));
        app
    }

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn enter_on_panel_6_opens_the_view_on_fixture_c() {
        let mut app = fixture_app("session-c");
        app.state.open = Some(6);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, Some(OWNER));
        insta::assert_snapshot!("agents_c_120x30", render_to_string(&app, 120, 30));
        insta::assert_snapshot!("agents_c_56x20", render_to_string(&app, 56, 20));
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("Agents ─ 2 · 0 running · ≈$"), "{out}");
        assert!(out.contains("killed"), "{out}");
        assert!(out.contains("↰62k"), "{out}");
        // Sorted by waste: the killed agent first.
        let killed = out.lines().find(|l| l.contains("✗ Explore")).unwrap();
        let fork = out.lines().find(|l| l.contains("✓ fork")).unwrap();
        assert!(
            killed.contains("sonnet") && killed.contains("  41 "),
            "{killed}"
        );
        assert!(
            fork.contains("  517  "),
            "ret of a 2 069-char result: {fork}"
        );
        assert!(fork.contains(" 0.00  ↰62k"), "{fork}");
        // Every row fits the pane's width: nothing past column `WIDTH`.
        for l in out
            .lines()
            .filter(|l| l.contains("Explore") || l.contains("fork"))
        {
            let past: String = l
                .chars()
                .skip(1 + WIDTH)
                .take_while(|c| *c != '│')
                .collect();
            assert!(past.trim().is_empty(), "{l}");
        }
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, None);
    }

    /// An agent with `n` opus calls at `start` (cold first call when
    /// `cold`), ending with text or with a tool call pending.
    fn agent(
        id: &str,
        ty: &str,
        model: &str,
        start: &str,
        n: usize,
        cold: bool,
        pending: bool,
    ) -> Agent {
        let mut a = Agent::new(
            id,
            Meta {
                agent_type: ty.into(),
                ..Default::default()
            },
        );
        let t0 = parse_ts_ms(start).unwrap();
        for i in 0..n {
            let (write, read) = if i == 0 && cold {
                (40_000, 0)
            } else {
                (200, 40_000)
            };
            let (block, stop) = if pending && i + 1 == n {
                (
                    r#"{"type":"tool_use","id":"t","name":"Bash","input":{}}"#,
                    "tool_use",
                )
            } else {
                (r#"{"type":"text","text":"done"}"#, "end_turn")
            };
            let at = t0 + i as i64 * 7_000;
            let s = at / 1000;
            let ts = format!(
                "2026-01-01T{:02}:{:02}:{:02}.000Z",
                (s / 3600) % 24,
                (s % 3600) / 60,
                s % 60
            );
            a.push(
                &TLine::parse(&format!(
                    r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}-m{i}","model":"{model}","content":[{block}],"stop_reason":"{stop}","usage":{{"cache_creation_input_tokens":{write},"cache_read_input_tokens":{read},"output_tokens":800}}}}}}"#
                ))
                .unwrap(),
            );
        }
        a
    }

    fn notify(state: &mut State, id: &str, status: &str, result: Option<&str>) {
        let result = result
            .map(|r| format!("<result>{r}</result>"))
            .unwrap_or_default();
        let text = format!(
            "<task-notification><task-id>{id}</task-id><status>{status}</status><summary>s</summary>{result}</task-notification>"
        );
        state.apply(
            &TLine::parse(&format!(
                r#"{{"type":"user","timestamp":"2026-01-01T01:00:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
                serde_json::to_string(&text).unwrap()
            ))
            .unwrap(),
        );
    }

    /// 25 agents: two failed, one killed, one idle, one empty result, three
    /// cold starts, one workflow run of eight (two of them failed in the
    /// journal), the rest done with a result.
    fn synthetic_app() -> App {
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.state = State::new(Pricing::bundled());
        let s = &mut app.state;
        let types = ["Explore", "general", "Plan", "claude"];
        let models = [
            "claude-haiku-4-5-20251001",
            "claude-opus-5",
            "claude-sonnet-5",
        ];
        for i in 0..25usize {
            let id = format!("{:017x}", 0xa000_0000_0000_0000u64 + i as u64);
            let start = format!("2026-01-01T00:{:02}:00Z", i);
            let cold = i % 8 == 1; // 1, 9, 17: three cold starts
            let mut a = agent(
                &id,
                types[i % 4],
                models[i % 3],
                &start,
                2 + i % 4,
                cold,
                i == 4, // the idle one keeps a call pending
            );
            if i >= 17 {
                a.workflow = Some("wf_abc-123".into());
            }
            s.agents.insert(id.clone(), a);
        }
        let id = |i: u64| format!("{:017x}", 0xa000_0000_0000_0000u64 + i);
        s.workflow_journals.push(crate::agents::WorkflowJournal {
            run: "wf_abc-123".into(),
            launched: 9,
            started: 8,
            results: 6,
            failed: 2,
            failed_ids: vec![id(17), id(18)],
        });
        // Notifications: two failed (2, 3), one killed (5), one empty (6),
        // the rest completed with a result; 4 stays silent (idle), the
        // workflow's agents report through the journal.
        for i in 0..17u64 {
            match i {
                2 | 3 => notify(s, &id(i), "failed", None),
                4 => {}
                5 => notify(s, &id(i), "killed", Some("partial")),
                6 => notify(s, &id(i), "completed", Some("")),
                _ => notify(s, &id(i), "completed", Some(&"r".repeat(1200))),
            }
        }
        s.clock_override = true;
        s.now_ms = parse_ts_ms("2026-01-01T01:00:00Z").unwrap();
        app
    }

    #[test]
    fn synthetic_25_agents_scroll_sort_and_group() {
        let mut app = synthetic_app();
        app.state.open = Some(6);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, Some(OWNER));
        insta::assert_snapshot!("agents_synthetic_120x30", render_to_string(&app, 120, 30));
        insta::assert_snapshot!("agents_synthetic_56x20", render_to_string(&app, 56, 20));
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("Agents ─ 25 · 1 running"), "{out}");
        assert!(
            out.contains("wf wf_abc-123"),
            "the run folds into one row: {out}"
        );
        assert!(out.contains("9 launched · 6 done · 2 failed"), "{out}");
        assert!(out.contains("idle 5"), "{out}");
        assert!(out.contains("no ret"), "{out}");
        assert!(out.contains("cold starts 3"), "{out}");
        assert!(out.contains("waste: failed"), "{out}");
        // The list overflows 20 rows: G reaches the group row at the end.
        app.handle_key(key('G'));
        let list = entries(&app.state, &app.state.agents_ui);
        assert_eq!(
            list.len(),
            18,
            "17 agents outside the run and one group row"
        );
        assert!(matches!(list.last(), Some(Entry::Group(_))));
        let out = render_to_string(&app, 56, 20);
        assert!(out.contains("wf wf_abc-123"), "{out}");
        // Enter expands the run: its eight agents follow the group row.
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let list = entries(&app.state, &app.state.agents_ui);
        assert_eq!(list.len(), 26);
        assert!(matches!(&list[18], Entry::Agent { member: true, .. }));
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("▾ wf_abc-123"), "{out}");
        // Sort: spend → waste → time → started, S flips.
        app.handle_key(key('s'));
        assert_eq!(app.state.agents_ui.sort, Sort::Waste);
        let out = render_to_string(&app, 120, 30);
        assert!(out.contains("↕waste↓"), "{out}");
        let first = out.lines().nth(2).unwrap();
        assert!(
            first.contains("idle 55m"),
            "the largest waste first: {first}"
        );
        app.handle_key(key('S'));
        assert!(app.state.agents_ui.ascending);
        app.handle_key(key('s'));
        app.handle_key(key('s'));
        assert_eq!(app.state.agents_ui.sort, Sort::Started);
        app.handle_key(key('j'));
        app.handle_key(key('k'));
        app.handle_key(key('g'));
        assert_eq!(app.state.agents_ui.selected, 0);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, None);
    }

    #[test]
    fn panel_6_itself_is_unchanged_but_the_fork_arrow() {
        let app = fixture_app("session-c");
        let mut app = app;
        app.state.open = Some(6);
        let out = render_to_string(&app, 80, 30);
        assert!(out.contains("✓ fork"), "{out}");
        assert!(out.contains("↰62k"), "{out}");
        assert!(
            !out.contains("wasted"),
            "the inline list has no dollars: {out}"
        );
    }
}
