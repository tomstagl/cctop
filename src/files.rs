//! Files the session touched, from file-tool inputs and results, plus git
//! line counts.
//!
//! Re-reads are counted the way the cost is paid: a `Read` that re-injects
//! the file counts, a ranged `Read` (offset/limit) or a `file_unchanged`
//! result (~30 tokens) does not, and a Bash `cat` / `sed -n` / `head` /
//! `tail` of a path counts like a `Read`. The counter resets when the file
//! changed under the model (an IDE edit, a stale-read recovery) and at every
//! context boundary, because the next read is then a fresh read.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::metrics::cost::parse_ts_ms;
use crate::transcript::{AssistantBlock, AttachmentKind, Line, ReadKind, ToolUseDetail};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileStats {
    pub path: String,
    pub reads: usize,
    pub edits: usize,
    pub writes: usize,
    pub last_touch_ms: i64,
    /// Whole-file reads since the last edit/write (or boundary).
    pub reads_since_edit: usize,
    /// Reads that cost nothing to speak of: ranged, or `file_unchanged`.
    pub cheap_reads: usize,
    /// Bash reads (`cat`, `sed -n`, `head`, `tail`) of this path.
    pub bash_reads: usize,
    /// `edit → re-read → edit` sequences: the model re-read the file it had
    /// just changed before changing it again.
    pub edit_reread_edit: usize,
    /// The person edited it in the IDE, or a stale-read recovery happened,
    /// since the model last touched it.
    pub stale: bool,
    /// Times the person edited it in the IDE (`edited_text_file`).
    pub ide_edits: usize,
    /// When the IDE last edited it (epoch ms), 0 = never.
    pub ide_edited_at_ms: i64,
    /// Checkpoint version from `file-history-delta` (rewind points).
    pub checkpoint_version: Option<u64>,
    /// Edits in the current turn.
    pub edits_this_turn: usize,
    /// `git diff --numstat` against the session-start commit.
    pub lines_added: Option<u64>,
    pub lines_removed: Option<u64>,
    /// After the last edit: 0 = nothing read since, 1 = read once…
    last_was_edit: bool,
    reread_after_edit: bool,
}

impl FileStats {
    pub fn touches(&self) -> usize {
        self.reads + self.edits + self.writes
    }
    /// ≥ 3 whole-file reads with no edit in between.
    pub fn reread_warning(&self) -> bool {
        self.reads_since_edit >= 3
    }

    fn note_read(&mut self, cheap: bool) {
        self.reads += 1;
        if cheap {
            self.cheap_reads += 1;
            return;
        }
        self.reads_since_edit += 1;
        if self.last_was_edit {
            self.reread_after_edit = true;
        }
    }

    fn note_edit(&mut self, write: bool) {
        if write {
            self.writes += 1;
        } else {
            self.edits += 1;
        }
        if self.reread_after_edit {
            self.edit_reread_edit += 1;
        }
        self.reads_since_edit = 0;
        self.stale = false;
        self.last_was_edit = true;
        self.reread_after_edit = false;
        self.edits_this_turn += 1;
    }

    fn reset(&mut self, stale: bool) {
        self.reads_since_edit = 0;
        self.last_was_edit = false;
        self.reread_after_edit = false;
        self.stale = stale;
    }
}

/// A `Read` issued and not yet answered.
struct PendingRead {
    path: String,
    /// `offset` / `limit` in the input: a slice, not the file.
    ranged: bool,
}

#[derive(Debug, Default)]
pub struct Files {
    pub files: BTreeMap<String, FileStats>,
    seen_tool_uses: std::collections::HashSet<String>,
    pending_reads: HashMap<String, PendingRead>,
    /// Bash calls whose command reads paths, awaiting their result.
    pending_bash: HashMap<String, Vec<String>>,
    /// Edit/Write calls awaiting their result (for `staleRecovered`).
    pending_edits: HashMap<String, String>,
    /// The session's working directory, to resolve relative Bash paths.
    cwd: Option<PathBuf>,
}

impl std::fmt::Debug for PendingRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PendingRead({}, ranged={})", self.path, self.ranged)
    }
}

impl Files {
    pub fn set_cwd(&mut self, cwd: &Path) {
        if self.cwd.is_none() && !cwd.as_os_str().is_empty() {
            self.cwd = Some(cwd.to_path_buf());
        }
    }

    fn entry(&mut self, path: &str) -> &mut FileStats {
        self.files
            .entry(path.to_string())
            .or_insert_with(|| FileStats {
                path: path.to_string(),
                ..Default::default()
            })
    }

