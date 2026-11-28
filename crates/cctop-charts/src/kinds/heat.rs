//! Tool calls: one row per series, one cell per turn, brighter = more
//! calls (four steps relative to the y axis's top: the theme's fixed
//! ramp for an `accent` row, else the row's colour scaled towards black).
//! A marker with a `row` (`failed`) paints that cell.

use super::{frac, Slots};
use crate::canvas::{rounded, solid, Canvas};
use crate::grid::Grid;
use crate::spec::{ChartSpec, Rgb, Role};
use crate::theme::{mix, Theme};

/// A role without a fixed ramp scales its colour towards black rather than
/// mixing it with the empty cell: that keeps the hue, so the xterm palette
/// has a hued entry for each step instead of greys.
const STEPS: [f64; 4] = [0.4, 0.6, 0.8, 1.0];

/// The cell colour of `v` calls in a row drawn with `role`.
fn shade(spec: &ChartSpec, t: &Theme, v: f64, role: Role) -> Rgb {
    if v <= 0.0 {
        return t.empty();
    }
    let f = frac(v, &spec.y);
    let step = if spec.y.to > spec.y.from {
        ((f * 4.0).ceil() as usize).clamp(1, 4)
    } else {
        4
    };
    match role {
        Role::Accent => t.heat[step - 1],
        r => mix((0, 0, 0), t.role(r), STEPS[step - 1]),
    }
}

/// Every cell to paint: (row, slot, colour), markers last.
fn marks(spec: &ChartSpec, t: &Theme) -> Vec<(usize, usize, Rgb)> {
    let slots = Slots::of(&spec.x);
    let mut out = Vec::new();
    for (r, s) in spec.series.iter().enumerate() {
        for p in &s.points {
            if let Some(i) = slots.index(p.x) {
                out.push((r, i, shade(spec, t, p.y, p.role.unwrap_or(s.role))));
            }
        }
    }
    for m in &spec.markers {
        if let (Some(r), Some(i)) = (m.row, slots.index(m.x)) {
            if r < spec.series.len() {
                out.push((r, i, t.role(m.role)));
            }
        }
    }
    out
}

pub(super) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    let slots = Slots::of(&spec.x);
    let rows = spec.series.len();
    if rows == 0 {
        return;
    }
    // Each row's band of pixel rows, its last pixel left as a gap once the
    // band is two or more high.
    let h = g.h;
    let band = |r: usize| {
        let (a, b) = (r * h / rows, (r + 1) * h / rows);
        if b - a >= 2 {
            a..b - 1
        } else {
            a..b
        }
    };
    for (r, i, colour) in marks(spec, t) {
        for c in slots.cols(i, g.w) {
            for y in band(r) {
                g.set(c, y, colour);
            }
        }
    }
}

pub(super) fn picture(spec: &ChartSpec, cv: &mut Canvas, t: &Theme) {
    let slots = Slots::of(&spec.x);
    let rows = spec.series.len();
    if rows == 0 {
        return;
    }
    let band = cv.h / rows as f32;
    let (bh, by) = (band * 0.75, band * 0.125);
    for (r, i, colour) in marks(spec, t) {
        let (x, slot) = slots.px(i, cv.w);
        let bw = slot * 0.87;
        cv.fill(
            rounded(
                x + (slot - bw) / 2.0,
                r as f32 * band + by,
                bw,
                bh,
                bw.min(bh) * 0.25,
            ),
            &solid(colour, 1.0),
        );
    }
}
