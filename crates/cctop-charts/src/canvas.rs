//! The picture canvas: a tiny-skia pixmap, antialiased paths, and the few
//! shapes the chart kinds draw with. There is no text API here on purpose.

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, Path,
    PathBuilder, Pixmap, Point, Rect, Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

use crate::spec::Rgb;

pub(crate) struct Canvas {
    pixmap: Pixmap,
    pub w: f32,
    pub h: f32,
    /// The stroke unit: 1 px per 128 px of height, never below 1 (a chart
    /// eight cells high at 1× has 1 px lines; at 2×, 2 px).
    pub u: f32,
}

impl Canvas {
    pub fn new(px_w: u32, px_h: u32) -> Option<Canvas> {
        let pixmap = Pixmap::new(px_w, px_h)?;
        let (w, h) = (px_w as f32, px_h as f32);
        Some(Canvas {
            pixmap,
            w,
            h,
            u: (h / 128.0).max(1.0),
        })
    }

    pub fn fill(&mut self, path: Option<Path>, paint: &Paint) {
        if let Some(p) = path {
            self.pixmap
                .fill_path(&p, paint, FillRule::Winding, Transform::identity(), None);
        }
    }

    pub fn fill_masked(&mut self, path: Option<Path>, paint: &Paint, mask: &Mask) {
        if let Some(p) = path {
            self.pixmap.fill_path(
                &p,
                paint,
                FillRule::Winding,
                Transform::identity(),
                Some(mask),
            );
        }
    }

    /// A mask of this canvas's size, covering `path`.
    pub fn mask(&self, path: &Option<Path>) -> Option<Mask> {
        let mut m = Mask::new(self.pixmap.width(), self.pixmap.height())?;
        m.fill_path(
            path.as_ref()?,
            FillRule::Winding,
            true,
            Transform::identity(),
        );
        Some(m)
    }

    /// Stroke `path` `width` wide; `dash` is (on, off).
    pub fn stroke(&mut self, path: Option<Path>, c: Rgb, width: f32, dash: Option<(f32, f32)>) {
        let Some(p) = path else { return };
        let stroke = Stroke {
            width,
            line_cap: if dash.is_some() {
                LineCap::Butt
            } else {
                LineCap::Round
            },
            line_join: LineJoin::Round,
            dash: dash.and_then(|(on, off)| StrokeDash::new(vec![on, off], 0.0)),
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(&p, &solid(c, 1.0), &stroke, Transform::identity(), None);
    }

    /// Straight (not premultiplied) RGBA, row-major. Demultiplied in
    /// integers (rounded to nearest): exact, and most pixels are fully
    /// transparent or opaque and skip the division.
    pub fn into_rgba(self) -> Vec<u8> {
        let mut out = self.pixmap.take();
        for p in out.chunks_exact_mut(4) {
            let a = p[3] as u32;
            if a != 0 && a != 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        out
    }
}

pub(crate) fn solid(c: Rgb, alpha: f32) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(c.0, c.1, c.2, alpha_u8(alpha));
    paint
}

/// A vertical gradient from `top` at `y0` to `bottom` at `y1`, each with
/// its alpha; solid `top` when the two ys meet.
pub(crate) fn vertical(top: (Rgb, f32), bottom: (Rgb, f32), y0: f32, y1: f32) -> Paint<'static> {
    let colour = |(c, a): (Rgb, f32)| Color::from_rgba8(c.0, c.1, c.2, alpha_u8(a));
    let shader = LinearGradient::new(
        Point::from_xy(0.0, y0),
        Point::from_xy(0.0, y1),
        vec![
            GradientStop::new(0.0, colour(top)),
            GradientStop::new(1.0, colour(bottom)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(colour(top)));
    Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    }
}

fn alpha_u8(a: f32) -> u8 {
    (a.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(crate) fn rect(x: f32, y: f32, w: f32, h: f32) -> Option<Path> {
    Some(PathBuilder::from_rect(Rect::from_xywh(x, y, w, h)?))
}

/// A rectangle with all four corners rounded by `r` (clamped to fit).
pub(crate) fn rounded(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r == 0.0 {
        return rect(x, y, w, h);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// A rectangle standing on `y + h` with its two top corners rounded.
pub(crate) fn rounded_top(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h).max(0.0);
    if r == 0.0 {
        return rect(x, y, w, h);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(x, y + h);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h);
    pb.close();
    pb.finish()
}

pub(crate) fn circle(cx: f32, cy: f32, r: f32) -> Option<Path> {
    PathBuilder::from_circle(cx, cy, r)
}

/// An open line through `points`; `close` makes it a polygon.
pub(crate) fn polyline(points: &[(f32, f32)], close: bool) -> Option<Path> {
    let (first, rest) = points.split_first()?;
    let mut pb = PathBuilder::new();
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    if close {
        pb.close();
    }
    pb.finish()
}
