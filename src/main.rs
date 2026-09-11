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
}

fn main() {
    let cli = Cli::parse();
    let name = match cli.command.unwrap_or(Command::Run(Attach::default())) {
        Command::Run(attach) => {
            let q = cctop::discover::Query::from_env(attach.session, attach.cwd);
            match cctop::discover::resolve_system(&q, attach.wait) {
                Ok(s) => println!(
                    "cctop run: would attach to {} ({}, pid {}, {})",
                    s.name,
                    s.session_id,
                    s.pid,
                    s.cwd.display()
                ),
                Err(e) => {
                    eprintln!("cctop: {e}");
                    std::process::exit(cctop::discover::DiscoverError::EXIT_CODE);
                }
            }
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
