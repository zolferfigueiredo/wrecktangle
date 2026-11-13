//! The Settings window on X11: an ordinary top-level window the window
//! manager decorates, drawn by `gfx` into a pixmap and put on screen with
//! PutImage, plus the language and theme lists as a popup. Input goes
//! through the shared `ui::session`; commands run after the window state is
//! let go.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use tiny_skia::Pixmap;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::properties::{WmSizeHints, WmSizeHintsSpecification};
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ButtonPressEvent, ChangeWindowAttributesAux, ClientMessageEvent, ConfigureWindowAux,
    ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, GrabMode, GrabStatus, ImageFormat,
    ImageOrder, NotifyMode, PropMode, StackMode, Window as XWindow, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_FROM_PARENT, CURRENT_TIME, NONE};

use super::gfx::{Gfx, Text};
use crate::lang;
use crate::linux::{app, desktop, tray, window_ops, x11};
use crate::theme::{self, Mode, Theme};
use crate::ui::palette::{Accent, Palette, palette};
use crate::ui::session::{Command, Key, Session};
use crate::ui::view::{Model, Op, Page, Shaper, Target, TextStyle, Weight, Wrap};
use crate::ui::{Rect, Snap};

const CLIENT_W: f32 = 640.0;
const CLIENT_H: f32 = 784.0;
const MIN_W: f32 = 480.0;
const MIN_H: f32 = 320.0;
// Room left for the window manager's title bar and the panel.
const SCREEN_MARGIN: i32 = 48;
const WHEEL_LINES: f32 = 3.0;

const POPUP_ROW: f32 = 28.0;
const POPUP_PAD: f32 = 4.0;
const POPUP_TEXT: TextStyle = TextStyle {
    size: 13.0,
    weight: Weight::Regular,
};

const KEY_TAB: u32 = 0xFF09;
const KEY_RETURN: u32 = 0xFF0D;
const KEY_KP_ENTER: u32 = 0xFF8D;
const KEY_SPACE: u32 = 0x0020;
const KEY_UP: u32 = 0xFF52;
const KEY_DOWN: u32 = 0xFF54;
const KEY_ESCAPE: u32 = 0xFF1B;

/// Which list the popup shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Language,
    Theme,
}

impl Choice {
    fn target(self) -> Target {
        match self {
            Choice::Language => Target::Language,
            Choice::Theme => Target::Theme,
        }
    }

    fn names(self) -> Vec<String> {
        match self {
            Choice::Language => lang::LANGUAGES
                .iter()
                .map(|language| language.name.to_string())
                .collect(),
            Choice::Theme => Theme::ALL
                .iter()
                .map(|theme| lang::t(theme.label_key()))
                .collect(),
        }
    }

    fn command(self, index: usize) -> Command {
        match self {
            Choice::Language => Command::Language(index),
            Choice::Theme => Command::SetTheme(Theme::ALL[index]),
        }
    }
}

struct Popup {
    id: XWindow,
    choice: Choice,
    pixmap: Pixmap,
    names: Vec<(Text, f32)>,
    current: usize,
    hover: Option<usize>,
    width: f32,
}

struct Settings {
    id: XWindow,
    gc: u32,
    depth: u8,
    gfx: Gfx,
    pixmap: Pixmap,
    image: Vec<u8>,
    session: Session<Text>,
    scale: f32,
    mode: Mode,
    accent: Option<Accent>,
    arrow: u32,
    hand: u32,
    hand_shown: bool,
    held: Option<u8>,
    popup: Option<Popup>,
    keyboard_grabbed: bool,
}

