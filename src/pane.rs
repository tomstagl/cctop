//! `cctop pane status`: why the cctop pane is, or is not, docked inside
//! Claude Code right now, one line per prerequisite with the action that
//! fixes it. The `/cctop` skill runs it when the function-hooks pane did not
//! answer, so the person is told the exact state instead of a shrug.
//!
//! The prerequisites and where each is read from are documented in
//! `docs/claude-code-panels.md` §7:
//!
//! 1. Claude Code ≥ 2.1.269 with function hooks on
//!    (`CLAUDE_CODE_ENABLE_FUNCTION_HOOKS` in the environment; the settings
//!    `env` block applies at the next start).
//! 2. The installed cctop plugin ships the hooks module and it loaded in
//!    this session (`~/.cctop/pane/<session>.json`, `loaded: true`, written
//!    by the module at session start), and the pair works together: not a
//!    known-broken pair ([`KNOWN_INCOMPATIBLE`]), and Claude Code no newer
//!    than the module's `TESTED_WITH` (function hooks are early access and
//!    move between releases; issue #4).
//! 3. The fullscreen renderer (`tui` in settings).
//! 4. A terminal of 110 columns or more.
//! 5. The diff panel closed (`diffSidebarOpen` in `~/.claude.json` says
//!    whether it was left open; while it shows, it holds the dock).
//!
//! `gather` reads the machine, `report` judges pure `Inputs` (tests hand
//! them in), `render` writes the lines a person reads.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// The Claude Code version function hooks first shipped in.
pub const MIN_CLAUDE_VERSION: (u64, u64, u64) = (2, 1, 269);
/// The narrowest terminal that docks a pane (docs/claude-code-panels.md §4).
pub const MIN_DOCK_COLUMNS: u16 = 110;
/// The cctop plugin version that first ships the hooks module.
pub const MIN_PLUGIN_VERSION: (u64, u64, u64) = (0, 2, 0);

/// A plugin range and a Claude Code range known not to work together.
#[derive(Debug, Clone, Copy)]
pub struct Incompatibility {
    /// Plugins older than this …
    pub plugin_before: (u64, u64, u64),
    /// … on Claude Code at least this.
    pub claude_from: (u64, u64, u64),
    /// What moved, in one clause.
    pub why: &'static str,
}

/// The pairs that fail, from experience; checked before `TESTED_WITH`, so a
/// listed pair is ✗ with the fix whatever the module says it was tested
/// with. Every entry is a contract change that took a plugin release to
/// follow; add one when the next lands.
pub const KNOWN_INCOMPATIBLE: &[Incompatibility] = &[Incompatibility {
    plugin_before: (0, 4, 1),
    claude_from: (2, 1, 271),
    why: "2.1.271 made `$.clock.now()` resolve a Promise and the module did arithmetic on it, so every hook failed (issue #3)",
}];
/// How stale a marker's `heartbeatAt` may be for `open: true` to count
/// (matches `split::pane_marker_open`).
const MARKER_STALE_AFTER_MS: i64 = 30_000;
/// Slack when comparing the marker's `loadedAt` with the session process
/// start: the registry entry is written before the hooks module loads.
const LOAD_GRACE_MS: i64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Ok,
    Warn,
    Fail,
    Unknown,
}

impl Verdict {
    fn glyph(self) -> &'static str {
        match self {
            Verdict::Ok => "✓",
            Verdict::Warn => "!",
            Verdict::Fail => "✗",
            Verdict::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub id: &'static str,
    pub verdict: Verdict,
    pub text: String,
    /// What the person does about it; absent when nothing is needed.
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub session_id: Option<String>,
    pub checks: Vec<Check>,
    /// The hooks module runs in this session: `/cctop-pane` will answer.
    pub ready: bool,
    /// The one thing to do next.
    pub next: String,
}

/// The cctop plugin as `~/.claude/plugins/installed_plugins.json` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub install_path: PathBuf,
    /// `<install_path>/hooks/hooks.json` names a hooks module.
    pub has_module: bool,
    /// `TESTED_WITH` in `<install_path>/hooks/model.ts`: the Claude Code
    /// version the module's contract was generated from (the header badge
    /// shows it). None when the file or the constant is missing.
    pub tested_with: Option<String>,
}

/// Everything the checks look at, read once.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub session_id: Option<String>,
    /// `claude --version`, e.g. `2.1.270`; None when `claude` did not answer.
    pub claude_version: Option<String>,
    /// `CLAUDE_CODE_ENABLE_FUNCTION_HOOKS` as this process sees it.
    pub env_flag: Option<String>,
    /// The same key in the `env` block of `~/.claude/settings.json`.
    pub settings_flag: Option<String>,
    pub installed: Option<Installed>,
    /// `~/.cctop/pane/<session>.json`, parsed.
    pub marker: Option<serde_json::Value>,
    /// `started_at` of the session's registry entry (epoch ms).
    pub session_started_at: Option<i64>,
    /// Whether a Claude Code process is registered and alive for the
    /// session; None when the registry could not be read.
    pub session_alive: Option<bool>,
    /// `tui` from the settings files and which file said so.
    pub tui: Option<(String, String)>,
    pub columns: Option<u16>,
    /// `diffSidebarOpen` from `~/.claude.json`.
    pub diff_sidebar_open: Option<bool>,
    pub now_ms: i64,
}

