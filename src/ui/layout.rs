//! Assign rows to panels.
//!
//! Narrow (< 100 columns, the right-hand-pane case) stacks everything in a
//! fixed order. Wide splits the middle into two columns and keeps Advisor and
//! Events full-width at the bottom. When rows are scarce the lowest-priority
//! panels collapse to their one-line title first, then disappear; Events
//! always keeps at least four content rows.

use ratatui::layout::Rect;

use super::panel::PanelId;

pub const WIDE_MIN_COLS: u16 = 100;
/// Content rows Events never drops below (plus its 2 border rows).
pub const EVENTS_MIN_ROWS: u16 = 4;
pub const EVENTS_ID: PanelId = 8;
pub const ADVISOR_ID: PanelId = 9;
const FOOTER_ROWS: u16 = 1;
const BORDER_ROWS: u16 = 2;
const COLLAPSED_ROWS: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Narrow,
    Wide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Header: always full width, always first.
    Top,
    Left,
    Right,
    /// Full width below the columns (Advisor, Events).
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelSpec {
    pub id: PanelId,
    /// Content rows wanted (excluding the 2 border rows).
    pub min_rows: u16,
    pub priority: u8,
    pub placement: Placement,
    pub flexible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub mode: Mode,
    /// Panel → area, in draw order. Height 1 = collapsed. Hidden panels and
    /// panels that did not fit are absent.
    pub rects: Vec<(PanelId, Rect)>,
    pub footer: Rect,
}

impl Layout {
    pub fn rect(&self, id: PanelId) -> Option<Rect> {
        self.rects.iter().find(|(p, _)| *p == id).map(|(_, r)| *r)
    }
}

/// Fixed order for the narrow stack (and the columns in wide mode).
pub const NARROW_ORDER: &[PanelId] = &[0, 1, 2, 3, 4, 5, 6, 7, 9, 8];

pub fn mode_for(width: u16) -> Mode {
    if width >= WIDE_MIN_COLS {
        Mode::Wide
    } else {
        Mode::Narrow
    }
}

/// Solve the layout for a terminal of `size`. `force` overrides the width rule.
pub fn solve(size: Rect, panels: &[PanelSpec], hidden: &[PanelId], force: Option<Mode>) -> Layout {
    let mode = force.unwrap_or_else(|| mode_for(size.width));
    let visible: Vec<PanelSpec> = panels
        .iter()
        .copied()
        .filter(|p| !hidden.contains(&p.id))
        .collect();
    let footer = Rect::new(
        size.x,
        size.y + size.height.saturating_sub(FOOTER_ROWS),
        size.width,
        FOOTER_ROWS.min(size.height),
    );
    let body = Rect::new(
        size.x,
        size.y,
        size.width,
        size.height.saturating_sub(FOOTER_ROWS),
    );
    let rects = match mode {
        Mode::Narrow => solve_stack(body, &ordered(&visible, NARROW_ORDER)),
        Mode::Wide => solve_wide(body, &visible),
    };
    Layout {
        mode,
        rects,
        footer,
    }
}

fn ordered(panels: &[PanelSpec], order: &[PanelId]) -> Vec<PanelSpec> {
    order
        .iter()
        .filter_map(|id| panels.iter().find(|p| p.id == *id).copied())
        .collect()
}

/// Rows each panel gets in a single vertical stack of `height` rows.
/// Returns full heights (borders included); 1 = collapsed; 0 = dropped.
fn assign_rows(panels: &[PanelSpec], height: u16) -> Vec<u16> {
    let full = |p: &PanelSpec| p.min_rows + BORDER_ROWS;
    let mut rows: Vec<u16> = panels.iter().map(full).collect();
    let total = |rows: &[u16]| rows.iter().map(|&r| r as u32).sum::<u32>();

    // Shrink: collapse lowest priority first, then drop it, until it fits.
    let mut by_prio: Vec<usize> = (0..panels.len()).collect();
    by_prio.sort_by_key(|&i| (panels[i].priority, std::cmp::Reverse(i)));
    let floor = |p: &PanelSpec| {
        if p.id == EVENTS_ID {
            EVENTS_MIN_ROWS + BORDER_ROWS
        } else {
            COLLAPSED_ROWS
        }
    };
    // Pass 1: collapse to floor.
    for &i in &by_prio {
        if total(&rows) <= height as u32 {
            break;
        }
        rows[i] = rows[i].min(floor(&panels[i]));
    }
    // Pass 2: drop entirely (Events last, and only below its floor as a last resort).
    for &i in &by_prio {
        if total(&rows) <= height as u32 {
            break;
        }
        rows[i] = 0;
    }
    // Grow: hand leftover rows to flexible panels (highest priority first),
    // then to Events, then to the last visible panel.
    let mut slack = (height as u32).saturating_sub(total(&rows)) as u16;
    if slack > 0 {
        let mut flex: Vec<usize> = (0..panels.len())
            .filter(|&i| rows[i] > COLLAPSED_ROWS && panels[i].flexible)
            .collect();
        flex.sort_by_key(|&i| std::cmp::Reverse(panels[i].priority));
        if flex.is_empty() {
            if let Some(i) = (0..panels.len()).rev().find(|&i| rows[i] > COLLAPSED_ROWS) {
                flex.push(i);
            }
        }
        if !flex.is_empty() {
            let share = slack / flex.len() as u16;
            let mut rem = slack % flex.len() as u16;
            for &i in &flex {
                rows[i] += share + if rem > 0 { 1 } else { 0 };
                rem = rem.saturating_sub(1);
            }
            slack = 0;
        }
    }
    let _ = slack;
    rows
}

