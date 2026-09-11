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
    Query(QueryArgs),
    /// Print the metrics registry as Markdown.
    Metrics(MetricsArgs),
    /// Install the status-line shim and hooks into ~/.claude/settings.json.
    Install(InstallArgs),
    /// Remove what `install` added.
    Uninstall(InstallArgs),
    /// Hook entry point: record one Claude Code hook event.
    Hook,
    /// Status-line entry point: tee the status JSON, then run the original command.
    StatuslineShim {
        /// The original status-line command and its arguments (after `--`).
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        original: Vec<String>,
    },
    /// Open cctop in a right-hand pane of the current multiplexer.
    Split,
    /// Print the current Advisor recommendations.
    Advise(AdviseArgs),
    /// Write an end-of-session report.
    Report(ReportArgs),
    /// Export ledger and events.
    Export(ExportArgs),
}

#[derive(Args, Debug, Default, Clone)]
struct InstallArgs {
    /// Apply without asking.
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Args, Debug, Clone)]
struct QueryArgs {
    #[command(subcommand)]
    what: QueryWhat,
    #[command(flatten)]
    attach: Attach,
}

#[derive(Subcommand, Debug, Clone)]
enum QueryWhat {
    /// Session, context, tokens, cost, limits at a glance.
    Summary,
    /// One row per turn.
    Ledger {
        /// Only the last N turns.
        #[arg(long)]
        last: Option<usize>,
    },
    /// Per-tool statistics and the largest results.
    Tools,
    /// Files touched.
    Files,
    /// Subagents, MCP servers, background tasks.
    Agents,
    /// Ranked Advisor recommendations with explanations.
    Advice,
    /// What rides on every request.
    Prefix,
    /// Event log.
    Events {
        /// Only events newer than this (e.g. 10m, 2h).
        #[arg(long)]
        since: Option<String>,
    },
    /// Definition of a metric id (see docs/metrics.md).
    Explain { metric_id: String },
    /// Medians over your last 7 days of sessions.
    Baseline,
}

#[derive(Args, Debug, Default, Clone)]
struct ReportArgs {
    #[command(flatten)]
    attach: Attach,
    /// Write here instead of ~/.cctop/reports/<date>-<name>.md (`-` for stdout).
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Args, Debug, Default, Clone)]
struct ExportArgs {
    #[command(flatten)]
    attach: Attach,
    /// JSON with ledger and events (default).
    #[arg(long)]
    json: bool,
    /// CSV of the ledger (or of the events with --events).
    #[arg(long)]
    csv: bool,
    /// With --csv: export the events instead of the ledger.
    #[arg(long)]
    events: bool,
    /// Write to this file instead of stdout.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Args, Debug, Default, Clone)]
struct AdviseArgs {
    #[command(flatten)]
    attach: Attach,
    /// Print JSON instead of a table.
    #[arg(long)]
    json: bool,
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
    /// Session id (or ≥ 8-char prefix), name, pid, or a fixture .jsonl path.
    #[arg(long, global = true)]
    session: Option<String>,
    /// Attach to the newest session running in this directory.
    #[arg(long, global = true)]
    cwd: Option<PathBuf>,
    /// Keep polling until a session appears.
    #[arg(long, global = true)]
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
        Command::Query(q) => {
            query(q);
            return;
        }
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
        Command::Install(a) => {
            let path = cctop::install::settings_path();
            if let Err(e) =
                cctop::install::apply(&path, a.yes, cctop::install::install_transform, "install")
            {
                eprintln!("cctop: {e}");
                std::process::exit(1);
            }
            return;
        }
        Command::Uninstall(a) => {
            let path = cctop::install::settings_path();
            if let Err(e) = cctop::install::apply(
                &path,
                a.yes,
                cctop::install::uninstall_transform,
                "uninstall",
            ) {
                eprintln!("cctop: {e}");
                std::process::exit(1);
            }
            return;
        }
        Command::Hook => std::process::exit(cctop::hooks::run_hook()),
        Command::StatuslineShim { original } => {
            let original: Vec<String> = original.into_iter().skip_while(|a| a == "--").collect();
            std::process::exit(cctop::status::run_shim(&original));
        }
        Command::Split => "split",
        Command::Advise(a) => {
            advise(a);
            return;
        }
        Command::Report(r) => {
            let state = load_state(&r.attach);
            let md = cctop::report::markdown(&state, state.baseline.as_ref());
            match r.out {
                Some(p) if p.as_os_str() == "-" => print!("{md}"),
                out => {
                    let path = out.unwrap_or_else(|| {
                        cctop::report::report_path(&cctop::status::cctop_dir(), &state)
                    });
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    if let Err(e) = std::fs::write(&path, md) {
                        eprintln!("cctop: cannot write {}: {e}", path.display());
                        std::process::exit(1);
                    }
                    println!("cctop: report written to {}", path.display());
                }
            }
            return;
        }
        Command::Export(x) => {
            let state = load_state(&x.attach);
            let text = if x.csv {
                cctop::report::export_csv(&state, x.events)
            } else {
                serde_json::to_string_pretty(&cctop::report::export_json(&state)).unwrap()
            };
            match x.out {
                Some(p) => {
                    if let Err(e) = std::fs::write(&p, text) {
                        eprintln!("cctop: cannot write {}: {e}", p.display());
                        std::process::exit(1);
                    }
                    println!("cctop: exported to {}", p.display());
                }
                None => print!("{text}"),
            }
            return;
        }
    };
    println!("cctop {name}: not implemented");
}

