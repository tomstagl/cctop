//! Number and time formatting shared by the panels: ≤ 3 significant digits,
//! right-alignable, no surprises.

/// `950` → `950`, `1500` → `1.5k`, `1_620_000` → `1.62M`.
pub fn tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=9_999 => format!("{:.1}k", n as f64 / 1e3),
        10_000..=999_999 => format!("{}k", n / 1_000),
        _ => format!("{:.2}M", n as f64 / 1e6),
    }
}

/// `claude-opus-5` → `opus`, `claude-haiku-4-5-20251001` → `haiku`: the
/// family alone, as Panel 6 and the agents view print it.
pub fn model_family(model: &str) -> String {
    model
        .trim_start_matches("claude-")
        .split('-')
        .next()
        .unwrap_or("")
        .to_string()
}

/// `claude-opus-5` → `opus-5`, `claude-haiku-4-5-20251001` → `haiku-4-5`.
pub fn model_short(model: &str) -> String {
    let m = model.trim_start_matches("claude-");
    // Drop a trailing date stamp.
    match m.rsplit_once('-') {
        Some((head, tail)) if tail.len() == 8 && tail.chars().all(|c| c.is_ascii_digit()) => {
            head.to_string()
        }
        _ => m.to_string(),
    }
}

/// `$4.37`, `$0.004`, `$12.5`.
pub fn usd(v: f64) -> String {
    if v >= 100.0 {
        format!("${v:.0}")
    } else if v >= 10.0 {
        format!("${v:.1}")
    } else if v >= 0.01 {
        format!("${v:.2}")
    } else {
        format!("${v:.3}")
    }
}

/// Milliseconds as `0:48`, `2:31`, `1h 12m`, `4d 03h`.
pub fn duration_ms(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    if s < 3600 {
        format!("{}:{:02}", s / 60, s % 60)
    } else if s < 86_400 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d {:02}h", s / 86_400, (s % 86_400) / 3600)
    }
}

/// Short duration for tables: `38ms`, `1.4s`, `2:05`.
pub fn short_ms(ms: u64) -> String {
    if ms < 1_000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        duration_ms(ms as i64)
    }
}

/// `412 MB`, `41 MB`, `900 kB`.
pub fn bytes(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.1} GB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{} MB", b >> 20)
    } else {
        format!("{} kB", b >> 10)
    }
}

/// `/Users/tom/code/cctop` → `~/code/cctop`.
pub fn shorten_home(path: &std::path::Path) -> String {
    let p = path.to_string_lossy();
    if let Some(home) = std::env::var_os("HOME") {
        let h = home.to_string_lossy();
        if let Some(rest) = p.strip_prefix(h.as_ref()) {
            return format!("~{rest}");
        }
    }
    p.into_owned()
}

/// Epoch ms → `hh:mm` local-agnostic (UTC) clock.
pub fn clock_hhmm(ms: i64) -> String {
    let s = ms.div_euclid(1000);
    let day = s.rem_euclid(86_400);
    format!("{:02}:{:02}", day / 3600, (day % 3600) / 60)
}

/// This machine's UTC offset in seconds (`tm_gmtoff`), for the hour-of-day
/// comparisons against `/insights` figures, which Claude Code keeps in
/// local time.
pub fn local_offset_secs() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as libc::time_t)
        .unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: `localtime_r` writes only into the `tm` we own.
    let ok = unsafe { !libc::localtime_r(&now, &mut tm).is_null() };
    if ok {
        tm.tm_gmtoff
    } else {
        0
    }
}

/// Epoch ms → the local hour of day (0–23).
pub fn local_hour(ms: i64) -> usize {
    let s = ms.div_euclid(1000) + local_offset_secs();
    (s.rem_euclid(86_400) / 3600) as usize
}

/// Epoch ms → `YYYY-MM-DD` (UTC), by the civil-from-days algorithm.
pub fn date_ymd(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Clip to `max` characters with an ellipsis.
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date_ymd(0), "1970-01-01");
        assert_eq!(date_ymd(1_767_225_600_000), "2026-01-01");
        assert_eq!(date_ymd(1_789_428_525_471), "2026-09-14");
        assert_eq!(date_ymd(951_782_400_000), "2000-02-29");
    }

    #[test]
    fn formats() {
        assert_eq!(tokens(950), "950");
        assert_eq!(tokens(1_500), "1.5k");
        assert_eq!(tokens(24_000), "24k");
        assert_eq!(tokens(1_620_000), "1.62M");
        assert_eq!(usd(4.37), "$4.37");
        assert_eq!(usd(12.5), "$12.5");
        assert_eq!(usd(0.004), "$0.004");
        assert_eq!(duration_ms(48_000), "0:48");
        assert_eq!(duration_ms(151_000), "2:31");
        assert_eq!(duration_ms(4_320_000), "1h 12m");
        assert_eq!(duration_ms(356_400_000), "4d 03h");
        assert_eq!(short_ms(38), "38ms");
        assert_eq!(short_ms(1_400), "1.4s");
        assert_eq!(bytes(412 << 20), "412 MB");
        assert_eq!(clock_hhmm(1_787_822_706_911), "09:25");
        assert_eq!(clip("abcdef", 4), "abc…");
    }
}
