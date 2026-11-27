//! cctop's chart renderer: one [`ChartSpec`] drawn either as half-block
//! terminal cells ([`render_cells`], for the TUI and the pane's Raster) or
//! as an antialiased picture ([`render_rgba`] then [`encode_png`], for the
//! pane's Image). Both come from the same code and the same spec, so the
//! two versions of a chart cannot disagree about what it shows.
//!
//! No text is ever drawn: every label field of a spec (`name`, `unit`) is
//! ignored here. Labels are terminal text the surface places beside the
//! chart, on whole cells.

mod canvas;
mod grid;
mod kinds;
mod spec;
mod theme;
#[cfg(target_arch = "wasm32")]
mod wasm;

pub use grid::{LOWER_HALF, UPPER_HALF};
pub use spec::{
    Axis, Cell, ChartSpec, Forecast, Kind, Marker, Point, Rgb, Role, Series, Threshold,
};
pub use theme::Theme;

/// The largest picture [`render_rgba`] draws, in pixels (16 MiB of RGBA).
pub const MAX_PIXELS: u64 = 4 << 20;

/// The chart as `cols` × `rows` cells, row-major, two pixels per cell
/// (upper and lower half; [`Cell`] lists the glyphs). Every painted colour
/// is an xterm 256-colour entry ([`Theme::CELLS`]); what nothing paints is
/// `None`, the terminal's default. A spec with a non-finite number draws an
/// empty chart.
pub fn render_cells(spec: &ChartSpec, cols: u16, rows: u16) -> Vec<Cell> {
    let theme = Theme::CELLS;
    let mut g = grid::Grid::new(cols, rows);
    if spec.is_finite() && cols > 0 && rows > 0 {
        kinds::cells(spec, &mut g, &theme);
    }
    g.into_cells()
}

/// The chart as a `px_w` × `px_h` picture: straight (not premultiplied)
/// RGBA, row-major, antialiased, on a transparent background. Empty when
/// either side is 0 or the picture is over [`MAX_PIXELS`]; a spec with a
/// non-finite number draws an empty (transparent) picture.
pub fn render_rgba(spec: &ChartSpec, px_w: u32, px_h: u32, theme: &Theme) -> Vec<u8> {
    if px_w as u64 * px_h as u64 > MAX_PIXELS {
        return Vec::new();
    }
    let Some(mut cv) = canvas::Canvas::new(px_w, px_h) else {
        return Vec::new();
    };
    if spec.is_finite() {
        kinds::picture(spec, &mut cv, theme);
    }
    cv.into_rgba()
}

/// `rgba` (straight RGBA, `w` × `h`) as a PNG file. Empty when the buffer's
/// length is not `w * h * 4` or a side is 0.
pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    if w == 0 || h == 0 || rgba.len() as u64 != w as u64 * h as u64 * 4 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.set_filter(png::Filter::NoFilter);
    let written = enc
        .write_header()
        .and_then(|mut wr| wr.write_image_data(rgba).and_then(|_| wr.finish()));
    if written.is_err() {
        return Vec::new();
    }
    out
}
