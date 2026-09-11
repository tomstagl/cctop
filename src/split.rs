//! `cctop split`: open the dashboard in a right-hand pane of the current
//! terminal multiplexer, attached to the session resolved *before* the split
//! so attachment is deterministic. The host terminal spawns the pane, so
//! cctop is never a child of the Claude Code process.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Tmux,
    Zellij,
    WezTerm,
    Kitty,
    ITerm2,
}

impl Host {
    pub fn name(self) -> &'static str {
        match self {
            Host::Tmux => "tmux",
            Host::Zellij => "zellij",
            Host::WezTerm => "WezTerm",
            Host::Kitty => "Kitty",
            Host::ITerm2 => "iTerm2",
        }
    }
}

/// Which multiplexer we are running inside, from the environment.
pub fn detect(env: &dyn Fn(&str) -> Option<String>) -> Option<Host> {
    let set = |k: &str| env(k).is_some_and(|v| !v.is_empty());
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
        None
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
    }
}

/// What to tell the user when no host is detected.
pub fn manual_hint(cctop: &str, session: &str) -> String {
    format!(
        "cctop split: no tmux, zellij, WezTerm, Kitty or iTerm2 detected.\n\
         Open a second terminal and run:\n\n    {cctop} run --session {session}\n"
    )
}

/// Run the split for the detected host. Returns the process exit code.
pub fn run(session_id: &str, size: u8) -> i32 {
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
    match Command::new(&argv[0]).args(&argv[1..]).status() {
        Ok(st) if st.success() => {
            println!(
                "cctop: opened in a {} pane, attached to {session_id}",
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
        assert!(manual_hint("cctop", "abc").contains("cctop run --session abc"));
    }
}
