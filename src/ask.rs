//! `a` on a panel: turn its numbers into a question for the session.
//!
//! Delivery: **Enter copies the prompt to the clipboard** (the user pastes and
//! submits it in Claude Code — a true draft). **S sends it over the session's
//! messaging socket** as a peer message; Claude Code has no draft slot on that
//! channel, so this arrives as an inbound message the session handles
//! according to its own peer-approval settings. See `docs/socket.md`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::ui::fmt;
use crate::ui::State;

pub const MAX_CHARS: usize = 600;

/// The question for the focused panel, from its current values.
pub fn compose(state: &State, panel: u8) -> Option<String> {
    let ctx = state.context();
    let u = state.agg.total;
    let text = match panel {
        0 | 1 => format!(
            "cctop says my context is {} of {} ({:.0} %), prefix {}, growing {}/turn{}. What should I change to keep this session lean?",
            fmt::tokens(ctx.size),
            fmt::tokens(ctx.window),
            ctx.ratio() * 100.0,
            fmt::tokens(ctx.prefix),
            ctx.velocity.map(|v| fmt::tokens(v.max(0.0) as u64)).unwrap_or_else(|| "—".into()),
            ctx.turns_until_compaction().map(|n| format!(", autocompact in ~{} turns", n.ceil() as u64)).unwrap_or_default()
        ),
        2 => format!(
            "cctop: {} tokens so far (cache read {}, cache write {}, fresh {}, output {}), cache hit {}, cost {}. Why is the mix like this and what would lower it?",
            fmt::tokens(u.total()),
            fmt::tokens(u.cache_read),
            fmt::tokens(u.cache_write()),
            fmt::tokens(u.input),
            fmt::tokens(u.output),
            u.cache_hit_ratio().map(|r| format!("{:.0} %", r * 100.0)).unwrap_or_else(|| "—".into()),
            state.cost.current().map(|c| fmt::usd(c.usd)).unwrap_or_else(|| "—".into())
        ),
        3 => match &state.limits {
            Some(l) => format!(
                "cctop: 5 h limit at {:.0} %, 7 d at {:.0} %{}. How should I pace the rest of this work?",
                l.five_hour_pct,
                l.seven_day_pct,
                l.exhaustion_ms.map(|e| format!(", projected to run out in {}", fmt::duration_ms(e - state.clock_ms()))).unwrap_or_default()
            ),
            None => "cctop has no rate-limit data (status-line shim not installed). Is it worth running `cctop install`?".into(),
        },
        4 => {
            let t = state.agg.current_turn();
            format!(
                "cctop: turn {} has run {} with {} API calls{}. What is it waiting on and is that expected?",
                t.map(|t| t.number).unwrap_or(0),
                t.and_then(|t| t.elapsed_ms(state.clock_ms())).map(fmt::duration_ms).unwrap_or_else(|| "—".into()),
                t.map(|t| t.api_calls).unwrap_or(0),
                state.tools.running().map(|c| format!(", currently in {} `{}`", c.name, c.input_summary)).unwrap_or_default()
            )
        }
        5 => {
            let by = state.tools.by_name();
            let mut rows: Vec<_> = by.values().collect();
            rows.sort_by_key(|t| std::cmp::Reverse(t.calls));
            let top3 = rows
                .iter()
                .take(3)
                .map(|t| format!("{} ×{} ({} errors, p95 {})", t.name, t.calls, t.errors, t.p95_ms.map(fmt::short_ms).unwrap_or_else(|| "—".into())))
                .collect::<Vec<_>>()
                .join("; ");
            let top_ctx = state
                .tools
                .top_ctx(3)
                .iter()
                .map(|c| format!("{} `{}` {}", c.name, c.input_summary, fmt::tokens(c.result_tokens_est)))
                .collect::<Vec<_>>()
                .join("; ");
            format!("cctop tools: {top3}. Largest results in context: {top_ctx}. Which of these can we make cheaper?")
        }
        6 => format!(
            "cctop: {} subagents ({} running), {} MCP servers, {} background tasks. Are all of them still earning their keep?",
            state.agents.len(),
            state.agents.values().filter(|a| a.state(state.clock_ms()) == crate::agents::State::Running).count(),
            state.procs.mcp.len(),
            state.tasks.len()
        ),
        7 => {
            let rereads: Vec<_> = state.files.files.values().filter(|f| f.reread_warning()).map(|f| f.path.clone()).collect();
            format!(
                "cctop: {} files touched{}. Can you summarise what changed and avoid re-reading files you already have?",
                state.files.files.len(),
                if rereads.is_empty() { String::new() } else { format!(", re-read without edits: {}", rereads.join(", ")) }
            )
        }
        8 => {
            let recent: Vec<String> = state.events.tail(5).iter().map(|e| format!("{} {}", e.kind.label(), e.text)).collect();
            format!("cctop recent events: {}. Anything here I should act on?", recent.join(" | "))
        }
        9 => {
            let a = state.advice.get(state.advice_index.min(state.advice.len().saturating_sub(1)))?;
            match a.action_kind {
                // A prompt or a slash command is the exact text to send.
                crate::advisor::ActionKind::Prompt | crate::advisor::ActionKind::Slash
                    if !a.action_text.is_empty() =>
                {
                    a.action_text.clone()
                }
                _ => format!("cctop advises: {} — {} ({}). Can you apply this now in this session?", a.headline, a.action, a.evidence),
            }
        }
        _ => return None,
    };
    Some(fmt::clip(&text, MAX_CHARS))
}

