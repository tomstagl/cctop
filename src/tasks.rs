//! Background tasks. On 2.1.269 the directory
//! `~/.claude/tasks/session-<id8>/` holds only `.lock` / `.highwatermark`;
//! the list itself arrives in the `Stop` hook's `background_tasks` (see
//! [`crate::ui::state::State::refresh_tasks`]). The directory scan stays for
//! older versions and skips dotfiles; each file is read generically (id =
//! stem, kind = extension or a `kind` field if the file is JSON, description
//! = first line) so a format change never breaks the panel.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: String,
    pub kind: String,
    pub description: String,
    /// Epoch ms of the file's creation (mtime as a fallback).
    pub started_at_ms: i64,
    /// Present in the file when it is JSON with a `status` field.
    pub status: Option<String>,
}

/// `~/.claude/tasks/session-<first 8 chars of the session id>/`.
pub fn dir_for(session_id: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let short: String = session_id.chars().take(8).collect();
    Some(
        PathBuf::from(home)
            .join(".claude/tasks")
            .join(format!("session-{short}")),
    )
}

/// Read every file in `dir` as a task, newest first.
pub fn load(dir: &Path) -> Vec<Task> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut tasks: Vec<Task> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| read_task(&e.path()))
        .collect();
    tasks.sort_by_key(|t| std::cmp::Reverse(t.started_at_ms));
    tasks
}

fn read_task(path: &Path) -> Option<Task> {
    let id = path.file_stem()?.to_string_lossy().into_owned();
    let mut kind = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "task".into());
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let mut description = text.lines().next().unwrap_or("").trim().to_string();
    let mut status = None;
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        for k in ["kind", "type"] {
            if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
                kind = s.to_string();
            }
        }
        for k in ["description", "command", "prompt", "title"] {
            if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
                description = s.to_string();
                break;
            }
        }
        status = v.get("status").and_then(|x| x.as_str()).map(str::to_string);
    }
    let meta = std::fs::metadata(path).ok()?;
    let started = meta
        .created()
        .or_else(|_| meta.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some(Task {
        id,
        kind,
        description: crate::ui::fmt::clip(&description, 60),
        started_at_ms: started,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_json_and_plain_files() {
        let dir = std::env::temp_dir().join(format!("cctop-tasks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("b7f3.json"),
            r#"{"kind":"bash","command":"cargo build --release","status":"running"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("m1.output"), "first line of output\nsecond\n").unwrap();
        std::fs::write(dir.join(".lock"), "").unwrap();
        std::fs::write(dir.join(".highwatermark"), "12").unwrap();
        let tasks = load(&dir);
        assert_eq!(tasks.len(), 2, "dotfiles are bookkeeping, not tasks");
        let bash = tasks.iter().find(|t| t.id == "b7f3").unwrap();
        assert_eq!(bash.kind, "bash");
        assert_eq!(bash.description, "cargo build --release");
        assert_eq!(bash.status.as_deref(), Some("running"));
        let out = tasks.iter().find(|t| t.id == "m1").unwrap();
        assert_eq!(out.kind, "output");
        assert_eq!(out.description, "first line of output");
        assert!(out.started_at_ms > 0);
        assert!(load(Path::new("/nonexistent")).is_empty());
        assert!(dir_for("12f423aa-fbba")
            .unwrap()
            .ends_with("session-12f423aa"));
    }
}
