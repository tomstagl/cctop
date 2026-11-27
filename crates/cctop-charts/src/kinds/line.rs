//! What `area_forecast` and `cumulative_forecast` share: a filled line over
//! the x axis, a forecast past its end, threshold lines and markers.
//!
//! Cells: each column samples the line at its centre and fills up to the
//! value; past the line's end the forecast fills in its (muted) colour; a
//! threshold is dashed across on alternate columns; a marker fills its
//! column up to the marker in its colour, except `now`, which lights the
//! one pixel at its value.

use super::{col_of, frac, row_of, value_at};
use crate::canvas::{circle, polyline, solid, vertical, Canvas};
use crate::grid::Grid;
use crate::spec::{ChartSpec, Point};
use crate::theme::Theme;

/// How the area under the line is filled in a picture.
#[derive(Clone, Copy)]
pub(super) enum AreaFill {
    /// The series colour fading from 60 % at the top to 6 % at the bottom.
    Fade,
    /// The series colour at 14 % throughout.
    Flat,
}

fn sorted(points: &[Point]) -> Vec<Point> {
    let mut v = points.to_vec();
    v.sort_by(|a, b| a.x.total_cmp(&b.x));
    v
}

pub(super) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    let (w, h) = (g.w, g.h);
    if w == 0 || h == 0 {
        return;
    }
    let span_x = spec.x.to - spec.x.from;
    let x_at = |c: usize| spec.x.from + (c as f64 + 0.5) / w as f64 * span_x;
    // A pixel row is filled when its centre is at or under the value.
    let filled = |py: usize, v: f64| (h - py) as f64 - 0.5 <= frac(v, &spec.y) * h as f64;
    let series: Vec<(Vec<Point>, _)> = spec
        .series
        .iter()
        .map(|s| (sorted(&s.points), t.role(s.role)))
        .collect();
    let end = series
        .iter()
        .filter_map(|(p, _)| p.last().map(|p| p.x))
        .fold(f64::MIN, f64::max);

    if let Some(f) = &spec.forecast {
        let colour = t.fill(f.role);
        for c in 0..w {
            let x = x_at(c);
            if x < f.from.x || x > f.to.x || x <= end {
                continue;
            }
            let k = if f.to.x > f.from.x {
                (x - f.from.x) / (f.to.x - f.from.x)
            } else {
                1.0
            };
            let v = f.from.y + (f.to.y - f.from.y) * k;
            for py in 0..h {
                if filled(py, v) {
                    g.set(c, py, colour);
                }
            }
        }
    }
    for th in &spec.thresholds {
        let f = frac(th.y, &spec.y);
        if (0.0..=1.0).contains(&f) {
            let py = row_of(f, h);
            for c in (0..w).step_by(2) {
                g.set(c, py, t.role(th.role));
            }
        }
    }
    for (points, colour) in &series {
        for c in 0..w {
            let Some(v) = value_at(points, x_at(c)) else {
                continue;
            };
            for py in 0..h {
                if filled(py, v) {
                    g.set(c, py, *colour);
                }
            }
        }
    }
    for m in &spec.markers {
        let Some(c) = col_of(frac(m.x, &spec.x), w) else {
            continue;
        };
        let y =
            m.y.or_else(|| series.iter().find_map(|(p, _)| value_at(p, m.x)));
        let Some(y) = y else { continue };
        let py = row_of(frac(y, &spec.y), h);
        if m.kind == "now" {
            g.set(c, py, t.role(m.role));
        } else {
            g.column(c, py, h, t.role(m.role));
        }
    }
}

pub(super) fn picture(spec: &ChartSpec, cv: &mut Canvas, t: &Theme, fill: AreaFill) {
    let (w, h, u) = (cv.w, cv.h, cv.u);
    let margin = 2.0 * u;
    let px = |x: f64| (frac(x, &spec.x).clamp(0.0, 1.0) as f32) * w;
    let py = |y: f64| (h - margin) - (frac(y, &spec.y).clamp(0.0, 1.0) as f32) * (h - 2.0 * margin);

    // The midline.
    cv.stroke(
        polyline(&[(0.0, h / 2.0), (w, h / 2.0)], false),
        t.track,
        u,
        None,
    );
    if let Some(f) = &spec.forecast {
        let cone = [
            (px(f.from.x), py(f.from.y)),
            (px(f.to.x), py(f.high)),
            (px(f.to.x), py(f.low)),
        ];
        cv.fill(polyline(&cone, true), &solid(t.role(f.role), 0.12));
    }
    for s in &spec.series {
        let points = sorted(&s.points);
        let (Some(first), Some(last)) = (points.first(), points.last()) else {
            continue;
        };
        let colour = t.role(s.role);
        let line: Vec<(f32, f32)> = points.iter().map(|p| (px(p.x), py(p.y))).collect();
        let mut area = line.clone();
        area.push((px(last.x), h));
        area.push((px(first.x), h));
        let paint = match fill {
            AreaFill::Fade => vertical((colour, 0.6), (colour, 0.06), py(spec.y.to), h),
            AreaFill::Flat => solid(colour, 0.14),
        };
        cv.fill(polyline(&area, true), &paint);
        cv.stroke(polyline(&line, false), colour, 2.2 * u, None);
    }
    if let Some(f) = &spec.forecast {
        let fc = [(px(f.from.x), py(f.from.y)), (px(f.to.x), py(f.to.y))];
        cv.stroke(
            polyline(&fc, false),
            t.role(f.role),
            1.6 * u,
            Some((5.0 * u, 4.0 * u)),
        );
    }
    for th in &spec.thresholds {
        let f = frac(th.y, &spec.y);
        if (0.0..=1.0).contains(&f) {
            let y = py(th.y);
            cv.stroke(
                polyline(&[(0.0, y), (w, y)], false),
                t.role(th.role),
                1.3 * u,
                Some((4.0 * u, 4.0 * u)),
            );
        }
    }
    for m in &spec.markers {
        if !(0.0..=1.0).contains(&frac(m.x, &spec.x)) {
            continue;
        }
        let y = m.y.or_else(|| {
            spec.series
                .iter()
                .find_map(|s| value_at(&sorted(&s.points), m.x))
        });
        let Some(y) = y else { continue };
        let r = if m.kind == "now" { 4.0 * u } else { 4.5 * u };
        cv.fill(circle(px(m.x), py(y), r), &solid(t.role(m.role), 1.0));
    }
}
