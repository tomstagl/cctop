//! Full-height sources inspector, opened with `m` on the Context panel: what
//! is in the window right now and what put it there (PRD context-residency
//! §4.4). The sibling of `prefix_view` — that one is the fixed half of the
//! window, this one is the rest.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::metrics::context::{Mode, Reference, Residency, Source};
use crate::ui::fmt;
use crate::ui::panel::Handled;
use crate::ui::State;

/// Enter / → open the per-file list under the `files` row; ← closes it. Esc
/// closes the inspector (the app handles it when nothing here does).
pub fn handle_key(key: KeyEvent, state: &mut State) -> Handled {
    match key.code {
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('f') => {
            state.sources_files = !state.sources_files;
            Handled::Yes
        }
        KeyCode::Left if state.sources_files => {
            state.sources_files = false;
            Handled::Yes
        }
        _ => Handled::No,
    }
}

/// `since session start` / `since model switch (−28.7k)`.
fn since_text(r: &Residency) -> String {
    match &r.since {
        Reference::SessionStart => "session start".to_string(),
        Reference::Boundary { kind, delta, .. } => {
            let sign = if *delta < 0 { "−" } else { "+" };
            format!(
                "{} ({sign}{})",
                kind.label(),
                fmt::tokens(delta.unsigned_abs())
            )
        }
    }
}

