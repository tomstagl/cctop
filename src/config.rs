//! `~/.config/cctop/config.toml`: layout, theme, refresh, hidden panels,
//! notify, pricing overrides. CLI flags override; the file is written back
//! when the user changes panels or theme in the TUI.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// The grid's `auto` / `narrow` / `wide` of older versions: read and ignored.
    pub layout: String,
    pub theme: String,
    pub refresh_ms: u64,
    /// Panels older versions hid: read and ignored (nothing is hidden now).
    pub hidden_panels: Vec<u8>,
    pub notify: bool,
    /// Extra pricing TOML merged over the bundled table.
    pub pricing: Option<PathBuf>,
    /// `dashboard` or `coach`: the view the TUI opens with.
    #[serde(default = "default_view")]
    pub view: String,
    /// `on` / `off` / `auto`: whether the coach shows its nudges, or
    /// alternates by session (the control arm of its own measurement).
    #[serde(default = "default_coach")]
    pub coach: String,
}

fn default_view() -> String {
    "dashboard".into()
}

fn default_coach() -> String {
    "on".into()
}

impl Default for Config {
    fn default() -> Config {
        Config {
            layout: "auto".into(),
            theme: "default-dark".into(),
            refresh_ms: crate::app::REFRESH_DEFAULT_MS,
            hidden_panels: Vec::new(),
            notify: false,
            pricing: None,
            view: "dashboard".into(),
            coach: "on".into(),
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        crate::theme::config_dir().map(|d| d.join("config.toml"))
    }

    pub fn load_from(path: &Path) -> Config {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| toml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn load() -> Config {
        Config::path()
            .map(|p| Config::load_from(&p))
            .unwrap_or_default()
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    pub fn save(&self) {
        if let Some(p) = Config::path() {
            let _ = self.save_to(&p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("cctop-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");
        assert_eq!(Config::load_from(&path), Config::default());
        let c = Config {
            layout: "wide".into(),
            theme: "nord".into(),
            refresh_ms: 500,
            hidden_panels: vec![6, 7],
            notify: true,
            pricing: Some(PathBuf::from("/x/pricing.toml")),
            view: "coach".into(),
            coach: "auto".into(),
        };
        c.save_to(&path).unwrap();
        assert_eq!(Config::load_from(&path), c);
        // Partial files fill from defaults.
        std::fs::write(&path, "theme = \"btop\"\n").unwrap();
        let p = Config::load_from(&path);
        assert_eq!(p.theme, "btop");
        assert_eq!(p.refresh_ms, crate::app::REFRESH_DEFAULT_MS);
        assert_eq!(
            p.view, "dashboard",
            "a file without `view` opens the dashboard"
        );
    }
}
