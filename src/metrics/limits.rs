//! Rate-limit projection. Limit units are plan-specific and account-wide, so
//! the forecast fits the reported percentage over time rather than tokens.

/// Window of samples the fit uses.
pub const FIT_WINDOW_MS: i64 = 30 * 60 * 1000;
pub const MIN_SAMPLES: usize = 3;

/// Least-squares slope (percent per ms) over samples within the window
/// ending at `now_ms`, and the fitted value at `now_ms`.
pub fn fit(series: &[(i64, f64)], now_ms: i64) -> Option<(f64, f64)> {
    let pts: Vec<(f64, f64)> = series
        .iter()
        .filter(|(t, _)| *t >= now_ms - FIT_WINDOW_MS && *t <= now_ms)
        .map(|(t, p)| ((*t - now_ms) as f64, *p))
        .collect();
    if pts.len() < MIN_SAMPLES {
        return None;
    }
    let n = pts.len() as f64;
    let mx = pts.iter().map(|p| p.0).sum::<f64>() / n;
    let my = pts.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    if sxx == 0.0 {
        return None;
    }
    let sxy: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let slope = sxy / sxx;
    let at_now = my + slope * (0.0 - mx);
    Some((slope, at_now))
}

/// Epoch ms when the fitted line reaches 100 %, if it is rising.
pub fn exhaustion(series: &[(i64, f64)], now_ms: i64) -> Option<i64> {
    let (slope, at_now) = fit(series, now_ms)?;
    if slope <= 0.0 || at_now >= 100.0 {
        return (at_now >= 100.0).then_some(now_ms);
    }
    Some(now_ms + ((100.0 - at_now) / slope) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_series_projects_exactly() {
        // 1 % per minute, starting at 50 % → 100 % in 50 minutes.
        let now = 10_000_000;
        let series: Vec<(i64, f64)> = (0..10)
            .map(|i| (now - (9 - i) * 60_000, 41.0 + i as f64))
            .collect();
        let ex = exhaustion(&series, now).unwrap();
        assert!((ex - (now + 50 * 60_000)).abs() < 1_000, "{ex}");
    }

    #[test]
    fn needs_three_samples_and_a_positive_slope() {
        let now = 1_000_000;
        assert_eq!(exhaustion(&[(now - 1000, 10.0), (now, 11.0)], now), None);
        let flat = [(now - 2000, 10.0), (now - 1000, 10.0), (now, 10.0)];
        assert_eq!(fit(&flat, now).map(|f| f.0), Some(0.0));
        assert_eq!(exhaustion(&flat, now), None);
        let falling = [(now - 2000, 30.0), (now - 1000, 20.0), (now, 10.0)];
        assert_eq!(exhaustion(&falling, now), None);
        // Samples older than the window are ignored.
        let old = [
            (now - 3 * FIT_WINDOW_MS, 0.0),
            (now - 2 * FIT_WINDOW_MS, 50.0),
            (now - 1000, 60.0),
            (now, 61.0),
        ];
        assert_eq!(fit(&old, now), None);
    }
}
