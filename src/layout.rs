#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    pub fn close_to(&self, other: &Rect, tolerance: i32) -> bool {
        (self.x - other.x).abs() <= tolerance
            && (self.y - other.y).abs() <= tolerance
            && (self.w - other.w).abs() <= tolerance
            && (self.h - other.h).abs() <= tolerance
    }

    pub fn shrink(&self, by: i32) -> Rect {
        Rect {
            x: self.x + by,
            y: self.y + by,
            w: self.w - 2 * by,
            h: self.h - 2 * by,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Maximize,
    Center,
    NextDisplay,
    PreviousDisplay,
    /// Opens the Windows Project menu, the one Win+P opens.
    DisplayMode,
}

impl Action {
    pub const ALL: [Action; 13] = [
        Action::Left,
        Action::Right,
        Action::Top,
        Action::Bottom,
        Action::TopLeft,
        Action::TopRight,
        Action::BottomLeft,
        Action::BottomRight,
        Action::Maximize,
        Action::Center,
        Action::NextDisplay,
        Action::PreviousDisplay,
        Action::DisplayMode,
    ];

    pub fn key(&self) -> &'static str {
        match self {
            Action::Left => "action.left",
            Action::Right => "action.right",
            Action::Top => "action.top",
            Action::Bottom => "action.bottom",
            Action::TopLeft => "action.top_left",
            Action::TopRight => "action.top_right",
            Action::BottomLeft => "action.bottom_left",
            Action::BottomRight => "action.bottom_right",
            Action::Maximize => "action.maximize",
            Action::Center => "action.center",
            Action::NextDisplay => "action.next_display",
            Action::PreviousDisplay => "action.previous_display",
            Action::DisplayMode => "action.display_mode",
        }
    }
}

fn round_frac(total: i32, fraction: f64) -> i32 {
    (total as f64 * fraction).round() as i32
}

fn anchors(action: Action) -> (bool, bool) {
    match action {
        Action::Left | Action::Top | Action::TopLeft => (false, false),
        Action::Right | Action::TopRight => (true, false),
        Action::Bottom | Action::BottomLeft => (false, true),
        Action::BottomRight => (true, true),
        Action::Maximize
        | Action::Center
        | Action::NextDisplay
        | Action::PreviousDisplay
        | Action::DisplayMode => (false, false),
    }
}

fn position_for(action: Action, w: i32, h: i32, work: Rect) -> Rect {
    let (anchor_right, anchor_bottom) = anchors(action);
    let x = if anchor_right {
        work.right() - w
    } else {
        work.x
    };
    let y = if anchor_bottom {
        work.bottom() - h
    } else {
        work.y
    };
    Rect { x, y, w, h }
}

// Maximize, Center, the display moves and Display mode are handled by the caller,
// not by a fraction of the work area; this returns the work area unchanged
// for them.
pub fn compute(action: Action, fraction: f64, work: Rect) -> Rect {
    match action {
        Action::Left | Action::Right => {
            let w = round_frac(work.w, fraction);
            position_for(action, w, work.h, work)
        }
        Action::Top | Action::Bottom => {
            let h = round_frac(work.h, fraction);
            position_for(action, work.w, h, work)
        }
        Action::TopLeft | Action::TopRight => {
            let w = round_frac(work.w, fraction);
            let h = work.h / 2;
            position_for(action, w, h, work)
        }
        Action::BottomLeft | Action::BottomRight => {
            let w = round_frac(work.w, fraction);
            let h = work.h - work.h / 2;
            position_for(action, w, h, work)
        }
        Action::Maximize
        | Action::Center
        | Action::NextDisplay
        | Action::PreviousDisplay
        | Action::DisplayMode => work,
    }
}

// Repositions using the size the app actually enforced, so the action's
// anchored edge still lines up instead of spilling past the work area.
pub fn reanchor(action: Action, actual_w: i32, actual_h: i32, work: Rect) -> Rect {
    position_for(action, actual_w, actual_h, work)
}

pub fn center(current_w: i32, current_h: i32, work: Rect) -> Rect {
    let w = current_w.min(work.w).max(1);
    let h = current_h.min(work.h).max(1);
    let x = work.x + (work.w - w) / 2;
    let y = work.y + (work.h - h) / 2;
    Rect { x, y, w, h }
}

