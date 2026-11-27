//! Hook events. `cctop install` registers `cctop hook` for the events below;
//! the hook appends one JSON line per event to `~/.cctop/events/<session>.jsonl`
//! and cctop tails that spool for exact timings, permission waits and
//! compactions.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "UserPromptExpansion",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PostToolBatch",
    "PermissionRequest",
    "PermissionDenied",
    "Notification",
    "SubagentStart",
    "SubagentStop",
    "PreCompact",
    "PostCompact",
    "PreModelSwitch",
    "PostModelSwitch",
    "InstructionsLoaded",
    "TaskCreated",
    "TaskCompleted",
    "Stop",
    "StopFailure",
    "SessionEnd",
    // Added in 0.10: each is safe for a hook that prints nothing and exits 0.
    "Elicitation",
    "ElicitationResult",
    "ConfigChange",
    "CwdChanged",
    "DirectoryAdded",
    "TeammateIdle",
    "PreSkillShellCommand",
];

/// Events cctop never subscribes to: a registered hook changes Claude Code's
/// behaviour or has no value for a silent observer. `without_hooks` still
/// strips cctop's entry from them, in case an older or hand-edited install
/// put one there.
pub const NEVER: &[&str] = &[
    // The hook replaces worktree creation (stdout is the path): silence breaks it.
    "WorktreeCreate",
    // Same family: the hook performs the removal.
    "WorktreeRemove",
    // Carries the assistant's text and can replace what is displayed: prose, in the display path.
    "MessageDisplay",
    // Fires only for init/maintenance; exit-0 stdout goes to Claude as context.
    "Setup",
    // Needs a matcher naming files to watch, which cctop has no reason to choose.
    "FileChanged",
];

/// Bound on the command text kept from a `tool_input` or a background task.
const COMMAND_CHARS: usize = 200;

pub const HOOK_COMMAND: &str = "cctop hook";
/// Spool files older than this are pruned on start.
pub const PRUNE_AFTER_MS: i64 = 7 * 24 * 3600 * 1000;

pub fn spool_dir(home: &Path) -> PathBuf {
    home.join("events")
}

pub fn spool_path(home: &Path, session_id: &str) -> PathBuf {
    spool_dir(home).join(format!("{session_id}.jsonl"))
}

/// One spooled event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HookEvent {
    /// Epoch ms when the hook ran.
    pub at: i64,
    pub event: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// The hook's stdin, minus the bulky fields.
    #[serde(default)]
    pub payload: Value,
}

impl HookEvent {
    /// Build from the JSON Claude Code pipes to a hook.
    pub fn from_stdin(at: i64, mut v: Value) -> Option<HookEvent> {
        let event = v.get("hook_event_name")?.as_str()?.to_string();
        let session_id = v
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let tool_use_id = v
            .get("tool_use_id")
            .and_then(Value::as_str)
            .map(str::to_string);
        let tool_name = v
            .get("tool_name")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(o) = v.as_object_mut() {
            // Keep the spool small and free of text: results and inputs
            // live in the transcript; prompts are measured, never kept.
            let is_agent = tool_name.as_deref() == Some("Agent");
            if let Some(resp) = o.remove("tool_response") {
                if is_agent {
                    o.insert("tool_response".into(), agent_response_facts(&resp));
                }
            }
            if let Some(input) = o.remove("tool_input") {
                if event == "PreSkillShellCommand" {
                    // Only the bounded command survives; the rest of the input is text.
                    if let Some(cmd) = input.get("command").and_then(Value::as_str) {
                        o.insert(
                            "tool_input".into(),
                            serde_json::json!({ "command": bounded(cmd, COMMAND_CHARS) }),
                        );
                    }
                } else if let Some(cmd) = input.get("command").and_then(Value::as_str) {
                    o.insert("command".into(), Value::String(bounded(cmd, COMMAND_CHARS)));
                }
            }
            o.remove("transcript_path");
            // MCP prompts: measured, never kept. Other events' `message`
            // (a Notification's own text) is not the person's and stays.
            if event == "Elicitation" {
                if let Some(Value::String(text)) = o.remove("message") {
                    o.insert("message_chars".into(), Value::from(text.chars().count()));
                }
                if let Some(schema) = o.remove("requested_schema") {
                    let n = schema
                        .get("properties")
                        .and_then(Value::as_object)
                        .map_or(0, |p| p.len());
                    o.insert("schema_fields".into(), Value::from(n));
                }
                // A URL-mode prompt's link can carry a token: only its presence stays.
                if let Some(url) = o.remove("url") {
                    o.insert("has_url".into(), Value::Bool(!url.is_null()));
                }
            }
            if event == "ElicitationResult" {
                let n = o
                    .remove("content")
                    .and_then(|c| c.as_object().map(|m| m.len()))
                    .unwrap_or(0);
                o.insert("content_fields".into(), Value::from(n));
            }
            for key in ["prompt", "last_assistant_message"] {
                if let Some(Value::String(text)) = o.remove(key) {
                    o.insert(format!("{key}_chars"), Value::from(text.chars().count()));
                    o.insert(
                        format!("{key}_ends_with_question"),
                        Value::Bool(text.trim_end().ends_with('?')),
                    );
                }
            }
            if let Some(Value::Array(tasks)) = o.get_mut("background_tasks") {
                for t in tasks.iter_mut() {
                    if let Some(cmd) = t.get("command").and_then(Value::as_str) {
                        let short = bounded(cmd, COMMAND_CHARS);
                        t["command"] = Value::String(short);
                    }
                }
            }
        }
        Some(HookEvent {
            at,
            event,
            session_id,
            tool_use_id,
            tool_name,
            payload: v,
        })
    }
}

