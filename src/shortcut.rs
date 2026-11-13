#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, HOT_KEY_MODIFIERS, MAPVK_VK_TO_VSC, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN, MapVirtualKeyExW, ToUnicodeEx, VK_CONTROL, VK_MENU,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseShortcutError;

impl std::fmt::Display for ParseShortcutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not a valid shortcut")
    }
}

impl std::error::Error for ParseShortcutError {}

fn key_name_table() -> Vec<(String, u16)> {
    let mut table = Vec::with_capacity(96);

    for c in b'A'..=b'Z' {
        table.push(((c as char).to_string(), c as u16));
    }
    for c in b'0'..=b'9' {
        table.push(((c as char).to_string(), c as u16));
    }
    for i in 1..=24u16 {
        table.push((format!("F{i}"), 0x6F + i));
    }
    for i in 0..=9u16 {
        table.push((format!("Numpad{i}"), 0x60 + i));
    }

    table.extend([
        ("Left".to_string(), 0x25),
        ("Up".to_string(), 0x26),
        ("Right".to_string(), 0x27),
        ("Down".to_string(), 0x28),
        ("Enter".to_string(), 0x0D),
        ("Space".to_string(), 0x20),
        ("Tab".to_string(), 0x09),
        ("Backspace".to_string(), 0x08),
        ("Delete".to_string(), 0x2E),
        ("Insert".to_string(), 0x2D),
        ("Home".to_string(), 0x24),
        ("End".to_string(), 0x23),
        ("PageUp".to_string(), 0x21),
        ("PageDown".to_string(), 0x22),
        (";".to_string(), 0xBA),
        ("=".to_string(), 0xBB),
        (",".to_string(), 0xBC),
        ("-".to_string(), 0xBD),
        (".".to_string(), 0xBE),
        ("/".to_string(), 0xBF),
        ("`".to_string(), 0xC0),
        ("[".to_string(), 0xDB),
        ("\\".to_string(), 0xDC),
        ("]".to_string(), 0xDD),
        ("'".to_string(), 0xDE),
    ]);

    table
}

fn lookup_vk(token: &str) -> Option<u16> {
    key_name_table()
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(token))
        .map(|(_, vk)| vk)
}

fn lookup_name(vk: u16) -> Option<String> {
    key_name_table()
        .into_iter()
        .find(|(_, v)| *v == vk)
        .map(|(name, _)| name)
}

pub fn parse(input: &str) -> Result<Shortcut, ParseShortcutError> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut win = false;
    let mut vk: Option<u16> = None;

    for token in input.split('+') {
        let token = token.trim();
        if token.is_empty() {
            return Err(ParseShortcutError);
        }
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            "win" | "windows" | "super" => win = true,
            _ => {
                if vk.is_some() {
                    return Err(ParseShortcutError);
                }
                vk = Some(lookup_vk(token).ok_or(ParseShortcutError)?);
            }
        }
    }

    let vk = vk.ok_or(ParseShortcutError)?;
    if !(ctrl || alt || win) {
        return Err(ParseShortcutError);
    }

    Ok(Shortcut {
        ctrl,
        alt,
        shift,
        win,
        vk,
    })
}

/// What the Win key is called on this system. Only display text uses it:
/// `format` always writes "Win", so a config.json works on both systems.
pub const WIN_LABEL: &str = if cfg!(target_os = "linux") {
    "Super"
} else {
    "Win"
};

fn parts(shortcut: &Shortcut, win_label: &str) -> Vec<String> {
    let mut parts = Vec::with_capacity(5);
    if shortcut.ctrl {
        parts.push("Ctrl".to_string());
    }
    if shortcut.alt {
        parts.push("Alt".to_string());
    }
    if shortcut.shift {
        parts.push("Shift".to_string());
    }
    if shortcut.win {
        parts.push(win_label.to_string());
    }
    parts.push(key_name(shortcut));
    parts
}

/// The shortcut as config.json stores it.
pub fn format(shortcut: &Shortcut) -> String {
    parts(shortcut, "Win").join("+")
}

/// The shortcut as Settings and notifications show it.
pub fn display(shortcut: &Shortcut) -> String {
    parts(shortcut, WIN_LABEL).join("+")
}

pub fn key_caps(shortcut: &Shortcut) -> Vec<String> {
    let mut caps = parts(shortcut, WIN_LABEL);
    let arrow = match shortcut.vk {
        0x25 => Some("\u{2190}"),
        0x26 => Some("\u{2191}"),
        0x27 => Some("\u{2192}"),
        0x28 => Some("\u{2193}"),
        _ => None,
    };
    if let (Some(arrow), Some(last)) = (arrow, caps.last_mut()) {
        *last = arrow.to_string();
    }
    caps
}

