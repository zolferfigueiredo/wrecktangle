//! The Settings window itself: a top-level Win32 window drawn with Direct2D.
//! Input handlers borrow the window state only to work out what happened,
//! and run the resulting `Command` (or any Win32 call that can re-enter this
//! window procedure) after the borrow is dropped.

use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{
    COLORREF, D2DERR_RECREATE_TARGET, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
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
    DestroyMenu, DestroyWindow, GetClassInfoExW, GetClientRect, GetCursorPos, HTCLIENT, IDC_ARROW,
    IDC_HAND, IsIconic, IsWindowVisible, LoadCursorW, MF_CHECKED, MF_STRING, MINMAXINFO,
    RegisterClassExW, SC_KEYMENU, SPI_GETWHEELSCROLLLINES, SW_RESTORE, SW_SHOW, SWP_NOACTIVATE,
    SWP_NOZORDER, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetCursor, SetForegroundWindow,
    SetWindowPos, SetWindowTextW, ShowWindow, SystemParametersInfoW, TPM_LEFTALIGN, TPM_RETURNCMD,
    TPM_TOPALIGN, TPM_VERTICAL, TPMPARAMS, TrackPopupMenuEx, WA_INACTIVE, WHEEL_DELTA,
    WINDOW_EX_STYLE, WM_ACTIVATE, WM_CAPTURECHANGED, WM_CLOSE, WM_DESTROY, WM_DISPLAYCHANGE,
    WM_DPICHANGED, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETCURSOR, WM_SETTINGCHANGE, WM_SIZE, WM_SYSCOMMAND,
    WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
};
use windows::core::{Error, PCWSTR, Result, w};

use super::gfx::{Gfx, Text};
use crate::lang;
use crate::theme::{self, Mode, Theme};
use crate::ui::Snap;
use crate::ui::palette::{Accent, Rgba, palette};
use crate::ui::session::{Key, Session};
use crate::ui::view::{Model, Page, Target};

const CLASS_NAME: PCWSTR = w!("WrecktangleSettingsWindow");
const CLIENT_W: f32 = 640.0;
// Fits the Shortcuts page with a note under every row in every language
// (782 DIPs at most, when the intro wraps), so it opens without scrolling.
const CLIENT_H: f32 = 784.0;
const MIN_W: f32 = 480.0;
const MIN_H: f32 = 320.0;
/// SystemParametersInfo's "scroll one page per notch" value.
const WHEEL_PAGESCROLL: u32 = u32::MAX;

struct Window {
    hwnd: HWND,
    gfx: Gfx,
    session: Session<Text>,
    mode: Mode,
    accent: Option<Accent>,
    tracking: bool,
}

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
    static HANDLE: Cell<Option<HWND>> = const { Cell::new(None) };
    static BACKGROUND: Cell<COLORREF> = const { Cell::new(COLORREF(0)) };
}

fn handle() -> Option<HWND> {
    HANDLE.with(Cell::get)
}

/// Runs `f` on the window state, then schedules a repaint if the session
/// changed what shows. Re-entering while it is borrowed is a bug (see
/// CLAUDE.md); it is skipped rather than aborting the process.
fn with<R>(f: impl FnOnce(&mut Window) -> R) -> Option<R> {
    WINDOW.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            debug_assert!(false, "Settings window state borrowed re-entrantly");
            return None;
        };
        let window = state.as_mut()?;
        let result = f(window);
        if window.session.take_dirty() {
            window.invalidate();
        }
        Some(result)
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
        // Registered on the first open only: the window is built on every
        // open, and loading the class icons again would leak them.
        let mut registered = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            ..Default::default()
        };
        if GetClassInfoExW(Some(hinstance.into()), CLASS_NAME, &mut registered).is_err() {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                hIcon: LoadIconMetric(Some(hinstance.into()), icon, LIM_LARGE).unwrap_or_default(),
                hIconSm: LoadIconMetric(Some(hinstance.into()), icon, LIM_SMALL)
                    .unwrap_or_default(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: CLASS_NAME,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0 {
                return Err(Error::from_thread());
            }
        }

        let (x, y, w, h, monitor_dpi) = placement();
        let mode = theme::current_mode(crate::app::current_config().theme);
        let accent = theme::system_accent();
        BACKGROUND.with(|bg| bg.set(colorref(palette(mode, false, None).background)));
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
            session: Session::default(),
            mode,
            accent,
            tracking: false,
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
    retheme_window(hwnd);
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
    with(|w| w.session.update(change));
}

/// Switches the page; false if it was already showing.
pub fn set_page(page: Page) -> bool {
    with(|w| w.session.set_page(page)).unwrap_or(false)
}

