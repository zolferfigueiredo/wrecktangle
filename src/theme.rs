//! Light/dark mode detection and the Win32 glue that applies it to the tray
//! menu (Windows-only). The Settings window follows the theme through Slint.

#[cfg(windows)]
use windows::Win32::Foundation::LPARAM;
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
#[cfg(windows)]
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
#[cfg(windows)]
use windows::core::{PCSTR, PCWSTR, w};

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

// `WECTANGLE_THEME=light|dark` overrides the system theme, so both themes
// can be exercised without changing Windows settings. Checked first on
// every platform, since it needs no OS call.
pub fn override_mode() -> Option<Mode> {
    match std::env::var("WECTANGLE_THEME")
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

// uxtheme.dll exports SetPreferredAppMode (ordinal 135) and FlushMenuThemes
// (ordinal 136) without header declarations; this is how Notepad++ and
// other apps dark-theme menus on Windows 10/11. Loaded by ordinal and
// skipped if either is missing, since it is undocumented and can change.
#[cfg(windows)]
pub fn apply_dark_menu(dark: bool) {
    const ALLOW_DARK: i32 = 1;
    const DEFAULT_MODE: i32 = 0;
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
            set_preferred_app_mode(if dark { ALLOW_DARK } else { DEFAULT_MODE });
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
            std::env::set_var("WECTANGLE_THEME", "light");
        }
        assert_eq!(current_mode(), Mode::Light);

        unsafe {
            std::env::set_var("WECTANGLE_THEME", "dark");
        }
        assert_eq!(current_mode(), Mode::Dark);

        unsafe {
            std::env::set_var("WECTANGLE_THEME", "LIGHT");
        }
        assert_eq!(override_mode(), Some(Mode::Light));

        unsafe {
            std::env::set_var("WECTANGLE_THEME", "sepia");
        }
        assert_eq!(override_mode(), None);

        unsafe {
            std::env::remove_var("WECTANGLE_THEME");
        }
        assert_eq!(override_mode(), None);
    }
}
