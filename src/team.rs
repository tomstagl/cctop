//! Agent teams: the teammates' own transcripts, read with the collectors the
//! lead's transcript has (`tasks/prd-cctop-team-costs.md`).
//!
//! A teammate is a full Claude Code session — `<projects>/<slug>/
//! <sessionId>.jsonl` with its own `cost-state` — whose every `user`,
//! `assistant`, `system` and `attachment` line carries `teamName`
//! (`session-<lead id8>`) and `agentName` (`docs/teams.md`). Discovery, in
//! order and merged by name: the team directory's `config.json` while it
//! exists (membership, `cwd`, `isActive`, `joinedAt`; the lead skipped), the
//! lead's `teammate_spawned` results (name, type, model), then a scan of the
//! first lines of every transcript in the directories the team could be in.
//! The directory goes when the team ends and the transcripts stay, so the
//! scan is the source of truth and the other two are decoration.
//!
//! Nothing of a teammate's text is kept: `agentName` and `teamName` are
//! labels, and every line goes through `transcript::Line`, `Aggregate` and
//! `CostTracker` exactly as the lead's do.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};
use serde::Deserialize;

use crate::agents::{FAILED_AFTER_MS, IDLE_MS};
use crate::harness_facts::teams as facts;
use crate::metrics::cost::{parse_ts_ms, Cost, CostTracker, Pricing};
use crate::metrics::Aggregate;
use crate::tail::{parse_file, Tailer};
use crate::tools::AgentSpawn;
use crate::transcript::{AssistantBlock, Line};

/// A member known (from the config or a spawn) for this long with no
/// transcript is reported in `Team::missing`.
pub const MISSING_AFTER_MS: i64 = 60_000;

/// The head scan runs at most this often on the tick (directory events run
/// it at once).
pub const SCAN_INTERVAL: Duration = Duration::from_secs(2);

/// `session-<id8>`: the team a session leads (`harness_facts::teams`).
pub fn team_name(lead_session_id: &str) -> Option<String> {
    let id8: String = lead_session_id.chars().take(facts::LEAD_ID_CHARS).collect();
    (id8.len() == facts::LEAD_ID_CHARS).then(|| format!("{}{id8}", facts::NAME_PREFIX))
}

/// Where a team's facts came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The team directory exists: membership and `isActive` are Claude
    /// Code's own.
    Config,
    /// Found from the transcripts (and the lead's spawns) alone.
    Transcripts,
}

/// Whether a teammate is working, as far as the files say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    /// `isActive: true` in the config.
    Active,
    /// No config verdict; a line within the last `IDLE_MS`.
    Recent,
    /// Its transcript ends with a `cost-state`, or `isActive: false`.
    Ended,
    /// No ledger, no config, no recent line: the team is over (the
    /// directory went) or the teammate stopped without a ledger.
    Gone,
    /// Known from the config or a spawn, no transcript found.
    Missing,
}

impl Liveness {
    /// The glyph of §4.3: `●` working, `○` ended, `—` unknown.
    pub fn glyph(self) -> &'static str {
        match self {
            Liveness::Active | Liveness::Recent => "●",
            Liveness::Ended | Liveness::Gone => "○",
            Liveness::Missing => "—",
        }
    }

    /// The status word of the row when there is one.
    pub fn word(self) -> Option<&'static str> {
        match self {
            Liveness::Active | Liveness::Recent => None,
            Liveness::Ended => Some("ended"),
            Liveness::Gone => Some("gone"),
            Liveness::Missing => Some("no transcript"),
        }
    }

    pub fn is_alive(self) -> bool {
        matches!(self, Liveness::Active | Liveness::Recent)
    }
}

/// Why a teammate's money counts as wasted (§4.4): structural evidence of
/// its own transcript only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WasteReason {
    /// Alive, no API call for `IDLE_MS`, nothing in flight in its
    /// transcript: the cost of its current turn.
    Idle,
    /// Its last turn ended in an API-error line and nothing followed within
    /// `FAILED_AFTER_MS`: the cost of that turn.
    Errored,
}

impl WasteReason {
    pub fn label(self) -> &'static str {
        match self {
            WasteReason::Idle => "idle",
            WasteReason::Errored => "errored",
        }
    }

    pub const ALL: [WasteReason; 2] = [WasteReason::Idle, WasteReason::Errored];
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waste {
    pub usd: f64,
    pub reason: WasteReason,
    /// For the `idle <age>` label.
    pub idle_ms: Option<i64>,
}

/// One member of the team, with its own aggregate and ledger.
#[derive(Debug, Clone)]
pub struct Teammate {
    pub name: String,
    pub agent_type: String,
    /// The model its transcript used, else the spawn's `model`.
    pub model: String,
    pub session_id: Option<String>,
    pub path: Option<PathBuf>,
    pub agg: Aggregate,
    pub cost: CostTracker,
    pub first_line_at: Option<i64>,
    pub last_line_at: Option<i64>,
    /// `isActive` from the config, when the directory exists.
    pub is_active: Option<bool>,
    /// `joinedAt` from the config, else the spawn result's time.
    pub joined_at: Option<i64>,
    /// When cctop first knew of the member (the config, a spawn, or its
    /// transcript's first line), for the `missing` rule.
    pub known_at: Option<i64>,
    /// The last line pushed was its `cost-state`: its own word that it ended.
    ended_by_ledger: bool,
    pending_tool_uses: usize,
    /// The last assistant line was an API error, and when.
    last_api_error_at: Option<i64>,
}

impl Teammate {
    pub fn new(name: &str, pricing: Pricing) -> Teammate {
        Teammate {
            name: name.to_string(),
            agent_type: String::new(),
            model: String::new(),
            session_id: None,
            path: None,
            agg: Aggregate::default(),
            cost: CostTracker::new(pricing),
            first_line_at: None,
            last_line_at: None,
            is_active: None,
            joined_at: None,
            known_at: None,
            ended_by_ledger: false,
            pending_tool_uses: 0,
            last_api_error_at: None,
        }
    }

