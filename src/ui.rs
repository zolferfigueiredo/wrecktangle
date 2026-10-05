//! Platform independent pieces of the Settings window: geometry, pixel
//! snapping, scrolling and focus order. `view` builds the pages from these
//! and `palette` holds the colors; `settings` only measures text and draws.

pub mod palette;
pub mod view;

/// A rectangle in device independent pixels (DIPs, 1/96 inch).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn inset(&self, by: f32) -> Rect {
        Rect::new(
            self.x + by,
            self.y + by,
            self.w - 2.0 * by,
            self.h - 2.0 * by,
        )
    }

    pub fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// Rounds DIP values to the physical pixel grid at a given scale (DPI / 96),
/// so edges stay sharp and gaps stay even, as Slint's `phx` unit did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub scale: f32,
}

impl Snap {
    pub fn new(scale: f32) -> Snap {
        Snap {
            scale: if scale > 0.0 { scale } else { 1.0 },
        }
    }

    /// One physical pixel, in DIPs.
    pub fn hairline(&self) -> f32 {
        1.0 / self.scale
    }

    /// The nearest physical pixel boundary.
    pub fn round(&self, dip: f32) -> f32 {
        (dip * self.scale).round() / self.scale
    }

    /// A length rounded to an even number of physical pixels, so content
    /// centered in it lands on whole pixels.
    pub fn even(&self, dip: f32) -> f32 {
        2.0 * (dip * self.scale / 2.0).round() / self.scale
    }

    pub fn rect(&self, r: Rect) -> Rect {
        let x = self.round(r.x);
        let y = self.round(r.y);
        Rect::new(x, y, self.round(r.x + r.w) - x, self.round(r.y + r.h) - y)
    }
}

/// What a layout icon shows inside its frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconGlyph {
    /// The fraction of the work area the action fills.
    Area {
        fx: f32,
        fy: f32,
        fw: f32,
        fh: f32,
    },
    Center,
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconShape {
    Fill(Rect),
    Chevron([(f32, f32); 3]),
}

/// A layout icon in whole physical pixels: a 28x20 DIP frame with the glyph
/// inset from it by the same gap on every side at any scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconGeometry {
    pub w: f32,
    pub h: f32,
    pub stroke: f32,
    pub radius: f32,
    pub glyph_radius: f32,
    pub shape: IconShape,
}

pub fn layout_icon(glyph: IconGlyph, scale: f32) -> IconGeometry {
    let even = |v: f32| 2.0 * (v / 2.0).round();
    let w = even(28.0 * scale);
    let h = even(20.0 * scale);
    let stroke = (1.25 * scale).round().max(1.0);
    let inset = stroke + (1.5 * scale).round().max(1.0);
    let cw = w - 2.0 * inset;
    let ch = h - 2.0 * inset;
    let shape = match glyph {
        IconGlyph::Area { fx, fy, fw, fh } => IconShape::Fill(Rect::new(
            inset + cw * fx,
            inset + ch * fy,
            cw * fw,
            ch * fh,
        )),
        IconGlyph::Center => {
            let cw2 = even(cw / 2.0);
            let ch2 = even(ch / 2.0);
            IconShape::Fill(Rect::new(
                inset + (cw - cw2) / 2.0,
                inset + (ch - ch2) / 2.0,
                cw2,
                ch2,
            ))
        }
        IconGlyph::Next | IconGlyph::Previous => {
            let chevron_w = 2.0 * (ch * 0.18).round();
            let chevron_h = 2.0 * chevron_w;
            let x = inset + (cw - chevron_w) / 2.0;
            let y = inset + (ch - chevron_h) / 2.0;
            let (tip, tail) = if glyph == IconGlyph::Next {
                (x + chevron_w, x)
            } else {
                (x, x + chevron_w)
            };
            IconShape::Chevron([(tail, y), (tip, y + chevron_h / 2.0), (tail, y + chevron_h)])
        }
    };
    IconGeometry {
        w,
        h,
        stroke,
        radius: (4.0 * scale).round(),
        glyph_radius: (1.5 * scale).round().max(1.0),
        shape,
    }
}

const SCROLLBAR_WIDTH: f32 = 14.0;
const SCROLLBAR_END_GAP: f32 = 16.0;
const THUMB_MIN: f32 = 16.0;

/// A vertical scroll position. `offset` runs from 0 to `max()`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Scroll {
    pub offset: f32,
    pub content: f32,
    pub viewport: f32,
}

impl Scroll {
    pub fn max(&self) -> f32 {
        (self.content - self.viewport).max(0.0)
    }

    pub fn scrollable(&self) -> bool {
        self.max() > 0.0
    }

    pub fn clamp(&mut self) {
        self.offset = self.offset.clamp(0.0, self.max());
    }