/// Clip to `max` characters, marking the cut.
fn bounded(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max - 1).collect::<String>())
    }
}

/// What a finished `Agent` reports, without its text: the model it ran on,
/// its usage and how many tools it called.
fn agent_response_facts(resp: &Value) -> Value {
    let mut out = serde_json::Map::new();
    for k in [
        "resolvedModel",
        "totalToolUseCount",
        "usage",
        "status",
        "agentId",
    ] {
        if let Some(v) = resp.get(k) {
            out.insert(k.into(), v.clone());
        }
    }
    if let Some(stats) = resp.get("toolStats") {
        out.insert("toolStats".into(), stats.clone());
    }
    Value::Object(out)
}

/// Append `payload` (the hook's stdin) to the spool. Never errors.
pub fn record(home: &Path, at: i64, payload: &[u8]) -> Option<PathBuf> {
    let v: Value = serde_json::from_slice(payload).ok()?;
    let ev = HookEvent::from_stdin(at, v)?;
    if ev.session_id.is_empty() {
        return None;
    }
    let path = spool_path(home, &ev.session_id);
    std::fs::create_dir_all(path.parent()?).ok()?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    let line = serde_json::to_string(&ev).ok()?;
    f.write_all(line.as_bytes()).ok()?;
    f.write_all(b"\n").ok()?;
    Some(path)
}

/// `cctop hook`: the hook entry point. Always exits 0. On `SessionEnd` it
/// also kicks off `cctop report` for the session, detached, so the hook
/// itself stays instant.
pub fn run_hook() -> i32 {
    let mut payload = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut payload);
    let _ = record(&crate::status::cctop_dir(), crate::app::now_ms(), &payload);
    if let Ok(v) = serde_json::from_slice::<Value>(&payload) {
        if v.get("hook_event_name").and_then(Value::as_str) == Some("SessionEnd") {
            if let Some(id) = v.get("session_id").and_then(Value::as_str) {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe)
                        .args(["report", "--session", id])
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            }
        }
    }
    0
}

/// Delete spool files untouched for longer than [`PRUNE_AFTER_MS`].
pub fn prune(home: &Path, now_ms: i64) -> usize {
    let Ok(entries) = std::fs::read_dir(spool_dir(home)) else {
        return 0;
    };
    let mut n = 0;
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        let age = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| now_ms - d.as_millis() as i64)
            .unwrap_or(0);
        if age > PRUNE_AFTER_MS && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// Tails a session's spool by byte offset (the file is append-only).
#[derive(Debug)]
pub struct Watcher {
    path: PathBuf,
    offset: u64,
    partial: String,
}

impl Watcher {
    pub fn new(home: &Path, session_id: &str) -> Watcher {
        Watcher {
            path: spool_path(home, session_id),
            offset: 0,
            partial: String::new(),
        }
    }

    /// New events since the last poll.
    pub fn poll(&mut self) -> Vec<HookEvent> {
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        use std::io::Seek;
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset {
            return Vec::new();
        }
        if f.seek(std::io::SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut buf = String::new();
        if f.read_to_string(&mut buf).is_err() {
            return Vec::new();
        }
        self.offset = len;
        self.partial.push_str(&buf);
        let mut out = Vec::new();
        while let Some(nl) = self.partial.find('\n') {
            let line = self.partial[..nl].trim().to_string();
            self.partial.drain(..=nl);
            if let Ok(ev) = serde_json::from_str::<HookEvent>(&line) {
                out.push(ev);
            }
        }
        out
    }
}

/// Merge the `cctop hook` command into a settings `hooks` object.
pub fn with_hooks(settings: &Value) -> Value {
    let mut out = settings.clone();
    let hooks = out
        .as_object_mut()
        .map(|m| m.entry("hooks").or_insert_with(|| serde_json::json!({})))
        .expect("settings is an object");
    for ev in EVENTS.iter().filter(|e| !NEVER.contains(e)) {
        let arr = hooks
            .as_object_mut()
            .map(|m| m.entry(*ev).or_insert_with(|| serde_json::json!([])))
            .expect("hooks is an object");
        let already = arr.as_array().is_some_and(|groups| {
            groups.iter().any(|g| {
                g.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
                    hs.iter()
                        .any(|h| h.get("command").and_then(Value::as_str) == Some(HOOK_COMMAND))
                })
            })
        });
        if !already {
            if let Some(a) = arr.as_array_mut() {
                a.push(
                    serde_json::json!({"hooks": [{"type": "command", "command": HOOK_COMMAND}]}),
                );
            }
        }
    }
    out
}

