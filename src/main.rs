#![windows_subsystem = "windows"]
// Off Windows, the pure modules (config, lang, layout, owners, shortcut,
// takeover, theme, update) are only exercised by `cargo test`; nothing else
// in the crate calls them there.
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod app;
mod config;
mod lang;
mod layout;
mod owners;
#[cfg(windows)]
mod settings;
mod shortcut;
#[cfg(windows)]
mod startup;
mod takeover;
mod theme;
#[cfg(windows)]
mod tray;
mod update;
#[cfg(windows)]
mod window_ops;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, GetWindowThreadProcessId, PostMessageW,
    SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_CLOSE,
};
#[cfg(windows)]
use windows::core::w;

#[cfg(windows)]
fn main() {
    unsafe {
        let mutex = CreateMutexW(None, true, w!("Local\\Wrecktangle.SingleInstance"));
        let already_running =
            mutex.is_ok() && windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS;

        if already_running {
            if let Ok(hwnd) = FindWindowW(app::WINDOW_CLASS_NAME, None) {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                let _ = AllowSetForegroundWindow(pid);
                let _ = PostMessageW(Some(hwnd), app::WM_APP_OPEN_SETTINGS, WPARAM(0), LPARAM(0));
            }
            if let Ok(handle) = mutex {
                let _ = CloseHandle(handle);
            }
            return;
        }

        close_legacy_instance();

        let Ok(_hwnd) = app::init() else {
            if let Ok(handle) = mutex {
                let _ = CloseHandle(handle);
            }
            return;
        };

        // Winit's loop dispatches every window on this thread, including the
        // hidden main window that owns the hotkeys, tray icon and hooks.
        let _ = slint::run_event_loop_until_quit();

        if let Ok(handle) = mutex {
            let _ = CloseHandle(handle);
        }
    }
}

/// Quits a copy still running under the app's earlier name, Wectangle, which
/// holds the same hotkeys. WM_CLOSE is its tray Quit path, and its hotkeys are
/// unregistered by the time SendMessageTimeoutW returns.
#[cfg(windows)]
fn close_legacy_instance() {
    unsafe {
        if let Ok(hwnd) = FindWindowW(w!("WectangleMainWindow"), None) {
            let _ = SendMessageTimeoutW(
                hwnd,
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
                SMTO_ABORTIFHUNG,
                5000,
                None,
            );
        }
    }
}

#[cfg(not(windows))]
fn main() {}
