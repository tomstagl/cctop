//! Colours by role, terminal capability detection, and the fallbacks that
//! keep the dashboard legible on 16-colour, monochrome and non-UTF-8 terminals.

use std::path::{Path, PathBuf};

use ratatui::style::{Color, Style};
use serde::Deserialize;

/// What the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub truecolor: bool,
    pub colors256: bool,
    /// `NO_COLOR` set: no colours at all.
    pub mono: bool,
    /// Locale is not UTF-8: draw with ASCII.
    pub ascii: bool,
}

impl Caps {
    pub fn detect(env: &dyn Fn(&str) -> Option<String>) -> Caps {
        let term = env("TERM").unwrap_or_default();
        let colorterm = env("COLORTERM").unwrap_or_default().to_lowercase();
        let lang = env("LC_ALL")
            .or_else(|| env("LC_CTYPE"))
            .or_else(|| env("LANG"))
            .unwrap_or_default()
            .to_lowercase();
        Caps {
            truecolor: colorterm.contains("truecolor")
                || colorterm.contains("24bit")
                || term.contains("truecolor")
                || term.contains("direct"),
            colors256: term.contains("256color")
                || term.contains("kitty")
                || term.contains("wezterm")
                || term.contains("alacritty")
                || term.contains("ghostty"),
            mono: env("NO_COLOR").is_some_and(|v| !v.is_empty()),
            ascii: !lang.contains("utf-8") && !lang.contains("utf8"),
        }
    }

    pub fn full() -> Caps {
        Caps {
            truecolor: true,
            colors256: true,
            mono: false,
            ascii: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ThemeFile {
    name: String,
    bg: String,
    fg: String,
    dim: String,
    accent: String,
    ok: String,
    warn: String,
    crit: String,
    border: String,
    border_focused: String,
}

/// Colours by role, already reduced to what the terminal can show.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub bg: Color,
    pub fg: Color,
    pub dim: Color,
    pub accent: Color,
    pub ok: Color,
    pub warn: Color,
    pub crit: Color,
    pub border: Color,
    pub border_focused: Color,
    pub ascii: bool,
}

pub const BUNDLED: &[(&str, &str)] = &[
    ("default-dark", include_str!("../themes/default-dark.toml")),
    (
        "default-light",
        include_str!("../themes/default-light.toml"),
    ),
    ("btop", include_str!("../themes/btop.toml")),
    ("nord", include_str!("../themes/nord.toml")),
    ("gruvbox", include_str!("../themes/gruvbox.toml")),
    (
        "catppuccin-mocha",
        include_str!("../themes/catppuccin-mocha.toml"),
    ),
];

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        if hex.len() == 6 {
            let v = u32::from_str_radix(hex, 16).ok()?;
            return Some(Color::Rgb(
                (v >> 16) as u8,
                ((v >> 8) & 0xff) as u8,
                (v & 0xff) as u8,
            ));
        }
    }
    match s.to_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" | "grey" => Some(Color::Gray),
        "darkgray" | "darkgrey" => Some(Color::DarkGray),
        "white" => Some(Color::White),
        "reset" => Some(Color::Reset),
        _ => None,
    }
}

/// Nearest of the 16 ANSI colours to an RGB value (used below 256-colour).
fn to_ansi16(c: Color) -> Color {
    let Color::Rgb(r, g, b) = c else { return c };
    let bright = r.max(g).max(b) > 160;
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let hue = match () {
        _ if (r - g).abs() < 40 && (g - b).abs() < 40 => "gray",
        _ if r > g && r > b => {
            if g > b + 40 {
                "yellow"
            } else if b > g + 40 {
                "magenta"
            } else {
                "red"
            }
        }
        _ if g > r && g > b => {
            if b > r + 40 {
                "cyan"
            } else if r > b + 40 {
                "yellow"
            } else {
                "green"
            }
        }
        _ => {
            if g > r + 40 {
                "cyan"
            } else if r > g + 40 {
                "magenta"
            } else {
                "blue"
            }
        }
    };
    match (hue, bright) {
        ("gray", true) => Color::White,
        ("gray", false) => Color::DarkGray,
        ("red", true) => Color::LightRed,
        ("red", false) => Color::Red,
        ("green", true) => Color::LightGreen,
        ("green", false) => Color::Green,
        ("yellow", true) => Color::LightYellow,
        ("yellow", false) => Color::Yellow,
        ("blue", true) => Color::LightBlue,
        ("blue", false) => Color::Blue,
        ("magenta", true) => Color::LightMagenta,
        ("magenta", false) => Color::Magenta,
        _ => {
            if bright {
                Color::LightCyan
            } else {
                Color::Cyan
            }
        }
    }
}