thread_local! {
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// The open window, for other threads to send a message to; 0 when closed.
static WINDOW_ID: AtomicU32 = AtomicU32::new(0);

fn with<R>(f: impl FnOnce(&mut Settings) -> R) -> Option<R> {
    SETTINGS.with(|cell| {
        let mut state = cell.try_borrow_mut().ok()?;
        state.as_mut().map(f)
    })
}

/// Repaints if anything changed. The app calls it once after each event,
/// so the many model updates one event can cause make one frame.
pub fn paint_if_dirty() {
    with(|s| {
        if s.session.take_dirty() {
            s.paint();
        }
    });
}

pub fn exists() -> bool {
    SETTINGS.with(|cell| cell.borrow().is_some())
}

/// The desktop's DPI from `Xft.dpi`, as 1.0 at 96 DPI.
fn desktop_scale() -> f32 {
    let conn = &x11::shared().conn;
    x11rb::resource_manager::new_from_default(conn)
        .ok()
        .and_then(|db| db.get_value::<u32>("Xft.dpi", "").ok().flatten())
        .map_or(1.0, |dpi| (dpi as f32 / 96.0).clamp(1.0, 4.0))
}

/// The family of the desktop's UI font, as Xfce stores it ("Ubuntu 10").
fn desktop_font() -> Option<String> {
    let output = std::process::Command::new("xfconf-query")
        .args(["-c", "xsettings", "-p", "/Gtk/FontName"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // The size comes last; style words such as "Bold" don't name a family.
    let family = name
        .rsplit_once(' ')
        .filter(|(_, size)| size.parse::<f32>().is_ok())
        .map_or(name.as_str(), |(family, _)| family);
    (!family.is_empty()).then(|| family.to_string())
}

fn background(mode: Mode) -> u32 {
    let c = palette(mode, false, None).background;
    (u32::from(c.r) << 16) | (u32::from(c.g) << 8) | u32::from(c.b)
}

/// Builds the window, centered on the work area of the screen under the
/// pointer, which is where the tray icon was just clicked.
pub fn create() -> bool {
    let shared = x11::shared();
    let conn = &shared.conn;
    let atoms = &shared.atoms;
    let screen = &conn.setup().roots[shared.screen];
    let scale = desktop_scale();
    let mode = theme::current_mode(app::current_config().theme);
    let accent = desktop::accent();

    let pointer = conn
        .query_pointer(shared.root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map_or((0, 0), |p| (i32::from(p.root_x), i32::from(p.root_y)));
    let monitors = window_ops::monitors();
    let work = monitors
        .iter()
        .find(|m| {
            let b = m.bounds;
            pointer.0 >= b.x && pointer.0 < b.right() && pointer.1 >= b.y && pointer.1 < b.bottom()
        })
        .or(monitors.first())
        .map_or(crate::layout::Rect::new(0, 0, 1280, 800), |m| m.work);
    let w = ((CLIENT_W * scale).round() as i32)
        .min(work.w - SCREEN_MARGIN)
        .max(1);
    let h = ((CLIENT_H * scale).round() as i32)
        .min(work.h - SCREEN_MARGIN)
        .max(1);
    let x = work.x + (work.w - w) / 2;
    let y = work.y + (work.h - h - SCREEN_MARGIN / 2) / 2;

    let (Ok(id), Ok(gc)) = (conn.generate_id(), conn.generate_id()) else {
        return false;
    };
    let mask = EventMask::EXPOSURE
        | EventMask::STRUCTURE_NOTIFY
        | EventMask::KEY_PRESS
        | EventMask::KEY_RELEASE
        | EventMask::BUTTON_PRESS
        | EventMask::BUTTON_RELEASE
        | EventMask::POINTER_MOTION
        | EventMask::LEAVE_WINDOW
        | EventMask::FOCUS_CHANGE;
    let created = conn
        .create_window(
            COPY_FROM_PARENT as u8,
            id,
            shared.root,
            x as i16,
            y as i16,
            w as u16,
            h as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            COPY_FROM_PARENT,
            &CreateWindowAux::new()
                .background_pixel(background(mode))
                .event_mask(mask),
        )
        .map(|cookie| cookie.check());
    if !matches!(created, Ok(Ok(()))) {
        return false;
    }
    let _ = conn.create_gc(gc, id, &CreateGCAux::new());

    let title = lang::t("window.title");
    let _ = conn.change_property8(
        PropMode::REPLACE,
        id,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        title.as_bytes(),
    );
    let _ = conn.change_property8(
        PropMode::REPLACE,
        id,
        atoms._NET_WM_NAME,
        atoms.UTF8_STRING,
        title.as_bytes(),
    );
    let _ = conn.change_property8(
        PropMode::REPLACE,
        id,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        b"wrecktangle\0Wrecktangle\0",
    );
    let _ = conn.change_property32(
        PropMode::REPLACE,
        id,
        atoms.WM_PROTOCOLS,
        AtomEnum::ATOM,
        &[atoms.WM_DELETE_WINDOW],
    );
    let _ = conn.change_property32(
        PropMode::REPLACE,
        id,
        atoms._NET_WM_WINDOW_TYPE,
        AtomEnum::ATOM,
        &[atoms._NET_WM_WINDOW_TYPE_NORMAL],
    );
    let _ = conn.change_property32(
        PropMode::REPLACE,
        id,
        atoms._NET_WM_ICON,
        AtomEnum::CARDINAL,
        &tray::window_icon(),
    );
    let mut hints = WmSizeHints::new();
    hints.position = Some((WmSizeHintsSpecification::UserSpecified, x, y));
    hints.size = Some((WmSizeHintsSpecification::UserSpecified, w, h));
    hints.min_size = Some((
        (MIN_W * scale).round() as i32,
        (MIN_H * scale).round() as i32,
    ));
    let _ = hints.set_normal_hints(conn, id);

    let cursors = x11rb::resource_manager::new_from_default(conn)
        .ok()
        .and_then(|db| {
            let handle = x11rb::cursor::Handle::new(conn, shared.screen, &db)
                .ok()?
                .reply()
                .ok()?;
            Some((
                handle.load_cursor(conn, "left_ptr").ok()?,
                handle.load_cursor(conn, "hand2").ok()?,
            ))
        })
        .unwrap_or((NONE, NONE));

    let Some(pixmap) = Pixmap::new(w as u32, h as u32) else {
        let _ = conn.destroy_window(id);
        return false;
    };
    let settings = Settings {
        id,
        gc,
        depth: screen.root_depth,
        gfx: Gfx::new(scale, desktop_font()),
        pixmap,
        image: Vec::new(),
        session: Session::default(),
        scale,
        mode,
        accent,
        arrow: cursors.0,
        hand: cursors.1,
        hand_shown: false,
        held: None,
        popup: None,
        keyboard_grabbed: false,
    };
    SETTINGS.with(|cell| *cell.borrow_mut() = Some(settings));
    WINDOW_ID.store(id, Ordering::Relaxed);
    let _ = conn.map_window(id);
    let _ = conn.flush();
    true
}

/// Brings the window to the front and gives it the keyboard.
pub fn show() {
    let Some(id) = with(|s| s.id) else {
        return;
    };
    let shared = x11::shared();
    let conn = &shared.conn;
    let _ = conn.configure_window(id, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE));
    let event = ClientMessageEvent::new(
        32,
        id,
        shared.atoms._NET_ACTIVE_WINDOW,
        [2, CURRENT_TIME, 0, 0, 0],
    );
    let _ = conn.send_event(
        false,
        shared.root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    );
    let _ = conn.flush();
}

/// Changes what the window shows; it repaints on its own.
pub fn update(change: impl FnOnce(&mut Model)) {
    with(|s| s.session.update(change));
}

/// Switches the page; false if it was already showing.
pub fn set_page(page: Page) -> bool {
    with(|s| s.session.set_page(page)).unwrap_or(false)
}

pub fn language_changed() {
    let id = with(|s| {
        s.gfx.language_changed();
        s.session.relayout();
        s.id
    });
    if let Some(id) = id {
        let atoms = &x11::shared().atoms;
        let conn = &x11::shared().conn;
        let title = lang::t("window.title");
        let _ = conn.change_property8(
            PropMode::REPLACE,
            id,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            title.as_bytes(),
        );
        let _ = conn.change_property8(
            PropMode::REPLACE,
            id,
            atoms._NET_WM_NAME,
            atoms.UTF8_STRING,
            title.as_bytes(),
        );
        let _ = conn.flush();
    }
}

/// The recorder takes every key while it runs, so the desktop's own
/// shortcuts don't fire while one is being recorded.
pub fn grab_keyboard() {
    with(|s| {
        let conn = &x11::shared().conn;
        s.keyboard_grabbed = conn
            .grab_keyboard(false, s.id, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.status == GrabStatus::SUCCESS);
    });
}

pub fn ungrab_keyboard() {
    with(|s| {
        if std::mem::take(&mut s.keyboard_grabbed) {
            let conn = &x11::shared().conn;
            let _ = conn.ungrab_keyboard(CURRENT_TIME);
            let _ = conn.flush();
        }
    });
}

pub fn destroy() {
    WINDOW_ID.store(0, Ordering::Relaxed);
    let state = SETTINGS.with(|cell| cell.borrow_mut().take());
    if let Some(state) = state {
        let conn = &x11::shared().conn;
        if let Some(popup) = &state.popup {
            let _ = conn.destroy_window(popup.id);
        }
        if state.keyboard_grabbed {
            let _ = conn.ungrab_keyboard(CURRENT_TIME);
        }
        let _ = conn.free_gc(state.gc);
        let _ = conn.destroy_window(state.id);
        let _ = conn.flush();
    }
}

/// Opens the language list under the language box.
pub fn open_language_popup(current: usize) {
    with(|s| s.open_popup(Choice::Language, current));
}

/// Opens the theme list under the theme box.
pub fn open_theme_popup(current: Theme) {
    with(|s| s.open_popup(Choice::Theme, current.index()));
}

/// Takes the light or dark mode and the accent again, after the Theme
/// setting or the desktop's theme changed.
pub fn retheme() {
    if !exists() {
        return;
    }
    let mode = theme::current_mode(app::current_config().theme);
    let accent = desktop::accent();
    with(|s| s.set_look(mode, accent));
}

/// Asks an open window to `retheme` on the main thread. Safe from any
/// thread; a no-op while the window is closed.
pub fn post_retheme() {
    let id = WINDOW_ID.load(Ordering::Relaxed);
    if id == 0 {
        return;
    }
    let shared = x11::shared();
    let event = ClientMessageEvent::new(32, id, shared.atoms.WRECKTANGLE_MESSAGE, [0; 5]);
    let _ = shared
        .conn
        .send_event(false, id, EventMask::NO_EVENT, event);
    let _ = shared.conn.flush();
}

/// What an event asked for, run once the window state is let go.
enum After {
    Nothing,
    Run(Command),
    RecorderKey(u8, u16),
    Retheme,
    Close,
}

/// Handles an event for the Settings window or its popup; false when it
/// belongs to something else.
pub fn handle_event(event: &Event) -> bool {
    let Some(ours) = with(|s| s.owns(event)) else {
        return false;
    };
    if !ours {
        return false;
    }
    let after = with(|s| s.handle(event)).unwrap_or(After::Nothing);
    match after {
        After::Nothing => {}
        After::Run(command) => super::run(command),
        After::RecorderKey(keycode, state) => super::on_recorder_key(keycode, state),
        After::Retheme => retheme(),
        After::Close => super::on_close(),
    }
    true
}

impl Settings {
    fn owns(&self, event: &Event) -> bool {
        let popup = self.popup.as_ref().map(|p| p.id);
        let is_ours = |w: XWindow| w == self.id || Some(w) == popup;
        match event {
            Event::Expose(e) => is_ours(e.window),
            Event::ConfigureNotify(e) => e.window == self.id,
            Event::ClientMessage(e) => e.window == self.id,
            Event::MotionNotify(e) => is_ours(e.event) || popup.is_some(),
            Event::ButtonPress(e) | Event::ButtonRelease(e) => is_ours(e.event) || popup.is_some(),
            Event::KeyPress(e) | Event::KeyRelease(e) => {
                is_ours(e.event) || popup.is_some() || self.keyboard_grabbed
            }
            Event::LeaveNotify(e) => e.event == self.id,
            Event::FocusIn(e) | Event::FocusOut(e) => e.event == self.id,
            _ => false,
        }
    }

    fn handle(&mut self, event: &Event) -> After {
        if self.popup.is_some()
            && let Some(after) = self.handle_popup(event)
        {
            return after;
        }
        match event {
            Event::Expose(e) if e.count == 0 => {
                if self.popup.as_ref().is_some_and(|p| p.id == e.window) {
                    self.paint_popup();
                } else {
                    self.paint();
                }
            }
            Event::ConfigureNotify(e) => {
                let (w, h) = (u32::from(e.width), u32::from(e.height));
                if (w, h) != (self.pixmap.width(), self.pixmap.height())
                    && let Some(pixmap) = Pixmap::new(w.max(1), h.max(1))
                {
                    self.pixmap = pixmap;
                    self.session.relayout();
                }
            }
            Event::ClientMessage(e) => {
                let atoms = &x11::shared().atoms;
                if e.type_ == atoms.WM_PROTOCOLS && e.data.as_data32()[0] == atoms.WM_DELETE_WINDOW
                {
                    return After::Close;
                }
                // Only `post_retheme` sends this to the Settings window.
                if e.type_ == atoms.WRECKTANGLE_MESSAGE {
                    return After::Retheme;
                }
            }
            Event::MotionNotify(e) => {
                let (x, y) = self.dip(e.event_x, e.event_y);
                self.ensure_layout();
                self.session.mouse_move(x, y);
                self.update_cursor(x, y);
            }
            Event::LeaveNotify(_) => self.session.mouse_leave(),
            Event::ButtonPress(e) => return self.button(e, true),
            Event::ButtonRelease(e) => return self.button(e, false),
            Event::KeyPress(e) => {
                if self.keyboard_grabbed {
                    return After::RecorderKey(e.detail, u16::from(e.state));
                }
                let repeat = self.held == Some(e.detail);
                self.held = Some(e.detail);
                let info = app::key_info(e.detail, u16::from(e.state));
                let key = match info.keysym {
                    KEY_TAB => Key::Tab,
                    KEY_RETURN | KEY_KP_ENTER => Key::Enter,
                    KEY_SPACE => Key::Space,
                    KEY_UP => Key::Up,
                    KEY_DOWN => Key::Down,
                    _ => return After::Nothing,
                };
                self.ensure_layout();
                let (_, command) = self.session.key(key, info.ctrl, info.shift, repeat);
                if let Some(command) = command {
                    return After::Run(command);
                }
            }
            Event::KeyRelease(e) => {
                if self.held == Some(e.detail) {
                    self.held = None;
                }
            }
            // A keyboard grab also moves the focus; only real focus changes count.
            Event::FocusOut(e) if e.mode == NotifyMode::NORMAL => self.session.deactivated(),
            _ => {}
        }
        After::Nothing
    }

    fn button(&mut self, e: &ButtonPressEvent, down: bool) -> After {
        let (x, y) = self.dip(e.event_x, e.event_y);
        self.ensure_layout();
        match (e.detail, down) {
            (1, true) => {
                // X11 keeps the pointer on this window until the button goes
                // up, so there is no capture to ask for.
                let _ = self.session.mouse_down(x, y);
            }
            (1, false) => {
                let (_, command) = self.session.mouse_up(x, y);
                if let Some(command) = command {
                    return After::Run(command);
                }
            }
            (4, true) => self.session.wheel(1.0, Some(WHEEL_LINES)),
            (5, true) => self.session.wheel(-1.0, Some(WHEEL_LINES)),
            _ => {}
        }
        After::Nothing
    }

    fn dip(&self, x: i16, y: i16) -> (f32, f32) {
        (f32::from(x) / self.scale, f32::from(y) / self.scale)
    }

    fn snap(&self) -> Snap {
        Snap::new(self.scale)
    }

    fn ensure_layout(&mut self) {
        let snap = self.snap();
        let width = self.pixmap.width() as f32 / snap.scale;
        let height = self.pixmap.height() as f32 / snap.scale;
        self.session
            .ensure_layout(&mut self.gfx, width, height, snap);
    }

    fn palette(&self) -> Palette {
        palette(
            self.mode,
            self.session.model.wrecktangle_colors,
            self.accent,
        )
    }

    fn set_look(&mut self, mode: Mode, accent: Option<Accent>) {
        if (mode, accent) == (self.mode, self.accent) {
            return;
        }
        self.mode = mode;
        self.accent = accent;
        // What the server shows before a frame arrives, as on a resize.
        let conn = &x11::shared().conn;
        let background = ChangeWindowAttributesAux::new().background_pixel(background(mode));
        let _ = conn.change_window_attributes(self.id, &background);
        if let Some(popup) = &self.popup {
            let _ = conn.change_window_attributes(popup.id, &background);
        }
        self.session.invalidate();
        if self.popup.is_some() {
            self.paint_popup();
        }
    }

    fn update_cursor(&mut self, x: f32, y: f32) {
        let hand = self.session.wants_hand_cursor(x, y);
        if hand != self.hand_shown {
            self.hand_shown = hand;
            let cursor = if hand { self.hand } else { self.arrow };
            let conn = &x11::shared().conn;
            let _ = conn.change_window_attributes(
                self.id,
                &ChangeWindowAttributesAux::new().cursor(cursor),
            );
            let _ = conn.flush();
        }
    }

    fn paint(&mut self) {
        self.ensure_layout();
        let palette = self.palette();
        let Some(ops) = self.session.paint(&palette) else {
            return;
        };
        self.gfx.draw(&mut self.pixmap, &ops);
        // Painting can settle the scroll offset; that was just drawn.
        self.session.take_dirty();
        present(self.id, self.gc, self.depth, &self.pixmap, &mut self.image);
    }

    fn open_popup(&mut self, choice: Choice, current: usize) {
        self.ensure_layout();
        let Some(anchor) = self.session.window_rect(choice.target()) else {
            return;
        };
        let mut widest = 0.0f32;
        let names: Vec<(Text, f32)> = choice
            .names()
            .iter()
            .map(|name| {
                let (text, w, h) = self.gfx.shape(name, POPUP_TEXT, f32::MAX, Wrap::Line);
                widest = widest.max(w);
                (text, h)
            })
            .collect();
        let width = anchor.w.max(widest + 56.0);
        let height = names.len() as f32 * POPUP_ROW + 2.0 * POPUP_PAD;
        let s = self.scale;
        let (w_px, h_px) = ((width * s).round() as u32, (height * s).round() as u32);

        let shared = x11::shared();
        let conn = &shared.conn;
        let Some(origin) = conn
            .translate_coordinates(self.id, shared.root, 0, 0)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        else {
            return;
        };
        let screen_h = i32::from(conn.setup().roots[shared.screen].height_in_pixels);
        let x = i32::from(origin.dst_x) + (anchor.x * s).round() as i32;
        let below = i32::from(origin.dst_y) + ((anchor.bottom() + 4.0) * s).round() as i32;
        let y = if below + h_px as i32 <= screen_h {
            below
        } else {
            i32::from(origin.dst_y) + ((anchor.y - 4.0) * s).round() as i32 - h_px as i32
        };

        let (Ok(id), Some(pixmap)) = (conn.generate_id(), Pixmap::new(w_px.max(1), h_px.max(1)))
        else {
            return;
        };
        let created = conn.create_window(
            COPY_FROM_PARENT as u8,
            id,
            shared.root,
            x as i16,
            y.max(0) as i16,
            w_px as u16,
            h_px as u16,
            0,
            WindowClass::INPUT_OUTPUT,
            COPY_FROM_PARENT,
            &CreateWindowAux::new()
                .override_redirect(1)
                .background_pixel(background(self.mode))
                .event_mask(
                    EventMask::EXPOSURE
                        | EventMask::BUTTON_PRESS
                        | EventMask::BUTTON_RELEASE
                        | EventMask::POINTER_MOTION
                        | EventMask::KEY_PRESS,
                ),
        );
        if created.is_err() {
            return;
        }
        let _ = conn.map_window(id);
        // The popup must be on screen before it can take the pointer and
        // the keyboard; the round trip makes sure it is.
        let _ = conn.get_input_focus().map(|cookie| cookie.reply());
        let _ = conn.grab_pointer(
            true,
            id,
            EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
            NONE,
            NONE,
            CURRENT_TIME,
        );
        let _ = conn.grab_keyboard(true, id, CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC);
        self.popup = Some(Popup {
            id,
            choice,
            pixmap,
            names,
            current,
            hover: Some(current),
            width,
        });
        self.paint_popup();
    }

    fn close_popup(&mut self) {
        if let Some(popup) = self.popup.take() {
            let conn = &x11::shared().conn;
            let _ = conn.ungrab_pointer(CURRENT_TIME);
            let _ = conn.ungrab_keyboard(CURRENT_TIME);
            let _ = conn.destroy_window(popup.id);
            let _ = conn.flush();
        }
    }

    fn paint_popup(&mut self) {
        let palette = self.palette();
        let Some(popup) = self.popup.as_mut() else {
            return;
        };
        let snap = Snap::new(self.scale);
        let width = popup.width;
        let height = popup.names.len() as f32 * POPUP_ROW + 2.0 * POPUP_PAD;
        let mut ops = vec![
            Op::Clear(palette.background),
            Op::Stroke {
                rect: Rect::new(0.0, 0.0, width, height),
                radius: 0.0,
                width: snap.hairline(),
                color: palette.card_border,
            },
        ];
        for (index, (text, h)) in popup.names.iter().enumerate() {
            let top = POPUP_PAD + index as f32 * POPUP_ROW;
            if popup.hover == Some(index) {
                ops.push(Op::Fill {
                    rect: snap.rect(Rect::new(
                        POPUP_PAD,
                        top,
                        width - 2.0 * POPUP_PAD,
                        POPUP_ROW,
                    )),
                    radius: 4.0,
                    color: palette.control_hover,
                });
            }
            if index == popup.current {
                let mid = top + POPUP_ROW / 2.0;
                ops.push(Op::Lines {
                    figures: vec![vec![(13.0, mid), (16.5, mid + 3.5), (23.0, mid - 4.0)]],
                    width: 1.5,
                    color: palette.accent_text,
                });
            }
            ops.push(Op::Text {
                text: text.clone(),
                x: 32.0,
                y: snap.round(top + (POPUP_ROW - h) / 2.0),
                color: palette.foreground,
            });
        }
        self.gfx.draw(&mut popup.pixmap, &ops);
        present(
            popup.id,
            self.gc,
            self.depth,
            &popup.pixmap,
            &mut self.image,
        );
    }

    /// The popup's share of an event, or None to handle it as usual.
    fn handle_popup(&mut self, event: &Event) -> Option<After> {
        let popup = self.popup.as_ref()?;
        let s = self.scale;
        let row_at = |x: i16, y: i16, window: XWindow| {
            if window != popup.id {
                return None;
            }
            let (x, y) = (f32::from(x) / s, f32::from(y) / s - POPUP_PAD);
            let row = (y / POPUP_ROW).floor();
            (x >= 0.0 && x < popup.width && row >= 0.0 && (row as usize) < popup.names.len())
                .then_some(row as usize)
        };
        match event {
            Event::MotionNotify(e) => {
                let hover = row_at(e.event_x, e.event_y, e.event);
                if hover.is_some() && hover != self.popup.as_ref()?.hover {
                    self.popup.as_mut()?.hover = hover;
                    self.paint_popup();
                }
                Some(After::Nothing)
            }
            Event::ButtonPress(e) => {
                if row_at(e.event_x, e.event_y, e.event).is_none() {
                    self.close_popup();
                }
                Some(After::Nothing)
            }
            Event::ButtonRelease(e) => {
                let chosen = row_at(e.event_x, e.event_y, e.event)?;
                let choice = popup.choice;
                self.close_popup();
                Some(After::Run(choice.command(chosen)))
            }
            Event::KeyPress(e) => {
                let info = app::key_info(e.detail, u16::from(e.state));
                let count = popup.names.len();
                let hover = popup.hover.unwrap_or(popup.current);
                match info.keysym {
                    KEY_ESCAPE | KEY_TAB => self.close_popup(),
                    KEY_UP | KEY_DOWN => {
                        let next = if info.keysym == KEY_UP {
                            (hover + count - 1) % count
                        } else {
                            (hover + 1) % count
                        };
                        self.popup.as_mut()?.hover = Some(next);
                        self.paint_popup();
                    }
                    KEY_RETURN | KEY_KP_ENTER | KEY_SPACE => {
                        let choice = popup.choice;
                        self.close_popup();
                        return Some(After::Run(choice.command(hover)));
                    }
                    _ => {}
                }
                Some(After::Nothing)
            }
            Event::Expose(e) if e.window == popup.id => {
                self.paint_popup();
                Some(After::Nothing)
            }
            _ => None,
        }
    }
}

/// Puts a pixmap on a window in bands that each fit one request.
fn present(window: XWindow, gc: u32, depth: u8, pixmap: &Pixmap, image: &mut Vec<u8>) {
    let conn = &x11::shared().conn;
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let lsb = conn.setup().image_byte_order == ImageOrder::LSB_FIRST;
    image.clear();
    image.reserve(w * h * 4);
    // Every pixel is opaque (each frame starts with a clear), so the
    // premultiplied color is the color.
    for p in pixmap.data().as_chunks::<4>().0 {
        let [r, g, b, _] = *p;
        if lsb {
            image.extend_from_slice(&[b, g, r, 0]);
        } else {
            image.extend_from_slice(&[0, r, g, b]);
        }
    }
    let stride = w * 4;
    let rows_per_band = ((conn.maximum_request_bytes() - 64) / stride.max(1)).max(1);
    let mut top = 0;
    while top < h {
        let rows = rows_per_band.min(h - top);
        let _ = conn.put_image(
            ImageFormat::Z_PIXMAP,
            window,
            gc,
            w as u16,
            rows as u16,
            0,
            top as i16,
            0,
            depth,
            &image[top * stride..(top + rows) * stride],
        );
        top += rows;
    }
    let _ = conn.flush();
}
