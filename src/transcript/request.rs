//! The three request lines Claude Code writes since 2.1.302: `api-request`
//! (one request's settings and where it sits in the cache), `api-request-shape`
//! (the system blocks and tool schemas a `shapeHash` stands for) and
//! `api-request-blob` (one message's content, keyed by hash).
//!
//! Shapes and blobs are prose. They are read from the borrowed `Value` and
//! only lengths and hashes survive: no struct here can hold a system block's
//! text, a tool description or schema, or a message's content.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde_json::Value;

/// One request: its settings, its shape, and its cache breakpoints.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiRequest {
    pub query_source: String,
    pub shape_hash: String,
    /// Messages reused from the previous request's prefix.
    pub keep: usize,
    /// How many new messages followed the kept prefix (the hashes are dropped).
    pub tail_len: usize,
    pub params: RequestParams,
    pub cache_breakpoints: Vec<Breakpoint>,
    pub timestamp: Option<String>,
}

/// The settings whose change rewrites the cached prefix. Betas are hashed
/// over the sorted list: only a change matters, not an ordering.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestParams {
    pub model: String,
    pub effort: Option<String>,
    pub betas_hash: u64,
    pub max_tokens: Option<u64>,
    pub thinking_type: Option<String>,
    pub edits: Vec<String>,
}

/// A `cache_control` marker, by where it sits (`system/1`, `messages/12/0`).
#[derive(Debug, Clone, PartialEq)]
pub struct Breakpoint {
    pub path: String,
    pub ttl: Option<String>,
    pub scope: Option<String>,
}

/// What a `shapeHash` stands for, as sizes.
#[derive(Debug, Clone, PartialEq)]
pub struct ShapeFacts {
    pub shape_hash: String,
    /// Characters per system block.
    pub system: Vec<usize>,
    pub tools: Vec<ToolFacts>,
    pub timestamp: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolFacts {
    pub name: String,
    /// Length of the `input_schema` serialised compactly.
    pub schema_chars: usize,
    pub description_chars: usize,
    pub deferred: bool,
}

/// One message's size, keyed by the hash `tail` names.
#[derive(Debug, Clone, PartialEq)]
pub struct BlobFacts {
    pub hash: String,
    pub role: String,
    /// Sum of the text lengths in the content.
    pub chars: usize,
    pub timestamp: Option<String>,
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn chars(v: &Value) -> usize {
    v.as_str().map_or(0, |s| s.chars().count())
}

impl ApiRequest {
    pub(crate) fn from_raw(raw: &Value) -> Option<Self> {
        let params = RequestParams::from_raw(raw.get("params")?)?;
        let cache_breakpoints = match raw.get("cacheControl") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(a)) => a.iter().filter_map(Breakpoint::from_raw).collect(),
            Some(_) => return None,
        };
        Some(ApiRequest {
            query_source: str_of(raw, "querySource")?,
            shape_hash: str_of(raw, "shapeHash")?,
            keep: raw.get("keep")?.as_u64()? as usize,
            tail_len: match raw.get("tail") {
                None | Some(Value::Null) => 0,
                Some(v) => v.as_array()?.len(),
            },
            params,
            cache_breakpoints,
            timestamp: str_of(raw, "timestamp"),
        })
    }
}

impl RequestParams {
    fn from_raw(p: &Value) -> Option<Self> {
        let mut betas: Vec<&str> = match p.get("betas") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => v.as_array()?.iter().filter_map(Value::as_str).collect(),
        };
        betas.sort_unstable();
        let mut h = DefaultHasher::new();
        betas.hash(&mut h);
        let edits = p
            .pointer("/context_management/edits")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|e| str_of(e, "type")).collect())
            .unwrap_or_default();
        Some(RequestParams {
            model: str_of(p, "model")?,
            effort: p
                .pointer("/output_config/effort")
                .and_then(Value::as_str)
                .map(str::to_owned),
            betas_hash: h.finish(),
            max_tokens: p.get("max_tokens").and_then(Value::as_u64),
            thinking_type: p
                .pointer("/thinking/type")
                .and_then(Value::as_str)
                .map(str::to_owned),
            edits,
        })
    }
}