pub fn key_name(shortcut: &Shortcut) -> String {
    lookup_name(shortcut.vk).unwrap_or_else(|| format!("VK_{:#X}", shortcut.vk))
}

/// X11 keysyms for the keys a shortcut can use. `Shortcut::vk` stays the
/// key's id on every system, so config.json is the same everywhere; X11
/// grabs and key events go through this table.
#[cfg(any(not(windows), test))]
pub mod keysym {
    fn table() -> Vec<(u16, u32)> {
        let mut table = Vec::with_capacity(96);
        // Letters map to the lowercase keysym an unshifted key reports.
        for i in 0..26u16 {
            table.push((0x41 + i, 0x61 + u32::from(i)));
        }
        for i in 0..10u16 {
            table.push((0x30 + i, 0x30 + u32::from(i)));
        }
        for i in 0..24u16 {
            table.push((0x70 + i, 0xFFBE + u32::from(i)));
        }
        for i in 0..10u16 {
            table.push((0x60 + i, 0xFFB0 + u32::from(i)));
        }
        table.extend([
            (0x25, 0xFF51),
            (0x26, 0xFF52),
            (0x27, 0xFF53),
            (0x28, 0xFF54),
            (0x0D, 0xFF0D),
            (0x20, 0x0020),
            (0x09, 0xFF09),
            (0x08, 0xFF08),
            (0x2E, 0xFFFF),
            (0x2D, 0xFF63),
            (0x24, 0xFF50),
            (0x23, 0xFF57),
            (0x21, 0xFF55),
            (0x22, 0xFF56),
            (0xBA, 0x003B),
            (0xBB, 0x003D),
            (0xBC, 0x002C),
            (0xBD, 0x002D),
            (0xBE, 0x002E),
            (0xBF, 0x002F),
            (0xC0, 0x0060),
            (0xDB, 0x005B),
            (0xDC, 0x005C),
            (0xDD, 0x005D),
            (0xDE, 0x0027),
        ]);
        table
    }

    pub fn from_vk(vk: u16) -> Option<u32> {
        table().into_iter().find(|(v, _)| *v == vk).map(|(_, k)| k)
    }

    pub fn to_vk(keysym: u32) -> Option<u16> {
        let keysym = match keysym {
            // Uppercase letters, as a key reports them with Shift held.
            0x41..=0x5A => keysym + 0x20,
            // Shift+Tab.
            0xFE20 => 0xFF09,
            // The keypad with NumLock off: KP_Home 7, KP_Left 4, KP_Up 8,
            // KP_Right 6, KP_Down 2, KP_Prior 9, KP_Next 3, KP_End 1,
            // KP_Begin 5, KP_Insert 0.
            0xFF95..=0xFF9E => {
                const DIGITS: [u32; 10] = [7, 4, 8, 6, 2, 9, 3, 1, 5, 0];
                0xFFB0 + DIGITS[(keysym - 0xFF95) as usize]
            }
            other => other,
        };
        table()
            .into_iter()
            .find(|(_, k)| *k == keysym)
            .map(|(v, _)| v)
    }
}

#[cfg(windows)]
pub fn hotkey_modifiers(shortcut: &Shortcut) -> HOT_KEY_MODIFIERS {
    let mut m = MOD_NOREPEAT;
    if shortcut.ctrl {
        m |= MOD_CONTROL;
    }
    if shortcut.alt {
        m |= MOD_ALT;
    }
    if shortcut.shift {
        m |= MOD_SHIFT;
    }
    if shortcut.win {
        m |= MOD_WIN;
    }
    m
}

