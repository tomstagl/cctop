//! Files the session touched, from file-tool inputs, plus git line counts.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use crate::metrics::cost::parse_ts_ms;
use crate::transcript::{AssistantBlock, Line};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileStats {
    pub path: String,
    pub reads: usize,
    pub edits: usize,
    pub writes: usize,
    pub last_touch_ms: i64,
    /// Consecutive reads since the last edit/write.
    pub reads_since_edit: usize,
    /// `git diff --numstat` against the session-start commit.
    pub lines_added: Option<u64>,
    pub lines_removed: Option<u64>,
}

impl FileStats {
    pub fn touches(&self) -> usize {
        self.reads + self.edits + self.writes
    }
    /// ≥ 3 reads with no edit in between.
    pub fn reread_warning(&self) -> bool {
        self.reads_since_edit >= 3
    }
}

#[derive(Debug, Default)]
pub struct Files {
    pub files: BTreeMap<String, FileStats>,
    seen_tool_uses: std::collections::HashSet<String>,
}

impl Files {
    pub fn push(&mut self, line: &Line) {
        let Line::Assistant(a) = line else {
            return;
        };
        let at = a.timestamp.as_deref().and_then(parse_ts_ms).unwrap_or(0);
        for b in &a.message.content {
            let AssistantBlock::ToolUse { id, name, input } = b else {
                continue;
            };
            if !self.seen_tool_uses.insert(id.clone()) {
                continue;
            }
            let path = input
                .get("file_path")
                .or_else(|| input.get("notebook_path"))
                .and_then(|v| v.as_str());
            let Some(path) = path else {
                continue;
            };
            let f = self
                .files
                .entry(path.to_string())
                .or_insert_with(|| FileStats {
                    path: path.to_string(),
                    ..Default::default()
                });
            match name.as_str() {
                "Read" => {
                    f.reads += 1;
                    f.reads_since_edit += 1;
                }
                "Edit" | "MultiEdit" | "NotebookEdit" => {
                    f.edits += 1;
                    f.reads_since_edit = 0;
                }
                "Write" => {
                    f.writes += 1;
                    f.reads_since_edit = 0;
                }
                _ => {
                    self.files.remove(path);
                    continue;
                }
            }
            f.last_touch_ms = f.last_touch_ms.max(at);
        }
    }

    /// Apply `git diff --numstat` figures (paths relative to `cwd`).
    pub fn apply_numstat(&mut self, cwd: &Path, numstat: &[(String, u64, u64)]) {
        for f in self.files.values_mut() {
            let rel = Path::new(&f.path)
                .strip_prefix(cwd)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| f.path.clone());
            match numstat.iter().find(|(p, _, _)| *p == rel) {
                Some((_, a, d)) => {
                    f.lines_added = Some(*a);
                    f.lines_removed = Some(*d);
                }
                None => {
                    f.lines_added = None;
                    f.lines_removed = None;
                }
            }
        }
    }

    pub fn total_lines(&self) -> (u64, u64) {
        self.files.values().fold((0, 0), |(a, d), f| {
            (
                a + f.lines_added.unwrap_or(0),
                d + f.lines_removed.unwrap_or(0),
            )
        })
    }
}

/// `HEAD` of `cwd` right now, for the session-start baseline.
pub fn head_commit(cwd: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `(path, added, removed)` for the working tree vs `base`.
pub fn numstat(cwd: &Path, base: &str) -> Vec<(String, u64, u64)> {
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["diff", "--numstat", base])
        .output()
    else {
        return Vec::new();
    };
    parse_numstat(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_numstat(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .filter_map(|l| {
            let mut it = l.split('\t');
            let a = it.next()?.parse().unwrap_or(0); // "-" for binary
            let d = it.next()?.parse().unwrap_or(0);
            Some((it.next()?.to_string(), a, d))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::parse_file;

    #[test]
    fn fixture_files() {
        let mut f = Files::default();
        for l in parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl"))
            .unwrap()
        {
            f.push(&l);
        }
        assert_eq!(f.files.len(), 4);
        assert_eq!(f.files.values().map(|x| x.reads).sum::<usize>(), 3);
        assert_eq!(f.files.values().map(|x| x.writes).sum::<usize>(), 1);
        assert!(f.files.values().all(|x| !x.reread_warning()));
    }

    #[test]
    fn reread_warning_and_numstat() {
        let mut f = Files::default();
        let mk = |id: &str, tool: &str, path: &str| {
            Line::parse(&format!(
                r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:0{}Z","message":{{"id":"{id}","model":"m","content":[{{"type":"tool_use","id":"{id}","name":"{tool}","input":{{"file_path":"{path}"}}}}],"usage":{{}}}}}}"#,
                id.len() % 10
            ))
            .unwrap()
        };
        for i in 0..3 {
            f.push(&mk(&format!("r{i}"), "Read", "/repo/src/a.rs"));
        }
        assert!(f.files["/repo/src/a.rs"].reread_warning());
        f.push(&mk("e1", "Edit", "/repo/src/a.rs"));
        assert!(!f.files["/repo/src/a.rs"].reread_warning());
        assert_eq!(f.files["/repo/src/a.rs"].touches(), 4);
        f.apply_numstat(
            Path::new("/repo"),
            &parse_numstat("210\t31\tsrc/a.rs\n-\t-\tbin.png\n"),
        );
        assert_eq!(f.files["/repo/src/a.rs"].lines_added, Some(210));
        assert_eq!(f.total_lines(), (210, 31));
        assert!(head_commit(Path::new(env!("CARGO_MANIFEST_DIR"))).is_some());
        assert_eq!(head_commit(Path::new("/")), None);
    }
}