fn share(tokens: u64, size: u64) -> String {
    if size == 0 {
        return "   —".into();
    }
    format!("{:>3}%", tokens * 100 / size)
}

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let v = state.context();
    let r = state.residency();
    let dim = state.theme.dim();
    let est = if v.window_exact { "" } else { " est" };
    let title = if state.sources_files {
        format!(
            " Sources › files — {} in the window, since {}  (← back · Esc close) ",
            fmt::tokens(r.source(Source::Files)),
            since_text(&r)
        )
    } else {
        format!(
            " Sources — {} of {}{}, since {}  (Esc back) ",
            fmt::tokens(r.size),
            fmt::tokens(v.window),
            est,
            since_text(&r)
        )
    };
    let block = Block::default().borders(Borders::ALL).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let note_w = (inner.width as usize).saturating_sub(38);
    let mut lines: Vec<Line> = Vec::new();

    if state.sources_files {
        render_files(&mut lines, &r, state, inner.width as usize);
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    lines.push(Line::from(Span::styled(
        format!(
            " {:<16} {:>8} {:>5}  {}",
            "SOURCE", "TOKENS", "SHARE", "NOTE"
        ),
        dim,
    )));
    // The prefix row: the other inspector's subject, one line here.
    let mut prefix_note = String::from("system + tools + skills + CLAUDE.md · i for the breakdown");
    if r.prefix_tightened {
        prefix_note.push_str(" · lowered at the boundary");
    }
    let prefix_mark = match r.mode {
        Mode::Estimated => "≈",
        Mode::Calibrated { .. } => "",
    };
    lines.push(
        Line::from(format!(
            " {:<16} {:>8} {:>5}  {}",
            "prefix",
            format!("{prefix_mark}{}", fmt::tokens(r.prefix)),
            share(r.prefix, r.size),
            fmt::clip(&prefix_note, note_w)
        ))
        .style(state.theme.accent().add_modifier(Modifier::BOLD)),
    );
    lines.push(Line::from(Span::styled(
        format!(
            " ── messages: {} over {} call{} ──",
            fmt::tokens(r.messages()),
            r.calls_since,
            if r.calls_since == 1 { "" } else { "s" }
        ),
        dim,
    )));
    let files_n = r.files.len();
    let summary = matches!(r.since, Reference::Boundary { .. });
    for (label, tokens, exact) in r.rows() {
        if tokens == 0 && label != "other" {
            continue;
        }
        let note: String = match label {
            "files" => format!(
                "{files_n} file{} · Enter for the list",
                if files_n == 1 { "" } else { "s" }
            ),
            "bash output" => "builds, tests, greps, multi-file cats".into(),
            "mcp results" => "MCP tool results".into(),
            "agent returns" => "what subagents reported back".into(),
            "web" => "WebFetch / WebSearch".into(),
            "other results" => "Grep, Glob, ToolSearch, edit confirmations".into(),
            "prompts" => "what you typed and pasted".into(),
            "harness" => "reminders, listings, injected files".into(),
            "thinking" if !r.thinking_known => "not written by this transcript's version".into(),
            "thinking" => "exact, from usage".into(),
            "tool inputs" => "what the model wrote into tool calls".into(),
            "prose" => "exact: output − thinking − inputs".into(),
            "other" if summary => "the compaction summary, reminders no attachment recorded".into(),
            "other" => "reminders no attachment recorded, cache accounting".into(),
            _ => String::new(),
        };
        let mark = if exact { "" } else { "≈" };
        let mut line = Line::from(format!(
            " {:<16} {:>8} {:>5}  {}",
            label,
            format!("{mark}{}", fmt::tokens(tokens)),
            share(tokens, r.size),
            fmt::clip(&note, note_w)
        ));
        if label == "files" {
            line = line.style(state.theme.accent());
        }
        lines.push(line);
    }
    if let Some((from, to, delta)) = &r.model_switch_kept {
        let sign = if *delta < 0 { "−" } else { "+" };
        lines.push(Line::from(Span::styled(
            format!(
                " model switched {} → {} ({sign}{}) — the rows before it are in the old model's tokens",
                fmt::model_short(from),
                fmt::model_short(to),
                fmt::tokens(delta.unsigned_abs())
            ),
            state.theme.warn(),
        )));
    }
    if r.reconciled > 0 {
        lines.push(Line::from(Span::styled(
            fmt::clip(
                &format!(
                    " ≈{} of estimated bytes did not fit the exact growth of their steps and were scaled down",
                    fmt::tokens(r.reconciled)
                ),
                inner.width as usize,
            ),
            dim,
        )));
    }
    let mode = match r.mode {
        Mode::Estimated => {
            " prefix ≈ the first call's cached part — run /context once to calibrate it".to_string()
        }
        Mode::Calibrated { turn } => format!(" prefix from the /context you ran in turn {turn}"),
    };
    let w = inner.width as usize;
    lines.push(Line::from(Span::styled(fmt::clip(&mode, w), dim)));
    lines.push(Line::from(Span::styled(
        fmt::clip(
            " results, inputs and prompts ≈ chars / 4, reconciled per step against Δcontext − previous output; thinking and prose exact",
            w,
        ),
        dim,
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_files(lines: &mut Vec<Line>, r: &Residency, state: &State, width: usize) {
    let dim = state.theme.dim();
    if r.files.is_empty() {
        lines.push(Line::from(Span::styled(
            " no file has been read or written since the window began",
            dim,
        )));
        return;
    }
    let path_w = width.saturating_sub(30).clamp(12, 60);
    lines.push(Line::from(Span::styled(
        format!(
            " {:<w$} {:>8} {:>8} {:>5}",
            "FILE",
            "READ",
            "WRITTEN",
            "READS",
            w = path_w
        ),
        dim,
    )));
    for f in &r.files {
        lines.push(Line::from(format!(
            " {:<w$} {:>8} {:>8} {:>5}",
            crate::ui::panels::files::short_path(&f.path, &state.session.cwd, path_w),
            if f.tokens > 0 {
                format!("≈{}", fmt::tokens(f.tokens))
            } else {
                "—".into()
            },
            if f.written > 0 {
                format!("≈{}", fmt::tokens(f.written))
            } else {
                "—".into()
            },
            f.reads,
            w = path_w
        )));
    }
    lines.push(Line::from(Span::styled(
        fmt::clip(
            " READ: what its reads occupy in the window (the files row); WRITTEN: what the model wrote into edits of it (in tool inputs)",
            width,
        ),
        dim,
    )));
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use crate::ui::state::{ContextView, SessionInfo, State};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

    fn app_on(fixture: &str) -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{fixture}.jsonl"));
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.state = State::new(Pricing::bundled());
        app.state.session = SessionInfo::from_fixture(&path);
        for l in parse_file(&path).unwrap() {
            app.feed(l);
        }
        app.state.open = Some(1);
        app
    }

    #[test]
    fn m_on_context_opens_the_sources_inspector_and_counts_the_open() {
        let mut app = app_on("session-a");
        assert_eq!(app.state.inspector_opens, 0);
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert_eq!(app.state.overlay, Some(1));
        assert_eq!(app.state.context_view, ContextView::Sources);
        assert_eq!(app.state.inspector_opens, 1);
        let out = render_to_string(&app, 110, 30);
        assert!(out.contains("Sources — "), "{out}");
        assert!(out.contains("since session start"), "{out}");
        assert!(out.contains(" prefix "), "{out}");
        assert!(out.contains("thinking"), "{out}");
        assert!(out.contains("exact: output − thinking − inputs"), "{out}");
        insta::assert_snapshot!("sources_a_110x30", out);
        // Enter opens the per-file list; ← closes it.
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.state.sources_files);
        let files = render_to_string(&app, 110, 30);
        assert!(files.contains("Sources › files"), "{files}");
        assert!(files.contains("READ"), "{files}");
        insta::assert_snapshot!("sources_a_files_110x30", files);
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert!(!app.state.sources_files);
        // i still opens the prefix inspector, and counts too.
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, None);
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.state.context_view, ContextView::Prefix);
        assert_eq!(app.state.inspector_opens, 2);
    }

    #[test]
    fn fixture_b_names_its_boundary_and_the_summary() {
        let mut app = app_on("session-b");
        app.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        let out = render_to_string(&app, 162, 30);
        // B's window opens at a model switch one call after its compaction
        // (PRD §3.2 I); the remainder is the compaction summary.
        assert!(out.contains("since model switch (−"), "{out}");
        assert!(out.contains("the compaction summary"), "{out}");
        insta::assert_snapshot!("sources_b_162x30", out);
    }
}
