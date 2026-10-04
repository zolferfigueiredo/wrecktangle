use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, MAPVK_VK_TO_VSC, MapVirtualKeyExW, ToUnicodeEx, VK_CONTROL, VK_MENU,
};
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

pub fn format(shortcut: &Shortcut) -> String {
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
        parts.push("Win".to_string());
    }
    parts.push(lookup_name(shortcut.vk).unwrap_or_else(|| format!("VK_{:#X}", shortcut.vk)));
    parts.join("+")
}

// On the US-International layout, Ctrl+Alt is the same chord as AltGr, so a
// Ctrl+Alt+<key> hotkey can block that key from typing its AltGr character
// anywhere else. wFlags 0x4 keeps ToUnicodeEx from disturbing dead-key state.
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
}
