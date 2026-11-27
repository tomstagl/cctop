//! The WebAssembly surface: what `plugin/hooks/charts.wasm` exports, for the
//! pane's `$.wasm.compile`. No imports, no wasm-bindgen: plain `extern "C"`
//! functions over linear memory.
//!
//! - `alloc(len) -> ptr` and `free(ptr, len)` hand the caller buffers (the
//!   spec goes in through one; a result comes out through [`render`]).
//! - `render(kind, spec_ptr, spec_len, cols, rows, scale, mode) -> ptr`
//!   draws the spec JSON (`kind` is the [`Kind`] index, overriding the
//!   spec's own) and returns `[u32 LE len][bytes]`; free it with
//!   `free(ptr, 4 + len)`. `mode` 0 is a PNG of `cols*8*scale` ×
//!   `rows*16*scale` pixels; `mode` 1 is the cells as the Raster element's
//!   `cells` prop: standard padded base64 (ASCII) of little-endian u32
//!   triplets `[code point, fg, bg]`, a colour `0x00RRGGBB` or `0x01000000`
//!   for the terminal's default. Anything that fails (bad JSON, a size over
//!   the limits) returns `len` 0.

use crate::{encode_cells, encode_png, render_cells, render_rgba, ChartSpec, Kind, Theme};

/// The kinds by index, in the order of [`Kind`]'s variants.
const KINDS: [Kind; 7] = [
    Kind::AreaForecast,
    Kind::CumulativeForecast,
    Kind::Bars,
    Kind::Pills,
    Kind::Heat,
    Kind::StackedBar,
    Kind::Progress,
];

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    Box::leak(vec![0u8; len].into_boxed_slice()).as_mut_ptr()
}

/// # Safety
/// `ptr` and `len` are exactly what `alloc(len)` (or `render`'s `4 + len`)
/// returned, freed once.
#[no_mangle]
pub unsafe extern "C" fn free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
    }
}

/// # Safety
/// `spec_ptr..spec_ptr + spec_len` is readable memory (from `alloc`).
#[no_mangle]
pub unsafe extern "C" fn render(
    kind: u32,
    spec_ptr: *const u8,
    spec_len: usize,
    cols: u32,
    rows: u32,
    scale: u32,
    mode: u32,
) -> *mut u8 {
    let json = std::slice::from_raw_parts(spec_ptr, spec_len);
    let bytes = draw(kind, json, cols, rows, scale, mode).unwrap_or_default();
    let mut out = Vec::with_capacity(4 + bytes.len());
    out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&bytes);
    Box::leak(out.into_boxed_slice()).as_mut_ptr()
}

fn draw(kind: u32, json: &[u8], cols: u32, rows: u32, scale: u32, mode: u32) -> Option<Vec<u8>> {
    let mut spec: ChartSpec = serde_json::from_slice(json).ok()?;
    spec.kind = *KINDS.get(kind as usize)?;
    // The Raster's own bounds: 1..=512 columns, 1..=256 rows.
    let (c, r) = (u16::try_from(cols).ok()?, u16::try_from(rows).ok()?);
    if c == 0 || r == 0 || c > 512 || r > 256 {
        return None;
    }
    match mode {
        0 => {
            let (w, h) = (
                (cols * 8).checked_mul(scale)?,
                (rows * 16).checked_mul(scale)?,
            );
            let rgba = render_rgba(&spec, w, h, &Theme::DARK);
            let png = encode_png(&rgba, w, h);
            (!png.is_empty()).then_some(png)
        }
        1 => Some(encode_cells(&render_cells(&spec, c, r))),
        _ => None,
    }
}
