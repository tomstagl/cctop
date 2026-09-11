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
    Run(RunArgs),
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
    Split(SplitArgs),
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
struct RunArgs {
    #[command(flatten)]
    attach: Attach,
    #[command(flatten)]
    headless: Headless,
    /// Also send critical alerts as desktop notifications.
    #[arg(long)]
    notify: bool,
}

#[derive(Args, Debug, Default, Clone)]
struct SplitArgs {
    #[command(flatten)]
    attach: Attach,
    /// Width of the new pane as a percentage.
    #[arg(long, default_value = "45", value_parser = clap::value_parser!(u8).range(10..=90))]
    size: u8,
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
}

/// Headless rendering options (`run --once`).
#[derive(Args, Debug, Default, Clone)]
struct Headless {
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
}

/// Print to stdout without panicking when the reader went away (`| head`).
fn emit(text: &str) {
    use std::io::Write;
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

fn main() {
    // A closed pipe (`cctop query … | head`) must not be a crash.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Run(RunArgs::default())) {
        Command::Run(args) => run(args),
        Command::Query(q) => query(q),
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
            }
            // `--md` is the default output; the flag exists for explicitness.
            let _ = args.md;
            print!("{}", registry::markdown());
        }
        Command::Install(a) => {
            let path = cctop::install::settings_path();
            if let Err(e) =
                cctop::install::apply(&path, a.yes, cctop::install::install_transform, "install")
            {
                eprintln!("cctop: {e}");
                std::process::exit(1);
            }
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
        }
        Command::Hook => std::process::exit(cctop::hooks::run_hook()),
        Command::StatuslineShim { original } => {
            let original: Vec<String> = original.into_iter().skip_while(|a| a == "--").collect();
            std::process::exit(cctop::status::run_shim(&original));
        }
        Command::Split(sp) => {
            // Resolve first so the pane attaches to exactly this session.
            let (_, info) = match cctop::load::resolve(&target(&sp.attach)) {
                Ok(x) => x,
                Err(e) => {
                    eprintln!("cctop: {e}");
                    std::process::exit(cctop::discover::DiscoverError::EXIT_CODE);
                }
            };
            let key = if info.session_id.is_empty() {
                sp.attach.session.clone().unwrap_or_default()
            } else {
                info.session_id
            };
            std::process::exit(cctop::split::run(&key, sp.size));
        }
        Command::Advise(a) => advise(a),
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
        }
    }
}

fn run(args: RunArgs) {
    use cctop::app::{self, App};
    use cctop::ui::state::{SessionInfo, State};
    let RunArgs {
        attach,
        headless,
        notify,
    } = args;
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
    app.desktop_notify = notify;
    app.state.now_ms = app::now_ms();
    if let Some(projects) = cctop::baseline::default_projects_dir() {
        app.state.baseline = Some(cctop::baseline::load_or_compute(
            &cctop::status::cctop_dir(),
            &projects,
            app.state.now_ms,
        ));
    }
    cctop::hooks::prune(&cctop::status::cctop_dir(), app.state.now_ms);

    if headless.once {
        cctop::attach::attach_headless(&mut app, &transcript, session_info);
        if let Some(keys) = headless.keys.as_deref() {
            for k in app::parse_keys(keys) {
                app.handle_key(k);
            }
        }
        let (w, h) = headless
            .size
            .as_deref()
            .and_then(app::parse_size)
            .unwrap_or((60, 51));
        let text = app::render_to_string(&app, w, h);
        match headless.render_to {
            Some(path) => std::fs::write(&path, text).unwrap_or_else(|e| {
                eprintln!("cctop: cannot write {}: {e}", path.display());
                std::process::exit(1);
            }),
            None => emit(&text),
        }
        return;
    }

    cctop::attach::attach(&mut app, &transcript, session_info, true);
    if let Err(e) = app::run_tui(app) {
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
    emit(&format!(
        "{}\n",
        serde_json::to_string_pretty(&out).unwrap()
    ));
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
