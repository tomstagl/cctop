//! `cctop query overview` end to end: the verb prints the overview object
//! for a fixture, and `--max-turns` cuts the per-turn charts.

use std::process::Command;

fn overview(extra: &[&str]) -> serde_json::Value {
    let home = std::env::temp_dir().join(format!("cctop-it-overview-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/session-b.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_cctop"))
        .args(["query", "overview", "--session", fixture])
        .args(extra)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn query_overview_prints_the_five_charts() {
    let v = overview(&[]);
    let ids: Vec<&str> = v["charts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["context", "spend", "cost_per_turn", "cache", "tool_calls"]
    );
    assert!(v["charts"][2]["label"].is_null());
    let v = overview(&["--max-turns", "2"]);
    assert_eq!(v["charts"][4]["label"], "last 2 turns");
    assert_eq!(
        v["charts"][3]["spec"]["series"][0]["points"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
