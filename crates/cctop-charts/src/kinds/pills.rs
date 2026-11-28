//! The cache: one pill per turn, the full height of the chart, coloured by
//! the point's role (rebuilt, the first turn, no call) or the series'
//! (reused). A marker (`thinking_dropped`) lights the top of its pill.

use super::Slots;
use crate::canvas::{circle, rounded, solid, Canvas};
use crate::grid::Grid;
use crate::spec::ChartSpec;
use crate::theme::Theme;

pub(super) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    let slots = Slots::of(&spec.x);
    for s in &spec.series {
        for p in &s.points {
            let Some(i) = slots.index(p.x) else { continue };
            for c in slots.cols(i, g.w) {
                g.column(c, 0, g.h, t.fill(p.role.unwrap_or(s.role)));
            }
        }
    }
    for m in &spec.markers {
        let Some(i) = slots.index(m.x) else { continue };
        for c in slots.cols(i, g.w) {
            g.set(c, 0, t.role(m.role));
        }
    }
}

pub(super) fn picture(spec: &ChartSpec, cv: &mut Canvas, t: &Theme) {
    let slots = Slots::of(&spec.x);
    let (top, ph) = (cv.h * 0.125, cv.h * 0.75);
    for s in &spec.series {
        for p in &s.points {
            let Some(i) = slots.index(p.x) else { continue };
            let (x, slot) = slots.px(i, cv.w);
            let pw = slot * 2.0 / 3.0;
            cv.fill(
                rounded(x, top, pw, ph, ph * 7.0 / 24.0),
                &solid(t.fill(p.role.unwrap_or(s.role)), 1.0),
            );
        }
    }
    for m in &spec.markers {
        let Some(i) = slots.index(m.x) else { continue };
        let (x, slot) = slots.px(i, cv.w);
        let pw = slot * 2.0 / 3.0;
        let r = pw.min(ph) * 0.18;
        cv.fill(
            circle(x + pw / 2.0, top + ph * 0.1 + r, r),
            &solid(t.role(m.role), 1.0),
        );
    }
}
