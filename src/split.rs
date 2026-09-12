//! `cctop split`: open the dashboard in a right-hand pane of the current
//! terminal multiplexer, attached to the session resolved *before* the split
//! so attachment is deterministic. The host terminal spawns the pane, so
//! cctop is never a child of the Claude Code process.

use std::process::Command;

/// How stale `~/.cctop/pane/<session>.json`'s `heartbeatAt` may be before the
/// pane it describes is treated as gone (matches the pane's own poller,
/// `STALE_AFTER_MS` in `plugin/hooks/poller.ts`).
const MARKER_STALE_AFTER_MS: i64 = 30_000;

/// Whether the function-hooks pane (US-008/US-009) already has this session
/// open: `~/.cctop/pane/<session_id>.json` exists, says `open: true` and its
/// `heartbeatAt` is no more than `MARKER_STALE_AFTER_MS` old. A missing,
/// unparsable or stale marker means "not open", so `cctop split` proceeds.
pub fn pane_marker_open(home: &str, session_id: &str) -> bool {
    pane_marker_open_at(home, session_id, crate::app::now_ms())
}

fn pane_marker_open_at(home: &str, session_id: &str, now_ms: i64) -> bool {
    let path = format!("{home}/.cctop/pane/{session_id}.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(marker) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    if marker.get("open").and_then(|v| v.as_bool()) != Some(true) {
        return false;
    }
    let Some(heartbeat_at) = marker.get("heartbeatAt").and_then(|v| v.as_str()) else {
        return false;
    };
    let Some(at) = crate::metrics::cost::parse_ts_ms(heartbeat_at) else {
        return false;
    };
    now_ms - at < MARKER_STALE_AFTER_MS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Tmux,
    Zellij,
    WezTerm,
    Kitty,
    ITerm2,
    /// Not inside a multiplexer, but iTerm2 is the terminal app: open a new
    /// window instead of splitting the (unmanaged) current one.
    ITerm2Window,
    /// Not inside a multiplexer, but Terminal.app is the terminal app: open
    /// a new window instead of splitting.
    AppleTerminalWindow,
}

impl Host {
    pub fn name(self) -> &'static str {
        match self {
            Host::Tmux => "tmux",
            Host::Zellij => "zellij",
            Host::WezTerm => "WezTerm",
            Host::Kitty => "Kitty",
            Host::ITerm2 => "iTerm2",
            Host::ITerm2Window => "iTerm2",
            Host::AppleTerminalWindow => "Terminal",
        }
    }

    /// True for hosts that open a brand-new window rather than splitting
    /// the pane the current session is already running in.
    pub fn opens_new_window(self) -> bool {
        matches!(self, Host::ITerm2Window | Host::AppleTerminalWindow)
    }
}

/// Which multiplexer we are running inside, from the environment. When none
/// is detected, fall back to opening a new window in the current terminal
/// app (Terminal.app or iTerm2) so the dashboard is still one command away
/// instead of requiring the user to already be inside a multiplexer.
pub fn detect(env: &dyn Fn(&str) -> Option<String>) -> Option<Host> {
    let set = |k: &str| env(k).is_some_and(|v| !v.is_empty());
    let val = |k: &str| env(k).filter(|v| !v.is_empty());
    if set("TMUX") {
        Some(Host::Tmux)
    } else if set("ZELLIJ") {
        Some(Host::Zellij)
    } else if set("WEZTERM_PANE") {
        Some(Host::WezTerm)
    } else if set("KITTY_LISTEN_ON") {
        Some(Host::Kitty)
    } else if set("ITERM_SESSION_ID") {
        Some(Host::ITerm2)
    } else {
        match val("TERM_PROGRAM").as_deref() {
            Some("iTerm.app") => Some(Host::ITerm2Window),
            Some("Apple_Terminal") => Some(Host::AppleTerminalWindow),
            _ => None,
        }
    }
}

