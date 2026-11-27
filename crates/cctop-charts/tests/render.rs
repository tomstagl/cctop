//! Golden tests for both renderings of every chart kind, on fixture specs:
//! `poc.json` is the session the approved boards draw (POC 7 pictures,
//! POC 8 cells), `session-b.json` is `cctop query overview` on
//! `fixtures/session-b.jsonl`. Accept new goldens with
//! `INSTA_UPDATE=always cargo test -p cctop-charts`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use cctop_charts::{
    encode_png, render_cells, render_rgba, Cell, ChartSpec, Kind, Rgb, Theme, HALF_BLOCK,
    MAX_PIXELS,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize, Clone)]
struct Fixture {
    name: String,
    cols: u16,
    rows: u16,
    spec: ChartSpec,
}

fn load(file: &str) -> Vec<Fixture> {
    let path = format!("{}/tests/fixtures/{file}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn poc() -> Vec<Fixture> {
    load("poc.json")
}

fn every() -> Vec<Fixture> {
    let mut v = poc();
    v.extend(load("session-b.json"));
    v
}

/// The picture size of a chart at `scale` × an 8 × 16 px cell.
fn px(f: &Fixture, scale: u32) -> (u32, u32) {
    (f.cols as u32 * 8 * scale, f.rows as u32 * 16 * scale)
}

/// Cells as pixels, two text lines per cell row, one letter per colour
/// (`.` the background), the colours listed below.
fn pixels_text(cells: &[Cell], cols: u16) -> String {
    let bg = Theme::CELLS.bg;
    let mut names: BTreeMap<Rgb, char> = BTreeMap::new();
    let mut order: Vec<Rgb> = Vec::new();
    let mut letter = |c: Rgb| -> char {
        if c == bg {
            return '.';
        }
        let next = (b'a' + order.len() as u8) as char;
        *names.entry(c).or_insert_with(|| {
            order.push(c);
            next
        })
    };
    let mut out = String::new();
    for row in cells.chunks(cols as usize) {
        let upper: String = row.iter().map(|c| letter(c.fg)).collect();
        let lower: String = row.iter().map(|c| letter(c.bg)).collect();
        writeln!(out, "{upper}\n{lower}").unwrap();
    }
    out.push('\n');
    for c in &order {
        writeln!(out, "{} #{:02x}{:02x}{:02x}", names[c], c.0, c.1, c.2).unwrap();
    }
    out
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut s, b| {
            write!(s, "{b:02x}").unwrap();
            s
        })
}

#[test]
fn every_kind_has_a_fixture() {
    let kinds: Vec<Kind> = poc().iter().map(|f| f.spec.kind).collect();
    for k in [
        Kind::AreaForecast,
        Kind::CumulativeForecast,
        Kind::Bars,
        Kind::Pills,
        Kind::Heat,
        Kind::StackedBar,
        Kind::Progress,
    ] {
        assert!(kinds.contains(&k), "{k:?}");
    }
}

#[test]
fn cells_per_kind() {
    for f in poc() {
        let cells = render_cells(&f.spec, f.cols, f.rows);
        insta::assert_snapshot!(format!("cells_{}", f.name), pixels_text(&cells, f.cols));
    }
}

#[test]
fn cells_are_half_blocks_in_the_xterm_palette() {
    const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let xterm = |c: Rgb| {
        [c.0, c.1, c.2].iter().all(|v| CUBE.contains(v))
            || (c.0 == c.1 && c.1 == c.2 && c.0 >= 8 && (c.0 - 8) % 10 == 0 && c.0 <= 238)
    };
    for f in every() {
        let cells = render_cells(&f.spec, f.cols, f.rows);
        assert_eq!(cells.len(), f.cols as usize * f.rows as usize, "{}", f.name);
        for c in &cells {
            assert_eq!(c.ch, HALF_BLOCK);
            assert!(xterm(c.fg) && xterm(c.bg), "{}: {c:?}", f.name);
        }
    }
}

#[test]
fn picture_hashes_at_1x() {
    let mut out = String::new();
    for (set, fixtures) in [("poc", poc()), ("session-b", load("session-b.json"))] {
        for f in fixtures {
            let (w, h) = px(&f, 1);
            let rgba = render_rgba(&f.spec, w, h, &Theme::DARK);
            assert_eq!(rgba.len(), (w * h * 4) as usize);
            writeln!(out, "{set} {} {w}x{h} {}", f.name, sha256(&rgba)).unwrap();
        }
    }
    insta::assert_snapshot!("rgba_sha256_1x", out);
}

