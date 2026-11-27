//! The request chain: each main-thread `api-request` paired with the
//! assistant line that answers it, and one located cause named for every
//! cache miss. Only sizes, hashes, names and counts are kept (FR-1).

use std::collections::HashMap;

use crate::advisor::rules::token::MISS_TOKENS;
use crate::metrics::cost::parse_ts_ms;
use crate::transcript::{ApiRequest, Line, ShapeFacts, SystemKind};

/// Why a request missed the cache, in the order the causes are checked.
#[derive(Debug, Clone, PartialEq)]
pub enum LocatedCause {
    ParamsChanged {
        field: &'static str,
        from: Option<String>,
        to: Option<String>,
    },
    ToolsChanged {
        added: Vec<String>,
        removed: Vec<String>,
        /// Tools whose schema + description size changed, with the change.
        resized: Vec<(String, i64)>,
    },
    /// `block` is the 0-based index (as in a breakpoint's `system/<n>` path).
    SystemChanged {
        block: usize,
        delta_chars: i64,
    },
    PrefixRewritten {
        from: usize,
        of: usize,
    },
    Expired {
        gap_ms: u64,
        ttl_ms: u64,
    },
    Unlocated,
}

/// `+1.2k` / `-340`: a signed count, thousands with one decimal.
fn signed(n: i64) -> String {
    let sign = if n < 0 { '-' } else { '+' };
    let a = n.unsigned_abs();
    if a >= 1000 {
        format!("{sign}{:.1}k", a as f64 / 1000.0)
    } else {
        format!("{sign}{a}")
    }
}

fn duration(ms: u64) -> String {
    if ms >= 3_600_000 && ms % 3_600_000 == 0 {
        format!("{}h", ms / 3_600_000)
    } else if ms >= 60_000 {
        format!("{}m", ms / 60_000)
    } else {
        format!("{}s", ms / 1000)
    }
}

