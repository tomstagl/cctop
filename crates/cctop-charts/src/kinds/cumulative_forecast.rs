//! Spend: a cumulative line over minutes on a light flat fill, a forecast
//! cone past now, budget and limit lines, a dot on each jump.

use super::line::{self, AreaFill};
use crate::canvas::Canvas;
use crate::grid::Grid;
use crate::spec::ChartSpec;
use crate::theme::Theme;

pub(super) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    line::cells(spec, g, t);
}

pub(super) fn picture(spec: &ChartSpec, c: &mut Canvas, t: &Theme) {
    line::picture(spec, c, t, AreaFill::Flat);
}