// Advances and wraps only when the same action fires again on a frame still
// matching the stored one; otherwise a fresh cycle starts at index 0.
pub fn next_cycle_index(
    stored: Option<(Action, usize, Rect)>,
    action: Action,
    current_frame: Rect,
    cycle_len: usize,
    tolerance: i32,
) -> usize {
    if cycle_len == 0 {
        return 0;
    }
    match stored {
        Some((stored_action, index, stored_frame))
            if stored_action == action && stored_frame.close_to(&current_frame, tolerance) =>
        {
            (index + 1) % cycle_len
        }
        _ => 0,
    }
}

// dpi_ratio is target DPI over source DPI. Size is scaled by it and clamped
// to the target work area; the frame's relative center position is kept.
pub fn scale_to_monitor(
    current: Rect,
    source_work: Rect,
    target_work: Rect,
    dpi_ratio: f64,
) -> Rect {
    let scaled_w = ((current.w as f64) * dpi_ratio).round() as i32;
    let scaled_h = ((current.h as f64) * dpi_ratio).round() as i32;
    let w = scaled_w.clamp(1, target_work.w.max(1));
    let h = scaled_h.clamp(1, target_work.h.max(1));

    let (cx, cy) = current.center();
    let rel_x = if source_work.w != 0 {
        (cx - source_work.x) as f64 / source_work.w as f64
    } else {
        0.5
    };
    let rel_y = if source_work.h != 0 {
        (cy - source_work.y) as f64 / source_work.h as f64
    } else {
        0.5
    };

    let target_cx = target_work.x as f64 + rel_x * target_work.w as f64;
    let target_cy = target_work.y as f64 + rel_y * target_work.h as f64;

    let x = (target_cx - w as f64 / 2.0).round() as i32;
    let y = (target_cy - h as f64 / 2.0).round() as i32;

    let x = x.clamp(target_work.x, (target_work.right() - w).max(target_work.x));
    let y = y.clamp(target_work.y, (target_work.bottom() - h).max(target_work.y));

    Rect { x, y, w, h }
}

pub fn wrap_monitor_index(current: usize, count: usize, forward: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if forward {
        (current + 1) % count
    } else {
        (current + count - 1) % count
    }
}

/// The screen edge a panel reserves space at.
#[cfg(any(not(windows), test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Space a panel reserves, as X11's `_NET_WM_STRUT_PARTIAL` describes it:
/// `size` pixels in from `edge` of the whole screen, along the pixels
/// `start..=end` of the other axis.
#[cfg(any(not(windows), test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strut {
    pub edge: Edge,
    pub size: i32,
    pub start: i32,
    pub end: i32,
}

/// Struts from a `_NET_WM_STRUT_PARTIAL` value (left, right, top, bottom,
/// then the start and end of each), or from the older four-value
/// `_NET_WM_STRUT`, which spans whole edges.
#[cfg(any(not(windows), test))]
pub fn struts_from(values: &[u32]) -> Vec<Strut> {
    const EDGES: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
    if values.len() < 4 {
        return Vec::new();
    }
    let value = |i: usize| i32::try_from(values[i]).unwrap_or(i32::MAX);
    EDGES
        .iter()
        .enumerate()
        .filter(|&(i, _)| values[i] > 0)
        .map(|(i, &edge)| {
            let (start, end) = if values.len() >= 12 {
                (value(4 + 2 * i), value(5 + 2 * i))
            } else {
                (0, i32::MAX)
            };
            Strut {
                edge,
                size: value(i),
                start,
                end,
            }
        })
        .collect()
}

