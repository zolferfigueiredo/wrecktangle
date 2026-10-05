//! The Settings window itself: a top-level Win32 window drawn with Direct2D.
//! Input handlers borrow the window state only to work out what happened,
//! and run the resulting `Command` (or any Win32 call that can re-enter this
//! window procedure) after the borrow is dropped.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{
    COLORREF, D2DERR_RECREATE_TARGET, ERROR_CLASS_ALREADY_EXISTS, GetLastError, HWND, LPARAM,
    LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, ClientToScreen, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
    GetMonitorInfoW, HDC, InvalidateRect, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL,
    MONITORINFO, MonitorFromPoint, MonitorFromWindow, PAINTSTRUCT, ScreenToClient,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{LIM_LARGE, LIM_SMALL, LoadIconMetric, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForMonitor, GetDpiForWindow, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    VIRTUAL_KEY, VK_CONTROL, VK_DOWN, VK_RETURN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CS_HREDRAW, CS_VREDRAW, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, GetClientRect, GetCursorPos, HTCLIENT, IDC_ARROW, IDC_HAND, IsIconic,
    IsWindowVisible, LoadCursorW, MF_CHECKED, MF_STRING, MINMAXINFO, RegisterClassExW, SC_KEYMENU,
    SPI_GETWHEELSCROLLLINES, SW_HIDE, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE, SWP_NOZORDER,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetCursor, SetForegroundWindow, SetWindowPos,
    SetWindowTextW, ShowWindow, SystemParametersInfoW, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_TOPALIGN,
    TPM_VERTICAL, TPMPARAMS, TrackPopupMenuEx, WA_INACTIVE, WHEEL_DELTA, WINDOW_EX_STYLE,
    WM_ACTIVATE, WM_CAPTURECHANGED, WM_CLOSE, WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETCURSOR,
    WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOMMAND, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{Error, PCWSTR, Result, w};

use super::Command;
use super::gfx::{Gfx, Text};
use crate::lang;
use crate::theme::{self, Mode};
use crate::ui::palette::{Rgba, palette};
use crate::ui::view::{self, Interaction, Layout, Model, PAGES, Page, Target};
use crate::ui::{Scroll, Snap, focus_step};

const CLASS_NAME: PCWSTR = w!("WrecktangleSettingsWindow");
const CLIENT_W: f32 = 640.0;
const CLIENT_H: f32 = 682.0;
const MIN_W: f32 = 480.0;
const MIN_H: f32 = 320.0;
const LINE_SCROLL: f32 = 16.0;
const REVEAL_MARGIN: f32 = 8.0;
/// SystemParametersInfo's "scroll one page per notch" value.
const WHEEL_PAGESCROLL: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Drag {
    start_y: f32,
    start_offset: f32,
}

struct Window {
    hwnd: HWND,
    gfx: Gfx,
    model: Model,
    layout: Option<Layout<Text>>,
    ui: Interaction,
    offsets: [f32; PAGES.len()],
    mode: Mode,
    mouse: Option<(f32, f32)>,
    tracking: bool,
    drag: Option<Drag>,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static HANDLE: Cell<Option<HWND>> = const { Cell::new(None) };
    static BACKGROUND: Cell<COLORREF> = const { Cell::new(COLORREF(0)) };
}

fn handle() -> Option<HWND> {
    HANDLE.with(Cell::get)
}

/// Runs `f` on the window state. Re-entering while it is borrowed is a bug
/// (see CLAUDE.md); it is skipped rather than aborting the process.
fn with<R>(f: impl FnOnce(&mut Window) -> R) -> Option<R> {
    WINDOW.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            debug_assert!(false, "Settings window state borrowed re-entrantly");
            return None;
        };
        state.as_mut().map(f)
    })
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn dpi(hwnd: HWND) -> u32 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 96,
        dpi => dpi,
    }
}

fn colorref(c: Rgba) -> COLORREF {
    COLORREF(u32::from(c.r) | (u32::from(c.g) << 8) | (u32::from(c.b) << 16))
}

fn page_index(page: Page) -> usize {
    PAGES.iter().position(|p| *p == page).unwrap_or(0)
}

