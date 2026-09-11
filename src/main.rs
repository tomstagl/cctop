//! cctop — a btop-style live dashboard for Claude Code internals.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Live dashboard for a running Claude Code session.
#[derive(Parser, Debug)]
#[command(name = "cctop", version, about, propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Attach to a session and show the dashboard (default).
    Run(Attach),
    /// Print session metrics as JSON.
    Query,
    /// Print the metrics registry as Markdown.
    Metrics(MetricsArgs),
    /// Install the status-line shim and hooks into ~/.claude/settings.json.
    Install,
    /// Remove what `install` added.
    Uninstall,
    /// Hook entry point: record one Claude Code hook event.
    Hook,
    /// Status-line entry point: tee the status JSON, then run the original command.
    StatuslineShim,
    /// Open cctop in a right-hand pane of the current multiplexer.
    Split,
    /// Print the current Advisor recommendations.
    Advise,
    /// Write an end-of-session report.
    Report,
    /// Export ledger and events.
    Export,
}

#[derive(Args, Debug, Default, Clone)]
struct MetricsArgs {
    /// Print the full Markdown reference (docs/metrics.md).
    #[arg(long)]
    md: bool,
    /// Rewrite the marked block in this README in place.
    #[arg(long, value_name = "FILE")]
    readme: Option<PathBuf>,
}

/// How to pick the Claude Code session to attach to.
#[derive(Args, Debug, Default, Clone)]
struct Attach {
    /// Session id (or ≥ 8-char prefix), name, or pid.
    #[arg(long)]
    session: Option<String>,
    /// Attach to the newest session running in this directory.
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// Keep polling until a session appears.
    #[arg(long)]
    wait: bool,
    /// Render one frame and exit (headless; for tests and scripts).
    #[arg(long)]
    once: bool,
    /// With --once: write the frame as plain text to this file (default stdout).
    #[arg(long, value_name = "FILE")]
    render_to: Option<PathBuf>,
    /// With --once: terminal size as WxH (default 60x51).
    #[arg(long, value_name = "WxH")]
    size: Option<String>,
    /// With --once: keys to press before rendering, comma separated (e.g. "Tab,Enter").
    #[arg(long)]
    keys: Option<String>,
    /// Also send critical alerts as desktop notifications.
    #[arg(long)]
    notify: bool,
}

fn main() {
    let cli = Cli::parse();
    let name = match cli.command.unwrap_or(Command::Run(Attach::default())) {
        Command::Run(attach) => {
            run(attach);
            return;
        }
        Command::Query => "query",
        Command::Metrics(args) => {
            use cctop::metrics::registry;
            if let Some(path) = args.readme {
                let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                    eprintln!("cctop: cannot read {}: {e}", path.display());
                    std::process::exit(1);
                });
                match registry::splice_readme(&text) {
                    Some(new) => {
                        if new != text {
                            std::fs::write(&path, new).expect("write readme");
                        }
                        println!("cctop: updated metrics block in {}", path.display());
                    }
                    None => {
                        eprintln!(
                            "cctop: {} has no <!-- metrics:start --> block",
                            path.display()
                        );
                        std::process::exit(1);
                    }
                }
                return;
            }
            // `--md` is the default output; the flag exists for explicitness.
            let _ = args.md;
            print!("{}", registry::markdown());
            return;
        }
        Command::Install => "install",
        Command::Uninstall => "uninstall",
        Command::Hook => "hook",
        Command::StatuslineShim => "statusline-shim",
        Command::Split => "split",
        Command::Advise => "advise",
        Command::Report => "report",
        Command::Export => "export",
    };
    println!("cctop {name}: not implemented");
}

/// A `--session` value naming an existing `.jsonl` (or a directory holding
/// `<name>.jsonl`) is a fixture: read it instead of the live registry.
fn fixture_path(session: &str) -> Option<PathBuf> {
    let p = PathBuf::from(session);
    if p.is_file() {
        return Some(p);
    }
    let with_ext = p.with_extension("jsonl");
    with_ext.is_file().then_some(with_ext)
}