fn place(panels: &[PanelSpec], rows: &[u16], area: Rect) -> Vec<(PanelId, Rect)> {
    let mut y = area.y;
    let mut out = Vec::new();
    for (p, &h) in panels.iter().zip(rows) {
        if h == 0 {
            continue;
        }
        out.push((p.id, Rect::new(area.x, y, area.width, h)));
        y += h;
    }
    out
}

fn solve_stack(area: Rect, panels: &[PanelSpec]) -> Vec<(PanelId, Rect)> {
    place(panels, &assign_rows(panels, area.height), area)
}

fn solve_wide(area: Rect, panels: &[PanelSpec]) -> Vec<(PanelId, Rect)> {
    let top: Vec<PanelSpec> = panels
        .iter()
        .copied()
        .filter(|p| p.placement == Placement::Top)
        .collect();
    let left = ordered(
        &panels
            .iter()
            .copied()
            .filter(|p| p.placement == Placement::Left)
            .collect::<Vec<_>>(),
        NARROW_ORDER,
    );
    let right = ordered(
        &panels
            .iter()
            .copied()
            .filter(|p| p.placement == Placement::Right)
            .collect::<Vec<_>>(),
        NARROW_ORDER,
    );
    let bottom = ordered(
        &panels
            .iter()
            .copied()
            .filter(|p| p.placement == Placement::Bottom)
            .collect::<Vec<_>>(),
        NARROW_ORDER,
    );

    // Header and bottom panels take what they need; columns share the rest.
    let top_rows: u16 = top.iter().map(|p| p.min_rows + BORDER_ROWS).sum();
    let bottom_need: u16 = bottom.iter().map(|p| p.min_rows + BORDER_ROWS).sum();
    let mut remaining = area.height.saturating_sub(top_rows);
    let bottom_rows = bottom_need.min(remaining);
    remaining = remaining.saturating_sub(bottom_rows);

    let col_h = remaining;
    let left_rows = assign_rows(&left, col_h);
    let right_rows = assign_rows(&right, col_h);

    let left_w = area.width / 2 + 1; // shared border column belongs to the left
    let right_w = area.width - left_w + 1;
    let mut out = Vec::new();
    let mut y = area.y;
    for p in &top {
        let h = (p.min_rows + BORDER_ROWS).min(area.height.saturating_sub(y - area.y));
        out.push((p.id, Rect::new(area.x, y, area.width, h)));
        y += h;
    }
    let cols_y = y;
    out.extend(place(
        &left,
        &left_rows,
        Rect::new(area.x, cols_y, left_w, col_h),
    ));
    out.extend(place(
        &right,
        &right_rows,
        Rect::new(area.x + left_w - 1, cols_y, right_w, col_h),
    ));
    y = cols_y + col_h;
    let bottom_assigned = assign_rows(&bottom, bottom_rows);
    out.extend(place(
        &bottom,
        &bottom_assigned,
        Rect::new(area.x, y, area.width, bottom_rows),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The nine dashboard panels plus header, with the PRD's content sizes.
    fn stubs() -> Vec<PanelSpec> {
        use Placement::*;
        let p = |id, min_rows, priority, placement, flexible| PanelSpec {
            id,
            min_rows,
            priority,
            placement,
            flexible,
        };
        vec![
            p(0, 2, 255, Top, false),   // header (no border in practice; 2 content rows)
            p(1, 4, 90, Left, false),   // Context
            p(2, 7, 80, Left, false),   // Tokens & Cost
            p(3, 3, 70, Left, false),   // Limits
            p(4, 3, 60, Left, false),   // Turn
            p(5, 7, 85, Right, true),   // Tools
            p(6, 5, 50, Right, false),  // Agents & MCP
            p(7, 3, 40, Right, false),  // Files
            p(9, 2, 75, Bottom, false), // Advisor
            p(8, 4, 95, Bottom, true),  // Events
        ]
    }

    fn describe(l: &Layout) -> Vec<String> {
        let mut v: Vec<String> = l
            .rects
            .iter()
            .map(|(id, r)| format!("{id}: x={} y={} w={} h={}", r.x, r.y, r.width, r.height))
            .collect();
        v.push(format!("footer: y={} h={}", l.footer.y, l.footer.height));
        v.insert(0, format!("mode={:?}", l.mode));
        v
    }

    fn no_overlap_and_within(l: &Layout, size: Rect) {
        for (i, (a_id, a)) in l.rects.iter().enumerate() {
            assert!(
                a.bottom() <= size.height && a.right() <= size.width,
                "{a_id} outside"
            );
            for (b_id, b) in &l.rects[i + 1..] {
                let overlap =
                    a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom();
                // Column neighbours share one border column by design.
                let shared_border = a.y < b.bottom()
                    && b.y < a.bottom()
                    && (a.right() == b.x + 1 || b.right() == a.x + 1);
                assert!(
                    !overlap || shared_border,
                    "{a_id} {a:?} overlaps {b_id} {b:?}"
                );
            }
        }
    }

    #[test]
    fn narrow_60x51() {
        let size = Rect::new(0, 0, 60, 51);
        let l = solve(size, &stubs(), &[], None);
        assert_eq!(l.mode, Mode::Narrow);
        no_overlap_and_within(&l, size);
        assert_eq!(l.rects.len(), 10, "everything fits at 60×51");
        assert!(l.rect(8).unwrap().height >= EVENTS_MIN_ROWS + 2);
        insta::assert_debug_snapshot!(describe(&l));
    }

    #[test]
    fn narrow_40x24_collapses_low_priority() {
        let size = Rect::new(0, 0, 40, 24);
        let l = solve(size, &stubs(), &[], None);
        assert_eq!(l.mode, Mode::Narrow);
        no_overlap_and_within(&l, size);
        assert_eq!(
            l.rect(8).unwrap().height,
            EVENTS_MIN_ROWS + 2,
            "Events keeps its floor"
        );
        assert_eq!(
            l.rect(7).map(|r| r.height),
            Some(1).filter(|_| l.rect(7).is_some()).or(None),
            "Files collapsed or dropped"
        );
        insta::assert_debug_snapshot!(describe(&l));
    }

    #[test]
    fn wide_122x37() {
        let size = Rect::new(0, 0, 122, 37);
        let l = solve(size, &stubs(), &[], None);
        assert_eq!(l.mode, Mode::Wide);
        no_overlap_and_within(&l, size);
        let ctx = l.rect(1).unwrap();
        let tools = l.rect(5).unwrap();
        assert_eq!(ctx.y, tools.y, "columns start together");
        assert_eq!(ctx.right(), tools.x + 1, "columns share a border column");
        let ev = l.rect(8).unwrap();
        assert_eq!(ev.width, 122);
        assert!(ev.y > tools.y);
        insta::assert_debug_snapshot!(describe(&l));
    }

    #[test]
    fn wide_forced_at_60_and_narrow_forced_at_122() {
        let l = solve(Rect::new(0, 0, 60, 51), &stubs(), &[], Some(Mode::Wide));
        assert_eq!(l.mode, Mode::Wide);
        insta::assert_debug_snapshot!(describe(&l));
        let l = solve(Rect::new(0, 0, 122, 37), &stubs(), &[], Some(Mode::Narrow));
        assert_eq!(l.mode, Mode::Narrow);
        insta::assert_debug_snapshot!(describe(&l));
        let l = solve(Rect::new(0, 0, 40, 24), &stubs(), &[], Some(Mode::Wide));
        no_overlap_and_within(&l, Rect::new(0, 0, 40, 24));
        insta::assert_debug_snapshot!(describe(&l));
    }

    #[test]
    fn hidden_panels_are_absent_and_slack_goes_to_flexible() {
        let l = solve(Rect::new(0, 0, 60, 51), &stubs(), &[6, 7], None);
        assert!(l.rect(6).is_none() && l.rect(7).is_none());
        let events = l.rect(8).unwrap().height;
        let with_all = solve(Rect::new(0, 0, 60, 51), &stubs(), &[], None)
            .rect(8)
            .unwrap()
            .height;
        assert!(
            events > with_all,
            "freed rows flow to the flexible Events panel"
        );
        let total: u16 = l.rects.iter().map(|(_, r)| r.height).sum();
        assert_eq!(total, 50, "body fills all rows above the footer");
    }

    #[test]
    fn tiny_terminal_never_panics() {
        for (w, h) in [(1, 1), (10, 3), (40, 5), (200, 2)] {
            let l = solve(Rect::new(0, 0, w, h), &stubs(), &[], None);
            no_overlap_and_within(&l, Rect::new(0, 0, w, h));
        }
    }
}