/// `1`, `true`, `yes`, `on` (any case) are on; anything else is off, as
/// Claude Code's own boolean env parsing reads them.
pub fn env_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// `major.minor.patch` out of a version string (`2.1.270 (Claude Code)`).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let head = text
        .trim()
        .split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .next()?;
    let mut parts = head.split('.').map(|p| p.parse::<u64>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

fn check(
    id: &'static str,
    verdict: Verdict,
    text: impl Into<String>,
    action: Option<String>,
) -> Check {
    Check {
        id,
        verdict,
        text: text.into(),
        action,
    }
}

/// Whether the marker was written by the hooks module in this session's
/// current process: `loaded: true`, the same session id, and a `loadedAt`
/// no older than the process start (a resumed session keeps its id, so a
/// marker from an earlier process must not count).
fn marker_loaded(inputs: &Inputs) -> bool {
    let Some(marker) = &inputs.marker else {
        return false;
    };
    if marker.get("loaded").and_then(|v| v.as_bool()) != Some(true) {
        return false;
    }
    if let (Some(want), Some(have)) = (
        inputs.session_id.as_deref(),
        marker.get("sessionId").and_then(|v| v.as_str()),
    ) {
        if want != have {
            return false;
        }
    }
    match (
        inputs.session_started_at,
        marker
            .get("loadedAt")
            .and_then(|v| v.as_str())
            .and_then(crate::metrics::cost::parse_ts_ms),
    ) {
        (Some(started), Some(loaded)) => loaded + LOAD_GRACE_MS >= started,
        _ => true,
    }
}

/// The marker's `open: true` with a fresh heartbeat.
fn marker_open(inputs: &Inputs) -> bool {
    let Some(marker) = &inputs.marker else {
        return false;
    };
    if marker.get("open").and_then(|v| v.as_bool()) != Some(true) {
        return false;
    }
    marker
        .get("heartbeatAt")
        .and_then(|v| v.as_str())
        .and_then(crate::metrics::cost::parse_ts_ms)
        .is_some_and(|at| inputs.now_ms - at < MARKER_STALE_AFTER_MS)
}

fn marker_str<'a>(inputs: &'a Inputs, key: &str) -> Option<&'a str> {
    inputs.marker.as_ref()?.get(key)?.as_str()
}

