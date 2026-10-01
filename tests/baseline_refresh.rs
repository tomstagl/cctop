//! Issue #15, end to end: `cctop query` for a live session answers at once
//! from a stale baseline cache, and a detached refresh rewrites the cache
//! behind it. Before the fix the query recomputed the 7-day baseline inline,
//! and over a heavy week that outlasted the pane's 5 s timeout.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

const SESSION: &str = "0e15ba5e-0000-4000-8000-00000000000f";

fn baseline(home: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(home.join(".cctop/baseline.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn query_serves_the_stale_baseline_and_a_detached_refresh_rewrites_it() {
    let home = std::env::temp_dir().join(format!("cctop-it-baseline-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    let cwd = home.join("work");
    std::fs::create_dir_all(&cwd).unwrap();
    // A live session: the registry names this test's own pid.
    let sessions = home.join(".claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let entry = serde_json::json!({
        "pid": std::process::id(),
        "sessionId": SESSION,
        "cwd": cwd,
        "startedAt": 0,
        "status": "idle",
        "updatedAt": 0,
    });
    std::fs::write(
        sessions.join(format!("{}.json", std::process::id())),
        entry.to_string(),
    )
    .unwrap();
    let project = home.join(".claude/projects").join(cctop::slug(&cwd));
    std::fs::create_dir_all(&project).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
    // Written just now: macOS keeps the fixture's mtime on a copy, and the
    // week counts from the mtime.
    let transcript = project.join(format!("{SESSION}.jsonl"));
    std::fs::copy(&fixture, &transcript).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&transcript)
        .unwrap()
        .set_modified(std::time::SystemTime::now())
        .unwrap();
    // A cache from the epoch: stale, and marked so it is recognisable.
    std::fs::create_dir_all(home.join(".cctop")).unwrap();
    std::fs::write(
        home.join(".cctop/baseline.json"),
        r#"{"computed_at_ms":0,"days":7,"sessions":4242,"cost_per_turn":null,"tokens_per_turn":null,"cache_hit_ratio":null,"tool_error_rate":null,"model_mix":{}}"#,
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_cctop"))
        .args([
            "query",
            "summary",
            "--session",
            SESSION,
            "--surface",
            "pane",
        ])
        .env("HOME", &home)
        .env_remove("CCTOP_HOME")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        baseline(&home)["sessions"],
        4242,
        "the query itself computed nothing"
    );

    // The detached refresh replaces the cache and frees its lock.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let b = baseline(&home);
        if b["computed_at_ms"].as_i64().unwrap_or(0) > 0 {
            assert_eq!(b["sessions"], 1, "computed over the one fixture session");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no refresh wrote the cache within 30 s"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let lock_deadline = Instant::now() + Duration::from_secs(5);
    while home.join(".cctop/baseline.lock").exists() {
        assert!(
            Instant::now() < lock_deadline,
            "the refresh left its lock behind"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&home);
}
