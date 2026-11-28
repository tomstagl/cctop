//! The cells as the pane's Raster element takes them. One encoder for the
//! wasm's mode 1 and for the Rust side's `fallback_cells`, so the two are
//! byte-identical by construction.

use crate::Cell;

/// A colour that is the terminal's default.
const DEFAULT_COLOUR: u32 = 0x0100_0000;

/// Standard padded base64 (ASCII) of little-endian u32 triplets
/// `[code point, fg, bg]` per cell, a colour `0x00RRGGBB` or `0x01000000`
/// for the terminal's default.
pub fn encode_cells(cells: &[Cell]) -> Vec<u8> {
    let colour = |c: Option<(u8, u8, u8)>| {
        c.map_or(DEFAULT_COLOUR, |(r, g, b)| {
            ((r as u32) << 16) | ((g as u32) << 8) | b as u32
        })
    };
    let mut words = Vec::with_capacity(cells.len() * 12);
    for cell in cells {
        for w in [cell.ch as u32, colour(cell.fg), colour(cell.bg)] {
            words.extend_from_slice(&w.to_le_bytes());
        }
    }
    base64(&words)
}

fn base64(data: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| (n << 8) | *b as u32) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[((n >> (18 - 6 * i)) & 63) as usize]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}