    /// The key a Bash path maps to: an existing entry it ends with, else the
    /// path resolved against the working directory.
    fn resolve(&self, raw: &str) -> String {
        if let Some(k) = self.files.keys().find(|k| {
            k.as_str() == raw || k.ends_with(&format!("/{}", raw.trim_start_matches("./")))
        }) {
            return k.clone();
        }
        if raw.starts_with('/') {
            return raw.to_string();
        }
        match &self.cwd {
            Some(c) => c
                .join(raw.trim_start_matches("./"))
                .to_string_lossy()
                .into_owned(),
            None => raw.to_string(),
        }
    }

    /// A context boundary (`/clear`, compaction, resume): every re-read
    /// counter starts over.
    pub fn boundary(&mut self) {
        for f in self.files.values_mut() {
            f.reset(false);
        }
    }

    /// A new turn: per-turn edit counts start over.
    pub fn new_turn(&mut self) {
        for f in self.files.values_mut() {
            f.edits_this_turn = 0;
        }
    }

    pub fn push(&mut self, line: &Line) {
        match line {
            Line::Assistant(a) => self.push_assistant(a),
            Line::User(u) => self.push_user(u),
            Line::Attachment(att) => {
                if let AttachmentKind::EditedTextFile { filename, .. } = att.kind() {
                    let key = self.resolve(&filename);
                    let at = att.timestamp.as_deref().and_then(parse_ts_ms).unwrap_or(0);
                    if let Some(f) = self.files.get_mut(&key) {
                        f.reset(true);
                        f.ide_edits += 1;
                        f.ide_edited_at_ms = f.ide_edited_at_ms.max(at);
                    }
                }
            }
            Line::FileHistoryDelta(d) => {
                if let (Some(p), Some(b)) = (&d.tracking_path, &d.backup) {
                    let key = self.resolve(p);
                    if let Some(f) = self.files.get_mut(&key) {
                        f.checkpoint_version = Some(b.version);
                    }
                }
            }
            Line::System(s) => {
                use crate::transcript::SystemKind;
                if matches!(
                    s.kind(),
                    SystemKind::CompactBoundary | SystemKind::MicrocompactBoundary
                ) {
                    self.boundary();
                }
            }
            Line::ContinuedIn(_) => self.boundary(),
            _ => {}
        }
    }