fn run(attach: Attach) {
    use cctop::app::{self, App};
    use cctop::ui::state::{SessionInfo, State};
    let (transcript, session_info): (PathBuf, SessionInfo) =
        match cctop::load::resolve(&target(&attach)) {
            Ok(x) => x,
            Err(e) => {
                eprintln!("cctop: {e}");
                std::process::exit(cctop::discover::DiscoverError::EXIT_CODE);
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
    if let Some(projects) = cctop::baseline::default_projects_dir() {
        app.state.baseline = Some(cctop::baseline::load_or_compute(
            &cctop::status::cctop_dir(),
            &projects,
            app.state.now_ms,
        ));
    }
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
    // CLAUDE.md files and the memory index for the prefix inspector.
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut last_scan = std::time::Instant::now() - std::time::Duration::from_secs(60);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if last_scan.elapsed() >= std::time::Duration::from_secs(30) {
            last_scan = std::time::Instant::now();
            let cwd = state.session.cwd.clone();
            state.prefix.scan(&cwd, home.as_deref());
        }
    }));
    // Other live sessions share the account's rate limit.
    let my_pid = app.state.session.pid;
    let mut last_reg = std::time::Instant::now() - std::time::Duration::from_secs(10);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if last_reg.elapsed() >= std::time::Duration::from_secs(5) {
            last_reg = std::time::Instant::now();
            if let Some(dir) = cctop::registry::default_dir() {
                state.other_live_sessions = cctop::registry::list(&dir)
                    .iter()
                    .filter(|s| Some(s.pid) != my_pid && s.is_alive())
                    .filter(|s| s.status() == cctop::registry::Status::Busy)
                    .count();
            }
        }
    }));
    // Hook spool: exact tool timings, permission waits, compactions.
    let home = cctop::status::cctop_dir();
    cctop::hooks::prune(&home, app.state.now_ms);
    let mut hooks = cctop::hooks::Watcher::new(&home, &app.state.session.session_id);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        for ev in hooks.poll() {
            state.apply_hook(&ev);
        }
    }));
    // Status-line samples (rate limits, exact context) when the shim is installed.
    let mut status = cctop::status::Watcher::new(&app.state.session.session_id);
    app.tick_hooks.push(Box::new(move |state: &mut State| {
        if status.poll() {
            state.apply_status(status.latest.as_ref().unwrap(), &status.series_5h);
        }
    }));
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

fn target(attach: &Attach) -> cctop::load::Target {
    cctop::load::Target {
        session: attach.session.clone(),
        cwd: attach.cwd.clone(),
        wait: attach.wait,
    }
}

fn load_state(attach: &Attach) -> cctop::ui::State {
    match cctop::load::state(&target(attach)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cctop: {e}");
            std::process::exit(cctop::discover::DiscoverError::EXIT_CODE);
        }
    }
}

fn query(q: QueryArgs) {
    use cctop::query as qy;
    let out = match &q.what {
        QueryWhat::Explain { metric_id } => qy::explain(metric_id),
        what => {
            let mut state = load_state(&q.attach);
            let mut engine = cctop::advisor::Engine::default();
            engine.evaluate(&state);
            state.advice = engine.current.clone();
            match what {
                QueryWhat::Summary => qy::summary(&state),
                QueryWhat::Ledger { last } => qy::ledger_json(&state, *last),
                QueryWhat::Tools => qy::tools(&state),
                QueryWhat::Files => qy::files(&state),
                QueryWhat::Agents => qy::agents(&state),
                QueryWhat::Advice => qy::advice(&state),
                QueryWhat::Prefix => qy::prefix(&state),
                QueryWhat::Baseline => qy::baseline(state.baseline.as_ref()),
                QueryWhat::Events { since } => {
                    let since_ms = match since.as_deref() {
                        Some(s) => match qy::parse_since(s) {
                            Some(ms) => Some(ms),
                            None => {
                                eprintln!("cctop: --since expects e.g. 10m, 2h, 90s");
                                std::process::exit(1);
                            }
                        },
                        None => None,
                    };
                    qy::events(&state, since_ms)
                }
                QueryWhat::Explain { .. } => unreachable!(),
            }
        }
    };
    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}

fn advise(a: AdviseArgs) {
    let state = load_state(&a.attach);
    let mut engine = cctop::advisor::Engine::default();
    engine.evaluate(&state);
    if a.json {
        let items: Vec<serde_json::Value> = engine
            .current
            .iter()
            .map(|x| {
                serde_json::json!({
                    "rule": x.rule,
                    "headline": x.headline,
                    "evidence": x.evidence,
                    "action": x.action,
                    "saving": x.saving.label(),
                    "doc_key": x.doc_key,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items).unwrap());
        return;
    }
    if engine.current.is_empty() {
        println!("cctop advise: no recommendation — the session looks efficient");
        return;
    }
    for (i, x) in engine.current.iter().enumerate() {
        println!("{}. [{}] {}", i + 1, x.rule, x.headline);
        println!("   evidence: {}", x.evidence);
        println!("   action:   {}", x.action);
        println!("   saving:   {}", x.saving.label());
    }
}
