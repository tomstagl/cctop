//! Colours by role: the two palettes a chart is drawn with, and the mixing
//! both renderers share.

use crate::spec::{Rgb, Role};

/// What a renderer resolves a spec's roles against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// What the chart sits on. Cells paint it; a picture leaves it
    /// transparent and only mixes against it.
    pub bg: Rgb,
    /// Tracks, the baseline, the midline: the parts of a chart with no data.
    pub track: Rgb,
    pub fg: Rgb,
    pub dim: Rgb,
    pub accent: Rgb,
    pub ok: Rgb,
    pub warn: Rgb,
    pub crit: Rgb,
    pub s0: Rgb,
    pub s1: Rgb,
    pub s2: Rgb,
}

impl Theme {
    /// Pictures on a dark terminal (the approved Ghostty board, POC 7).
    pub const DARK: Theme = Theme {
        bg: (0x17, 0x19, 0x1C),
        track: (0x25, 0x2A, 0x31),
        fg: (0xF1, 0xF3, 0xF5),
        dim: (0x7C, 0x86, 0x93),
        accent: (0x4C, 0xC2, 0xC2),
        ok: (0x5F, 0xC7, 0x7E),
        warn: (0xE2, 0xB0, 0x4A),
        crit: (0xE5, 0x60, 0x5E),
        s0: (0x7F, 0xA8, 0xC9),
        s1: (0xB4, 0x8C, 0xE0),
        s2: (0x4C, 0xC2, 0xC2),
    };

    /// Cells: xterm 256-colour entries only, so a chart looks the same in
    /// every terminal (the approved Terminal.app board, POC 8). Mixed
    /// colours are snapped back to the palette when the cells are cut.
    pub const CELLS: Theme = Theme {
        bg: (0x1C, 0x1C, 0x1C),
        track: (0x3A, 0x3A, 0x3A),
        fg: (0xEE, 0xEE, 0xEE),
        dim: (0x80, 0x80, 0x80),
        accent: (0x5F, 0xD7, 0xD7),
        ok: (0x5F, 0xD7, 0x87),
        warn: (0xD7, 0xAF, 0x5F),
        crit: (0xD7, 0x5F, 0x5F),
        s0: (0x5F, 0x87, 0xAF),
        s1: (0xAF, 0x87, 0xD7),
        s2: (0x5F, 0xD7, 0xD7),
    };

    pub fn role(&self, role: Role) -> Rgb {
        match role {
            Role::Fg => self.fg,
            Role::Dim => self.dim,
            Role::Accent => self.accent,
            Role::Ok => self.ok,
            Role::Warn => self.warn,
            Role::Crit => self.crit,
            Role::S0 => self.s0,
            Role::S1 => self.s1,
            Role::S2 => self.s2,
        }
    }

    /// A filled mark's colour: `dim` is drawn at half strength over the
    /// background (the first turn's pill, a forecast, free space), so it
    /// reads as "not data" rather than as grey data.
    pub(crate) fn fill(&self, role: Role) -> Rgb {
        match role {
            Role::Dim => mix(self.bg, self.dim, 0.5),
            r => self.role(r),
        }
    }

    /// An empty heat cell.
    pub(crate) fn empty(&self) -> Rgb {
        mix(self.bg, self.track, 0.5)
    }
}

/// `a` → `b` by `t` in 0..=1, per channel in sRGB.
pub(crate) fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let ch = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    (ch(a.0, b.0), ch(a.1, b.1), ch(a.2, b.2))
}

/// The nearest xterm 256-colour entry (the colour cube and the grey ramp;
/// the 16 system colours are left out because terminals restyle them). A
/// colour with a hue is matched in the cube only, so a dark teal stays teal
/// instead of landing on the nearest grey.
pub(crate) fn xterm256(c: Rgb) -> Rgb {
    const HUED: u8 = 24;
    const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let dist = |p: Rgb| {
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        d(c.0, p.0) + d(c.1, p.1) + d(c.2, p.2)
    };
    let mut best = (i32::MAX, c);
    let cube = CUBE.iter().flat_map(|r| {
        CUBE.iter()
            .flat_map(move |g| CUBE.iter().map(move |b| (*r, *g, *b)))
    });
    let chroma = c.0.max(c.1).max(c.2) - c.0.min(c.1).min(c.2);
    let greys = (0..24u8)
        .filter(|_| chroma < HUED)
        .map(|i| (8 + 10 * i, 8 + 10 * i, 8 + 10 * i));
    for p in cube.chain(greys) {
        let d = dist(p);
        if d < best.0 {
            best = (d, p);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cell_palette_is_xterm_entries() {
        let t = Theme::CELLS;
        for c in [
            t.bg, t.track, t.fg, t.dim, t.accent, t.ok, t.warn, t.crit, t.s0, t.s1, t.s2,
        ] {
            assert_eq!(xterm256(c), c, "{c:?}");
        }
    }

    #[test]
    fn mixing() {
        assert_eq!(mix((0, 0, 0), (200, 100, 50), 0.5), (100, 50, 25));
        assert_eq!(mix((10, 10, 10), (20, 20, 20), 0.0), (10, 10, 10));
        assert_eq!(mix((10, 10, 10), (20, 20, 20), 1.0), (20, 20, 20));
        // Dim at half strength over the cell background: POC 8's first pill.
        assert_eq!(Theme::CELLS.fill(Role::Dim), (0x4E, 0x4E, 0x4E));
    }

    #[test]
    fn hued_colours_keep_their_hue_in_the_palette() {
        // The darkest heat step of the accent: a teal, not #444444.
        assert_eq!(xterm256((38, 86, 86)), (0x00, 0x5F, 0x5F));
        // Near-neutral colours may land on the grey ramp: an empty heat cell.
        assert_eq!(xterm256(Theme::CELLS.empty()), (0x26, 0x26, 0x26));
    }
}
