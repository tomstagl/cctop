//! Session lineage: the chain a transcript belongs to. Ancestors are found
//! through `derived-from` (the first entry of the last such line of a
//! transcript that names another session), descendants through `continued-in`. Each relative is looked
//! up by id as `<id>.jsonl`, in this transcript's project directory first
//! and then in every project directory under `projects_root`. The walk is
//! bounded (`MAX_DEPTH`) and guarded against cycles; `--lines` does not
//! apply to relatives. Only ids, `via` tokens, timestamps and costs are
//! kept.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::transcript::Line;

/// The walk stops after this many hops in either direction.
pub const MAX_DEPTH: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Member {
    pub id: String,
    /// How this session began (its own `derived-from` entry): `clear`,
    /// `compact`, `subprocess`, … shown as written.
    pub via: Option<String>,
    pub started_at: Option<String>,
    pub cost_usd: Option<f64>,
    pub transcript_found: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Lineage {
    /// Oldest first, ending at the parent of `this`.
    pub ancestors: Vec<Member>,
    pub this: Member,
    /// Nearest first.
    pub descendants: Vec<Member>,
    /// The found members' costs, `this` included.
    pub chain_cost_usd: f64,
    /// Relatives named by a link whose transcript is not on disk.
    pub missing: usize,
}

/// What one transcript says about its place in a chain.
struct Scan {
    id: String,
    /// `(id, via)` of the first entry of the last `derived-from` line.
    derived: Option<(String, String)>,
    continued_in: Option<String>,
    started_at: Option<String>,
    cost_usd: Option<f64>,
}

fn scan(path: &Path) -> Scan {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let info = crate::ui::state::SessionInfo::from_transcript(path);
    let state = crate::load::state_from(path, info);
    let mut derived = None;
    let mut continued_in = None;
    let mut started_at = None;
    for line in crate::transcript::parse_file(path).unwrap_or_default() {
        if started_at.is_none() {
            started_at = line.timestamp().map(str::to_string);
        }
        match &line {
            Line::DerivedFrom(d) => {
                let own: Vec<&str> = [Some(stem.as_str()), state.agg.session_id.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect();
                derived = d.parent(&own).map(|s| (s.id.clone(), s.via.clone()));
            }
            Line::ContinuedIn(c) => {
                if let Some(id) = c.continued_in_session_id.clone().filter(|i| !i.is_empty()) {
                    continued_in = Some(id);
                }
            }
            _ => {}
        }
    }
    Scan {
        id: state.agg.session_id.clone().unwrap_or(stem),
        derived,
        continued_in,
        started_at,
        cost_usd: state.cost_combined().map(|c| c.usd),
    }
}

fn member(s: &Scan) -> Member {
    Member {
        id: s.id.clone(),
        via: s
            .derived
            .as_ref()
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty()),
        started_at: s.started_at.clone(),
        cost_usd: s.cost_usd,
        transcript_found: true,
    }
}

fn absent(id: &str) -> Member {
    Member {
        id: id.to_string(),
        via: None,
        started_at: None,
        cost_usd: None,
        transcript_found: false,
    }
}

/// `<id>.jsonl` beside `dir` first, then under any project directory.
fn find(id: &str, dir: Option<&Path>, projects_root: &Path) -> Option<PathBuf> {
    if id.is_empty() || id.contains(['/', '\\']) {
        return None;
    }
    if let Some(p) = dir
        .map(|d| d.join(format!("{id}.jsonl")))
        .filter(|p| p.is_file())
    {
        return Some(p);
    }
    std::fs::read_dir(projects_root)
        .ok()?
        .flatten()
        .map(|e| e.path().join(format!("{id}.jsonl")))
        .find(|p| p.is_file())
}

/// Walk from `start` one link at a time; `next` reads the link out of a
/// scan. Stops at the first missing transcript, a repeated id, or
/// `MAX_DEPTH`.
fn walk(
    start: &Scan,
    dir: Option<&Path>,
    projects_root: &Path,
    seen: &mut HashSet<String>,
    next: impl Fn(&Scan) -> Option<String>,
) -> (Vec<Member>, usize) {
    let mut out = Vec::new();
    let mut missing = 0;
    let mut link = next(start);
    for _ in 0..MAX_DEPTH {
        let Some(id) = link.take() else { break };
        if !seen.insert(id.clone()) {
            break;
        }
        match find(&id, dir, projects_root) {
            Some(p) => {
                let s = scan(&p);
                out.push(member(&s));
                link = next(&s);
            }
            None => {
                out.push(absent(&id));
                missing += 1;
            }
        }
    }
    (out, missing)
}

pub fn chain(session_path: &Path, projects_root: &Path) -> Lineage {
    let here = scan(session_path);
    let dir = session_path.parent();
    let mut seen = HashSet::from([here.id.clone()]);
    let (mut ancestors, m1) = walk(&here, dir, projects_root, &mut seen, |s| {
        s.derived.as_ref().map(|(id, _)| id.clone())
    });
    ancestors.reverse();
    let (descendants, m2) = walk(&here, dir, projects_root, &mut seen, |s| {
        s.continued_in.clone()
    });
    let this = member(&here);
    let chain_cost_usd = ancestors
        .iter()
        .chain(std::iter::once(&this))
        .chain(&descendants)
        .filter_map(|m| m.cost_usd)
        .sum();
    Lineage {
        ancestors,
        this,
        descendants,
        chain_cost_usd,
        missing: m1 + m2,
    }
}

/// The header's `from <id8> · <via>` for a `(id, via)` pair.
pub fn header_text(id: &str, via: &str) -> String {
    let short: String = id.chars().take(8).collect();
    if via.is_empty() {
        format!("from {short}")
    } else {
        format!("from {short} · {via}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant(ts: &str) -> String {
        format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"id":"m1","model":"claude-sonnet-5","role":"assistant","content":[{{"type":"text","text":"x"}}],"usage":{{"input_tokens":1000,"output_tokens":1000,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}}}}}"#
        )
    }

    fn write(dir: &Path, id: &str, derived: Option<(&str, &str)>, cont: Option<&str>) {
        std::fs::create_dir_all(dir).unwrap();
        let mut s = String::new();
        if let Some((from, via)) = derived {
            s += &format!(
                r#"{{"type":"derived-from","sessionId":"{id}","sessions":[{{"type":"claude_code_session","id":"{from}","via":"{via}"}}]}}"#
            );
            s.push('\n');
        }
        s += &assistant("2026-11-27T10:00:00.000Z");
        s.push('\n');
        if let Some(c) = cont {
            s += &format!(
                r#"{{"type":"continued-in","timestamp":"2026-11-27T11:00:00.000Z","sessionId":"{id}","continuedInSessionId":"{c}"}}"#
            );
            s.push('\n');
        }
        std::fs::write(dir.join(format!("{id}.jsonl")), s).unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cctop-lineage-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn three_session_chain_with_costs() {
        let root = tmp("chain");
        let p = root.join("proj-a");
        write(&p, "aaa", None, Some("bbb"));
        write(&p, "bbb", Some(("aaa", "clear")), Some("ccc"));
        write(&p, "ccc", Some(("bbb", "compact")), None);
        let l = chain(&p.join("bbb.jsonl"), &root);
        assert_eq!(l.ancestors.len(), 1);
        assert_eq!(l.ancestors[0].id, "aaa");
        assert_eq!(l.this.id, "bbb");
        assert_eq!(l.this.via.as_deref(), Some("clear"));
        assert_eq!(l.descendants.len(), 1);
        assert_eq!(l.descendants[0].id, "ccc");
        assert_eq!(l.descendants[0].via.as_deref(), Some("compact"));
        assert_eq!(l.missing, 0);
        let each = l.this.cost_usd.unwrap();
        assert!(each > 0.0);
        assert!((l.chain_cost_usd - 3.0 * each).abs() < 1e-9);
        // From the tip, the whole chain is ancestors, oldest first.
        let tip = chain(&p.join("ccc.jsonl"), &root);
        let ids: Vec<_> = tip.ancestors.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["aaa", "bbb"]);
        assert!(tip.descendants.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ancestor_in_another_project_directory_is_found() {
        let root = tmp("other");
        write(&root.join("proj-a"), "aaa", None, None);
        write(
            &root.join("proj-b"),
            "bbb",
            Some(("aaa", "subprocess")),
            None,
        );
        let l = chain(&root.join("proj-b/bbb.jsonl"), &root);
        assert!(l.ancestors[0].transcript_found);
        assert_eq!(l.this.via.as_deref(), Some("subprocess"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_ancestor_is_counted() {
        let root = tmp("missing");
        let p = root.join("proj");
        write(&p, "bbb", Some(("gone", "clear")), None);
        let l = chain(&p.join("bbb.jsonl"), &root);
        assert_eq!(l.missing, 1);
        assert_eq!(l.ancestors.len(), 1);
        assert_eq!(l.ancestors[0].id, "gone");
        assert!(!l.ancestors[0].transcript_found);
        assert_eq!(l.ancestors[0].cost_usd, None);
        assert_eq!(l.chain_cost_usd, l.this.cost_usd.unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cycles_terminate() {
        let root = tmp("cycle");
        let p = root.join("proj");
        write(&p, "aaa", Some(("bbb", "clear")), Some("bbb"));
        write(&p, "bbb", Some(("aaa", "clear")), Some("aaa"));
        let l = chain(&p.join("aaa.jsonl"), &root);
        assert_eq!(l.ancestors.len(), 1);
        assert_eq!(l.ancestors[0].id, "bbb");
        // bbb already seen as an ancestor: not repeated as a descendant.
        assert!(l.descendants.is_empty());
        assert_eq!(l.missing, 0);
        // A transcript derived from itself.
        write(&p, "self", Some(("self", "clear")), Some("self"));
        let s = chain(&p.join("self.jsonl"), &root);
        assert!(s.ancestors.is_empty() && s.descendants.is_empty());
        assert_eq!(s.chain_cost_usd, s.this.cost_usd.unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn write_raw(dir: &Path, id: &str, entries: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        let sessions: Vec<String> = entries
            .iter()
            .map(|(i, v)| format!(r#"{{"type":"claude_code_session","id":"{i}","via":"{v}"}}"#))
            .collect();
        let s = format!(
            r#"{{"type":"derived-from","sessionId":"{id}","sessions":[{}]}}"#,
            sessions.join(",")
        ) + "\n"
            + &assistant("2026-11-27T10:00:00.000Z")
            + "\n";
        std::fs::write(dir.join(format!("{id}.jsonl")), s).unwrap();
    }

    #[test]
    fn a_compact_entry_naming_the_session_itself_is_not_a_parent() {
        let root = tmp("selfcompact");
        let p = root.join("proj");
        // Compact-only file (2.1.332): [{self, compact}].
        write_raw(&p, "cmp", &[("cmp", "compact")]);
        let l = chain(&p.join("cmp.jsonl"), &root);
        assert!(l.ancestors.is_empty(), "{:?}", l.ancestors);
        assert_eq!(l.this.via, None);
        assert_eq!(l.missing, 0);
        // Cleared, then compacted: [{parent, clear}, {self, compact}].
        write(&p, "par", None, None);
        write_raw(&p, "cc", &[("par", "clear"), ("cc", "compact")]);
        let l = chain(&p.join("cc.jsonl"), &root);
        assert_eq!(l.ancestors.len(), 1);
        assert_eq!(l.ancestors[0].id, "par");
        assert_eq!(l.this.via.as_deref(), Some("clear"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn depth_is_bounded() {
        let root = tmp("depth");
        let p = root.join("proj");
        write(&p, "s0", None, None);
        for i in 1..15 {
            let parent = format!("s{}", i - 1);
            write(&p, &format!("s{i}"), Some((parent.as_str(), "clear")), None);
        }
        let l = chain(&p.join("s14.jsonl"), &root);
        assert_eq!(l.ancestors.len(), MAX_DEPTH);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lone_session() {
        let root = tmp("lone");
        let p = root.join("proj");
        write(&p, "solo", None, None);
        let l = chain(&p.join("solo.jsonl"), &root);
        assert!(l.ancestors.is_empty() && l.descendants.is_empty());
        assert_eq!(l.missing, 0);
        assert_eq!(l.this.via, None);
        assert_eq!(l.chain_cost_usd, l.this.cost_usd.unwrap());
        let v = serde_json::to_value(&l).unwrap();
        for k in [
            "ancestors",
            "this",
            "descendants",
            "chain_cost_usd",
            "missing",
        ] {
            assert!(v.get(k).is_some(), "{k}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn header_text_forms() {
        assert_eq!(
            header_text("1a2b3c4d-0000-1111", "clear"),
            "from 1a2b3c4d · clear"
        );
        assert_eq!(header_text("1a2b3c4d", ""), "from 1a2b3c4d");
        assert_eq!(
            header_text("65b51d4c-x", "subprocess"),
            "from 65b51d4c · subprocess"
        );
    }
}
