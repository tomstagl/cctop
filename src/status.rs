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
    pub context_window_size: u64,
    pub used_percentage: Option<f64>,
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
    /// Plan name, whichever key Claude Code uses.
    #[serde(alias = "subscription_type", alias = "plan_type")]
    pub plan: Option<String>,
    /// Filled by the watcher: epoch ms the file was written.
    #[serde(skip)]
    pub at_ms: i64,
}

impl Sample {
    pub fn parse(text: &str) -> Option<Sample> {
        serde_json::from_str(text).ok()
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
        let start = std::time::Instant::now();
        for _ in 0..20 {
            store_in(&home, PAYLOAD.as_bytes()).unwrap();
        }
        let per = start.elapsed() / 20;
        assert!(
            per < std::time::Duration::from_millis(3),
            "{per:?} per store"
        );
    }
}