    fn push_assistant(&mut self, a: &crate::transcript::AssistantLine) {
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
            match (name.as_str(), path) {
                ("Read", Some(p)) => {
                    let ranged = input.get("offset").is_some() || input.get("limit").is_some();
                    let f = self.entry(p);
                    f.last_touch_ms = f.last_touch_ms.max(at);
                    self.pending_reads.insert(
                        id.clone(),
                        PendingRead {
                            path: p.to_string(),
                            ranged,
                        },
                    );
                }
                ("Edit" | "MultiEdit" | "NotebookEdit", Some(p)) => {
                    let f = self.entry(p);
                    f.note_edit(false);
                    f.last_touch_ms = f.last_touch_ms.max(at);
                    self.pending_edits.insert(id.clone(), p.to_string());
                }
                ("Write", Some(p)) => {
                    let f = self.entry(p);
                    f.note_edit(true);
                    f.last_touch_ms = f.last_touch_ms.max(at);
                    self.pending_edits.insert(id.clone(), p.to_string());
                }
                ("Bash", _) => {
                    let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
                    if crate::phase::read_only_bash(cmd) {
                        let paths = bash_read_paths(cmd);
                        if !paths.is_empty() {
                            self.pending_bash.insert(id.clone(), paths);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn push_user(&mut self, u: &crate::transcript::UserLine) {
        let detail = u.tool_use_detail();
        for r in u.message.content.tool_results() {
            if let Some(p) = self.pending_reads.remove(&r.tool_use_id) {
                let cheap = p.ranged
                    || matches!(
                        &detail,
                        Some(ToolUseDetail::Read(rd)) if rd.kind == ReadKind::FileUnchanged || rd.is_ranged()
                    );
                self.entry(&p.path).note_read(cheap);
            }
            if let Some(paths) = self.pending_bash.remove(&r.tool_use_id) {
                if !r.is_error {
                    for raw in paths {
                        let key = self.resolve(&raw);
                        let f = self.entry(&key);
                        f.bash_reads += 1;
                        f.note_read(false);
                    }
                }
            }
            if let Some(path) = self.pending_edits.remove(&r.tool_use_id) {
                if let Some(ToolUseDetail::Edit(e) | ToolUseDetail::Write(e)) = &detail {
                    if e.stale_recovered {
                        self.entry(&path).reset(true);
                    }
                }
            }
            if let Some(ToolUseDetail::Bash(b)) = &detail {
                for raw in &b.stale_read_paths {
                    let key = self.resolve(raw);
                    self.entry(&key).reset(true);
                }
            }
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

/// Paths a read-only Bash command reads: the file arguments of `cat`,
/// `sed -n`, `head`, `tail` segments.
fn bash_read_paths(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    for seg in cmd.split(['|', ';', '\n']) {
        let seg = seg.trim().trim_start_matches("&&").trim();
        let mut words = seg.split_whitespace();
        let Some(head) = words.next() else { continue };
        let is_reader =
            matches!(head, "cat" | "head" | "tail") || (head == "sed" && seg.contains(" -n"));
        if !is_reader {
            continue;
        }
        for w in words {
            let w = w.trim_matches(['\'', '"']);
            if w.starts_with('-') || w.is_empty() {
                continue;
            }
            if (w.contains('/') || w.contains('.')) && !out.contains(&w.to_string()) {
                out.push(w.to_string());
            }
        }
    }
    out
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
    fn fixture_b_counts_bash_reads_and_resets_on_ide_edits() {
        let mut f = Files::default();
        f.set_cwd(Path::new("/home/user/project"));
        for l in parse_file(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl"))
            .unwrap()
        {
            f.push(&l);
        }
        assert!(f.files.len() > 10, "{}", f.files.len());
        let edits: usize = f.files.values().map(|x| x.edits + x.writes).sum();
        assert_eq!(edits, 30);
        assert!(
            f.files.values().any(|x| x.bash_reads > 0),
            "sed -n / cat reads count"
        );
        assert!(
            f.files.values().any(|x| x.checkpoint_version.is_some()),
            "file-history-delta"
        );
        // The IDE edits (edited_text_file) reset their files' counters; the
        // model edited all three again afterwards, so none is stale now.
        assert_eq!(f.files.values().map(|x| x.ide_edits).sum::<usize>(), 3);
        assert!(f.files.values().all(|x| !x.stale));
    }

    fn call(id: &str, tool: &str, input: &str, t: u32) -> Line {
        Line::parse(&format!(
            r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:{t:02}Z","message":{{"id":"{id}","model":"m","content":[{{"type":"tool_use","id":"{id}","name":"{tool}","input":{input}}}],"usage":{{}}}}}}"#
        ))
        .unwrap()
    }

    fn result(id: &str, tur: &str, t: u32) -> Line {
        Line::parse(&format!(
            r#"{{"type":"user","timestamp":"2026-01-01T00:00:{t:02}Z","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"x"}}]}},"toolUseResult":{tur}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn reread_rules() {
        let mut f = Files::default();
        f.set_cwd(Path::new("/repo"));
        let whole = r#"{"type":"text","file":{"filePath":"/repo/src/a.rs","content":"…","numLines":10,"startLine":1,"totalLines":10}}"#;
        for i in 0..3 {
            f.push(&call(
                &format!("r{i}"),
                "Read",
                r#"{"file_path":"/repo/src/a.rs"}"#,
                i,
            ));
            f.push(&result(&format!("r{i}"), whole, i));
        }
        assert!(f.files["/repo/src/a.rs"].reread_warning());
        assert_eq!(f.files["/repo/src/a.rs"].reads, 3);
        // A ranged read and a file_unchanged result are cheap: no re-read.
        f.push(&call(
            "r3",
            "Read",
            r#"{"file_path":"/repo/src/a.rs","offset":10,"limit":20}"#,
            3,
        ));
        f.push(&result("r3", r#"{"type":"text","file":{"filePath":"/repo/src/a.rs","content":"…","numLines":20,"startLine":10,"totalLines":100}}"#, 3));
        f.push(&call("r4", "Read", r#"{"file_path":"/repo/src/a.rs"}"#, 4));
        f.push(&result(
            "r4",
            r#"{"type":"file_unchanged","file":{"filePath":"/repo/src/a.rs"}}"#,
            4,
        ));
        let a = &f.files["/repo/src/a.rs"];
        assert_eq!(a.reads, 5);
        assert_eq!(a.cheap_reads, 2);
        assert_eq!(a.reads_since_edit, 3);
        // An edit resets; edit → re-read → edit is counted as churn.
        f.push(&call(
            "e1",
            "Edit",
            r#"{"file_path":"/repo/src/a.rs","old_string":"a","new_string":"b"}"#,
            5,
        ));
        f.push(&result("e1", r#"{"filePath":"/repo/src/a.rs","oldString":"a","newString":"b","originalFile":"a","structuredPatch":[],"userModified":false}"#, 5));
        assert!(!f.files["/repo/src/a.rs"].reread_warning());
        f.push(&call("r5", "Read", r#"{"file_path":"/repo/src/a.rs"}"#, 6));
        f.push(&result("r5", whole, 6));
        f.push(&call(
            "e2",
            "Edit",
            r#"{"file_path":"/repo/src/a.rs","old_string":"b","new_string":"c"}"#,
            7,
        ));
        f.push(&result("e2", r#"{"filePath":"/repo/src/a.rs","oldString":"b","newString":"c","originalFile":"b","structuredPatch":[],"userModified":false,"staleRecovered":true}"#, 7));
        let a = &f.files["/repo/src/a.rs"];
        assert_eq!(a.edit_reread_edit, 1);
        assert!(a.stale, "staleRecovered: the file changed under the model");
        assert_eq!(a.touches(), 8);
        // Bash reads of a relative path resolve to the same file and count.
        f.push(&call(
            "b1",
            "Bash",
            r#"{"command":"sed -n '1,40p' src/a.rs && cat src/b.rs | head -5"}"#,
            8,
        ));
        f.push(&result(
            "b1",
            r#"{"stdout":"…","stderr":"","interrupted":false}"#,
            8,
        ));
        assert_eq!(f.files["/repo/src/a.rs"].bash_reads, 1);
        assert_eq!(f.files["/repo/src/a.rs"].reads_since_edit, 1);
        assert_eq!(f.files["/repo/src/b.rs"].bash_reads, 1);
        // A writing Bash command counts nothing; a stale hint resets.
        f.push(&call(
            "b2",
            "Bash",
            r#"{"command":"cat src/a.rs > /tmp/x"}"#,
            9,
        ));
        f.push(&result("b2", r#"{"stdout":"","stderr":"","interrupted":false,"staleReadFileStateHint":"[This command modified 1 file you've previously read: src/a.rs]"}"#, 9));
        assert_eq!(f.files["/repo/src/a.rs"].bash_reads, 1);
        assert!(f.files["/repo/src/a.rs"].stale);
        assert_eq!(f.files["/repo/src/a.rs"].reads_since_edit, 0);
        // An IDE edit and a boundary reset too.
        f.push(&call("r6", "Read", r#"{"file_path":"/repo/src/b.rs"}"#, 10));
        f.push(&result("r6", r#"{"type":"text","file":{"filePath":"/repo/src/b.rs","content":"…","numLines":5,"startLine":1,"totalLines":5}}"#, 10));
        assert_eq!(f.files["/repo/src/b.rs"].reads_since_edit, 2);
        f.push(&Line::parse(r#"{"type":"attachment","timestamp":"t","attachment":{"type":"edited_text_file","filename":"/repo/src/b.rs","snippet":"…"}}"#).unwrap());
        assert_eq!(f.files["/repo/src/b.rs"].reads_since_edit, 0);
        assert!(f.files["/repo/src/b.rs"].stale);
        f.push(&call("r7", "Read", r#"{"file_path":"/repo/src/b.rs"}"#, 11));
        f.push(&result("r7", r#"{"type":"text","file":{"filePath":"/repo/src/b.rs","content":"…","numLines":5,"startLine":1,"totalLines":5}}"#, 11));
        f.push(
            &Line::parse(r#"{"type":"continued-in","timestamp":"t","continuedInSessionId":"x"}"#)
                .unwrap(),
        );
        assert_eq!(f.files["/repo/src/b.rs"].reads_since_edit, 0);
        f.push(&Line::parse(r#"{"type":"file-history-delta","messageId":"m","trackingPath":"/repo/src/b.rs","backup":{"version":3},"timestamp":"t"}"#).unwrap());
        assert_eq!(f.files["/repo/src/b.rs"].checkpoint_version, Some(3));
        assert_eq!(
            bash_read_paths("cat a.rs b.rs | grep x; head -20 'src/c.rs'; ls src"),
            ["a.rs", "b.rs", "src/c.rs"]
        );
    }

    #[test]
    fn numstat() {
        let mut f = Files::default();
        f.push(&call("e1", "Edit", r#"{"file_path":"/repo/src/a.rs"}"#, 1));
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
