//! Files: blast radius and wasted re-reads.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::files::FileStats;
use crate::ui::fmt;
use crate::ui::layout::Placement;
use crate::ui::panel::{Handled, Panel, PanelId};
use crate::ui::state::{FileSort, State};

pub struct FilesPanel;

impl FilesPanel {
    fn rows(state: &State) -> Vec<&FileStats> {
        let mut v: Vec<&FileStats> = state.files.files.values().collect();
        match state.files_sort {
            FileSort::LastTouch => v.sort_by_key(|f| std::cmp::Reverse(f.last_touch_ms)),
            FileSort::Touches => v.sort_by_key(|f| std::cmp::Reverse(f.touches())),
            FileSort::Lines => v.sort_by_key(|f| {
                std::cmp::Reverse(f.lines_added.unwrap_or(0) + f.lines_removed.unwrap_or(0))
            }),
            FileSort::Name => v.sort_by(|a, b| a.path.cmp(&b.path)),
        }
        v
    }

    fn short(path: &str, cwd: &std::path::Path, width: usize) -> String {
        let rel = std::path::Path::new(path)
            .strip_prefix(cwd)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| fmt::shorten_home(std::path::Path::new(path)));
        if rel.chars().count() <= width {
            rel
        } else {
            let tail: String = rel
                .chars()
                .rev()
                .take(width - 1)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("…{tail}")
        }
    }
}

impl Panel for FilesPanel {
    fn id(&self) -> PanelId {
        7
    }
    fn title(&self) -> String {
        "Files".into()
    }
    fn summary(&self, state: &State) -> Option<String> {
        let n = state.files.files.len();
        let (a, d) = state.files.total_lines();
        let git = state.files.files.values().any(|f| f.lines_added.is_some());
        let sort = match state.files_sort {
            FileSort::LastTouch => "",
            FileSort::Touches => " · ↕touches",
            FileSort::Lines => " · ↕lines",
            FileSort::Name => " · ↕name",
        };
        Some(if git {
            format!("{n} touched · +{a} −{d}{sort}")
        } else {
            format!("{n} touched{sort}")
        })
    }
    fn min_rows(&self) -> u16 {
        4
    }
    fn priority(&self) -> u8 {
        40
    }
    fn placement(&self) -> Placement {
        Placement::Right
    }

    fn handle_key(&mut self, key: KeyEvent, state: &mut State) -> Handled {
        if key.code == KeyCode::Char('s') {
            state.files_sort = state.files_sort.next();
            return Handled::Yes;
        }
        Handled::No
    }