/// A monitor's work area: its bounds minus the struts that reach into it.
/// `screen_w` and `screen_h` are the whole X screen's size, which struts
/// are measured from.
#[cfg(any(not(windows), test))]
pub fn work_area(monitor: Rect, screen_w: i32, screen_h: i32, struts: &[Strut]) -> Rect {
    let (mut left, mut top) = (monitor.x, monitor.y);
    let (mut right, mut bottom) = (monitor.right(), monitor.bottom());
    // A panel that sets no range (0 to 0) spans its whole edge.
    let spans = |strut: &Strut, from: i32, to: i32| {
        (strut.start == 0 && strut.end == 0) || (strut.start < to && strut.end >= from)
    };
    for strut in struts {
        match strut.edge {
            Edge::Left if spans(strut, monitor.y, monitor.bottom()) && strut.size > left => {
                left = strut.size;
            }
            Edge::Right
                if spans(strut, monitor.y, monitor.bottom()) && screen_w - strut.size < right =>
            {
                right = screen_w - strut.size;
            }
            Edge::Top if spans(strut, monitor.x, monitor.right()) && strut.size > top => {
                top = strut.size;
            }
            Edge::Bottom
                if spans(strut, monitor.x, monitor.right()) && screen_h - strut.size < bottom =>
            {
                bottom = screen_h - strut.size;
            }
            _ => {}
        }
    }
    if right <= left || bottom <= top {
        return monitor;
    }
    Rect::new(left, top, right - left, bottom - top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_keys_are_unique_catalog_keys() {
        let mut seen = std::collections::HashSet::new();
        for action in Action::ALL {
            assert!(action.key().starts_with("action."));
            assert!(seen.insert(action.key()));
        }
    }

    #[test]
    fn display_mode_follows_the_display_moves() {
        assert_eq!(Action::ALL.last(), Some(&Action::DisplayMode));
        assert_eq!(Action::DisplayMode.key(), "action.display_mode");
    }

    #[test]
    fn display_mode_leaves_the_work_area_alone() {
        assert_eq!(compute(Action::DisplayMode, 0.5, MONITOR_1), MONITOR_1);
    }

    const MONITOR_0: Rect = Rect {
        x: 0,
        y: 0,
        w: 2560,
        h: 1392,
    };
    const MONITOR_1: Rect = Rect {
        x: 2560,
        y: 0,
        w: 2560,
        h: 1392,
    };
    const SIZES: [f64; 4] = [1.0 / 2.0, 2.0 / 3.0, 1.0 / 3.0, 2.0 / 7.0];
    const LEFT_WIDTHS: [i32; 4] = [1280, 1707, 853, 731];
    const TOP_HEIGHTS: [i32; 4] = [696, 928, 464, 398];

    #[test]
    fn left_widths_on_both_monitors() {
        for (work, origin_x) in [(MONITOR_0, 0), (MONITOR_1, 2560)] {
            for (f, expected_w) in SIZES.iter().zip(LEFT_WIDTHS) {
                let r = compute(Action::Left, *f, work);
                assert_eq!(r.w, expected_w, "fraction {f}");
                assert_eq!(r.x, origin_x);
                assert_eq!(r.y, 0);
                assert_eq!(r.h, 1392);
            }
        }
    }

    #[test]
    fn right_widths_and_x_on_both_monitors() {
        let expected_x = [1280, 853, 1707, 1829];
        for (work, origin_x) in [(MONITOR_0, 0), (MONITOR_1, 2560)] {
            for (f, expected_x) in SIZES.iter().zip(expected_x) {
                let r = compute(Action::Right, *f, work);
                assert_eq!(r.x, origin_x + expected_x, "fraction {f}");
                assert_eq!(r.y, 0);
                assert_eq!(r.h, 1392);
                assert_eq!(r.right(), work.right());
            }
        }
    }

    #[test]
    fn top_and_bottom_heights_on_both_monitors() {
        for work in [MONITOR_0, MONITOR_1] {
            for (f, expected_h) in SIZES.iter().zip(TOP_HEIGHTS) {
                let top = compute(Action::Top, *f, work);
                assert_eq!(top.h, expected_h, "fraction {f}");
                assert_eq!(top.y, work.y);
                assert_eq!(top.w, work.w);

                let bottom = compute(Action::Bottom, *f, work);
                assert_eq!(bottom.h, expected_h, "fraction {f}");
                assert_eq!(bottom.y, work.y + work.h - expected_h);
                assert_eq!(bottom.bottom(), work.bottom());
            }
        }
    }

    #[test]
    fn corners_use_half_height() {
        for work in [MONITOR_0, MONITOR_1] {
            for f in SIZES {
                let tl = compute(Action::TopLeft, f, work);
                let tr = compute(Action::TopRight, f, work);
                let bl = compute(Action::BottomLeft, f, work);
                let br = compute(Action::BottomRight, f, work);

                assert_eq!(tl.h, 696);
                assert_eq!(tr.h, 696);
                assert_eq!(tl.y, work.y);
                assert_eq!(tr.y, work.y);
                assert_eq!(tr.right(), work.right());

                assert_eq!(bl.h, work.h - 696);
                assert_eq!(br.h, work.h - 696);
                assert_eq!(bl.y, tl.bottom());
                assert_eq!(br.y, tr.bottom());
                assert_eq!(br.right(), work.right());
            }
        }
    }

    #[test]
    fn shrink_insets_every_side() {
        let frame = Rect::new(2559, -1, 855, 1394);
        assert_eq!(frame.shrink(1), Rect::new(2560, 0, 853, 1392));
        assert_eq!(frame.shrink(0), frame);
    }

    #[test]
    fn left_and_right_tile_with_no_gap() {
        let left = compute(Action::Left, 1.0 / 3.0, MONITOR_0);
        let right = compute(Action::Right, 2.0 / 3.0, MONITOR_0);
        assert_eq!(left.right(), right.x);
        assert_eq!(left.w + right.w, MONITOR_0.w);
    }

    #[test]
    fn odd_height_corners_tile_with_no_gap() {
        let work = Rect::new(0, 0, 2560, 1393);
        let tl = compute(Action::TopLeft, 0.5, work);
        let bl = compute(Action::BottomLeft, 0.5, work);
        assert_eq!(tl.h, 696);
        assert_eq!(bl.h, 697);
        assert_eq!(tl.bottom(), bl.y);
        assert_eq!(tl.h + bl.h, work.h);
    }

    #[test]
    fn reanchor_keeps_right_edge_when_app_enforces_wider_size() {
        let work = MONITOR_0;
        let r = reanchor(Action::Right, 900, 1392, work);
        assert_eq!(r.right(), work.right());
        assert_eq!(r.x, work.right() - 900);
    }

    #[test]
    fn reanchor_keeps_bottom_right_corner() {
        let work = MONITOR_1;
        let r = reanchor(Action::BottomRight, 1000, 500, work);
        assert_eq!(r.right(), work.right());
        assert_eq!(r.bottom(), work.bottom());
    }

    #[test]
    fn center_clamps_to_work_area_and_centers_within_one_pixel() {
        let work = MONITOR_0;
        let r = center(400, 300, work);
        let (cx, cy) = r.center();
        let (wcx, wcy) = work.center();
        assert!((cx - wcx).abs() <= 1);
        assert!((cy - wcy).abs() <= 1);

        let oversized = center(5000, 5000, work);
        assert_eq!(oversized.w, work.w);
        assert_eq!(oversized.h, work.h);
    }

    #[test]
    fn cycle_advances_when_action_and_frame_match() {
        let frame = compute(Action::Left, 0.5, MONITOR_0);
        let stored = Some((Action::Left, 0, frame));
        let idx = next_cycle_index(stored, Action::Left, frame, 4, 2);
        assert_eq!(idx, 1);
    }

    #[test]
    fn cycle_wraps_after_last_index() {
        let frame = compute(Action::Left, 2.0 / 7.0, MONITOR_0);
        let stored = Some((Action::Left, 3, frame));
        let idx = next_cycle_index(stored, Action::Left, frame, 4, 2);
        assert_eq!(idx, 0);
    }

    #[test]
    fn cycle_resets_on_different_action() {
        let frame = compute(Action::Left, 0.5, MONITOR_0);
        let stored = Some((Action::Right, 2, frame));
        let idx = next_cycle_index(stored, Action::Left, frame, 4, 2);
        assert_eq!(idx, 0);
    }

    #[test]
    fn cycle_resets_when_frame_moved_away() {
        let frame = compute(Action::Left, 0.5, MONITOR_0);
        let moved = Rect::new(frame.x + 50, frame.y, frame.w, frame.h);
        let stored = Some((Action::Left, 1, frame));
        let idx = next_cycle_index(stored, Action::Left, moved, 4, 2);
        assert_eq!(idx, 0);
    }

    #[test]
    fn cycle_tolerates_small_drift() {
        let frame = compute(Action::Left, 0.5, MONITOR_0);
        let drifted = Rect::new(frame.x + 1, frame.y - 1, frame.w, frame.h);
        let stored = Some((Action::Left, 1, frame));
        let idx = next_cycle_index(stored, Action::Left, drifted, 4, 2);
        assert_eq!(idx, 2);
    }

    #[test]
    fn cycle_starts_at_zero_with_no_stored_state() {
        let frame = compute(Action::Left, 0.5, MONITOR_0);
        let idx = next_cycle_index(None, Action::Left, frame, 4, 2);
        assert_eq!(idx, 0);
    }

    #[test]
    fn scale_to_monitor_same_dpi_keeps_size_and_relative_center() {
        let current = Rect::new(0, 0, 1280, 1392);
        let mapped = scale_to_monitor(current, MONITOR_0, MONITOR_1, 1.0);
        assert_eq!(mapped.w, 1280);
        assert_eq!(mapped.h, 1392);
        assert_eq!(mapped.x, MONITOR_1.x);
    }

    #[test]
    fn scale_to_monitor_scales_by_dpi_ratio_and_clamps() {
        let current = Rect::new(0, 0, 1280, 1392);
        let small_target = Rect::new(2560, 0, 1920, 1080);
        let mapped = scale_to_monitor(current, MONITOR_0, small_target, 1.5);
        assert_eq!(mapped.w, 1920);
        assert_eq!(mapped.h, 1080);
        assert!(mapped.x >= small_target.x);
        assert!(mapped.right() <= small_target.right());
        assert!(mapped.y >= small_target.y);
        assert!(mapped.bottom() <= small_target.bottom());
    }

    #[test]
    fn scale_to_monitor_preserves_relative_position() {
        let current = Rect::new(1920, 0, 640, 696);
        let mapped = scale_to_monitor(current, MONITOR_0, MONITOR_1, 1.0);
        let (cx, _) = current.center();
        let rel = (cx - MONITOR_0.x) as f64 / MONITOR_0.w as f64;
        let (mapped_cx, _) = mapped.center();
        let mapped_rel = (mapped_cx - MONITOR_1.x) as f64 / MONITOR_1.w as f64;
        assert!((rel - mapped_rel).abs() < 0.01);
    }

    #[test]
    fn wrap_monitor_index_forward_and_backward() {
        assert_eq!(wrap_monitor_index(0, 2, true), 1);
        assert_eq!(wrap_monitor_index(1, 2, true), 0);
        assert_eq!(wrap_monitor_index(0, 2, false), 1);
        assert_eq!(wrap_monitor_index(1, 2, false), 0);
        assert_eq!(wrap_monitor_index(0, 1, true), 0);
    }

    #[test]
    fn struts_read_partial_and_plain_values() {
        assert_eq!(
            struts_from(&[0, 0, 0, 33, 0, 0, 0, 0, 0, 0, 0, 1365]),
            [Strut {
                edge: Edge::Bottom,
                size: 33,
                start: 0,
                end: 1365
            }]
        );
        assert_eq!(
            struts_from(&[40, 0, 0, 0]),
            [Strut {
                edge: Edge::Left,
                size: 40,
                start: 0,
                end: i32::MAX
            }]
        );
        assert!(struts_from(&[0, 0]).is_empty());
    }

    #[test]
    fn a_bottom_panel_shortens_its_monitor() {
        let monitor = Rect::new(0, 0, 1366, 768);
        let struts = struts_from(&[0, 0, 0, 33, 0, 0, 0, 0, 0, 0, 0, 1365]);
        assert_eq!(
            work_area(monitor, 1366, 768, &struts),
            Rect::new(0, 0, 1366, 735)
        );
    }

    #[test]
    fn struts_only_reach_the_monitors_they_span() {
        // Two 1920x1080 screens side by side, a top panel on the right one
        // and a left panel on the left one.
        let left = Rect::new(0, 0, 1920, 1080);
        let right = Rect::new(1920, 0, 1920, 1080);
        let struts = [
            Strut {
                edge: Edge::Top,
                size: 28,
                start: 1920,
                end: 3839,
            },
            Strut {
                edge: Edge::Left,
                size: 48,
                start: 0,
                end: 1079,
            },
        ];
        assert_eq!(
            work_area(left, 3840, 1080, &struts),
            Rect::new(48, 0, 1872, 1080)
        );
        assert_eq!(
            work_area(right, 3840, 1080, &struts),
            Rect::new(1920, 28, 1920, 1052)
        );
    }

    #[test]
    fn a_bottom_panel_on_a_shorter_monitor_measures_from_the_screen_bottom() {
        // A 1080 px screen beside a 768 px one: the screen is 1080 tall, so
        // a panel at the bottom of the small one reserves 312 + 30 px.
        let small = Rect::new(1920, 0, 1366, 768);
        let struts = [Strut {
            edge: Edge::Bottom,
            size: 342,
            start: 1920,
            end: 3285,
        }];
        assert_eq!(
            work_area(small, 3286, 1080, &struts),
            Rect::new(1920, 0, 1366, 738)
        );
    }

    #[test]
    fn a_strut_that_would_leave_nothing_is_ignored() {
        let monitor = Rect::new(0, 0, 800, 600);
        let struts = [Strut {
            edge: Edge::Top,
            size: 700,
            start: 0,
            end: 0,
        }];
        assert_eq!(work_area(monitor, 800, 600, &struts), monitor);
    }
}
