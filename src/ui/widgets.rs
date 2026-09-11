//! Small drawing helpers shared by panels: gauges and sparklines.

use ratatui::style::Style;
use ratatui::text::Span;

use crate::theme::Theme;

/// Colour for a 0–1 fill: ok below `warn`, warn below `crit`, crit above.
pub fn band_style(t: &Theme, ratio: f64, warn: f64, crit: f64) -> Style {
    if ratio >= crit {
        t.crit()
    } else if ratio >= warn {
        t.warn()
    } else {
        t.ok()
    }
}

/// A horizontal gauge of `width` cells, filled `ratio` (0–1).
pub fn gauge(t: &Theme, ratio: f64, width: usize, style: Style) -> Vec<Span<'static>> {
    let filled = ((ratio.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    vec![
        Span::styled(t.gauge_fill().repeat(filled), style),
        Span::styled(t.gauge_empty().repeat(width - filled), t.dim()),
    ]
}

/// Sparkline of the last `width` values, scaled to their max.
pub fn sparkline(t: &Theme, values: &[u64], width: usize) -> String {
    let chars = t.spark_chars();
    let start = values.len().saturating_sub(width);
    let v = &values[start..];
    let max = v.iter().copied().max().unwrap_or(0);
    if max == 0 {
        return String::new();
    }
    v.iter()
        .map(|&x| chars[((x as f64 / max as f64) * 7.0).round() as usize])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spark_and_gauge() {
        let t = Theme::default();
        assert_eq!(sparkline(&t, &[1, 2, 4, 8], 4), "▂▃▅█");
        assert_eq!(sparkline(&t, &[0, 0], 2), "");
        assert_eq!(sparkline(&t, &[1, 2, 3, 4, 5], 2), "▇█");
        let g = gauge(&t, 0.5, 10, Style::default());
        assert_eq!(g[0].content, "▇▇▇▇▇");
        assert_eq!(g[1].content, "▁▁▁▁▁");
        assert_eq!(band_style(&t, 0.5, 0.6, 0.8).fg, Some(t.ok));
        assert_eq!(band_style(&t, 0.7, 0.6, 0.8).fg, Some(t.warn));
        assert_eq!(band_style(&t, 0.9, 0.6, 0.8).fg, Some(t.crit));
        let a = Theme::default().for_caps(crate::theme::Caps {
            truecolor: true,
            colors256: true,
            mono: false,
            ascii: true,
        });
        assert_eq!(sparkline(&a, &[1, 8], 2), ".#");
        assert_eq!(gauge(&a, 0.5, 4, Style::default())[0].content, "##");
    }
}
