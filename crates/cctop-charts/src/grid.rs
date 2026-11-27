//! The cell canvas: `cols` × `rows * 2` pixels, two per cell, cut into
//! half-block cells (upper pixel in `fg`, lower in `bg`).

use crate::spec::{Cell, Rgb};
use crate::theme::xterm256;

/// The upper half block: its foreground is the cell's upper pixel, its
/// background the lower one.
pub const HALF_BLOCK: char = '▀';

pub(crate) struct Grid {
    pub w: usize,
    pub h: usize,
    px: Vec<Rgb>,
}

impl Grid {
    pub fn new(cols: u16, rows: u16, bg: Rgb) -> Grid {
        let (w, h) = (cols as usize, rows as usize * 2);
        Grid {
            w,
            h,
            px: vec![bg; w * h],
        }
    }

    /// Paint one pixel; outside the canvas is a no-op.
    pub fn set(&mut self, x: usize, y: usize, c: Rgb) {
        if x < self.w && y < self.h {
            self.px[y * self.w + x] = c;
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
                out.push(Cell {
                    ch: HALF_BLOCK,
                    fg: xterm256(self.px[2 * row * self.w + x]),
                    bg: xterm256(self.px[(2 * row + 1) * self.w + x]),
                });
            }
        }
        out
    }
}
