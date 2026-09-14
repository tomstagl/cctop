//! `~/.claude/history.jsonl`: what the person typed, one row per prompt
//! (`display`, `pastedContents`, `timestamp`, `project`, `sessionId`).
//! cctop reads it for the slash commands only — a `/model`, `/fast`,
//! `/effort`, `/clear`, `/compact`, `/loop`, `/goal` or `/rewind` row is a
//! habit signal the transcript does not carry the same way — and for the
//! size of a paste. Prompt text is never kept: a row that is not a slash
//! command survives as its timestamp and paste size alone.

use std::io::{BufRead, Seek};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The slash commands cctop keeps (with their first argument).
pub const KEPT: &[&str] = &[
    "/model", "/fast", "/effort", "/clear", "/compact", "/loop", "/goal", "/rewind", "/btw",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub at_ms: i64,
    pub session_id: String,
    pub project: String,
    /// `/model` … when the row was one of [`KEPT`]; `None` for a prompt.
    pub command: Option<String>,
    /// The first argument of a kept command (`sonnet`, `medium`, `5m`).
    pub arg: Option<String>,
    /// Characters pasted with the row (`pastedContents`), never their text.
    pub pasted_chars: usize,
}

/// Parse one line; prompt text is reduced to its paste size.
pub fn parse_row(line: &str) -> Option<Row> {
    let v: Value = serde_json::from_str(line).ok()?;
    let display = v.get("display").and_then(Value::as_str).unwrap_or("");
    let mut words = display.split_whitespace();
    let first = words.next().unwrap_or("");
    let (command, arg) = if KEPT.contains(&first) {
        (
            Some(first.to_string()),
            words
                .next()
                .filter(|a| !a.starts_with('-'))
                .map(|a| a.chars().take(24).collect::<String>()),
        )
    } else {
        (None, None)
    };
    let pasted_chars = v
        .get("pastedContents")
        .and_then(Value::as_object)
        .map(|m| {
            m.values()
                .map(|p| {
                    p.get("content")
                        .and_then(Value::as_str)
                        .map(|s| s.chars().count())
                        .unwrap_or(0)
                })
                .sum()
        })
        .unwrap_or(0);
    Some(Row {
        at_ms: v.get("timestamp").and_then(Value::as_i64)?,
        session_id: v
            .get("sessionId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        project: v
            .get("project")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        command,
        arg,
        pasted_chars,
    })
}

pub fn default_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/history.jsonl"))
}

/// Reads new rows from the file's last offset.
#[derive(Debug)]
pub struct Tailer {
    path: PathBuf,
    offset: u64,
}

impl Tailer {
    /// From the start: the whole file on the first poll (the session's own
    /// rows and the project's habits are both in the past).
    pub fn new(path: &Path) -> Tailer {
        Tailer {
            path: path.to_path_buf(),
            offset: 0,
        }
    }

    /// Rows appended since the last poll.
    pub fn poll(&mut self) -> Vec<Row> {
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            self.offset = 0; // truncated or rotated
        }
        if f.seek(std::io::SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut reader = std::io::BufReader::new(&mut f);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(n) => {
                    if !line.ends_with('\n') {
                        break; // a partial line: read it next time
                    }
                    self.offset += n as u64;
                    if let Some(r) = parse_row(line.trim_end()) {
                        out.push(r);
                    }
                }
                Err(_) => break,
            }
        }
        out
    }
}

/// What the history says about this session and its project.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    /// This session's kept commands, in order.
    pub commands: Vec<Row>,
    /// `/model` / `/fast` / `/effort` rows in this project, all sessions.
    pub project_switches: usize,
    /// `/clear` rows in this project, all sessions.
    pub project_clears: usize,
    /// `/compact` rows in this project, all sessions.
    pub project_compacts: usize,
    /// The last paste of this session: `(at_ms, chars)`.
    pub last_paste: Option<(i64, usize)>,
}