/// The command each host runs to open the pane. `size` is a percentage.
pub fn command(
    host: Host,
    cctop: &str,
    session: &str,
    size: u8,
    env: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let inner = format!("{cctop} run --session {session}");
    let s = |x: &str| x.to_string();
    match host {
        Host::Tmux => {
            let mut v = vec![s("tmux"), s("split-window"), s("-h"), s("-l"), format!("{size}%")];
            if let Some(pane) = env("TMUX_PANE").filter(|p| !p.is_empty()) {
                v.push(s("-t"));
                v.push(pane);
            }
            v.push(inner);
            v
        }
        Host::Zellij => vec![s("zellij"), s("action"), s("new-pane"), s("-d"), s("right"), s("--"), s(cctop), s("run"), s("--session"), s(session)],
        Host::WezTerm => vec![s("wezterm"), s("cli"), s("split-pane"), s("--right"), s("--percent"), size.to_string(), s("--"), s(cctop), s("run"), s("--session"), s(session)],
        Host::Kitty => vec![s("kitten"), s("@"), s("launch"), s("--location=vsplit"), s(cctop), s("run"), s("--session"), s(session)],
        Host::ITerm2 => vec![
            s("osascript"),
            s("-e"),
            format!(
                "tell application \"iTerm2\" to tell current session of current window to write text \"{}\" in (split vertically with default profile)",
                inner.replace('"', "\\\"")
            ),
        ],
        Host::ITerm2Window => vec![
            s("osascript"),
            s("-e"),
            format!(
                "tell application \"iTerm2\" to tell (create window with default profile) to tell current session to write text \"{}\"",
                inner.replace('"', "\\\"")
            ),
        ],
        Host::AppleTerminalWindow => {
            // Terminal.app has no split-pane API, only windows/tabs, so the
            // closest approximation of a right-hand panel is a second
            // window butted up against the current one. Only the *new*
            // window's bounds are touched — the frontmost window's bounds
            // are read, never written, so existing windows/tabs are left
            // exactly as they were.
            let script = format!(
                "tell application \"Terminal\"\n\
                 set b to bounds of front window\n\
                 set x1 to item 1 of b\n\
                 set y1 to item 2 of b\n\
                 set x2 to item 3 of b\n\
                 set y2 to item 4 of b\n\
                 set newW to ((x2 - x1) * {size}) / 100\n\
                 do script \"{inner}\"\n\
                 delay 0.3\n\
                 set bounds of front window to {{x2, y1, x2 + newW, y2}}\n\
                 end tell",
                size = size,
                inner = inner.replace('"', "\\\"")
            );
            vec![s("osascript"), s("-e"), script]
        }
    }
}

/// What to tell the user when no host is detected.
pub fn manual_hint(cctop: &str, session: &str) -> String {
    format!(
        "cctop split: no tmux, zellij, WezTerm, Kitty, iTerm2 or Terminal.app detected.\n\
         Open a second terminal and run:\n\n    {cctop} run --session {session}\n"
    )
}