/// The outer size of a window whose client area is `w` x `h` DIPs.
fn frame_size(w: f32, h: f32, dpi: u32) -> (i32, i32) {
    let scale = dpi as f32 / 96.0;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: (w * scale).round() as i32,
        bottom: (h * scale).round() as i32,
    };
    let _ = unsafe {
        AdjustWindowRectExForDpi(
            &mut rect,
            WS_OVERLAPPEDWINDOW,
            false,
            WINDOW_EX_STYLE::default(),
            dpi,
        )
    };
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// Centered on the work area of the monitor under the cursor, which is where
/// the tray icon was just clicked; returns x, y, width, height and the DPI.
fn placement() -> (i32, i32, i32, i32, u32) {
    unsafe {
        let mut cursor = POINT::default();
        let _ = GetCursorPos(&mut cursor);
        let monitor = MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1280,
                bottom: 800,
            }
        };
        let (mut dpi_x, mut dpi_y) = (96, 96);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let (w, h) = frame_size(CLIENT_W, CLIENT_H, dpi_x);
        let work_w = work.right - work.left;
        let work_h = work.bottom - work.top;
        let (w, h) = (w.min(work_w), h.min(work_h));
        (
            work.left + (work_w - w) / 2,
            work.top + (work_h - h) / 2,
            w,
            h,
            dpi_x,
        )
    }
}

pub fn exists() -> bool {
    handle().is_some()
}

pub fn create() -> Result<()> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;
        // MAKEINTRESOURCEW(1): the pointer value itself is resource id 1
        // (assets/wrecktangle.rc), not a real memory address.
        #[allow(clippy::manual_dangling_ptr)]
        let icon = PCWSTR(1usize as *const u16);
        let class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            hIcon: LoadIconMetric(Some(hinstance.into()), icon, LIM_LARGE).unwrap_or_default(),
            hIconSm: LoadIconMetric(Some(hinstance.into()), icon, LIM_SMALL).unwrap_or_default(),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        // A failed CreateWindowExW below leaves the class registered for the
        // next attempt.
        if RegisterClassExW(&class) == 0 && GetLastError() != ERROR_CLASS_ALREADY_EXISTS {
            return Err(Error::from_thread());
        }

        let (x, y, w, h, monitor_dpi) = placement();
        let mode = theme::current_mode();
        BACKGROUND.with(|bg| bg.set(colorref(palette(mode, false).background)));
        let gfx = Gfx::new(monitor_dpi)?;
        let title = wide(&lang::t("window.title"));
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            CLASS_NAME,
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;
        let mut window = Window {
            hwnd,
            gfx,
            model: Model::default(),
            layout: None,
            ui: Interaction::default(),
            offsets: [0.0; PAGES.len()],
            mode,
            mouse: None,
            tracking: false,
            drag: None,
        };
        window.gfx.set_dpi(dpi(hwnd));
        WINDOW.with(|cell| *cell.borrow_mut() = Some(window));
        HANDLE.with(|h| h.set(Some(hwnd)));
        theme::apply_title_bar(hwnd, mode == Mode::Dark);
        Ok(())
    }
}

