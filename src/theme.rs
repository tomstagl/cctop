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
    /// What `for_caps` reduced to; `series` reduces the ramp the same way.
    pub caps: Caps,
    /// The file's `bg` and `accent` before reduction: the series ramp is
    /// derived from these (a reduced colour may be an index with no RGB).
    pub source_bg: Color,
    pub source_accent: Color,
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
        let bg = parse_color(&f.bg)?;
        let accent = parse_color(&f.accent)?;
        Some(Theme {
            name: f.name,
            bg,
            fg: parse_color(&f.fg)?,
            dim: parse_color(&f.dim)?,
            accent,
            ok: parse_color(&f.ok)?,
            warn: parse_color(&f.warn)?,
            crit: parse_color(&f.crit)?,
            border: parse_color(&f.border)?,
            border_focused: parse_color(&f.border_focused)?,
            ascii: false,
            caps: Caps::full(),
            source_bg: bg,
            source_accent: accent,
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

    /// One colour as the terminal can show it.
    fn reduce(caps: Caps, c: Color) -> Color {
        if caps.mono {
            Color::Reset
        } else if caps.truecolor {
            c
        } else if caps.colors256 {
            to_256(c)
        } else {
            to_ansi16(c)
        }
    }

    /// Reduce colours to what the terminal supports. The file's `bg` and
    /// `accent` are kept aside for `series`.
    pub fn for_caps(mut self, caps: Caps) -> Theme {
        let reduce = |c: Color| Theme::reduce(caps, c);
        self.caps = caps;
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

    /// The composition ramp: `n` colours from the theme's own `accent`,
    /// step 0 what the person cannot change, step `n − 1` what they can
    /// (`crate::series`, PRD dashboard-v2 §5.2). Derived from the
    /// unreduced accent and background, then reduced like every other
    /// colour — on 16 colours and under `NO_COLOR` steps may coincide, and
    /// the alternating fill glyph carries the order alone (FR-9). A theme
    /// whose `bg` or `accent` is a named colour has no RGB to derive from
    /// and gets the accent at every step.
    pub fn series(&self, n: usize) -> Vec<Color> {
        let rgb = |c: Color| match c {
            Color::Rgb(r, g, b) => Some((r, g, b)),
            _ => None,
        };
        match (rgb(self.source_bg), rgb(self.source_accent)) {
            (Some(bg), Some(accent)) => crate::series::ramp(bg, accent, n)
                .into_iter()
                .map(|(r, g, b)| Theme::reduce(self.caps, Color::Rgb(r, g, b)))
                .collect(),
            _ => vec![self.accent; n],
        }
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
                '▆' => '=',
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
    fn dashboard_default_dark_snapshot() {
        let (text, buf) = render(&app_with(Caps::full()));
        insta::assert_snapshot!("dashboard_default_dark", text);
        assert!(
            colours_used(&buf) >= 4,
            "truecolour theme uses several fg colours"
        );
    }

    /// `Theme::series` is derived from the file's colours, not the reduced
    /// ones, and reduced like the rest (PRD dashboard-v2 US-105). Measured
    /// on the six bundled themes: 256 colours keep three distinct steps on
    /// every theme; 16 colours keep the fixed step apart from the rest and
    /// merge the two bright ones on five of six (nord keeps three) — so the
    /// 16-colour terminal is declared glyph-only beyond step 0, which the
    /// alternating fill in `stacked_bar` carries; `NO_COLOR` is glyph-only
    /// entirely.
    #[test]
    fn series_survives_reduction_as_declared() {
        let distinct = |v: &[Color]| {
            v.iter()
                .map(|c| format!("{c:?}"))
                .collect::<std::collections::BTreeSet<_>>()
                .len()
        };
        for (name, _) in BUNDLED {
            let t = Theme::bundled(name).unwrap();
            let full = t.clone().for_caps(Caps::full()).series(3);
            assert!(full.iter().all(|c| matches!(c, Color::Rgb(..))), "{name}");
            assert_eq!(distinct(&full), 3, "{name} truecolor");
            let c256 = t
                .clone()
                .for_caps(Caps {
                    truecolor: false,
                    colors256: true,
                    mono: false,
                    ascii: false,
                })
                .series(3);
            assert!(
                c256.iter().all(|c| matches!(c, Color::Indexed(_))),
                "{name}"
            );
            assert_eq!(distinct(&c256), 3, "{name} 256: {c256:?}");
            let c16 = t
                .clone()
                .for_caps(Caps {
                    truecolor: false,
                    colors256: false,
                    mono: false,
                    ascii: false,
                })
                .series(3);
            assert_eq!(c16[0], Color::DarkGray, "{name} 16: {c16:?}");
            assert!(distinct(&c16) >= 2, "{name} 16: {c16:?}");
            let mono = t
                .for_caps(Caps {
                    truecolor: true,
                    colors256: true,
                    mono: true,
                    ascii: false,
                })
                .series(3);
            assert!(mono.iter().all(|c| *c == Color::Reset), "{name} NO_COLOR");
        }
        // A reduced theme still derives from the file's accent: the ramp of
        // a 16-colour theme is not the ramp of `Color::Cyan`.
        let t = Theme::bundled("default-dark").unwrap().for_caps(Caps {
            truecolor: false,
            colors256: false,
            mono: false,
            ascii: false,
        });
        assert_eq!(t.accent, Color::LightCyan);
        assert_eq!(t.source_accent, Color::Rgb(0x4C, 0xC2, 0xC2));
        // A theme with a named colour has nothing to derive from.
        let named = Theme {
            source_accent: Color::Cyan,
            ..Theme::default()
        };
        assert_eq!(named.series(3), vec![named.accent; 3]);
    }

    #[test]
    fn dashboard_no_color_is_monochrome() {
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
        assert!(text.contains("1: ctx"), "{text}");
    }

    #[test]
    fn dashboard_ascii_snapshot() {
        let caps = Caps {
            truecolor: false,
            colors256: false,
            mono: false,
            ascii: true,
        };
        // The context body: its bars are the gauges ASCII must survive.
        let mut app = app_with(caps);
        app.state.console_body = Some(0);
        let (text, _) = render(&app);
        insta::assert_snapshot!("dashboard_ascii", text);
        assert!(text.contains("###"), "ASCII gauge: {text}");
        assert!(
            !text.contains('▇') && !text.contains('╭') && !text.contains('█'),
            "{text}"
        );
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
        assert_eq!(
            app.theme_name, "nord",
            "layout and hidden_panels are read and ignored"
        );
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

    /// Console at the widths the dock gives (PRD dashboard-v2 §3.6: 35,
    /// 54, 67, 85) and the standalone terminal (122), every body's rows
    /// measured before render — a `TestBackend` buffer is `width` cells by
    /// construction, so measuring after would be vacuous — and every panel
    /// full-screen.
    #[test]
    fn dashboard_sizes_and_full_screen_panels() {
        let app = app();
        let d = app.dashboard();
        for (w, h) in [(122, 24), (85, 24), (67, 24), (54, 24), (35, 24)] {
            for open in 0..d.bodies.len() {
                let rows = crate::ui::dashboard::compose(&app.state.theme, &d, w, h, open);
                assert!(
                    rows.len() <= h as usize,
                    "{w}x{h} body {open}: {} rows",
                    rows.len()
                );
                for r in &rows {
                    let cells: usize = r.spans.iter().map(|s| s.content.chars().count()).sum();
                    assert!(
                        cells <= w as usize,
                        "{w}x{h} body {open}: {cells} cells: {r:?}"
                    );
                }
            }
            insta::assert_snapshot!(
                format!("fixture_b_dashboard_{w}x{h}"),
                render_to_string(&app, w, h)
            );
        }
        let mut app = app;
        app.state.console_body = Some(0);
        insta::assert_snapshot!(
            "fixture_b_console_context_85x24",
            render_to_string(&app, 85, 24)
        );
        app.state.console_body = Some(3);
        insta::assert_snapshot!(
            "fixture_b_console_cost_85x24",
            render_to_string(&app, 85, 24)
        );
        app.state.console_body = Some(5);
        insta::assert_snapshot!(
            "fixture_b_console_tools_85x24",
            render_to_string(&app, 85, 24)
        );
        app.state.console_body = Some(crate::dashboard::ADVISOR_BODY);
        insta::assert_snapshot!(
            "fixture_b_console_advisor_54x24",
            render_to_string(&app, 54, 24)
        );
        app.state.console_body = None;
        for id in 1..=9u8 {
            app.state.overlay = Some(id);
            insta::assert_snapshot!(
                format!("fixture_b_panel_{id}_80x30"),
                render_to_string(&app, 80, 30)
            );
        }
    }
}