impl History {
    /// Fold rows for `session_id` in `project`.
    pub fn absorb(&mut self, rows: &[Row], session_id: &str, project: &Path) {
        let project = project.to_string_lossy();
        for r in rows {
            if r.project == project.as_ref() {
                match r.command.as_deref() {
                    Some("/model" | "/fast" | "/effort") => self.project_switches += 1,
                    Some("/clear") => self.project_clears += 1,
                    Some("/compact") => self.project_compacts += 1,
                    _ => {}
                }
            }
            if r.session_id == session_id {
                if r.pasted_chars > 0 {
                    self.last_paste = Some((r.at_ms, r.pasted_chars));
                }
                if r.command.is_some() {
                    self.commands.push(r.clone());
                }
            }
        }
    }

    /// This session's last `command`, if any.
    pub fn last(&self, command: &str) -> Option<&Row> {
        self.commands
            .iter()
            .rev()
            .find(|r| r.command.as_deref() == Some(command))
    }

    /// This session's `/model` and `/fast` rows after `since_ms`.
    pub fn switches_since(&self, since_ms: i64) -> Vec<&Row> {
        self.commands
            .iter()
            .filter(|r| {
                r.at_ms > since_ms && matches!(r.command.as_deref(), Some("/model" | "/fast"))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINES: &str = concat!(
        r#"{"display":"/model sonnet","pastedContents":{},"timestamp":1000,"project":"/p","sessionId":"s1"}"#,
        "\n",
        r#"{"display":"fix the parser please","pastedContents":{"1":{"id":1,"type":"text","content":"aaaa bbbb"}},"timestamp":2000,"project":"/p","sessionId":"s1"}"#,
        "\n",
        r#"{"display":"/clear","pastedContents":{},"timestamp":3000,"project":"/p","sessionId":"s2"}"#,
        "\n",
        r#"{"display":"/loop 5m check ci","pastedContents":{},"timestamp":4000,"project":"/q","sessionId":"s1"}"#,
        "\n",
    );

    #[test]
    fn rows_keep_commands_and_paste_sizes_never_text() {
        let rows: Vec<Row> = LINES.lines().filter_map(parse_row).collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0].command.as_deref(), Some("/model"));
        assert_eq!(rows[0].arg.as_deref(), Some("sonnet"));
        assert_eq!(rows[1].command, None);
        assert_eq!(rows[1].pasted_chars, 9);
        assert_eq!(rows[3].arg.as_deref(), Some("5m"));
        let debug = format!("{rows:?}");
        assert!(!debug.contains("fix the parser") && !debug.contains("aaaa"));
        let mut h = History::default();
        h.absorb(&rows, "s1", Path::new("/p"));
        assert_eq!(h.project_switches, 1);
        assert_eq!(
            h.project_clears, 1,
            "another session's /clear in the same project"
        );
        assert_eq!(
            h.commands.len(),
            2,
            "/model and /loop (the /loop is in another project but this session)"
        );
        assert_eq!(h.last_paste, Some((2000, 9)));
        assert_eq!(h.last("/loop").unwrap().at_ms, 4000);
        assert_eq!(h.switches_since(500).len(), 1);
    }

    #[test]
    fn tailer_reads_from_the_last_offset_and_waits_for_a_whole_line() {
        let dir = std::env::temp_dir().join(format!("cctop-history-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("history.jsonl");
        std::fs::write(&path, LINES).unwrap();
        let mut t = Tailer::new(&path);
        assert_eq!(t.poll().len(), 4);
        assert!(t.poll().is_empty(), "nothing new");
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        use std::io::Write;
        write!(f, r#"{{"display":"/compact focus","pastedContents":{{}},"timestamp":5000,"project":"/p","sessionId":"s1"}}"#).unwrap();
        assert!(t.poll().is_empty(), "a partial line waits");
        writeln!(f).unwrap();
        let rows = t.poll();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].command.as_deref(), Some("/compact"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
