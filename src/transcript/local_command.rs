//! `system/local_command` lines: Claude Code records a slash command as
//! `<command-name>/x</command-name>…` and, when it printed something, a
//! second line with `<local-command-stdout>…</local-command-stdout>`. The
//! one capture cctop reads is `/context`'s table, the official breakdown of
//! the prefix and the autocompact window.

/// One `local_command` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalCommand {
    /// `/context`, `/model`, `/clear` … as typed (arguments dropped).
    Command(String),
    /// The command's printed output, ANSI escapes removed.
    Stdout(String),
}

impl LocalCommand {
    pub fn parse(content: &str) -> Option<LocalCommand> {
        let t = content.trim_start();
        if let Some(rest) = t.strip_prefix("<command-name>") {
            let end = rest.find("</command-name>")?;
            return Some(LocalCommand::Command(rest[..end].trim().to_string()));
        }
        if let Some(rest) = t.strip_prefix("<local-command-stdout>") {
            let body = rest.strip_suffix("</local-command-stdout>").unwrap_or(rest);
            return Some(LocalCommand::Stdout(strip_ansi(body)));
        }
        None
    }

    /// The `/context` table, when this is its output.
    pub fn context_capture(&self) -> Option<ContextCapture> {
        match self {
            LocalCommand::Stdout(s) if s.contains("Context Usage") => ContextCapture::parse(s),
            _ => None,
        }
    }
}

/// Remove CSI escape sequences (`ESC [ … m` and friends).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                // Parameters and intermediates, then the final byte 0x40–0x7E.
                for c in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// What `/context` printed: the official prefix categories, the window and
/// the autocompact buffer, in tokens.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextCapture {
    pub model: Option<String>,
    /// `88.7k/1m tokens (9%)`.
    pub used_tokens: u64,
    pub window_tokens: u64,
    pub used_pct: Option<f64>,
    /// `(category, tokens)` in the printed order: System prompt, System
    /// tools, Skills, Messages, Memory files, …
    pub categories: Vec<(String, u64)>,
    pub free_tokens: Option<u64>,
    /// `Autocompact buffer: 33k tokens` — where the warn band starts.
    pub autocompact_buffer: Option<u64>,
    /// `Auto-compact window: 1m tokens` — the effective window.
    pub autocompact_window: Option<u64>,
    /// `(count, tokens)` of MCP tools and of skills.
    pub mcp_tools: Option<(u64, u64)>,
    pub skills: Option<(u64, u64)>,
}

impl ContextCapture {
    pub fn parse(text: &str) -> Option<ContextCapture> {
        let mut cap = ContextCapture::default();
        let mut section = "";
        let mut seen_usage = false;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            // Strip the histogram glyphs at the left of the top block.
            let line = line.trim_start_matches(['⛁', '⛀', '⛶', '⛝', ' ']).trim();
            if let Some((count, tok)) = line
                .strip_prefix("└")
                .and_then(|r| parse_count_tokens(r.trim()))
            {
                match section {
                    "mcp" => cap.mcp_tools = Some((count, tok)),
                    "skills" => cap.skills = Some((count, tok)),
                    _ => {}
                }
                continue;
            }
            if line.starts_with("MCP tools") {
                section = "mcp";
                continue;
            }
            if line.starts_with("Skills") && (line.contains("/skills") || line.ends_with("Skills"))
            {
                section = "skills";
                continue;
            }
            if let Some(rest) = line.strip_prefix("Auto-compact window:") {
                cap.autocompact_window = parse_tokens(rest.trim());
                continue;
            }
            if let Some(rest) = line.strip_prefix("Autocompact buffer:") {
                cap.autocompact_buffer = parse_tokens(rest.trim());
                continue;
            }
            if let Some(rest) = line.strip_prefix("Free space:") {
                cap.free_tokens = parse_tokens(rest.trim());
                continue;
            }
            if !seen_usage {
                if let Some((used, window, pct)) = parse_usage(line) {
                    cap.used_tokens = used;
                    cap.window_tokens = window;
                    cap.used_pct = pct;
                    seen_usage = true;
                    continue;
                }
                if line.starts_with("claude-") {
                    cap.model = Some(line.to_string());
                    continue;
                }
            }
            if let Some((name, rest)) = line.split_once(':') {
                if !name.contains(' ') || name.split_whitespace().count() <= 3 {
                    if let Some(tok) = parse_tokens(rest.trim()) {
                        cap.categories.push((name.trim().to_string(), tok));
                    }
                }
            }
        }
        (seen_usage || !cap.categories.is_empty()).then_some(cap)
    }

    /// Tokens of a category by its printed name.
    pub fn category(&self, name: &str) -> Option<u64> {
        self.categories
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, t)| *t)
    }
}

/// `88.7k/1m tokens (9%)` → `(88_700, 1_000_000, Some(9.0))`.
fn parse_usage(line: &str) -> Option<(u64, u64, Option<f64>)> {
    let (used, rest) = line.split_once('/')?;
    let used = parse_k(used.trim())?;
    let mut it = rest.split_whitespace();
    let window = parse_k(it.next()?)?;
    if it.next()? != "tokens" {
        return None;
    }
    let pct = it.next().and_then(|p| {
        p.trim_matches(|c| c == '(' || c == ')' || c == '%')
            .parse::<f64>()
            .ok()
    });
    Some((used, window, pct))
}

/// `10.2k tokens (1.0%)` / `877.9k (87.8%)` / `0 tokens` → tokens.
fn parse_tokens(s: &str) -> Option<u64> {
    parse_k(s.split_whitespace().next()?)
}

