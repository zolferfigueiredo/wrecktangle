#![windows_subsystem = "windows"]
// Off Windows, the pure modules (config, layout, owners, shortcut, takeover,
// theme) are only exercised by `cargo test`; nothing else in the crate calls
// them there.
#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod app;
mod config;
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
#[cfg(windows)]
mod window_ops;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
#[cfg(windows)]
use windows::Win32::System::Threading::CreateMutexW;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, DispatchMessageW, FindWindowW, GetMessageW, GetWindowThreadProcessId,
    IsDialogMessageW, MSG, PostMessageW, TranslateMessage,
};
#[cfg(windows)]
use windows::core::w;

#[cfg(windows)]
fn main() {
    unsafe {
        let mutex = CreateMutexW(None, true, w!("Local\\Wectangle.SingleInstance"));
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

        let Ok(_hwnd) = app::init() else {
            if let Ok(handle) = mutex {
                let _ = CloseHandle(handle);
            }
            return;
        };

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let handled = match settings::hwnd() {
                Some(settings_hwnd) => IsDialogMessageW(settings_hwnd, &msg).as_bool(),
                None => false,
            };
            if !handled {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        if let Ok(handle) = mutex {
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
fn main() {}
