//! The session's process tree: the `claude` process, its MCP servers and the
//! command it is running right now. Sampled through `ps` (portable, cheap)
//! behind a trait so tests inject a fake table.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub cpu_pct: f32,
    pub rss_bytes: u64,
    /// Seconds since the process started.
    pub elapsed_s: u64,
    pub cmdline: String,
}

/// Source of process rows.
pub trait ProcessTable {
    fn all(&self) -> Vec<Proc>;
}

/// `ps -axo pid,ppid,pcpu,rss,etime,command`.
pub struct Ps;

impl ProcessTable for Ps {
    fn all(&self) -> Vec<Proc> {
        let Ok(out) = Command::new("ps")
            .args(["-axo", "pid=,ppid=,pcpu=,rss=,etime=,command="])
            .output()
        else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(parse_ps_line)
            .collect()
    }
}

fn parse_ps_line(line: &str) -> Option<Proc> {
    let mut it = line.split_whitespace();
    let pid = it.next()?.parse().ok()?;
    let ppid = it.next()?.parse().ok()?;
    let cpu_pct: f32 = it.next()?.replace(',', ".").parse().ok()?;
    let rss_kb: u64 = it.next()?.parse().ok()?;
    let elapsed_s = parse_etime(it.next()?)?;
    let cmdline = it.collect::<Vec<_>>().join(" ");
    Some(Proc {
        pid,
        ppid,
        cpu_pct,
        rss_bytes: rss_kb * 1024,
        elapsed_s,
        cmdline,
    })
}

/// `[[dd-]hh:]mm:ss` → seconds.
fn parse_etime(s: &str) -> Option<u64> {
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, s),
    };
    let parts: Vec<u64> = rest
        .split(':')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let secs = match parts.as_slice() {
        [m, s] => m * 60 + s,
        [h, m, s] => h * 3600 + m * 60 + s,
        _ => return None,
    };
    Some(days * 86_400 + secs)
}