pub fn is_visible() -> bool {
    handle().is_some_and(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
}

pub fn show() {
    let Some(hwnd) = handle() else { return };
    retheme(hwnd);
    unsafe {
        if MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL).is_invalid() {
            let (x, y, w, h, _) = placement();
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        }
        let command = if IsIconic(hwnd).as_bool() {
            SW_RESTORE
        } else {
            SW_SHOW
        };
        let _ = ShowWindow(hwnd, command);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Changes what the window shows; it repaints on its own.
pub fn update(change: impl FnOnce(&mut Model)) {
    with(|w| {
        change(&mut w.model);
        w.layout = None;
        w.invalidate();
    });
}

/// Switches the page; false if it was already showing.
pub fn set_page(page: Page) -> bool {
    with(|w| {
        if w.model.page == page {
            return false;
        }
        w.model.page = page;
        if matches!(w.ui.focus, Some(Target::Tab(_))) {
            w.ui.focus = Some(Target::Tab(page));
        }
        w.layout = None;
        w.invalidate();
        true
    })
    .unwrap_or(false)
}

pub fn language_changed() {
    with(|w| {
        w.gfx.language_changed();
        w.layout = None;
        w.invalidate();
    });
    if let Some(hwnd) = handle() {
        let title = wide(&lang::t("window.title"));
        unsafe {
            let _ = SetWindowTextW(hwnd, PCWSTR(title.as_ptr()));
        }
    }
}

/// Shows the languages as a menu under the language box and returns the
/// chosen index.
pub fn pick_language(current: usize) -> Option<usize> {
    let hwnd = handle()?;
    let (rect, scale) = with(|w| {
        w.ensure_layout();
        let rect = w
            .layout
            .as_ref()?
            .window_rect(Target::Language, w.painted_offset())?;
        Some((rect, w.snap().scale))
    })??;
    let to_px = |v: f32| (v * scale).round() as i32;
    let mut top_left = POINT {
        x: to_px(rect.x),
        y: to_px(rect.y),
    };
    let mut bottom_right = POINT {
        x: to_px(rect.right()),
        y: to_px(rect.bottom()),
    };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut top_left);
        let _ = ClientToScreen(hwnd, &mut bottom_right);
        let menu = CreatePopupMenu().ok()?;
        for (index, language) in lang::LANGUAGES.iter().enumerate() {
            let name = wide(language.name);
            let flags = if index == current {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            let _ = AppendMenuW(menu, flags, index + 1, PCWSTR(name.as_ptr()));
        }
        // rcExclude keeps the menu off the box, flipping it above when there
        // is no room below.
        let params = TPMPARAMS {
            cbSize: std::mem::size_of::<TPMPARAMS>() as u32,
            rcExclude: RECT {
                left: top_left.x,
                top: top_left.y,
                right: bottom_right.x,
                bottom: bottom_right.y,
            },
        };
        let chosen = TrackPopupMenuEx(
            menu,
            (TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN | TPM_VERTICAL).0,
            top_left.x,
            bottom_right.y,
            hwnd,
            Some(&params),
        );
        let _ = DestroyMenu(menu);
        usize::try_from(chosen.0).ok()?.checked_sub(1)
    }
}

impl Window {
    fn snap(&self) -> Snap {
        Snap::new(dpi(self.hwnd) as f32 / 96.0)
    }

    fn invalidate(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn ensure_layout(&mut self) {
        if self.layout.is_some() {
            return;
        }
        let snap = self.snap();
        let mut client = RECT::default();
        let _ = unsafe { GetClientRect(self.hwnd, &mut client) };
        let width = (client.right - client.left) as f32 / snap.scale;
        let height = (client.bottom - client.top) as f32 / snap.scale;
        let layout = view::layout(&self.model, &mut self.gfx, width, height, snap);
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

    fn set_offset(&mut self, offset: f32) {
        let mut scroll = self.scroll();
        scroll.offset = offset;
        scroll.clamp();
        if scroll.offset != self.offset() {
            self.offsets[page_index(self.model.page)] = scroll.offset;
            self.invalidate();
        }
    }

    /// The offset content is painted and hit tested with, on whole pixels.
    fn painted_offset(&self) -> f32 {
        self.snap().round(self.offset())
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
            self.invalidate();
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
            Target::Colors => Command::SetColors(!self.model.wrecktangle_colors),
            Target::Size(index) => Command::ToggleSize(index),
            Target::Startup => Command::SetStartup(!self.model.startup),
            Target::AutoUpdate => Command::SetAutoUpdate(!self.model.auto_update),
            Target::CheckNow => Command::CheckNow,
            Target::Download => Command::Download,
            Target::Link(link) => Command::OpenUrl(link.url()),
        }
    }

    /// Draws a frame; true if the device was lost and it must be redrawn.
    fn paint(&mut self) -> bool {
        self.ensure_layout();
        let Some(layout) = &self.layout else {
            return false;
        };
        let ops = view::paint(
            layout,
            &self.ui,
            &self.scroll(),
            &palette(self.mode, self.model.wrecktangle_colors),
            self.snap(),
        );
        matches!(self.gfx.draw(self.hwnd, &ops), Err(e) if e.code() == D2DERR_RECREATE_TARGET)
    }
}

fn retheme(hwnd: HWND) {
    let mode = theme::current_mode();
    BACKGROUND.with(|bg| bg.set(colorref(palette(mode, false).background)));
    with(|w| {
        w.mode = mode;
        w.invalidate();
    });
    theme::apply_title_bar(hwnd, mode == Mode::Dark);
}

fn mouse_point(hwnd: HWND, lparam: LPARAM) -> (f32, f32) {
    let scale = dpi(hwnd) as f32 / 96.0;
    let x = (lparam.0 & 0xFFFF) as u16 as i16;
    let y = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16;
    (f32::from(x) / scale, f32::from(y) / scale)
}

fn mouse_move(hwnd: HWND, x: f32, y: f32) {
    let start_tracking = with(|w| {
        w.ensure_layout();
        w.mouse = Some((x, y));
        if let Some(drag) = w.drag {
            let viewport = w.layout.as_ref().map(|l| l.viewport).unwrap_or_default();
            let offset = w
                .scroll()
                .drag(viewport, drag.start_offset, y - drag.start_y);
            w.set_offset(offset);
        } else {
            w.update_hover();
        }
        !std::mem::replace(&mut w.tracking, true)
    })
    .unwrap_or(false);
    if start_tracking {
        let mut track = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        unsafe {
            let _ = TrackMouseEvent(&mut track);
        }
    }
}

fn mouse_down(hwnd: HWND, x: f32, y: f32) {
    let capture = with(|w| {
        w.ensure_layout();
        w.mouse = Some((x, y));
        w.ui.focus_visible = false;
        w.update_hover();
        w.invalidate();
        if w.over_scrollbar(x, y) {
            let scroll = w.scroll();
            let viewport = w.layout.as_ref().map(|l| l.viewport).unwrap_or_default();
            let Some(thumb) = scroll.thumb(viewport, true) else {
                return false;
            };
            if y < thumb.y {
                w.set_offset(scroll.offset - viewport.h);
            } else if y >= thumb.bottom() {
                w.set_offset(scroll.offset + viewport.h);
            } else {
                w.drag = Some(Drag {
                    start_y: y,
                    start_offset: scroll.offset,
                });
                w.ui.bar_drag = true;
            }
            return w.drag.is_some();
        }
        let target = w.target_at(x, y);
        w.ui.pressed = target;
        if target.is_some() {
            w.ui.focus = target;
        }
        target.is_some()
    })
    .unwrap_or(false);
    if capture {
        unsafe {
            SetCapture(hwnd);
        }
    }
}

fn mouse_up(x: f32, y: f32) {
    let (release, command) = with(|w| {
        let dragged = w.drag.take().is_some();
        w.ui.bar_drag = false;
        w.mouse = Some((x, y));
        w.update_hover();
        let pressed = w.ui.pressed.take();
        w.invalidate();
        let clicked = pressed.filter(|t| w.ui.hover == Some(*t));
        (dragged || pressed.is_some(), clicked.map(|t| w.command(t)))
    })
    .unwrap_or((false, None));
    if release {
        unsafe {
            let _ = ReleaseCapture();
        }
    }
    if let Some(command) = command {
        super::run(command);
    }
}

fn wheel(delta: i16) {
    let mut lines = 3u32;
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWHEELSCROLLLINES,
            0,
            Some(&mut lines as *mut u32 as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    with(|w| {
        w.ensure_layout();
        let step = if lines == WHEEL_PAGESCROLL {
            w.scroll().viewport
        } else {
            lines as f32 * LINE_SCROLL
        };
        let amount = -f32::from(delta) / WHEEL_DELTA as f32 * step;
        w.set_offset(w.offset() + amount);
        w.update_hover();
    });
}

fn set_cursor(hwnd: HWND) {
    let scale = dpi(hwnd) as f32 / 96.0;
    let mut point = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut point);
        let _ = ScreenToClient(hwnd, &mut point);
    }
    let hand = with(|w| {
        w.ensure_layout();
        w.target_at(point.x as f32 / scale, point.y as f32 / scale)
            .is_some_and(Target::uses_hand_cursor)
    })
    .unwrap_or(false);
    let cursor = if hand { IDC_HAND } else { IDC_ARROW };
    unsafe {
        SetCursor(LoadCursorW(None, cursor).ok());
    }
}

fn key_down(vk: VIRTUAL_KEY, repeat: bool) -> bool {
    let pressed = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    match vk {
        VK_TAB if pressed(VK_CONTROL) => {
            let backwards = pressed(VK_SHIFT);
            let page = with(|w| {
                let i = page_index(w.model.page);
                let n = PAGES.len();
                PAGES[if backwards {
                    (i + n - 1) % n
                } else {
                    (i + 1) % n
                }]
            });
            if let Some(page) = page {
                super::run(Command::Page(page));
            }
            true
        }
        VK_TAB => {
            let backwards = pressed(VK_SHIFT);
            with(|w| {
                w.ensure_layout();
                let order = w
                    .layout
                    .as_ref()
                    .map(|l| l.focus_order())
                    .unwrap_or_default();
                w.ui.focus = focus_step(&order, w.ui.focus, backwards);
                w.ui.focus_visible = true;
                if let Some(target) = w.ui.focus {
                    w.reveal(target);
                }
                w.update_hover();
                w.invalidate();
            });
            true
        }
        VK_RETURN | VK_SPACE => {
            // Only the first key-down: holding the key must not repeat, and
            // the key-up of a just recorded Space or Enter must not activate.
            if repeat {
                return true;
            }
            let command = with(|w| {
                w.ensure_layout();
                let focus = w.ui.focus?;
                w.ui.focus_visible = true;
                w.invalidate();
                Some(w.command(focus))
            })
            .flatten();
            if let Some(command) = command {
                super::run(command);
            }
            true
        }
        VK_UP | VK_DOWN => {
            let current =
                with(|w| (w.ui.focus == Some(Target::Language)).then_some(w.model.language))
                    .flatten();
            let Some(current) = current else {
                return false;
            };
            let next = if vk == VK_UP {
                current.checked_sub(1)
            } else {
                Some(current + 1).filter(|i| *i < lang::LANGUAGES.len())
            };
            if let Some(next) = next {
                super::run(Command::Language(next));
            }
            true
        }
        _ => false,
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            // BeginPaint can send WM_ERASEBKGND, so it comes before the borrow.
            let mut ps = PAINTSTRUCT::default();
            unsafe { BeginPaint(hwnd, &mut ps) };
            let lost = with(|w| w.paint()).unwrap_or(false);
            unsafe {
                let _ = EndPaint(hwnd, &ps);
                if lost {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            unsafe {
                let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let brush = CreateSolidBrush(BACKGROUND.with(Cell::get));
                FillRect(hdc, &client, brush);
                let _ = DeleteObject(brush.into());
            }
            LRESULT(1)
        }
        WM_SIZE => {
            let width = (lparam.0 & 0xFFFF) as u32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
            with(|w| {
                w.gfx.resize(width, height);
                w.layout = None;
                w.invalidate();
            });
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as u32;
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            with(|w| {
                w.gfx.set_dpi(new_dpi);
                w.layout = None;
                w.invalidate();
            });
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let (w, h) = frame_size(MIN_W, MIN_H, dpi(hwnd));
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            info.ptMinTrackSize = POINT { x: w, y: h };
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = mouse_point(hwnd, lparam);
            mouse_move(hwnd, x, y);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|w| {
                w.tracking = false;
                w.mouse = None;
                w.update_hover();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = mouse_point(hwnd, lparam);
            mouse_down(hwnd, x, y);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = mouse_point(hwnd, lparam);
            mouse_up(x, y);
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            with(|w| {
                w.drag = None;
                w.ui.bar_drag = false;
                w.ui.pressed = None;
                w.update_hover();
                w.invalidate();
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            wheel(((wparam.0 >> 16) & 0xFFFF) as u16 as i16);
            LRESULT(0)
        }
        WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
            set_cursor(hwnd);
            LRESULT(1)
        }
        WM_KEYDOWN => {
            let repeat = lparam.0 & (1 << 30) != 0;
            if key_down(VIRTUAL_KEY(wparam.0 as u16), repeat) {
                LRESULT(0)
            } else {
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        // A lone Alt press and release would enter menu mode, which a
        // window with no menu bar has no use for. The recorder swallows the
        // key between Alt's press and release, so this happens after every
        // recorded Alt shortcut. Alt+Space (lParam is the key) still works.
        WM_SYSCOMMAND if (wparam.0 as u32 & 0xFFF0) == SC_KEYMENU && lparam.0 == 0 => LRESULT(0),
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                with(|w| {
                    w.ui.focus_visible = false;
                    w.invalidate();
                });
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SETTINGCHANGE => {
            if theme::is_immersive_color_set_change(lparam) {
                retheme(hwnd);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_CLOSE => {
            super::on_close();
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