/// Copy to the system clipboard (pbcopy / wl-copy / xclip). Ok(tool name).
pub fn copy_to_clipboard(text: &str) -> Result<&'static str, String> {
    let candidates: [(&str, &[&str]); 3] = [
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
    ];
    for (cmd, args) in candidates {
        let child = std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Ok(mut c) = child {
            if let Some(mut stdin) = c.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            if c.wait().map(|s| s.success()).unwrap_or(false) {
                return Ok(cmd);
            }
        }
    }
    Err("no clipboard tool (pbcopy, wl-copy, xclip)".into())
}

/// The `peerToken` for a session pid, from `~/.claude/sessions/<pid>.*.key`.
pub fn peer_token(sessions_dir: &Path, pid: u32) -> Option<String> {
    let prefix = format!("{pid}.");
    let entries = std::fs::read_dir(sessions_dir).ok()?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(&prefix) && name.ends_with(".key") {
            let v: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok()?;
            return v
                .get("peerToken")
                .and_then(|t| t.as_str())
                .map(str::to_string);
        }
    }
    None
}

/// Send `text` as a user message over the session's messaging socket, using
/// the newline-delimited JSON framing Claude Code documents for injection:
/// an `auth` line, then a `user` line. Returns the first reply line, if any.
pub fn send_over_socket(
    socket: &Path,
    token: Option<&str>,
    text: &str,
) -> Result<Option<String>, String> {
    let mut s = UnixStream::connect(socket).map_err(|e| format!("cannot connect: {e}"))?;
    let _ = s.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = s.set_write_timeout(Some(Duration::from_millis(800)));
    let mut payload = String::new();
    if let Some(t) = token {
        payload.push_str(&serde_json::json!({"type": "auth", "token": t}).to_string());
        payload.push('\n');
    }
    payload.push_str(
        &serde_json::json!({"type": "user", "message": {"role": "user", "content": text}})
            .to_string(),
    );
    payload.push('\n');
    s.write_all(payload.as_bytes())
        .map_err(|e| format!("write failed: {e}"))?;
    let mut reader = BufReader::new(s);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(n) if n > 0 => Ok(Some(line.trim().to_string())),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::state::tests_support::fixture_state;
    use std::os::unix::net::UnixListener;

    #[test]
    fn composes_a_bounded_prompt_for_every_panel() {
        let mut s = fixture_state();
        s.session.ended_at_ms = s.last_line_at_ms;
        for p in 0..=8 {
            let q = compose(&s, p).unwrap_or_else(|| panic!("panel {p}"));
            assert!(q.starts_with("cctop"), "{p}: {q}");
            assert!(q.chars().count() <= MAX_CHARS, "{p} too long");
            assert!(q.contains('?'), "{p}: no question: {q}");
        }
        assert!(compose(&s, 9).is_none(), "no advice → nothing to ask");
        assert!(compose(&s, 42).is_none());
        let tools = compose(&s, 5).unwrap();
        assert!(tools.contains("mcp:claude-in-chrome ×214"), "{tools}");
        assert!(tools.contains("Largest results in context"), "{tools}");
    }

    #[test]
    fn socket_framing_is_auth_then_user_line() {
        let dir = std::env::temp_dir().join(format!("cctop-ask-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sock = dir.join("s.sock");
        let listener = UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut a = String::new();
            let mut b = String::new();
            r.read_line(&mut a).unwrap();
            r.read_line(&mut b).unwrap();
            let mut w = stream;
            w.write_all(b"{\"type\":\"connected\"}\n").unwrap();
            (a, b)
        });
        let reply = send_over_socket(&sock, Some("tok123"), "hello there").unwrap();
        let (a, b) = server.join().unwrap();
        let a: serde_json::Value = serde_json::from_str(&a).unwrap();
        let b: serde_json::Value = serde_json::from_str(&b).unwrap();
        assert_eq!(a["type"], "auth");
        assert_eq!(a["token"], "tok123");
        assert_eq!(b["type"], "user");
        assert_eq!(b["message"]["content"], "hello there");
        assert_eq!(reply.as_deref(), Some("{\"type\":\"connected\"}"));
        assert!(send_over_socket(&dir.join("missing.sock"), None, "x").is_err());
        // Key lookup.
        std::fs::write(
            dir.join("4242.abc.key"),
            r#"{"peerToken":"pt","pidDomain":"darwin"}"#,
        )
        .unwrap();
        assert_eq!(peer_token(&dir, 4242).as_deref(), Some("pt"));
        assert_eq!(peer_token(&dir, 1), None);
    }
}

#[cfg(test)]
mod app_tests {
    use crate::app::{render_to_string, App};
    use crate::metrics::Pricing;
    use crate::transcript::parse_file;
    use crate::ui::state::{SessionInfo, State};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::Path;

    #[test]
    fn a_opens_a_draft_overlay_and_esc_cancels() {
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
        app.state.open = Some(5);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(app.state.ask.is_some());
        let out = render_to_string(&app, 100, 30);
        assert!(out.contains("ask about panel 5"), "{out}");
        assert!(out.contains("Enter copy"), "{out}");
        assert!(!out.contains("S send"), "no socket for a fixture: {out}");
        assert!(out.contains("(no messaging socket)"), "{out}");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.state.ask.is_none());
        // The Tokens panel keeps `a` for its own toggle while it is open.
        app.state.open = Some(2);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(app.state.ask.is_none());
        assert!(!app.state.tokens_include_agents);
        // On the dashboard `a` asks about the nudge (panel 9).
        app.state.open = None;
        app.tick();
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(app.state.ask.as_ref().map(|(p, _)| *p), Some(9));
    }
}