impl Breakpoint {
    fn from_raw(v: &Value) -> Option<Self> {
        let path = v
            .get("path")?
            .as_array()?
            .iter()
            .map(|s| match s {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join("/");
        let value = v.get("value");
        Some(Breakpoint {
            path,
            ttl: value.and_then(|x| str_of(x, "ttl")),
            scope: value.and_then(|x| str_of(x, "scope")),
        })
    }
}

impl ShapeFacts {
    pub(crate) fn from_raw(raw: &Value) -> Option<Self> {
        let shape = raw.get("shape")?;
        let system = shape
            .get("system")?
            .as_array()?
            .iter()
            .map(|b| b.get("text").map_or(0, chars))
            .collect();
        let tools = match shape.get("tools") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => v
                .as_array()?
                .iter()
                .map(|t| {
                    Some(ToolFacts {
                        name: str_of(t, "name")?,
                        schema_chars: t
                            .get("input_schema")
                            .map_or(0, |s| serde_json::to_string(s).map_or(0, |s| s.len())),
                        description_chars: t.get("description").map_or(0, chars),
                        deferred: t
                            .get("defer_loading")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        };
        Some(ShapeFacts {
            shape_hash: str_of(raw, "shapeHash")?,
            system,
            tools,
            timestamp: str_of(raw, "timestamp"),
        })
    }
}

/// Characters of text in a content value: a bare string, `text` fields, and
/// nested `content` (tool results), recursively.
fn content_chars(v: &Value) -> usize {
    match v {
        Value::String(s) => s.chars().count(),
        Value::Array(a) => a.iter().map(content_chars).sum(),
        Value::Object(o) => {
            o.get("text").map_or(0, chars) + o.get("content").map_or(0, content_chars)
        }
        _ => 0,
    }
}

impl BlobFacts {
    pub(crate) fn from_raw(raw: &Value) -> Option<Self> {
        let message = raw.get("message")?;
        Some(BlobFacts {
            hash: str_of(raw, "hash")?,
            role: str_of(message, "role")?,
            chars: content_chars(message.get("content")?),
            timestamp: str_of(raw, "timestamp"),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::transcript::Line;
    use serde_json::json;

    fn parse(v: serde_json::Value) -> Line {
        Line::parse(&v.to_string()).unwrap()
    }

    fn request(betas: serde_json::Value) -> serde_json::Value {
        json!({
            "type": "api-request", "querySource": "repl_main_thread", "shapeHash": "s1",
            "params": {"model": "m", "betas": betas, "max_tokens": 64000,
                "thinking": {"type": "adaptive"},
                "context_management": {"edits": [{"type": "clear_thinking_20251015"}]},
                "output_config": {"effort": "high"}},
            "keep": 48, "tail": ["a", "b", "c"],
            "cacheControl": [{"path": ["system", 1], "value": {"type": "ephemeral", "ttl": "1h", "scope": "global"}}],
            "timestamp": "2026-11-27T10:00:00Z"
        })
    }

    #[test]
    fn shape_keeps_no_prose() {
        let line = parse(json!({
            "type": "api-request-shape", "shapeHash": "h",
            "shape": {"system": [{"type": "text", "text": "SYSTEM-SECRET-xyz"}],
                "tools": [{"name": "Bash", "description": "DESC-SECRET-abc",
                    "input_schema": {"type": "object"}, "defer_loading": true}]}
        }));
        let Line::ApiRequestShape(s) = &line else {
            panic!("not a shape: {line:?}")
        };
        let dbg = format!("{line:?}");
        assert!(!dbg.contains("SYSTEM-SECRET") && !dbg.contains("DESC-SECRET"));
        assert_eq!(s.system, vec![17]);
        assert_eq!(s.tools[0].description_chars, 15);
        assert_eq!(s.tools[0].schema_chars, r#"{"type":"object"}"#.len());
        assert!(s.tools[0].deferred);
        assert_eq!(line.kind(), "api-request-shape");
    }

    #[test]
    fn blob_keeps_no_prose() {
        let line = parse(json!({"type": "api-request-blob", "hash": "b",
            "message": {"role": "user", "content": [{"type": "text", "text": "BLOB-SECRET"}]}}));
        let Line::ApiRequestBlob(b) = &line else {
            panic!("not a blob")
        };
        assert!(!format!("{line:?}").contains("BLOB-SECRET"));
        assert_eq!((b.chars, b.role.as_str()), (11, "user"));
        assert_eq!(line.kind(), "api-request-blob");
    }

    #[test]
    fn request_fields() {
        let line = parse(request(json!(["x", "y"])));
        let Line::ApiRequest(r) = &line else {
            panic!("not a request")
        };
        assert_eq!(r.tail_len, 3);
        assert_eq!(r.keep, 48);
        assert_eq!(r.params.effort.as_deref(), Some("high"));
        assert_eq!(r.params.edits, vec!["clear_thinking_20251015"]);
        assert_eq!(r.cache_breakpoints[0].path, "system/1");
        assert_eq!(r.cache_breakpoints[0].ttl.as_deref(), Some("1h"));
        assert_eq!(line.kind(), "api-request");
        assert_eq!(line.timestamp(), Some("2026-11-27T10:00:00Z"));

        let betas = |v| match parse(request(v)) {
            Line::ApiRequest(r) => r.params.betas_hash,
            _ => panic!(),
        };
        assert_eq!(betas(json!(["x", "y"])), betas(json!(["y", "x"])));
        assert_ne!(betas(json!(["x", "y"])), betas(json!(["x", "y", "z"])));
    }

    #[test]
    fn malformed_becomes_unknown_without_prose() {
        let line = parse(json!({"type": "api-request-shape", "shapeHash": "h",
            "shape": {"system": "notalist", "tools": [{"name": "t", "description": "DESC-SECRET-abc"}]}}));
        let Line::Unknown(v) = &line else {
            panic!("not unknown")
        };
        assert!(v.get("shape").is_none());
        assert!(!format!("{line:?}").contains("DESC-SECRET"));

        let line = parse(json!({"type": "api-request-blob", "hash": "b",
            "message": {"content": [{"type": "text", "text": "BLOB-SECRET"}]}}));
        let Line::Unknown(v) = &line else {
            panic!("not unknown")
        };
        assert!(v.get("message").is_none());
        assert!(!format!("{line:?}").contains("BLOB-SECRET"));

        assert!(matches!(
            parse(json!({"type": "api-request", "keep": "x"})),
            Line::Unknown(_)
        ));
    }
}
