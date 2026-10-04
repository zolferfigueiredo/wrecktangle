//! Light/dark mode detection and the Win32 glue that applies it to window
//! title bars and menus (Windows-only). The Fluent-style color palette
//! lives here too, added alongside the code that paints with it.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb { r, g, b }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Rgb,
    pub surface: Rgb,
    pub surface_hover: Rgb,
    pub border: Rgb,
    pub text: Rgb,
    pub text_muted: Rgb,
    pub accent: Rgb,
    pub warning: Rgb,
    pub info: Rgb,
}

const FALLBACK_ACCENT: Rgb = Rgb::new(0x00, 0x78, 0xD4);

pub fn light() -> Palette {
    Palette {
        bg: Rgb::new(0xF3, 0xF3, 0xF3),
        surface: Rgb::new(0xFF, 0xFF, 0xFF),
        surface_hover: Rgb::new(0xEC, 0xEC, 0xEC),
        border: Rgb::new(0xE1, 0xE1, 0xE1),
        text: Rgb::new(0x1A, 0x1A, 0x1A),
        text_muted: Rgb::new(0x6B, 0x6B, 0x6B),
        accent: FALLBACK_ACCENT,
        warning: Rgb::new(0x9D, 0x5D, 0x00),
        info: Rgb::new(0x00, 0x5F, 0xB8),
    }
}

pub fn dark() -> Palette {
    Palette {
        bg: Rgb::new(0x20, 0x20, 0x20),
        surface: Rgb::new(0x2C, 0x2C, 0x2C),
        surface_hover: Rgb::new(0x38, 0x38, 0x38),
        border: Rgb::new(0x45, 0x45, 0x45),
        text: Rgb::new(0xF2, 0xF2, 0xF2),
        text_muted: Rgb::new(0x9C, 0x9C, 0x9C),
        accent: FALLBACK_ACCENT,
        warning: Rgb::new(0xFF, 0xC8, 0x3D),
        info: Rgb::new(0x60, 0xCD, 0xFF),
    }
}

pub fn palette(mode: Mode) -> Palette {
    match mode {
        Mode::Light => light(),
        Mode::Dark => dark(),
    }
}

// Same palette as `palette`, with the live system accent color (or the
// fallback) filled in.
pub fn themed_palette(mode: Mode) -> Palette {
    let mut p = palette(mode);
    p.accent = accent();
    p
}

// The registry stores AccentColor as a packed ABGR DWORD: byte 0 is red,
// byte 1 green, byte 2 blue, byte 3 alpha (alpha is unused here).
fn accent_color_from_abgr(v: u32) -> Rgb {
    Rgb::new(
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
    )
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

#[cfg(windows)]
fn system_accent_raw() -> Option<u32> {
    read_dword(w!("Software\\Microsoft\\Windows\\DWM"), w!("AccentColor"))
}

pub fn accent() -> Rgb {
    #[cfg(windows)]
    {
        system_accent_raw()
            .map(accent_color_from_abgr)
            .unwrap_or(FALLBACK_ACCENT)
    }
    #[cfg(not(windows))]
    {
        FALLBACK_ACCENT
    }
}

// `WECTANGLE_THEME=light|dark` overrides the system theme, so both themes
// can be exercised without changing Windows settings. Checked first on
// every platform, since it needs no OS call.
pub fn current_mode() -> Mode {
    if let Ok(v) = std::env::var("WECTANGLE_THEME") {
        match v.to_ascii_lowercase().as_str() {
            "light" => return Mode::Light,
            "dark" => return Mode::Dark,
            _ => {}
        }
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

/// Sets the window's title bar to dark or light immersive mode.
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

    #[test]
    fn light_and_dark_backgrounds_differ() {
        assert_ne!(light().bg, dark().bg);
    }

    #[test]
    fn dark_background_is_darker_than_light_background() {
        let luminance = |c: Rgb| c.r as u32 + c.g as u32 + c.b as u32;
        assert!(luminance(dark().bg) < luminance(light().bg));
    }

    #[test]
    fn dark_text_is_lighter_than_dark_background() {
        let p = dark();
        let luminance = |c: Rgb| c.r as u32 + c.g as u32 + c.b as u32;
        assert!(luminance(p.text) > luminance(p.bg));
    }

    #[test]
    fn light_text_is_darker_than_light_background() {
        let p = light();
        let luminance = |c: Rgb| c.r as u32 + c.g as u32 + c.b as u32;
        assert!(luminance(p.text) < luminance(p.bg));
    }

    #[test]
    fn surface_hover_differs_from_surface() {
        assert_ne!(light().surface, light().surface_hover);
        assert_ne!(dark().surface, dark().surface_hover);
    }

    #[test]
    fn accent_color_parses_abgr_dword() {
        // #0078D4 packed as ABGR (alpha 0xFF, blue 0xD4, green 0x78, red 0x00).
        assert_eq!(accent_color_from_abgr(0xFFD47800), FALLBACK_ACCENT);
    }

    #[test]
    fn palette_selects_by_mode() {
        assert_eq!(palette(Mode::Light), light());
        assert_eq!(palette(Mode::Dark), dark());
    }

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
        assert_eq!(current_mode(), Mode::Light);

        unsafe {
            std::env::remove_var("WECTANGLE_THEME");
        }
    }
}