pub fn report(inputs: &Inputs) -> Report {
    let loaded = marker_loaded(inputs);
    let hooks_on = loaded || inputs.env_flag.as_deref().is_some_and(env_truthy);
    let mut checks = Vec::new();

    // 0. The session itself: a report about a process that is gone would
    // otherwise read as if the pane were still there.
    if let (Some(_), Some(false)) = (inputs.session_id.as_deref(), inputs.session_alive) {
        checks.push(check(
            "session",
            Verdict::Warn,
            "no running Claude Code process is registered for this session; the lines below describe its last state",
            None,
        ));
    }

    // 1. Claude Code version.
    match inputs.claude_version.as_deref().and_then(parse_version) {
        Some(v) if v >= MIN_CLAUDE_VERSION => checks.push(check(
            "claude",
            Verdict::Ok,
            format!(
                "Claude Code {}.{}.{} (function hooks need {}.{}.{} or newer)",
                v.0, v.1, v.2, MIN_CLAUDE_VERSION.0, MIN_CLAUDE_VERSION.1, MIN_CLAUDE_VERSION.2
            ),
            None,
        )),
        Some(v) => checks.push(check(
            "claude",
            Verdict::Fail,
            format!(
                "Claude Code {}.{}.{} is older than {}.{}.{}, the first with function hooks",
                v.0, v.1, v.2, MIN_CLAUDE_VERSION.0, MIN_CLAUDE_VERSION.1, MIN_CLAUDE_VERSION.2
            ),
            Some("update Claude Code (`claude update`), then restart it".into()),
        )),
        None => checks.push(check(
            "claude",
            Verdict::Unknown,
            "Claude Code version unknown (`claude --version` did not answer from here)",
            None,
        )),
    }

    // 2. Function hooks.
    if loaded {
        checks.push(check(
            "function-hooks",
            Verdict::Ok,
            "function hooks on (the hooks module is running in this session)",
            None,
        ));
    } else if hooks_on {
        checks.push(check(
            "function-hooks",
            Verdict::Ok,
            "function hooks on (CLAUDE_CODE_ENABLE_FUNCTION_HOOKS is set in this session's environment)",
            None,
        ));
    } else if inputs.settings_flag.as_deref().is_some_and(env_truthy) {
        checks.push(check(
            "function-hooks",
            Verdict::Warn,
            "function hooks are set in ~/.claude/settings.json but not in this session's environment",
            Some("restart Claude Code so the env block applies".into()),
        ));
    } else {
        checks.push(check(
            "function-hooks",
            Verdict::Fail,
            "function hooks off",
            Some(
                "add \"CLAUDE_CODE_ENABLE_FUNCTION_HOOKS\": \"1\" to the \"env\" block of ~/.claude/settings.json (or export it in the shell), then restart Claude Code"
                    .into(),
            ),
        ));
    }

    // 3. The hooks module. Its session.start self-check (the marker's
    // `selfCheck`, plugin 0.8.0+) names a `$` surface that moved; the
    // module then runs on its fallbacks and says so here.
    if loaded {
        let version = marker_str(inputs, "version").unwrap_or("?");
        match marker_str(inputs, "selfCheck") {
            Some(problems) if problems != "ok" => checks.push(check(
                "hooks-module",
                Verdict::Warn,
                format!(
                    "hooks module loaded in this session (cctop plugin {version}), but its self-check failed: {problems}"
                ),
                Some(format!(
                    "{UPDATE_PLUGIN}, then restart Claude Code; if the newest plugin fails too, report it with `claude --version`"
                )),
            )),
            _ => checks.push(check(
                "hooks-module",
                Verdict::Ok,
                format!("hooks module loaded in this session (cctop plugin {version})"),
                None,
            )),
        }
    } else {
        match &inputs.installed {
            None => checks.push(check(
                "hooks-module",
                Verdict::Fail,
                "cctop plugin not installed",
                Some(
                    "`claude plugin marketplace add tomstagl/cctop` then `claude plugin install cctop@cctop`, then restart Claude Code"
                        .into(),
                ),
            )),
            Some(installed) if !installed.has_module => checks.push(check(
                "hooks-module",
                Verdict::Fail,
                format!(
                    "installed cctop plugin {} has no hooks module (skills only; {}.{}.{} or newer ships it)",
                    installed.version, MIN_PLUGIN_VERSION.0, MIN_PLUGIN_VERSION.1, MIN_PLUGIN_VERSION.2
                ),
                Some(
                    "`claude plugin update cctop@cctop` (or start with `claude --plugin-dir <checkout>/plugin`), then restart Claude Code"
                        .into(),
                ),
            )),
            Some(installed) => {
                let action = if hooks_on {
                    "restart Claude Code (or /reload-plugins) and accept the module when /plugin asks"
                } else {
                    "turn function hooks on (above); the module loads at the next start"
                };
                let (verdict, text) = match inputs.session_id.as_deref() {
                    Some(_) => (
                        Verdict::Fail,
                        format!(
                            "cctop plugin {} ships the hooks module, but it is not loaded in this session",
                            installed.version
                        ),
                    ),
                    None => (
                        Verdict::Unknown,
                        format!(
                            "cctop plugin {} ships the hooks module; whether it loaded is unknown without a session (pass --session <id>)",
                            installed.version
                        ),
                    ),
                };
                checks.push(check("hooks-module", verdict, text, Some(action.into())));
            }
        }
    }

    // 3b. The pair: the plugin that runs (the marker's version, else the
    // installed one) against this Claude Code. A known-broken pair is ✗
    // with the fix; a Claude Code newer than what the module was tested
    // with is a warning, since function hooks move between releases and
    // the module cannot know what changed (issue #4).
    if let Some(c) = compatibility(inputs, loaded) {
        checks.push(c);
    }

    // 4. Renderer.
    match &inputs.tui {
        Some((mode, source)) if mode == "fullscreen" => checks.push(check(
            "renderer",
            Verdict::Ok,
            format!("fullscreen renderer (tui = \"fullscreen\" in {source})"),
            None,
        )),
        Some((mode, source)) => checks.push(check(
            "renderer",
            Verdict::Fail,
            format!("classic renderer (tui = \"{mode}\" in {source}): the pane draws above the prompt, never docked"),
            Some("run /tui fullscreen (saved to settings)".into()),
        )),
        None => checks.push(check(
            "renderer",
            Verdict::Warn,
            "renderer not pinned in settings: Claude Code decides per start (often the classic renderer)",
            Some("run /tui fullscreen once; it is saved and the pane docks from then on".into()),
        )),
    }

    // 5. Terminal width.
    match inputs.columns {
        Some(c) if c >= MIN_DOCK_COLUMNS => checks.push(check(
            "terminal",
            Verdict::Ok,
            format!("terminal {c} columns ({MIN_DOCK_COLUMNS} or more dock the pane)"),
            None,
        )),
        Some(c) => checks.push(check(
            "terminal",
            Verdict::Fail,
            format!("terminal {c} columns: below {MIN_DOCK_COLUMNS} the pane draws above the prompt"),
            Some(format!("widen the terminal to {MIN_DOCK_COLUMNS}+ columns")),
        )),
        None => checks.push(check(
            "terminal",
            Verdict::Unknown,
            format!("terminal width not measurable from here ({MIN_DOCK_COLUMNS} or more columns dock the pane)"),
            None,
        )),
    }

    // 6. The diff panel.
    let pane_hidden = marker_open(inputs) && marker_str(inputs, "visibility") == Some("hidden");
    if pane_hidden {
        checks.push(check(
            "diff-panel",
            Verdict::Warn,
            "the /diff panel holds the side dock: the cctop pane is open but not shown",
            Some("run /diff to hide it; cctop takes the dock".into()),
        ));
    } else if inputs.diff_sidebar_open == Some(true) {
        checks.push(check(
            "diff-panel",
            Verdict::Warn,
            "the /diff panel was open when last toggled (diffSidebarOpen in ~/.claude.json); while it shows, it holds the side dock",
            Some("if it is showing, run /diff to hide it; cctop takes the dock".into()),
        ));
    } else {
        checks.push(check("diff-panel", Verdict::Ok, "diff panel closed", None));
    }

    // 7. The pane itself, when the module runs.
    if loaded {
        if marker_open(inputs) {
            let placement = marker_str(inputs, "placement").unwrap_or("dock");
            let width = inputs
                .marker
                .as_ref()
                .and_then(|m| m.get("bodyColumns"))
                .and_then(|v| v.as_u64())
                .map(|w| format!(" ({w} columns)"))
                .unwrap_or_default();
            let (verdict, text, action) = match marker_str(inputs, "visibility") {
                Some("hidden") => (
                    Verdict::Warn,
                    "pane open but hidden behind the /diff panel".to_string(),
                    Some("run /diff".to_string()),
                ),
                Some("visible") if placement == "inline" => (
                    Verdict::Ok,
                    "pane open, drawn above the prompt (inline)".to_string(),
                    None,
                ),
                Some("visible") => (Verdict::Ok, format!("pane open, docked{width}"), None),
                _ => (Verdict::Ok, "pane open".to_string(), None),
            };
            checks.push(check("pane", verdict, text, action));
        } else {
            checks.push(check("pane", Verdict::Ok, "pane closed", None));
        }
    }

    let next = if !loaded {
        "not ready: fix the ✗ lines, restart Claude Code, then run /cctop-pane".to_string()
    } else if !marker_open(inputs) {
        "run /cctop-pane to open the dashboard beside the transcript".to_string()
    } else if pane_hidden {
        "run /diff to hide the diff panel; the cctop pane appears in its place".to_string()
    } else {
        "the cctop pane is open; /cctop-pane closes it, /cctop-pane <view> switches views"
            .to_string()
    };

    Report {
        session_id: inputs.session_id.clone(),
        checks,
        ready: loaded,
        next,
    }
}

const UPDATE_PLUGIN: &str =
    "`claude plugin marketplace update cctop && claude plugin update cctop@cctop`";

