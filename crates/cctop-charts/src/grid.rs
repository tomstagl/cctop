//! The cell canvas: `cols` × `rows * 2` pixels, two per cell, each painted
//! or left empty, cut into half-block cells (see [`Cell`] for the glyphs).

use crate::spec::{Cell, Rgb};
use crate::theme::xterm256;

/// The upper half block: `fg` paints the upper pixel, `bg` the lower.
pub const UPPER_HALF: char = '▀';
/// The lower half block: `fg` paints the lower pixel over an empty upper.
pub const LOWER_HALF: char = '▄';

pub(crate) struct Grid {
    pub w: usize,
    pub h: usize,
    px: Vec<Option<Rgb>>,
}

impl Grid {
    pub fn new(cols: u16, rows: u16) -> Grid {
        let (w, h) = (cols as usize, rows as usize * 2);
        Grid {
            w,
            h,
            px: vec![None; w * h],
        }
    }

    /// Paint one pixel; outside the canvas is a no-op.
    pub fn set(&mut self, x: usize, y: usize, c: Rgb) {
        if x < self.w && y < self.h {
            self.px[y * self.w + x] = Some(c);
        }
    }

    /// Paint pixel rows `y0..y1` of column `x`.
    pub fn column(&mut self, x: usize, y0: usize, y1: usize, c: Rgb) {
        for y in y0..y1.min(self.h) {
            self.set(x, y, c);
        }
    }

    /// Row-major cells, every colour snapped to the xterm palette.
    pub fn into_cells(self) -> Vec<Cell> {
        let mut out = Vec::with_capacity(self.w * self.h / 2);
        for row in 0..self.h / 2 {
            for x in 0..self.w {
                let upper = self.px[2 * row * self.w + x].map(xterm256);
                let lower = self.px[(2 * row + 1) * self.w + x].map(xterm256);
                out.push(match (upper, lower) {
                    (Some(_), _) => Cell {
                        ch: UPPER_HALF,
                        fg: upper,
                        bg: lower,
                    },
                    (None, Some(_)) => Cell {
                        ch: LOWER_HALF,
                        fg: lower,
                        bg: None,
                    },
                    (None, None) => Cell {
                        ch: ' ',
                        fg: None,
                        bg: None,
                    },
                });
            }
        }
        out
    }
}
