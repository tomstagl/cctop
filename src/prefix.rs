//! What rides on every request: the fixed prefix (system prompt, CLAUDE.md
//! files, tool schemas, skills, MCP instructions, memory index). Sizes come
//! from the files on disk and from the listing attachments Claude Code writes
//! to the transcript; the remainder against the first API call is "other".

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::transcript::Line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ClaudeMd,
    Tools,
    Mcp,
    Skills,
    Agents,
    Memory,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub name: String,
    pub bytes: u64,
    /// `bytes / 4`, or the reconciled remainder for `Other`.
    pub tokens_est: u64,
    /// Tool count for tool rows.
    pub count: usize,
}

#[derive(Debug, Default)]
pub struct Prefix {
    /// From transcript attachments; refreshed as deltas arrive.
    tool_lines: std::collections::BTreeMap<String, u64>, // tool name → bytes
    mcp_instruction_bytes: std::collections::BTreeMap<String, u64>,
    skills_bytes: u64,
    skills_count: usize,
    agents_bytes: u64,
    agents_count: usize,
    /// From disk (refreshed by `scan`).
    files: Vec<(String, u64)>,
    memory_bytes: Option<u64>,
}

impl Prefix {
    /// Fold a transcript line (only `attachment` lines matter).
    pub fn push(&mut self, line: &Line) {
        let Line::Attachment(a) = line else { return };
        let v = &a.attachment;
        match v.get("type").and_then(Value::as_str) {
            Some("deferred_tools_delta") => {
                let names = v.get("addedNames").and_then(Value::as_array);
                let lines = v.get("addedLines").and_then(Value::as_array);
                if let (Some(names), Some(lines)) = (names, lines) {
                    for (n, l) in names.iter().zip(lines) {
                        if let (Some(n), Some(l)) = (n.as_str(), l.as_str()) {
                            self.tool_lines.insert(n.to_string(), l.len() as u64);
                        }
                    }
                }
                for n in v
                    .get("removedNames")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    self.tool_lines.remove(n);
                }
            }
            Some("mcp_instructions_delta") => {
                let names = v.get("addedNames").and_then(Value::as_array);
                let blocks = v.get("addedBlocks").and_then(Value::as_array);
                if let (Some(names), Some(blocks)) = (names, blocks) {
                    for (n, b) in names.iter().zip(blocks) {
                        if let Some(n) = n.as_str() {
                            let bytes = match b {
                                Value::String(s) => s.len(),
                                other => other.to_string().len(),
                            };
                            self.mcp_instruction_bytes
                                .insert(n.to_string(), bytes as u64);
                        }
                    }
                }
            }
            Some("skill_listing") => {
                self.skills_bytes = v
                    .get("content")
                    .and_then(Value::as_str)
                    .map(|s| s.len() as u64)
                    .unwrap_or(0);
                self.skills_count =
                    v.get("skillCount").and_then(Value::as_u64).unwrap_or(0) as usize;
            }
            Some("agent_listing_delta") => {
                let lines = v.get("addedLines").and_then(Value::as_array);
                self.agents_bytes += lines
                    .map(|l| {
                        l.iter()
                            .filter_map(Value::as_str)
                            .map(|s| s.len() as u64)
                            .sum()
                    })
                    .unwrap_or(0);
                self.agents_count += v
                    .get("addedTypes")
                    .and_then(Value::as_array)
                    .map(|a| a.len())
                    .unwrap_or(0);
            }
            _ => {}
        }
    }

    /// Stat the CLAUDE.md files and the memory index for `cwd`.
    pub fn scan(&mut self, cwd: &Path, home: Option<&Path>) {
        self.files = claude_md_paths(cwd, home)
            .into_iter()
            .filter_map(|p| {
                std::fs::metadata(&p)
                    .ok()
                    .map(|m| (crate::ui::fmt::shorten_home(&p), m.len()))
            })
            .collect();
        self.memory_bytes = home
            .map(|h| {
                h.join(".claude/projects")
                    .join(crate::slug(cwd))
                    .join("memory/MEMORY.md")
            })
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len());
    }

    /// Schema tokens and tool count per MCP server (`mcp:<server>` keys).
    pub fn mcp_schema_tokens(&self) -> std::collections::BTreeMap<String, (u64, usize)> {
        let mut out: std::collections::BTreeMap<String, (u64, usize)> = Default::default();
        for (name, bytes) in &self.tool_lines {
            if let (d, Some(_)) = crate::tools::display_name(name) {
                let e = out.entry(d).or_default();
                e.0 += bytes / 4;
                e.1 += 1;
            }
        }
        out
    }

    /// Rows sorted by size, with the remainder against `first_call_tokens` as `other`.
    pub fn rows(&self, first_call_tokens: u64) -> Vec<Row> {
        let mut rows = Vec::new();
        for (name, bytes) in &self.files {
            rows.push(Row {
                kind: Kind::ClaudeMd,
                name: name.clone(),
                bytes: *bytes,
                tokens_est: bytes / 4,
                count: 1,
            });
        }
        // Tools grouped: built-in vs each MCP server.
        let mut groups: std::collections::BTreeMap<String, (u64, usize)> = Default::default();
        for (name, bytes) in &self.tool_lines {
            let key = match crate::tools::display_name(name) {
                (d, Some(_)) => d,
                _ => "built-in tools".to_string(),
            };
            let e = groups.entry(key).or_default();
            e.0 += bytes;
            e.1 += 1;
        }
        for (name, (bytes, count)) in groups {
            let kind = if name.starts_with("mcp:") {
                Kind::Mcp
            } else {
                Kind::Tools
            };
            rows.push(Row {
                kind,
                name,
                bytes,
                tokens_est: bytes / 4,
                count,
            });
        }
        for (name, bytes) in &self.mcp_instruction_bytes {
            rows.push(Row {
                kind: Kind::Mcp,
                name: format!("mcp:{name} instructions"),
                bytes: *bytes,
                tokens_est: bytes / 4,
                count: 0,
            });
        }
        if self.skills_bytes > 0 {
            rows.push(Row {
                kind: Kind::Skills,
                name: "skills listing".into(),
                bytes: self.skills_bytes,
                tokens_est: self.skills_bytes / 4,
                count: self.skills_count,
            });
        }
        if self.agents_bytes > 0 {
            rows.push(Row {
                kind: Kind::Agents,
                name: "agent listing".into(),
                bytes: self.agents_bytes,
                tokens_est: self.agents_bytes / 4,
                count: self.agents_count,
            });
        }
        if let Some(b) = self.memory_bytes {
            rows.push(Row {
                kind: Kind::Memory,
                name: "memory index".into(),
                bytes: b,
                tokens_est: b / 4,
                count: 1,
            });
        }
        let known: u64 = rows.iter().map(|r| r.tokens_est).sum();
        let other = first_call_tokens.saturating_sub(known);
        rows.push(Row {
            kind: Kind::Other,
            name: "system prompt + other".into(),
            bytes: 0,
            tokens_est: other,
            count: 0,
        });
        rows.sort_by_key(|r| std::cmp::Reverse(r.tokens_est));
        rows
    }
}