    /// Feed one line of its transcript.
    pub fn push(&mut self, line: &Line) {
        let at = line.timestamp().and_then(parse_ts_ms);
        if let Some(at) = at {
            self.first_line_at = self.first_line_at.or(Some(at));
            self.last_line_at = Some(self.last_line_at.map_or(at, |m| m.max(at)));
            self.known_at = Some(self.known_at.map_or(at, |k| k.min(at)));
        }
        self.agg.push(line);
        self.cost.push(line);
        match line {
            Line::CostState(_) => self.ended_by_ledger = true,
            Line::Assistant(a) => {
                self.ended_by_ledger = false;
                if a.is_api_error() {
                    self.last_api_error_at = at.or(self.last_api_error_at).or(Some(0));
                } else {
                    self.last_api_error_at = None;
                    self.pending_tool_uses += a
                        .message
                        .content
                        .iter()
                        .filter(|b| matches!(b, AssistantBlock::ToolUse { .. }))
                        .count();
                }
            }
            Line::User(u) => {
                self.ended_by_ledger = false;
                let results = u.message.content.tool_results().count();
                if results > 0 {
                    self.pending_tool_uses = self.pending_tool_uses.saturating_sub(results);
                }
            }
            Line::System(_) => self.ended_by_ledger = false,
            _ => {}
        }
        if let Some(m) = &self.agg.model {
            self.model = m.clone();
        }
    }

    /// Its money: Claude Code's own `cost-state` where one exists, priced
    /// after it (or throughout, while it runs); `None` without a
    /// transcript or a priceable call.
    pub fn cost(&self) -> Option<Cost> {
        self.path.as_ref()?;
        self.cost.current()
    }

    /// Whether it is working, given the clock and the team's source.
    pub fn liveness(&self, now_ms: i64, source: Source) -> Liveness {
        if self.path.is_none() {
            return Liveness::Missing;
        }
        if self.ended_by_ledger {
            return Liveness::Ended;
        }
        if source == Source::Config {
            match self.is_active {
                Some(true) => return Liveness::Active,
                Some(false) => return Liveness::Ended,
                None => {}
            }
        }
        match self.last_line_at {
            Some(at) if now_ms.saturating_sub(at) < IDLE_MS => Liveness::Recent,
            _ => Liveness::Gone,
        }
    }

    /// Time since its first line, to its last when it ended.
    pub fn elapsed_ms(&self, now_ms: i64, alive: Liveness) -> Option<i64> {
        let end = if alive.is_alive() {
            now_ms
        } else {
            self.last_line_at.unwrap_or(now_ms)
        };
        self.first_line_at.map(|s| (end - s).max(0))
    }

    /// Human and machine turns of its transcript (`(human, machine)`).
    pub fn turns(&self) -> (usize, usize) {
        let human = self.agg.human_turns();
        (human, self.agg.turns.len() - human)
    }

    /// The priced cost of its current turn, on the turn's first model.
    fn current_turn_usd(&self) -> f64 {
        let Some(t) = self.agg.current_turn() else {
            return 0.0;
        };
        let model = t
            .models
            .first()
            .map(String::as_str)
            .unwrap_or(self.model.as_str());
        self.cost.pricing().estimate(&t.usage, model).unwrap_or(0.0)
    }

    /// §4.4: `idle` (alive, silent for `IDLE_MS`, nothing in flight) or
    /// `errored` (the last turn ended in an API error and nothing followed
    /// for `FAILED_AFTER_MS`); the amount is the current turn's cost.
    pub fn waste(&self, now_ms: i64, alive: Liveness) -> Option<Waste> {
        if let Some(err_at) = self.last_api_error_at {
            let settled = now_ms.saturating_sub(err_at) >= FAILED_AFTER_MS;
            if settled && self.agg.api_calls() > 0 {
                return Some(Waste {
                    usd: self.current_turn_usd(),
                    reason: WasteReason::Errored,
                    idle_ms: None,
                });
            }
        }
        if !alive.is_alive() || self.pending_tool_uses > 0 {
            return None;
        }
        let last_api = self.agg.last_api_at.as_deref().and_then(parse_ts_ms)?;
        let age = now_ms.saturating_sub(last_api);
        (age >= IDLE_MS).then(|| Waste {
            usd: self.current_turn_usd(),
            reason: WasteReason::Idle,
            idle_ms: Some(age),
        })
    }
}

/// `~/.claude/teams/<team>/config.json` — the keys cctop reads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub lead_session_id: String,
    #[serde(default)]
    pub members: Vec<Member>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub agent_type: String,
    #[serde(default)]
    pub backend_type: String,
    #[serde(default)]
    pub model: String,
    pub cwd: Option<String>,
    pub joined_at: Option<i64>,
    pub is_active: Option<bool>,
}

impl Config {
    pub fn read(path: &Path) -> Option<Config> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    /// The members other than the lead; empty for a solo session's
    /// one-member directory.
    pub fn teammates(&self) -> impl Iterator<Item = &Member> {
        self.members
            .iter()
            .filter(|m| m.agent_type != "team-lead" && m.backend_type != "in-process")
    }
}

/// The team the attached session leads.
#[derive(Debug, Clone)]
pub struct Team {
    /// `session-<lead id8>`.
    pub name: String,
    pub source: Source,
    /// By member name.
    pub members: BTreeMap<String, Teammate>,
    /// Directories the head scan looked in for transcripts.
    pub looked_in: Vec<PathBuf>,
    /// The team directory existed at some point of this watch.
    pub dir_seen: bool,
}

impl Team {
    /// Members with a transcript.
    pub fn read(&self) -> usize {
        self.members.values().filter(|m| m.path.is_some()).count()
    }

    /// Names known for more than `MISSING_AFTER_MS` (at once when the lead
    /// is not alive) with no transcript.
    pub fn missing(&self, now_ms: i64, lead_alive: bool) -> Vec<String> {
        self.members
            .values()
            .filter(|m| m.path.is_none())
            .filter(|m| {
                !lead_alive
                    || m.known_at
                        .is_none_or(|k| now_ms.saturating_sub(k) >= MISSING_AFTER_MS)
            })
            .map(|m| m.name.clone())
            .collect()
    }

