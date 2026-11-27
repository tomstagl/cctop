//! Witness states: one named, deterministic state per rule variant, built
//! the way the rules' own tests build them, so a rule's tests and
//! `docs/coach-copy.md` (`cctop coach --copy-md`) fire the same thing.
//! Each witness is a state a rule fires on, with its own clock (`now_ms`
//! fixed; nothing here reads the wall clock). The rules' tests call the
//! builders and assert on what fires; [`all`] lists them for the copy table.
//!
//! This module is compiled into the library (not `cfg(test)`) because the
//! `cctop coach --copy-md` generator fires the witnesses.

use super::fixtures::{prompt, response, tool};
use crate::metrics::Pricing;
use crate::transcript::Line;
use crate::ui::State;

/// A timestamp in epoch milliseconds.
pub(crate) fn t(ts: &str) -> i64 {
    crate::metrics::cost::parse_ts_ms(ts).unwrap()
}

// -- outcome-axis helpers (A33–A47) ----------------------------------------

/// A tool call and its result: `extra` is spliced into the result's
/// top-level object (`"is_error":true` goes into the block).
pub(crate) fn call(
    s: &mut State,
    id: &str,
    ts: &str,
    name: &str,
    input: &str,
    result: &str,
    extra: &str,
) {
    call_ctx(s, id, ts, name, input, result, extra, 50_000);
}