impl LocatedCause {
    pub fn kind(&self) -> &'static str {
        match self {
            LocatedCause::ParamsChanged { .. } => "params_changed",
            LocatedCause::ToolsChanged { .. } => "tools_changed",
            LocatedCause::SystemChanged { .. } => "system_changed",
            LocatedCause::PrefixRewritten { .. } => "prefix_rewritten",
            LocatedCause::Expired { .. } => "expired",
            LocatedCause::Unlocated => "unlocated",
        }
    }

    /// The headline form: what changed in few cells — the first changed
    /// tool name (`+` added before `−` removed) and a count of the rest.
    pub fn compact(&self) -> String {
        match self {
            LocatedCause::ToolsChanged { added, removed, .. } => {
                let names: Vec<String> = added
                    .iter()
                    .map(|n| format!("+{n}"))
                    .chain(removed.iter().map(|n| format!("−{n}")))
                    .collect();
                match names.as_slice() {
                    [] => "tool schema resized".to_string(),
                    [one] => one.clone(),
                    [first, rest @ ..] => format!("{first} +{} more", rest.len()),
                }
            }
            LocatedCause::PrefixRewritten { from, of } => format!("prefix from msg {from}/{of}"),
            LocatedCause::Expired { .. } => "cache expired".to_string(),
            _ => self.short(),
        }
    }

    /// The one-line form every surface draws.
    pub fn short(&self) -> String {
        match self {
            LocatedCause::ParamsChanged { field, from, to } => {
                if *field == "betas" {
                    "betas changed".to_string()
                } else {
                    let f = |v: &Option<String>| v.clone().unwrap_or_else(|| "none".into());
                    format!("{field} {}→{}", f(from), f(to))
                }
            }
            LocatedCause::ToolsChanged { added, removed, .. } => {
                let list = |v: &Vec<String>| {
                    if v.is_empty() {
                        "0".to_string()
                    } else {
                        v.join(",")
                    }
                };
                format!("tools changed: +{} −{}", list(added), list(removed))
            }
            LocatedCause::SystemChanged { block, delta_chars } => {
                format!("system block {block} {} chars", signed(*delta_chars))
            }
            LocatedCause::PrefixRewritten { from, of } => {
                format!("prefix re-sent from msg {from}/{of}")
            }
            LocatedCause::Expired { gap_ms, ttl_ms } => {
                format!(
                    "cache expired ({} gap > {} TTL)",
                    duration(*gap_ms),
                    duration(*ttl_ms)
                )
            }
            LocatedCause::Unlocated => "unlocated".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocatedMiss {
    pub message_id: String,
    pub turn: Option<usize>,
    pub at: Option<String>,
    /// The miss's cache write, in tokens.
    pub tokens: u64,
    pub cause: LocatedCause,
    /// The API's own `diagnostics.cache_miss_reason.type`, kept beside the
    /// located cause.
    pub api_reason: Option<String>,
}

/// A request paired with its response.
#[derive(Debug, Clone)]
pub struct Closed {
    pub request: ApiRequest,
    pub message_id: String,
    pub cache_read: u64,
    pub cache_creation: u64,
    /// The response line's timestamp.
    pub at: Option<String>,
    /// Messages (keep + tail) in the main-thread request sent immediately
    /// before this one, paired or not: `keep` counts what was reused from
    /// the last request sent.
    pub prior_sent: Option<usize>,
}

/// The cache TTL a request asked for: its message-content breakpoint's,
/// else the caller's default.
fn ttl_ms(req: &ApiRequest, default_ttl_ms: u64) -> u64 {
    req.cache_breakpoints
        .iter()
        .filter(|b| b.path.starts_with("messages"))
        .find_map(|b| match b.ttl.as_deref() {
            Some("1h") => Some(3_600_000),
            Some("5m") => Some(300_000),
            _ => None,
        })
        .unwrap_or(default_ttl_ms)
}

fn params_changed(prev: &ApiRequest, cur: &ApiRequest) -> Option<LocatedCause> {
    let (p, c) = (&prev.params, &cur.params);
    let some = |s: &str| Some(s.to_string());
    let change = |field, from: Option<String>, to: Option<String>| {
        Some(LocatedCause::ParamsChanged { field, from, to })
    };
    if p.model != c.model {
        return change("model", some(&p.model), some(&c.model));
    }
    if p.effort != c.effort {
        return change("effort", p.effort.clone(), c.effort.clone());
    }
    if p.betas_hash != c.betas_hash {
        return change("betas", None, None);
    }
    if p.thinking_type != c.thinking_type {
        return change("thinking", p.thinking_type.clone(), c.thinking_type.clone());
    }
    if p.max_tokens != c.max_tokens {
        return change(
            "max_tokens",
            p.max_tokens.map(|n| n.to_string()),
            c.max_tokens.map(|n| n.to_string()),
        );
    }
    if p.edits != c.edits {
        return change("edits", Some(p.edits.join(",")), Some(c.edits.join(",")));
    }
    None
}

fn tools_changed(a: &ShapeFacts, b: &ShapeFacts) -> Option<LocatedCause> {
    let size = |t: &crate::transcript::ToolFacts| (t.schema_chars + t.description_chars) as i64;
    let old: HashMap<&str, i64> = a.tools.iter().map(|t| (t.name.as_str(), size(t))).collect();
    let new: HashMap<&str, i64> = b.tools.iter().map(|t| (t.name.as_str(), size(t))).collect();
    let added: Vec<String> = b
        .tools
        .iter()
        .filter(|t| !old.contains_key(t.name.as_str()))
        .map(|t| t.name.clone())
        .collect();
    let removed: Vec<String> = a
        .tools
        .iter()
        .filter(|t| !new.contains_key(t.name.as_str()))
        .map(|t| t.name.clone())
        .collect();
    let resized: Vec<(String, i64)> = b
        .tools
        .iter()
        .filter_map(|t| {
            let o = *old.get(t.name.as_str())?;
            (o != size(t)).then(|| (t.name.clone(), size(t) - o))
        })
        .collect();
    if added.is_empty() && removed.is_empty() && resized.is_empty() {
        return None;
    }
    Some(LocatedCause::ToolsChanged {
        added,
        removed,
        resized,
    })
}

fn system_changed(a: &ShapeFacts, b: &ShapeFacts) -> Option<LocatedCause> {
    let n = a.system.len().max(b.system.len());
    let at = |s: &ShapeFacts, i: usize| s.system.get(i).copied().unwrap_or(0) as i64;
    (0..n).find_map(|i| {
        let d = at(b, i) - at(a, i);
        (d != 0).then_some(LocatedCause::SystemChanged {
            block: i,
            delta_chars: d,
        })
    })
}

/// The one cause for the miss `cur`, given the request before it.
pub fn locate(
    prev: &Closed,
    cur: &Closed,
    shapes: &HashMap<String, ShapeFacts>,
    default_ttl_ms: u64,
) -> LocatedCause {
    let (p, c) = (&prev.request, &cur.request);
    if let Some(cause) = params_changed(p, c) {
        return cause;
    }
    if p.shape_hash != c.shape_hash {
        if let (Some(a), Some(b)) = (shapes.get(&p.shape_hash), shapes.get(&c.shape_hash)) {
            if let Some(cause) = tools_changed(a, b) {
                return cause;
            }
            if let Some(cause) = system_changed(a, b) {
                return cause;
            }
        }
    }
    let prev_total = cur.prior_sent.unwrap_or(p.keep + p.tail_len);
    if c.keep < prev_total {
        return LocatedCause::PrefixRewritten {
            from: c.keep,
            of: prev_total,
        };
    }
    let started = prev.at.as_deref().or(p.timestamp.as_deref());
    if let (Some(a), Some(b)) = (
        started.and_then(parse_ts_ms),
        c.timestamp.as_deref().and_then(parse_ts_ms),
    ) {
        let ttl = ttl_ms(c, default_ttl_ms);
        let gap = (b - a).max(0) as u64;
        if gap > ttl {
            return LocatedCause::Expired {
                gap_ms: gap,
                ttl_ms: ttl,
            };
        }
    }
    LocatedCause::Unlocated
}

fn is_main_thread(source: &str) -> bool {
    source.starts_with("repl_main_thread") || source == "sdk"
}

#[derive(Debug, Clone, Default)]
pub struct RequestChain {
    shapes: HashMap<String, ShapeFacts>,
    latest_shape: Option<String>,
    pending: Option<ApiRequest>,
    /// keep + tail of the last main-thread request sent, paired or not.
    last_sent: Option<usize>,
    /// `last_sent` as it was when `pending` was sent.
    pending_prior: Option<usize>,
    prev: Option<Closed>,
    misses: Vec<LocatedMiss>,
    by_id: HashMap<String, usize>,
}

impl RequestChain {
    pub fn apply(&mut self, line: &Line, turn: Option<usize>, default_ttl_ms: u64) {
        match line {
            Line::ApiRequestShape(s) => {
                self.latest_shape = Some(s.shape_hash.clone());
                self.shapes.insert(s.shape_hash.clone(), s.clone());
            }
            Line::ApiRequest(r) if is_main_thread(&r.query_source) => {
                self.pending_prior = self.last_sent;
                self.last_sent = Some(r.keep + r.tail_len);
                self.pending = Some(r.clone());
            }
            Line::Assistant(a) if !a.is_sidechain && a.request_id.is_some() => {
                if a.is_api_error_message {
                    return;
                }
                let Some(request) = self.pending.take() else {
                    return;
                };
                let usage = &a.message.usage;
                let cur = Closed {
                    request,
                    message_id: a.message.id.clone(),
                    cache_read: usage.cache_read_input_tokens,
                    cache_creation: usage.cache_creation_input_tokens,
                    at: a.timestamp.clone(),
                    prior_sent: self.pending_prior,
                };
                let reason = a.cache_miss_reason().map(|r| r.kind.clone());
                let shrank = self.prev.as_ref().is_some_and(|p| {
                    (p.cache_read + p.cache_creation).saturating_sub(cur.cache_read) >= MISS_TOKENS
                });
                if reason.is_some() || shrank {
                    let cause = match &self.prev {
                        Some(p) => locate(p, &cur, &self.shapes, default_ttl_ms),
                        None => LocatedCause::Unlocated,
                    };
                    self.by_id.insert(cur.message_id.clone(), self.misses.len());
                    self.misses.push(LocatedMiss {
                        message_id: cur.message_id.clone(),
                        turn,
                        at: a.timestamp.clone(),
                        tokens: cur.cache_creation,
                        cause,
                        api_reason: reason,
                    });
                }
                self.prev = Some(cur);
            }
            Line::System(s) if s.kind() == SystemKind::CompactBoundary => self.reset(),
            Line::ContinuedIn(_) => self.reset(),
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.pending = None;
        self.last_sent = None;
        self.pending_prior = None;
        self.prev = None;
    }

    pub fn misses(&self) -> &[LocatedMiss] {
        &self.misses
    }

    pub fn latest_shape(&self) -> Option<&ShapeFacts> {
        self.latest_shape.as_ref().and_then(|h| self.shapes.get(h))
    }

    pub fn miss_for(&self, message_id: &str) -> Option<&LocatedMiss> {
        self.by_id.get(message_id).map(|&i| &self.misses[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(v: serde_json::Value) -> Line {
        Line::parse(&v.to_string()).unwrap()
    }

    fn req(shape: &str, keep: usize, tail: usize, effort: &str, ts: &str) -> Line {
        req_src("repl_main_thread", shape, keep, tail, effort, ts)
    }

    fn req_src(src: &str, shape: &str, keep: usize, tail: usize, effort: &str, ts: &str) -> Line {
        parse(json!({
            "type": "api-request", "querySource": src, "shapeHash": shape,
            "params": {"model": "m", "betas": ["b"], "max_tokens": 64000,
                "output_config": {"effort": effort}},
            "keep": keep, "tail": vec!["h"; tail],
            "cacheControl": [], "timestamp": ts
        }))
    }

    fn shape(hash: &str, system: &[usize], tools: &[(&str, usize)]) -> Line {
        parse(json!({
            "type": "api-request-shape", "shapeHash": hash,
            "shape": {
                "system": system.iter().map(|n| json!({"type":"text","text":"x".repeat(*n)})).collect::<Vec<_>>(),
                "tools": tools.iter().map(|(n, d)| json!({"name": n, "description": "d".repeat(*d), "input_schema": {}})).collect::<Vec<_>>()
            },
            "timestamp": "2026-11-27T10:00:00Z"
        }))
    }

    fn reply(id: &str, read: u64, create: u64, ts: &str, reason: Option<&str>) -> Line {
        let mut m = json!({"id": id, "model": "m", "content": [],
            "usage": {"input_tokens": 1, "cache_read_input_tokens": read,
                "cache_creation_input_tokens": create}});
        if let Some(r) = reason {
            m["diagnostics"] = json!({"cache_miss_reason": {"type": r}});
        }
        parse(
            json!({"type": "assistant", "timestamp": ts, "requestId": format!("r-{id}"), "message": m}),
        )
    }

    const T0: &str = "2026-11-27T10:00:00Z";
    const T1: &str = "2026-11-27T10:00:30Z";
    const T7: &str = "2026-11-27T10:07:00Z";

    fn run(lines: &[Line]) -> RequestChain {
        let mut c = RequestChain::default();
        for l in lines {
            c.apply(l, Some(1), 300_000);
        }
        c
    }

    /// First request warm, second a miss (prefix shrank by 30k).
    fn miss_pair(second: Line, extra: Vec<Line>) -> RequestChain {
        let mut lines = extra;
        lines.push(req("s1", 10, 2, "high", T0));
        lines.push(reply("a", 0, 40_000, T0, None));
        lines.push(second);
        lines.push(reply("b", 5_000, 30_000, T1, None));
        run(&lines)
    }

    #[test]
    fn compact_forms() {
        let t = |a: &[&str], r: &[&str], z: &[(&str, i64)]| LocatedCause::ToolsChanged {
            added: a.iter().map(|x| x.to_string()).collect(),
            removed: r.iter().map(|x| x.to_string()).collect(),
            resized: z.iter().map(|(n, d)| (n.to_string(), *d)).collect(),
        };
        assert_eq!(
            t(&["mcp__x__search"], &[], &[]).compact(),
            "+mcp__x__search"
        );
        assert_eq!(
            t(&["mcp__a__b", "c", "d"], &[], &[]).compact(),
            "+mcp__a__b +2 more"
        );
        assert_eq!(t(&[], &["WebSearch"], &[]).compact(), "−WebSearch");
        assert_eq!(t(&["a"], &["b"], &[]).compact(), "+a +1 more");
        assert_eq!(
            t(&[], &[], &[("Bash", 40)]).compact(),
            "tool schema resized"
        );
        let sys = LocatedCause::SystemChanged {
            block: 2,
            delta_chars: 1200,
        };
        assert_eq!(sys.compact(), sys.short());
        assert_eq!(sys.compact(), "system block 2 +1.2k chars");
        assert_eq!(
            LocatedCause::PrefixRewritten { from: 4, of: 9 }.compact(),
            "prefix from msg 4/9"
        );
        assert_eq!(
            LocatedCause::Expired {
                gap_ms: 400_000,
                ttl_ms: 300_000
            }
            .compact(),
            "cache expired"
        );
        let p = LocatedCause::ParamsChanged {
            field: "effort",
            from: Some("high".into()),
            to: Some("max".into()),
        };
        assert_eq!(p.compact(), "effort high→max");
    }

    #[test]
    fn params_changed() {
        let c = miss_pair(req("s1", 12, 1, "max", T1), vec![]);
        let m = &c.misses()[0];
        assert_eq!(m.cause.kind(), "params_changed");
        assert_eq!(m.cause.short(), "effort high→max");
        assert_eq!(m.tokens, 30_000);
    }

    #[test]
    fn tools_changed() {
        let c = miss_pair(
            req("s2", 12, 1, "high", T1),
            vec![
                shape("s1", &[10], &[("Bash", 5)]),
                shape("s2", &[10], &[("Bash", 5), ("mcp__x__search", 9)]),
            ],
        );
        let m = &c.misses()[0];
        assert_eq!(m.cause.short(), "tools changed: +mcp__x__search −0");
        assert_eq!(m.cause.kind(), "tools_changed");
    }

    #[test]
    fn system_changed() {
        let c = miss_pair(
            req("s2", 12, 1, "high", T1),
            vec![
                shape("s1", &[10, 20, 30], &[("Bash", 5)]),
                shape("s2", &[10, 20, 1230], &[("Bash", 5)]),
            ],
        );
        assert_eq!(c.misses()[0].cause.short(), "system block 2 +1.2k chars");
    }

    #[test]
    fn prefix_rewritten() {
        // prev: keep 50 + tail 2 = 52; cur keeps 48.
        let lines = [
            req("s1", 50, 2, "high", T0),
            reply("a", 0, 40_000, T0, None),
            req("s1", 48, 1, "high", T1),
            reply("b", 5_000, 30_000, T1, None),
        ];
        let c = run(&lines);
        assert_eq!(
            c.misses()[0].cause,
            LocatedCause::PrefixRewritten { from: 48, of: 52 }
        );
        assert_eq!(c.misses()[0].cause.short(), "prefix re-sent from msg 48/52");
    }

    #[test]
    fn keep_compares_with_the_unpaired_request_before() {
        // A paired (keep 40 + 2), B unpaired (47 + 3 = 50), C keeps 48.
        let lines = [
            req("s1", 40, 2, "high", T0),
            reply("a", 0, 40_000, T0, None),
            req("s1", 47, 3, "high", T1),
            req("s1", 48, 2, "high", T1),
            reply("c", 5_000, 30_000, T1, None),
        ];
        let c = run(&lines);
        assert_eq!(
            c.misses()[0].cause,
            LocatedCause::PrefixRewritten { from: 48, of: 50 }
        );
    }

    #[test]
    fn expired() {
        let lines = [
            req("s1", 10, 2, "high", T0),
            reply("a", 0, 40_000, T0, None),
            req("s1", 12, 1, "high", T7),
            reply("b", 5_000, 30_000, T7, None),
        ];
        let c = run(&lines);
        assert_eq!(
            c.misses()[0].cause.short(),
            "cache expired (7m gap > 5m TTL)"
        );
    }

    #[test]
    fn unlocated() {
        let c = miss_pair(req("s1", 12, 1, "high", T1), vec![]);
        assert_eq!(c.misses()[0].cause, LocatedCause::Unlocated);
        assert_eq!(c.misses()[0].cause.short(), "unlocated");
    }

    #[test]
    fn order_params_beat_shape() {
        let c = miss_pair(
            req("s2", 12, 1, "max", T1),
            vec![
                shape("s1", &[10], &[("Bash", 5)]),
                shape("s2", &[10], &[("Bash", 5), ("T", 1)]),
            ],
        );
        assert_eq!(c.misses()[0].cause.kind(), "params_changed");
    }

    #[test]
    fn pairs_by_order() {
        let lines = [
            req("s1", 10, 2, "high", T0),
            reply("a", 0, 40_000, T0, None),
            req("s1", 12, 1, "high", T1), // dropped unpaired
            req("s1", 13, 1, "max", T1),
            reply("b", 5_000, 30_000, T1, None),
        ];
        let c = run(&lines);
        assert_eq!(c.misses()[0].message_id, "b");
        assert_eq!(c.misses()[0].cause.short(), "effort high→max");
    }

    #[test]
    fn compact_resets_chain() {
        let boundary = parse(json!({"type": "system", "subtype": "compact_boundary",
            "timestamp": T1}));
        let lines = [
            req("s1", 10, 2, "high", T0),
            reply("a", 0, 40_000, T0, None),
            boundary,
            req("s1", 2, 1, "high", T1),
            reply("b", 0, 30_000, T1, Some("messages_changed")),
        ];
        let c = run(&lines);
        // No previous request: the miss is kept, unlocated.
        assert_eq!(c.misses().len(), 1);
        assert_eq!(c.misses()[0].cause, LocatedCause::Unlocated);
    }

    #[test]
    fn missing_shape_falls_through() {
        let lines = [
            req("s1", 50, 2, "high", T0),
            reply("a", 0, 40_000, T0, None),
            req("s9", 48, 1, "high", T1),
            reply("b", 5_000, 30_000, T1, None),
        ];
        let c = run(&lines);
        assert_eq!(c.misses()[0].cause.kind(), "prefix_rewritten");
    }

    #[test]
    fn api_reason_kept_beside() {
        let lines = [
            req("s1", 10, 2, "high", T0),
            reply("a", 0, 40_000, T0, None),
            req("s1", 12, 1, "high", T1),
            reply("b", 40_000, 100, T1, Some("tools_changed")),
        ];
        let c = run(&lines);
        let m = c.miss_for("b").unwrap();
        assert_eq!(m.api_reason.as_deref(), Some("tools_changed"));
        assert_eq!(m.cause, LocatedCause::Unlocated);
    }

    #[test]
    fn not_a_miss_when_prefix_held() {
        let lines = [
            req("s1", 10, 2, "high", T0),
            reply("a", 0, 40_000, T0, None),
            req("s1", 12, 1, "high", T1),
            reply("b", 40_000, 30_000, T1, None),
        ];
        assert!(run(&lines).misses().is_empty());
    }

    #[test]
    fn sidechain_requests_are_not_chained() {
        let lines = [
            req_src("agent:general", "s1", 10, 2, "high", T0),
            reply("a", 0, 40_000, T0, Some("tools_changed")),
        ];
        assert!(run(&lines).misses().is_empty());
    }

    #[test]
    fn linear_time() {
        let mut lines = Vec::new();
        for i in 0..10_000 {
            lines.push(req("s1", i, 1, "high", T0));
            lines.push(reply(&format!("m{i}"), 1000, 10, T0, None));
        }
        let t = std::time::Instant::now();
        let c = run(&lines);
        assert!(c.misses().is_empty());
        if !cfg!(debug_assertions) {
            assert!(t.elapsed().as_millis() < 200, "{:?}", t.elapsed());
        }
    }
}

#[cfg(test)]
mod fixture_tests {
    #[test]
    fn fixture_r_located() {
        let path = std::path::Path::new("fixtures/session-r.jsonl");
        let info = crate::ui::state::SessionInfo::from_fixture(path);
        let state = crate::load::state_from(path, info);
        // Pinned from the run. PrefixRewritten is covered by the synthetic
        // unit test only: no real session has it beside a shape miss.
        let misses = state.requests.misses();
        let ids: Vec<(&str, &str)> = misses
            .iter()
            .map(|m| (m.message_id.as_str(), m.cause.kind()))
            .collect();
        assert_eq!(
            ids,
            [
                ("msg_011CgMcApk8Wt1XX5BxnZmun", "tools_changed"),
                ("msg_011CgYPRTTRGbJ4Rrtfi1Prt", "params_changed"),
                ("msg_011CgYRZof3gtWbRy45bcbgt", "expired"),
            ]
        );
        assert!(state.requests.latest_shape().is_some());
    }
}