/// `<cwd>/CLAUDE.md` and `<cwd>/.claude/CLAUDE.md`, the same in each parent up to
/// the git root, then `~/.claude/CLAUDE.md`. Only paths that exist are returned.
pub fn claude_md_paths(cwd: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let root = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or_else(|| cwd.to_path_buf());
    let mut out = Vec::new();
    let mut dir = Some(cwd);
    while let Some(d) = dir {
        for cand in [
            d.join("CLAUDE.md"),
            d.join(".claude/CLAUDE.md"),
            d.join("CLAUDE.local.md"),
        ] {
            if cand.is_file() {
                out.push(cand);
            }
        }
        if d == root {
            break;
        }
        dir = d.parent();
    }
    if let Some(h) = home {
        let global = h.join(".claude/CLAUDE.md");
        if global.is_file() {
            out.push(global);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(v: Value) -> Line {
        Line::from_value(serde_json::json!({"type":"attachment","attachment":v}))
    }

    #[test]
    fn rows_from_attachments_and_files_reconcile() {
        let mut p = Prefix::default();
        p.push(&attachment(serde_json::json!({"type":"deferred_tools_delta","addedNames":["Read","mcp__github__get_me","mcp__github__list_issues"],"addedLines":["Read: 40 chars xxxxxxxxxxxxxxxxxxxxxxxx","mcp__github__get_me: xxxxxxxxxx","mcp__github__list_issues: xxxxxxxxxxxx"]})));
        p.push(&attachment(serde_json::json!({"type":"mcp_instructions_delta","addedNames":["github"],"addedBlocks":["x".repeat(400)]})));
        p.push(&attachment(
            serde_json::json!({"type":"skill_listing","content":"s".repeat(8000),"skillCount":47}),
        ));
        p.push(&attachment(serde_json::json!({"type":"agent_listing_delta","addedTypes":["a","b"],"addedLines":["aaaa","bbbb"]})));
        let dir = std::env::temp_dir().join(format!("cctop-prefix-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "c".repeat(2000)).unwrap();
        std::fs::write(dir.join(".claude/CLAUDE.md"), "d".repeat(400)).unwrap();
        p.scan(&dir, None);
        let rows = p.rows(10_000);
        let by = |name: &str| rows.iter().find(|r| r.name == name).cloned().unwrap();
        assert_eq!(by("skills listing").tokens_est, 2000);
        assert_eq!(by("skills listing").count, 47);
        assert_eq!(by("mcp:github").count, 2);
        assert_eq!(by("built-in tools").count, 1);
        assert_eq!(by("mcp:github instructions").tokens_est, 100);
        assert_eq!(by("agent listing").tokens_est, 2);
        assert!(rows
            .iter()
            .any(|r| r.kind == Kind::ClaudeMd && r.tokens_est == 500));
        assert!(rows
            .iter()
            .any(|r| r.kind == Kind::ClaudeMd && r.tokens_est == 100));
        let known: u64 = rows
            .iter()
            .filter(|r| r.kind != Kind::Other)
            .map(|r| r.tokens_est)
            .sum();
        assert_eq!(by("system prompt + other").tokens_est, 10_000 - known);
        assert!(
            rows.windows(2).all(|w| w[0].tokens_est >= w[1].tokens_est),
            "sorted by size"
        );
        // Removal deltas drop tools.
        p.push(&attachment(serde_json::json!({"type":"deferred_tools_delta","addedNames":[],"addedLines":[],"removedNames":["Read"]})));
        assert!(!p.rows(0).iter().any(|r| r.name == "built-in tools"));
    }

    #[test]
    fn claude_md_lookup_walks_to_git_root_and_home() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
        let paths = claude_md_paths(&repo.join("src"), None);
        assert!(paths.iter().all(|p| p.starts_with(repo)));
    }
}