/// One configured MCP server (`command` + `args` from any of the config files).
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct McpConfig {
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
struct McpFile {
    #[serde(default, rename = "mcpServers")]
    mcp_servers: BTreeMap<String, McpConfig>,
}

/// Servers from `~/.claude/mcp.json`, `~/.claude/settings.json` and
/// `<cwd>/.mcp.json`, later files overriding earlier ones by name.
pub fn mcp_configs(cwd: &Path) -> BTreeMap<String, McpConfig> {
    let mut out = BTreeMap::new();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let mut files = Vec::new();
    if let Some(h) = &home {
        files.push(h.join(".claude/mcp.json"));
        files.push(h.join(".claude/settings.json"));
    }
    files.push(cwd.join(".mcp.json"));
    for f in files {
        if let Ok(text) = std::fs::read_to_string(&f) {
            if let Ok(parsed) = serde_json::from_str::<McpFile>(&text) {
                out.extend(parsed.mcp_servers);
            }
        }
    }
    out
}

/// How well `cmdline` matches a configured server: the number of
/// non-flag args found in it, plus one for the command's basename. `npx -y
/// @scope/pkg` runs as `npm exec @scope/pkg`, so args carry the signal.
fn match_score(cmdline: &str, cfg: &McpConfig) -> usize {
    let mut score = 0;
    let base = Path::new(&cfg.command)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !base.is_empty() && base != "npx" && cmdline.contains(&base) {
        score += 1;
    }
    score += cfg
        .args
        .iter()
        .filter(|a| !a.starts_with('-') && a.len() > 2 && cmdline.contains(a.as_str()))
        .count();
    score
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpServer {
    pub name: String,
    pub pid: u32,
    pub rss_bytes: u64,
    pub cpu_pct: f32,
    pub elapsed_s: u64,
    pub restarts: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunningCommand {
    pub pid: u32,
    pub cmdline: String,
    pub elapsed_s: u64,
}

/// One sample of the session's tree.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// The `claude` process itself, if found.
    pub main: Option<Proc>,
    pub mcp: Vec<McpServer>,
    /// Servers configured but with no process this sample.
    pub mcp_missing: Vec<String>,
    pub running_command: Option<RunningCommand>,
    /// Every descendant, for the curious.
    pub descendants: Vec<Proc>,
}

/// Keeps per-server pids across samples to count restarts.
#[derive(Debug, Default)]
pub struct Sampler {
    last_pid: HashMap<String, u32>,
    restarts: HashMap<String, u32>,
    /// Servers that had a process last sample and none now.
    pub exited: Vec<String>,
}

impl Sampler {
    pub fn snapshot(
        &mut self,
        table: &dyn ProcessTable,
        session_pid: u32,
        configs: &BTreeMap<String, McpConfig>,
    ) -> Snapshot {
        let all = table.all();
        let main = all.iter().find(|p| p.pid == session_pid).cloned();
        let descendants = descendants_of(&all, session_pid);
        let children: Vec<&Proc> = descendants
            .iter()
            .filter(|p| p.ppid == session_pid)
            .collect();

        // Match each configured server to the best-scoring direct child.
        let mut mcp = Vec::new();
        let mut used: Vec<u32> = Vec::new();
        for (name, cfg) in configs {
            let best = children
                .iter()
                .filter(|p| !used.contains(&p.pid))
                .map(|p| (match_score(&p.cmdline, cfg), *p))
                .filter(|(s, _)| *s > 0)
                .max_by_key(|(s, _)| *s);
            if let Some((_, p)) = best {
                used.push(p.pid);
                let restarts = match self.last_pid.get(name) {
                    Some(&prev) if prev != p.pid => {
                        *self.restarts.entry(name.clone()).or_insert(0) += 1;
                        self.restarts[name]
                    }
                    _ => self.restarts.get(name).copied().unwrap_or(0),
                };
                self.last_pid.insert(name.clone(), p.pid);
                mcp.push(McpServer {
                    name: name.clone(),
                    pid: p.pid,
                    rss_bytes: p.rss_bytes,
                    cpu_pct: p.cpu_pct,
                    elapsed_s: p.elapsed_s,
                    restarts,
                });
            }
        }
        let mcp_missing: Vec<String> = configs
            .keys()
            .filter(|n| !mcp.iter().any(|m| &m.name == *n))
            .cloned()
            .collect();
        self.exited = mcp_missing
            .iter()
            .filter(|n| self.last_pid.contains_key(*n))
            .cloned()
            .collect();
        for n in &self.exited {
            self.last_pid.remove(n);
        }

        // The foreground command: a shell child that is not an MCP server.
        let running_command = children
            .iter()
            .filter(|p| !used.contains(&p.pid))
            .filter(|p| is_shell(&p.cmdline))
            .max_by_key(|p| p.pid) // newest
            .map(|p| RunningCommand {
                pid: p.pid,
                cmdline: strip_shell(&p.cmdline),
                elapsed_s: p.elapsed_s,
            });

        Snapshot {
            main,
            mcp,
            mcp_missing,
            running_command,
            descendants,
        }
    }
}

fn descendants_of(all: &[Proc], root: u32) -> Vec<Proc> {
    let mut out = Vec::new();
    let mut frontier = vec![root];
    while let Some(parent) = frontier.pop() {
        for p in all.iter().filter(|p| p.ppid == parent) {
            frontier.push(p.pid);
            out.push(p.clone());
        }
    }
    out
}

fn is_shell(cmdline: &str) -> bool {
    let first = cmdline.split_whitespace().next().unwrap_or("");
    let base = Path::new(first)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    matches!(base.as_str(), "zsh" | "bash" | "sh" | "fish" | "dash")
}

/// `/bin/zsh -c 'cargo test'` → `cargo test`.
fn strip_shell(cmdline: &str) -> String {
    let mut it = cmdline.split_whitespace();
    it.next();
    let rest: Vec<&str> = it.collect();
    let rest = match rest.as_slice() {
        ["-c", cmd @ ..] | ["-lc", cmd @ ..] | ["-ic", cmd @ ..] => cmd.join(" "),
        other => other.join(" "),
    };
    rest.trim_matches(|c| c == '\'' || c == '"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake(Vec<Proc>);
    impl ProcessTable for Fake {
        fn all(&self) -> Vec<Proc> {
            self.0.clone()
        }
    }

    fn p(pid: u32, ppid: u32, rss_kb: u64, elapsed_s: u64, cmd: &str) -> Proc {
        Proc {
            pid,
            ppid,
            cpu_pct: 1.5,
            rss_bytes: rss_kb * 1024,
            elapsed_s,
            cmdline: cmd.into(),
        }
    }

    fn configs() -> BTreeMap<String, McpConfig> {
        BTreeMap::from([
            (
                "Playwright".to_string(),
                McpConfig {
                    command: "npx".into(),
                    args: vec!["-y".into(), "@playwright/mcp@latest".into()],
                },
            ),
            (
                "github".to_string(),
                McpConfig {
                    command: "/usr/local/bin/github-mcp-server".into(),
                    args: vec!["stdio".into()],
                },
            ),
            (
                "never-started".to_string(),
                McpConfig {
                    command: "ghost".into(),
                    args: vec![],
                },
            ),
        ])
    }

    fn table() -> Vec<Proc> {
        vec![
            p(1, 0, 100, 99_999, "/sbin/launchd"),
            p(100, 1, 550_000, 5_000, "claude"),
            p(101, 100, 2_000, 5_000, "caffeinate -i -t 300"),
            p(102, 100, 45_000, 4_990, "npm exec @playwright/mcp@latest"),
            p(103, 100, 30_000, 4_990, "github-mcp-server stdio"),
            p(104, 100, 3_000, 48, "/bin/zsh -c 'cargo test --workspace'"),
            p(105, 104, 200_000, 47, "cargo test --workspace"),
            p(200, 1, 1_000, 10, "/bin/zsh -c 'unrelated'"),
        ]
    }

    #[test]
    fn snapshot_finds_main_mcp_and_running_command() {
        let mut s = Sampler::default();
        let snap = s.snapshot(&Fake(table()), 100, &configs());
        assert_eq!(snap.main.as_ref().unwrap().rss_bytes, 550_000 * 1024);
        assert_eq!(snap.descendants.len(), 5);
        let names: Vec<_> = snap.mcp.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Playwright", "github"]);
        assert_eq!(snap.mcp[0].pid, 102);
        assert_eq!(snap.mcp[1].pid, 103);
        assert_eq!(snap.mcp_missing, vec!["never-started".to_string()]);
        let rc = snap.running_command.unwrap();
        assert_eq!(rc.pid, 104);
        assert_eq!(rc.cmdline, "cargo test --workspace");
        assert_eq!(rc.elapsed_s, 48);
        assert!(s.exited.is_empty());
    }

    #[test]
    fn restarts_and_exits_are_tracked_across_samples() {
        let mut s = Sampler::default();
        s.snapshot(&Fake(table()), 100, &configs());
        // Playwright restarted with a new pid; github is gone.
        let mut t2: Vec<Proc> = table()
            .into_iter()
            .filter(|p| p.pid != 102 && p.pid != 103)
            .collect();
        t2.push(p(150, 100, 40_000, 3, "npm exec @playwright/mcp@latest"));
        let snap = s.snapshot(&Fake(t2.clone()), 100, &configs());
        let pw = snap.mcp.iter().find(|m| m.name == "Playwright").unwrap();
        assert_eq!(pw.pid, 150);
        assert_eq!(pw.restarts, 1);
        assert_eq!(s.exited, vec!["github".to_string()]);
        // Next sample: github still missing but no longer "just exited".
        let snap = s.snapshot(&Fake(t2), 100, &configs());
        assert!(s.exited.is_empty());
        assert!(snap.mcp_missing.contains(&"github".to_string()));
    }

    #[test]
    fn ps_line_and_etime_parsing() {
        let p =
            parse_ps_line("86697 86143   0,0  45196    01:02:03 npm exec @playwright/mcp@latest")
                .unwrap();
        assert_eq!((p.pid, p.ppid), (86697, 86143));
        assert_eq!(p.rss_bytes, 45196 * 1024);
        assert_eq!(p.elapsed_s, 3723);
        assert_eq!(p.cmdline, "npm exec @playwright/mcp@latest");
        assert_eq!(parse_etime("05:07"), Some(307));
        assert_eq!(parse_etime("2-01:00:00"), Some(2 * 86_400 + 3600));
        assert_eq!(parse_etime("x"), None);
        assert!(is_shell("/bin/zsh -c ls"));
        assert!(!is_shell("cargo test"));
        assert_eq!(strip_shell("/bin/bash -lc \"make check\""), "make check");
    }

    #[test]
    fn live_ps_returns_this_process() {
        let me = std::process::id();
        assert!(Ps.all().iter().any(|p| p.pid == me));
    }
}