// On the US-International layout, Ctrl+Alt is the same chord as AltGr, so a
// Ctrl+Alt+<key> hotkey can block that key from typing its AltGr character
// anywhere else. wFlags 0x4 keeps ToUnicodeEx from disturbing dead-key state.
#[cfg(windows)]
pub fn altgr_char(shortcut: &Shortcut) -> Option<char> {
    if !(shortcut.ctrl && shortcut.alt) || shortcut.shift || shortcut.win {
        return None;
    }
    unsafe {
        let foreground = GetForegroundWindow();
        let thread_id = GetWindowThreadProcessId(foreground, None);
        let layout = GetKeyboardLayout(thread_id);

        let mut state = [0u8; 256];
        state[VK_CONTROL.0 as usize] = 0x80;
        state[VK_MENU.0 as usize] = 0x80;

        let scan = MapVirtualKeyExW(shortcut.vk as u32, MAPVK_VK_TO_VSC, Some(layout));
        let mut buf = [0u16; 8];
        let result = ToUnicodeEx(
            shortcut.vk as u32,
            scan,
            &state,
            &mut buf,
            0x4,
            Some(layout),
        );
        if result >= 1 {
            char::from_u32(buf[0] as u32)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_left_shortcut() {
        let s = parse("Ctrl+Alt+Left").unwrap();
        assert!(s.ctrl);
        assert!(s.alt);
        assert!(!s.shift);
        assert!(!s.win);
        assert_eq!(s.vk, 0x25);
    }

    #[test]
    fn parse_is_case_insensitive() {
        assert_eq!(parse("ctrl+alt+left"), parse("CTRL+ALT+LEFT"));
    }

    #[test]
    fn format_uses_canonical_modifier_order() {
        let s = parse("Win+Shift+Alt+Ctrl+Right").unwrap();
        assert_eq!(format(&s), "Ctrl+Alt+Shift+Win+Right");
    }

    #[test]
    fn key_caps_split_modifiers_and_draw_arrows() {
        let s = parse("Ctrl+Alt+Win+Left").unwrap();
        assert_eq!(key_caps(&s), ["Ctrl", "Alt", WIN_LABEL, "\u{2190}"]);
        let s = parse("Ctrl+Alt+Home").unwrap();
        assert_eq!(key_caps(&s), ["Ctrl", "Alt", "Home"]);
    }

    #[test]
    fn parse_format_round_trip() {
        for input in [
            "Ctrl+Alt+Left",
            "Ctrl+Alt+Right",
            "Ctrl+Alt+Up",
            "Ctrl+Alt+Down",
            "Ctrl+Alt+F",
            "Ctrl+Alt+G",
            "Ctrl+Alt+V",
            "Ctrl+Alt+B",
            "Ctrl+Alt+Enter",
            "Ctrl+Alt+Space",
            "Ctrl+Alt+Win+Right",
            "Ctrl+Alt+Win+Left",
            "Alt+Win+F13",
            "Ctrl+Numpad5",
            "Ctrl+Alt+;",
        ] {
            let parsed = parse(input).unwrap();
            let formatted = format(&parsed);
            let reparsed = parse(&formatted).unwrap();
            assert_eq!(parsed, reparsed, "round trip for {input}");
        }
    }

    #[test]
    fn rejects_combo_without_ctrl_alt_or_win() {
        assert!(parse("Left").is_err());
        assert!(parse("Shift+Left").is_err());
    }

    #[test]
    fn rejects_two_key_tokens() {
        assert!(parse("Ctrl+Left+Right").is_err());
    }

    #[test]
    fn rejects_unknown_key_name() {
        assert!(parse("Ctrl+Alt+NotAKey").is_err());
    }

    #[test]
    fn rejects_empty_and_modifier_only_input() {
        assert!(parse("").is_err());
        assert!(parse("Ctrl+Alt").is_err());
    }

    #[test]
    fn parses_function_and_numpad_keys() {
        assert_eq!(parse("Ctrl+F13").unwrap().vk, 0x7C);
        assert_eq!(parse("Ctrl+F24").unwrap().vk, 0x87);
        assert_eq!(parse("Alt+Numpad5").unwrap().vk, 0x65);
    }

    #[test]
    fn parses_oem_punctuation() {
        assert_eq!(parse("Ctrl+Alt+;").unwrap().vk, 0xBA);
        assert_eq!(parse("Ctrl+Alt+,").unwrap().vk, 0xBC);
    }

    #[test]
    fn display_names_the_win_key_for_this_system_but_config_keeps_win() {
        let s = parse("Ctrl+Super+Left").unwrap();
        assert_eq!(format(&s), "Ctrl+Win+Left");
        assert_eq!(display(&s), format!("Ctrl+{WIN_LABEL}+Left"));
    }

    #[test]
    fn every_named_key_has_a_keysym_that_maps_back() {
        for (name, vk) in key_name_table() {
            let sym = keysym::from_vk(vk).unwrap_or_else(|| panic!("{name} has no keysym"));
            assert_eq!(keysym::to_vk(sym), Some(vk), "{name}");
        }
    }

    #[test]
    fn keysyms_of_common_keys() {
        assert_eq!(keysym::from_vk(0x25), Some(0xFF51));
        assert_eq!(keysym::from_vk(0x46), Some(0x66));
        assert_eq!(keysym::from_vk(0x0D), Some(0xFF0D));
        assert_eq!(keysym::from_vk(0x87), Some(0xFFD5));
        assert_eq!(keysym::from_vk(0xFF), None);
    }

    #[test]
    fn shifted_and_keypad_keysyms_map_to_their_keys() {
        assert_eq!(keysym::to_vk(0x46), Some(0x46));
        assert_eq!(keysym::to_vk(0xFE20), Some(0x09));
        assert_eq!(keysym::to_vk(0xFF99), Some(0x62));
        assert_eq!(keysym::to_vk(0xFF9E), Some(0x60));
        assert_eq!(keysym::to_vk(0xFF95), Some(0x67));
        assert_eq!(keysym::to_vk(0xFFE9), None);
    }
}