    /// Scrolls the least distance that shows `top..bottom` (content
    /// coordinates) with `margin` around it.
    pub fn reveal(&mut self, top: f32, bottom: f32, margin: f32) {
        if top - margin < self.offset {
            self.offset = top - margin;
        } else if bottom + margin > self.offset + self.viewport {
            self.offset = bottom + margin - self.viewport;
        }
        self.clamp();
    }

    /// The strip along the right edge of `viewport` that reacts to the mouse.
    pub fn bar(&self, viewport: Rect) -> Option<Rect> {
        self.scrollable().then(|| {
            Rect::new(
                viewport.right() - SCROLLBAR_WIDTH,
                viewport.y,
                SCROLLBAR_WIDTH,
                viewport.h,
            )
        })
    }

    fn track(&self, bar: &Rect) -> f32 {
        (bar.h - 2.0 * SCROLLBAR_END_GAP).max(0.0)
    }

    fn thumb_len(&self, bar: &Rect) -> f32 {
        let proportional = self.track(bar) * self.viewport / self.content.max(1.0);
        proportional.max(THUMB_MIN.min(bar.h))
    }

    /// The thumb, 2 DIPs wide at rest and 6 while the bar is hovered.
    pub fn thumb(&self, viewport: Rect, wide: bool) -> Option<Rect> {
        let bar = self.bar(viewport)?;
        let len = self.thumb_len(&bar);
        let room = (self.track(&bar) - len).max(0.0);
        let width = if wide { 6.0 } else { 2.0 };
        let y = bar.y + SCROLLBAR_END_GAP + room * (self.offset / self.max());
        Some(Rect::new(bar.right() - 4.0 - width, y, width, len))
    }

    /// The offset after dragging the thumb by `dy` from where it was when
    /// the drag started at `start_offset`.
    pub fn drag(&self, viewport: Rect, start_offset: f32, dy: f32) -> f32 {
        let Some(bar) = self.bar(viewport) else {
            return 0.0;
        };
        let room = self.track(&bar) - self.thumb_len(&bar);
        if room <= 0.0 {
            return start_offset;
        }
        (start_offset + dy * self.max() / room).clamp(0.0, self.max())
    }
}

