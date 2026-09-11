//! Small drawing helpers shared by panels: gauges and sparklines.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Colour for a 0–1 fill: ok below `warn`, amber below `crit`, red above.
pub fn band_style(ratio: f64, warn: f64, crit: f64) -> Style {
    let c = if ratio >= crit {
        Color::Red
    } else if ratio >= warn {
        Color::Yellow
    } else {
        Color::Green
    };
    Style::default().fg(c)
}

/// A horizontal gauge of `width` cells, filled `ratio` (0–1).
pub fn gauge(ratio: f64, width: usize, style: Style) -> Vec<Span<'static>> {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    vec![
        Span::styled("▇".repeat(filled), style),
        Span::styled(
            "▁".repeat(width - filled),
            Style::default().fg(Color::DarkGray),
        ),
    ]
}

/// Block-character sparkline of the last `width` values, scaled to their max.
pub fn sparkline(values: &[u64], width: usize) -> String {
    let start = values.len().saturating_sub(width);
    let v = &values[start..];
    let max = v.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return String::new();
    }
    v.iter()
        .map(|&x| BLOCKS[((x as f64 / max as f64) * 7.0).round() as usize])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spark_and_gauge() {
        assert_eq!(sparkline(&[1, 2, 4, 8], 4), "▂▃▅█");
        assert_eq!(sparkline(&[0, 0], 2), "");
        assert_eq!(sparkline(&[1, 2, 3, 4, 5], 2), "▇█");
        let g = gauge(0.5, 10, Style::default());
        assert_eq!(g[0].content, "▇▇▇▇▇");
        assert_eq!(g[1].content, "▁▁▁▁▁");
        assert_eq!(band_style(0.5, 0.6, 0.8).fg, Some(Color::Green));
        assert_eq!(band_style(0.7, 0.6, 0.8).fg, Some(Color::Yellow));
        assert_eq!(band_style(0.9, 0.6, 0.8).fg, Some(Color::Red));
    }
}
