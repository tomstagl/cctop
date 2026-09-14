//! The coach measured on its own record (US-012): every fire the engine
//! kept in `~/.cctop/<session>.advisor.json`, aggregated per rule —
//! exposed fires (a surface showed it), acted, snoozed, `X`, reflex
//! dismissals (x within 2 s), the view toggled away within 10 s, expired
//! unacted — against the control arm (fires recorded while the coach was
//! off, `--coach off|auto`) and the corpus replay (`coach-replay`); the
//! exposure assignment (`on` / `off` alternating per stratum); the demotion
//! rule; and what the coach itself costs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::advisor::{Demotion, FireRecord, Persisted};

/// Exposed fires a rule needs before its record demotes it.
pub const DEMOTION_MIN_FIRES: usize = 10;
/// False positives (snoozed + expired unacted, corrected for acted-anyway)
/// over exposed fires: past this, the next row.
pub const DEMOTION_FP_RATE: f64 = 0.20;
/// A precision collapse on the current version: at least this many exposed
/// fires on it, at least this false-positive rate, while the rule's
/// all-version rate stays under the bar.
pub const COLLAPSE_MIN_FIRES: usize = 5;
pub const COLLAPSE_FP_RATE: f64 = 0.50;

/// One session's advisor file, as read for the stats.
#[derive(Debug, Clone)]
pub struct SessionRecords {
    pub session_id: String,
    pub exposure: String,
    pub records: Vec<FireRecord>,
    pub cost: Cost,
}

/// What the coach itself costs, per session, written by the TUI.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// Prompts sent to the session through the socket (the act popup's S).
    pub socket_sends: usize,
    /// Characters of those prompts (≈ tokens × 4).
    pub socket_chars: usize,
    /// CPU seconds this cctop process used (the 1 s tick, the pollers).
    pub cpu_s: f64,
    /// `git` shell-outs (status, numstat, diff) the session made.
    pub git_shellouts: usize,
    /// Hook latency Claude Code measured for the previous session here
    /// (`~/.claude.json lastSessionMetrics`): p50 / p95 / p99 ms.
    pub hook_ms: Option<(u64, u64, u64)>,
}

/// Every advisor file under `home` whose newest fire is after `since_ms`
/// (or that has no fire at all and was written after it).
pub fn load(home: &Path, since_ms: i64) -> Vec<SessionRecords> {
    let Ok(entries) = std::fs::read_dir(home) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".advisor.json"))
        })
        .collect();
    paths.sort();
    for p in paths {
        let Ok(text) = std::fs::read_to_string(&p) else {
            continue;
        };
        let Ok(persisted) = serde_json::from_str::<Persisted>(&text) else {
            continue;
        };
        let newest = persisted
            .records
            .iter()
            .map(|r| r.shown_at_ms)
            .max()
            .or_else(|| {
                p.metadata()
                    .ok()?
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis() as i64)
            })
            .unwrap_or(0);
        if newest < since_ms {
            continue;
        }
        let session_id = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .trim_end_matches(".advisor.json")
            .to_string();
        out.push(SessionRecords {
            session_id,
            exposure: persisted.exposure.clone(),
            records: persisted.records,
            cost: persisted.cost,
        });
    }
    out
}

/// One rule's row of the stats table.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RuleStats {
    pub rule: String,
    pub family: String,
    /// Fires a surface showed.
    pub exposed: usize,
    pub acted: usize,
    pub snoozed: usize,
    /// Snoozed for the session (`X`, or a third `x`).
    pub session_snoozes: usize,
    /// `x` within 2 s of the showing.
    pub reflex: usize,
    /// The coach view left within 10 s of the promotion.
    pub toggled_away: usize,
    pub expired_unacted: usize,
    /// Fires on the control arm (no surface), and how many of those the
    /// person acted on anyway: the baseline the causal estimate subtracts.
    pub control: usize,
    pub control_acted: usize,
    /// (snoozed + expired unacted − acted-anyway share × those) / exposed.
    pub false_positive_rate: Option<f64>,
    /// acted / exposed − control_acted / control.
    pub causal_lift: Option<f64>,
    /// The exposed fires on the newest Claude Code version seen, and their
    /// false-positive rate (a collapse demotes the rule to LATER).
    pub newest_version: String,
    pub newest_version_exposed: usize,
    pub newest_version_fp_rate: Option<f64>,
    pub demotion: Option<Demotion>,
}

