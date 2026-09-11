//! Full-height prefix inspector, opened with `i` on the Context panel.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::prefix::Kind;
use crate::ui::fmt;
use crate::ui::State;

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let ctx = state.context();
    let rows = state.prefix.rows(ctx.prefix);
    let block = Block::default().borders(Borders::ALL).title(format!(
        " Prefix — {} on every request  (Esc back) ",
        fmt::tokens(ctx.prefix)
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let dim = Style::default().fg(Color::DarkGray);
    let mut lines = vec![Line::from(Span::styled(
        format!(
            " {:<10} {:<40} {:>8} {:>8}  {}",
            "KIND", "WHAT", "BYTES", "TOKENS", "NOTE"
        ),
        dim,
    ))];
    for (i, r) in rows
        .iter()
        .enumerate()
        .take(inner.height.saturating_sub(2) as usize)
    {
        let kind = match r.kind {
            Kind::ClaudeMd => "CLAUDE.md",
            Kind::Tools => "tools",
            Kind::Mcp => "mcp",
            Kind::Skills => "skills",
            Kind::Agents => "agents",
            Kind::Memory => "memory",
            Kind::Other => "other",
        };
        let note = match r.kind {
            Kind::Tools | Kind::Mcp if r.count > 0 => format!("{} tool schemas", r.count),
            Kind::Skills => format!("{} skills", r.count),
            Kind::Agents => format!("{} agent types", r.count),
            Kind::Other => "first call minus the rows above".into(),
            _ => String::new(),
        };
        let bytes = if r.bytes > 0 {
            fmt::bytes(r.bytes)
        } else {
            "—".into()
        };
        let mut line = Line::from(format!(
            " {:<10} {:<40} {:>8} {:>8}  {}",
            kind,
            fmt::clip(&r.name, 40),
            bytes,
            fmt::tokens(r.tokens_est),
            note
        ));
        if i < 2 && r.kind != Kind::Other {
            line = line.style(
                Style::default()
                    .add_modifier(Modifier::BOLD)
                    .fg(Color::Yellow),
            );
        }
        lines.push(line);
    }
    lines.push(Line::from(Span::styled(
        " tokens ≈ bytes / 4 · sizes from the transcript's listing attachments and the files on disk",
        dim,
    )));
    frame.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::{parse_file, Line};
    use crate::ui::state::{SessionInfo, State};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

    #[test]
    fn i_on_context_opens_inspector_with_reconciled_rows() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.state = State::new(Pricing::bundled());
        app.state.session = SessionInfo::from_fixture(&path);
        for l in parse_file(&path).unwrap() {
            app.feed(l);
        }
        // On top of the fixture's own (anonymised) listings, add known-size ones.
        app.feed(Line::from_value(serde_json::json!({"type":"attachment","attachment":{"type":"skill_listing","content":"s".repeat(13_897),"skillCount":47}})));
        app.feed(Line::from_value(serde_json::json!({"type":"attachment","attachment":{"type":"deferred_tools_delta","addedNames":["Read","mcp__github__get_me"],"addedLines":["r".repeat(4000),"g".repeat(2000)]}})));
        app.state.focused = Some(1);
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(app.state.overlay, Some(1));
        let out = render_to_string(&app, 100, 20);
        assert!(out.contains("Prefix — 60k on every request"), "{out}");
        assert!(out.contains("skills     skills listing"), "{out}");
        assert!(out.contains("47 skills"), "{out}");
        assert!(
            out.contains("mcp        mcp:github") && out.contains("1 tool schemas"),
            "{out}"
        );
        assert!(out.contains("tools      built-in tools"), "{out}");
        assert!(out.contains("other      system prompt + other"), "{out}");
        // Reconciled: other = first-call prefix − every measured row.
        let rows = app.state.prefix.rows(app.state.context().prefix);
        let known: u64 = rows
            .iter()
            .filter(|r| r.kind != crate::prefix::Kind::Other)
            .map(|r| r.tokens_est)
            .sum();
        let other = rows
            .iter()
            .find(|r| r.kind == crate::prefix::Kind::Other)
            .unwrap()
            .tokens_est;
        assert_eq!(known + other, 60_582);
        assert!(rows.windows(2).all(|w| w[0].tokens_est >= w[1].tokens_est));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.state.overlay, None);
        // Enter still opens the ledger.
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(render_to_string(&app, 100, 20).contains("Turn ledger"));
    }
}
