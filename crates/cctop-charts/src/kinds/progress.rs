//! The cache countdown: one value of a whole, the first series' first
//! point's `x` along the x axis, on a track. Cells round down, so the bar
//! is full only when the value is.

use super::frac;
use crate::canvas::{rounded, solid, Canvas};
use crate::grid::Grid;
use crate::spec::{ChartSpec, Role};
use crate::theme::Theme;

fn value(spec: &ChartSpec) -> Option<(f64, Role)> {
    let s = spec.series.first()?;
    let p = s.points.first()?;
    Some((frac(p.x, &spec.x).clamp(0.0, 1.0), p.role.unwrap_or(s.role)))
}

pub(super) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    let w = g.w;
    for c in 0..w {
        g.column(c, 0, g.h, t.track);
    }
    if let Some((f, role)) = value(spec) {
        let mut n = (f * w as f64).floor() as usize;
        if n == 0 && f > 0.0 {
            n = 1;
        }
        for c in 0..n.min(w) {
            g.column(c, 0, g.h, t.fill(role));
        }
    }
}

pub(super) fn picture(spec: &ChartSpec, cv: &mut Canvas, t: &Theme) {
    let (top, bh) = (cv.h / 4.0, cv.h / 2.0);
    cv.fill(rounded(0.0, top, cv.w, bh, bh / 2.0), &solid(t.track, 1.0));
    if let Some((f, role)) = value(spec) {
        let fw = f as f32 * cv.w;
        if fw > 0.0 {
            cv.fill(
                rounded(0.0, top, fw, bh, bh / 2.0),
                &solid(t.fill(role), 1.0),
            );
        }
    }
}
