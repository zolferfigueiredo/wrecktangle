//! What the Settings window does with the pointer and the keyboard: hover,
//! press, focus, scrolling and the scrollbar drag, turned into commands.
//! Shared by the Windows and Linux windows, which feed it events in DIPs,
//! repaint when `take_dirty` says so and run the commands it returns once
//! they have let go of their own state.

use super::palette::Palette;
use super::view::{self, Interaction, Layout, Model, Op, PAGES, Page, Shaper, Target};
use super::{Rect, Scroll, Snap, focus_step};
use crate::theme::Theme;

/// One wheel line, in DIPs.
pub const LINE_SCROLL: f32 = 16.0;
const REVEAL_MARGIN: f32 = 8.0;

/// What the window asks the controller for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Page(Page),
    Record(usize),
    Clear(usize),
    RestoreDefaults,
    LanguageMenu,
    Language(usize),
    ThemeMenu,
    SetTheme(Theme),
    SetColors(bool),
    ToggleSize(usize),
    SetStartup(bool),
    SetAutoUpdate(bool),
    SetAutoInstall(bool),
    SetBlockSnap(bool),
    CheckNow,
    Update,
    OpenUrl(&'static str),
}

/// The keys the window handles itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Tab,
    Enter,
    Space,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    start_y: f32,
    start_offset: f32,
}

pub struct Session<T> {
    pub model: Model,
    pub ui: Interaction,
    layout: Option<Layout<T>>,
    snap: Snap,
    offsets: [f32; PAGES.len()],
    mouse: Option<(f32, f32)>,
    drag: Option<Drag>,
    dirty: bool,
}

impl<T: Clone> Default for Session<T> {
    fn default() -> Self {
        Session {
            model: Model::default(),
            ui: Interaction::default(),
            layout: None,
            snap: Snap::new(1.0),
            offsets: [0.0; PAGES.len()],
            mouse: None,
            drag: None,
            dirty: false,
        }
    }
}

fn page_index(page: Page) -> usize {
    PAGES.iter().position(|p| *p == page).unwrap_or(0)
}

impl<T: Clone> Session<T> {
    /// Whether something changed that needs a repaint; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Drops the layout after the model, the size, the scale or the
    /// language changed; the next `ensure_layout` builds it again.
    pub fn relayout(&mut self) {
        self.layout = None;
        self.dirty = true;
    }

    pub fn snap(&self) -> Snap {
        self.snap
    }

    /// Lays the page out for a client area of `width` x `height` DIPs, if
    /// it isn't already.
    pub fn ensure_layout<S: Shaper<Text = T>>(
        &mut self,
        shaper: &mut S,
        width: f32,
        height: f32,
        snap: Snap,
    ) {
        if self.layout.is_some() {
            return;
        }
        self.snap = snap;
        let layout = view::layout(&self.model, shaper, width, height, snap);
        if self.ui.focus.is_some_and(|t| !layout.has_target(t)) {
            self.ui.focus = None;
        }
        if self.ui.pressed.is_some_and(|t| !layout.has_target(t)) {
            self.ui.pressed = None;
        }
        self.layout = Some(layout);
        self.set_offset(self.offset());
        self.update_hover();
    }

    pub fn paint(&self, palette: &Palette) -> Option<Vec<Op<T>>> {
        let layout = self.layout.as_ref()?;
        Some(view::paint(
            layout,
            &self.ui,
            &self.scroll(),
            palette,
            self.snap,
        ))
    }

    fn offset(&self) -> f32 {
        self.offsets[page_index(self.model.page)]
    }

    fn scroll(&self) -> Scroll {
        match &self.layout {
            Some(layout) => Scroll {
                offset: self.offset(),
                content: layout.content_height,
                viewport: layout.viewport.h,
            },
            None => Scroll::default(),
        }
    }

    fn viewport(&self) -> Rect {
        self.layout.as_ref().map(|l| l.viewport).unwrap_or_default()
    }

    fn set_offset(&mut self, offset: f32) {
        let mut scroll = self.scroll();
        scroll.offset = offset;
        scroll.clamp();
        if scroll.offset != self.offset() {
            self.offsets[page_index(self.model.page)] = scroll.offset;
            self.dirty = true;
        }
    }

    /// The offset content is painted and hit tested with, on whole pixels.
    fn painted_offset(&self) -> f32 {
        self.snap.round(self.offset())
    }

