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

/// Everything `cctop install` adds: the status-line shim and the hooks.
pub fn install_transform(settings: &Value) -> Value {
    crate::hooks::with_hooks(&with_shim(settings))
}

/// Everything `cctop uninstall` removes.
pub fn uninstall_transform(settings: &Value) -> Value {
    crate::hooks::without_hooks(&without_shim(settings))
}

/// The events in `hooks::EVENTS` that have no `cctop hook` entry in these
/// settings, in `EVENTS` order. An old install lists the events added since.
pub fn hooks_missing(settings: &Value) -> Vec<&'static str> {
    crate::hooks::EVENTS
        .iter()
        .copied()
        .filter(|ev| {
            !settings
                .get("hooks")
                .and_then(|h| h.get(*ev))
                .and_then(Value::as_array)
                .is_some_and(|groups| {
                    groups.iter().any(|g| {
                        g.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
                            hs.iter().any(|h| {
                                h.get("command").and_then(Value::as_str)
                                    == Some(crate::hooks::HOOK_COMMAND)
                            })
                        })
                    })
                })
        })
        .collect()
}

/// What an old install should be told: the number of missing events, only
/// when cctop's hooks are installed at all (nothing installed is the
/// existing "run cctop install" hint, not "new events").
pub fn hooks_missing_of_install(settings: &Value) -> Vec<&'static str> {
    let missing = hooks_missing(settings);
    if missing.len() == crate::hooks::EVENTS.len() {
        Vec::new()
    } else {
        missing
    }
}

/// `7 new events, run cctop install` for the sources view and `pane status`.
pub fn hooks_missing_text(n: usize) -> String {
    format!(
        "hooks: {n} new event{}, run cctop install",
        if n == 1 { "" } else { "s" }
    )
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

    /// The 22 events cctop registered before the seven were added.
    fn old_install() -> Value {
        let user = json!({"hooks": {"PreToolUse": [{"matcher":"Bash","hooks":[{"type":"command","command":"lint.sh"}]}]}});
        let mut out = crate::hooks::with_hooks(&user);
        for ev in &crate::hooks::EVENTS[22..] {
            out["hooks"].as_object_mut().unwrap().remove(*ev);
        }
        out
    }

    #[test]
    fn hooks_missing_lists_new_events() {
        let old = old_install();
        let missing = hooks_missing(&old);
        assert_eq!(missing, &crate::hooks::EVENTS[22..]);
        assert_eq!(missing.len(), 7);
        assert_eq!(hooks_missing_of_install(&old).len(), 7);
        assert!(hooks_missing(&install_transform(&old)).is_empty());
        // Nothing installed: all 29 are missing, but it is not an old install.
        assert_eq!(hooks_missing(&json!({})).len(), 29);
        assert!(hooks_missing_of_install(&json!({})).is_empty());
        assert_eq!(
            hooks_missing_text(7),
            "hooks: 7 new events, run cctop install"
        );
    }

    #[test]
    fn install_adds_exactly_seven() {
        let old = old_install();
        let new = install_transform(&old);
        for ev in &crate::hooks::EVENTS[..22] {
            assert_eq!(new["hooks"][*ev], old["hooks"][*ev], "{ev} untouched");
        }
        for ev in &crate::hooks::EVENTS[22..] {
            assert_eq!(new["hooks"][*ev].as_array().unwrap().len(), 1, "{ev}");
        }
        assert_eq!(install_transform(&new), new, "idempotent");
        let d = diff(&pretty(&old), &pretty(&new), Path::new("s.json"));
        assert!(d.contains("+    \"Elicitation\": ["), "{d}");
        assert_eq!(
            d.matches("+            \"command\": \"cctop hook\"")
                .count(),
            7,
            "{d}"
        );
    }

    #[test]
    fn install_beside_user_hooks() {
        // A user hook on a new event, and one on a NEVER event, both kept.
        let user = json!({"hooks": {
            "ConfigChange": [{"hooks":[{"type":"command","command":"audit.sh"}]}],
            "WorktreeCreate": [{"hooks":[{"type":"command","command":"mkwt.sh"}]}],
        }});
        let w = install_transform(&user);
        assert_eq!(
            w["hooks"]["ConfigChange"][0]["hooks"][0]["command"],
            "audit.sh"
        );
        assert_eq!(w["hooks"]["ConfigChange"].as_array().unwrap().len(), 2);
        assert_eq!(
            w["hooks"]["WorktreeCreate"],
            user["hooks"]["WorktreeCreate"]
        );
        assert_eq!(
            uninstall_transform(&w),
            user,
            "uninstall restores the user's"
        );
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