fn fmt_version(v: (u64, u64, u64)) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// The `compatibility` line, when there is a module to pair: the running
/// module's version (the marker's when it loaded and says one, else the
/// installed plugin's) and what it was tested with (the marker's
/// `testedWith`, else the install's `hooks/model.ts`) against
/// `claude --version`. None without a module or without a Claude Code
/// version to compare with (those have their own lines).
fn compatibility(inputs: &Inputs, loaded: bool) -> Option<Check> {
    let installed = inputs.installed.as_ref();
    let has_module = loaded || installed.is_some_and(|i| i.has_module);
    if !has_module {
        return None;
    }
    let claude = inputs.claude_version.as_deref().and_then(parse_version)?;
    let marker_version = if loaded {
        marker_str(inputs, "version")
    } else {
        None
    };
    let plugin_text = marker_version
        .or(installed.map(|i| i.version.as_str()))
        .unwrap_or("?");
    let plugin = parse_version(plugin_text)?;
    let tested_with = if loaded {
        marker_str(inputs, "testedWith")
    } else {
        None
    }
    .or(installed.and_then(|i| i.tested_with.as_deref()));

    if let Some(pair) = KNOWN_INCOMPATIBLE
        .iter()
        .find(|p| plugin < p.plugin_before && claude >= p.claude_from)
    {
        return Some(check(
            "compatibility",
            Verdict::Fail,
            format!(
                "cctop plugin {} does not work on Claude Code {}: {}",
                fmt_version(plugin),
                fmt_version(claude),
                pair.why
            ),
            Some(format!(
                "{UPDATE_PLUGIN} (plugin {} or newer), then restart Claude Code",
                fmt_version(pair.plugin_before)
            )),
        ));
    }
    Some(match tested_with.and_then(parse_version) {
        Some(t) if claude > t => check(
            "compatibility",
            Verdict::Warn,
            format!(
                "Claude Code {} is newer than cctop plugin {} was tested with ({}); function hooks are early access and change between releases",
                fmt_version(claude),
                fmt_version(plugin),
                fmt_version(t)
            ),
            Some(format!(
                "{UPDATE_PLUGIN} when a newer plugin is out, then restart Claude Code; if a hook fails meanwhile, report it with both versions"
            )),
        ),
        Some(t) if claude == t => check(
            "compatibility",
            Verdict::Ok,
            format!(
                "cctop plugin {} tested with Claude Code {}",
                fmt_version(plugin),
                fmt_version(t)
            ),
            None,
        ),
        Some(t) => check(
            "compatibility",
            Verdict::Ok,
            format!(
                "cctop plugin {} tested with Claude Code {} (this is {}, older; the module awaits every host call, so it runs on both)",
                fmt_version(plugin),
                fmt_version(t),
                fmt_version(claude)
            ),
            None,
        ),
        None => check(
            "compatibility",
            Verdict::Unknown,
            format!(
                "cctop plugin {}: the Claude Code version it was tested with is not readable (TESTED_WITH in hooks/model.ts)",
                fmt_version(plugin)
            ),
            None,
        ),
    })
}

/// The lines a person reads (the skill relays them verbatim).
pub fn render(report: &Report) -> String {
    let mut out = String::new();
    match &report.session_id {
        Some(id) => out.push_str(&format!("cctop pane status · session {}\n", short_id(id))),
        None => out.push_str("cctop pane status · session unknown\n"),
    }
    for c in &report.checks {
        out.push_str(&format!("  {} {}", c.verdict.glyph(), c.text));
        if let Some(action) = &c.action {
            out.push_str(&format!(" → {action}"));
        }
        out.push('\n');
    }
    out.push_str(&format!("→ {}\n", report.next));
    out
}

fn short_id(id: &str) -> &str {
    let end = id.find('-').unwrap_or(id.len().min(8));
    &id[..end.min(id.len())]
}

// ---------------------------------------------------------------- gathering

/// The session id: `--session`, else what Claude Code exports to its child
/// processes (`CLAUDE_CODE_SESSION_ID`; older docs say `CLAUDE_SESSION_ID`).
pub fn session_from_env(explicit: Option<&str>) -> Option<String> {
    if let Some(s) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(s.to_string());
    }
    ["CLAUDE_CODE_SESSION_ID", "CLAUDE_SESSION_ID"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// The `tui` key across the settings files Claude Code merges, the most
/// specific file winning: user, user-local, project, project-local.
pub fn read_tui(home: &Path, cwd: &Path) -> Option<(String, String)> {
    let candidates = [
        (
            home.join(".claude/settings.json"),
            "~/.claude/settings.json",
        ),
        (
            home.join(".claude/settings.local.json"),
            "~/.claude/settings.local.json",
        ),
        (cwd.join(".claude/settings.json"), ".claude/settings.json"),
        (
            cwd.join(".claude/settings.local.json"),
            ".claude/settings.local.json",
        ),
    ];
    let mut found = None;
    for (path, label) in candidates {
        if let Some(mode) =
            read_json(&path).and_then(|v| v.get("tui").and_then(|t| t.as_str()).map(str::to_string))
        {
            found = Some((mode, label.to_string()));
        }
    }
    found
}

/// The `env.CLAUDE_CODE_ENABLE_FUNCTION_HOOKS` value of `~/.claude/settings.json`.
pub fn read_settings_flag(home: &Path) -> Option<String> {
    read_json(&home.join(".claude/settings.json"))?
        .get("env")?
        .get("CLAUDE_CODE_ENABLE_FUNCTION_HOOKS")?
        .as_str()
        .map(str::to_string)
}

/// The cctop entry of `~/.claude/plugins/installed_plugins.json`: the
/// user-scope install when there is one, else the first listed.
pub fn read_installed(home: &Path) -> Option<Installed> {
    let doc = read_json(&home.join(".claude/plugins/installed_plugins.json"))?;
    let plugins = doc.get("plugins")?.as_object()?;
    let (_, entries) = plugins
        .iter()
        .find(|(k, _)| k.as_str() == "cctop" || k.starts_with("cctop@"))?;
    let entries = entries.as_array()?;
    let entry = entries
        .iter()
        .find(|e| e.get("scope").and_then(|s| s.as_str()) == Some("user"))
        .or_else(|| entries.first())?;
    let install_path = PathBuf::from(entry.get("installPath")?.as_str()?);
    let version = entry
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    Some(Installed {
        has_module: hooks_module_declared(&install_path),
        tested_with: read_tested_with(&install_path),
        version,
        install_path,
    })
}

/// Whether `<plugin>/hooks/hooks.json` names at least one hooks module.
pub fn hooks_module_declared(plugin_root: &Path) -> bool {
    read_json(&plugin_root.join("hooks/hooks.json"))
        .and_then(|v| v.get("modules")?.as_array().map(|m| !m.is_empty()))
        .unwrap_or(false)
}

/// `TESTED_WITH = '2.1.273'` out of `<plugin>/hooks/model.ts` (the module
/// ships as source; every plugin with the module has the line, since 0.2.0).
pub fn read_tested_with(plugin_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(plugin_root.join("hooks/model.ts")).ok()?;
    let rest = text.split("TESTED_WITH").nth(1)?;
    let quoted = rest.split(['\'', '"']).nth(1)?;
    parse_version(quoted).map(fmt_version)
}

fn read_claude_version() -> Option<String> {
    let out = Command::new("claude").arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    parse_version(&text).map(|(a, b, c)| format!("{a}.{b}.{c}"))
}

/// The column count of the terminal device at `path` (`/dev/tty`, or the
/// session's own `/dev/ttysNNN`), by TIOCGWINSZ.
#[cfg(unix)]
fn columns_of_tty(path: &Path) -> Option<u16> {
    use std::os::unix::io::AsRawFd;
    let tty = std::fs::File::open(path).ok()?;
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    // SAFETY: TIOCGWINSZ writes a winsize into the struct we pass.
    let rc = unsafe { libc::ioctl(tty.as_raw_fd(), libc::TIOCGWINSZ, &mut ws) };
    (rc == 0 && ws.ws_col > 0).then_some(ws.ws_col)
}

#[cfg(not(unix))]
fn columns_of_tty(_path: &Path) -> Option<u16> {
    None
}

/// The controlling terminal of `pid` as `ps` names it (`ttys003`).
fn tty_of_pid(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", "tty=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!tty.is_empty() && tty != "??" && tty != "?").then_some(tty)
}