    /// The team's money: Σ the members' own figures (§4.1), marked `≈`
    /// when any member is priced or still working, or when a transcript is
    /// missing; `None` when nothing could be priced.
    pub fn cost(&self, now_ms: i64) -> Option<Cost> {
        let mut sum: Option<Cost> = None;
        let mut approx = false;
        for m in self.members.values() {
            match m.cost() {
                Some(c) => {
                    approx |= c.approx || m.liveness(now_ms, self.source).is_alive();
                    sum = Some(sum.map_or(c, |s| s.plus(c)));
                }
                None if m.path.is_none() => approx = true,
                None => {}
            }
        }
        sum.map(|c| Cost {
            approx: c.approx || approx,
            ..c
        })
    }

    /// Σ waste over the members.
    pub fn waste_usd(&self, now_ms: i64) -> f64 {
        self.members
            .values()
            .filter_map(|m| m.waste(now_ms, m.liveness(now_ms, self.source)))
            .map(|w| w.usd)
            .sum()
    }

    pub fn active(&self, now_ms: i64) -> usize {
        self.members
            .values()
            .filter(|m| m.liveness(now_ms, self.source).is_alive())
            .count()
    }

    fn member(&mut self, name: &str, pricing: &Pricing) -> &mut Teammate {
        self.members
            .entry(name.to_string())
            .or_insert_with(|| Teammate::new(name, pricing.clone()))
    }

    /// (1) the config: membership, `cwd`, `isActive`, `joinedAt`.
    pub fn apply_config(&mut self, cfg: &Config, pricing: &Pricing, now_ms: i64) {
        self.source = Source::Config;
        self.dir_seen = true;
        for m in cfg.teammates() {
            let t = self.member(&m.name, pricing);
            if t.agent_type.is_empty() {
                t.agent_type = m.agent_type.clone();
            }
            if t.model.is_empty() {
                t.model = m.model.clone();
            }
            t.is_active = m.is_active;
            t.joined_at = m.joined_at.or(t.joined_at);
            let known = m.joined_at.unwrap_or(now_ms);
            t.known_at = Some(t.known_at.map_or(known, |k| k.min(known)));
        }
    }

    /// (2) the lead's `teammate_spawned` results: name, type, model.
    pub fn apply_spawns<'a>(
        &mut self,
        spawns: impl IntoIterator<Item = &'a AgentSpawn>,
        pricing: &Pricing,
        now_ms: i64,
    ) {
        for s in spawns {
            let Some(name) = s.name.as_deref() else {
                continue;
            };
            if s.team_name.as_deref().is_some_and(|t| t != self.name) {
                continue;
            }
            let t = self.member(name, pricing);
            if t.agent_type.is_empty() {
                t.agent_type = s.agent_type.clone().unwrap_or_default();
            }
            if t.model.is_empty() {
                t.model = s.resolved_model.clone().unwrap_or_default();
            }
            t.joined_at = t.joined_at.or(s.at);
            let known = s.at.unwrap_or(now_ms);
            t.known_at = Some(t.known_at.map_or(known, |k| k.min(known)));
        }
    }

    /// The directory went while the lead runs on: every live member is
    /// `Gone` until its lines say otherwise.
    pub fn note_dir_gone(&mut self) {
        if self.source == Source::Config {
            self.source = Source::Transcripts;
            for m in self.members.values_mut() {
                m.is_active = None;
            }
        }
    }
}

/// What the head of a transcript says: `(teamName, agentName, sessionId)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub team: String,
    pub agent: String,
    pub session_id: Option<String>,
    /// Lines read; fewer than `HEAD_SCAN_LINES` with no key means the file
    /// may still be too short to judge.
    pub lines: usize,
}

/// Read the first `HEAD_SCAN_LINES` lines of `path` for the team keys.
/// `Err(lines)` when none carries them.
pub fn read_head(path: &Path) -> Result<Head, usize> {
    use std::io::BufRead;
    let Ok(f) = std::fs::File::open(path) else {
        return Err(0);
    };
    let mut n = 0;
    for line in std::io::BufReader::new(f)
        .lines()
        .take(facts::HEAD_SCAN_LINES)
    {
        let Ok(line) = line else {
            break;
        };
        n += 1;
        let Ok(parsed) = Line::parse(line.trim()) else {
            continue;
        };
        if let Some((team, agent)) = parsed.team() {
            return Ok(Head {
                team: team.to_string(),
                agent: agent.to_string(),
                session_id: parsed.session_id().map(str::to_string),
                lines: n,
            });
        }
    }
    Err(n)
}

/// Where a team's files are: the config to read, the directories to scan,
/// and the directory whose events mean the team appeared or went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub config: Option<PathBuf>,
    pub scan_dirs: Vec<PathBuf>,
    /// `~/.claude/teams` for a live session (the team directory is created
    /// and removed under it); nothing for a fixture, whose team file does
    /// not change.
    pub watch: Option<PathBuf>,
}

impl Layout {
    /// For a live session: `~/.claude/teams/<team>/config.json` and the
    /// lead's own project directory (plus each config member's `cwd`
    /// slug under `projects`, added by the scan when the config is read).
    pub fn live(lead_transcript: &Path, team: Option<&str>, teams_dir: Option<&Path>) -> Layout {
        Layout {
            config: teams_dir
                .zip(team)
                .map(|(d, team)| d.join(team).join("config.json")),
            scan_dirs: lead_transcript
                .parent()
                .map(Path::to_path_buf)
                .into_iter()
                .collect(),
            watch: teams_dir.map(Path::to_path_buf),
        }
    }

    /// For a fixture: `<stem>.team.json` beside the file and
    /// `<stem>/teammates/` — the way A's `subagents/` is found.
    pub fn fixture(lead_transcript: &Path) -> Layout {
        let stem = lead_transcript.with_extension("");
        Layout {
            config: Some(stem.with_extension("team.json")),
            scan_dirs: vec![stem.join("teammates")],
            watch: None,
        }
    }

    /// The fixture layout when `<stem>/teammates/` or `<stem>.team.json`
    /// exists beside the transcript, else the live one.
    pub fn for_transcript(
        lead_transcript: &Path,
        team: Option<&str>,
        teams_dir: Option<&Path>,
    ) -> Layout {
        let fx = Layout::fixture(lead_transcript);
        if fx.scan_dirs[0].is_dir() || fx.config.as_ref().is_some_and(|c| c.is_file()) {
            fx
        } else {
            Layout::live(lead_transcript, team, teams_dir)
        }
    }

