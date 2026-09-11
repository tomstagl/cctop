//! Branch and dirtiness of the session's working directory.

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitInfo {
    pub branch: Option<String>,
    pub dirty: bool,
}

/// `None` when `cwd` is not inside a git work tree (or git is missing).
pub fn info(cwd: &Path) -> Option<GitInfo> {
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    git(&["rev-parse", "--is-inside-work-tree"]).filter(|s| s == "true")?;
    let branch = git(&["symbolic-ref", "--short", "HEAD"])
        .or_else(|| git(&["rev-parse", "--short", "HEAD"]))
        .filter(|b| !b.is_empty());
    let dirty = git(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    Some(GitInfo { branch, dirty })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_repo_has_a_branch_and_nowhere_has_none() {
        let here = Path::new(env!("CARGO_MANIFEST_DIR"));
        let i = info(here).expect("cctop is a git repo");
        assert!(i.branch.is_some());
        assert_eq!(info(Path::new("/")), None);
    }
}
