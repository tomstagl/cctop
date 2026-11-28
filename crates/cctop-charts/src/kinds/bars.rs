//! Cost per turn: one bar per turn from the baseline, coloured by the
//! point's role (a helper agent, a cache rebuild) or the series'. A marker
//! (`over_200k`) lights the top of its turn's bar.

use super::{frac, Slots};
use crate::canvas::{circle, polyline, rounded_top, solid, vertical, Canvas};
use crate::grid::Grid;
use crate::spec::{ChartSpec, Point, Role};
use crate::theme::{mix, Theme};

/// Each turn's (slot, value, role) of the first series.
fn bars(spec: &ChartSpec) -> Vec<(usize, f64, Role)> {
    let slots = Slots::of(&spec.x);
    let Some(s) = spec.series.first() else {
        return Vec::new();
    };
    s.points
        .iter()
        .filter_map(|p: &Point| Some((slots.index(p.x)?, p.y, p.role.unwrap_or(s.role))))
        .collect()
}

pub(super) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    let slots = Slots::of(&spec.x);
    let h = g.h;
    let height = |v: f64| {
        let f = frac(v, &spec.y).clamp(0.0, 1.0);
        let n = (f * h as f64).round() as usize;
        if n == 0 && v > spec.y.from {
            1
        } else {
            n
        }
    };
    let all = bars(spec);
    for (i, v, role) in &all {
        let n = height(*v);
        for c in slots.cols(*i, g.w) {
            g.column(c, h - n, h, t.fill(*role));
        }
    }
    for m in &spec.markers {
        let Some(i) = slots.index(m.x) else { continue };
        let n = all
            .iter()
            .find(|b| b.0 == i)
            .map_or(0, |b| height(b.1))
            .max(1);
        for c in slots.cols(i, g.w) {
            g.set(c, h - n, t.role(m.role));
        }
    }
}

pub(super) fn picture(spec: &ChartSpec, cv: &mut Canvas, t: &Theme) {
    let slots = Slots::of(&spec.x);
    let (w, h, u) = (cv.w, cv.h, cv.u);
    let base = h - u;
    let full = h - 4.0 * u;
    cv.stroke(
        polyline(&[(0.0, h - 0.5 * u), (w, h - 0.5 * u)], false),
        t.track,
        u,
        None,
    );
    let height = |v: f64| {
        let bh = frac(v, &spec.y).clamp(0.0, 1.0) as f32 * full;
        if v > spec.y.from {
            bh.max(u)
        } else {
            bh
        }
    };
    let all = bars(spec);
    for (i, v, role) in &all {
        let bh = height(*v);
        if bh <= 0.0 {
            continue;
        }
        let (x, slot) = slots.px(*i, w);
        let bw = slot * 2.0 / 3.0;
        let top = base - bh;
        let c = t.fill(*role);
        let paint = vertical(
            (mix(c, (255, 255, 255), 0.25), 1.0),
            (mix(c, (0, 0, 0), 0.35), 1.0),
            top,
            base,
        );
        cv.fill(rounded_top(x, top, bw, bh, bw * 0.2), &paint);
    }
    for m in &spec.markers {
        let Some(i) = slots.index(m.x) else { continue };
        let bh = all.iter().find(|b| b.0 == i).map_or(0.0, |b| height(b.1));
        let (x, slot) = slots.px(i, w);
        let bw = slot * 2.0 / 3.0;
        let r = (bw / 4.0).min(2.0 * u);
        cv.fill(
            circle(x + bw / 2.0, (base - bh + r + u).min(base - r), r),
            &solid(t.role(m.role), 1.0),
        );
    }
}