fn is_acted(r: &FireRecord) -> bool {
    r.retired.as_ref().is_some_and(|(w, _)| w == "acted")
}

fn is_expired(r: &FireRecord) -> bool {
    r.retired.as_ref().is_some_and(|(w, _)| w == "expired")
}

/// Version ordering as Claude Code numbers them (`2.1.270` > `2.1.99`).
fn version_key(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// Aggregate every session's records per rule.
pub fn stats(sessions: &[SessionRecords]) -> Vec<RuleStats> {
    let mut by_rule: BTreeMap<String, Vec<&FireRecord>> = BTreeMap::new();
    for s in sessions {
        for r in &s.records {
            by_rule.entry(r.rule.clone()).or_default().push(r);
        }
    }
    let mut out = Vec::new();
    for (rule, records) in by_rule {
        let exposed: Vec<&FireRecord> = records
            .iter()
            .copied()
            .filter(|r| r.surface != "none")
            .collect();
        let control: Vec<&FireRecord> = records
            .iter()
            .copied()
            .filter(|r| r.surface == "none")
            .collect();
        let acted = exposed.iter().filter(|r| is_acted(r)).count();
        let snoozed = exposed.iter().filter(|r| r.snoozed).count();
        let expired = exposed.iter().filter(|r| is_expired(r)).count();
        let control_acted = control.iter().filter(|r| is_acted(r)).count();
        let anyway = if control.is_empty() {
            0.0
        } else {
            control_acted as f64 / control.len() as f64
        };
        let fp = |n_exposed: usize, snoozed: usize, expired: usize| -> Option<f64> {
            (n_exposed > 0).then(|| {
                // Expired fires the person would have acted on anyway are
                // not the coach's misses.
                let expired = expired as f64 * (1.0 - anyway);
                (snoozed as f64 + expired) / n_exposed as f64
            })
        };
        let newest_version = exposed
            .iter()
            .map(|r| r.version.as_str())
            .filter(|v| !v.is_empty())
            .max_by_key(|v| version_key(v))
            .unwrap_or("")
            .to_string();
        let on_newest: Vec<&FireRecord> = exposed
            .iter()
            .copied()
            .filter(|r| !newest_version.is_empty() && r.version == newest_version)
            .collect();
        let newest_fp = fp(
            on_newest.len(),
            on_newest.iter().filter(|r| r.snoozed).count(),
            on_newest.iter().filter(|r| is_expired(r)).count(),
        );
        let false_positive_rate = fp(exposed.len(), snoozed, expired);
        let demotion = if exposed.len() >= DEMOTION_MIN_FIRES
            && false_positive_rate.is_some_and(|r| r > DEMOTION_FP_RATE)
        {
            Some(Demotion::NextRow)
        } else if on_newest.len() >= COLLAPSE_MIN_FIRES
            && newest_fp.is_some_and(|r| r >= COLLAPSE_FP_RATE)
            && false_positive_rate.is_some_and(|r| r <= DEMOTION_FP_RATE)
        {
            Some(Demotion::Later)
        } else {
            None
        };
        out.push(RuleStats {
            rule,
            family: records
                .first()
                .map(|r| r.family.clone())
                .unwrap_or_default(),
            exposed: exposed.len(),
            acted,
            snoozed,
            session_snoozes: exposed.iter().filter(|r| r.snoozed_session).count(),
            reflex: exposed
                .iter()
                .filter(|r| r.time_to_x_ms.is_some_and(|t| t < 2_000))
                .count(),
            toggled_away: exposed.iter().filter(|r| r.toggled_away).count(),
            expired_unacted: expired,
            control: control.len(),
            control_acted,
            false_positive_rate,
            causal_lift: (!exposed.is_empty() && !control.is_empty())
                .then(|| acted as f64 / exposed.len() as f64 - anyway),
            newest_version,
            newest_version_exposed: on_newest.len(),
            newest_version_fp_rate: newest_fp,
            demotion,
        });
    }
    out
}

/// The rules the record demotes, for the engine to apply at attach.
pub fn demotions(home: &Path, since_ms: i64) -> BTreeMap<String, Demotion> {
    stats(&load(home, since_ms))
        .into_iter()
        .filter_map(|s| s.demotion.map(|d| (s.rule, d)))
        .collect()
}

/// The coach's cost summed over the sessions.
pub fn cost(sessions: &[SessionRecords]) -> Cost {
    let mut c = Cost::default();
    for s in sessions {
        c.socket_sends += s.cost.socket_sends;
        c.socket_chars += s.cost.socket_chars;
        c.cpu_s += s.cost.cpu_s;
        c.git_shellouts += s.cost.git_shellouts;
        if s.cost.hook_ms.is_some() {
            c.hook_ms = s.cost.hook_ms;
        }
    }
    c
}

fn pct(v: Option<f64>) -> String {
    v.map(|x| format!("{:.0} %", x * 100.0))
        .unwrap_or_else(|| "—".into())
}

/// The table as text: one row per rule, the arms and the cost below.
pub fn table(sessions: &[SessionRecords], replay: Option<&crate::replay::Replay>) -> String {
    let rows = stats(sessions);
    let on = sessions.iter().filter(|s| s.exposure != "off").count();
    let off = sessions.len() - on;
    let mut s = format!(
        "{} sessions · {on} coach on · {off} coach off\n{:<6} {:>7} {:>5} {:>7} {:>3} {:>6} {:>6} {:>7} {:>8} {:>7} {:>6} {:>7} {}\n",
        sessions.len(),
        "rule",
        "exposed",
        "acted",
        "snoozed",
        "X",
        "reflex",
        "toggle",
        "expired",
        "control",
        "fp",
        "lift",
        "replay",
        "verdict"
    );
    for r in &rows {
        let replay_acted = replay
            .and_then(|rp| rp.by_rule.get(&r.rule))
            .filter(|t| t.fires > 0)
            .map(|t| format!("{:.0} %", t.acted as f64 * 100.0 / t.fires as f64))
            .unwrap_or_else(|| "—".into());
        let verdict = match r.demotion {
            Some(Demotion::NextRow) => "demote: next row".to_string(),
            Some(Demotion::Later) => format!("demote: LATER (v{})", r.newest_version),
            None if r.exposed < DEMOTION_MIN_FIRES => {
                format!("{} more exposed fires", DEMOTION_MIN_FIRES - r.exposed)
            }
            None => "keeps its slot".into(),
        };
        s.push_str(&format!(
            "{:<6} {:>7} {:>5} {:>7} {:>3} {:>6} {:>6} {:>7} {:>8} {:>6} {:>7} {:>7} {}\n",
            r.rule,
            r.exposed,
            r.acted,
            r.snoozed,
            r.session_snoozes,
            r.reflex,
            r.toggled_away,
            r.expired_unacted,
            format!("{}/{}", r.control_acted, r.control),
            pct(r.false_positive_rate),
            r.causal_lift
                .map(|l| format!("{:+.0} pt", l * 100.0))
                .unwrap_or_else(|| "—".into()),
            replay_acted,
            verdict
        ));
    }
    let c = cost(sessions);
    s.push_str(&format!(
        "cost: {} socket sends (≈{} tokens) · cpu {:.1} s · {} git shell-outs · hooks p99 {}\n",
        c.socket_sends,
        c.socket_chars / 4,
        c.cpu_s,
        c.git_shellouts,
        c.hook_ms
            .map(|(_, _, p99)| format!("{p99} ms"))
            .unwrap_or_else(|| "—".into())
    ));
    s
}

/// `~/.cctop/exposure.json`: per stratum (project, model family, Claude
/// Code version), how many sessions were assigned so far; `auto`
/// alternates on / off within each stratum.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Assignments {
    #[serde(default)]
    pub strata: BTreeMap<String, u64>,
}