    fn over_scrollbar(&self, x: f32, y: f32) -> bool {
        self.layout
            .as_ref()
            .and_then(|layout| self.scroll().bar(layout.viewport))
            .is_some_and(|bar| bar.contains(x, y))
    }

    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.over_scrollbar(x, y) {
            return None;
        }
        self.layout.as_ref()?.hit(x, y, self.painted_offset())
    }

    fn update_hover(&mut self) {
        if self.drag.is_some() {
            return;
        }
        let (hover, bar) = match self.mouse {
            Some((x, y)) => (self.target_at(x, y), self.over_scrollbar(x, y)),
            None => (None, false),
        };
        if hover != self.ui.hover || bar != self.ui.bar_hover {
            self.ui.hover = hover;
            self.ui.bar_hover = bar;
            self.dirty = true;
        }
    }

    fn reveal(&mut self, target: Target) {
        let Some(rect) = self.layout.as_ref().and_then(|l| l.content_rect(target)) else {
            return;
        };
        let mut scroll = self.scroll();
        scroll.reveal(rect.y, rect.bottom(), REVEAL_MARGIN);
        self.set_offset(scroll.offset);
    }

    fn command(&self, target: Target) -> Command {
        match target {
            Target::Tab(page) => Command::Page(page),
            Target::Pill(index) => Command::Record(index),
            Target::Clear(index) => Command::Clear(index),
            Target::RestoreDefaults => Command::RestoreDefaults,
            Target::Language => Command::LanguageMenu,
            Target::Theme => Command::ThemeMenu,
            Target::Colors => Command::SetColors(!self.model.wrecktangle_colors),
            Target::Size(index) => Command::ToggleSize(index),
            Target::Startup => Command::SetStartup(!self.model.startup),
            Target::AutoUpdate => Command::SetAutoUpdate(!self.model.auto_update),
            Target::AutoInstall => Command::SetAutoInstall(!self.model.auto_install),
            Target::BlockSnap => Command::SetBlockSnap(!self.model.block_windows_snap),
            Target::CheckNow => Command::CheckNow,
            Target::Update => Command::Update,
            Target::Link(link) => Command::OpenUrl(link.url()),
        }
    }

    /// Changes what the window shows.
    pub fn update(&mut self, change: impl FnOnce(&mut Model)) {
        change(&mut self.model);
        self.relayout();
    }

    /// Switches the page; false if it was already showing.
    pub fn set_page(&mut self, page: Page) -> bool {
        if self.model.page == page {
            return false;
        }
        self.model.page = page;
        if matches!(self.ui.focus, Some(Target::Tab(_))) {
            self.ui.focus = Some(Target::Tab(page));
        }
        self.relayout();
        true
    }

    /// A target's rectangle in window coordinates, as painted.
    pub fn window_rect(&self, target: Target) -> Option<Rect> {
        self.layout
            .as_ref()?
            .window_rect(target, self.painted_offset())
    }

    /// The pointer moved to `x`, `y`; true the first time since it entered,
    /// so the window can ask to hear when it leaves.
    pub fn mouse_move(&mut self, x: f32, y: f32) {
        self.mouse = Some((x, y));
        if let Some(drag) = self.drag {
            let offset = self
                .scroll()
                .drag(self.viewport(), drag.start_offset, y - drag.start_y);
            self.set_offset(offset);
        } else {
            self.update_hover();
        }
    }

    pub fn mouse_leave(&mut self) {
        self.mouse = None;
        self.update_hover();
    }

    /// The left button went down; true if the window should capture the
    /// pointer until it goes up.
    pub fn mouse_down(&mut self, x: f32, y: f32) -> bool {
        self.mouse = Some((x, y));
        self.ui.focus_visible = false;
        self.update_hover();
        self.dirty = true;
        if self.over_scrollbar(x, y) {
            let scroll = self.scroll();
            let viewport = self.viewport();
            let Some(thumb) = scroll.thumb(viewport, true) else {
                return false;
            };
            if y < thumb.y {
                self.set_offset(scroll.offset - viewport.h);
            } else if y >= thumb.bottom() {
                self.set_offset(scroll.offset + viewport.h);
            } else {
                self.drag = Some(Drag {
                    start_y: y,
                    start_offset: scroll.offset,
                });
                self.ui.bar_drag = true;
            }
            return self.drag.is_some();
        }
        let target = self.target_at(x, y);
        self.ui.pressed = target;
        if target.is_some() {
            self.ui.focus = target;
        }
        target.is_some()
    }

    /// The left button went up: whether to release the capture, and the
    /// command of a click (press and release on the same target).
    pub fn mouse_up(&mut self, x: f32, y: f32) -> (bool, Option<Command>) {
        let dragged = self.drag.take().is_some();
        self.ui.bar_drag = false;
        self.mouse = Some((x, y));
        self.update_hover();
        let pressed = self.ui.pressed.take();
        self.dirty = true;
        let clicked = pressed.filter(|t| self.ui.hover == Some(*t));
        (
            dragged || pressed.is_some(),
            clicked.map(|t| self.command(t)),
        )
    }

    /// The window lost the pointer capture it asked for.
    pub fn capture_lost(&mut self) {
        self.drag = None;
        self.ui.bar_drag = false;
        self.ui.pressed = None;
        self.update_hover();
        self.dirty = true;
    }

    /// Scrolls by `notches` wheel notches, each `lines` lines, or a page per
    /// notch when `lines` is None.
    pub fn wheel(&mut self, notches: f32, lines: Option<f32>) {
        let step = match lines {
            Some(lines) => lines * LINE_SCROLL,
            None => self.scroll().viewport,
        };
        self.set_offset(self.offset() - notches * step);
        self.update_hover();
    }

    pub fn wants_hand_cursor(&self, x: f32, y: f32) -> bool {
        self.target_at(x, y).is_some_and(Target::uses_hand_cursor)
    }

    /// The window was deactivated: focus rings hide until keys are used again.
    pub fn deactivated(&mut self) {
        self.ui.focus_visible = false;
        self.dirty = true;
    }

    /// A key went down: whether the window handled it, and what to run.
    /// `repeat` is a key held down; Enter and Space act on the first press
    /// only, so holding them does not repeat and the release of a just
    /// recorded Space or Enter does nothing.
    pub fn key(
        &mut self,
        key: Key,
        ctrl: bool,
        shift: bool,
        repeat: bool,
    ) -> (bool, Option<Command>) {
        match key {
            Key::Tab if ctrl => {
                let i = page_index(self.model.page);
                let n = PAGES.len();
                let page = PAGES[if shift { (i + n - 1) % n } else { (i + 1) % n }];
                (true, Some(Command::Page(page)))
            }
            Key::Tab => {
                let order = self
                    .layout
                    .as_ref()
                    .map(|l| l.focus_order())
                    .unwrap_or_default();
                self.ui.focus = focus_step(&order, self.ui.focus, shift);
                self.ui.focus_visible = true;
                if let Some(target) = self.ui.focus {
                    self.reveal(target);
                }
                self.update_hover();
                self.dirty = true;
                (true, None)
            }
            Key::Enter | Key::Space => {
                if repeat {
                    return (true, None);
                }
                let Some(focus) = self.ui.focus else {
                    return (true, None);
                };
                self.ui.focus_visible = true;
                self.dirty = true;
                (true, Some(self.command(focus)))
            }
            // Up and Down step through a focused box's choices, as in a
            // Windows combo box, without wrapping around.
            Key::Up | Key::Down => {
                let step = |current: usize, count: usize| {
                    if key == Key::Up {
                        current.checked_sub(1)
                    } else {
                        Some(current + 1).filter(|i| *i < count)
                    }
                };
                match self.ui.focus {
                    Some(Target::Language) => (
                        true,
                        step(self.model.language, crate::lang::LANGUAGES.len())
                            .map(Command::Language),
                    ),
                    Some(Target::Theme) => (
                        true,
                        step(self.model.theme.index(), Theme::ALL.len())
                            .map(|i| Command::SetTheme(Theme::ALL[i])),
                    ),
                    _ => (false, None),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::view::{Row, TextStyle, Wrap};

    struct FakeShaper;

    impl Shaper for FakeShaper {
        type Text = String;
        fn shape(
            &mut self,
            text: &str,
            style: TextStyle,
            max_width: f32,
            wrap: Wrap,
        ) -> (String, f32, f32) {
            let w = text.chars().count() as f32 * style.size * 0.5;
            let w = match wrap {
                Wrap::WordsCentered => max_width,
                Wrap::Line => w,
                _ => w.min(max_width),
            };
            (text.to_string(), w, style.size * 1.33)
        }
    }

    fn session() -> Session<String> {
        let mut s = Session::default();
        s.model.rows = vec![
            Row {
                label: "Left".into(),
                ..Row::default()
            };
            13
        ];
        s.model.displays = 2;
        s.ensure_layout(&mut FakeShaper, 640.0, 400.0, Snap::new(1.0));
        s
    }

    fn center(s: &Session<String>, target: Target) -> (f32, f32) {
        let r = s.window_rect(target).expect("target on page");
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    #[test]
    fn a_click_is_a_press_and_release_on_the_same_target() {
        let mut s = session();
        let (x, y) = center(&s, Target::Tab(Page::General));
        assert!(s.mouse_down(x, y));
        assert_eq!(s.mouse_up(x, y), (true, Some(Command::Page(Page::General))));
        // Released somewhere else: no click.
        assert!(s.mouse_down(x, y));
        assert_eq!(s.mouse_up(1.0, 399.0), (true, None));
    }

    #[test]
    fn tab_moves_focus_and_enter_activates_it_once() {
        let mut s = session();
        assert_eq!(s.key(Key::Tab, false, false, false), (true, None));
        assert!(s.ui.focus_visible);
        let focus = s.ui.focus.expect("focus");
        assert_eq!(focus, Target::Tab(Page::Shortcuts));
        assert_eq!(
            s.key(Key::Enter, false, false, false),
            (true, Some(Command::Page(Page::Shortcuts)))
        );
        assert_eq!(s.key(Key::Enter, false, false, true), (true, None));
    }

    #[test]
    fn ctrl_tab_cycles_pages_both_ways() {
        let mut s = session();
        assert_eq!(
            s.key(Key::Tab, true, false, false),
            (true, Some(Command::Page(Page::General)))
        );
        assert_eq!(
            s.key(Key::Tab, true, true, false),
            (true, Some(Command::Page(Page::About)))
        );
    }

    #[test]
    fn arrows_change_the_language_only_when_its_box_has_focus() {
        let mut s = session();
        assert_eq!(s.key(Key::Down, false, false, false), (false, None));
        s.set_page(Page::General);
        s.ensure_layout(&mut FakeShaper, 640.0, 400.0, Snap::new(1.0));
        s.ui.focus = Some(Target::Language);
        s.model.language = 0;
        assert_eq!(
            s.key(Key::Down, false, false, false),
            (true, Some(Command::Language(1)))
        );
        assert_eq!(s.key(Key::Up, false, false, false), (true, None));
    }

    #[test]
    fn arrows_step_through_the_themes_when_their_box_has_focus() {
        let mut s = session();
        s.set_page(Page::General);
        s.ensure_layout(&mut FakeShaper, 640.0, 400.0, Snap::new(1.0));
        s.ui.focus = Some(Target::Theme);
        s.model.theme = Theme::System;
        assert_eq!(
            s.key(Key::Down, false, false, false),
            (true, Some(Command::SetTheme(Theme::Light)))
        );
        assert_eq!(s.key(Key::Up, false, false, false), (true, None));
        s.model.theme = Theme::Dark;
        assert_eq!(s.key(Key::Down, false, false, false), (true, None));
        assert_eq!(
            s.key(Key::Up, false, false, false),
            (true, Some(Command::SetTheme(Theme::Light)))
        );
        assert_eq!(
            s.key(Key::Enter, false, false, false),
            (true, Some(Command::ThemeMenu))
        );
    }

    #[test]
    fn the_wheel_scrolls_within_the_content() {
        let mut s = session();
        s.wheel(-1.0, Some(3.0));
        assert_eq!(s.scroll().offset, 48.0);
        s.wheel(100.0, Some(3.0));
        assert_eq!(s.scroll().offset, 0.0);
        s.wheel(-1.0, None);
        assert_eq!(s.scroll().offset, s.scroll().viewport.min(s.scroll().max()));
    }

    #[test]
    fn hover_marks_the_window_dirty_only_when_it_changes() {
        let mut s = session();
        s.take_dirty();
        let (x, y) = center(&s, Target::Tab(Page::About));
        s.mouse_move(x, y);
        assert!(s.take_dirty());
        assert_eq!(s.ui.hover, Some(Target::Tab(Page::About)));
        assert!(s.wants_hand_cursor(x, y));
        s.mouse_move(x + 1.0, y);
        assert!(!s.take_dirty());
        s.mouse_leave();
        assert!(s.take_dirty());
        assert_eq!(s.ui.hover, None);
    }
}
