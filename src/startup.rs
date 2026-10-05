use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ, RegCloseKey, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{Error, HRESULT, PCWSTR, w};

const RUN_KEY_PATH: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("Wrecktangle");
// Written by the app under its earlier name, Wectangle.
const LEGACY_VALUE_NAME: PCWSTR = w!("Wectangle");

fn check(err: WIN32_ERROR) -> windows::core::Result<()> {
    if err.0 == 0 {
        Ok(())
    } else {
        Err(Error::from_hresult(HRESULT::from_win32(err.0)))
    }
}

fn current_exe_quoted() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\"", exe.display()))
}

pub fn is_enabled() -> bool {
    unsafe {
        let mut hkey = HKEY::default();
        if check(RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY_PATH,
            Some(0),
            KEY_READ,
            &mut hkey,
        ))
        .is_err()
        {
            return false;
        }
        let mut buf = [0u8; 1024];
        let mut len = buf.len() as u32;
        let found = check(RegQueryValueExW(
            hkey,
            VALUE_NAME,
            None,
            None,
            Some(buf.as_mut_ptr()),
            Some(&mut len),
        ))
        .is_ok();
        let _ = RegCloseKey(hkey);
        found
    }
}

pub fn set_enabled(enabled: bool) -> windows::core::Result<()> {
    unsafe {
        let mut hkey = HKEY::default();
        check(RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY_PATH,
            Some(0),
            KEY_WRITE,
            &mut hkey,
        ))?;

        let result = if enabled {
            match current_exe_quoted() {
                Some(path) => {
                    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
                    let bytes =
                        std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
                    check(RegSetValueExW(
                        hkey,
                        VALUE_NAME,
                        Some(0),
                        REG_SZ,
                        Some(bytes),
                    ))
                }
                None => Ok(()),
            }
        } else {
            let err = RegDeleteValueW(hkey, VALUE_NAME);
            if err.0 == 0 || err.0 == ERROR_FILE_NOT_FOUND.0 {
                Ok(())
            } else {
                check(err)
            }
        };

        let _ = RegCloseKey(hkey);
        result
    }
}

/// Replaces a launch-at-startup entry left under the old name with one for
/// this exe.
pub fn migrate_legacy() {
    unsafe {
        let mut hkey = HKEY::default();
        if check(RegOpenKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY_PATH,
            Some(0),
            KEY_WRITE,
            &mut hkey,
        ))
        .is_err()
        {
            return;
        }
        let had_legacy = RegDeleteValueW(hkey, LEGACY_VALUE_NAME).0 == 0;
        let _ = RegCloseKey(hkey);
        if had_legacy {
            let _ = set_enabled(true);
        }
    }
}

pub fn toggle() -> windows::core::Result<bool> {
    let enabled = !is_enabled();
    set_enabled(enabled)?;
    Ok(enabled)
}
