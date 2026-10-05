//! Light/dark mode detection and the Win32 glue that applies it to menus and
//! the Settings window's title bar (Windows-only). The Settings colors are in
//! `ui::palette`.

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
#[cfg(windows)]
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    IsWindowVisible, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SetWindowPos,
};
#[cfg(windows)]
use windows::core::{BOOL, PCSTR, PCWSTR, w};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Light,
    Dark,
}

#[cfg(windows)]
fn read_dword(subkey: PCWSTR, value: PCWSTR) -> Option<u32> {
    unsafe {
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let ok = RegGetValueW(
            HKEY_CURRENT_USER,
            subkey,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        );
        if ok.is_ok() { Some(data) } else { None }
    }
}

#[cfg(windows)]
fn system_prefers_light() -> bool {
    read_dword(
        w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        w!("AppsUseLightTheme"),
    )
    .map(|v| v != 0)
    .unwrap_or(true)
}

// `WRECKTANGLE_THEME=light|dark` overrides the system theme, so both themes
// can be exercised without changing Windows settings. Checked first on
// every platform, since it needs no OS call.
pub fn override_mode() -> Option<Mode> {
    match std::env::var("WRECKTANGLE_THEME")
        .ok()?
        .to_ascii_lowercase()
        .as_str()
    {
        "light" => Some(Mode::Light),
        "dark" => Some(Mode::Dark),
        _ => None,
    }
}

pub fn current_mode() -> Mode {
    if let Some(mode) = override_mode() {
        return mode;
    }
    #[cfg(windows)]
    {
        if system_prefers_light() {
            Mode::Light
        } else {
            Mode::Dark
        }
    }
    #[cfg(not(windows))]
    {
        Mode::Dark
    }
}

/// Sets the window's title bar to dark or light immersive mode. It calls
/// SetWindowPos, so never call it while holding a RefCell borrow.
#[cfg(windows)]
pub fn apply_title_bar(hwnd: HWND, dark: bool) {
    let value = BOOL::from(dark);
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &value as *const BOOL as *const core::ffi::c_void,
            std::mem::size_of::<BOOL>() as u32,
        );
        // A visible window keeps its old title bar until the frame is
        // recalculated.
        if IsWindowVisible(hwnd).as_bool() {
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
}

// uxtheme.dll exports SetPreferredAppMode (ordinal 135) and FlushMenuThemes
// (ordinal 136) without header declarations; this is how Notepad++ and
// other apps dark-theme menus on Windows 10/11. Loaded by ordinal and
// skipped if either is missing, since it is undocumented and can change.
// Forcing the mode, rather than allowing dark, lets WRECKTANGLE_THEME reach
// the menus too.
#[cfg(windows)]
pub fn apply_dark_menu(dark: bool) {
    const FORCE_DARK: i32 = 2;
    const FORCE_LIGHT: i32 = 3;
    unsafe {
        let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) else {
            return;
        };
        #[allow(clippy::manual_dangling_ptr)]
        let set_preferred_app_mode = GetProcAddress(uxtheme, PCSTR(135usize as *const u8));
        #[allow(clippy::manual_dangling_ptr)]
        let flush_menu_themes = GetProcAddress(uxtheme, PCSTR(136usize as *const u8));

        if let Some(proc) = set_preferred_app_mode {
            let set_preferred_app_mode: extern "system" fn(i32) -> i32 = std::mem::transmute(proc);
            set_preferred_app_mode(if dark { FORCE_DARK } else { FORCE_LIGHT });
        }
        if let Some(proc) = flush_menu_themes {
            let flush_menu_themes: extern "system" fn() = std::mem::transmute(proc);
            flush_menu_themes();
        }
    }
}

/// True if a WM_SETTINGCHANGE message's lParam names "ImmersiveColorSet",
/// the area Windows broadcasts when the light/dark theme or accent color
/// changes.
#[cfg(windows)]
pub fn is_immersive_color_set_change(lparam: LPARAM) -> bool {
    if lparam.0 == 0 {
        return false;
    }
    unsafe {
        let ptr = lparam.0 as *const u16;
        let mut len = 0usize;
        while len < 64 && *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        String::from_utf16_lossy(slice) == "ImmersiveColorSet"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // One test, not three: std::env mutation is process-global, and cargo
    // test runs tests on multiple threads by default, so separate tests
    // touching the same variable could race each other.
    #[test]
    fn env_override_selects_mode_and_is_case_insensitive() {
        unsafe {
            std::env::set_var("WRECKTANGLE_THEME", "light");
        }
        assert_eq!(current_mode(), Mode::Light);

        unsafe {
            std::env::set_var("WRECKTANGLE_THEME", "dark");
        }
        assert_eq!(current_mode(), Mode::Dark);

        unsafe {
            std::env::set_var("WRECKTANGLE_THEME", "LIGHT");
        }
        assert_eq!(override_mode(), Some(Mode::Light));

        unsafe {
            std::env::set_var("WRECKTANGLE_THEME", "sepia");
        }
        assert_eq!(override_mode(), None);

        unsafe {
            std::env::remove_var("WRECKTANGLE_THEME");
        }
        assert_eq!(override_mode(), None);
    }
}