/// Remove only cctop's hook entries, leaving user hooks intact.
pub fn without_hooks(settings: &Value) -> Value {
    let mut out = settings.clone();
    let Some(hooks) = out.get_mut("hooks").and_then(Value::as_object_mut) else {
        return out;
    };
    let mut empty = Vec::new();
    for (ev, arr) in hooks.iter_mut() {
        if let Some(groups) = arr.as_array_mut() {
            for g in groups.iter_mut() {
                if let Some(hs) = g.get_mut("hooks").and_then(Value::as_array_mut) {
                    hs.retain(|h| h.get("command").and_then(Value::as_str) != Some(HOOK_COMMAND));
                }
            }
            groups.retain(|g| {
                g.get("hooks")
                    .and_then(Value::as_array)
                    .is_none_or(|hs| !hs.is_empty())
            });
            if groups.is_empty() {
                empty.push(ev.clone());
            }
        }
    }
    for ev in empty {
        hooks.remove(&ev);
    }
    if hooks.is_empty() {
        out.as_object_mut().map(|m| m.remove("hooks"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdin(event: &str, id: Option<&str>) -> Vec<u8> {
        let mut v = serde_json::json!({"session_id":"s1","hook_event_name":event,"tool_name":"Bash","tool_input":{"command":"ls"},"tool_response":{"big":"x"},"cwd":"/x"});
        if let Some(i) = id {
            v["tool_use_id"] = serde_json::json!(i);
        }
        serde_json::to_vec(&v).unwrap()
    }

    #[test]
    fn record_and_watch_roundtrip_under_five_ms() {
        let home = std::env::temp_dir().join(format!("cctop-hooks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let start = std::time::Instant::now();
        for i in 0..10 {
            record(&home, 1000 + i, &stdin("PreToolUse", Some("t1"))).unwrap();
        }
        assert!(start.elapsed() / 10 < std::time::Duration::from_millis(5));
        assert!(record(&home, 1, b"not json").is_none());
        assert!(
            record(&home, 1, b"{\"hook_event_name\":\"Stop\"}").is_none(),
            "no session id"
        );
        let mut w = Watcher::new(&home, "s1");
        let evs = w.poll();
        assert_eq!(evs.len(), 10);
        assert_eq!(evs[0].event, "PreToolUse");
        assert_eq!(evs[0].tool_use_id.as_deref(), Some("t1"));
        assert_eq!(evs[0].tool_name.as_deref(), Some("Bash"));
        assert!(
            evs[0].payload.get("tool_response").is_none(),
            "bulky fields dropped"
        );
        assert!(w.poll().is_empty());
        record(&home, 2000, &stdin("PostToolUse", Some("t1")));
        assert_eq!(w.poll().len(), 1);
    }

    #[test]
    fn new_events_are_spooled_as_facts() {
        let line = |ev: &HookEvent| serde_json::to_string(ev).unwrap();
        let ev = HookEvent::from_stdin(1, serde_json::json!({
            "session_id":"s","hook_event_name":"Elicitation","mcp_server_name":"srv","message":"SECRET-MSG please","mode":"form","elicitation_id":"e1",
            "requested_schema":{"type":"object","properties":{"a":{"description":"SECRET-SCHEMA"},"b":{}}},
            "url":"https://x.example/auth?t=SECRET-TOKEN"
        })).unwrap();
        assert!(!line(&ev).contains("SECRET"));
        assert_eq!(ev.payload["has_url"], true);
        assert!(ev.payload.get("url").is_none());
        assert_eq!(ev.payload["message_chars"], 17);
        assert_eq!(ev.payload["schema_fields"], 2);
        assert_eq!(ev.payload["mcp_server_name"], "srv");
        assert_eq!(ev.payload["elicitation_id"], "e1");
        let ev = HookEvent::from_stdin(1, serde_json::json!({
            "session_id":"s","hook_event_name":"ElicitationResult","mcp_server_name":"srv","action":"accept","mode":"form","elicitation_id":"e1",
            "content":{"x":"SECRET-ANSWER","y":1,"z":2}
        })).unwrap();
        assert!(!line(&ev).contains("SECRET"));
        assert_eq!(ev.payload["content_fields"], 3);
        assert_eq!(ev.payload["action"], "accept");
        assert_eq!(ev.payload["mode"], "form");
        assert_eq!(ev.payload["elicitation_id"], "e1");
        let ev = HookEvent::from_stdin(
            1,
            serde_json::json!({
                "session_id":"s","hook_event_name":"ElicitationResult","action":"cancel"
            }),
        )
        .unwrap();
        assert_eq!(ev.payload["content_fields"], 0);
        let ev = HookEvent::from_stdin(1, serde_json::json!({
            "session_id":"s","hook_event_name":"PreSkillShellCommand","command_name":"review","command_source":"project","invoked_by":"user",
            "tool_input":{"command":"c".repeat(500),"description":"SECRET-DESC"}
        })).unwrap();
        assert!(!line(&ev).contains("SECRET"));
        assert_eq!(
            ev.payload["tool_input"]["command"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            200
        );
        assert_eq!(ev.payload["tool_input"].as_object().unwrap().len(), 1);
        assert_eq!(ev.payload["command_name"], "review");
        for (name, extra) in [
            (
                "ConfigChange",
                serde_json::json!({"source":"project_settings","file_path":"/p/.claude/settings.json"}),
            ),
            (
                "CwdChanged",
                serde_json::json!({"old_cwd":"/a","new_cwd":"/b"}),
            ),
            ("DirectoryAdded", serde_json::json!({"directory":"/d"})),
            (
                "TeammateIdle",
                serde_json::json!({"teammate_name":"t","team_name":"x"}),
            ),
        ] {
            let mut v = extra.clone();
            v["session_id"] = "s".into();
            v["hook_event_name"] = name.into();
            let ev = HookEvent::from_stdin(1, v).unwrap();
            for (k, val) in extra.as_object().unwrap() {
                assert_eq!(&ev.payload[k], val, "{name}.{k} kept");
            }
        }
    }

    #[test]
    fn only_elicitation_loses_its_message() {
        let ev = HookEvent::from_stdin(1, serde_json::json!({
            "session_id":"s","hook_event_name":"Notification","notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"
        })).unwrap();
        assert_eq!(
            ev.payload["message"],
            "Claude needs your permission to use Bash"
        );
        assert!(ev.payload.get("message_chars").is_none());
        assert!(ev.payload.get("schema_fields").is_none());
        let ev = HookEvent::from_stdin(
            1,
            serde_json::json!({
                "session_id":"s","hook_event_name":"Elicitation","mcp_server_name":"srv"
            }),
        )
        .unwrap();
        assert!(ev.payload.get("message_chars").is_none());
        assert!(ev.payload.get("schema_fields").is_none());
    }

    #[test]
    fn spool_keeps_facts_and_never_text() {
        let ev = HookEvent::from_stdin(1, serde_json::json!({
            "session_id":"s1","hook_event_name":"UserPromptSubmit","prompt":"what is the plan?","prompt_id":"p1"
        })).unwrap();
        assert!(ev.payload.get("prompt").is_none());
        assert_eq!(ev.payload["prompt_chars"], 17);
        assert_eq!(ev.payload["prompt_ends_with_question"], true);
        let ev = HookEvent::from_stdin(1, serde_json::json!({
            "session_id":"s1","hook_event_name":"Stop","last_assistant_message":"Done.","background_tasks":[{"id":"b1","type":"shell","status":"running","command":"x".repeat(900),"description":"builds"}],"session_crons":[]
        })).unwrap();
        assert!(ev.payload.get("last_assistant_message").is_none());
        assert_eq!(ev.payload["last_assistant_message_chars"], 5);
        assert_eq!(
            ev.payload["last_assistant_message_ends_with_question"],
            false
        );
        assert_eq!(
            ev.payload["background_tasks"][0]["command"]
                .as_str()
                .unwrap()
                .chars()
                .count(),
            200
        );
        let ev = HookEvent::from_stdin(1, serde_json::json!({
            "session_id":"s1","hook_event_name":"PostToolUse","tool_name":"Agent","tool_use_id":"t","tool_input":{"prompt":"go","subagent_type":"Explore"},"tool_response":{"content":"long text","resolvedModel":"claude-haiku-4-5-20251001","totalToolUseCount":9,"usage":{"input_tokens":1},"toolStats":{"Read":4}}
        })).unwrap();
        assert!(ev.payload.get("tool_input").is_none());
        assert_eq!(
            ev.payload["tool_response"]["resolvedModel"],
            "claude-haiku-4-5-20251001"
        );
        assert_eq!(ev.payload["tool_response"]["totalToolUseCount"], 9);
        assert!(ev.payload["tool_response"].get("content").is_none());
        let ev = HookEvent::from_stdin(1, serde_json::json!({
            "session_id":"s1","hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":"gh api repos/x"},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"gh api:*"}]}]
        })).unwrap();
        assert_eq!(ev.payload["command"], "gh api repos/x");
        assert_eq!(
            ev.payload["permission_suggestions"][0]["rules"][0]["ruleContent"],
            "gh api:*"
        );
        assert_eq!(EVENTS.len(), 29);
    }

    #[test]
    fn prune_removes_old_spools() {
        let home = std::env::temp_dir().join(format!("cctop-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        record(&home, 1, &stdin("Stop", None)).unwrap();
        let now = crate::app::now_ms();
        assert_eq!(prune(&home, now), 0);
        assert_eq!(prune(&home, now + PRUNE_AFTER_MS + 60_000), 1);
    }

    #[test]
    fn events_and_never_disjoint() {
        for ev in NEVER {
            assert!(!EVENTS.contains(ev), "{ev} is in both");
        }
        assert_eq!(NEVER.len(), 5);
        let mut seen = std::collections::BTreeSet::new();
        assert!(EVENTS.iter().all(|e| seen.insert(*e)), "no duplicates");
    }

    #[test]
    fn uninstall_removes_never_events_too() {
        let user = serde_json::json!({"hooks":{"Setup":[{"hooks":[{"type":"command","command":"mine.sh"}]}]}});
        let mut s = user.clone();
        for ev in NEVER.iter().filter(|e| **e != "Setup") {
            s["hooks"][*ev] =
                serde_json::json!([{"hooks":[{"type":"command","command":HOOK_COMMAND}]}]);
        }
        s["hooks"]["Setup"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"hooks":[{"type":"command","command":HOOK_COMMAND}]}));
        let out = without_hooks(&s);
        assert_eq!(out, user, "only the user's Setup hook is left");
        // And with_hooks never writes one.
        for ev in NEVER {
            assert!(with_hooks(&serde_json::json!({}))["hooks"]
                .get(*ev)
                .is_none());
        }
    }

    #[test]
    fn hooks_merge_and_unmerge_without_touching_user_hooks() {
        let user = serde_json::json!({"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lint.sh"}]}]}});
        let w = with_hooks(&user);
        for ev in EVENTS {
            let groups = w["hooks"][*ev].as_array().unwrap();
            assert!(
                groups
                    .iter()
                    .any(|g| g["hooks"][0]["command"] == HOOK_COMMAND),
                "{ev}"
            );
        }
        assert_eq!(
            w["hooks"]["PreToolUse"][0]["hooks"][0]["command"], "lint.sh",
            "user hook first"
        );
        assert_eq!(w["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(with_hooks(&w), w, "idempotent");
        assert_eq!(without_hooks(&w), user, "restores exactly");
        assert_eq!(
            without_hooks(&with_hooks(&serde_json::json!({}))),
            serde_json::json!({})
        );
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use crate::metrics::Pricing;
    use crate::transcript::Line;
    use crate::ui::State;

    fn ev(event: &str, at: i64, id: &str) -> HookEvent {
        HookEvent {
            at,
            event: event.into(),
            session_id: "s".into(),
            tool_use_id: (!id.is_empty()).then(|| id.to_string()),
            tool_name: Some("Bash".into()),
            payload: serde_json::json!({}),
        }
    }

    #[test]
    fn exact_durations_permission_waits_and_compaction() {
        let mut s = State::new(Pricing::bundled());
        // Transcript: a Bash call issued at T, result at T+30s (includes the wait).
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"id":"m1","model":"claude-opus-5","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":150000}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:30Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#).unwrap());
        let t0 = crate::metrics::cost::parse_ts_ms("2026-01-01T00:00:00Z").unwrap();
        assert_eq!(s.tools.get("t1").unwrap().duration_ms, Some(30_000));
        assert!(s.tools.get("t1").unwrap().approx_duration);

        s.apply_hook(&ev("PermissionRequest", t0 + 1_000, "t1"));
        assert!(s.session.permission_pending);
        s.apply_hook(&ev("PreToolUse", t0 + 25_000, "t1"));
        s.apply_hook(&ev("PostToolUse", t0 + 30_000, "t1"));
        let c = s.tools.get("t1").unwrap();
        assert_eq!(c.duration_ms, Some(5_000), "exact from hooks");
        assert!(!c.approx_duration);
        assert!(!s.session.permission_pending);
        assert_eq!(s.session.permission_waits, 1);
        // wait = post − perm − median(5 s, now exact) = 29 s − 5 s = 24 s
        assert_eq!(s.session.permission_wait_ms, 24_000);
        assert!(s.session.hooks_installed);

        s.apply_hook(&ev("PreCompact", t0 + 40_000, ""));
        assert_eq!(s.learned_thresholds.get("claude-opus-5"), Some(&150_000));
        assert!(s.events.iter().any(|e| e.text == "compacting at 150k"));
        assert!(s
            .events
            .iter()
            .any(|e| e.text.starts_with("Bash allowed after ≈0:24")));
    }

    fn with(event: &str, at: i64, id: &str, payload: serde_json::Value) -> HookEvent {
        let mut e = ev(event, at, id);
        e.payload = payload;
        e
    }

    #[test]
    fn cwd_changed_moves_display() {
        let mut s = State::new(Pricing::bundled());
        s.session.cwd = "/work/proj".into();
        assert_eq!(s.work_dir(), std::path::Path::new("/work/proj"));
        s.apply_hook(&with(
            "CwdChanged",
            1_000,
            "",
            serde_json::json!({"old_cwd":"/work/proj","new_cwd":"/elsewhere/lib"}),
        ));
        // Git and file paths follow; the session's own folder does not move.
        assert_eq!(s.work_dir(), std::path::Path::new("/elsewhere/lib"));
        assert_eq!(s.session.cwd, std::path::PathBuf::from("/work/proj"));
        assert!(s.events.iter().any(|e| e.text == "cwd proj → lib"));
    }

    #[test]
    fn directory_added_no_double_count() {
        let mut s = State::new(Pricing::bundled());
        let added = |dir: &str| {
            with(
                "DirectoryAdded",
                2_000,
                "",
                serde_json::json!({"directory": dir, "source": "command"}),
            )
        };
        s.apply_hook(&added("/x/y"));
        assert_eq!(s.added_dirs_count(), 1);
        // The status line names the same folder, and another: union by path.
        let st = crate::status::Sample::parse(
            r#"{"session_id":"s","workspace":{"added_dirs":["/x/y","/z"]}}"#,
        )
        .unwrap();
        s.apply_status(&st, &[]);
        assert_eq!(s.added_dirs_count(), 2);
        // The hook again, and one the status line already has.
        s.apply_hook(&added("/x/y"));
        s.apply_hook(&added("/z/"));
        assert_eq!(s.added_dirs_count(), 2);
        assert_eq!(
            s.events
                .iter()
                .filter(|e| e.text == "dir added · y")
                .count(),
            2
        );
    }

    #[test]
    fn skill_shell_counts() {
        let mut s = State::new(Pricing::bundled());
        let run = |name: &str, cmd: &str| {
            with(
                "PreSkillShellCommand",
                3_000,
                "",
                serde_json::json!({"command_name": name, "tool_input": {"command": cmd}}),
            )
        };
        s.apply_hook(&run("review", "git diff"));
        s.apply_hook(&run("review", "git log"));
        s.apply_hook(&run("deploy", "make"));
        assert_eq!(s.skill_shell["review"], 2);
        assert_eq!(s.skill_shell["deploy"], 1);
        assert!(s
            .events
            .iter()
            .any(|e| e.text == "skill shell: review · git diff"));
        let q = crate::query::tools(&s);
        assert_eq!(q["skill_shell"][0]["skill"], "deploy");
        assert_eq!(q["skill_shell"][1]["count"], 2);
    }

    #[test]
    fn teammate_idle_hook_reaches_a_team_loaded_later() {
        let mut s = State::new(Pricing::bundled());
        s.apply_hook(&with(
            "TeammateIdle",
            4_000,
            "",
            serde_json::json!({"teammate_name": "t", "team_name": "x"}),
        ));
        assert!(s.events.iter().any(|e| e.text == "t idle"));
        let mut team = crate::team::Team {
            name: "session-x".into(),
            source: crate::team::Source::Transcripts,
            members: Default::default(),
            looked_in: vec![],
            dir_seen: false,
        };
        team.members.insert(
            "t".into(),
            crate::team::Teammate::new("t", Pricing::bundled()),
        );
        s.team = Some(team);
        s.sync_teammate_idle();
        assert!(s.team.as_ref().unwrap().members["t"].is_hook_idle());
    }

    #[test]
    fn settings_row_names_the_file_and_source_on_either_separator() {
        let mut s = State::new(Pricing::bundled());
        for (at, path) in [
            (1_000, "/p/.claude/settings.json"),
            (2_000, r"C:\p\.claude\settings.local.json"),
        ] {
            s.apply_hook(&with(
                "ConfigChange",
                at,
                "",
                serde_json::json!({"source": "project_settings", "file_path": path}),
            ));
        }
        let rows: Vec<&str> = s.events.iter().map(|e| e.text.as_str()).collect();
        assert!(rows.contains(&"settings changed · settings.json (project_settings)"));
        assert!(rows.contains(&"settings changed · settings.local.json (project_settings)"));
        assert!(rows.iter().all(|r| !r.contains(".claude")), "{rows:?}");
    }

    #[test]
    fn teammate_idle_with_an_agent_id_still_reaches_the_arm() {
        let mut s = State::new(Pricing::bundled());
        s.apply_hook(&with(
            "TeammateIdle",
            4_000,
            "",
            serde_json::json!({"teammate_name": "t", "agent_id": "t@x", "agent_type": "w"}),
        ));
        assert!(s.events.iter().any(|e| e.text == "t idle"));
        assert!(s.agents.is_empty(), "not taken for a subagent's event");
    }

    #[test]
    fn mcp_prompts_wait_fifo_per_server_and_are_counted_by_action() {
        use crate::ui::state::WaitingKind;
        let mut s = State::new(Pricing::bundled());
        let el = |event: &str, at: i64, server: &str, action: &str| {
            with(
                event,
                at,
                "",
                serde_json::json!({"mcp_server_name": server, "action": action}),
            )
        };
        assert!(s.waiting().is_none());
        s.apply_hook(&el("Elicitation", 1_000, "github", ""));
        s.apply_hook(&el("Elicitation", 5_000, "linear", ""));
        s.apply_hook(&el("Elicitation", 9_000, "github", ""));
        let w = s.waiting().unwrap();
        assert_eq!((w.kind, w.since_ms), (WaitingKind::Elicitation, 1_000));
        assert_eq!(s.open_elicitation(), Some(("github", 1_000)));
        // Review Focus 1: the result pops github's OLDEST prompt; its second
        // stays open, and the oldest open is now linear's.
        s.apply_hook(&el("ElicitationResult", 11_000, "github", "accept"));
        assert_eq!(s.open_elicitation(), Some(("linear", 5_000)));
        assert_eq!(s.elicitations["github"].len(), 1);
        // Review Focus 2: a result for the other server leaves github's open.
        s.apply_hook(&el("ElicitationResult", 12_000, "linear", "decline"));
        assert_eq!(s.open_elicitation(), Some(("github", 9_000)));
        assert!(!s.elicitations.contains_key("linear"));
        s.apply_hook(&el("ElicitationResult", 20_000, "github", "cancel"));
        assert!(s.waiting().is_none());
        let w = s.elicitation_waits;
        assert_eq!((w.count, w.accept, w.decline, w.cancel), (3, 1, 1, 1));
        // 10 s + 7 s + 11 s
        assert_eq!(w.total_wait_ms, 28_000);
        // A result with nothing open counts as nothing.
        s.apply_hook(&el("ElicitationResult", 30_000, "ghost", "accept"));
        assert_eq!(s.elicitation_waits.total_wait_ms, 28_000);
        assert_eq!(
            (s.elicitation_waits.count, s.elicitation_waits.accept),
            (3, 1)
        );
        assert!(s.waiting().is_none());
    }

    #[test]
    fn stop_and_the_next_prompt_clear_every_open_mcp_prompt() {
        for closer in ["Stop", "UserPromptSubmit"] {
            let mut s = State::new(Pricing::bundled());
            s.apply_hook(&with(
                "Elicitation",
                1_000,
                "",
                serde_json::json!({"mcp_server_name": "a"}),
            ));
            s.apply_hook(&with(
                "Elicitation",
                2_000,
                "",
                serde_json::json!({"mcp_server_name": "b"}),
            ));
            assert!(s.waiting().is_some());
            s.apply_hook(&ev(closer, 3_000, ""));
            assert!(s.waiting().is_none(), "{closer}");
            assert_eq!(s.elicitation_waits.count, 0, "cleared, not completed");
        }
    }

    #[test]
    fn the_mcp_waiting_line_is_the_same_text_on_every_surface_and_fits_the_width() {
        let long = "m".repeat(40);
        for server in ["github", long.as_str()] {
            let mut s = State::new(Pricing::bundled());
            s.apply_hook(&with(
                "Elicitation",
                1_000,
                "",
                serde_json::json!({"mcp_server_name": server}),
            ));
            let act = crate::dashboard::text_of(
                &crate::dashboard::snapshot(&s, &crate::advisor::Engine::for_state(&s))
                    .act
                    .line,
            );
            let state_line =
                crate::coach::state_line(&s, crate::advisor::SessionMode::Interactive).line;
            for line in [&act, &state_line] {
                assert!(line.chars().count() <= crate::coach::WIDTH, "{line}");
                assert!(line.starts_with("◆ WAITING "), "{line}");
                assert!(line.ends_with(" asks"), "{line}");
                assert!(line.contains("MCP "), "{line}");
            }
            if server == "github" {
                assert!(act.ends_with("— MCP github asks"), "{act}");
                assert!(state_line.ends_with("· MCP github asks"), "{state_line}");
            }
        }
    }

    #[test]
    fn every_field_of_the_spool_lands_in_state() {
        use crate::metrics::usage::BoundaryKind;
        let mut s = State::new(Pricing::bundled());
        s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"id":"m1","model":"claude-opus-5","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls"}}],"usage":{"input_tokens":1000}}}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:00:30Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"#).unwrap());
        let t0 = crate::metrics::cost::parse_ts_ms("2026-01-01T00:00:00Z").unwrap();
        // duration_ms is the tool's own run time: exact, no PreToolUse needed.
        s.apply_hook(&with("PostToolUse", t0 + 30_000, "t1", serde_json::json!({"duration_ms": 909, "permission_mode": "acceptEdits", "effort": {"level": "high"}, "prompt_id": "p1"})));
        let c = s.tools.get("t1").unwrap();
        assert_eq!(c.duration_ms, Some(909));
        assert!(!c.approx_duration);
        assert_eq!(s.session.permission_mode.as_deref(), Some("acceptEdits"));
        assert_eq!(s.status_facts.effort_level.as_deref(), Some("high"));
        assert_eq!(s.session.hook_prompt_id.as_deref(), Some("p1"));
        // A failure with its error and the interrupt flag.
        s.apply_hook(&with("PostToolUseFailure", t0 + 31_000, "t1", serde_json::json!({"duration_ms": 65, "error": "Command failed with exit code 1", "is_interrupt": false})));
        assert!(s
            .events
            .iter()
            .any(|e| e.text == "Bash failed: Command failed with exit code 1"));
        s.apply_hook(&with(
            "PostToolUseFailure",
            t0 + 32_000,
            "t1",
            serde_json::json!({"is_interrupt": true}),
        ));
        assert!(s.events.iter().any(|e| e.text == "Bash interrupted"));
        // Auto mode: the permission_prompt notification is the only WAITING sign.
        s.apply_hook(&with("Notification", t0 + 40_000, "", serde_json::json!({"notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"})));
        assert!(s.session.permission_pending);
        assert_eq!(s.session.permission_waiting_since_ms, Some(t0 + 40_000));
        assert_eq!(
            s.session.notification.as_ref().map(|(k, _)| k.as_str()),
            Some("permission_prompt")
        );
        s.apply_hook(&with(
            "UserPromptSubmit",
            t0 + 41_000,
            "",
            serde_json::json!({"prompt_chars": 12}),
        ));
        assert!(s.session.notification.is_none());
        // A resume: Claude Code's own cold-write estimate, and a boundary.
        s.apply_hook(&with("SessionStart", t0 + 50_000, "", serde_json::json!({"source": "resume", "seconds_since_last_response": 7200, "context_tokens": 223927, "prompt_cache_likely_expired": true, "estimated_cache_write_usd": 2.2393})));
        let r = s.session.resume.as_ref().unwrap();
        assert_eq!(r.source, "resume");
        assert_eq!(r.context_tokens, Some(223_927));
        assert_eq!(r.prompt_cache_likely_expired, Some(true));
        assert_eq!(r.estimated_cache_write_usd, Some(2.2393));
        assert_eq!(s.agg.boundaries.last().unwrap().kind, BoundaryKind::Resume);
        assert_eq!(s.session.start_source.as_deref(), Some("resume"));
        // Stop: background tasks, crons, the question flag; the task list follows.
        s.apply_hook(&with("Stop", t0 + 60_000, "", serde_json::json!({"background_tasks": [{"id": "b1", "type": "shell", "status": "running", "command": "cargo build --release", "description": "builds the thing"}], "session_crons": [{"id": "c1"}], "last_assistant_message_chars": 40, "last_assistant_message_ends_with_question": true})));
        assert_eq!(s.session.background_tasks.len(), 1);
        assert_eq!(s.session.background_tasks[0].kind, "shell");
        assert_eq!(s.session.session_crons, 1);
        assert_eq!(s.session.last_stop_asked, Some(true));
        s.refresh_tasks(Vec::new());
        assert_eq!(s.tasks.len(), 1);
        assert_eq!(s.tasks[0].description, "builds the thing");
        assert!(s.events.iter().any(|e| e.text == "Stop · 1 background"));
        // The new events.
        s.apply_hook(&with(
            "StopFailure",
            t0 + 61_000,
            "",
            serde_json::json!({"error": "rate_limit"}),
        ));
        assert!(s.events.iter().any(|e| e.text == "turn died: rate_limit"));
        s.apply_hook(&with("PreModelSwitch", t0 + 62_000, "", serde_json::json!({"from_model": "claude-opus-5", "to_model": "claude-sonnet-5", "prompt_cache_warm": true, "estimated_cache_write_usd": 0.8})));
        assert!(s.events.iter().any(|e| e.text
            == "model switch claude-opus-5 → claude-sonnet-5 · cache warm · re-write ≈$0.80"));
        s.apply_hook(&with(
            "PermissionDenied",
            t0 + 63_000,
            "",
            serde_json::json!({"reason": "matches deny rule"}),
        ));
        assert!(s
            .events
            .iter()
            .any(|e| e.text == "Bash denied: matches deny rule"));
        s.apply_hook(&with(
            "SessionEnd",
            t0 + 64_000,
            "",
            serde_json::json!({"reason": "prompt_input_exit"}),
        ));
        assert_eq!(s.session.end_reason.as_deref(), Some("prompt_input_exit"));
        // A subagent's tool events go to the agent, not the session's tools.
        s.apply_hook(&with(
            "PostToolUse",
            t0 + 65_000,
            "t9",
            serde_json::json!({"agent_id": "a1", "agent_type": "Explore", "duration_ms": 120}),
        ));
        s.apply_hook(&with(
            "PostToolUseFailure",
            t0 + 66_000,
            "t9",
            serde_json::json!({"agent_id": "a1", "agent_type": "Explore", "duration_ms": 30}),
        ));
        let a = s.agents.get("a1").unwrap();
        assert_eq!(a.agent_type, "Explore");
        assert_eq!(a.hook_tool_calls, 2);
        assert_eq!(a.hook_tool_errors, 1);
        assert_eq!(a.hook_tool_ms, 150);
        assert!(s.tools.get("t9").is_none());
    }
}
