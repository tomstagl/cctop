//! The window's composition: one segment per series in order, each as wide
//! as its first point's `y` is of the x axis; what is left is the track.

use super::frac;
use crate::canvas::{rect, rounded, solid, Canvas};
use crate::grid::Grid;
use crate::spec::{ChartSpec, Role};
use crate::theme::Theme;

/// Each segment's (start, end) as fractions of the width, and its role.
fn segments(spec: &ChartSpec) -> Vec<(f64, f64, Role)> {
    let mut at = spec.x.from;
    let mut out = Vec::new();
    for s in &spec.series {
        let v = s.points.first().map_or(0.0, |p| p.y.max(0.0));
        let a = frac(at, &spec.x).clamp(0.0, 1.0);
        at += v;
        let b = frac(at, &spec.x).clamp(0.0, 1.0);
        if v > 0.0 {
            out.push((a, b, s.role));
        }
    }
    out
}

pub(super) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    let w = g.w;
    for c in 0..w {
        g.column(c, 0, g.h, t.track);
    }
    // Rounded edges tile the width; a segment that rounds to nothing still
    // gets one column while there is room.
    let mut end = 0;
    for (_, b, role) in segments(spec) {
        let start = end;
        end = ((b * w as f64).round() as usize).max(start + 1).min(w);
        for c in start..end {
            g.column(c, 0, g.h, t.fill(role));
        }
    }
}

pub(super) fn picture(spec: &ChartSpec, cv: &mut Canvas, t: &Theme) {
    let (top, bh) = (cv.h * 3.0 / 16.0, cv.h * 10.0 / 16.0);
    let track = rounded(0.0, top, cv.w, bh, bh / 2.0);
    let Some(mask) = cv.mask(&track) else { return };
    cv.fill(track, &solid(t.track, 1.0));
    for (a, b, role) in segments(spec) {
        let (x0, x1) = (a as f32 * cv.w, b as f32 * cv.w);
        cv.fill_masked(rect(x0, top, x1 - x0, bh), &solid(t.fill(role), 1.0), &mask);
    }
}