    fn render(&self, frame: &mut Frame, inner: Rect, state: &State) {
        let dim = state.theme.dim();
        let rows = Self::rows(state);
        if rows.is_empty() {
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled(" no files touched yet", dim))),
                inner,
            );
            return;
        }
        let name_w = (inner.width as usize).saturating_sub(36).clamp(8, 40);
        // The commit line: what is not committed, and how long ago the last
        // commit was.
        let mut status: Vec<Span> = Vec::new();
        if let Some((a, d, n)) = state.uncommitted {
            if n > 0 {
                status.push(Span::raw(format!(
                    " uncommitted +{a} −{d} across {n} file{}",
                    if n == 1 { "" } else { "s" }
                )));
            } else {
                status.push(Span::styled(" clean", state.theme.ok()));
            }
        }
        if let Some((at, sha, edits)) = state.last_commit() {
            status.push(Span::styled(
                format!(
                    " · last commit {} {} ago / {edits} edit{} ago",
                    fmt::clip(&sha, 7),
                    fmt::duration_ms(state.clock_ms() - at),
                    if edits == 1 { "" } else { "s" }
                ),
                dim,
            ));
        }
        let (checkpoints, bash_writes) = state.rewind_points();
        if checkpoints > 0 || bash_writes > 0 {
            status.push(Span::styled(
                format!(
                    " · rewind {checkpoints} checkpoint{}{}",
                    if checkpoints == 1 { "" } else { "s" },
                    if bash_writes > 0 {
                        format!(
                            " ({bash_writes} bash write{} not covered)",
                            if bash_writes == 1 { "" } else { "s" }
                        )
                    } else {
                        String::new()
                    }
                ),
                dim,
            ));
        }
        let status_line = (!status.is_empty()).then(|| Line::from(status));
        let take = inner.height as usize - status_line.is_some() as usize;
        let mut lines: Vec<Line> = rows
            .iter()
            .take(take)
            .map(|f| {
                let mut spans = vec![Span::raw(format!(
                    " {:<w$} ",
                    Self::short(&f.path, &state.session.cwd, name_w),
                    w = name_w
                ))];
                let mut counts = String::new();
                if f.reads > 0 {
                    counts.push_str(&format!("R×{} ", f.reads));
                }
                if f.edits > 0 {
                    counts.push_str(&format!("E×{} ", f.edits));
                }
                if f.writes > 0 {
                    counts.push_str(&format!("W×{} ", f.writes));
                }
                spans.push(Span::raw(format!("{counts:<12}")));
                match (f.lines_added, f.lines_removed) {
                    (Some(a), Some(d)) => {
                        spans.push(Span::styled(format!("+{a:<4}"), state.theme.ok()));
                        spans.push(Span::styled(format!("−{d:<4}"), state.theme.crit()));
                    }
                    _ => spans.push(Span::styled("—        ", dim)),
                }
                if f.reread_warning() {
                    spans.push(Span::styled(" re-read ⚠", state.theme.warn()));
                }
                if f.edits_this_turn > 0 {
                    spans.push(Span::styled(
                        format!(" ×{} this turn", f.edits_this_turn),
                        dim,
                    ));
                }
                if f.stale {
                    spans.push(Span::styled(" stale", state.theme.warn()));
                } else if f.ide_edits > 0 {
                    spans.push(Span::styled(" IDE edit", dim));
                }
                if let Some(v) = f.checkpoint_version.filter(|v| *v >= 8) {
                    spans.push(Span::styled(format!(" v{v}"), state.theme.warn()));
                }
                if f.edit_reread_edit > 0 {
                    spans.push(Span::styled(
                        format!(" churn ×{}", f.edit_reread_edit),
                        state.theme.warn(),
                    ));
                }
                Line::from(spans)
            })
            .collect();
        if let Some(s) = status_line {
            lines.push(s);
        }
        frame.render_widget(Paragraph::new(lines), inner);
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::{parse_file, Line};
    use crate::ui::state::{FileSort, SessionInfo, State};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

    fn fixture_app() -> App {
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
        app.state.session.ended_at_ms = app.state.last_line_at_ms;
        app
    }

    #[test]
    fn files_panel_on_fixture() {
        let app = fixture_app();
        let out = render_to_string(&app, 60, 70);
        assert!(out.contains("7 Files ─ 4 touched"), "{out}");
        assert!(out.contains("src/ad450c.rs"), "{out}");
        assert!(out.contains("W×1"), "{out}");
        assert!(out.contains("R×1"), "{out}");
        assert!(out.contains("—"), "no git: dash");
    }

    #[test]
    fn commit_line_and_markers_on_fixture_b() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
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
        app.state.uncommitted = Some((412, 87, 9));
        app.mode_override = Some(crate::ui::layout::Mode::Narrow);
        let out = render_to_string(&app, 140, 80);
        assert!(out.contains("uncommitted +412 −87 across 9 files"), "{out}");
        assert!(out.contains("· last commit 1d70205 "), "{out}");
        assert!(out.contains("edits ago"), "{out}");
        assert!(out.contains("IDE edit") || out.contains("stale"), "{out}");
    }

    #[test]
    fn reread_warning_git_lines_and_sort() {
        let mut app = fixture_app();
        for i in 0..3 {
            app.feed(Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-08-27T10:2{i}:00Z","message":{{"id":"rr{i}","model":"m","content":[{{"type":"tool_use","id":"rr{i}","name":"Read","input":{{"file_path":"/home/user/project/src/render.rs"}}}}],"usage":{{}}}}}}"#)).unwrap());
            // A read counts once its result shows the whole file came back.
            app.feed(Line::parse(&format!(r#"{{"type":"user","timestamp":"2026-08-27T10:2{i}:01Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"rr{i}","content":"…"}}]}},"toolUseResult":{{"type":"text","file":{{"filePath":"/home/user/project/src/render.rs","content":"…","numLines":40,"startLine":1,"totalLines":40}}}}}}"#)).unwrap());
        }
        app.state.files.apply_numstat(
            Path::new("/home/user/project"),
            &crate::files::parse_numstat("210\t31\tsrc/render.rs\n"),
        );
        let out = render_to_string(&app, 64, 70);
        assert!(out.contains("7 Files ─ 5 touched · +210 −31"), "{out}");
        assert!(out.contains("src/render.rs"), "{out}");
        assert!(out.contains("R×3"), "{out}");
        assert!(out.contains("+210 −31"), "{out}");
        assert!(out.contains("re-read ⚠"), "{out}");
        app.state.focused = Some(7);
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(app.state.files_sort, FileSort::Touches);
        assert!(render_to_string(&app, 64, 70).contains("↕touches"));
    }
}
