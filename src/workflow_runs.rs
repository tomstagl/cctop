//! A `Workflow` run's record (`<session dir>/workflows/wf_*.json`), read for
//! its identifiers and, per phase, where the script launches its agents.
//!
//! This is the record half; the derivation over records, journals and agents
//! lives beside it.

use std::collections::BTreeMap;
use std::path::Path;

use crate::agents::WorkflowJournal;
use crate::workflow_script::{pointer, Pointer};

/// One run record, as far as cctop keeps it: identifiers, times and one
/// pointer per phase — never the script, nor any prose the run produced.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowRecord {
    pub run: String,
    pub name: Option<String>,
    /// `"completed"` | `"killed"` | …
    pub status: Option<String>,
    pub start_ms: Option<i64>,
    pub duration_ms: Option<u64>,
    pub script_path: Option<String>,
    /// Phase title → where its agents are launched in the script.
    pub pointers: BTreeMap<String, Pointer>,
    /// The record file's modification time, epoch ms: a cache key only,
    /// never rendered or serialised.
    pub mtime_ms: Option<i64>,
    /// The (phase, label prefixes) the pointers were computed from: a cache
    /// key only, never rendered or serialised.
    pub scanned_with: Vec<(String, Vec<String>)>,
}

/// The fields of a run record cctop reads. Deliberately not read, so never
/// materialised: `detail` (per phase), `args`, `result`, `logs`, `summary`,
/// `error` and every other field; `script` is held only for the pointer scan
/// and dropped before the record is kept (spec §4.6).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRecord {
    run_id: Option<String>,
    workflow_name: Option<String>,
    status: Option<String>,
    start_time: Option<i64>,
    duration_ms: Option<u64>,
    script_path: Option<String>,
    script: Option<String>,
}

/// `<session dir>/workflows/wf_*.json`; `journals` supplies each phase's label
/// prefixes; `prev` is the last read, reused per file while its mtime and the
/// prefixes are unchanged, so a script is tokenised once per change rather
/// than once per tick. Unreadable or invalid files are skipped, as is anything
/// that is not `wf_*.json` (the `scripts/` directory lives beside the records).
/// A record whose run has no journal gets no pointers.
pub fn read_records(
    session_dir: &Path,
    journals: &[WorkflowJournal],
    prev: &[WorkflowRecord],
) -> Vec<WorkflowRecord> {
    let Ok(entries) = std::fs::read_dir(session_dir.join("workflows")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        let Some(stem) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|s| s.starts_with("wf_"))
            .map(str::to_owned)
        else {
            continue;
        };
        let Ok(meta) = e.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        if let Some(p) = prev.iter().find(|p| {
            p.run == stem
                && mtime_ms.is_some()
                && p.mtime_ms == mtime_ms
                && p.scanned_with == scan_key(journals, &p.run)
        }) {
            out.push(p.clone());
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(raw) = serde_json::from_str::<RawRecord>(&text) else {
            continue;
        };
        drop(text);
        let run = raw.run_id.unwrap_or(stem);
        let scanned_with = scan_key(journals, &run);
        let pointers = match raw.script.as_deref() {
            Some(script) => scanned_with
                .iter()
                .filter_map(|(phase, prefixes)| {
                    pointer(script, phase, prefixes).map(|p| (phase.clone(), p))
                })
                .collect(),
            None => BTreeMap::new(),
        };
        out.push(WorkflowRecord {
            run,
            name: raw.workflow_name,
            status: raw.status,
            start_ms: raw.start_time,
            duration_ms: raw.duration_ms,
            script_path: raw.script_path,
            pointers,
            mtime_ms,
            scanned_with,
        });
    }
    out.sort_by(|a, b| a.run.cmp(&b.run));
    out
}

/// The (phase, label prefixes) of `run`'s journal, in phase order; empty
/// without a journal.
fn scan_key(journals: &[WorkflowJournal], run: &str) -> Vec<(String, Vec<String>)> {
    journals
        .iter()
        .find(|j| j.run == run)
        .map(|j| {
            j.phases
                .iter()
                .map(|p| (p.title.clone(), p.label_prefixes.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow_script::{Call, Pointer};

    #[test]
    fn a_record_keeps_identifiers_and_pointers_only() {
        let dir = std::env::temp_dir().join(format!("cctop-wf-record-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let wf = dir.join("workflows");
        std::fs::create_dir_all(&wf).unwrap();
        std::fs::write(wf.join("wf_t.json"), serde_json::json!({
            "runId": "wf_t", "workflowName": "sweep", "status": "completed", "startTime": 1000,
            "durationMs": 50, "scriptPath": "/x/sweep-2.js",
            "script": "phase('Verify')\nconst v = await pipeline(xs,\n c => agent(p, {label: `verify:${c}`, phase: 'Verify'}))\n",
            "result": "PROSE", "logs": ["PROSE"], "summary": "PROSE", "args": {"q": "PROSE"},
            "phases": [{"title": "Verify", "detail": "PROSE"}]
        }).to_string()).unwrap();
        let j = crate::agents::WorkflowJournal {
            run: "wf_t".into(),
            phases: vec![crate::agents::JournalPhase {
                title: "Verify".into(),
                label_prefixes: vec!["verify".into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let records = read_records(&dir, &[j], &[]);
        let _ = std::fs::remove_dir_all(&dir);
        let r = &records[0];
        assert_eq!(r.name.as_deref(), Some("sweep"));
        assert_eq!(
            r.pointers["Verify"],
            Pointer {
                line: 2,
                call: Call::Pipeline
            }
        );
        assert!(!format!("{r:?}").contains("PROSE"));
    }

    #[test]
    fn a_record_is_rescanned_only_when_it_changes() {
        let dir = std::env::temp_dir().join(format!("cctop-wf-rescan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let wf = dir.join("workflows");
        std::fs::create_dir_all(wf.join("scripts")).unwrap();
        std::fs::write(wf.join("scripts").join("sweep-2.js"), "not a record").unwrap();
        std::fs::write(wf.join("wf_bad.json"), "{torn").unwrap();
        std::fs::write(wf.join("wf_t.json"), serde_json::json!({
            "runId": "wf_t", "workflowName": "sweep",
            "script": "phase('Verify')\nconst v = await pipeline(xs,\n c => agent(p, {label: `verify:${c}`, phase: 'Verify'}))\n"
        }).to_string()).unwrap();
        let journal = |prefix: &str| crate::agents::WorkflowJournal {
            run: "wf_t".into(),
            phases: vec![crate::agents::JournalPhase {
                title: "Verify".into(),
                label_prefixes: vec![prefix.into()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let sentinel = Pointer {
            line: 999,
            call: Call::Parallel,
        };
        let mut first = read_records(&dir, &[journal("verify")], &[]);
        assert_eq!(
            first.len(),
            1,
            "only wf_*.json files that parse are records"
        );
        first[0].pointers.insert("Verify".into(), sentinel);
        let reused = read_records(&dir, &[journal("verify")], &first);
        let rescanned = read_records(&dir, &[journal("check")], &first);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            reused[0].pointers["Verify"], sentinel,
            "unchanged file and prefixes: reused"
        );
        assert_ne!(
            rescanned[0].pointers.get("Verify"),
            Some(&sentinel),
            "new prefix: re-scanned"
        );
    }
}