/// Nearest xterm-256 index for an RGB value.
fn to_256(c: Color) -> Color {
    let Color::Rgb(r, g, b) = c else { return c };
    let q = |v: u8| ((v as u16 + 25) / 51) as u8; // 0..=5
    Color::Indexed(16 + 36 * q(r) + 6 * q(g) + q(b))
}

impl Theme {
    pub fn parse(text: &str) -> Option<Theme> {
        let f: ThemeFile = toml::from_str(text).ok()?;
        Some(Theme {
            name: f.name,
            bg: parse_color(&f.bg)?,
            fg: parse_color(&f.fg)?,
            dim: parse_color(&f.dim)?,
            accent: parse_color(&f.accent)?,
            ok: parse_color(&f.ok)?,
            warn: parse_color(&f.warn)?,
            crit: parse_color(&f.crit)?,
            border: parse_color(&f.border)?,
            border_focused: parse_color(&f.border_focused)?,
            ascii: false,
        })
    }

    pub fn bundled(name: &str) -> Option<Theme> {
        BUNDLED
            .iter()
            .find(|(n, _)| *n == name)
            .and_then(|(_, t)| Theme::parse(t))
    }

    /// `themes/*.toml` under `dir`, by name.
    pub fn user_themes(dir: &Path) -> Vec<Theme> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<Theme> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "toml"))
            .filter_map(|p| std::fs::read_to_string(p).ok())
            .filter_map(|t| Theme::parse(&t))
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Bundled names followed by user theme names.
    pub fn names(user_dir: Option<&Path>) -> Vec<String> {
        let mut v: Vec<String> = BUNDLED.iter().map(|(n, _)| n.to_string()).collect();
        if let Some(d) = user_dir {
            for t in Theme::user_themes(d) {
                if !v.contains(&t.name) {
                    v.push(t.name);
                }
            }
        }
        v
    }

    /// Find a theme by name (user dir first, then bundled).
    pub fn find(name: &str, user_dir: Option<&Path>) -> Option<Theme> {
        user_dir
            .and_then(|d| Theme::user_themes(d).into_iter().find(|t| t.name == name))
            .or_else(|| Theme::bundled(name))
    }

    /// Reduce colours to what the terminal supports.
    pub fn for_caps(mut self, caps: Caps) -> Theme {
        let reduce = |c: Color| -> Color {
            if caps.mono {
                Color::Reset
            } else if caps.truecolor {
                c
            } else if caps.colors256 {
                to_256(c)
            } else {
                to_ansi16(c)
            }
        };
        self.bg = if caps.mono {
            Color::Reset
        } else {
            reduce(self.bg)
        };
        self.fg = reduce(self.fg);
        self.dim = reduce(self.dim);
        self.accent = reduce(self.accent);
        self.ok = reduce(self.ok);
        self.warn = reduce(self.warn);
        self.crit = reduce(self.crit);
        self.border = reduce(self.border);
        self.border_focused = reduce(self.border_focused);
        self.ascii = caps.ascii;
        self
    }

    pub fn dim(&self) -> Style {
        Style::default().fg(self.dim)
    }
    pub fn accent(&self) -> Style {
        Style::default().fg(self.accent)
    }
    pub fn ok(&self) -> Style {
        Style::default().fg(self.ok)
    }
    pub fn warn(&self) -> Style {
        Style::default().fg(self.warn)
    }
    pub fn crit(&self) -> Style {
        Style::default().fg(self.crit)
    }

    /// The coach's glyphs (○ ◐ ● ◆ ▸ ↻ ✓ ▇ ▁) with their ASCII fallbacks
    /// (`-` `+` `!` `?` `>` `~` `v` `#` `-`), applied to a whole line.
    pub fn coach_text(&self, s: &str) -> String {
        if !self.ascii {
            return s.to_string();
        }
        s.chars()
            .map(|c| match c {
                '○' => '-',
                '◐' => '+',
                '●' => '!',
                '◆' => '?',
                '▸' => '>',
                '↻' => '~',
                '✓' => 'v',
                '✗' => 'x',
                '▇' => '#',
                '▁' => '-',
                '─' | '—' => '-',
                '│' => '|',
                '╭' | '╮' | '╰' | '╯' | '├' | '┤' => '+',
                '·' => '.',
                '→' => '>',
                '≈' => '~',
                '…' => '.',
                '×' => 'x',
                c => c,
            })
            .collect()
    }

    /// Gauge / sparkline glyphs for this terminal.
    pub fn gauge_fill(&self) -> &'static str {
        if self.ascii {
            "#"
        } else {
            "▇"
        }
    }
    /// The alternate fill of a stacked bar's odd segments.
    pub fn gauge_half(&self) -> &'static str {
        if self.ascii {
            "="
        } else {
            "▆"
        }
    }
    pub fn gauge_empty(&self) -> &'static str {
        if self.ascii {
            "-"
        } else {
            "▁"
        }
    }
    pub fn spark_chars(&self) -> &'static [char] {
        if self.ascii {
            &['_', '.', ':', '-', '=', '+', '*', '#']
        } else {
            &['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█']
        }
    }
    pub fn border_set(&self) -> ratatui::symbols::border::Set {
        if self.ascii {
            ratatui::symbols::border::Set {
                top_left: "+",
                top_right: "+",
                bottom_left: "+",
                bottom_right: "+",
                vertical_left: "|",
                vertical_right: "|",
                horizontal_top: "-",
                horizontal_bottom: "-",
            }
        } else {
            ratatui::symbols::border::ROUNDED
        }
    }
    pub fn hline(&self) -> &'static str {
        if self.ascii {
            "-"
        } else {
            "─"
        }
    }
}