/// Run the split for the detected host. Returns the process exit code.
pub fn run(session_id: &str, size: u8) -> i32 {
    if let Some(home) = std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
        if pane_marker_open(&home, session_id) {
            println!("cctop pane is already open in this session");
            return 0;
        }
    }
    let env = |k: &str| std::env::var(k).ok();
    let cctop = std::env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "cctop".into());
    let Some(host) = detect(&env) else {
        eprintln!("{}", manual_hint("cctop", session_id));
        return 3;
    };
    let argv = command(host, &cctop, session_id, size, &env);
    // Discard stdout: osascript echoes the AppleScript result (e.g. a tab
    // reference), which is noise here — only the exit status matters.
    let result = Command::new(&argv[0])
        .args(&argv[1..])
        .stdout(std::process::Stdio::null())
        .status();
    match result {
        Ok(st) if st.success() => {
            let place = if host.opens_new_window() {
                "window"
            } else {
                "pane"
            };
            println!(
                "cctop: opened in a new {} {place}, attached to {session_id}",
                host.name()
            );
            0
        }
        Ok(st) => {
            eprintln!("cctop: {} returned {st}", argv[0]);
            1
        }
        Err(e) => {
            eprintln!(
                "cctop: cannot run {}: {e}\n{}",
                argv[0],
                manual_hint("cctop", session_id)
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k: &str| m.get(k).cloned()
    }

    #[test]
    fn detects_hosts_in_priority_order() {
        assert_eq!(detect(&env(&[("TMUX", "/tmp/x")])), Some(Host::Tmux));
        assert_eq!(detect(&env(&[("ZELLIJ", "0")])), Some(Host::Zellij));
        assert_eq!(detect(&env(&[("WEZTERM_PANE", "1")])), Some(Host::WezTerm));
        assert_eq!(
            detect(&env(&[("KITTY_LISTEN_ON", "unix:/x")])),
            Some(Host::Kitty)
        );
        assert_eq!(
            detect(&env(&[("ITERM_SESSION_ID", "w0t0p0")])),
            Some(Host::ITerm2)
        );
        assert_eq!(detect(&env(&[("TMUX", "")])), None);
        assert_eq!(detect(&env(&[])), None);
    }

    #[test]
    fn falls_back_to_a_new_window_outside_any_multiplexer() {
        assert_eq!(
            detect(&env(&[("TERM_PROGRAM", "Apple_Terminal")])),
            Some(Host::AppleTerminalWindow)
        );
        assert_eq!(
            detect(&env(&[("TERM_PROGRAM", "iTerm.app")])),
            Some(Host::ITerm2Window)
        );
        // Already inside an iTerm2 session: split, don't open a new window.
        assert_eq!(
            detect(&env(&[
                ("TERM_PROGRAM", "iTerm.app"),
                ("ITERM_SESSION_ID", "w0t0p0")
            ])),
            Some(Host::ITerm2)
        );
        assert_eq!(detect(&env(&[("TERM_PROGRAM", "vscode")])), None);
    }

    #[test]
    fn commands_per_host_carry_the_session() {
        let e = env(&[("TMUX_PANE", "%3")]);
        let t = command(Host::Tmux, "cctop", "abc", 45, &e);
        assert_eq!(
            t,
            [
                "tmux",
                "split-window",
                "-h",
                "-l",
                "45%",
                "-t",
                "%3",
                "cctop run --session abc"
            ]
        );
        let z = command(Host::Zellij, "cctop", "abc", 45, &e);
        assert_eq!(z[..5], ["zellij", "action", "new-pane", "-d", "right"]);
        assert!(z.ends_with(&[
            "cctop".into(),
            "run".into(),
            "--session".into(),
            "abc".into()
        ]));
        let w = command(Host::WezTerm, "/usr/bin/cctop", "abc", 40, &e);
        assert_eq!(
            w[..6],
            ["wezterm", "cli", "split-pane", "--right", "--percent", "40"]
        );
        let k = command(Host::Kitty, "cctop", "abc", 45, &e);
        assert_eq!(k[..4], ["kitten", "@", "launch", "--location=vsplit"]);
        let i = command(Host::ITerm2, "cctop", "abc", 45, &e);
        assert_eq!(i[0], "osascript");
        assert!(i[2].contains("split vertically") && i[2].contains("cctop run --session abc"));

        let iw = command(Host::ITerm2Window, "cctop", "abc", 45, &e);
        assert_eq!(iw[0], "osascript");
        assert!(iw[2].contains("create window") && iw[2].contains("cctop run --session abc"));

        let at = command(Host::AppleTerminalWindow, "cctop", "abc", 45, &e);
        assert_eq!(at[0], "osascript");
        assert!(
            at[2].contains("Terminal")
                && at[2].contains("cctop run --session abc")
                && at[2].contains("bounds of front window")
        );

        assert!(manual_hint("cctop", "abc").contains("cctop run --session abc"));
    }

    #[test]
    fn pane_marker_open_reads_freshness() {
        let home = std::env::temp_dir().join(format!("cctop-split-marker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let dir = home.join(".cctop").join("pane");
        std::fs::create_dir_all(&dir).unwrap();
        let home = home.to_str().unwrap();
        let base = crate::metrics::cost::parse_ts_ms("2026-01-01T00:00:00.000Z").unwrap();

        // Missing marker: not open.
        assert!(!pane_marker_open_at(home, "sess-1", base));

        std::fs::write(
            dir.join("sess-1.json"),
            r#"{"version":"0.2.0","sessionId":"sess-1","openedAt":"2026-01-01T00:00:00.000Z","heartbeatAt":"2026-01-01T00:00:00.000Z","open":true}"#,
        )
        .unwrap();

        // Fresh heartbeat, open: true.
        assert!(pane_marker_open_at(home, "sess-1", base + 10_000));

        // A 30 s-old heartbeat is already stale (matches the poller's own threshold).
        assert!(!pane_marker_open_at(home, "sess-1", base + 30_000));

        std::fs::write(
            dir.join("sess-1.json"),
            r#"{"version":"0.2.0","sessionId":"sess-1","openedAt":null,"heartbeatAt":"2026-01-01T00:00:00.000Z","open":false}"#,
        )
        .unwrap();

        // `open: false`: not open regardless of freshness.
        assert!(!pane_marker_open_at(home, "sess-1", base));

        // A different session id at the same home: no marker of its own.
        assert!(!pane_marker_open_at(home, "sess-2", base));
    }
}