pub fn language_changed() {
    with(|w| {
        w.gfx.language_changed();
        w.session.relayout();
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
    let names: Vec<&str> = lang::LANGUAGES.iter().map(|l| l.name).collect();
    pick(Target::Language, &names, current)
}

/// Shows System, Light and Dark as a menu under the theme box and returns
/// the chosen index into `Theme::ALL`.
pub fn pick_theme(current: Theme) -> Option<usize> {
    let names: Vec<String> = Theme::ALL
        .iter()
        .map(|theme| lang::t(theme.label_key()))
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    pick(Target::Theme, &names, current.index())
}

/// A menu of `names` under a box, with `current` checked; the chosen index.
fn pick(target: Target, names: &[&str], current: usize) -> Option<usize> {
    let hwnd = handle()?;
    let (rect, scale) = with(|w| {
        w.ensure_layout();
        let rect = w.session.window_rect(target)?;
        Some((rect, w.session.snap().scale))
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
        for (index, name) in names.iter().enumerate() {
            let name = wide(name);
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
        let snap = self.snap();
        let mut client = RECT::default();
        let _ = unsafe { GetClientRect(self.hwnd, &mut client) };
        let width = (client.right - client.left) as f32 / snap.scale;
        let height = (client.bottom - client.top) as f32 / snap.scale;
        self.session
            .ensure_layout(&mut self.gfx, width, height, snap);
    }

    /// Draws a frame; true if the device was lost and it must be redrawn.
    fn paint(&mut self) -> bool {
        self.ensure_layout();
        let palette = palette(
            self.mode,
            self.session.model.wrecktangle_colors,
            self.accent,
        );
        let Some(ops) = self.session.paint(&palette) else {
            return false;
        };
        matches!(self.gfx.draw(self.hwnd, &ops), Err(e) if e.code() == D2DERR_RECREATE_TARGET)
    }
}

/// Takes the light or dark mode and the accent again, after the Theme
/// setting changed.
pub fn retheme() {
    if let Some(hwnd) = handle() {
        retheme_window(hwnd);
    }
}

// Also when the accent color changes, which Windows announces the same way.
fn retheme_window(hwnd: HWND) {
    let mode = theme::current_mode(crate::app::current_config().theme);
    // Read before borrowing the window state, since it calls into Windows.
    let accent = theme::system_accent();
    BACKGROUND.with(|bg| bg.set(colorref(palette(mode, false, None).background)));
    with(|w| {
        w.mode = mode;
        w.accent = accent;
        w.session.invalidate();
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
        w.session.mouse_move(x, y);
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
        w.session.mouse_down(x, y)
    })
    .unwrap_or(false);
    if capture {
        unsafe {
            SetCapture(hwnd);
        }
    }
}

fn mouse_up(x: f32, y: f32) {
    let (release, command) = with(|w| w.session.mouse_up(x, y)).unwrap_or((false, None));
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
    let lines = (lines != WHEEL_PAGESCROLL).then_some(lines as f32);
    with(|w| {
        w.ensure_layout();
        w.session
            .wheel(f32::from(delta) / WHEEL_DELTA as f32, lines);
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
        w.session
            .wants_hand_cursor(point.x as f32 / scale, point.y as f32 / scale)
    })
    .unwrap_or(false);
    let cursor = if hand { IDC_HAND } else { IDC_ARROW };
    unsafe {
        SetCursor(LoadCursorW(None, cursor).ok());
    }
}

fn key_down(vk: VIRTUAL_KEY, repeat: bool) -> bool {
    let key = match vk {
        VK_TAB => Key::Tab,
        VK_RETURN => Key::Enter,
        VK_SPACE => Key::Space,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        _ => return false,
    };
    let pressed = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
    let (ctrl, shift) = (pressed(VK_CONTROL), pressed(VK_SHIFT));
    let (handled, command) = with(|w| {
        w.ensure_layout();
        w.session.key(key, ctrl, shift, repeat)
    })
    .unwrap_or((false, None));
    if let Some(command) = command {
        super::run(command);
    }
    handled
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
                w.session.relayout();
            });
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as u32;
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            with(|w| {
                w.gfx.set_dpi(new_dpi);
                w.session.relayout();
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
                w.session.mouse_leave();
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
            with(|w| w.session.capture_lost());
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
                with(|w| w.session.deactivated());
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_SETTINGCHANGE => {
            if theme::is_immersive_color_set_change(lparam) {
                retheme_window(hwnd);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_DISPLAYCHANGE => {
            super::show_displays();
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        // Destroyed rather than hidden: its Direct2D device and swap chain
        // stay allocated as long as the window exists. settings::open builds
        // it again.
        WM_CLOSE => {
            super::on_close();
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            HANDLE.with(|h| h.set(None));
            // Taken out of the cell first, so Gfx is released after the borrow.
            let state = WINDOW.with(|cell| cell.try_borrow_mut().ok().and_then(|mut s| s.take()));
            drop(state);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
