//! The sequential series ramp: composition colours derived from a theme's
//! own `accent`, so `ok` / `warn` / `crit` keep meaning threshold state and
//! nothing else (PRD dashboard-v2 §5). Lightness carries the order — how
//! much the person can do about a slice — and the near end is solved
//! against the theme's background until it clears the contrast floor, so a
//! user-authored theme gets a correct ramp for free. A port of
//! `tasks/design-dashboard-v2/series-ramp.mjs`, which generates the table
//! PRD §5.2 quotes; the tests pin the six bundled themes to it.

/// sRGB, 0–255 per channel.
pub type Rgb = (u8, u8, u8);

/// WCAG contrast the near endpoint must clear against the background.
pub const FLOOR: f64 = 3.2;

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn unit(rgb: Rgb) -> [f64; 3] {
    [
        rgb.0 as f64 / 255.0,
        rgb.1 as f64 / 255.0,
        rgb.2 as f64 / 255.0,
    ]
}

/// Quantise the way the generator does (`Math.round` after clamping), so
/// the pinned hexes agree byte for byte.
fn quantise(v: [f64; 3]) -> Rgb {
    let q = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    (q(v[0]), q(v[1]), q(v[2]))
}

/// sRGB → OKLab (Björn Ottosson's matrices).
pub fn rgb_to_oklab(rgb: Rgb) -> [f64; 3] {
    let [r, g, b] = unit(rgb).map(srgb_to_linear);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ]
}

