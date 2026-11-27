//! Session lineage: the chain a transcript belongs to. Ancestors are found
//! through `derived-from` (the first entry of the last such line of a
//! transcript that names another session), descendants through `continued-in`. Each relative is looked
//! up by id as `<id>.jsonl`, in this transcript's project directory first
//! and then in every project directory under `projects_root`. The walk is
//! bounded (`MAX_DEPTH`) and guarded against cycles; `--lines` does not
//! apply to relatives. Only ids, `via` tokens, timestamps and costs are
//! kept.
//!
//! The session itself is never read here: its part comes from the loaded
//! [`State`], live. Each relative is read once per version of its file:
//! what it says is kept in `~/.cctop/lineage-cache.json`, keyed by path,
//! size and mtime, so a finished ancestor is never loaded twice.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ui::state::State;

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Scan {
    id: String,
    /// `(id, via)` of the first entry of the last `derived-from` line.
    derived: Option<(String, String)>,
    continued_in: Option<String>,
    started_at: Option<String>,
    cost_usd: Option<f64>,
}

impl Scan {
    fn of_state(state: &State, stem: &str) -> Scan {
        Scan {
            id: state
                .agg
                .session_id
                .clone()
                .unwrap_or_else(|| stem.to_string()),
            derived: state.agg.derived_from.clone(),
            continued_in: state.agg.continued_in.clone(),
            started_at: state.agg.first_at.clone(),
            cost_usd: state.cost_combined().map(|c| c.usd),
        }
    }
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A relative's transcript, read in full (with its subagents).
fn scan(path: &Path) -> Scan {
    let info = crate::ui::state::SessionInfo::from_transcript(path);
    let state = crate::load::state_from(path, info);
    Scan::of_state(&state, &stem_of(path))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    size: u64,
    mtime_ns: u64,
    scan: Scan,
}

/// What each relative said, by transcript path, at the size and mtime it
/// was read at. A changed file is read again.
#[derive(Debug, Default)]
pub struct Cache {
    file: Option<PathBuf>,
    entries: BTreeMap<String, Entry>,
    dirty: bool,
}

/// `~/.cctop/lineage-cache.json`.
pub fn default_cache_file() -> PathBuf {
    crate::status::cctop_dir().join("lineage-cache.json")
}

fn stamp(path: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(path).ok()?;
    let ns = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((m.len(), u64::try_from(ns).unwrap_or(u64::MAX)))
}

impl Cache {
    /// The cache kept in `file` (empty when it is missing or unreadable).
    pub fn open(file: PathBuf) -> Cache {
        let entries = std::fs::read(&file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Cache {
            file: Some(file),
            entries,
            dirty: false,
        }
    }

    /// A cache that lives only as long as the value.
    pub fn in_memory() -> Cache {
        Cache::default()
    }

    fn scan(&mut self, path: &Path) -> Scan {
        let key = path.to_string_lossy().into_owned();
        let Some((size, mtime_ns)) = stamp(path) else {
            return scan(path);
        };
        if let Some(e) = self
            .entries
            .get(&key)
            .filter(|e| e.size == size && e.mtime_ns == mtime_ns)
        {
            return e.scan.clone();
        }
        let s = scan(path);
        self.entries.insert(
            key,
            Entry {
                size,
                mtime_ns,
                scan: s.clone(),
            },
        );
        self.dirty = true;
        s
    }

    /// Write the cache back when a read added to it, dropping entries whose
    /// transcript is gone. Best effort: a failed write costs a re-read.
    pub fn save(&mut self) {
        let Some(file) = self.file.as_ref().filter(|_| self.dirty) else {
            return;
        };
        self.entries.retain(|k, _| Path::new(k).is_file());
        let Ok(bytes) = serde_json::to_vec(&self.entries) else {
            return;
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = file.with_extension(format!("json.{}.tmp", std::process::id()));
        if std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, file).is_ok() {
            self.dirty = false;
        } else {
            let _ = std::fs::remove_file(&tmp);
        }
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

/// Walk from the link `first` one link at a time; `next` reads the link
/// out of a scan. Stops at the first missing transcript, a repeated id, or
/// `MAX_DEPTH`.
fn walk(
    first: Option<String>,
    dir: Option<&Path>,
    projects_root: &Path,
    seen: &mut HashSet<String>,
    cache: &mut Cache,
    next: impl Fn(&Scan) -> Option<String>,
) -> (Vec<Member>, usize) {
    let mut out = Vec::new();
    let mut missing = 0;
    let mut link = first;
    for _ in 0..MAX_DEPTH {
        let Some(id) = link.take() else { break };
        if !seen.insert(id.clone()) {
            break;
        }
        match find(&id, dir, projects_root) {
            Some(p) => {
                let s = cache.scan(&p);
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

/// Where a session's ancestors are looked up from: what the session says
/// of itself. Built from a loaded [`State`] ([`Ask::of`]), so it can be
/// handed to another thread.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    /// The session's own id.
    pub own: String,
    /// The parent `derived-from` names.
    pub parent: Option<String>,
    /// The session the last `continued-in` names.
    pub child: Option<String>,
    /// The session's transcript directory (relatives are looked for here first).
    pub dir: Option<PathBuf>,
}

impl Ask {
    pub fn of(state: &State) -> Ask {
        let stem = state.transcript.as_deref().map(stem_of).unwrap_or_default();
        Ask {
            own: state.agg.session_id.clone().unwrap_or(stem),
            parent: state.agg.derived_from.as_ref().map(|(id, _)| id.clone()),
            child: state.agg.continued_in.clone(),
            dir: state
                .transcript
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf),
        }
    }
}

/// The ancestors, oldest first, and how many links named a missing transcript.
pub fn ancestors(ask: &Ask, projects_root: &Path, cache: &mut Cache) -> (Vec<Member>, usize) {
    let mut seen = HashSet::from([ask.own.clone()]);
    let (mut out, missing) = walk(
        ask.parent.clone(),
        ask.dir.as_deref(),
        projects_root,
        &mut seen,
        cache,
        |s| s.derived.as_ref().map(|(id, _)| id.clone()),
    );
    out.reverse();
    (out, missing)
}

/// The ancestors' (finished) cost: the part of the chain's cost that is
/// not this session's. Cached in `cache`, which is saved.
pub fn ancestors_usd(ask: &Ask, projects_root: &Path, cache: &mut Cache) -> f64 {
    let (members, _) = ancestors(ask, projects_root, cache);
    cache.save();
    members.iter().filter_map(|m| m.cost_usd).sum()
}

/// The chain's cost: the ancestors' finished cost plus this session's live
/// one. The one sum every surface shows (TUI, `query overview`, the pane).
pub fn chain_usd(ancestors_usd: f64, state: &State) -> f64 {
    ancestors_usd + state.cost_combined().map(|c| c.usd).unwrap_or(0.0)
}

/// The whole chain of the session `state` holds: relatives from their
/// transcripts (through `cache`), this session from `state`, live.
pub fn chain(state: &State, projects_root: &Path, cache: &mut Cache) -> Lineage {
    let ask = Ask::of(state);
    let (ancestors, m1) = ancestors(&ask, projects_root, cache);
    let mut seen: HashSet<String> = ancestors.iter().map(|m| m.id.clone()).collect();
    seen.insert(ask.own.clone());
    let (descendants, m2) = walk(
        ask.child.clone(),
        ask.dir.as_deref(),
        projects_root,
        &mut seen,
        cache,
        |s| s.continued_in.clone(),
    );
    cache.save();
    let stem = state.transcript.as_deref().map(stem_of).unwrap_or_default();
    let this = member(&Scan::of_state(state, &stem));
    let ancestors_usd: f64 = ancestors.iter().filter_map(|m| m.cost_usd).sum();
    let chain_cost_usd = chain_usd(ancestors_usd, state)
        + descendants.iter().filter_map(|m| m.cost_usd).sum::<f64>();
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
    use crate::ui::state::SessionInfo;

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

    /// The chain of the transcript at `path`, loaded as `cctop query` loads it.
    fn chain_at(path: &Path, root: &Path) -> Lineage {
        let state =
            crate::load::state_from(path, crate::ui::state::SessionInfo::from_transcript(path));
        chain(&state, root, &mut Cache::in_memory())
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
        let l = chain_at(&p.join("bbb.jsonl"), &root);
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
        let tip = chain_at(&p.join("ccc.jsonl"), &root);
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
        let l = chain_at(&root.join("proj-b/bbb.jsonl"), &root);
        assert!(l.ancestors[0].transcript_found);
        assert_eq!(l.this.via.as_deref(), Some("subprocess"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_ancestor_is_counted() {
        let root = tmp("missing");
        let p = root.join("proj");
        write(&p, "bbb", Some(("gone", "clear")), None);
        let l = chain_at(&p.join("bbb.jsonl"), &root);
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
        let l = chain_at(&p.join("aaa.jsonl"), &root);
        assert_eq!(l.ancestors.len(), 1);
        assert_eq!(l.ancestors[0].id, "bbb");
        // bbb already seen as an ancestor: not repeated as a descendant.
        assert!(l.descendants.is_empty());
        assert_eq!(l.missing, 0);
        // A transcript derived from itself.
        write(&p, "self", Some(("self", "clear")), Some("self"));
        let s = chain_at(&p.join("self.jsonl"), &root);
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
        let l = chain_at(&p.join("cmp.jsonl"), &root);
        assert!(l.ancestors.is_empty(), "{:?}", l.ancestors);
        assert_eq!(l.this.via, None);
        assert_eq!(l.missing, 0);
        // Cleared, then compacted: [{parent, clear}, {self, compact}].
        write(&p, "par", None, None);
        write_raw(&p, "cc", &[("par", "clear"), ("cc", "compact")]);
        let l = chain_at(&p.join("cc.jsonl"), &root);
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
        let l = chain_at(&p.join("s14.jsonl"), &root);
        assert_eq!(l.ancestors.len(), MAX_DEPTH);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lone_session() {
        let root = tmp("lone");
        let p = root.join("proj");
        write(&p, "solo", None, None);
        let l = chain_at(&p.join("solo.jsonl"), &root);
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

    fn loads() -> Vec<PathBuf> {
        crate::load::tests::LOADS.with(|l| std::mem::take(&mut *l.borrow_mut()))
    }

    #[test]
    fn ancestors_are_read_once_per_file_version_through_the_cache() {
        let root = tmp("cache");
        let p = root.join("proj");
        write(&p, "aaa", None, None);
        write(&p, "bbb", Some(("aaa", "clear")), None);
        write(&p, "ccc", Some(("bbb", "clear")), None);
        let file = root.join("home/.cctop/lineage-cache.json");
        let path = p.join("ccc.jsonl");
        let state = crate::load::state_from(&path, SessionInfo::from_transcript(&path));
        let ask = Ask::of(&state);
        assert_eq!(ask.parent.as_deref(), Some("bbb"));
        let _ = loads();
        // Miss: each ancestor loaded once, the session itself never.
        let usd = ancestors_usd(&ask, &root, &mut Cache::open(file.clone()));
        assert_eq!(loads(), [p.join("bbb.jsonl"), p.join("aaa.jsonl")]);
        assert!(file.is_file(), "the cache is written under the cctop dir");
        let each = state.cost_combined().unwrap().usd;
        assert!((usd - 2.0 * each).abs() < 1e-9);
        // Hit: a new process (a fresh `Cache::open`) loads nothing.
        let again = ancestors_usd(&ask, &root, &mut Cache::open(file.clone()));
        assert!(loads().is_empty());
        assert_eq!(again, usd);
        // A changed file is a miss for that file only.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(p.join("aaa.jsonl"))
            .unwrap();
        std::io::Write::write_all(
            &mut f,
            (assistant("2026-11-27T10:05:00.000Z").replace("m1", "m2") + "\n").as_bytes(),
        )
        .unwrap();
        drop(f);
        let grown = ancestors_usd(&ask, &root, &mut Cache::open(file.clone()));
        assert_eq!(loads(), [p.join("aaa.jsonl")]);
        assert!((grown - 3.0 * each).abs() < 1e-9);
        // The lineage verb's chain reads the session from the state too.
        let l = chain(&state, &root, &mut Cache::open(file));
        assert!(loads().is_empty());
        assert_eq!(l.ancestors.len(), 2);
        assert!((l.chain_cost_usd - 4.0 * each).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `cctop query overview` on a derived session: one load of the
    /// session, none of it again for the chain; the chain cost moves with
    /// the session's own spend (never frozen at the first read).
    #[test]
    fn overview_verb_loads_the_session_once_and_its_chain_stays_live() {
        let root = tmp("overview");
        let p = root.join("proj");
        write(&p, "aaa", None, None);
        write(&p, "bbb", Some(("aaa", "clear")), None);
        let path = p.join("bbb.jsonl");
        let _ = loads();
        let mut state = crate::load::state_from(&path, SessionInfo::from_transcript(&path));
        let mut cache = Cache::open(root.join("home/.cctop/lineage-cache.json"));
        state.refresh_lineage_with(&root, &mut cache);
        let v = crate::query::overview(&state, 50);
        let seen = loads();
        assert_eq!(seen.iter().filter(|l| **l == path).count(), 1, "{seen:?}");
        assert_eq!(seen.len(), 2, "the session once, its parent once: {seen:?}");
        let own = state.cost_combined().unwrap().usd;
        let chain = state.chain_usd().unwrap();
        assert!((chain - 2.0 * own).abs() < 1e-9);
        let spend = v["charts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "spend")
            .unwrap();
        assert!(spend["readouts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["text"]
                .as_str()
                .unwrap()
                .contains("with the sessions it came from")));
        // The session spends more: the chain follows, the ancestors are not re-read.
        let line = crate::transcript::Line::parse(
            &assistant("2026-11-27T10:10:00.000Z").replace("m1", "m2"),
        );
        state.apply(&line.unwrap());
        let more = state.cost_combined().unwrap().usd;
        assert!(more > own);
        assert!((state.chain_usd().unwrap() - (own + more)).abs() < 1e-9);
        state.refresh_lineage_with(&root, &mut cache);
        assert!(loads().is_empty());
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