/// The next (or previous) enabled entry after `current`, wrapping around.
/// With nothing focused, Tab starts at the first entry and Shift+Tab at the
/// last.
pub fn focus_step<T: Copy + PartialEq>(
    order: &[(T, bool)],
    current: Option<T>,
    backwards: bool,
) -> Option<T> {
    let enabled: Vec<T> = order
        .iter()
        .filter(|(_, on)| *on)
        .map(|(t, _)| *t)
        .collect();
    if enabled.is_empty() {
        return None;
    }
    let len = enabled.len();
    let position = current.and_then(|c| enabled.iter().position(|t| *t == c));
    let next = match (position, backwards) {
        (None, false) => 0,
        (None, true) => len - 1,
        (Some(i), false) => (i + 1) % len,
        (Some(i), true) => (i + len - 1) % len,
    };
    Some(enabled[next])
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALES: [f32; 6] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.5];

    fn is_whole(v: f32) -> bool {
        (v - v.round()).abs() < 1e-4
    }

    fn fill(geometry: &IconGeometry) -> Rect {
        match geometry.shape {
            IconShape::Fill(r) => r,
            IconShape::Chevron(_) => panic!("expected a fill"),
        }
    }

    #[test]
    fn icon_frames_are_even_and_area_gaps_equal_at_every_scale() {
        for scale in SCALES {
            let full = layout_icon(
                IconGlyph::Area {
                    fx: 0.0,
                    fy: 0.0,
                    fw: 1.0,
                    fh: 1.0,
                },
                scale,
            );
            assert_eq!(full.w % 2.0, 0.0, "scale {scale}");
            assert_eq!(full.h % 2.0, 0.0, "scale {scale}");
            let r = fill(&full);
            let left = r.x;
            let top = r.y;
            let right = full.w - r.right();
            let bottom = full.h - r.bottom();
            assert!(left > full.stroke, "scale {scale}");
            assert_eq!(left, top, "scale {scale}");
            assert_eq!(left, right, "scale {scale}");
            assert_eq!(left, bottom, "scale {scale}");
        }
    }

    #[test]
    fn icon_halves_and_quarters_meet_on_whole_pixels() {
        for scale in SCALES {
            for (fx, fy, fw, fh) in [
                (0.0, 0.0, 0.5, 1.0),
                (0.5, 0.0, 0.5, 1.0),
                (0.0, 0.5, 1.0, 0.5),
                (0.5, 0.5, 0.5, 0.5),
            ] {
                let r = fill(&layout_icon(IconGlyph::Area { fx, fy, fw, fh }, scale));
                for v in [r.x, r.y, r.w, r.h] {
                    assert!(is_whole(v), "scale {scale}: {r:?}");
                }
            }
        }
    }

    #[test]
    fn centered_icon_glyph_is_centered_on_whole_pixels() {
        for scale in SCALES {
            let g = layout_icon(IconGlyph::Center, scale);
            let r = fill(&g);
            assert!(is_whole(r.x) && is_whole(r.y), "scale {scale}");
            assert_eq!(r.x, g.w - r.right(), "scale {scale}");
            assert_eq!(r.y, g.h - r.bottom(), "scale {scale}");
        }
    }

    #[test]
    fn chevrons_point_the_right_way() {
        let IconShape::Chevron(next) = layout_icon(IconGlyph::Next, 1.0).shape else {
            panic!("expected a chevron");
        };
        let IconShape::Chevron(prev) = layout_icon(IconGlyph::Previous, 1.0).shape else {
            panic!("expected a chevron");
        };
        assert!(next[1].0 > next[0].0);
        assert!(prev[1].0 < prev[0].0);
        assert_eq!(next[0].1 + next[2].1, 2.0 * next[1].1);
    }

    #[test]
    fn snap_rounds_to_physical_pixels() {
        let snap = Snap::new(1.5);
        assert_eq!(snap.hairline(), 1.0 / 1.5);
        assert!(is_whole(snap.round(10.3) * 1.5));
        assert_eq!(snap.even(36.0) * 1.5, 54.0);
        assert_eq!(Snap::new(1.25).even(36.0) * 1.25, 46.0);
        assert_eq!(Snap::new(0.0).scale, 1.0);
    }

    fn scroll() -> Scroll {
        Scroll {
            offset: 0.0,
            content: 1000.0,
            viewport: 250.0,
        }
    }

    #[test]
    fn scroll_clamps_and_reveals() {
        let mut s = scroll();
        s.offset = -50.0;
        s.clamp();
        assert_eq!(s.offset, 0.0);
        s.offset = 5000.0;
        s.clamp();
        assert_eq!(s.offset, 750.0);
        s.reveal(100.0, 120.0, 8.0);
        assert_eq!(s.offset, 92.0);
        s.reveal(500.0, 520.0, 8.0);
        assert_eq!(s.offset, 278.0);
        s.reveal(300.0, 320.0, 8.0);
        assert_eq!(s.offset, 278.0);
        let mut short = Scroll {
            offset: 30.0,
            content: 100.0,
            viewport: 250.0,
        };
        short.clamp();
        assert_eq!(short.offset, 0.0);
        assert!(short.bar(Rect::new(0.0, 0.0, 400.0, 250.0)).is_none());
    }

    #[test]
    fn thumb_tracks_offset_and_drag_round_trips() {
        let viewport = Rect::new(0.0, 41.0, 400.0, 250.0);
        let mut s = scroll();
        let top = s.thumb(viewport, false).expect("scrollable");
        assert_eq!(top.y, 41.0 + 16.0);
        assert_eq!(top.right(), 396.0);
        assert_eq!(top.w, 2.0);
        assert_eq!(s.thumb(viewport, true).expect("scrollable").w, 6.0);
        s.offset = s.max();
        let bottom = s.thumb(viewport, false).expect("scrollable");
        assert!((bottom.bottom() - (viewport.bottom() - 16.0)).abs() < 1e-3);

        s.offset = 0.0;
        let dy = bottom.y - top.y;
        assert!((s.drag(viewport, 0.0, dy) - s.max()).abs() < 1e-3);
        assert_eq!(s.drag(viewport, 0.0, -40.0), 0.0);
        let half = s.drag(viewport, 0.0, dy / 2.0);
        assert!((half - s.max() / 2.0).abs() < 1e-3);
    }

    #[test]
    fn focus_step_wraps_and_skips_disabled() {
        let order = [(1, true), (2, false), (3, true), (4, true)];
        assert_eq!(focus_step(&order, None, false), Some(1));
        assert_eq!(focus_step(&order, None, true), Some(4));
        assert_eq!(focus_step(&order, Some(1), false), Some(3));
        assert_eq!(focus_step(&order, Some(4), false), Some(1));
        assert_eq!(focus_step(&order, Some(1), true), Some(4));
        assert_eq!(focus_step(&order, Some(2), false), Some(1));
        assert_eq!(focus_step::<i32>(&[(1, false)], None, false), None);
    }

    #[test]
    fn rect_helpers() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert!(r.contains(10.0, 20.0));
        assert!(!r.contains(40.0, 20.0));
        assert_eq!(r.inset(1.0), Rect::new(11.0, 21.0, 28.0, 38.0));
        assert_eq!(r.offset(1.0, -2.0), Rect::new(11.0, 18.0, 30.0, 40.0));
    }
}
