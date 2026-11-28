//! The chart spec: what a chart shows, as data. Series, markers, a
//! forecast and threshold lines, each with a colour *role* the renderer
//! resolves against its theme — never geometry, never a colour, never text
//! to draw (names here are for the legend and the readouts beside the
//! chart; a renderer ignores them).
//!
//! Self-contained on purpose (serde only, nothing else from the crate):
//! the chart renderer crate takes this module over as it is.

use serde::{Deserialize, Serialize};

/// The chart kinds, one renderer each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Context: a filled area over turns, a forecast cone, threshold lines.
    AreaForecast,
    /// Spend: a cumulative line over minutes, a forecast cone.
    CumulativeForecast,
    /// Cost per turn: one bar per turn.
    Bars,
    /// The cache: one cell per turn, coloured by role.
    Pills,
    /// Tool calls: one row per series, one cell per turn, brighter = more.
    Heat,
    /// The window's composition: one segment per series, in order.
    StackedBar,
    /// The cache countdown: one value of a whole.
    Progress,
}

/// What a mark is drawn with; every renderer maps it to its theme's colour
/// (the same roles as the TUI's: `src/theme.rs`, `src/series.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Fg,
    Dim,
    Accent,
    Ok,
    Warn,
    Crit,
    /// The series ramp's steps: what cannot change this session, what the
    /// next boundary drops, what the person can move.
    S0,
    S1,
    S2,
}

/// A data domain. `from`/`to` are in the axis unit, not in pixels or cells.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Axis {
    pub from: f64,
    pub to: f64,
    /// `turn`, `min`, `tokens`, `USD`, `calls`, `ms`.
    pub unit: String,
}

/// One data point; `role` overrides the series' role for this point (a bar
/// that was a cache rebuild, a pill that missed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Series {
    /// For the legend and the readouts; never drawn.
    pub name: String,
    pub role: Role,
    pub points: Vec<Point>,
}

/// A point of interest: `now`, a compaction, a jump in spend, a failed call
/// (`row` names the heat map's series), a thinking drop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Marker {
    /// `now`, `compaction`, `jump`, `failed`, `thinking_dropped`, `over_200k`.
    pub kind: String,
    pub x: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<usize>,
    pub role: Role,
}

/// A straight forecast from `from` to `to`, with a cone between `low` and
/// `high` at `to.x`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Forecast {
    pub from: Point,
    pub to: Point,
    pub low: f64,
    pub high: f64,
    pub role: Role,
}

/// A horizontal line at `y`: auto-compact, a budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Threshold {
    /// For the legend; never drawn.
    pub name: String,
    pub y: f64,
    pub role: Role,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChartSpec {
    pub kind: Kind,
    pub x: Axis,
    pub y: Axis,
    pub series: Vec<Series>,
    pub markers: Vec<Marker>,
    pub forecast: Option<Forecast>,
    pub thresholds: Vec<Threshold>,
    /// Every role the spec draws with, in first-use order (series, points,
    /// markers, forecast, thresholds): what a renderer resolves once.
    pub roles: Vec<Role>,
}

impl ChartSpec {
    /// A spec of `kind` over the two domains, with nothing in it yet.
    pub fn new(kind: Kind, x: Axis, y: Axis) -> ChartSpec {
        ChartSpec {
            kind,
            x,
            y,
            series: Vec::new(),
            markers: Vec::new(),
            forecast: None,
            thresholds: Vec::new(),
            roles: Vec::new(),
        }
    }

    /// Fill `roles` from what the spec holds.
    pub fn finish(mut self) -> ChartSpec {
        let mut roles: Vec<Role> = Vec::new();
        let mut add = |r: Role| {
            if !roles.contains(&r) {
                roles.push(r);
            }
        };
        for s in &self.series {
            add(s.role);
            for p in &s.points {
                if let Some(r) = p.role {
                    add(r);
                }
            }
        }
        for m in &self.markers {
            add(m.role);
        }
        if let Some(f) = &self.forecast {
            add(f.role);
        }
        for t in &self.thresholds {
            add(t.role);
        }
        self.roles = roles;
        self
    }

    /// Every number in the spec is finite (no NaN, no infinity).
    pub fn is_finite(&self) -> bool {
        let p = |p: &Point| p.x.is_finite() && p.y.is_finite();
        [self.x.from, self.x.to, self.y.from, self.y.to]
            .iter()
            .all(|v| v.is_finite())
            && self.series.iter().all(|s| s.points.iter().all(p))
            && self
                .markers
                .iter()
                .all(|m| m.x.is_finite() && m.y.is_none_or(f64::is_finite))
            && self
                .forecast
                .as_ref()
                .is_none_or(|f| p(&f.from) && p(&f.to) && f.low.is_finite() && f.high.is_finite())
            && self.thresholds.iter().all(|t| t.y.is_finite())
    }
}

/// sRGB, 0–255 per channel.
pub type Rgb = (u8, u8, u8);

/// One terminal cell of a chart drawn in half-blocks. A colour of `None` is
/// the terminal's default (nothing painted there), so an empty part of a
/// chart shows whatever the surface sits on. By what the two halves hold:
///
/// | upper   | lower   | `ch` | `fg`  | `bg`  |
/// |---------|---------|------|-------|-------|
/// | painted | painted | `▀`  | upper | lower |
/// | painted | empty   | `▀`  | upper | None  |
/// | empty   | painted | `▄`  | lower | None  |
/// | empty   | empty   | ` `  | None  | None  |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub ch: char,
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
}
