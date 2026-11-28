//! Context: a filled area over turns that fades towards the axis, a
//! forecast cone past now, the auto-compact line, `now` and compactions.

use super::line::{self, AreaFill};
use crate::canvas::Canvas;
use crate::grid::Grid;
use crate::spec::ChartSpec;
use crate::theme::Theme;

pub(super) fn cells(spec: &ChartSpec, g: &mut Grid, t: &Theme) {
    line::cells(spec, g, t);
}

pub(super) fn picture(spec: &ChartSpec, c: &mut Canvas, t: &Theme) {
    line::picture(spec, c, t, AreaFill::Fade);
}
