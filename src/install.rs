//! `cctop install` / `cctop uninstall`: wire the status-line shim (and, from
//! the hooks story, the hook emitter) into `~/.claude/settings.json`, with a
//! backup and a visible diff.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub const SHIM_MARK: &str = "cctop statusline-shim";

pub fn settings_path() -> PathBuf {
    std::env::var_os("CLAUDE_SETTINGS")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/settings.json"))
        })
        .unwrap_or_else(|| PathBuf::from("settings.json"))
}

/// The settings with the shim wrapped around the current status line.
/// Idempotent: an already-shimmed command is left alone.
pub fn with_shim(settings: &Value) -> Value {
    let mut out = settings.clone();
    let current = settings
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if current.starts_with(SHIM_MARK) {
        return out;
    }
    let command = if current.trim().is_empty() {
        SHIM_MARK.to_string()
    } else {
        format!("{SHIM_MARK} -- {current}")
    };
    let mut sl = settings
        .get("statusLine")
        .cloned()
        .unwrap_or_else(|| json!({}));
    sl["type"] = json!("command");
    sl["command"] = json!(command);
    out["statusLine"] = sl;
    out
}

/// The settings with the shim removed (original command restored).
pub fn without_shim(settings: &Value) -> Value {
    let mut out = settings.clone();
    let current = settings
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(rest) = current.strip_prefix(SHIM_MARK) {
        let original = rest
            .trim_start()
            .strip_prefix("--")
            .map(str::trim)
            .unwrap_or("");
        if original.is_empty() {
            out.as_object_mut().map(|m| m.remove("statusLine"));
        } else {
            out["statusLine"]["command"] = json!(original);
        }
    }
    out
}

pub fn pretty(v: &Value) -> String {
    let mut s = serde_json::to_string_pretty(v).unwrap_or_default();
    s.push('\n');
    s
}

/// Unified diff of two settings texts.
pub fn diff(before: &str, after: &str, path: &Path) -> String {
    let name = path.display().to_string();
    similar::TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&format!("{name} (before)"), &format!("{name} (after)"))
        .to_string()
}

/// Copy the settings file to `<home>/backups/settings-<ts>.json`.
pub fn backup(home: &Path, path: &Path) -> std::io::Result<PathBuf> {
    let dir = home.join("backups");
    std::fs::create_dir_all(&dir)?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = dir.join(format!("settings-{ts}.json"));
    std::fs::copy(path, &dest)?;
    Ok(dest)
}

pub fn read_settings(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({}))
}

/// Ask on the terminal; `yes` skips the question.
fn confirm(question: &str, yes: bool) -> bool {
    if yes {
        return true;
    }
    eprint!("{question} [y/N] ");
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
    matches!(line.trim(), "y" | "Y" | "yes")
}

/// Apply `transform` to the settings file with backup, diff and confirmation.
/// Returns true when the file was changed.
pub fn apply(
    path: &Path,
    yes: bool,
    transform: impl Fn(&Value) -> Value,
    verb: &str,
) -> std::io::Result<bool> {
    apply_in(&crate::status::cctop_dir(), path, yes, transform, verb)
}

/// [`apply`] with an explicit cctop home (backups and the install marker).
pub fn apply_in(
    home: &Path,
    path: &Path,
    yes: bool,
    transform: impl Fn(&Value) -> Value,
    verb: &str,
) -> std::io::Result<bool> {
    let before_v = read_settings(path);
    let after_v = transform(&before_v);
    let before = if path.exists() {
        pretty(&before_v)
    } else {
        String::new()
    };
    let after = pretty(&after_v);
    if before == after {
        println!("cctop: nothing to {verb} in {}", path.display());
        return Ok(false);
    }
    print!("{}", diff(&before, &after, path));
    if !confirm(&format!("Apply these changes to {}?", path.display()), yes) {
        println!("cctop: aborted");
        return Ok(false);
    }
    if path.exists() {
        let b = backup(home, path)?;
        println!("cctop: backup at {}", b.display());
    } else if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, after)?;
    let marker = home.join("installed");
    if verb == "install" {
        let _ = std::fs::create_dir_all(home);
        let _ = std::fs::write(marker, "");
    } else {
        let _ = std::fs::remove_file(marker);
    }
    println!("cctop: {verb}ed in {}", path.display());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_wraps_and_unwraps_idempotently() {
        let s = json!({"statusLine":{"type":"command","command":"bash ~/.claude/statusline-command.sh"},"other":1});
        let w = with_shim(&s);
        assert_eq!(
            w["statusLine"]["command"],
            "cctop statusline-shim -- bash ~/.claude/statusline-command.sh"
        );
        assert_eq!(w["other"], 1);
        assert_eq!(with_shim(&w), w, "idempotent");
        assert_eq!(without_shim(&w), s, "restores the original");
        // No status line configured: plain shim, and uninstall removes it.
        let empty = json!({"model":"opus"});
        let w = with_shim(&empty);
        assert_eq!(w["statusLine"]["command"], "cctop statusline-shim");
        assert_eq!(without_shim(&w), empty);
    }

    #[test]
    fn diff_shows_the_changed_line() {
        let a = json!({"statusLine":{"command":"x"}});
        let d = diff(&pretty(&a), &pretty(&with_shim(&a)), Path::new("s.json"));
        assert!(d.contains("-    \"command\": \"x\""), "{d}");
        assert!(
            d.contains("+    \"command\": \"cctop statusline-shim -- x\""),
            "{d}"
        );
    }

    #[test]
    fn apply_writes_backup_and_marker() {
        let home = std::env::temp_dir().join(format!("cctop-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let path = home.join("settings.json");
        std::fs::write(&path, pretty(&json!({"statusLine":{"command":"orig"}}))).unwrap();
        assert!(apply_in(&home, &path, true, with_shim, "install").unwrap());
        assert!(read_settings(&path)["statusLine"]["command"]
            .as_str()
            .unwrap()
            .starts_with(SHIM_MARK));
        assert!(home.join("installed").exists());
        assert_eq!(std::fs::read_dir(home.join("backups")).unwrap().count(), 1);
        assert!(
            !apply_in(&home, &path, true, with_shim, "install").unwrap(),
            "second run is a no-op"
        );
        assert!(apply_in(&home, &path, true, without_shim, "uninstall").unwrap());
        assert_eq!(read_settings(&path)["statusLine"]["command"], "orig");
        assert!(!home.join("installed").exists());
    }
}
