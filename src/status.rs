//! The status-line JSON Claude Code pipes to the user's status-line command.
//! `cctop statusline-shim` tees it to `~/.cctop/status/<session_id>.json`;
//! [`Watcher`] reads that file. This is the only source of rate limits.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

/// `~/.cctop`.
pub fn cctop_dir() -> PathBuf {
    std::env::var_os("CCTOP_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cctop")))
        .unwrap_or_else(|| PathBuf::from(".cctop"))
}

pub fn status_path(session_id: &str) -> PathBuf {
    cctop_dir()
        .join("status")
        .join(format!("{session_id}.json"))
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ContextWindow {
    #[serde(default)]
    pub total_input_tokens: u64,
    #[serde(default)]
    pub total_output_tokens: u64,
    #[serde(default)]
    pub context_window_size: u64,
    pub used_percentage: Option<f64>,
    pub remaining_percentage: Option<f64>,
    /// The last call's usage, as the API reported it.
    #[serde(default)]
    pub current_usage: Option<CurrentUsage>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct CurrentUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Limit {
    #[serde(default)]
    pub used_percentage: f64,
    /// Unix seconds (as Claude Code writes it).
    pub resets_at: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct RateLimits {
    #[serde(default)]
    pub five_hour: Limit,
    #[serde(default)]
    pub seven_day: Limit,
    /// The monthly spend limit, for accounts that have one.
    #[serde(default)]
    pub spend_limit: Option<Limit>,
}

/// Claude Code's own prompt-cache diagnosis (2.1.251+; causes 2.1.260+).
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct PromptCache {
    #[serde(default)]
    pub warm: bool,
    #[serde(default)]
    pub caching_observed: bool,
    /// `1h` or `5m`.
    pub ttl: Option<String>,
    /// Unix seconds when the entry expires, while warm.
    pub expires_at: Option<f64>,
    #[serde(default)]
    pub requests: u64,
    #[serde(default)]
    pub misses: u64,
    #[serde(default)]
    pub expected_rebuilds: u64,
    pub hit_ratio: Option<f64>,
    #[serde(default)]
    pub cache_write_tokens: u64,
    /// Tokens re-written by the misses so far.
    #[serde(default)]
    pub miss_recache_tokens: u64,
    pub last_miss_at: Option<f64>,
    #[serde(default)]
    pub last_miss_cause: Option<MissCause>,
    /// Cause → count over the session (`model_changed`, `tools_changed`,
    /// `ttl_expired_1h`, `messages_rewritten`, `likely_server_side`…).
    #[serde(default)]
    pub miss_causes: std::collections::BTreeMap<String, u64>,
    /// What the next call would re-write if the cache went cold now.
    #[serde(default)]
    pub recache_tokens_if_cold: u64,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct MissCause {
    #[serde(default)]
    pub causes: Vec<String>,
}

impl PromptCache {
    /// The named cause of the last miss (the first one Claude Code lists).
    pub fn last_cause(&self) -> Option<&str> {
        self.last_miss_cause
            .as_ref()
            .and_then(|c| c.causes.first())
            .map(String::as_str)
    }

    /// TTL as a duration in milliseconds.
    pub fn ttl_ms(&self) -> Option<i64> {
        match self.ttl.as_deref() {
            Some("1h") => Some(3_600_000),
            Some("5m") => Some(300_000),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Effort {
    pub level: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Thinking {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct CostInfo {
    pub total_cost_usd: Option<f64>,
    pub total_duration_ms: Option<u64>,
    pub total_api_duration_ms: Option<u64>,
    pub total_lines_added: Option<u64>,
    pub total_lines_removed: Option<u64>,
}

/// One status-line payload. Unknown fields are ignored.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Sample {
    pub session_id: Option<String>,
    #[serde(default)]
    pub model: ModelInfo,
    #[serde(default)]
    pub context_window: ContextWindow,
    #[serde(default)]
    pub rate_limits: Option<RateLimits>,
    #[serde(default)]
    pub cost: Option<CostInfo>,
    #[serde(default)]
    pub prompt_cache: Option<PromptCache>,
    #[serde(default)]
    pub effort: Option<Effort>,
    #[serde(default)]
    pub thinking: Option<Thinking>,
    #[serde(default)]
    pub fast_mode: bool,
    #[serde(default)]
    pub exceeds_200k_tokens: bool,
    pub session_name: Option<String>,
    pub prompt_id: Option<String>,
    /// The Claude Code version that wrote the payload.
    pub version: Option<String>,
    /// Pull-request facts (`number`, `url`, `review_state`…), kept raw.
    #[serde(default)]
    pub pr: Option<serde_json::Value>,
    /// Worktree facts (`name`, `path`, `branch`…), kept raw.
    #[serde(default)]
    pub worktree: Option<serde_json::Value>,
    /// Filled by the watcher: epoch ms the file was written.
    #[serde(skip)]
    pub at_ms: i64,
}

impl Sample {
    pub fn parse(text: &str) -> Option<Sample> {
        serde_json::from_str(text).ok()
    }

    pub fn effort_level(&self) -> Option<&str> {
        self.effort.as_ref().and_then(|e| e.level.as_deref())
    }

    pub fn thinking_enabled(&self) -> Option<bool> {
        self.thinking.as_ref().map(|t| t.enabled)
    }

    /// `pr.number`, when the payload carries a PR.
    pub fn pr_number(&self) -> Option<u64> {
        self.pr.as_ref()?.get("number")?.as_u64()
    }

    pub fn pr_review_state(&self) -> Option<&str> {
        self.pr.as_ref()?.get("review_state")?.as_str()
    }
}

/// Write `payload` atomically to the status file for its session.
pub fn store(payload: &[u8]) -> std::io::Result<Option<PathBuf>> {
    store_in(&cctop_dir(), payload)
}

/// [`store`] under an explicit cctop home.
pub fn store_in(home: &Path, payload: &[u8]) -> std::io::Result<Option<PathBuf>> {
    let Some(s) = std::str::from_utf8(payload).ok().and_then(Sample::parse) else {
        return Ok(None);
    };
    let Some(id) = s.session_id.filter(|i| !i.is_empty()) else {
        return Ok(None);
    };
    let path = home.join("status").join(format!("{id}.json"));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    std::fs::write(&tmp, payload)?;
    std::fs::rename(&tmp, &path)?;
    Ok(Some(path))
}

/// `cctop statusline-shim [-- original command...]`: tee stdin to the status
/// file, then run the original command on the same input. Never fails the
/// status line: storage errors are swallowed.
pub fn run_shim(original: &[String]) -> i32 {
    let mut payload = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut payload);
    let _ = store(&payload);
    let Some((cmd, args)) = original.split_first() else {
        return 0;
    };
    let child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cctop statusline-shim: cannot run {cmd}: {e}");
            return 0;
        }
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&payload);
    }
    child.wait().map(|s| s.code().unwrap_or(0)).unwrap_or(0)
}

/// Polls the status file for a session and keeps a used-percentage series.
#[derive(Debug)]
pub struct Watcher {
    path: PathBuf,
    last_mtime: Option<std::time::SystemTime>,
    pub latest: Option<Sample>,
    /// `(epoch ms, five_hour used %)`, oldest first, last 24 h.
    pub series_5h: Vec<(i64, f64)>,
}

impl Watcher {
    pub fn new(session_id: &str) -> Watcher {
        Watcher::at(status_path(session_id))
    }

    pub fn at(path: PathBuf) -> Watcher {
        let mut w = Watcher {
            path,
            last_mtime: None,
            latest: None,
            series_5h: Vec::new(),
        };
        w.poll();
        w
    }

    /// Re-read the file if it changed; returns true on a new sample.
    pub fn poll(&mut self) -> bool {
        let Ok(meta) = std::fs::metadata(&self.path) else {
            return false;
        };
        let mtime = meta.modified().ok();
        if mtime.is_some() && mtime == self.last_mtime {
            return false;
        }
        self.last_mtime = mtime;
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return false;
        };
        let Some(mut s) = Sample::parse(&text) else {
            return false;
        };
        s.at_ms = mtime
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if let Some(rl) = &s.rate_limits {
            self.series_5h.push((s.at_ms, rl.five_hour.used_percentage));
            let cutoff = s.at_ms - 24 * 3600 * 1000;
            self.series_5h.retain(|(t, _)| *t >= cutoff);
        }
        self.latest = Some(s);
        true
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAYLOAD: &str = r#"{"session_id":"abc-123","model":{"id":"claude-opus-5","display_name":"Opus 5"},"workspace":{"current_dir":"/x"},"context_window":{"total_input_tokens":134000,"context_window_size":200000},"rate_limits":{"five_hour":{"used_percentage":62,"resets_at":1789158000},"seven_day":{"used_percentage":23,"resets_at":1789500000}},"cost":{"total_cost_usd":4.37}}"#;

    const FULL: &str = r#"{"session_id":"50658b8f","prompt_id":"9434b2e9","effort":{"level":"max"},"session_name":"coach","model":{"id":"claude-opus-5","display_name":"Opus 5"},"version":"2.1.270","cost":{"total_cost_usd":7.99,"total_duration_ms":565680,"total_api_duration_ms":363788,"total_lines_added":196,"total_lines_removed":0},"context_window":{"total_input_tokens":291664,"total_output_tokens":633,"context_window_size":1000000,"current_usage":{"input_tokens":2,"output_tokens":633,"cache_creation_input_tokens":4595,"cache_read_input_tokens":287067},"used_percentage":29,"remaining_percentage":71},"exceeds_200k_tokens":true,"prompt_cache":{"warm":true,"caching_observed":true,"ttl":"1h","expires_at":1789405956,"requests":49,"misses":2,"expected_rebuilds":0,"hit_ratio":0.9735,"cache_write_tokens":257729,"miss_recache_tokens":95700,"last_miss_at":1789223965,"last_miss_cause":{"causes":["model_changed","betas_changed"]},"miss_causes":{"model_changed":1,"ttl_expired_1h":1},"recache_tokens_if_cold":291664},"fast_mode":false,"thinking":{"enabled":true},"rate_limits":{"five_hour":{"used_percentage":11,"resets_at":1789411800},"seven_day":{"used_percentage":13,"resets_at":1789812000},"spend_limit":{"used_percentage":40}},"pr":{"number":142,"review_state":"approved"},"worktree":{"name":"feat"}}"#;

    #[test]
    fn full_payload_of_2_1_270() {
        let s = Sample::parse(FULL).unwrap();
        let pc = s.prompt_cache.as_ref().unwrap();
        assert!(pc.warm);
        assert_eq!(pc.ttl.as_deref(), Some("1h"));
        assert_eq!(pc.ttl_ms(), Some(3_600_000));
        assert_eq!(pc.expires_at, Some(1_789_405_956.0));
        assert_eq!(pc.misses, 2);
        assert_eq!(pc.recache_tokens_if_cold, 291_664);
        assert_eq!(pc.last_cause(), Some("model_changed"));
        assert_eq!(pc.miss_causes["ttl_expired_1h"], 1);
        assert_eq!(pc.miss_recache_tokens, 95_700);
        assert_eq!(s.effort_level(), Some("max"));
        assert_eq!(s.thinking_enabled(), Some(true));
        assert!(!s.fast_mode);
        assert!(s.exceeds_200k_tokens);
        assert_eq!(s.context_window.total_output_tokens, 633);
        assert_eq!(s.context_window.remaining_percentage, Some(71.0));
        assert_eq!(
            s.context_window
                .current_usage
                .as_ref()
                .unwrap()
                .cache_read_input_tokens,
            287_067
        );
        assert_eq!(
            s.rate_limits
                .as_ref()
                .unwrap()
                .spend_limit
                .as_ref()
                .unwrap()
                .used_percentage,
            40.0
        );
        assert_eq!(s.session_name.as_deref(), Some("coach"));
        assert_eq!(s.prompt_id.as_deref(), Some("9434b2e9"));
        assert_eq!(s.version.as_deref(), Some("2.1.270"));
        assert_eq!(s.pr_number(), Some(142));
        assert_eq!(s.pr_review_state(), Some("approved"));
        // The old, smaller payload still parses; nothing new is required.
        let old = Sample::parse(PAYLOAD).unwrap();
        assert!(old.prompt_cache.is_none());
        assert_eq!(old.effort_level(), None);
        assert_eq!(old.pr_number(), None);
    }

    #[test]
    fn parse_sample() {
        let s = Sample::parse(PAYLOAD).unwrap();
        assert_eq!(s.session_id.as_deref(), Some("abc-123"));
        assert_eq!(s.context_window.total_input_tokens, 134_000);
        assert_eq!(s.context_window.context_window_size, 200_000);
        let rl = s.rate_limits.unwrap();
        assert_eq!(rl.five_hour.used_percentage, 62.0);
        assert_eq!(rl.five_hour.resets_at, Some(1_789_158_000.0));
        assert_eq!(s.cost.unwrap().total_cost_usd, Some(4.37));
        assert_eq!(s.model.display_name.as_deref(), Some("Opus 5"));
        assert!(Sample::parse("nope").is_none());
    }

    #[test]
    fn store_and_watch_roundtrip() {
        let home = std::env::temp_dir().join(format!("cctop-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let path = store_in(&home, PAYLOAD.as_bytes()).unwrap().unwrap();
        assert!(path.ends_with("status/abc-123.json"));
        let mut w = Watcher::at(path.clone());
        assert_eq!(
            w.latest.as_ref().unwrap().context_window.total_input_tokens,
            134_000
        );
        assert_eq!(w.series_5h.len(), 1);
        assert!(!w.poll(), "unchanged file is not a new sample");
        // A later write with a different mtime is picked up.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = PAYLOAD.replace("\"used_percentage\":62", "\"used_percentage\":64");
        std::fs::write(&path, newer).unwrap();
        let t = std::fs::metadata(&path).unwrap().modified().unwrap()
            + std::time::Duration::from_secs(1);
        let _ = std::fs::File::options()
            .write(true)
            .open(&path)
            .and_then(|f| f.set_modified(t));
        assert!(w.poll());
        assert_eq!(w.series_5h.len(), 2);
        assert_eq!(w.series_5h[1].1, 64.0);
        assert!(store_in(&home, b"{\"no_session\":1}").unwrap().is_none());
    }

    #[test]
    fn shim_adds_under_three_milliseconds() {
        let home = std::env::temp_dir().join(format!("cctop-shim-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        store_in(&home, PAYLOAD.as_bytes()).unwrap(); // warm: creates the dir
                                                      // The best of five batches: the shim's own cost, not a shared CI
                                                      // runner's scheduling hiccup (one batch measured 6.9 ms there).
        let per = (0..5)
            .map(|_| {
                let start = std::time::Instant::now();
                for _ in 0..20 {
                    store_in(&home, PAYLOAD.as_bytes()).unwrap();
                }
                start.elapsed() / 20
            })
            .min()
            .unwrap();
        assert!(
            per < std::time::Duration::from_millis(3),
            "{per:?} per store"
        );
    }
}