/// [`call`] on a context of `ctx` tokens.
#[allow(clippy::too_many_arguments)]
pub(crate) fn call_ctx(
    s: &mut State,
    id: &str,
    ts: &str,
    name: &str,
    input: &str,
    result: &str,
    extra: &str,
    ctx: u64,
) {
    s.apply(&Line::parse(&format!(
        r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"{id}","model":"claude-opus-5","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}],"usage":{{"input_tokens":2,"cache_read_input_tokens":{ctx},"output_tokens":10}}}}}}"#
    )).unwrap());
    s.apply(&Line::parse(&format!(
        r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{result}"{}}}]}}{extra}}}"#,
        if extra.contains("\"is_error\"") { r#","is_error":true"# } else { "" }
    )).unwrap());
}

pub(crate) fn edit(s: &mut State, id: &str, ts: &str, path: &str) {
    call(
        s,
        id,
        ts,
        "Edit",
        &format!(r#"{{"file_path":"{path}","old_string":"a","new_string":"b"}}"#),
        "ok",
        "",
    );
}

pub(crate) fn bash(s: &mut State, id: &str, ts: &str, cmd: &str, stdout: &str, extra: &str) {
    call(
        s,
        id,
        ts,
        "Bash",
        &format!(r#"{{"command":"{cmd}"}}"#),
        stdout,
        &format!(
            r#","toolUseResult":{{"stdout":"{stdout}","stderr":"","interrupted":false{extra}}}"#
        ),
    );
}

/// A live session at 10:00:02 with one prompt and one 50k response.
pub(crate) fn fresh() -> State {
    let mut s = State::new(Pricing::bundled());
    s.session.alive = true;
    s.apply(&prompt("2026-01-01T10:00:00Z"));
    s.apply(&response("m0", "2026-01-01T10:00:01Z", 2, 50_000, 0));
    s.now_ms = t("2026-01-01T10:00:02Z");
    s
}

// -- token axis -------------------------------------------------------------

/// A04: one file read whole three times with no edit between.
pub(crate) fn a04_reads() -> State {
    let mut s = State::new(Pricing::bundled());
    for i in 0..3 {
        for l in tool(
            &format!("r{i}"),
            "2026-01-01T00:00:00Z",
            "Read",
            "/p/src/render.rs",
            8_000,
        ) {
            s.apply(&l);
        }
    }
    s
}

/// A04 `churn`: after an edit, two more edit → re-read → edit rounds.
pub(crate) fn a04_churn() -> State {
    let mut s = a04_reads();
    for l in tool("e1", "2026-01-01T00:00:00Z", "Edit", "/p/src/render.rs", 10) {
        s.apply(&l);
    }
    for i in 0..2 {
        for l in tool(
            &format!("rr{i}"),
            "2026-01-01T00:00:00Z",
            "Read",
            "/p/src/render.rs",
            8_000,
        ) {
            s.apply(&l);
        }
        for l in tool(
            &format!("ee{i}"),
            "2026-01-01T00:00:00Z",
            "Edit",
            "/p/src/render.rs",
            10,
        ) {
            s.apply(&l);
        }
    }
    s
}

/// A25: eight read-only calls in a row, 12k each.
pub(crate) fn a25() -> State {
    let mut s = State::new(Pricing::bundled());
    for i in 0..8 {
        let (name, input) = if i % 2 == 0 {
            ("Read", "/p/f.rs")
        } else {
            ("Bash", "rg -n todo src")
        };
        for l in tool(
            &format!("g{i}"),
            "2026-01-01T00:00:00Z",
            name,
            input,
            12_000,
        ) {
            s.apply(&l);
        }
    }
    s
}

/// A07: an MCP server idle for 25 minutes.
pub(crate) fn a07_mcp() -> State {
    let mut s = State::new(Pricing::bundled());
    s.apply(&prompt("2026-01-01T00:00:00Z"));
    s.apply(&Line::from_value(serde_json::json!({"type":"attachment","attachment":{"type":"deferred_tools_delta","addedNames":["mcp__playwright__click","mcp__playwright__snapshot"],"addedLines":["x".repeat(6000),"y".repeat(6000)]}})));
    s.session.alive = true;
    s.now_ms = t("2026-01-01T00:00:00Z") + 25 * 60_000;
    s
}

/// A07 `plugin`: an enabled plugin unused for 38 startups, 4.1k always-on.
pub(crate) fn a07_plugin() -> State {
    let mut p = State::new(Pricing::bundled());
    p.prefix.scan_plugins(
        &serde_json::json!({"enabledPlugins": {"frontend-design@claude-plugins-official": true}}),
        Some(&serde_json::json!({"catalog": [{"plugin": "frontend-design", "tokens": {"claude-opus-5": {"always_on": 4100}}}]})),
        Some(&serde_json::json!({"numStartups": 438, "pluginUsage": {"frontend-design@claude-plugins-official": {"lastUsedNumStartups": 400}}})),
        "claude-opus-5",
    );
    p
}

pub(crate) fn think(id: &str, model: &str, out: u64, thinking: u64) -> Line {
    Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","perTurnEffort":"high","message":{{"id":"{id}","model":"{model}","content":[{{"type":"text","text":"ok"}}],"usage":{{"output_tokens":{out},"output_tokens_details":{{"thinking_tokens":{thinking}}}}}}}}}"#)).unwrap()
}

/// A08: five edit-heavy turns on `model` with 60 % thinking at high effort.
pub(crate) fn a08_on(model: &str) -> State {
    let mut s = State::new(Pricing::bundled());
    for i in 0..5 {
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        for l in tool(
            &format!("e{i}"),
            "2026-01-01T00:00:00Z",
            "Edit",
            "/p/a.rs",
            10,
        ) {
            s.apply(&l);
        }
        s.apply(&think(&format!("t{i}"), model, 1_000, 600));
    }
    s
}

pub(crate) fn a08_sonnet() -> State {
    a08_on("claude-sonnet-5")
}

pub(crate) fn a08_fable() -> State {
    a08_on("claude-fable-5")
}

/// A11: the first call of a turn sent 6k uncached input tokens.
pub(crate) fn a11_input() -> State {
    let mut s = State::new(Pricing::bundled());
    s.apply(&prompt("2026-01-01T00:00:00Z"));
    s.apply(&response("f1", "2026-01-01T00:00:01Z", 6_000, 0, 50_000));
    s
}

/// A11 `paste`: a 412-line paste recorded in history.jsonl for the turn.
pub(crate) fn a11_paste() -> State {
    use crate::history::Paste;
    let mut s = State::new(Pricing::bundled());
    s.apply(&prompt("2026-01-01T00:00:00Z"));
    s.apply(&response("f1", "2026-01-01T00:00:01Z", 2, 9_800, 50_000));
    s.history.pastes.push(Paste {
        at_ms: t("2026-01-01T00:00:00Z") - 300,
        chars: 38_000,
        lines: 412,
    });
    s
}

/// A12: 25 short prompts within the last hour.
pub(crate) fn a12() -> State {
    let mut s = State::new(Pricing::bundled());
    for i in 0..25 {
        s.apply(&Line::parse(&format!(r#"{{"type":"user","timestamp":"2026-01-01T00:{i:02}:00Z","promptSource":"typed","message":{{"role":"user","content":"ok go"}}}}"#)).unwrap());
        s.apply(&response(
            &format!("c{i}"),
            &format!("2026-01-01T00:{i:02}:01Z"),
            10,
            0,
            100_000,
        ));
    }
    s.session.alive = true;
    s.now_ms = t("2026-01-01T00:30:00Z");
    s
}

fn agent(id: &str, kind: &str) -> crate::agents::Agent {
    let mut a = crate::agents::Agent::new(
        id,
        crate::agents::Meta {
            agent_type: kind.into(),
            ..Default::default()
        },
    );
    a.model = "claude-opus-5".into();
    a
}

/// An Opus agent that made `tools` calls (by tool name): 400k cache read, 5k output.
pub(crate) fn search_agent(id: &str, kind: &str, tools: &[(&str, usize)]) -> crate::agents::Agent {
    let mut a = agent(id, kind);
    a.usage.cache_read = 400_000;
    a.usage.output = 5_000;
    for (t, n) in tools {
        a.tools_by_name.insert((*t).into(), *n);
    }
    a
}

/// A14: an Explore agent on Opus with seven searches and no edits.
pub(crate) fn a14() -> State {
    let mut s = State::new(Pricing::bundled());
    s.agents.insert(
        "x".into(),
        search_agent("x", "Explore", &[("Grep", 4), ("Read", 3)]),
    );
    s
}

/// A task-notification line for agent `id`.
pub(crate) fn notify(s: &mut State, id: &str, status: &str, result: &str) {
    let text = format!(
        "<task-notification><task-id>{id}</task-id><status>{status}</status><summary>s</summary><result>{result}</result></task-notification>"
    );
    s.apply(&Line::parse(&format!(
        r#"{{"type":"user","timestamp":"2026-01-01T00:10:00Z","promptSource":"system","origin":{{"kind":"task-notification"}},"message":{{"role":"user","content":{}}}}}"#,
        serde_json::to_string(&text).unwrap()
    )).unwrap());
}

/// An opus agent at about $1.25 (1.5M cache read, 20k output).
pub(crate) fn costly_agent(id: &str) -> crate::agents::Agent {
    let mut a = agent(id, "Explore");
    a.usage.cache_read = 1_500_000;
    a.usage.output = 20_000;
    a
}

/// Four costly agents that all returned: nothing wasted yet.
pub(crate) fn a48_returned() -> State {
    let mut s = State::new(Pricing::bundled());
    for id in [
        "0000000000000000a",
        "0000000000000000b",
        "0000000000000000c",
        "0000000000000000d",
    ] {
        s.agents.insert(id.into(), costly_agent(id));
    }
    for id in [
        "0000000000000000a",
        "0000000000000000b",
        "0000000000000000c",
        "0000000000000000d",
    ] {
        notify(&mut s, id, "completed", &"r".repeat(800));
    }
    s
}

/// A48: one of the four failed, one killed: about half the spend wasted.
pub(crate) fn a48() -> State {
    let mut s = a48_returned();
    notify(&mut s, "0000000000000000a", "failed", "");
    notify(&mut s, "0000000000000000b", "killed", "");
    s
}

/// The session of one turn with a 150k context, the cache written at
/// 10:00:05 on the 5 m TTL, a question pending since 10:00:06.
pub(crate) fn asked() -> State {
    let mut s = State::new(Pricing::bundled());
    s.session.alive = true;
    s.apply(&prompt("2026-01-01T10:00:00Z"));
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":2,"cache_creation_input_tokens":150000,"cache_read_input_tokens":0,"output_tokens":10}}}"#).unwrap());
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:06Z","message":{"id":"m2","model":"claude-opus-5","content":[{"type":"tool_use","id":"q","name":"AskUserQuestion","input":{"questions":[]}}],"usage":{"input_tokens":2,"cache_read_input_tokens":150000,"output_tokens":10}}}"#).unwrap());
    s
}

/// A21: a warm model switch reported by the hook, 50 seconds after the cache write.
pub(crate) fn a21() -> State {
    let mut s = asked();
    s.now_ms = t("2026-01-01T10:01:00Z");
    s.apply_hook(&crate::hooks::HookEvent::from_stdin(t("2026-01-01T10:00:50Z"), serde_json::json!({"hook_event_name":"PreModelSwitch","from_model":"claude-opus-5","to_model":"claude-sonnet-5","source":"command","prompt_cache_warm":true,"context_tokens":150000,"estimated_cache_write_usd":0.9})).unwrap());
    s
}

/// A21b: the cache is cold and the person switches projects.
pub(crate) fn a21b() -> State {
    let mut cold = asked();
    cold.now_ms = t("2026-01-01T10:20:00Z");
    cold.history.project_switches = 4;
    cold
}

/// A19: the 5 m cache entry has 1:36 left while a question is pending.
pub(crate) fn a19() -> State {
    let mut s = asked();
    s.now_ms = t("2026-01-01T10:03:30Z");
    s
}

/// A27: a loop armed on a 180k context.
pub(crate) fn a27() -> State {
    let mut s = State::new(Pricing::bundled());
    s.apply(&prompt("2026-01-01T10:00:00Z"));
    s.apply(&response("m1", "2026-01-01T10:00:05Z", 2, 0, 180_000));
    s.session.session_crons = 1;
    s
}

/// A30: idle 2 h 14 m on a 182k cache-written context.
pub(crate) fn a30_idle() -> State {
    let mut s = State::new(Pricing::bundled());
    s.session.alive = true;
    s.apply(&prompt("2026-01-01T10:00:00Z"));
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:00:05Z","message":{"id":"m1","model":"claude-opus-5","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":182000,"output_tokens":10}}}"#).unwrap());
    s.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T10:00:06Z","durationMs":6000}"#).unwrap());
    s.now_ms = t("2026-01-01T12:14:06Z");
    s
}

/// A30 `resume`: the resume hook says the cache likely expired.
pub(crate) fn a30_resume() -> State {
    let mut r = State::new(Pricing::bundled());
    r.session.alive = true;
    r.apply(&prompt("2026-01-01T10:00:00Z"));
    r.apply(&response("m1", "2026-01-01T10:00:05Z", 2, 0, 150_000));
    r.apply_hook(&crate::hooks::HookEvent::from_stdin(t("2026-01-01T10:00:10Z"), serde_json::json!({"hook_event_name":"SessionStart","source":"resume","seconds_since_last_response":7200,"context_tokens":150000,"prompt_cache_likely_expired":true,"estimated_cache_write_usd":1.1})).unwrap());
    r.now_ms = t("2026-01-01T10:00:20Z");
    r
}

// -- events axis (A06–A16) --------------------------------------------------

/// A06 `retrigger`: one compaction that left the context near the threshold.
pub(crate) fn a06_retrigger() -> State {
    let mut s = State::new(Pricing::bundled());
    s.apply(&prompt("2026-01-01T00:00:00Z"));
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:01Z","version":"2.1.270","message":{"id":"a","model":"claude-opus-5","content":[],"usage":{"input_tokens":900000}}}"#).unwrap());
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:02Z","version":"2.1.270","message":{"id":"b","model":"claude-opus-5","content":[],"usage":{"input_tokens":200000}}}"#).unwrap());
    s.apply(&Line::parse(r#"{"type":"system","subtype":"compact_boundary","timestamp":"2026-01-01T00:00:03Z","version":"2.1.270","compactMetadata":{"trigger":"auto","preTokens":960000,"postTokens":955000,"durationMs":80000}}"#).unwrap());
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:04Z","version":"2.1.270","message":{"id":"c","model":"claude-opus-5","content":[],"usage":{"input_tokens":955000}}}"#).unwrap());
    s
}

/// A06: two compactions that left plenty of room.
pub(crate) fn a06_two() -> State {
    let mut two = State::new(Pricing::bundled());
    for i in 0..2 {
        two.apply(&prompt("2026-01-01T00:00:00Z"));
        two.apply(&Line::parse(&format!(r#"{{"type":"system","subtype":"compact_boundary","timestamp":"2026-01-01T00:0{i}:03Z","version":"2.1.270","compactMetadata":{{"trigger":"auto","preTokens":960000,"postTokens":230000,"durationMs":80000}}}}"#)).unwrap());
        two.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T00:0{i}:04Z","version":"2.1.270","message":{{"id":"c{i}","model":"claude-opus-5","content":[],"usage":{{"input_tokens":230000}}}}}}"#)).unwrap());
    }
    two
}

/// A permission request for the Bash command `cmd`, with Claude Code's own
/// rule suggestion when it carried one.
pub(crate) fn permission_ask(s: &mut State, cmd: &str, suggestion: Option<&str>) {
    let mut payload = serde_json::json!({"hook_event_name":"PermissionRequest","tool_name":"Bash","tool_input":{"command":cmd}});
    if let Some(r) = suggestion {
        payload["permission_suggestions"] =
            serde_json::json!([{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":r}]}]);
    }
    s.apply_hook(&crate::hooks::HookEvent::from_stdin(0, payload).unwrap());
}

/// A09: three approvals of `gh api`, 45 seconds of waiting, Claude Code's
/// own suggestion for the rule.
pub(crate) fn a09() -> State {
    let mut s = State::new(Pricing::bundled());
    s.apply(&prompt("2026-01-01T00:00:00Z"));
    permission_ask(&mut s, "gh api repos/x", None);
    permission_ask(&mut s, "gh api repos/y --jq .id", Some("gh api:*"));
    permission_ask(&mut s, "gh api graphql", None);
    s.session.permission_waits = 3;
    s.session.permission_wait_ms = 45_000;
    s
}

/// A10: `cargo build --release` blocked the turn for 1:58.
pub(crate) fn a10() -> State {
    let mut s = State::new(Pricing::bundled());
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:00Z","message":{"id":"b1","model":"m","content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"cargo build --release"}}],"usage":{"output_tokens":1}}}"#).unwrap());
    s.apply(&Line::parse(r#"{"type":"user","timestamp":"2026-01-01T00:01:58Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"ok"}]}}"#).unwrap());
    s
}

/// A13: 80 % of the 5-hour limit used, exhausted 8:20 before the reset, on Opus.
pub(crate) fn a13() -> State {
    let mut s = State::new(Pricing::bundled());
    s.session.alive = true;
    s.now_ms = 1_000_000;
    s.limits = Some(crate::ui::state::Limits {
        five_hour_pct: 80.0,
        seven_day_pct: 10.0,
        five_hour_resets_at_ms: Some(2_000_000),
        seven_day_resets_at_ms: None,
        exhaustion_ms: Some(1_500_000),
        exhaustion_in_active_hours: None,
    });
    s.apply(&prompt("2026-01-01T00:00:00Z"));
    s.apply(&response("m", "2026-01-01T00:00:01Z", 10, 0, 1000));
    s
}

/// A13 `cheap`: the same on Sonnet, where only /effort medium is left.
pub(crate) fn a13_cheap() -> State {
    let mut s = a13();
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T00:00:03Z","message":{"id":"n","model":"claude-sonnet-5","content":[],"usage":{"input_tokens":10}}}"#).unwrap());
    s
}

/// A16: a 1.4 s hook on each of three turns.
pub(crate) fn a16() -> State {
    let mut s = State::new(Pricing::bundled());
    for i in 0..3 {
        s.apply(&prompt("2026-01-01T00:00:00Z"));
        s.apply(&response(
            &format!("h{i}"),
            "2026-01-01T00:00:01Z",
            10,
            0,
            100,
        ));
        s.apply(&Line::parse(r#"{"type":"system","subtype":"stop_hook_summary","timestamp":"2026-01-01T00:00:02Z","hookInfos":[{"command":"lint","durationMs":1400},{"command":"cctop hook","durationMs":5}],"hookErrors":[]}"#).unwrap());
        s.apply(&Line::parse(r#"{"type":"system","subtype":"turn_duration","timestamp":"2026-01-01T00:00:02Z","durationMs":8000}"#).unwrap());
    }
    s
}

// -- outcome axis (A32–A47) --------------------------------------------------

fn commit(s: &mut State, id: &str, ts: &str, sha: &str) {
    bash(
        s,
        id,
        ts,
        "git commit -m x",
        "1 file changed",
        &format!(
            r#","gitOperation":{{"commit":{{"sha":"{sha}","kind":"committed","branch":"main"}}}}"#
        ),
    )
}

/// A33: a commit with two source edits and no test since.
pub(crate) fn a33() -> State {
    let mut s = fresh();
    bash(
        &mut s,
        "t0",
        "2026-01-01T10:00:30Z",
        "npm test",
        "12 passed in 0.4s",
        "",
    );
    edit(&mut s, "e1", "2026-01-01T10:01:00Z", "/p/src/a.ts");
    edit(&mut s, "e2", "2026-01-01T10:02:00Z", "/p/src/b.ts");
    commit(&mut s, "c1", "2026-01-01T10:03:00Z", "a1b2c3d4e");
    s.now_ms = t("2026-01-01T10:03:01Z");
    s
}

/// A33 `failed`: a commit right after a failing test run.
pub(crate) fn a33_failed() -> State {
    let mut s = fresh();
    bash(
        &mut s,
        "t0",
        "2026-01-01T10:00:30Z",
        "npm test",
        "12 passed in 0.4s",
        "",
    );
    edit(&mut s, "e4", "2026-01-01T10:07:00Z", "/p/src/d.ts");
    bash(
        &mut s,
        "t2",
        "2026-01-01T10:08:00Z",
        "npm test",
        "1 failed, 12 passed in 0.4s",
        "",
    );
    commit(&mut s, "c3", "2026-01-01T10:09:00Z", "c3d4e5f6a");
    s.now_ms = t("2026-01-01T10:09:01Z");
    s
}

/// A38: three failed calls, two of one shape (`no_pr` twice, a rejected
/// push), their output carrying `marker` (a test puts a secret there to see
/// it never comes back).
pub(crate) fn a38_with(marker: &str) -> State {
    let mut s = fresh();
    let no_pr = format!("no pull requests found for branch {marker}");
    let rejected = format!("! [rejected] {marker} (non-fast-forward)");
    for (i, (cmd, out)) in [
        ("gh pr view 24", &no_pr),
        ("gh pr view --json url", &no_pr),
        ("git push", &rejected),
    ]
    .iter()
    .enumerate()
    {
        bash(
            &mut s,
            &format!("f{i}"),
            &format!("2026-01-01T10:0{}:00Z", i + 1),
            cmd,
            out,
            r#","is_error":true"#,
        );
    }
    s.now_ms = t("2026-01-01T10:04:00Z");
    s
}

pub(crate) fn a38() -> State {
    a38_with("main")
}

/// A40 before the diff figures arrive: a pushed PR, two edits.
pub(crate) fn a40_pushed() -> State {
    let mut s = fresh();
    s.status_facts.pr_number = Some(142);
    edit(&mut s, "e1", "2026-01-01T10:00:30Z", "/p/src/a.rs");
    edit(&mut s, "e2", "2026-01-01T10:00:40Z", "/p/README.md");
    bash(
        &mut s,
        "p",
        "2026-01-01T10:01:00Z",
        "git push -u origin feat",
        "ok",
        r#","gitOperation":{"push":{"branch":"feat"}}"#,
    );
    s
}

/// A40: the pushed PR with 380 source lines changed and no review pass.
pub(crate) fn a40() -> State {
    let mut s = a40_pushed();
    s.files.apply_numstat(
        std::path::Path::new("/p"),
        &[("src/a.rs".into(), 300, 80), ("README.md".into(), 200, 0)],
    );
    s
}

/// The tool result of a TaskUpdate that completed a task.
pub(crate) const TASK_DONE: &str = r#","toolUseResult":{"taskId":"1","updatedFields":["status"],"statusChange":{"from":"in_progress","to":"completed"},"success":true}"#;

/// A live session on a context of `ctx` tokens, one prompt in.
pub(crate) fn big(ctx: u64) -> State {
    let mut s = State::new(Pricing::bundled());
    s.session.alive = true;
    s.apply(&prompt("2026-01-01T10:00:00Z"));
    s.apply(&response("m0", "2026-01-01T10:00:01Z", 2, ctx, 0));
    s.now_ms = t("2026-01-01T10:00:02Z");
    s
}

/// A task completed on a context of `ctx` tokens.
pub(crate) fn task_done(s: &mut State, ctx: u64) {
    call_ctx(
        s,
        "tu",
        "2026-01-01T10:01:00Z",
        "TaskUpdate",
        r#"{"taskId":"1","status":"completed"}"#,
        "ok",
        TASK_DONE,
        ctx,
    );
}

/// A42: a task completed on a 312k context.
pub(crate) fn a42() -> State {
    let mut s = big(312_000);
    task_done(&mut s, 312_000);
    s
}

/// A43 on a clean tree: `git reset --hard` is an event only.
pub(crate) fn a43_clean() -> State {
    let mut s = fresh();
    bash(
        &mut s,
        "g",
        "2026-01-01T10:01:00Z",
        "git reset --hard HEAD~1",
        "ok",
        "",
    );
    s
}

/// A43: `git reset --hard` on a dirty tree.
pub(crate) fn a43() -> State {
    let mut s = a43_clean();
    s.session.git_dirty = true;
    s
}

/// A Bash call that auto mode blocked.
pub(crate) fn automode_blocked(s: &mut State, id: &str, ts: &str, cmd: &str) {
    call(
        s,
        id,
        ts,
        "Bash",
        &format!(r#"{{"command":"{cmd}"}}"#),
        "blocked",
        r#","is_error":true,"toolDenialKind":"automode-blocked""#,
    );
}

/// A45: auto mode blocked `gh api` twice this turn.
pub(crate) fn a45() -> State {
    let mut s = fresh();
    automode_blocked(&mut s, "d1", "2026-01-01T10:01:00Z", "gh api repos/x/y");
    automode_blocked(
        &mut s,
        "d2",
        "2026-01-01T10:02:00Z",
        "cd /p && gh api repos/x/z",
    );
    s
}

/// A47 on a five-hour 429 with its reset time and retry hint.
pub(crate) fn a47_five_hour() -> State {
    let mut s = fresh();
    s.apply(&Line::parse(r#"{"type":"assistant","timestamp":"2026-01-01T10:01:00Z","message":{"id":"e1","model":"<synthetic>","content":[{"type":"text","text":"Rate limit hit"}],"stop_reason":"stop_sequence","usage":{"input_tokens":0,"output_tokens":0}},"quotaLimits":{"status":"rejected","resetsAt":1767265200,"rateLimitType":"five_hour","lowPriorityRetryAfterSeconds":20},"error":"rate_limit","isApiErrorMessage":true,"apiErrorStatus":429}"#).unwrap());
    s.now_ms = t("2026-01-01T10:01:30Z");
    s
}

/// A47: the turn ended on the API error line `error`/`status`.
pub(crate) fn a47_error(error: &str, status: u16) -> State {
    let mut s = fresh();
    s.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T10:01:00Z","message":{{"id":"e1","model":"<synthetic>","content":[{{"type":"text","text":"x"}}],"stop_reason":"stop_sequence","usage":{{"input_tokens":0,"output_tokens":0}}}},"error":"{error}","isApiErrorMessage":true,"apiErrorStatus":{status}}}"#)).unwrap());
    s.now_ms = t("2026-01-01T10:01:30Z");
    s
}

/// A47 with a 429 whose quota line says `kind` (`,"rateLimitType":"…"` or empty).
pub(crate) fn a47_rate(kind: &str) -> State {
    let mut s = fresh();
    s.apply(&Line::parse(&format!(r#"{{"type":"assistant","timestamp":"2026-01-01T10:01:00Z","message":{{"id":"e1","model":"<synthetic>","content":[{{"type":"text","text":"Rate limit hit"}}],"stop_reason":"stop_sequence","usage":{{"input_tokens":0,"output_tokens":0}}}},"quotaLimits":{{"status":"rejected"{kind}}},"error":"rate_limit","isApiErrorMessage":true,"apiErrorStatus":429}}"#)).unwrap());
    s.now_ms = t("2026-01-01T10:01:30Z");
    s
}

/// A47 `rate_limit_model`: the weekly Opus limit.
pub(crate) fn a47_model_limit() -> State {
    a47_rate(r#","rateLimitType":"seven_day_opus""#)
}

/// Every witness, by name, for the copy table.
pub fn all() -> Vec<(&'static str, State)> {
    vec![
        ("a04_reads", a04_reads()),
        ("a04_churn", a04_churn()),
        ("a06_retrigger", a06_retrigger()),
        ("a06_two", a06_two()),
        ("a07_mcp", a07_mcp()),
        ("a07_plugin", a07_plugin()),
        ("a08_sonnet", a08_sonnet()),
        ("a08_fable", a08_fable()),
        ("a09", a09()),
        ("a10", a10()),
        ("a11_input", a11_input()),
        ("a11_paste", a11_paste()),
        ("a12", a12()),
        ("a13", a13()),
        ("a13_cheap", a13_cheap()),
        ("a14", a14()),
        ("a16", a16()),
        ("a19", a19()),
        ("a21", a21()),
        ("a21b", a21b()),
        ("a25", a25()),
        ("a27", a27()),
        ("a30_idle", a30_idle()),
        ("a30_resume", a30_resume()),
        ("a33", a33()),
        ("a33_failed", a33_failed()),
        ("a38", a38()),
        ("a40", a40()),
        ("a42", a42()),
        ("a43", a43()),
        ("a45", a45()),
        ("a47_five_hour", a47_five_hour()),
        ("a47_model_limit", a47_model_limit()),
        ("a47_spend", a47_error("spend_limit", 402)),
        ("a47_auth", a47_error("authentication_failed", 401)),
        ("a47_too_long", a47_error("prompt_too_long", 400)),
        ("a47_server", a47_error("server_error", 500)),
        ("a48", a48()),
    ]
}