    /// The scan directories plus the config members' project slugs.
    fn scan_dirs_with(&self, cfg: Option<&Config>, projects: Option<&Path>) -> Vec<PathBuf> {
        let mut dirs = self.scan_dirs.clone();
        if let (Some(cfg), Some(projects)) = (cfg, projects) {
            for m in cfg.teammates() {
                if let Some(cwd) = &m.cwd {
                    let d = projects.join(crate::slug(Path::new(cwd)));
                    if !dirs.contains(&d) {
                        dirs.push(d);
                    }
                }
            }
        }
        dirs
    }
}

/// The lead as the collector needs it.
#[derive(Debug, Clone)]
pub struct Lead {
    pub session_id: String,
    pub transcript: PathBuf,
    /// The lead's first line: a transcript older than this is not a
    /// teammate's.
    pub started_at_ms: Option<i64>,
    /// The lead's process is running. A finished session's team can only
    /// be found from the transcripts (its directory went), so the watcher
    /// scans once for it even with nothing else known.
    pub alive: bool,
}

/// Every `.jsonl` in `dir`, newest first, other than the lead's own.
fn candidates(dir: &Path, lead: &Lead) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, u64)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl") && p != &lead.transcript)
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as i64)
                .unwrap_or(i64::MAX);
            // A file last written before the lead started cannot be its
            // teammate's; a fixture's files are checked out later than
            // their lines, so this never excludes one.
            (lead.started_at_ms.is_none_or(|s| modified >= s)).then_some((p, meta.len()))
        })
        .collect();
    out.sort();
    out
}

/// Build the team once for `cctop query`, the fixtures and the TUI's
/// start-up: every transcript read whole. `None` when the session leads no
/// team (no config with a second member, no spawn, no transcript naming
/// it).
pub fn load(
    lead: &Lead,
    layout: &Layout,
    spawns: &[AgentSpawn],
    projects: Option<&Path>,
    pricing: &Pricing,
    now_ms: i64,
) -> Option<Team> {
    let name = team_name(&lead.session_id)?;
    let mut team = Team {
        name: name.clone(),
        source: Source::Transcripts,
        members: BTreeMap::new(),
        looked_in: Vec::new(),
        dir_seen: false,
    };
    let cfg = layout.config.as_deref().and_then(Config::read);
    if let Some(cfg) = &cfg {
        if cfg.name == name || cfg.lead_session_id == lead.session_id || cfg.name.is_empty() {
            team.apply_config(cfg, pricing, now_ms);
        }
    }
    team.apply_spawns(spawns, pricing, now_ms);
    // The one-shot read always scans (FR-7: a finished session's team is
    // in the transcripts alone); the files older than the lead's first
    // line are skipped by `candidates`, and the heads are ten lines each.
    for dir in layout.scan_dirs_with(cfg.as_ref(), projects) {
        if !dir.is_dir() {
            continue;
        }
        team.looked_in.push(dir.clone());
        for (path, _) in candidates(&dir, lead) {
            let Ok(head) = read_head(&path) else {
                continue;
            };
            if head.team != name {
                continue;
            }
            let t = team.member(&head.agent, pricing);
            t.session_id = head.session_id.clone();
            t.path = Some(path.clone());
            for line in parse_file(&path).unwrap_or_default() {
                t.push(&line);
            }
        }
    }
    (!team.members.is_empty()).then_some(team)
}

/// Live view of a team: a notify watch on the team directory (membership,
/// `isActive`, its disappearance) and on every scanned project directory
/// (new transcripts), one `Tailer` per member. A sibling of
/// `agents::AgentWatcher`; call [`TeamWatcher::poll`] from the render loop.
pub struct TeamWatcher {
    lead: Lead,
    layout: Layout,
    projects: Option<PathBuf>,
    pricing: Pricing,
    name: Option<String>,
    tailers: HashMap<String, Tailer>,
    /// Files scanned and found to carry no team key, with the lines read:
    /// re-read only while shorter than the head.
    rejected: HashMap<PathBuf, usize>,
    /// Files accepted, by member.
    accepted: HashMap<PathBuf, String>,
    watched: Vec<PathBuf>,
    /// Created on the first directory watched: a session that leads no
    /// team and has no directory to watch costs no watcher at all.
    watcher: Option<notify::RecommendedWatcher>,
    event_tx: std_mpsc::Sender<()>,
    events: std_mpsc::Receiver<()>,
    last_scan: Option<Instant>,
    /// The config as last read; `None` once the directory went.
    config_present: bool,
    last_config: Option<Instant>,
    cfg: Option<Config>,
    pub stats: WatchStats,
}

/// What a poll cost, for the budget test.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WatchStats {
    /// `read_dir` calls (the team directory and the scan directories).
    pub dir_reads: usize,
    /// Transcript heads opened.
    pub head_reads: usize,
    /// Config files read.
    pub config_reads: usize,
    /// Lines the tailers delivered.
    pub lines: usize,
}

impl TeamWatcher {
    pub fn watch(lead: Lead, layout: Layout, projects: Option<PathBuf>, pricing: Pricing) -> Self {
        let (tx, rx) = std_mpsc::channel();
        let name = team_name(&lead.session_id);
        let mut w = TeamWatcher {
            lead,
            layout,
            projects,
            pricing,
            name,
            tailers: HashMap::new(),
            rejected: HashMap::new(),
            accepted: HashMap::new(),
            watched: Vec::new(),
            watcher: None,
            event_tx: tx,
            events: rx,
            last_scan: None,
            config_present: false,
            last_config: None,
            cfg: None,
            stats: WatchStats::default(),
        };
        // `~/.claude/teams`, so the team directory's creation and removal
        // are seen (the directory itself is watched once it exists).
        if let Some(dir) = w.layout.watch.clone() {
            w.watch_dir(&dir);
        }
        w
    }