#[test]
fn the_widest_chart_at_2x_is_a_png_under_2_mib() {
    for f in every() {
        // 103 × the chart's rows at 2× of 8 × 16 px: the context chart
        // at full pane width is 1648 × 256.
        let wide = Fixture {
            cols: 103,
            ..f.clone()
        };
        let (w, h) = px(&wide, 2);
        let rgba = render_rgba(&wide.spec, w, h, &Theme::DARK);
        let png = encode_png(&rgba, w, h);
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "{}", f.name);
        assert!(png.len() < 2 << 20, "{}: {} bytes", f.name, png.len());
        // And it decodes back to the same pixels.
        let mut reader = png::Decoder::new(std::io::Cursor::new(&png))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (w, h));
        assert_eq!(&buf[..info.buffer_size()], &rgba[..], "{}", f.name);
    }
}

/// Every label field (`name`, `unit`) set to `to`, wherever it is.
fn relabel(v: &mut serde_json::Value, to: &str) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, x) in m.iter_mut() {
                if (k == "name" || k == "unit") && x.is_string() {
                    *x = serde_json::Value::String(to.to_string());
                } else {
                    relabel(x, to);
                }
            }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(|x| relabel(x, to)),
        _ => {}
    }
}

#[test]
fn label_fields_are_never_drawn() {
    for f in every() {
        let (w, h) = px(&f, 1);
        let cells = render_cells(&f.spec, f.cols, f.rows);
        let rgba = render_rgba(&f.spec, w, h, &Theme::DARK);
        for to in [
            "",
            "a much longer label that a renderer drawing text would show",
        ] {
            let mut v = serde_json::to_value(&f.spec).unwrap();
            relabel(&mut v, to);
            let other: ChartSpec = serde_json::from_value(v).unwrap();
            assert_ne!(other, f.spec, "{}: relabelled", f.name);
            assert_eq!(render_cells(&other, f.cols, f.rows), cells, "{}", f.name);
            assert!(
                render_rgba(&other, w, h, &Theme::DARK) == rgba,
                "{}: picture moved with its labels",
                f.name
            );
        }
    }
}

#[test]
fn degenerate_specs_draw_an_empty_or_partial_chart_without_panicking() {
    for f in every() {
        let mut empty = f.spec.clone();
        empty.series.clear();
        empty.markers.clear();
        empty.forecast = None;
        empty.thresholds.clear();
        let mut flat = f.spec.clone();
        flat.x.to = flat.x.from;
        flat.y.to = flat.y.from;
        let mut nan = f.spec.clone();
        nan.y.to = f64::NAN;
        let mut far = f.spec.clone();
        far.x.to = 1e300;
        for spec in [&empty, &flat, &nan, &far] {
            for (cols, rows) in [(1, 1), (3, 1), (f.cols, f.rows)] {
                let cells = render_cells(spec, cols, rows);
                assert_eq!(cells.len(), cols as usize * rows as usize);
                let rgba = render_rgba(spec, cols as u32 * 8, rows as u32 * 16, &Theme::DARK);
                assert_eq!(rgba.len(), cols as usize * rows as usize * 8 * 16 * 4);
            }
        }
        // A non-finite spec draws nothing at all.
        let blank = render_cells(&nan, f.cols, f.rows);
        assert!(blank
            .iter()
            .all(|c| c.fg == Theme::CELLS.bg && c.bg == Theme::CELLS.bg));
        assert!(render_rgba(&nan, 64, 32, &Theme::DARK)
            .iter()
            .all(|b| *b == 0));
        assert!(render_cells(&f.spec, 0, 4).is_empty());
        assert!(render_cells(&f.spec, 4, 0).is_empty());
        assert!(render_rgba(&f.spec, 0, 16, &Theme::DARK).is_empty());
    }
}

#[test]
fn oversized_pictures_and_mismatched_buffers_are_refused() {
    let f = &poc()[0];
    assert!(render_rgba(&f.spec, 4096, (MAX_PIXELS / 4096 + 1) as u32, &Theme::DARK).is_empty());
    assert!(encode_png(&[0; 15], 2, 2).is_empty());
    assert!(encode_png(&[], 0, 0).is_empty());
    assert!(!encode_png(&[0; 16], 2, 2).is_empty());
}

/// The render budget: under 30 ms per chart, picture and PNG together, at
/// 2× and at full pane width. Timing means nothing in a debug build, so
/// this runs with `cargo test -p cctop-charts --release`.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "timing: run with cargo test -p cctop-charts --release"
)]
fn each_chart_renders_in_under_30_ms() {
    for f in every() {
        let wide = Fixture {
            cols: 103,
            ..f.clone()
        };
        let (w, h) = px(&wide, 2);
        let best = (0..5)
            .map(|_| {
                let t = std::time::Instant::now();
                let png = encode_png(&render_rgba(&wide.spec, w, h, &Theme::DARK), w, h);
                let cells = render_cells(&wide.spec, wide.cols, wide.rows);
                assert!(!png.is_empty() && !cells.is_empty());
                t.elapsed()
            })
            .min()
            .unwrap();
        assert!(best.as_millis() < 30, "{}: {best:?}", wide.name);
    }
}