/// The width of the terminal the session is drawn on: this process's own
/// controlling TTY when it has one, else the session process's TTY (a
/// Claude Code Bash tool has no TTY, but the session it runs in does; the
/// device is the person's own, so it opens), else `$COLUMNS`.
fn terminal_columns(session_pid: Option<u32>) -> Option<u16> {
    if let Some(c) = columns_of_tty(Path::new("/dev/tty")) {
        return Some(c);
    }
    if let Some(c) = session_pid
        .and_then(tty_of_pid)
        .and_then(|tty| columns_of_tty(&Path::new("/dev").join(tty)))
    {
        return Some(c);
    }
    std::env::var("COLUMNS").ok()?.trim().parse().ok()
}

/// Reads everything `report` needs from this machine.
pub fn gather(session: Option<&str>) -> Inputs {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let session_id = session_from_env(session);
    let marker = session_id
        .as_deref()
        .and_then(|id| read_json(&home.join(format!(".cctop/pane/{id}.json"))));
    // The session's live registry entry: when its process started (to tell a
    // marker of this process from an earlier one) and its pid (for the TTY).
    let live = session_id.as_deref().and_then(|id| {
        let dir = crate::registry::default_dir()?;
        crate::registry::list(&dir)
            .into_iter()
            .filter(|s| s.session_id == id && s.is_alive())
            .max_by_key(|s| s.started_at)
    });
    let session_started_at = live.as_ref().map(|s| s.started_at as i64);
    let session_pid = live.as_ref().map(|s| s.pid);
    let session_alive = session_id
        .as_deref()
        .and_then(|_| crate::registry::default_dir())
        .filter(|dir| dir.is_dir())
        .map(|_| live.is_some());
    Inputs {
        session_id,
        claude_version: read_claude_version(),
        env_flag: std::env::var("CLAUDE_CODE_ENABLE_FUNCTION_HOOKS").ok(),
        settings_flag: read_settings_flag(&home),
        installed: read_installed(&home),
        marker,
        session_started_at,
        session_alive,
        tui: read_tui(&home, &cwd),
        columns: terminal_columns(session_pid),
        diff_sidebar_open: read_json(&home.join(".claude.json"))
            .and_then(|v| v.get("diffSidebarOpen")?.as_bool()),
        now_ms: crate::app::now_ms(),
    }
}

