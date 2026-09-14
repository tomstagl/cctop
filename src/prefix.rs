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
    /// A system prompt section from `prompt_snapshot`.
    SystemPrompt,
    /// A plugin's always-on tokens from the catalog cache.
    Plugin,
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
    /// `prompt_snapshot`: system prompt sections (chars) and tool schemas
    /// (name, bytes) as sent; exact where present (2.1.266+).
    system_prompt_sections: Vec<usize>,
    schema_bytes: std::collections::BTreeMap<String, usize>,
    cli_prefix_chars: usize,
    /// `instructions` files (CLAUDE.md, AutoMem…): path → chars, as loaded.
    instruction_files: Vec<(String, String, usize)>,
    /// `nested_memory` re-injections: count and chars.
    nested_memory: (usize, usize),
    /// Skills invoked so far (`invoked_skills`).
    pub invoked_skills: Vec<String>,
    /// `/context`'s own table, when the person ran it.
    pub context_capture: Option<crate::transcript::ContextCapture>,
    /// Plugins: name → (always-on tokens, unused for N startups).
    plugins: Vec<(String, u64, Option<u64>)>,
}

impl Prefix {
    /// Fold a transcript line (attachments and the `/context` capture).
    pub fn push(&mut self, line: &Line) {
        if let Line::System(s) = line {
            if let Some(cap) = s.local_command().and_then(|c| c.context_capture()) {
                self.context_capture = Some(cap);
            }
            return;
        }
        let Line::Attachment(a) = line else { return };
        use crate::transcript::AttachmentKind;
        match a.kind() {
            AttachmentKind::PromptSnapshot {
                system_prompt_chars,
                tools,
                cli_prefix_chars,
            } => {
                self.system_prompt_sections = system_prompt_chars;
                self.schema_bytes = tools.into_iter().collect();
                self.cli_prefix_chars = cli_prefix_chars;
            }
            AttachmentKind::Instructions { files } => {
                self.instruction_files = files
                    .into_iter()
                    .map(|f| (f.path, f.kind, f.chars))
                    .collect();
            }
            AttachmentKind::NestedMemory { chars, .. } => {
                self.nested_memory.0 += 1;
                self.nested_memory.1 += chars;
            }
            AttachmentKind::InvokedSkills { skills } => {
                for s in skills {
                    if !self.invoked_skills.contains(&s) {
                        self.invoked_skills.push(s);
                    }
                }
            }
            _ => {}
        }
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

    /// Plugins enabled in settings, priced with the catalog cache
    /// (`tokens[model].always_on`) and flagged when unused for startups.
    pub fn scan_plugins(
        &mut self,
        settings: &Value,
        catalog: Option<&Value>,
        claude_json: Option<&Value>,
        model: &str,
    ) {
        self.plugins.clear();
        let Some(enabled) = settings.get("enabledPlugins").and_then(Value::as_object) else {
            return;
        };
        let startups = claude_json.and_then(crate::claude_home::num_startups);
        let usage = claude_json
            .and_then(|v| v.get("pluginUsage"))
            .and_then(Value::as_object);
        let catalog: Vec<&Value> = catalog
            .and_then(|c| c.get("catalog"))
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        for (key, on) in enabled {
            if on.as_bool() != Some(true) {
                continue;
            }
            let name = key.split('@').next().unwrap_or(key);
            let always_on = catalog
                .iter()
                .find(|e| e.get("plugin").and_then(Value::as_str) == Some(name))
                .and_then(|e| e.get("tokens"))
                .and_then(Value::as_object)
                .and_then(|t| {
                    fn family(m: &str) -> &str {
                        m.split('-').nth(1).unwrap_or("")
                    }
                    t.iter()
                        .find(|(m, _)| family(m) == family(model))
                        .or_else(|| t.iter().next())
                        .and_then(|(_, v)| v.get("always_on"))
                        .and_then(Value::as_u64)
                })
                .unwrap_or(0);
            let unused_for = usage
                .and_then(|u| u.get(key))
                .and_then(|u| u.get("lastUsedNumStartups"))
                .and_then(Value::as_u64)
                .and_then(|last| startups.map(|n| n.saturating_sub(last)))
                .or(startups);
            self.plugins.push((name.to_string(), always_on, unused_for));
        }
    }

    /// Enabled plugins: `(name, always-on tokens, unused for N startups)`.
    pub fn plugins(&self) -> &[(String, u64, Option<u64>)] {
        &self.plugins
    }

    /// Skills listing budget: Claude Code caps it at 1 % of the window's
    /// characters; `(used chars, budget chars)`.
    pub fn skills_budget(&self, window: u64) -> (u64, u64) {
        (self.skills_bytes, window * 4 / 100)
    }

    /// System prompt sections as sent (`prompt_snapshot`), tokens each.
    pub fn system_prompt_tokens(&self) -> Vec<u64> {
        self.system_prompt_sections
            .iter()
            .map(|c| (*c / 4) as u64)
            .collect()
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
        // Exact schema sizes from the snapshot replace the listing lines.
        if !self.schema_bytes.is_empty() {
            rows.retain(|r| {
                !matches!(r.kind, Kind::Tools | Kind::Mcp) || r.name.ends_with(" instructions")
            });
            let mut groups: std::collections::BTreeMap<String, (u64, usize)> = Default::default();
            for (name, bytes) in &self.schema_bytes {
                let key = match crate::tools::display_name(name) {
                    (d, Some(_)) => d,
                    _ => "built-in tools".to_string(),
                };
                let e = groups.entry(key).or_default();
                e.0 += *bytes as u64;
                e.1 += 1;
            }
            for (name, (bytes, count)) in groups {
                rows.push(Row {
                    kind: if name.starts_with("mcp:") {
                        Kind::Mcp
                    } else {
                        Kind::Tools
                    },
                    name: format!("{name} (schemas)"),
                    bytes,
                    tokens_est: bytes / 4,
                    count,
                });
            }
        }
        // Instruction files as loaded (exact chars), replacing the stat sizes.
        if !self.instruction_files.is_empty() {
            rows.retain(|r| r.kind != Kind::ClaudeMd);
            for (path, kind, chars) in &self.instruction_files {
                rows.push(Row {
                    kind: Kind::ClaudeMd,
                    name: format!("{} ({kind})", crate::ui::fmt::shorten_home(Path::new(path))),
                    bytes: *chars as u64,
                    tokens_est: (*chars / 4) as u64,
                    count: 1,
                });
            }
        }
        if self.nested_memory.0 > 0 {
            rows.push(Row {
                kind: Kind::Memory,
                name: "nested CLAUDE.md re-injections".into(),
                bytes: self.nested_memory.1 as u64,
                tokens_est: (self.nested_memory.1 / 4) as u64,
                count: self.nested_memory.0,
            });
        }
        for (i, chars) in self.system_prompt_sections.iter().enumerate() {
            rows.push(Row {
                kind: Kind::SystemPrompt,
                name: format!("system prompt §{}", i + 1),
                bytes: *chars as u64,
                tokens_est: (*chars / 4) as u64,
                count: 0,
            });
        }
        for (name, always_on, unused) in &self.plugins {
            let tag = match unused {
                Some(n) if *n >= 20 => format!(" · unused for {n} startups"),
                _ => String::new(),
            };
            rows.push(Row {
                kind: Kind::Plugin,
                name: format!("plugin {name}{tag}"),
                bytes: 0,
                tokens_est: *always_on,
                count: 0,
            });
        }
        let known: u64 = rows.iter().map(|r| r.tokens_est).sum();
        let other = first_call_tokens.saturating_sub(known);
        rows.push(Row {
            kind: Kind::Other,
            name: if self.system_prompt_sections.is_empty() {
                "system prompt + other".into()
            } else {
                "other".into()
            },
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
    crate::git::note_shellout();
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
    fn snapshot_instructions_plugins_and_the_context_capture() {
        let mut p = Prefix::default();
        for l in crate::transcript::parse_file(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl"),
        )
        .unwrap()
        {
            p.push(&l);
        }
        assert!(!p.system_prompt_sections.is_empty(), "prompt_snapshot");
        assert!(!p.schema_bytes.is_empty());
        assert!(!p.instruction_files.is_empty(), "instructions attachment");
        assert!(!p.invoked_skills.is_empty());
        let cap = p.context_capture.as_ref().expect("/context capture");
        assert!(cap.category("System prompt").is_some());
        let rows = p.rows(60_000);
        assert!(rows.iter().any(|r| r.kind == Kind::SystemPrompt));
        assert!(rows
            .iter()
            .any(|r| r.kind == Kind::Tools && r.name.ends_with("(schemas)")));
        assert!(rows
            .iter()
            .any(|r| r.kind == Kind::ClaudeMd && r.name.contains('(')));
        assert_eq!(rows.iter().filter(|r| r.kind == Kind::Other).count(), 1);
        assert_eq!(
            rows.last().map(|r| r.tokens_est <= rows[0].tokens_est),
            Some(true),
            "sorted"
        );
        // Plugins priced from the catalog cache, unused ones flagged.
        let settings = serde_json::json!({"enabledPlugins": {"cctop@cctop": true, "old@m": true, "off@m": false}});
        let catalog = serde_json::json!({"catalog": [{"plugin": "cctop", "tokens": {"claude-opus-4-7": {"always_on": 371, "on_invoke": 900}, "claude-sonnet-4-6": {"always_on": 300, "on_invoke": 800}}}]});
        let claude_json = serde_json::json!({"numStartups": 438, "pluginUsage": {"cctop@cctop": {"usageCount": 5, "lastUsedNumStartups": 437}, "old@m": {"usageCount": 1, "lastUsedNumStartups": 400}}});
        p.scan_plugins(
            &settings,
            Some(&catalog),
            Some(&claude_json),
            "claude-opus-5",
        );
        assert_eq!(p.plugins.len(), 2, "disabled plugins are not listed");
        assert_eq!(p.plugins[0], ("cctop".to_string(), 371, Some(1)));
        assert_eq!(p.plugins[1], ("old".to_string(), 0, Some(38)));
        let rows = p.rows(60_000);
        assert!(rows
            .iter()
            .any(|r| r.name == "plugin cctop" && r.tokens_est == 371));
        assert!(rows
            .iter()
            .any(|r| r.name == "plugin old · unused for 38 startups"));
        assert_eq!(p.skills_budget(1_000_000).1, 40_000);
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