impl Default for Theme {
    fn default() -> Theme {
        Theme::bundled("default-dark").expect("bundled default-dark parses")
    }
}

/// `~/.config/cctop`.
pub fn config_dir() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .map(|c| c.join("cctop"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_theme_parses() {
        for (name, _) in BUNDLED {
            let t = Theme::bundled(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(t.name, *name);
            assert!(matches!(t.accent, Color::Rgb(..)));
        }
        assert_eq!(Theme::names(None).len(), 6);
    }

    #[test]
    fn capability_reduction() {
        let t = Theme::default();
        let env = |pairs: &'static [(&str, &str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(kk, _)| *kk == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        let mono = Caps::detect(&env(&[
            ("NO_COLOR", "1"),
            ("TERM", "xterm-256color"),
            ("LANG", "en_US.UTF-8"),
        ]));
        assert!(mono.mono && !mono.ascii);
        assert_eq!(t.clone().for_caps(mono).accent, Color::Reset);
        let c256 = Caps::detect(&env(&[("TERM", "xterm-256color"), ("LANG", "en_US.UTF-8")]));
        assert!(c256.colors256 && !c256.truecolor);
        assert!(matches!(t.clone().for_caps(c256).accent, Color::Indexed(_)));
        let tc = Caps::detect(&env(&[
            ("TERM", "xterm-256color"),
            ("COLORTERM", "truecolor"),
            ("LANG", "en_US.UTF-8"),
        ]));
        assert!(tc.truecolor);
        assert!(matches!(t.clone().for_caps(tc).accent, Color::Rgb(..)));
        let c16 = Caps::detect(&env(&[("TERM", "xterm"), ("LANG", "C")]));
        assert!(!c16.colors256 && c16.ascii);
        let r = t.clone().for_caps(c16);
        assert_eq!(r.crit, Color::LightRed);
        assert_eq!(r.ok, Color::LightGreen);
        assert!(r.ascii);
        assert_eq!(r.gauge_fill(), "#");
        assert_eq!(r.border_set().top_left, "+");
        assert_eq!(to_256(Color::Rgb(0, 0, 0)), Color::Indexed(16));
        assert_eq!(to_256(Color::Rgb(255, 255, 255)), Color::Indexed(231));
    }

    #[test]
    fn user_theme_dir_and_lookup() {
        let dir = std::env::temp_dir().join(format!("cctop-themes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mine.toml"), "name=\"mine\"\nbg=\"black\"\nfg=\"white\"\ndim=\"darkgray\"\naccent=\"#ff00ff\"\nok=\"green\"\nwarn=\"yellow\"\ncrit=\"red\"\nborder=\"gray\"\nborder_focused=\"#ff00ff\"\n").unwrap();
        std::fs::write(dir.join("broken.toml"), "name=\"x\"\n").unwrap();
        let names = Theme::names(Some(&dir));
        assert_eq!(names.len(), 7);
        assert_eq!(names[6], "mine");
        assert_eq!(
            Theme::find("mine", Some(&dir)).unwrap().accent,
            Color::Rgb(255, 0, 255)
        );
        assert!(Theme::find("nope", Some(&dir)).is_none());
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::app::{buffer_to_string, App};
    use crate::ui::state::{SessionInfo, State};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::Path;

    fn app_with(caps: Caps) -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-a.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.caps = caps;
        crate::attach::attach_headless(&mut app, &path, SessionInfo::from_fixture(&path));
        app.set_theme("default-dark");
        app
    }

    fn render(app: &App) -> (String, ratatui::buffer::Buffer) {
        let mut term = Terminal::new(TestBackend::new(60, 44)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        let buf = term.backend().buffer().clone();
        (buffer_to_string(&buf), buf)
    }

    fn colours_used(buf: &ratatui::buffer::Buffer) -> usize {
        let mut set = std::collections::HashSet::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                set.insert(format!("{:?}", buf[(x, y)].fg));
            }
        }
        set.len()
    }

    #[test]
    fn context_panel_default_dark_snapshot() {
        let (text, buf) = render(&app_with(Caps::full()));
        insta::assert_snapshot!("context_default_dark", text);
        assert!(
            colours_used(&buf) >= 4,
            "truecolour theme uses several fg colours"
        );
    }

    #[test]
    fn context_panel_no_color_is_monochrome() {
        let caps = Caps {
            truecolor: true,
            colors256: true,
            mono: true,
            ascii: false,
        };
        let (text, buf) = render(&app_with(caps));
        assert_eq!(
            colours_used(&buf),
            1,
            "NO_COLOR: every cell has the reset colour"
        );
        assert!(text.contains("1 Context"), "{text}");
    }

    #[test]
    fn context_panel_ascii_snapshot() {
        let caps = Caps {
            truecolor: false,
            colors256: false,
            mono: false,
            ascii: true,
        };
        let (text, _) = render(&app_with(caps));
        insta::assert_snapshot!("context_ascii", text);
        assert!(text.contains("+-"), "ASCII corners: {text}");
        assert!(text.contains("###"), "ASCII gauge: {text}");
        assert!(!text.contains('▇') && !text.contains('╭'), "{text}");
    }

    #[test]
    fn theme_cycles_and_config_roundtrip() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = app_with(Caps::full());
        assert_eq!(app.theme_name, "default-dark");
        app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
        assert_eq!(app.theme_name, "default-light");
        assert_eq!(app.config.theme, "default-light");
        let c = crate::config::Config {
            theme: "nord".into(),
            hidden_panels: vec![7],
            layout: "wide".into(),
            ..Default::default()
        };
        app.apply_config(c);
        assert_eq!(app.theme_name, "nord");
        assert_eq!(app.state.hidden, vec![7]);
        assert_eq!(app.mode_override, Some(crate::ui::layout::Mode::Wide));
    }
}

#[cfg(test)]
mod fixture_b_snapshots {
    //! Every panel on fixture B at the two sizes the coach PRD names
    //! (US-004): the numbers here are the ones the coach's lights read.
    use crate::app::{render_to_string, App};
    use crate::ui::state::{SessionInfo, State};
    use std::path::Path;

    fn app() -> App {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/session-b.jsonl");
        let mut app = App::new(
            crate::ui::panels::all(),
            Box::new(|l, s: &mut State| s.apply(l)),
        );
        app.caps = crate::theme::Caps::full();
        crate::attach::attach_headless(&mut app, &path, SessionInfo::from_fixture(&path));
        app.set_theme("default-dark");
        app
    }

    #[test]
    fn panels_at_60x51_and_40x24() {
        let app = app();
        insta::assert_snapshot!("fixture_b_60x51", render_to_string(&app, 60, 51));
        insta::assert_snapshot!("fixture_b_40x24", render_to_string(&app, 40, 24));
    }
}