/// `cctop pane status`: prints the report; exit 0 when the hooks module runs
/// in this session (the pane can answer), 2 otherwise.
pub fn run_status(session: Option<&str>, json: bool) -> i32 {
    let inputs = gather(session);
    let report = report(&inputs);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".into())
        );
    } else {
        print!("{}", render(&report));
    }
    if report.ready {
        0
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_789_400_000_000;

    fn marker(open: bool, visibility: &str, heartbeat_offset_ms: i64) -> serde_json::Value {
        // A fixed-format ISO string parse_ts_ms accepts.
        let at = |ms: i64| iso(ms / 1000, ms % 1000);
        serde_json::json!({
            "version": "0.2.0",
            "sessionId": "sess-1",
            "loaded": true,
            "loadedAt": at(T0),
            "openedAt": if open { serde_json::Value::String(at(T0)) } else { serde_json::Value::Null },
            "heartbeatAt": at(T0 + heartbeat_offset_ms),
            "open": open,
            "visibility": visibility,
            "placement": "dock",
            "bodyColumns": 72,
            "viewportColumns": 162,
        })
    }

    // 2026-09-14T…Z for an epoch: enough of a formatter for the tests, the
    // crate has none (see split.rs's marker tests, which hand-write theirs).
    fn iso(secs: i64, sub: i64) -> String {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (h, m, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
        // Civil-from-days (Howard Hinnant), for dates after 1970.
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let mo = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if mo <= 2 { y + 1 } else { y };
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.{sub:03}Z")
    }

    fn ready_inputs() -> Inputs {
        Inputs {
            session_id: Some("sess-1".into()),
            claude_version: Some("2.1.270".into()),
            env_flag: Some("1".into()),
            settings_flag: Some("1".into()),
            installed: Some(Installed {
                version: "0.2.0".into(),
                install_path: "/plugins/cctop".into(),
                has_module: true,
                tested_with: Some("2.1.270".into()),
            }),
            marker: Some(marker(false, "unknown", 0)),
            session_started_at: Some(T0 - 5_000),
            session_alive: Some(true),
            tui: Some(("fullscreen".into(), "~/.claude/settings.json".into())),
            columns: Some(162),
            diff_sidebar_open: Some(false),
            now_ms: T0 + 1_000,
        }
    }

    fn verdicts(report: &Report) -> Vec<(&'static str, Verdict)> {
        report.checks.iter().map(|c| (c.id, c.verdict)).collect()
    }

    #[test]
    fn iso_round_trips_through_parse_ts_ms() {
        let text = iso(T0 / 1000, T0 % 1000);
        assert_eq!(crate::metrics::cost::parse_ts_ms(&text), Some(T0), "{text}");
    }

    #[test]
    fn env_truthy_matches_claude_code() {
        for on in ["1", "true", "TRUE", " yes ", "on"] {
            assert!(env_truthy(on), "{on}");
        }
        for off in ["0", "false", "", "no", "2"] {
            assert!(!env_truthy(off), "{off}");
        }
    }

    #[test]
    fn parse_version_reads_claude_output() {
        assert_eq!(parse_version("2.1.270 (Claude Code)\n"), Some((2, 1, 270)));
        assert_eq!(parse_version("2.1.270"), Some((2, 1, 270)));
        assert_eq!(parse_version("cctop 0.2.0"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn everything_in_place_is_ready_and_says_run_the_command() {
        let report = super::report(&ready_inputs());
        assert!(report.ready);
        assert!(
            report.checks.iter().all(|c| c.verdict == Verdict::Ok),
            "{report:?}"
        );
        assert_eq!(
            verdicts(&report)
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            [
                "claude",
                "function-hooks",
                "hooks-module",
                "compatibility",
                "renderer",
                "terminal",
                "diff-panel",
                "pane"
            ]
        );
        assert_eq!(
            report.next,
            "run /cctop-pane to open the dashboard beside the transcript"
        );
        let text = render(&report);
        assert!(
            text.starts_with("cctop pane status · session sess\n"),
            "{text}"
        );
        assert!(
            text.contains("✓ hooks module loaded in this session (cctop plugin 0.2.0)"),
            "{text}"
        );
        assert!(
            text.contains("✓ cctop plugin 0.2.0 tested with Claude Code 2.1.270\n"),
            "{text}"
        );
        assert!(
            text.ends_with("→ run /cctop-pane to open the dashboard beside the transcript\n"),
            "{text}"
        );
    }

    #[test]
    fn a_failed_self_check_in_the_marker_is_relayed_on_the_module_line() {
        let mut inputs = ready_inputs();
        let mut m = marker(false, "unknown", 0);
        m["selfCheck"] =
            serde_json::Value::String("$.clock.now() resolved object, not a number".into());
        inputs.marker = Some(m);
        let report = super::report(&inputs);
        assert!(report.ready, "the module runs, on its fallbacks");
        let module = report
            .checks
            .iter()
            .find(|c| c.id == "hooks-module")
            .unwrap();
        assert_eq!(module.verdict, Verdict::Warn);
        assert_eq!(
            module.text,
            "hooks module loaded in this session (cctop plugin 0.2.0), but its self-check failed: $.clock.now() resolved object, not a number"
        );
        assert!(
            module
                .action
                .as_deref()
                .unwrap()
                .starts_with("`claude plugin marketplace update cctop"),
            "{:?}",
            module.action
        );
        // `ok`, or a marker from before the self-check, is the plain line.
        let mut m = marker(false, "unknown", 0);
        m["selfCheck"] = serde_json::Value::String("ok".into());
        inputs.marker = Some(m);
        let report = super::report(&inputs);
        let module = report
            .checks
            .iter()
            .find(|c| c.id == "hooks-module")
            .unwrap();
        assert_eq!(module.verdict, Verdict::Ok);
    }

    #[test]
    fn the_known_broken_pair_is_a_failure_with_the_update() {
        // Issue #3 as this machine had it on 2026-09-15: plugin 0.4.0 on
        // Claude Code 2.1.272, every hook failing, no marker.
        let mut inputs = ready_inputs();
        inputs.claude_version = Some("2.1.272".into());
        inputs.installed = Some(Installed {
            version: "0.4.0".into(),
            install_path: "/plugins/cctop/0.4.0".into(),
            has_module: true,
            tested_with: Some("2.1.270".into()),
        });
        inputs.marker = None;
        let report = super::report(&inputs);
        assert!(!report.ready);
        let pair = report
            .checks
            .iter()
            .find(|c| c.id == "compatibility")
            .unwrap();
        assert_eq!(pair.verdict, Verdict::Fail);
        assert_eq!(
            pair.text,
            "cctop plugin 0.4.0 does not work on Claude Code 2.1.272: 2.1.271 made `$.clock.now()` resolve a Promise and the module did arithmetic on it, so every hook failed (issue #3)"
        );
        assert_eq!(
            pair.action.as_deref(),
            Some("`claude plugin marketplace update cctop && claude plugin update cctop@cctop` (plugin 0.4.1 or newer), then restart Claude Code")
        );
        // The same plugin on the Claude Code before the change is only
        // unverified for newer releases, not broken.
        inputs.claude_version = Some("2.1.270".into());
        let report = super::report(&inputs);
        let pair = report
            .checks
            .iter()
            .find(|c| c.id == "compatibility")
            .unwrap();
        assert_eq!(pair.verdict, Verdict::Ok, "{}", pair.text);
        // And 0.4.1 on 2.1.272 is past the pair: a warning about the drift.
        inputs.claude_version = Some("2.1.272".into());
        inputs.installed.as_mut().unwrap().version = "0.4.1".into();
        let report = super::report(&inputs);
        let pair = report
            .checks
            .iter()
            .find(|c| c.id == "compatibility")
            .unwrap();
        assert_eq!(pair.verdict, Verdict::Warn, "{}", pair.text);
    }

    #[test]
    fn a_newer_claude_code_than_tested_with_warns_and_names_both() {
        let mut inputs = ready_inputs();
        inputs.claude_version = Some("2.1.280".into());
        inputs.installed = Some(Installed {
            version: "0.7.0".into(),
            install_path: "/plugins/cctop/0.7.0".into(),
            has_module: true,
            tested_with: Some("2.1.273".into()),
        });
        let mut m = marker(false, "unknown", 0);
        m["version"] = serde_json::Value::String("0.7.0".into());
        inputs.marker = Some(m);
        let report = super::report(&inputs);
        assert!(report.ready, "a warning does not unready the pane");
        let pair = report
            .checks
            .iter()
            .find(|c| c.id == "compatibility")
            .unwrap();
        assert_eq!(pair.verdict, Verdict::Warn);
        assert_eq!(
            pair.text,
            "Claude Code 2.1.280 is newer than cctop plugin 0.7.0 was tested with (2.1.273); function hooks are early access and change between releases"
        );
        assert!(
            pair.action.as_deref().unwrap().starts_with(
                "`claude plugin marketplace update cctop && claude plugin update cctop@cctop` when a newer plugin is out"
            ),
            "{:?}",
            pair.action
        );
        // An older Claude Code than the module was tested with is fine and
        // says so.
        inputs.claude_version = Some("2.1.272".into());
        let report = super::report(&inputs);
        let pair = report
            .checks
            .iter()
            .find(|c| c.id == "compatibility")
            .unwrap();
        assert_eq!(pair.verdict, Verdict::Ok);
        assert!(
            pair.text.contains("(this is 2.1.272, older;"),
            "{}",
            pair.text
        );
    }

    #[test]
    fn a_loaded_module_is_paired_by_what_its_marker_says() {
        // The install moved to 0.7.0 (tested with 2.1.273) while this
        // session still runs the 0.4.1 module it loaded at start: the
        // marker's versions are the running pair, the install's are not.
        let mut inputs = ready_inputs();
        inputs.claude_version = Some("2.1.273".into());
        inputs.installed = Some(Installed {
            version: "0.7.0".into(),
            install_path: "/plugins/cctop/0.7.0".into(),
            has_module: true,
            tested_with: Some("2.1.273".into()),
        });
        let mut m = marker(false, "unknown", 0);
        m["version"] = serde_json::Value::String("0.4.1".into());
        m["testedWith"] = serde_json::Value::String("2.1.272".into());
        inputs.marker = Some(m);
        let report = super::report(&inputs);
        let pair = report
            .checks
            .iter()
            .find(|c| c.id == "compatibility")
            .unwrap();
        assert_eq!(pair.verdict, Verdict::Warn);
        assert!(
            pair.text.starts_with(
                "Claude Code 2.1.273 is newer than cctop plugin 0.4.1 was tested with (2.1.272)"
            ),
            "{}",
            pair.text
        );
        // A marker without `testedWith` (plugins before 0.8.0) falls back to
        // the install's model.ts; a null `version` (a -p run) to the install's.
        let mut m = marker(false, "unknown", 0);
        m["version"] = serde_json::Value::Null;
        inputs.marker = Some(m);
        let report = super::report(&inputs);
        let pair = report
            .checks
            .iter()
            .find(|c| c.id == "compatibility")
            .unwrap();
        assert_eq!(pair.verdict, Verdict::Ok);
        assert_eq!(
            pair.text,
            "cctop plugin 0.7.0 tested with Claude Code 2.1.273"
        );
        // No module at all (skills-only plugin, or none): nothing to pair.
        inputs.marker = None;
        inputs.installed = None;
        let report = super::report(&inputs);
        assert!(
            report.checks.iter().all(|c| c.id != "compatibility"),
            "{report:?}"
        );
    }

    #[test]
    fn this_machine_before_the_fix_names_every_action() {
        // 2026-09-14: flag unset, marketplace plugin 0.1.0 without the module,
        // fullscreen pinned, /diff left open, 162 columns, no marker.
        let inputs = Inputs {
            session_id: Some("ab339470-a2c6".into()),
            claude_version: Some("2.1.270".into()),
            env_flag: None,
            settings_flag: None,
            installed: Some(Installed {
                version: "0.1.0".into(),
                install_path: "/plugins/cctop/0.1.0".into(),
                has_module: false,
                tested_with: None,
            }),
            marker: None,
            session_started_at: Some(T0),
            session_alive: Some(true),
            tui: Some(("fullscreen".into(), "~/.claude/settings.json".into())),
            columns: Some(162),
            diff_sidebar_open: Some(true),
            now_ms: T0,
        };
        let report = super::report(&inputs);
        assert!(!report.ready);
        assert_eq!(
            verdicts(&report),
            [
                ("claude", Verdict::Ok),
                ("function-hooks", Verdict::Fail),
                ("hooks-module", Verdict::Fail),
                ("renderer", Verdict::Ok),
                ("terminal", Verdict::Ok),
                ("diff-panel", Verdict::Warn),
            ]
        );
        let text = render(&report);
        assert!(
            text.contains(
                "✗ function hooks off → add \"CLAUDE_CODE_ENABLE_FUNCTION_HOOKS\": \"1\""
            ),
            "{text}"
        );
        assert!(
            text.contains("✗ installed cctop plugin 0.1.0 has no hooks module"),
            "{text}"
        );
        assert!(
            text.contains("`claude plugin update cctop@cctop`"),
            "{text}"
        );
        assert!(
            text.contains("! the /diff panel was open when last toggled"),
            "{text}"
        );
        assert!(
            text.contains(
                "→ not ready: fix the ✗ lines, restart Claude Code, then run /cctop-pane"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_flag_only_in_settings_asks_for_a_restart() {
        let mut inputs = ready_inputs();
        inputs.env_flag = None;
        inputs.marker = None;
        let report = super::report(&inputs);
        let hooks = report
            .checks
            .iter()
            .find(|c| c.id == "function-hooks")
            .unwrap();
        assert_eq!(hooks.verdict, Verdict::Warn);
        assert_eq!(
            hooks.action.as_deref(),
            Some("restart Claude Code so the env block applies")
        );
        let module = report
            .checks
            .iter()
            .find(|c| c.id == "hooks-module")
            .unwrap();
        assert_eq!(module.verdict, Verdict::Fail);
        assert!(
            module
                .action
                .as_deref()
                .unwrap()
                .starts_with("turn function hooks on"),
            "{module:?}"
        );
    }

    #[test]
    fn a_module_that_ships_but_did_not_load_asks_for_reload_when_hooks_are_on() {
        let mut inputs = ready_inputs();
        inputs.marker = None;
        let report = super::report(&inputs);
        assert!(!report.ready);
        let module = report
            .checks
            .iter()
            .find(|c| c.id == "hooks-module")
            .unwrap();
        assert_eq!(module.verdict, Verdict::Fail);
        assert!(module.text.contains("not loaded in this session"));
        assert!(module
            .action
            .as_deref()
            .unwrap()
            .starts_with("restart Claude Code (or /reload-plugins)"));
        // Without a session nothing can be said about loading.
        inputs.session_id = None;
        let report = super::report(&inputs);
        let module = report
            .checks
            .iter()
            .find(|c| c.id == "hooks-module")
            .unwrap();
        assert_eq!(module.verdict, Verdict::Unknown);
    }

    #[test]
    fn a_marker_from_an_earlier_process_of_a_resumed_session_does_not_count() {
        let mut inputs = ready_inputs();
        // The process started well after the marker's loadedAt.
        inputs.session_started_at = Some(T0 + LOAD_GRACE_MS + 1);
        assert!(!marker_loaded(&inputs));
        assert!(!super::report(&inputs).ready);
        // Within the grace it counts (the registry entry precedes the load).
        inputs.session_started_at = Some(T0 + LOAD_GRACE_MS - 1);
        assert!(marker_loaded(&inputs));
        // Another session's marker never counts.
        inputs.session_id = Some("sess-2".into());
        assert!(!marker_loaded(&inputs));
    }

    #[test]
    fn an_open_pane_reports_where_it_is_and_the_diff_panel_when_hidden() {
        let mut inputs = ready_inputs();
        inputs.marker = Some(marker(true, "visible", 0));
        let report = super::report(&inputs);
        let pane = report.checks.iter().find(|c| c.id == "pane").unwrap();
        assert_eq!(pane.text, "pane open, docked (72 columns)");
        assert!(report.next.starts_with("the cctop pane is open"));

        inputs.marker = Some(marker(true, "hidden", 0));
        let report = super::report(&inputs);
        let diff = report.checks.iter().find(|c| c.id == "diff-panel").unwrap();
        assert_eq!(diff.verdict, Verdict::Warn);
        assert!(diff.text.starts_with("the /diff panel holds the side dock"));
        let pane = report.checks.iter().find(|c| c.id == "pane").unwrap();
        assert_eq!(pane.verdict, Verdict::Warn);
        assert_eq!(
            report.next,
            "run /diff to hide the diff panel; the cctop pane appears in its place"
        );

        // A stale heartbeat means the pane is not open, module still loaded.
        inputs.marker = Some(marker(true, "visible", -MARKER_STALE_AFTER_MS));
        let report = super::report(&inputs);
        assert!(report.ready);
        let pane = report.checks.iter().find(|c| c.id == "pane").unwrap();
        assert_eq!(pane.text, "pane closed");
    }

    #[test]
    fn a_dead_session_is_said_first() {
        let mut inputs = ready_inputs();
        inputs.session_alive = Some(false);
        inputs.session_started_at = None;
        let report = super::report(&inputs);
        assert_eq!(report.checks[0].id, "session");
        assert_eq!(report.checks[0].verdict, Verdict::Warn);
        // Unknown registry: nothing is said either way.
        inputs.session_alive = None;
        assert_eq!(super::report(&inputs).checks[0].id, "claude");
    }

    #[test]
    fn renderer_and_width_verdicts() {
        let mut inputs = ready_inputs();
        inputs.tui = Some(("default".into(), ".claude/settings.local.json".into()));
        inputs.columns = Some(100);
        let report = super::report(&inputs);
        let renderer = report.checks.iter().find(|c| c.id == "renderer").unwrap();
        assert_eq!(renderer.verdict, Verdict::Fail);
        assert!(renderer.text.contains(".claude/settings.local.json"));
        let terminal = report.checks.iter().find(|c| c.id == "terminal").unwrap();
        assert_eq!(terminal.verdict, Verdict::Fail);
        assert_eq!(
            terminal.action.as_deref(),
            Some("widen the terminal to 110+ columns")
        );

        inputs.tui = None;
        inputs.columns = None;
        let report = super::report(&inputs);
        assert_eq!(
            report
                .checks
                .iter()
                .find(|c| c.id == "renderer")
                .unwrap()
                .verdict,
            Verdict::Warn
        );
        assert_eq!(
            report
                .checks
                .iter()
                .find(|c| c.id == "terminal")
                .unwrap()
                .verdict,
            Verdict::Unknown
        );
    }

    #[test]
    fn settings_and_plugin_files_are_read_from_disk() {
        let dir = std::env::temp_dir().join(format!("cctop-pane-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = dir.join("home");
        let cwd = dir.join("project");
        std::fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        std::fs::create_dir_all(cwd.join(".claude")).unwrap();
        std::fs::write(
            home.join(".claude/settings.json"),
            r#"{"env":{"CLAUDE_CODE_ENABLE_FUNCTION_HOOKS":"1"},"tui":"default"}"#,
        )
        .unwrap();
        std::fs::write(cwd.join(".claude/settings.json"), r#"{"tui":"fullscreen"}"#).unwrap();
        assert_eq!(read_settings_flag(&home).as_deref(), Some("1"));
        assert_eq!(
            read_tui(&home, &cwd),
            Some(("fullscreen".into(), ".claude/settings.json".into())),
            "the project file wins over the user file"
        );

        let plugin = dir.join("cache/cctop/0.2.0");
        std::fs::create_dir_all(plugin.join("hooks")).unwrap();
        std::fs::write(
            plugin.join("hooks/hooks.json"),
            r#"{ "modules": ["./pane.tsx"] }"#,
        )
        .unwrap();
        std::fs::write(
            plugin.join("hooks/model.ts"),
            "// the module\nexport const TESTED_WITH = '2.1.273';\nexport const X = 1;\n",
        )
        .unwrap();
        std::fs::write(
            home.join(".claude/plugins/installed_plugins.json"),
            format!(
                r#"{{"version":2,"plugins":{{"cctop@cctop":[{{"scope":"project","installPath":"/elsewhere","version":"0.1.0"}},{{"scope":"user","installPath":"{}","version":"0.2.0"}}]}}}}"#,
                plugin.display()
            ),
        )
        .unwrap();
        assert_eq!(
            read_installed(&home),
            Some(Installed {
                version: "0.2.0".into(),
                install_path: plugin.clone(),
                has_module: true,
                tested_with: Some("2.1.273".into()),
            })
        );
        std::fs::write(plugin.join("hooks/hooks.json"), r#"{ "hooks": {} }"#).unwrap();
        assert!(
            !hooks_module_declared(&plugin),
            "no modules key means skills-only"
        );
        std::fs::write(plugin.join("hooks/model.ts"), "export const OTHER = 1;\n").unwrap();
        assert_eq!(read_tested_with(&plugin), None, "no constant, no version");
        std::fs::remove_file(plugin.join("hooks/model.ts")).unwrap();
        assert_eq!(read_tested_with(&plugin), None, "no file, no version");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
