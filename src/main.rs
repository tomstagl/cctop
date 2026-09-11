//! cctop — a btop-style live dashboard for Claude Code internals.

use clap::{Parser, Subcommand};

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
    Run,
    /// Print session metrics as JSON.
    Query,
    /// Print the metrics registry as Markdown.
    Metrics,
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

fn main() {
    let cli = Cli::parse();
    let name = match cli.command.unwrap_or(Command::Run) {
        Command::Run => "run",
        Command::Query => "query",
        Command::Metrics => "metrics",
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
