//! The `claude` process's environment, for the handful of variables that
//! change how Claude Code behaves (autocompact overrides, the cache TTL
//! source, reminders). Read from `ps eww` on macOS and `/proc/<pid>/environ`
//! on Linux; only allowlisted keys are kept, and secrets are never read.

use std::collections::BTreeMap;
use std::process::Command;

/// Variables cctop reads. Values are kept for these; presence only for
/// [`PRESENCE_ONLY`].
pub const KEYS: &[&str] = &[
    "CLAUDE_CODE_AUTO_COMPACT_WINDOW",
    "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE",
    "DISABLE_AUTO_COMPACT",
    "DISABLE_COMPACT",
    "CLAUDE_CODE_TOTAL_TOKENS_REMINDER",
    "CLAUDE_CODE_ENABLE_FUNCTION_HOOKS",
    "CLAUDE_CODE_ENTRYPOINT",
    "MAX_THINKING_TOKENS",
    "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
    "OTEL_METRIC_EXPORT_INTERVAL",
];

/// Variables whose presence matters and whose value must never be read.
pub const PRESENCE_ONLY: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
];

/// The allowlisted variables of a process: value, or `"<set>"` for the
/// presence-only ones.
pub fn env_of(pid: u32) -> BTreeMap<String, String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(bytes) = std::fs::read(format!("/proc/{pid}/environ")) {
            return filter(
                bytes
                    .split(|b| *b == 0)
                    .map(|s| String::from_utf8_lossy(s).into_owned()),
            );
        }
    }
    let Ok(out) = Command::new("ps")
        .args(["eww", "-o", "command=", "-p", &pid.to_string()])
        .output()
    else {
        return BTreeMap::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    filter(text.split_whitespace().map(str::to_string))
}

/// Keep only the allowlisted `KEY=VALUE` pairs.
pub fn filter(pairs: impl Iterator<Item = String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for pair in pairs {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if KEYS.contains(&k) {
            out.insert(k.to_string(), v.to_string());
        } else if PRESENCE_ONLY.contains(&k) {
            out.insert(k.to_string(), "<set>".to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_keeps_allowlisted_keys_and_hides_secrets() {
        let env = filter(
            [
                "CLAUDE_CODE_AUTO_COMPACT_WINDOW=400000",
                "ANTHROPIC_API_KEY=sk-ant-secret",
                "CLAUDE_CODE_MESSAGING_TOKEN=nope",
                "HOME=/x",
                "garbage",
            ]
            .into_iter()
            .map(String::from),
        );
        assert_eq!(env["CLAUDE_CODE_AUTO_COMPACT_WINDOW"], "400000");
        assert_eq!(env["ANTHROPIC_API_KEY"], "<set>");
        assert!(!env.contains_key("CLAUDE_CODE_MESSAGING_TOKEN"));
        assert!(!env.contains_key("HOME"));
        assert_eq!(env.len(), 2);
        // Our own process: the call works and returns only allowlisted keys.
        let own = env_of(std::process::id());
        assert!(own
            .keys()
            .all(|k| KEYS.contains(&k.as_str()) || PRESENCE_ONLY.contains(&k.as_str())));
    }
}