/// `297 tools · 0 tokens` / `49 skills · 4.8k tokens` → `(297, 0)`.
fn parse_count_tokens(s: &str) -> Option<(u64, u64)> {
    let (count, rest) = s.split_once("·")?;
    let count = count.split_whitespace().next()?.parse().ok()?;
    Some((count, parse_tokens(rest.trim())?))
}

/// `88.7k` / `1m` / `33k` / `1234` → tokens.
pub fn parse_k(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches(',');
    let (num, mult) = match s.chars().last()? {
        'k' | 'K' => (&s[..s.len() - 1], 1_000.0),
        'm' | 'M' => (&s[..s.len() - 1], 1_000_000.0),
        _ => (s, 1.0),
    };
    let n: f64 = num.parse().ok()?;
    Some((n * mult).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::tests_support::fixture;
    use crate::transcript::{Line, SystemKind};

    const CAPTURE: &str = "<local-command-stdout> \u{1b}[1mContext Usage\u{1b}[22m\n\u{1b}[38;2;136;136;136m⛁ ⛁ \u{1b}[38;2;153;153;153m⛁ ⛁ ⛶ ⛶ \u{1b}[39m  Sonnet 5\n\u{1b}[38;2;153;153;153m⛶ ⛶ \u{1b}[39m  \u{1b}[38;2;153;153;153mclaude-sonnet-5\u{1b}[39m\n⛶ ⛶   \u{1b}[38;2;153;153;153m88.7k/1m tokens (9%)\u{1b}[39m\n⛶ ⛶ \n⛶ ⛶   \u{1b}[3mEstimated usage by category\u{1b}[23m\n⛶ ⛶   \u{1b}[38;2;136;136;136m⛁\u{1b}[39m System prompt: \u{1b}[38;2;153;153;153m10.2k tokens (1.0%)\u{1b}[39m\n⛶ ⛶   ⛁ System tools: 31.2k tokens (3.1%)\n⛶ ⛶   ⛁ Skills: 4.8k tokens (0.5%)\n⛶ ⛶   ⛁ Messages: 42.9k tokens (4.3%)\n⛶ ⛝ ⛝   ⛶ Free space: 877.9k (87.8%)\n                                          ⛝ Autocompact buffer: 33k tokens (3.3%)\n\n\u{1b}[1mAuto-compact window: \u{1b}[22m1m tokens\n\n\u{1b}[1mMCP tools\u{1b}[22m · /mcp (loaded on-demand)\n└ 297 tools · 0 tokens\n\n\u{1b}[1mSkills\u{1b}[22m · /skills\n└ 49 skills · 4.8k tokens\n\n/context all to expand</local-command-stdout>";

    #[test]
    fn strip_ansi_and_parse_numbers() {
        assert_eq!(
            strip_ansi("\u{1b}[1mbold\u{1b}[22m x \u{1b}[38;2;1;2;3mc\u{1b}[39m"),
            "bold x c"
        );
        assert_eq!(parse_k("88.7k"), Some(88_700));
        assert_eq!(parse_k("1m"), Some(1_000_000));
        assert_eq!(parse_k("33k"), Some(33_000));
        assert_eq!(parse_k("0"), Some(0));
        assert_eq!(parse_k("x"), None);
        assert_eq!(
            parse_usage("88.7k/1m tokens (9%)"),
            Some((88_700, 1_000_000, Some(9.0)))
        );
        assert_eq!(parse_count_tokens("297 tools · 0 tokens"), Some((297, 0)));
    }

    #[test]
    fn context_table_is_parsed() {
        let cmd = LocalCommand::parse(
            "<command-name>/context</command-name>\n<command-message>context</command-message>",
        )
        .unwrap();
        assert_eq!(cmd, LocalCommand::Command("/context".into()));
        assert_eq!(cmd.context_capture(), None);
        let out = LocalCommand::parse(CAPTURE).unwrap();
        let cap = out.context_capture().expect("capture");
        assert_eq!(cap.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(cap.used_tokens, 88_700);
        assert_eq!(cap.window_tokens, 1_000_000);
        assert_eq!(cap.used_pct, Some(9.0));
        assert_eq!(
            cap.categories,
            vec![
                ("System prompt".to_string(), 10_200),
                ("System tools".to_string(), 31_200),
                ("Skills".to_string(), 4_800),
                ("Messages".to_string(), 42_900),
            ]
        );
        assert_eq!(cap.category("system tools"), Some(31_200));
        assert_eq!(cap.free_tokens, Some(877_900));
        assert_eq!(cap.autocompact_buffer, Some(33_000));
        assert_eq!(cap.autocompact_window, Some(1_000_000));
        assert_eq!(cap.mcp_tools, Some((297, 0)));
        assert_eq!(cap.skills, Some((49, 4_800)));
        assert_eq!(LocalCommand::parse("plain text"), None);
    }

    #[test]
    fn fixture_b_carries_a_context_capture() {
        let lines = fixture("session-b");
        let caps: Vec<ContextCapture> = lines
            .iter()
            .filter_map(|l| match l {
                Line::System(s) if s.kind() == SystemKind::LocalCommand => {
                    s.local_command()?.context_capture()
                }
                _ => None,
            })
            .collect();
        assert_eq!(caps.len(), 1);
        assert!(caps[0].window_tokens >= 200_000);
        assert!(caps[0].category("System prompt").is_some());
        let names: Vec<LocalCommand> = lines
            .iter()
            .filter_map(|l| match l {
                Line::System(s) => s.local_command(),
                _ => None,
            })
            .collect();
        // `/clear` is recorded on its user line only; `/context` also as a
        // local_command pair (the command, then the captured table).
        assert!(names.contains(&LocalCommand::Command("/context".into())));
        assert!(
            names
                .iter()
                .filter(|n| matches!(n, LocalCommand::Stdout(_)))
                .count()
                >= 2
        );
    }
}
