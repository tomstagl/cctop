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

/// A stacked bar of `width` cells: `parts` are `(tokens, style)` slices of
/// `total`, drawn left to right; the rest of `total` is empty. Adjacent
/// *drawn* segments alternate the fill glyph with the half glyph so they
/// read without colour: the alternation counts emitted segments, never the
/// input index, so a slice that gets no cell (a session with no extended
/// thinking between two others) does not leave its neighbours in the same
/// glyph — the pane has no colour ramp and depends on this alone (PRD
/// dashboard-v2 §3.6, FR-9).
pub fn stacked_bar(
    t: &Theme,
    parts: &[(u64, Style)],
    total: u64,
    width: usize,
) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0usize;
    if total == 0 {
        return vec![Span::styled(t.gauge_empty().repeat(width), t.dim())];
    }
    let mut acc = 0u64;
    let mut emitted = 0usize;
    for (tokens, style) in parts {
        acc += tokens;
        let end = ((acc as f64 / total as f64) * width as f64).round() as usize;
        let cells = end.min(width).saturating_sub(used);
        if cells == 0 {
            continue;
        }
        let glyph = if emitted % 2 == 0 {
            t.gauge_fill()
        } else {
            t.gauge_half()
        };
        emitted += 1;
        out.push(Span::styled(glyph.repeat(cells), *style));
        used += cells;
    }
    if used < width {
        out.push(Span::styled(t.gauge_empty().repeat(width - used), t.dim()));
    }
    out
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
        let s = stacked_bar(
            &t,
            &[
                (20, Style::default()),
                (30, Style::default()),
                (0, Style::default()),
            ],
            100,
            10,
        );
        assert_eq!(
            s.iter().map(|x| x.content.as_ref()).collect::<Vec<_>>(),
            ["▇▇", "▆▆▆", "▁▁▁▁▁"]
        );
        assert_eq!(stacked_bar(&t, &[], 0, 4)[0].content, "▁▁▁▁");
        // A zero-cell middle slice does not consume an alternation: the
        // two slices around it still differ by glyph (the FR-9 test).
        let mono = Theme::default().for_caps(crate::theme::Caps {
            truecolor: false,
            colors256: false,
            mono: true,
            ascii: false,
        });
        let s = stacked_bar(
            &mono,
            &[
                (50, Style::default()),
                (0, Style::default()),
                (50, Style::default()),
            ],
            100,
            10,
        );
        let drawn: Vec<&str> = s.iter().map(|x| x.content.as_ref()).collect();
        assert_eq!(drawn, ["▇▇▇▇▇", "▆▆▆▆▆"]);
        assert!(
            s.iter().all(|x| x.style.fg.is_none()),
            "the bar adds no colour of its own"
        );
        // And a slice too thin for a cell in a wide set neither.
        let s = stacked_bar(
            &mono,
            &[
                (40, Style::default()),
                (1, Style::default()),
                (40, Style::default()),
                (19, Style::default()),
            ],
            100,
            10,
        );
        let drawn: Vec<&str> = s.iter().map(|x| x.content.as_ref()).collect();
        assert_eq!(drawn, ["▇▇▇▇", "▆▆▆▆", "▇▇"]);
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

/// The 3 × 3 block-digit font of the dashboard tiles: one cell between
/// digits, `—` for a missing figure. Every glyph is three rows of three
/// cells of `▀ ▄ █` and spaces.
const BIG: &[(char, [&str; 3])] = &[
    ('0', ["█▀█", "█ █", "▀▀▀"]),
    ('1', [" ▄█", "  █", "  ▀"]),
    ('2', ["▀▀█", "█▀▀", "▀▀▀"]),
    ('3', ["▀▀█", "▀▀█", "▀▀▀"]),
    ('4', ["█ █", "▀▀█", "  ▀"]),
    ('5', ["█▀▀", "▀▀█", "▀▀▀"]),
    ('6', ["█▀▀", "█▀█", "▀▀▀"]),
    ('7', ["▀▀█", "  █", "  ▀"]),
    ('8', ["█▀█", "█▀█", "▀▀▀"]),
    ('9', ["█▀█", "▀▀█", "▀▀▀"]),
    ('—', ["   ", "▀▀▀", "   "]),
    ('.', ["   ", "   ", " ▀ "]),
];

/// `text` (digits, `.`, `—`) as three rows of block glyphs, one cell apart;
/// ASCII terminals get the text itself on the middle row, so the tile keeps
/// its three rows.
pub fn big_digits(t: &Theme, text: &str) -> [String; 3] {
    if t.ascii {
        return [String::new(), text.to_string(), String::new()];
    }
    let mut rows = [String::new(), String::new(), String::new()];
    for (i, c) in text.chars().enumerate() {
        let glyph = BIG
            .iter()
            .find(|(g, _)| *g == c)
            .map(|(_, g)| *g)
            .unwrap_or(["   ", "   ", "   "]);
        for (r, row) in rows.iter_mut().enumerate() {
            if i > 0 {
                row.push(' ');
            }
            row.push_str(glyph[r]);
        }
    }
    rows
}

#[cfg(test)]
mod big_tests {
    use super::*;

    #[test]
    fn every_glyph_is_three_by_three_and_digits_space_one_cell() {
        for (c, g) in BIG {
            for row in g {
                assert_eq!(row.chars().count(), 3, "{c}: {row:?}");
            }
        }
        let t = Theme::default();
        let rows = big_digits(&t, "41");
        assert_eq!(rows, ["█ █  ▄█", "▀▀█   █", "  ▀   ▀"]);
        for r in &rows {
            assert_eq!(r.chars().count(), 7);
        }
        assert_eq!(big_digits(&t, "—")[1], "▀▀▀");
        let ascii = Theme {
            ascii: true,
            ..Theme::default()
        };
        assert_eq!(big_digits(&ascii, "41"), ["", "41", ""]);
    }
}
