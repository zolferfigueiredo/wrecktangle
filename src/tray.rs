use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{LIM_SMALL, LoadIconMetric};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NIM_SETVERSION, NIN_BALLOONUSERCLICK, NIN_SELECT, NOTIFYICON_VERSION_4,
    NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::core::PCWSTR;

// Not exposed by this version of the windows crate; matches shellapi.h's
// NIN_KEYSELECT = (NIN_SELECT | 0x1).
const NIN_KEYSELECT: u32 = NIN_SELECT | 1;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, HICON, MF_CHECKED, MF_SEPARATOR, MF_STRING,
    MF_UNCHECKED, PostMessageW, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WM_CLOSE, WM_CONTEXTMENU, WM_NULL,
};

use crate::{app, lang, settings, startup, update};

const TRAY_ICON_ID: u32 = 1;
const ID_SETTINGS: u16 = 1001;
const ID_STARTUP: u16 = 1002;
const ID_QUIT: u16 = 1003;
const ID_CHECK_UPDATES: u16 = 1004;

thread_local! {
    // Clicking any balloon sends NIN_BALLOONUSERCLICK, so only the update
    // balloon, when it is the last one shown, may open the download.
    static UPDATE_BALLOON: Cell<bool> = const { Cell::new(false) };
}

fn to_wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

// Returns a whole array rather than filling a field in place: on 32-bit
// Windows NOTIFYICONDATAW is packed, so its fields can't be borrowed.
fn wide_buf<const N: usize>(text: &str) -> [u16; N] {
    let mut buf = [0u16; N];
    for (slot, unit) in buf.iter_mut().zip(text.encode_utf16().take(N - 1)) {
        *slot = unit;
    }
    buf
}

fn base_nid(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ICON_ID,
        ..Default::default()
    }
}

fn load_icon() -> HICON {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        // MAKEINTRESOURCEW(1): the pointer value itself is resource id 1
        // (assets/wrecktangle.rc), not a real memory address.
        #[allow(clippy::manual_dangling_ptr)]
        let name = PCWSTR(1usize as *const u16);
        LoadIconMetric(Some(hinstance.into()), name, LIM_SMALL).unwrap_or_default()
    }
}

/// Adds the tray icon and switches it to version 4 so NIN_SELECT and
/// WM_CONTEXTMENU replace the legacy mouse messages.
pub fn add(hwnd: HWND) -> windows::core::Result<()> {
    unsafe {
        let mut nid = base_nid(hwnd);
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP;
        nid.uCallbackMessage = app::WM_APP_TRAY;
        nid.hIcon = load_icon();
        nid.szTip = wide_buf("Wrecktangle");

        Shell_NotifyIconW(NIM_ADD, &nid).ok()?;
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        Shell_NotifyIconW(NIM_SETVERSION, &nid).ok()?;
        Ok(())
    }
}

pub fn remove(hwnd: HWND) {
    unsafe {
        let nid = base_nid(hwnd);
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

pub fn notify(hwnd: HWND, title: &str, message: &str) {
    UPDATE_BALLOON.with(|b| b.set(false));
    show_balloon(hwnd, title, message);
}

pub fn notify_update(hwnd: HWND, title: &str, message: &str) {
    show_balloon(hwnd, title, message);
    UPDATE_BALLOON.with(|b| b.set(true));
}

fn show_balloon(hwnd: HWND, title: &str, message: &str) {
    unsafe {
        let mut nid = base_nid(hwnd);
        nid.uFlags = NIF_INFO;
        nid.szInfo = wide_buf(message);
        nid.szInfoTitle = wide_buf(title);
        nid.dwInfoFlags = NIIF_INFO;
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

/// Dispatches the tray callback message. With NOTIFYICON_VERSION_4 the
/// mouse or keyboard event is in LOWORD(lParam); the cursor position is in
/// wParam. Settings opens on NIN_SELECT/NIN_KEYSELECT only, since a left
/// click also sends WM_LBUTTONUP and handling both would open it twice.
pub fn handle_callback(hwnd: HWND, wparam: WPARAM, lparam: LPARAM) {
    let event = (lparam.0 as u32) & 0xFFFF;
    match event {
        e if e == NIN_SELECT || e == NIN_KEYSELECT => {
            settings::open();
        }
        e if e == NIN_BALLOONUSERCLICK => {
            if UPDATE_BALLOON.with(|b| b.replace(false)) {
                update::open_download();
            }
        }
        e if e == WM_CONTEXTMENU => {
            let x = (wparam.0 as u16) as i16 as i32;
            let y = ((wparam.0 >> 16) as u16) as i16 as i32;
            show_context_menu(hwnd, x, y);
        }
        _ => {}
    }
}

fn show_context_menu(hwnd: HWND, x: i32, y: i32) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };

        let settings = to_wide(&lang::t("tray.settings"));
        let check_updates = to_wide(&lang::t("tray.check_updates"));
        let startup = to_wide(&lang::t("tray.startup"));
        let quit = to_wide(&lang::t("tray.quit"));
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_SETTINGS as usize,
            PCWSTR(settings.as_ptr()),
        );
        let _ = AppendMenuW(
            menu,
            MF_STRING,
            ID_CHECK_UPDATES as usize,
            PCWSTR(check_updates.as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let startup_flags = if startup::is_enabled() {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING | MF_UNCHECKED
        };
        let _ = AppendMenuW(
            menu,
            startup_flags,
            ID_STARTUP as usize,
            PCWSTR(startup.as_ptr()),
        );
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT as usize, PCWSTR(quit.as_ptr()));

        // TrackPopupMenu needs the window in the foreground, or the menu can
        // fail to dismiss when the user clicks away.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            x,
            y,
            Some(0),
            hwnd,
            None,
        );
        // Required after TrackPopupMenu so the menu closes if the user clicks
        // outside it without choosing an item.
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);

        match cmd.0 as u16 {
            ID_SETTINGS => settings::open(),
            ID_CHECK_UPDATES => {
                settings::open();
                update::check_now();
                settings::on_update_state_changed();
            }
            ID_STARTUP => {
                let _ = startup::toggle();
            }
            ID_QUIT => {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            _ => {}
        }
    }
}
