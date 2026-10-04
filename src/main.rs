#![windows_subsystem = "windows"]

mod app;
mod config;
mod layout;
mod settings;
mod shortcut;
mod startup;
mod tray;
mod window_ops;

use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, LPARAM, WPARAM};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, DispatchMessageW, FindWindowW, GetMessageW, GetWindowThreadProcessId,
    MSG, PostMessageW, TranslateMessage,
};
use windows::core::w;

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
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if let Ok(handle) = mutex {
            let _ = CloseHandle(handle);
        }
    }
}
