//! One module per chart kind, each drawing a spec two ways: into the cell
//! grid and onto the picture canvas. The geometry both share lives here.

mod area_forecast;
mod bars;
mod cumulative_forecast;
mod heat;
mod line;
mod pills;
mod progress;
mod stacked_bar;

use crate::canvas::Canvas;
use crate::grid::Grid;
use crate::spec::{Axis, ChartSpec, Kind, Point};
use crate::theme::Theme;

pub(crate) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    match spec.kind {
        Kind::AreaForecast => area_forecast::cells(spec, g, t),
        Kind::CumulativeForecast => cumulative_forecast::cells(spec, g, t),
        Kind::Bars => bars::cells(spec, g, t),
        Kind::Pills => pills::cells(spec, g, t),
        Kind::Heat => heat::cells(spec, g, t),
        Kind::StackedBar => stacked_bar::cells(spec, g, t),
        Kind::Progress => progress::cells(spec, g, t),
    }
}

pub(crate) fn picture(spec: &ChartSpec, c: &mut Canvas, t: &Theme) {
    match spec.kind {
        Kind::AreaForecast => area_forecast::picture(spec, c, t),
        Kind::CumulativeForecast => cumulative_forecast::picture(spec, c, t),
        Kind::Bars => bars::picture(spec, c, t),
        Kind::Pills => pills::picture(spec, c, t),
        Kind::Heat => heat::picture(spec, c, t),
        Kind::StackedBar => stacked_bar::picture(spec, c, t),
        Kind::Progress => progress::picture(spec, c, t),
    }
}

/// Where `v` falls in the axis's domain, 0 at `from` and 1 at `to`
/// (unclamped). An empty domain puts everything at 0.
fn frac(v: f64, a: &Axis) -> f64 {
    let span = a.to - a.from;
    if span > 0.0 {
        (v - a.from) / span
    } else {
        0.0
    }
}

/// The column (of `w`) a fraction of the width falls in, if inside.
fn col_of(f: f64, w: usize) -> Option<usize> {
    (0.0..=1.0)
        .contains(&f)
        .then(|| ((f * w as f64) as usize).min(w.saturating_sub(1)))
}

/// The pixel row (of `h`, 0 on top) a fraction of the height falls in.
fn row_of(f: f64, h: usize) -> usize {
    let up = ((f.clamp(0.0, 1.0) * h as f64) as usize).min(h.saturating_sub(1));
    h.saturating_sub(1) - up
}

/// The per-turn layout of `bars`, `pills` and `heat`: one slot per whole
/// step of the x axis, `from` to `to` inclusive.
struct Slots {
    from: f64,
    n: usize,
}

impl Slots {
    fn of(x: &Axis) -> Slots {
        let span = (x.to - x.from).max(0.0).round();
        Slots {
            from: x.from,
            n: (span as usize).saturating_add(1),
        }
    }

    fn index(&self, x: f64) -> Option<usize> {
        let i = (x - self.from).round();
        (i >= 0.0 && (i as usize) < self.n).then_some(i as usize)
    }

    /// The columns slot `i` marks in a grid `w` wide: its share of the
    /// width, less one column of gap once the share is three or more.
    fn cols(&self, i: usize, w: usize) -> std::ops::Range<usize> {
        let edge = |k: usize| k * w / self.n;
        let (a, b) = (edge(i), edge(i + 1));
        if b - a >= 3 {
            a..b - 1
        } else {
            a..b
        }
    }

    /// Slot `i`'s left edge and width in a picture `w` px wide.
    fn px(&self, i: usize, w: f32) -> (f32, f32) {
        let slot = w / self.n as f32;
        (i as f32 * slot, slot)
    }
}

/// The value of a line through `points` (ascending x) at `x`, if `x` is
/// inside the line.
fn value_at(points: &[Point], x: f64) -> Option<f64> {
    let first = points.first()?;
    let last = points.last()?;
    if x < first.x || x > last.x {
        return None;
    }
    let i = points.partition_point(|p| p.x <= x);
    if i == 0 || i >= points.len() {
        return Some(points[i.saturating_sub(1).min(points.len() - 1)].y);
    }
    let (a, b) = (&points[i - 1], &points[i]);
    let t = if b.x > a.x {
        (x - a.x) / (b.x - a.x)
    } else {
        1.0
    };
    Some(a.y + (b.y - a.y) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(from: f64, to: f64) -> Axis {
        Axis {
            from,
            to,
            unit: String::new(),
        }
    }

    #[test]
    fn slots_leave_a_gap_once_there_is_room() {
        let s = Slots::of(&axis(1.0, 14.0));
        assert_eq!(s.n, 14);
        assert_eq!(s.cols(0, 42), 0..2);
        assert_eq!(s.cols(13, 42), 39..41);
        assert_eq!(s.cols(0, 28), 0..2);
        assert_eq!(s.index(14.0), Some(13));
        assert_eq!(s.index(15.0), None);
        assert_eq!(s.index(0.0), None);
    }

    #[test]
    fn lines_interpolate_inside_and_stop_at_the_ends() {
        let p = |x, y| Point { x, y, role: None };
        let pts = [p(0.0, 0.0), p(2.0, 4.0), p(4.0, 4.0)];
        assert_eq!(value_at(&pts, 1.0), Some(2.0));
        assert_eq!(value_at(&pts, 2.0), Some(4.0));
        assert_eq!(value_at(&pts, 4.0), Some(4.0));
        assert_eq!(value_at(&pts, 4.5), None);
        assert_eq!(value_at(&pts, -0.1), None);
        assert_eq!(value_at(&[p(3.0, 7.0)], 3.0), Some(7.0));
    }

    #[test]
    fn rows_and_columns() {
        assert_eq!(row_of(0.0, 16), 15);
        assert_eq!(row_of(1.0, 16), 0);
        assert_eq!(col_of(1.0, 10), Some(9));
        assert_eq!(col_of(1.01, 10), None);
        assert_eq!(frac(5.0, &axis(5.0, 5.0)), 0.0);
    }
}