/// The family of a model id, for the stratum.
pub fn model_family(model: &str) -> &'static str {
    for f in ["fable", "opus", "sonnet", "haiku", "mythos"] {
        if model.contains(f) {
            return match f {
                "fable" => "fable",
                "opus" => "opus",
                "sonnet" => "sonnet",
                "haiku" => "haiku",
                _ => "mythos",
            };
        }
    }
    "other"
}

/// Assign this session an arm: `on` / `off` as given, or `auto` — the next
/// in its stratum's alternation, the count written back.
pub fn assign(home: &Path, mode: &str, project: &str, model: &str, version: &str) -> bool {
    match mode {
        "off" => return false,
        "auto" => {}
        _ => return true,
    }
    let path = home.join("exposure.json");
    let mut a: Assignments = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let stratum = format!("{project}|{}|{version}", model_family(model));
    let n = a.strata.entry(stratum).or_default();
    let on = *n % 2 == 0;
    *n += 1;
    let _ = std::fs::create_dir_all(home);
    let _ = std::fs::write(&path, serde_json::to_string_pretty(&a).unwrap_or_default());
    on
}

/// This process's CPU time so far (user + system), seconds.
pub fn process_cpu_s() -> f64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: `getrusage` fills the struct we own for this process.
    let rc = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    if rc != 0 {
        return 0.0;
    }
    let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
    secs(usage.ru_utime) + secs(usage.ru_stime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisor::{SessionMode, Urgency};

    fn record(rule: &str, surface: &str, retired: Option<&str>, version: &str) -> FireRecord {
        FireRecord {
            rule: rule.into(),
            family: "verify-gap".into(),
            class: Urgency::Next,
            turn: 1,
            shown_at_ms: 1_000,
            retired: retired.map(|w| (w.to_string(), 2_000)),
            acted_delay_ms: None,
            snoozed: retired == Some("snoozed"),
            session_mode: SessionMode::Interactive,
            surface: surface.into(),
            human_idle_ms: Some(0),
            time_to_x_ms: (retired == Some("snoozed")).then_some(1_500),
            snoozed_session: false,
            toggled_away: false,
            version: version.into(),
            model: "claude-opus-5".into(),
            project: "/p".into(),
        }
    }

    #[test]
    fn stats_rates_demotions_and_the_control_arm() {
        // A32: 12 exposed, 5 acted, 4 snoozed (reflex), 3 expired; control
        // 4 fires of which 2 acted anyway.
        let mut records = Vec::new();
        for i in 0..12 {
            let retired = match i % 4 {
                0 | 1 => Some("acted"),
                2 => Some("snoozed"),
                _ => Some("expired"),
            };
            records.push(record("A32", "tui-coach", retired, "2.1.270"));
        }
        records.pop();
        records.push(record("A32", "tui-coach", Some("acted"), "2.1.270"));
        for i in 0..4 {
            records.push(record(
                "A32",
                "none",
                Some(if i < 2 { "acted" } else { "expired" }),
                "2.1.270",
            ));
        }
        // A41: 3 exposed on the old version fine, 5 on the new all snoozed.
        for _ in 0..3 {
            records.push(record("A41", "pane", Some("acted"), "2.1.269"));
        }
        for _ in 0..5 {
            records.push(record("A41", "pane", Some("snoozed"), "2.1.270"));
        }
        let sessions = vec![SessionRecords {
            session_id: "s1".into(),
            exposure: "on".into(),
            records,
            cost: Cost {
                socket_sends: 2,
                socket_chars: 400,
                cpu_s: 1.5,
                git_shellouts: 7,
                hook_ms: Some((10, 20, 30)),
            },
        }];
        let rows = stats(&sessions);
        let a32 = rows.iter().find(|r| r.rule == "A32").unwrap();
        assert_eq!(
            (a32.exposed, a32.acted, a32.snoozed, a32.expired_unacted),
            (12, 7, 3, 2)
        );
        assert_eq!((a32.control, a32.control_acted), (4, 2));
        assert_eq!(a32.reflex, 3, "x within 2 s");
        // fp = (3 + 2 × (1 − 0.5)) / 12 = 4/12
        assert!((a32.false_positive_rate.unwrap() - 4.0 / 12.0).abs() < 1e-9);
        assert!((a32.causal_lift.unwrap() - (7.0 / 12.0 - 0.5)).abs() < 1e-9);
        assert_eq!(
            a32.demotion,
            Some(Demotion::NextRow),
            "past 20 % with ≥ 10 fires"
        );
        let a41 = rows.iter().find(|r| r.rule == "A41").unwrap();
        assert_eq!(a41.newest_version, "2.1.270");
        assert_eq!(a41.newest_version_exposed, 5);
        assert_eq!(a41.newest_version_fp_rate, Some(1.0));
        assert_eq!(
            a41.demotion, None,
            "8 fires with 5 snoozed is past the bar overall, but under 10 fires"
        );
        let text = table(&sessions, None);
        assert!(
            text.starts_with("1 sessions · 1 coach on · 0 coach off"),
            "{text}"
        );
        assert!(
            text.contains("A32") && text.contains("demote: next row"),
            "{text}"
        );
        assert!(text.contains("cost: 2 socket sends (≈100 tokens) · cpu 1.5 s · 7 git shell-outs · hooks p99 30 ms"), "{text}");
        // A precision collapse on the newest version: LATER.
        let mut records = Vec::new();
        for _ in 0..30 {
            records.push(record("A45", "tui-dashboard", Some("acted"), "2.1.269"));
        }
        for _ in 0..5 {
            records.push(record("A45", "tui-dashboard", Some("snoozed"), "2.1.271"));
        }
        let sessions = vec![SessionRecords {
            session_id: "s2".into(),
            exposure: "on".into(),
            records,
            cost: Cost::default(),
        }];
        let a45 = &stats(&sessions)[0];
        assert_eq!(a45.demotion, Some(Demotion::Later));
        assert!(table(&sessions, None).contains("demote: LATER (v2.1.271)"));
    }

    #[test]
    fn files_load_by_recency_and_auto_alternates_per_stratum() {
        let home = std::env::temp_dir().join(format!("cctop-coach-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let mut p = Persisted::default();
        p.version = 2;
        p.records
            .push(record("A32", "tui-coach", Some("acted"), "2.1.270"));
        p.exposure = "on".into();
        std::fs::write(
            home.join("s1.advisor.json"),
            serde_json::to_string(&p).unwrap(),
        )
        .unwrap();
        let mut old = p.clone();
        old.records[0].shown_at_ms = 10;
        old.exposure = "off".into();
        std::fs::write(
            home.join("s0.advisor.json"),
            serde_json::to_string(&old).unwrap(),
        )
        .unwrap();
        std::fs::write(home.join("s1.advisor.lock"), "1").unwrap();
        let all = load(&home, 0);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].session_id, "s0");
        assert_eq!(all[0].exposure, "off");
        let recent = load(&home, 500);
        assert_eq!(recent.len(), 1, "s0's fire is older than the cutoff");
        assert_eq!(demotions(&home, 0).len(), 0);
        // auto: on, off, on within a stratum; another stratum starts anew.
        assert!(assign(&home, "auto", "/p", "claude-opus-5", "2.1.270"));
        assert!(!assign(&home, "auto", "/p", "claude-opus-5", "2.1.270"));
        assert!(assign(&home, "auto", "/p", "claude-opus-5", "2.1.270"));
        assert!(assign(&home, "auto", "/p", "claude-sonnet-5", "2.1.270"));
        assert!(assign(&home, "on", "/p", "claude-opus-5", "2.1.270"));
        assert!(!assign(&home, "off", "/p", "claude-opus-5", "2.1.270"));
        let a: Assignments =
            serde_json::from_str(&std::fs::read_to_string(home.join("exposure.json")).unwrap())
                .unwrap();
        assert_eq!(a.strata["/p|opus|2.1.270"], 3);
        assert_eq!(a.strata["/p|sonnet|2.1.270"], 1);
        assert_eq!(model_family("claude-fable-5-1"), "fable");
        assert!(process_cpu_s() >= 0.0);
        let _ = std::fs::remove_dir_all(&home);
    }
}