fn run(attach: Attach) {
    use cctop::app::{self, App};
    use cctop::ui::state::{SessionInfo, State};
    let (transcript, session_info): (PathBuf, SessionInfo) =
        match attach.session.as_deref().and_then(fixture_path) {
            Some(p) => {
                let info = SessionInfo::from_fixture(&p);
                (p, info)
            }
            None => {
                let q =
                    cctop::discover::Query::from_env(attach.session.clone(), attach.cwd.clone());
                let s = match cctop::discover::resolve_system(&q, attach.wait) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("cctop: {e}");
                        std::process::exit(cctop::discover::DiscoverError::EXIT_CODE);
                    }
                };
                (cctop::transcript_path(&s), SessionInfo::from_registry(&s))
            }
        };
    let mut app = App::new(
        cctop::ui::panels::all(),
        Box::new(|line, state: &mut State| state.apply(line)),
    );
    app.state = State::new(cctop::metrics::Pricing::load());
    app.state.session = session_info;
    app.state.now_ms = app::now_ms();
    app.desktop_notify = attach.notify;
    // Clock-driven collectors: liveness and git, at most every 5 s.
    let mut last_git = std::time::Instant::now() - std::time::Duration::from_secs(10);
    let base_commit = cctop::files::head_commit(&app.state.session.cwd);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        state.session.refresh_alive(state.now_ms);
        if last_git.elapsed() >= std::time::Duration::from_secs(5) {
            last_git = std::time::Instant::now();
            if let Some(g) = cctop::git::info(&state.session.cwd) {
                state.session.git_branch = g.branch;
                state.session.git_dirty = g.dirty;
            }
            if let Some(base) = &base_commit {
                let ns = cctop::files::numstat(&state.session.cwd, base);
                let cwd = state.session.cwd.clone();
                state.files.apply_numstat(&cwd, &ns);
            }
        }
    }));

    let session_dir = transcript.with_extension("");
    if attach.once {
        for line in cctop::transcript::parse_file(&transcript).unwrap_or_default() {
            app.feed(line);
        }
        app.state.agents = cctop::agents::load(&session_dir);
        if app.state.session.ended_at_ms.is_none() && !app.state.session.alive {
            app.state.session.ended_at_ms = app.state.last_line_at_ms;
        }
        app.tick();
        if let Some(keys) = attach.keys.as_deref() {
            for k in app::parse_keys(keys) {
                app.handle_key(k);
            }
        }
        let (w, h) = attach
            .size
            .as_deref()
            .and_then(app::parse_size)
            .unwrap_or((60, 51));
        let text = app::render_to_string(&app, w, h);
        match attach.render_to {
            Some(path) => std::fs::write(&path, text).unwrap_or_else(|e| {
                eprintln!("cctop: cannot write {}: {e}", path.display());
                std::process::exit(1);
            }),
            None => print!("{text}"),
        }
        return;
    }

    // Process tree once a second: cpu/rss, MCP servers, the running command.
    if let Some(pid) = app.state.session.pid {
        let configs = cctop::procs::mcp_configs(&app.state.session.cwd);
        let mut sampler = cctop::procs::Sampler::default();
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(5);
        app.tick_hooks.push(Box::new(move |state: &mut State| {
            if last.elapsed() < std::time::Duration::from_secs(1) {
                return;
            }
            last = std::time::Instant::now();
            let snap = sampler.snapshot(&cctop::procs::Ps, pid, &configs);
            if let Some(m) = &snap.main {
                state.session.cpu_pct = Some(m.cpu_pct);
                state.session.rss_bytes = Some(m.rss_bytes);
            }
            state.mcp_exited = sampler.exited.clone();
            state.procs = snap;
        }));
    }
    // Background tasks: rescan the session's task directory every 2 s.
    if let Some(dir) = cctop::tasks::dir_for(&app.state.session.session_id) {
        let mut last = std::time::Instant::now() - std::time::Duration::from_secs(5);
        app.tick_hooks.push(Box::new(move |state: &mut State| {
            if last.elapsed() >= std::time::Duration::from_secs(2) {
                last = std::time::Instant::now();
                state.tasks = cctop::tasks::load(&dir);
            }
        }));
    }
    let mut agents = cctop::agents::AgentWatcher::watch(&session_dir);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if agents.poll() || state.agents.len() != agents.agents.len() {
            state.agents = agents.agents.clone();
        }
    }));
    let tailer = match cctop::tail::Tailer::open(&transcript) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cctop: cannot tail {}: {e}", transcript.display());
            std::process::exit(1);
        }
    };
    if let Err(e) = app::run_tui(app, vec![Box::new(tailer)]) {
        eprintln!("cctop: {e}");
        std::process::exit(1);
    }
}