/// OKLab → sRGB, clamped to gamut.
pub fn oklab_to_rgb(lab: [f64; 3]) -> Rgb {
    let [big_l, a, b] = lab;
    let l = (big_l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (big_l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (big_l - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    quantise([
        linear_to_srgb(4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s),
        linear_to_srgb(-1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s),
        linear_to_srgb(-0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s),
    ])
}

/// OKLCH: lightness, chroma, hue (radians).
fn lch(rgb: Rgb) -> (f64, f64, f64) {
    let [l, a, b] = rgb_to_oklab(rgb);
    (l, a.hypot(b), b.atan2(a))
}

fn from_lch(l: f64, c: f64, h: f64) -> Rgb {
    oklab_to_rgb([l, c * h.cos(), c * h.sin()])
}

/// OKLab lightness, 0–1.
pub fn lightness(rgb: Rgb) -> f64 {
    rgb_to_oklab(rgb)[0]
}

fn luminance(rgb: Rgb) -> f64 {
    let [r, g, b] = unit(rgb).map(srgb_to_linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// WCAG contrast ratio, ≥ 1.
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// OKLab distance × 100 (the ΔE the generator reports).
pub fn delta_e(a: Rgb, b: Rgb) -> f64 {
    let (p, q) = (rgb_to_oklab(a), rgb_to_oklab(b));
    100.0 * ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
}

/// `n` steps from `accent`'s hue, dim to bright on a dark background and
/// bright to dim on a light one: step 0 is what the person cannot change,
/// step `n − 1` what they can. The near end sits where it just clears
/// [`FLOOR`] against `bg`.
pub fn ramp(bg: Rgb, accent: Rgb, n: usize) -> Vec<Rgb> {
    let (a_l, a_c, a_h) = lch(accent);
    let bg_l = lch(bg).0;
    let dark = bg_l < 0.5;
    let c_near = a_c * 0.28;
    let (mut lo, mut hi) = if dark { (bg_l, 0.98) } else { (0.0, bg_l) };
    for _ in 0..40 {
        let m = (lo + hi) / 2.0;
        let ok = contrast(from_lch(m, c_near, a_h), bg) >= FLOOR;
        match (dark, ok) {
            (true, true) | (false, false) => hi = m,
            (true, false) | (false, true) => lo = m,
        }
    }
    let near = if dark { hi } else { lo };
    let span = if n <= 3 { 0.30 } else { 0.26 };
    let far = if dark {
        (near + span).max(a_l + 0.14).min(0.94)
    } else {
        (near - span).min(a_l - 0.10).max(0.30)
    };
    (0..n)
        .map(|i| {
            let p = if n > 1 {
                i as f64 / (n - 1) as f64
            } else {
                0.0
            };
            from_lch(near + (far - near) * p, a_c * (0.28 + 0.72 * p), a_h)
        })
        .collect()
}

/// `#RRGGBB`, upper case, as the generator prints.
pub fn hex(rgb: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb.0, rgb.1, rgb.2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(h: &str) -> Rgb {
        let v = u32::from_str_radix(&h[1..], 16).unwrap();
        ((v >> 16) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
    }

    /// The six bundled themes, pinned to `series-ramp.mjs`'s output — the
    /// table in PRD dashboard-v2 §5.2 and every artboard. A property test
    /// alone would let the port drift from all of them.
    #[test]
    fn bundled_themes_match_the_generator() {
        let table = [
            ("#0E1318", "#4CC2C2", ["#516A69", "#69AAAA", "#7EF0F0"]),
            ("#F1F3F6", "#177F86", ["#768B8D", "#316569", "#003F46"]),
            ("#2E3440", "#88C0D0", ["#768488", "#95B7C1", "#B4EEFE"]),
            ("#282828", "#83A598", ["#6F7774", "#92A89F", "#B7DBCD"]),
            ("#1E1E2E", "#89DCEB", ["#5E7275", "#83B4BD", "#A7FBFF"]),
            ("#000000", "#C2A0FF", ["#615A71", "#A591CC", "#EFCDFF"]),
        ];
        for (bg, accent, want) in table {
            let got: Vec<String> = ramp(parse(bg), parse(accent), 3)
                .iter()
                .map(|c| hex(*c))
                .collect();
            assert_eq!(got, want, "{bg} / {accent}");
        }
        // Five steps, default-dark, as the generator prints for panel 1.
        let five: Vec<String> = ramp(parse("#0E1318"), parse("#4CC2C2"), 5)
            .iter()
            .map(|c| hex(*c))
            .collect();
        assert_eq!(
            five,
            ["#516A69", "#5D8989", "#69AAAA", "#74CDCC", "#7EF0F0"]
        );
    }

    /// Monotone lightness in the background's direction and the contrast
    /// floor at every step, for the six bundled themes and two synthetic
    /// extremes (achromatic, both polarities).
    #[test]
    fn monotone_and_above_the_floor() {
        let cases = [
            ("#0E1318", "#4CC2C2"),
            ("#F1F3F6", "#177F86"),
            ("#2E3440", "#88C0D0"),
            ("#282828", "#83A598"),
            ("#1E1E2E", "#89DCEB"),
            ("#000000", "#C2A0FF"),
            ("#000000", "#FFFFFF"),
            ("#FFFFFF", "#000000"),
        ];
        for (bg, accent) in cases {
            let (bg, accent) = (parse(bg), parse(accent));
            let dark = lightness(bg) < 0.5;
            for n in [3, 5] {
                let steps = ramp(bg, accent, n);
                assert_eq!(steps.len(), n);
                for w in steps.windows(2) {
                    let (a, b) = (lightness(w[0]), lightness(w[1]));
                    assert!(if dark { b > a } else { b < a }, "{bg:?} n={n} {a} → {b}");
                }
                for s in &steps {
                    assert!(
                        contrast(*s, bg) >= FLOOR - 0.01,
                        "{bg:?} {} {:.2}",
                        hex(*s),
                        contrast(*s, bg)
                    );
                }
            }
        }
    }

    /// Three steps clear the categorical floor (ΔE ≥ 15) on five bundled
    /// themes; `default-light`'s tighter pair lands at 13.7, where the
    /// alternating glyph makes it legal (PRD §5.2). Five never do — which
    /// is why panel 1 names every slice.
    #[test]
    fn adjacent_distances_as_the_prd_says() {
        let three = ramp(parse("#F1F3F6"), parse("#177F86"), 3);
        assert!((delta_e(three[1], three[2]) - 13.7).abs() < 0.1);
        let dark = ramp(parse("#0E1318"), parse("#4CC2C2"), 3);
        assert!(delta_e(dark[0], dark[1]) > 15.0 && delta_e(dark[1], dark[2]) > 15.0);
        let five = ramp(parse("#0E1318"), parse("#4CC2C2"), 5);
        assert!(five.windows(2).all(|w| delta_e(w[0], w[1]) < 15.0));
        assert_eq!(ramp(parse("#000000"), parse("#4CC2C2"), 1).len(), 1);
    }
}