    fn watch_dir(&mut self, dir: &Path) {
        if self.watched.iter().any(|d| d == dir) || !dir.is_dir() {
            return;
        }
        if self.watcher.is_none() {
            let tx = self.event_tx.clone();
            self.watcher =
                notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                    // inotify reports every open and close of a watched
                    // directory's files (the tailers' own reads): only a
                    // file appearing, going or being rewritten is news.
                    if res.is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_))) {
                        let _ = tx.send(());
                    }
                })
                .ok();
        }
        // Non-recursive: a config rewrite, a new transcript and the team
        // directory's coming and going all happen at the watched level.
        if let Some(w) = self.watcher.as_mut() {
            if w.watch(dir, RecursiveMode::NonRecursive).is_ok() {
                self.watched.push(dir.to_path_buf());
            }
        }
    }

    /// The config, when the directory is there.
    fn read_config(&mut self, team: &mut Option<Team>, now_ms: i64) -> Option<Config> {
        let path = self.layout.config.clone()?;
        let name = self.name.clone()?;
        self.stats.config_reads += 1;
        match Config::read(&path) {
            Some(cfg)
                if cfg.name == name
                    || cfg.lead_session_id == self.lead.session_id
                    || cfg.name.is_empty() =>
            {
                self.config_present = true;
                if cfg.teammates().next().is_some() {
                    let t = team.get_or_insert_with(|| Team {
                        name: name.clone(),
                        source: Source::Config,
                        members: BTreeMap::new(),
                        looked_in: Vec::new(),
                        dir_seen: true,
                    });
                    t.apply_config(&cfg, &self.pricing, now_ms);
                    // A live team's directory: membership and liveness. A
                    // fixture's team file (no `watch`) never changes.
                    if let (Some(dir), true) = (path.parent(), self.layout.watch.is_some()) {
                        self.watch_dir(dir);
                    }
                }
                Some(cfg)
            }
            _ => {
                if self.config_present {
                    self.config_present = false;
                    if let Some(t) = team.as_mut() {
                        t.note_dir_gone();
                    }
                }
                None
            }
        }
    }

    /// The head scan over the scan directories.
    fn scan(&mut self, team: &mut Option<Team>, cfg: Option<&Config>) {
        let Some(name) = self.name.clone() else {
            return;
        };
        let dirs = self.layout.scan_dirs_with(cfg, self.projects.as_deref());
        for dir in dirs {
            if !dir.is_dir() {
                continue;
            }
            self.watch_dir(&dir);
            self.stats.dir_reads += 1;
            if let Some(t) = team.as_mut() {
                if !t.looked_in.contains(&dir) {
                    t.looked_in.push(dir.clone());
                }
            }
            for (path, _) in candidates(&dir, &self.lead) {
                if self.accepted.contains_key(&path) {
                    continue;
                }
                if self
                    .rejected
                    .get(&path)
                    .is_some_and(|n| *n >= facts::HEAD_SCAN_LINES)
                {
                    continue;
                }
                self.stats.head_reads += 1;
                match read_head(&path) {
                    Ok(head) if head.team == name => {
                        let t = team.get_or_insert_with(|| Team {
                            name: name.clone(),
                            source: Source::Transcripts,
                            members: BTreeMap::new(),
                            looked_in: vec![dir.clone()],
                            dir_seen: false,
                        });
                        let m = t.member(&head.agent, &self.pricing);
                        // A second file for the same member (`/clear`
                        // rotated its session): follow the newer one.
                        if let Some(old) = m.path.clone() {
                            if old == path {
                                continue;
                            }
                            self.tailers.remove(&head.agent);
                            self.accepted.remove(&old);
                        }
                        m.session_id = head.session_id.clone();
                        m.path = Some(path.clone());
                        if let Ok(tailer) = Tailer::open(&path) {
                            self.tailers.insert(head.agent.clone(), tailer);
                        }
                        self.accepted.insert(path, head.agent);
                    }
                    Ok(head) => {
                        self.rejected
                            .insert(path, head.lines.max(facts::HEAD_SCAN_LINES));
                    }
                    Err(n) => {
                        self.rejected.insert(path, n);
                    }
                }
            }
        }
    }

    /// Drain events and tailers; returns true when the team changed. The
    /// config (one file) is read on a directory event and otherwise at
    /// most every `SCAN_INTERVAL`; the head scan runs on the same cadence
    /// and only once a team is known.
    pub fn poll(&mut self, team: &mut Option<Team>, spawns: &[AgentSpawn], now_ms: i64) -> bool {
        let mut changed = false;
        let members_before = team.as_ref().map_or(0, |t| t.members.len());
        let event = self.events.try_recv().is_ok();
        while self.events.try_recv().is_ok() {}
        if event
            || self
                .last_config
                .is_none_or(|t| t.elapsed() >= SCAN_INTERVAL)
        {
            self.last_config = Some(Instant::now());
            self.cfg = self.read_config(team, now_ms);
        }
        let cfg = self.cfg.clone();
        if let Some(name) = self.name.clone() {
            if spawns.iter().any(|s| s.name.is_some()) {
                let t = team.get_or_insert_with(|| Team {
                    name,
                    source: Source::Transcripts,
                    members: BTreeMap::new(),
                    looked_in: Vec::new(),
                    dir_seen: false,
                });
                t.apply_spawns(spawns, &self.pricing, now_ms);
            }
        }
        // FR-3: the head scan only once a team is known — from the config,
        // a spawn, or a fixture's `teammates/` directory — and once, at the
        // first poll, for a finished session (its directory is gone).
        let known = team.as_ref().is_some_and(|t| !t.members.is_empty())
            || self
                .layout
                .scan_dirs
                .first()
                .is_some_and(|d| d.file_name().is_some_and(|n| n == "teammates") && d.is_dir())
            || (!self.lead.alive && self.last_scan.is_none());
        let due = self.last_scan.is_none_or(|t| t.elapsed() >= SCAN_INTERVAL);
        if known && (event || due) {
            self.last_scan = Some(Instant::now());
            self.scan(team, cfg.as_ref());
        }
        if let Some(t) = team.as_mut() {
            for (name, tailer) in self.tailers.iter_mut() {
                let Some(m) = t.members.get_mut(name) else {
                    continue;
                };
                while let Some(line) = tailer.try_recv() {
                    m.push(&line);
                    self.stats.lines += 1;
                    changed = true;
                }
            }
            if t.members.is_empty() {
                *team = None;
            }
        }
        changed || event || team.as_ref().map_or(0, |t| t.members.len()) != members_before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("fixtures/{name}.jsonl"))
    }

    const LEAD: &str = "afd065d3-054a-5984-b05e-c46a3fa0a9ca";

    fn lead_d() -> (Lead, Vec<AgentSpawn>, i64) {
        let path = fixture("session-d");
        let lines = parse_file(&path).unwrap();
        let stats = crate::tools::Stats::from_lines(&lines);
        let first = lines
            .iter()
            .find_map(|l| l.timestamp())
            .and_then(parse_ts_ms);
        let last = lines
            .iter()
            .filter_map(|l| l.timestamp())
            .filter_map(parse_ts_ms)
            .max()
            .unwrap();
        (
            Lead {
                session_id: LEAD.into(),
                transcript: path,
                started_at_ms: first,
                alive: false,
            },
            stats.agent_spawns,
            last,
        )
    }

    #[test]
    fn team_name_needs_eight_characters() {
        assert_eq!(team_name(LEAD).as_deref(), Some("session-afd065d3"));
        assert_eq!(team_name(""), None);
        assert_eq!(team_name("abc"), None);
    }

    #[test]
    fn fixture_d_from_the_transcripts_alone() {
        // FR-7: no team directory, no team file — the spawns name three
        // members, the head scan finds two transcripts.
        let (lead, spawns, now) = lead_d();
        let layout = Layout {
            config: None,
            scan_dirs: vec![lead.transcript.with_extension("").join("teammates")],
            watch: None,
        };
        let team = load(&lead, &layout, &spawns, None, &Pricing::bundled(), now).unwrap();
        assert_eq!(team.name, "session-afd065d3");
        assert_eq!(team.source, Source::Transcripts);
        assert_eq!(team.members.len(), 3);
        assert_eq!(team.read(), 2);
        assert_eq!(team.missing(now, false), ["diff-pane-research-3"]);
        assert_eq!(team.looked_in.len(), 1);
        assert!(team.looked_in[0].ends_with("session-d/teammates"));

        let real = &team.members["diff-pane-research"];
        assert_eq!(real.agent_type, "claude-code-guide");
        assert_eq!(
            real.model, "claude-haiku-4-5-20251001",
            "the transcript's model, not the spawn's alias"
        );
        assert_eq!(
            real.session_id.as_deref(),
            Some("85424f6c-500e-5993-641b-0a66d897a888")
        );
        let c = real.cost().unwrap();
        assert!(
            !c.approx,
            "it ended with a cost-state: Claude Code's number"
        );
        assert!((c.usd - 0.3149226).abs() < 1e-6, "{}", c.usd);
        assert_eq!(real.liveness(now, team.source), Liveness::Ended);
        assert_eq!(
            real.turns(),
            (0, 1),
            "one teammate message started its one turn; the other 12 user lines are tool results"
        );
        assert!(real.agg.context_size() > 0);
        assert!(real.waste(now, Liveness::Ended).is_none());

        let priced = &team.members["diff-pane-research-2"];
        let c = priced.cost().unwrap();
        assert!(c.approx, "cut before its cost-state: priced");
        assert!(c.usd > 0.0);
        assert_eq!(
            priced.liveness(now, team.source),
            Liveness::Recent,
            "its lines run past the lead's last one"
        );

        let missing = &team.members["diff-pane-research-3"];
        assert!(missing.path.is_none());
        assert_eq!(missing.cost(), None);
        assert_eq!(missing.liveness(now, team.source), Liveness::Missing);
        assert_eq!(missing.agent_type, "claude-code-guide");
        assert_eq!(missing.model, "haiku", "the spawn's word, nothing better");

        let total = team.cost(now).unwrap();
        assert!(total.approx, "a running member and a missing one");
        assert!((total.usd - (0.3149226 + priced.cost().unwrap().usd)).abs() < 1e-9);
        assert_eq!(team.active(now), 1);
    }

    #[test]
    fn fixture_d_with_its_team_file() {
        // The config path: membership and `isActive` from the team file.
        let (lead, spawns, now) = lead_d();
        let layout = Layout::for_transcript(&lead.transcript, Some("session-afd065d3"), None);
        assert!(layout
            .config
            .as_ref()
            .unwrap()
            .ends_with("session-d.team.json"));
        let team = load(&lead, &layout, &spawns, None, &Pricing::bundled(), now).unwrap();
        assert_eq!(team.source, Source::Config);
        assert_eq!(team.members.len(), 3, "the lead is skipped");
        assert_eq!(
            team.members["diff-pane-research"].liveness(now, team.source),
            Liveness::Ended,
            "isActive: false"
        );
        assert_eq!(
            team.members["diff-pane-research-2"].liveness(now, team.source),
            Liveness::Active,
            "isActive: true"
        );
        assert_eq!(
            team.members["diff-pane-research-3"].liveness(now, team.source),
            Liveness::Missing
        );
        assert!(team.members["diff-pane-research"].joined_at.is_some());
        // Without spawns the config alone still names every member.
        let team = load(&lead, &layout, &[], None, &Pricing::bundled(), now).unwrap();
        assert_eq!(team.members.len(), 3);
        assert_eq!(team.read(), 2);
    }

    #[test]
    fn a_session_without_a_team_has_none() {
        // Fixtures A, B and C: no spawn, no `teammates/`, no team file.
        for name in ["session-a", "session-b", "session-c"] {
            let path = fixture(name);
            let lines = parse_file(&path).unwrap();
            let spawns = crate::tools::Stats::from_lines(&lines).agent_spawns;
            let lead = Lead {
                session_id: "b34fc081-f0e9-48ae-9c1a-552587db6403".into(),
                transcript: path.clone(),
                started_at_ms: None,
                alive: false,
            };
            let layout = Layout::for_transcript(&path, Some("session-b34fc081"), None);
            assert_eq!(
                layout,
                Layout::live(&path, Some("session-b34fc081"), None),
                "{name}: nothing beside the file"
            );
            assert!(load(&lead, &layout, &spawns, None, &Pricing::bundled(), 0).is_none());
        }
        // A one-member directory (a solo session) is not a team.
        let dir = std::env::temp_dir().join(format!("cctop-team-solo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("teams/session-deadbeef")).unwrap();
        std::fs::write(
            dir.join("teams/session-deadbeef/config.json"),
            r#"{"name":"session-deadbeef","leadSessionId":"deadbeef-1111","members":[{"agentId":"team-lead@session-deadbeef","name":"team-lead","agentType":"team-lead","backendType":"in-process"}]}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("projects/-x")).unwrap();
        let transcript = dir.join("projects/-x/deadbeef-1111.jsonl");
        std::fs::write(&transcript, "{}\n").unwrap();
        let lead = Lead {
            session_id: "deadbeef-1111".into(),
            transcript: transcript.clone(),
            started_at_ms: None,
            alive: true,
        };
        let layout = Layout::live(
            &transcript,
            Some("session-deadbeef"),
            Some(&dir.join("teams")),
        );
        assert!(load(&lead, &layout, &[], None, &Pricing::bundled(), 0).is_none());
    }

    #[test]
    fn read_head_finds_the_keys_on_line_four_and_rejects_the_lead() {
        let dir = fixture("session-d").with_extension("").join("teammates");
        let head = read_head(&dir.join("85424f6c-500e-5993-641b-0a66d897a888.jsonl")).unwrap();
        assert_eq!(head.team, "session-afd065d3");
        assert_eq!(head.agent, "diff-pane-research");
        assert_eq!(head.lines, 4);
        assert_eq!(
            head.session_id.as_deref(),
            Some("85424f6c-500e-5993-641b-0a66d897a888")
        );
        assert_eq!(
            read_head(&fixture("session-d")),
            Err(facts::HEAD_SCAN_LINES)
        );
        assert_eq!(read_head(Path::new("/nonexistent.jsonl")), Err(0));
    }

    #[test]
    fn liveness_and_waste_on_synthetic_lines() {
        let asst = |id: &str, ts: &str, block: &str, err: bool| -> Line {
            let err = if err {
                r#""isApiErrorMessage":true,"#
            } else {
                ""
            };
            Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"{ts}",{err}"teamName":"session-x","agentName":"t","message":{{"id":"{id}","model":"claude-opus-5","content":[{block}],"stop_reason":"end_turn","usage":{{"cache_read_input_tokens":50000,"output_tokens":1000}}}}}}"#
            ))
            .unwrap()
        };
        let text = r#"{"type":"text","text":"done"}"#;
        let tool = r#"{"type":"tool_use","id":"t1","name":"Bash","input":{}}"#;
        let prompt = |ts: &str| -> Line {
            Line::parse(&format!(
                r#"{{"type":"user","timestamp":"{ts}","teamName":"session-x","agentName":"t","message":{{"role":"user","content":"<teammate-message from=\"lead\">go</teammate-message>"}}}}"#
            ))
            .unwrap()
        };
        let t0 = parse_ts_ms("2026-01-01T00:00:00Z").unwrap();
        let mut m = Teammate::new("t", Pricing::bundled());
        m.path = Some(PathBuf::from("/x.jsonl"));
        m.push(&prompt("2026-01-01T00:00:00Z"));
        m.push(&asst("m1", "2026-01-01T00:00:05Z", text, false));
        assert_eq!(
            m.liveness(t0 + 10_000, Source::Transcripts),
            Liveness::Recent
        );
        assert_eq!(m.waste(t0 + 10_000, Liveness::Recent), None);
        // Silent for five minutes with nothing in flight: idle, the turn's cost.
        let w = m.waste(t0 + IDLE_MS + 5_000, Liveness::Recent).unwrap();
        assert_eq!(w.reason, WasteReason::Idle);
        assert_eq!(w.idle_ms, Some(IDLE_MS));
        assert!(w.usd > 0.0);
        // But five minutes of silence without config is `Gone`, not alive.
        assert_eq!(
            m.liveness(t0 + IDLE_MS + 5_000, Source::Transcripts),
            Liveness::Gone
        );
        // With `isActive: true` it stays alive and the idle reason holds.
        m.is_active = Some(true);
        let alive = m.liveness(t0 + IDLE_MS + 5_000, Source::Config);
        assert_eq!(alive, Liveness::Active);
        assert!(m.waste(t0 + IDLE_MS + 5_000, alive).is_some());
        // A tool call in flight is not idle.
        m.push(&asst("m2", "2026-01-01T00:00:06Z", tool, false));
        assert_eq!(m.waste(t0 + IDLE_MS + 10_000, Liveness::Active), None);
        // An API error that nothing followed for a minute: errored.
        m.push(&prompt("2026-01-01T00:01:00Z"));
        m.push(&asst("err", "2026-01-01T00:01:01Z", text, true));
        let at = parse_ts_ms("2026-01-01T00:01:01Z").unwrap();
        assert_eq!(m.waste(at + 1_000, Liveness::Active), None);
        let w = m.waste(at + FAILED_AFTER_MS, Liveness::Active).unwrap();
        assert_eq!(w.reason, WasteReason::Errored);
        // A response after it clears the error.
        m.push(&asst("m3", "2026-01-01T00:02:00Z", text, false));
        assert!(m
            .waste(at + FAILED_AFTER_MS, Liveness::Active)
            .is_none_or(|w| w.reason != WasteReason::Errored));
        // Its own cost-state at the end of the file: ended, whatever isActive says.
        m.push(&Line::parse(r#"{"type":"cost-state","totalCostUSD":1.5}"#).unwrap());
        assert_eq!(m.liveness(t0, Source::Config), Liveness::Ended);
        let c = m.cost().unwrap();
        assert!(!c.approx);
        assert_eq!(c.usd, 1.5);
        // Nothing found on disk: missing, no money.
        let mut none = Teammate::new("n", Pricing::bundled());
        assert_eq!(none.liveness(t0, Source::Config), Liveness::Missing);
        none.known_at = Some(t0);
        let team = Team {
            name: "session-x".into(),
            source: Source::Config,
            members: [("n".to_string(), none)].into_iter().collect(),
            looked_in: Vec::new(),
            dir_seen: true,
        };
        assert!(
            team.missing(t0 + 1_000, true).is_empty(),
            "known for a second"
        );
        assert_eq!(team.missing(t0 + MISSING_AFTER_MS, true), ["n"]);
        assert_eq!(team.missing(t0, false), ["n"], "a finished lead: at once");
        assert_eq!(
            team.cost(t0),
            None,
            "nothing priced, but never a silent zero"
        );
    }

    #[tokio::test]
    async fn watcher_and_load_agree_on_fixture_d() {
        let (lead, spawns, now) = lead_d();
        let layout = Layout::for_transcript(&lead.transcript, Some("session-afd065d3"), None);
        let loaded = load(&lead, &layout, &spawns, None, &Pricing::bundled(), now).unwrap();
        let mut w = TeamWatcher::watch(lead.clone(), layout, None, Pricing::bundled());
        let mut team = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            w.poll(&mut team, &spawns, now);
            let done = team.as_ref().is_some_and(|t: &Team| {
                t.members.len() == 3
                    && t.members
                        .values()
                        .filter(|m| m.path.is_some())
                        .all(|m| m.agg.api_calls() == loaded.members[&m.name].agg.api_calls())
            });
            if done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let team = team.expect("the watcher found the team");
        assert_eq!(team.source, Source::Config);
        assert_eq!(team.read(), 2);
        assert_eq!(team.looked_in, loaded.looked_in);
        for (name, l) in &loaded.members {
            let p = &team.members[name];
            assert_eq!(l.path, p.path, "{name}");
            assert_eq!(l.session_id, p.session_id);
            assert_eq!(l.agent_type, p.agent_type);
            assert_eq!(l.model, p.model);
            assert_eq!(l.agg.api_calls(), p.agg.api_calls());
            assert_eq!(l.agg.total, p.agg.total);
            assert_eq!(l.cost(), p.cost());
            assert_eq!(l.first_line_at, p.first_line_at);
            assert_eq!(l.last_line_at, p.last_line_at);
            assert_eq!(l.is_active, p.is_active);
            assert_eq!(l.liveness(now, team.source), p.liveness(now, team.source));
            assert_eq!(l.turns(), p.turns());
        }
        assert_eq!(loaded.cost(now), team.cost(now));
        // A quiet poll inside the interval reads nothing at all.
        w.stats = WatchStats::default();
        w.last_scan = Some(Instant::now());
        w.last_config = Some(Instant::now());
        assert!(!w.poll(&mut Some(team), &spawns, now));
        assert_eq!(w.stats, WatchStats::default());
    }

    #[tokio::test]
    async fn watcher_finds_a_team_that_appears_and_notes_the_directory_going() {
        let dir = std::env::temp_dir().join(format!("cctop-team-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let projects = dir.join("projects/-Users-me-code-one");
        let teams = dir.join("teams");
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::create_dir_all(&teams).unwrap();
        let lead_path = projects.join("deadbeef-1111-4000-8000-000000000000.jsonl");
        std::fs::write(&lead_path, "{\"type\":\"mode\"}\n").unwrap();
        let lead = Lead {
            session_id: "deadbeef-1111-4000-8000-000000000000".into(),
            transcript: lead_path.clone(),
            started_at_ms: None,
            alive: true,
        };
        let layout = Layout::live(&lead_path, Some("session-deadbeef"), Some(&teams));
        let mut w =
            TeamWatcher::watch(lead, layout, Some(dir.join("projects")), Pricing::bundled());
        let mut team = None;
        assert!(!w.poll(&mut team, &[], 0));
        assert!(team.is_none(), "no team: no scan either");
        assert_eq!(w.stats.head_reads, 0);
        // The team directory appears with one teammate.
        std::fs::create_dir_all(teams.join("session-deadbeef")).unwrap();
        std::fs::write(
            teams.join("session-deadbeef/config.json"),
            r#"{"name":"session-deadbeef","leadSessionId":"deadbeef-1111-4000-8000-000000000000","members":[{"name":"team-lead","agentType":"team-lead","backendType":"in-process"},{"name":"worker","agentType":"general-purpose","backendType":"tmux","model":"opus","cwd":"/Users/me/code/one","joinedAt":1000,"isActive":true}]}"#,
        )
        .unwrap();
        // Its transcript, with the team keys on line 4.
        let tm = projects.join("cafe0001-0000-4000-8000-000000000000.jsonl");
        std::fs::write(
            &tm,
            r#"{"type":"agent-setting","sessionId":"cafe0001-0000-4000-8000-000000000000"}
{"type":"mode","mode":"normal"}
{"type":"permission-mode","permissionMode":"auto"}
{"type":"user","timestamp":"2026-01-01T00:00:00Z","sessionId":"cafe0001-0000-4000-8000-000000000000","teamName":"session-deadbeef","agentName":"worker","message":{"role":"user","content":"<teammate-message>go</teammate-message>"}}
{"type":"assistant","timestamp":"2026-01-01T00:00:05Z","sessionId":"cafe0001-0000-4000-8000-000000000000","teamName":"session-deadbeef","agentName":"worker","message":{"id":"m1","model":"claude-opus-5","content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","usage":{"cache_read_input_tokens":1000,"output_tokens":10}}}
"#,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            w.poll(&mut team, &[], 0);
            if team.as_ref().is_some_and(|t| {
                t.members
                    .get("worker")
                    .is_some_and(|m| m.agg.api_calls() == 1)
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let t = team.as_ref().expect("found");
        assert_eq!(t.source, Source::Config);
        let m = &t.members["worker"];
        assert_eq!(m.path.as_deref(), Some(tm.as_path()));
        assert_eq!(m.model, "claude-opus-5");
        assert_eq!(m.liveness(0, t.source), Liveness::Active);
        assert!(t.looked_in.contains(&projects));
        // The directory goes: the member is `Gone` (no ledger, no recent line).
        std::fs::remove_dir_all(teams.join("session-deadbeef")).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            w.poll(&mut team, &[], i64::MAX / 2);
            if team
                .as_ref()
                .is_some_and(|t| t.source == Source::Transcripts)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let t = team.as_ref().unwrap();
        assert_eq!(t.source, Source::Transcripts);
        assert!(t.dir_seen);
        assert_eq!(
            t.members["worker"].liveness(i64::MAX / 2, t.source),
            Liveness::Gone
        );
        assert_eq!(t.members.len(), 1, "the transcript keeps the member");
    }
}
