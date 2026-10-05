//! Names the app that most likely owns a shortcut Wectangle had to take
//! over. Sources are read-only and limited to hotkey settings: other keys in
//! those files (tokens, device keys) are never deserialized or kept.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Map, Value};

#[cfg(windows)]
use windows::Win32::Foundation::CloseHandle;
#[cfg(windows)]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
#[cfg(windows)]
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW,
};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::VkKeyScanW;
#[cfg(windows)]
use windows::core::{PWSTR, w};

use crate::lang;
use crate::shortcut::Shortcut;

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAGNIFIER_EXE: &str = "magnify.exe";
const CLAUDE_EXE: &str = "claude.exe";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnerName {
    Windows(&'static str),
    Product(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    pub name: OwnerName,
    pub certain: bool,
}

impl Owner {
    pub fn note(&self) -> String {
        let name = match &self.name {
            OwnerName::Windows(key) => lang::t(key),
            OwnerName::Product(name) => name.clone(),
        };
        let key = if self.certain {
            "status.overrides"
        } else {
            "status.overrides_probably"
        };
        lang::format(key, &[("owner", &name)])
    }
}

pub type KeyLookup<'a> = &'a dyn Fn(char) -> Option<(u16, bool)>;

#[derive(Debug, Default)]
struct Known {
    apps: Vec<(Shortcut, String)>,
    claude_default_possible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudePref {
    Absent,
    Off,
    Accelerator(String),
}

pub fn find_owner(sc: &Shortcut) -> Option<Owner> {
    resolve(sc, &load_known(), &process_running)
}

fn resolve(sc: &Shortcut, known: &Known, running: &dyn Fn(&str) -> bool) -> Option<Owner> {
    if let Some((_, name)) = known.apps.iter().find(|(s, _)| s == sc) {
        return Some(Owner {
            name: OwnerName::Product(name.clone()),
            certain: true,
        });
    }
    if let Some(key) = windows_builtin(sc) {
        return Some(Owner {
            name: OwnerName::Windows(key),
            certain: true,
        });
    }
    if is_magnifier_chord(sc) && running(MAGNIFIER_EXE) {
        return Some(Owner {
            name: OwnerName::Windows("owner.magnifier"),
            certain: false,
        });
    }
    if known.claude_default_possible && is_claude_default(sc) && running(CLAUDE_EXE) {
        return Some(Owner {
            name: OwnerName::Product("Claude".to_string()),
            certain: false,
        });
    }
    None
}

fn is_claude_default(sc: &Shortcut) -> bool {
    sc.ctrl && sc.alt && !sc.shift && !sc.win && sc.vk == 0x20
}

fn is_magnifier_chord(sc: &Shortcut) -> bool {
    if !sc.ctrl || !sc.alt || sc.shift || sc.win {
        return false;
    }
    matches!(sc.vk, 0x25..=0x28 | 0x20 | 0x0D)
        || u8::try_from(sc.vk).is_ok_and(|v| b"FDLMIR".contains(&v))
}

pub fn windows_builtin(sc: &Shortcut) -> Option<&'static str> {
    if !sc.win {
        return None;
    }
    let arrow = (0x25..=0x28).contains(&sc.vk);
    match (sc.ctrl, sc.alt, sc.shift) {
        (true, true, true) => Some("owner.office_key"),
        (false, false, false) => {
            if arrow {
                Some("owner.snap")
            } else {
                win_key_name(sc.vk)
            }
        }
        (false, true, false) | (false, false, true) if arrow => Some("owner.window_management"),
        (false, false, true) => match sc.vk {
            0x53 => Some("owner.snipping_tool"),
            0x4D => Some("owner.restore_minimized"),
            _ => None,
        },
        (true, false, false) => match sc.vk {
            0x25 | 0x27 | 0x44 | 0x73 => Some("owner.virtual_desktops"),
            0x43 => Some("owner.color_filters"),
            0x51 => Some("owner.quick_assist"),
            0x0D => Some("owner.narrator"),
            _ => None,
        },
        (false, true, false) => match sc.vk {
            0x52 | 0x47 | 0x2C => Some("owner.game_bar"),
            0x42 => Some("owner.hdr_toggle"),
            _ => None,
        },
        (true, false, true) => match sc.vk {
            0x42 => Some("owner.graphics_reset"),
            _ => None,
        },
        _ => None,
    }
}

fn win_key_name(vk: u16) -> Option<&'static str> {
    Some(match vk {
        0x41 => "owner.quick_settings",
        0x42 => "owner.notification_area",
        0x44 => "owner.show_desktop",
        0x45 => "owner.file_explorer",
        0x46 => "owner.feedback_hub",
        0x47 => "owner.game_bar",
        0x48 => "owner.voice_typing",
        0x49 => "owner.settings",
        0x4B => "owner.cast",
        0x4D => "owner.minimize_all",
        0x4E => "owner.notification_center",
        0x4F => "owner.orientation_lock",
        0x50 => "owner.project",
        0x51 | 0x53 => "owner.search",
        0x52 => "owner.run",
        0x54 => "owner.taskbar",
        0x55 => "owner.accessibility",
        0x56 => "owner.clipboard_history",
        0x57 => "owner.widgets",
        0x58 => "owner.quick_link_menu",
        0x5A => "owner.snap_layouts",
        0x20 => "owner.input_switcher",
        0x09 => "owner.task_view",
        0xBC => "owner.desktop_peek",
        0xBE | 0xBA => "owner.emoji_panel",
        0x24 => "owner.minimize_others",
        0x31..=0x39 => "owner.taskbar_apps",
        0xBB | 0xBD | 0x1B => "owner.magnifier",
        0x0D => "owner.narrator",
        0x2C => "owner.screenshot",
        0x13 => "owner.system_information",
        _ => return None,
    })
}

fn is_hotkey_object(map: &Map<String, Value>) -> bool {
    ["win", "ctrl", "alt", "shift"]
        .iter()
        .all(|k| map.get(*k).is_some_and(Value::is_boolean))
        && map.get("code").is_some_and(Value::is_number)
}

fn hotkey_from_object(map: &Map<String, Value>) -> Option<Shortcut> {
    let flag = |k: &str| map.get(k).and_then(Value::as_bool).unwrap_or(false);
    let code = map.get("code").and_then(Value::as_u64)?;
    if !(1..=0xFF).contains(&code) {
        return None;
    }
    Some(Shortcut {
        ctrl: flag("ctrl"),
        alt: flag("alt"),
        shift: flag("shift"),
        win: flag("win"),
        vk: code as u16,
    })
}

// Keys starting with "Default" hold a module's factory shortcut, not the
// one in effect.
fn collect_hotkeys(value: &Value, out: &mut Vec<Shortcut>) {
    match value {
        Value::Object(map) if is_hotkey_object(map) => out.extend(hotkey_from_object(map)),
        Value::Object(map) => {
            for (key, child) in map {
                if !key.starts_with("Default") {
                    collect_hotkeys(child, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_hotkeys(child, out);
            }
        }
        _ => {}
    }
}

pub fn powertoys_enabled(json: &str) -> Vec<String> {
    #[derive(Deserialize)]
    struct Global {
        #[serde(default)]
        enabled: HashMap<String, Value>,
    }
    let Ok(global) = serde_json::from_str::<Global>(json) else {
        return Vec::new();
    };
    let mut modules: Vec<String> = global
        .enabled
        .into_iter()
        .filter(|(_, v)| v.as_bool() == Some(true))
        .map(|(name, _)| name)
        .collect();
    modules.sort();
    modules
}

pub fn powertoys_hotkeys(json: &str) -> Vec<Shortcut> {
    let mut out = Vec::new();
    if let Ok(value) = serde_json::from_str::<Value>(json) {
        collect_hotkeys(&value, &mut out);
    }
    out
}

pub fn command_palette_hotkey(json: &str) -> Option<Shortcut> {
    #[derive(Deserialize)]
    struct File {
        #[serde(rename = "Hotkey")]
        hotkey: Option<Value>,
    }
    let file: File = serde_json::from_str(json).ok()?;
    match file.hotkey? {
        Value::Object(map) if is_hotkey_object(&map) => hotkey_from_object(&map),
        _ => None,
    }
}

pub fn powertoys_name(module: &str) -> String {
    let spaced = if module.contains(' ') || module == "FancyZones" {
        module.to_string()
    } else {
        let mut out = String::with_capacity(module.len() + 4);
        let mut previous: Option<char> = None;
        for c in module.chars() {
            if c.is_ascii_uppercase() && previous.is_some_and(|p| !p.is_ascii_uppercase()) {
                out.push(' ');
            }
            out.push(c);
            previous = Some(c);
        }
        out
    };
    if spaced.starts_with("PowerToys") {
        spaced
    } else {
        format!("PowerToys {spaced}")
    }
}

fn accelerator_key(token: &str, lookup: KeyLookup) -> Option<(u16, bool)> {
    let mut chars = token.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some((c.to_ascii_uppercase() as u16, false));
        }
        return lookup(c);
    }

    let lower = token.to_ascii_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&n)
    {
        return Some((0x6F + n, false));
    }
    if let Some(n) = lower
        .strip_prefix("num")
        .and_then(|n| n.parse::<u16>().ok())
        && n <= 9
    {
        return Some((0x60 + n, false));
    }
    let vk = match lower.as_str() {
        "plus" => return lookup('+'),
        "space" => 0x20,
        "tab" => 0x09,
        "capslock" => 0x14,
        "numlock" => 0x90,
        "scrolllock" => 0x91,
        "backspace" => 0x08,
        "delete" => 0x2E,
        "insert" => 0x2D,
        "return" | "enter" => 0x0D,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "escape" | "esc" => 0x1B,
        "printscreen" => 0x2C,
        "numdec" => 0x6E,
        "numadd" => 0x6B,
        "numsub" => 0x6D,
        "nummult" => 0x6A,
        "numdiv" => 0x6F,
        _ => return None,
    };
    Some((vk, false))
}

/// Parses an Electron accelerator such as `Alt+Shift+)`. A shifted symbol
/// implies Shift, so `lookup` maps a character to its key and whether the
/// layout needs Shift to type it.
pub fn parse_accelerator(accelerator: &str, lookup: KeyLookup) -> Option<Shortcut> {
    let mut sc = Shortcut {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        vk: 0,
    };
    let mut rest = accelerator.trim();
    while let Some((head, tail)) = rest.split_once('+') {
        match head.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "commandorcontrol" | "cmdorctrl" => sc.ctrl = true,
            "alt" | "option" => sc.alt = true,
            "altgr" => {
                sc.ctrl = true;
                sc.alt = true;
            }
            "shift" => sc.shift = true,
            "super" | "meta" | "win" | "windows" | "cmd" | "command" => sc.win = true,
            _ => break,
        }
        rest = tail;
    }
    if rest.is_empty() {
        return None;
    }
    let (vk, needs_shift) = accelerator_key(rest.trim(), lookup)?;
    sc.vk = vk;
    sc.shift |= needs_shift;
    Some(sc)
}

pub fn twinkle_tray_hotkeys(json: &str, lookup: KeyLookup) -> Vec<Shortcut> {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        hotkeys: Vec<Value>,
    }
    let Ok(file) = serde_json::from_str::<File>(json) else {
        return Vec::new();
    };
    file.hotkeys
        .iter()
        .filter(|h| h.get("active").and_then(Value::as_bool) == Some(true))
        .filter_map(|h| h.get("accelerator").and_then(Value::as_str))
        .filter_map(|accelerator| parse_accelerator(accelerator, lookup))
        .collect()
}

fn shortcut_from_vks(vks: &[u32]) -> Option<Shortcut> {
    let mut sc = Shortcut {
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
        vk: 0,
    };
    let mut key = None;
    for &vk in vks {
        match vk {
            0x10 | 0xA0 | 0xA1 => sc.shift = true,
            0x11 | 0xA2 | 0xA3 => sc.ctrl = true,
            0x12 | 0xA4 | 0xA5 => sc.alt = true,
            0x5B | 0x5C => sc.win = true,
            1..=0xFF if key.is_none() => key = Some(vk as u16),
            _ => return None,
        }
    }
    sc.vk = key?;
    Some(sc)
}

/// `values` holds the DWORD at the start of each hotkey-related binary value
/// under the NVIDIA overlay key. A hotkey is `<Name>Count` plus
/// `<Name>0..Count-1`, one virtual key per value.
pub fn nvidia_hotkeys(values: &HashMap<String, u32>) -> Vec<Shortcut> {
    let enabled = values.get("IsShadowPlayEnabled") == Some(&1)
        && values
            .get("IsShadowPlayEnabledUser")
            .is_none_or(|v| *v == 1);
    if !enabled {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (name, &count) in values {
        let Some(prefix) = name.strip_suffix("Count") else {
            continue;
        };
        if !prefix.contains("HKey") || count == 0 || count > 8 {
            continue;
        }
        let vks: Option<Vec<u32>> = (0..count)
            .map(|i| values.get(&format!("{prefix}{i}")).copied())
            .collect();
        if let Some(sc) = vks.and_then(|vks| shortcut_from_vks(&vks)) {
            out.push(sc);
        }
    }
    out
}

/// Reads only `preferences.quickEntryShortcut`; the rest of the file can hold
/// tokens and is skipped by the deserializer.
pub fn parse_claude_quick_entry(json: &str) -> ClaudePref {
    #[derive(Deserialize)]
    struct File {
        preferences: Option<Preferences>,
    }
    #[derive(Deserialize)]
    struct Preferences {
        #[serde(rename = "quickEntryShortcut")]
        quick_entry: Option<Value>,
    }
    let Ok(file) = serde_json::from_str::<File>(json) else {
        return ClaudePref::Absent;
    };
    let Some(value) = file.preferences.and_then(|p| p.quick_entry) else {
        return ClaudePref::Absent;
    };
    let accelerator = match &value {
        Value::String(s) => Some(s.as_str()),
        Value::Object(map) => map.get("accelerator").and_then(Value::as_str),
        _ => None,
    };
    match accelerator {
        Some(s) if s.eq_ignore_ascii_case("off") => ClaudePref::Off,
        Some(s) if !s.trim().is_empty() => ClaudePref::Accelerator(s.to_string()),
        _ => ClaudePref::Absent,
    }
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn package_dirs(prefix: &str, contains: bool) -> Vec<PathBuf> {
    let Some(local) = env_dir("LOCALAPPDATA") else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(local.join("Packages")) else {
        return Vec::new();
    };
    let needle = prefix.to_ascii_lowercase();
    entries
        .filter_map(Result::ok)
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_ascii_lowercase();
            if contains {
                name.contains(&needle)
            } else {
                name.starts_with(&needle)
            }
        })
        .map(|e| e.path())
        .collect()
}

fn load_powertoys(apps: &mut Vec<(Shortcut, String)>) {
    let Some(local) = env_dir("LOCALAPPDATA") else {
        return;
    };
    let root = local.join("Microsoft").join("PowerToys");
    let Some(global) = read_text(&root.join("settings.json")) else {
        return;
    };
    for module in powertoys_enabled(&global) {
        if module.contains(['/', '\\']) || module.contains("..") {
            continue;
        }
        if module == "CmdPal" {
            let file = local
                .join("Packages")
                .join("Microsoft.CommandPalette_8wekyb3d8bbwe")
                .join("LocalState")
                .join("settings.json");
            if let Some(sc) = read_text(&file).and_then(|t| command_palette_hotkey(&t)) {
                apps.push((sc, "PowerToys Command Palette".to_string()));
            }
            continue;
        }
        let Some(text) = read_text(&root.join(&module).join("settings.json")) else {
            continue;
        };
        let name = powertoys_name(&module);
        apps.extend(
            powertoys_hotkeys(&text)
                .into_iter()
                .map(|sc| (sc, name.clone())),
        );
    }
}

fn load_twinkle_tray(apps: &mut Vec<(Shortcut, String)>) {
    let mut files = Vec::new();
    for dir in package_dirs("twinkletray", true) {
        let roaming = dir.join("LocalCache").join("Roaming");
        files.push(roaming.join("twinkle-tray-appx").join("settings.json"));
        files.push(roaming.join("twinkle-tray").join("settings.json"));
    }
    if let Some(roaming) = env_dir("APPDATA") {
        files.push(roaming.join("twinkle-tray").join("settings.json"));
    }
    for file in files {
        if let Some(text) = read_text(&file) {
            apps.extend(
                twinkle_tray_hotkeys(&text, &system_key_lookup)
                    .into_iter()
                    .map(|sc| (sc, "Twinkle Tray".to_string())),
            );
            return;
        }
    }
}

// Returns whether Claude's default Quick Entry shortcut can still be in
// effect, that is, the preference is not set to anything else.
fn load_claude(apps: &mut Vec<(Shortcut, String)>) -> bool {
    let mut files: Vec<PathBuf> = package_dirs("claude_", false)
        .into_iter()
        .map(|dir| {
            dir.join("LocalCache")
                .join("Roaming")
                .join("Claude")
                .join("claude_desktop_config.json")
        })
        .collect();
    if let Some(roaming) = env_dir("APPDATA") {
        files.push(roaming.join("Claude").join("claude_desktop_config.json"));
    }
    for file in files {
        let Some(text) = read_text(&file) else {
            continue;
        };
        match parse_claude_quick_entry(&text) {
            ClaudePref::Absent => continue,
            ClaudePref::Off => return false,
            ClaudePref::Accelerator(accelerator) => {
                if let Some(sc) = parse_accelerator(&accelerator, &system_key_lookup) {
                    apps.push((sc, "Claude".to_string()));
                }
                return false;
            }
        }
    }
    true
}

fn load_known() -> Known {
    let mut apps = Vec::new();
    load_powertoys(&mut apps);
    load_twinkle_tray(&mut apps);
    let nvidia = nvidia_values();
    apps.extend(
        nvidia_hotkeys(&nvidia)
            .into_iter()
            .map(|sc| (sc, "NVIDIA overlay".to_string())),
    );
    let claude_default_possible = load_claude(&mut apps);
    Known {
        apps,
        claude_default_possible,
    }
}

#[cfg(windows)]
fn system_key_lookup(c: char) -> Option<(u16, bool)> {
    let mut buf = [0u16; 2];
    let units = c.encode_utf16(&mut buf);
    if units.len() != 1 {
        return None;
    }
    let result = unsafe { VkKeyScanW(units[0]) };
    if result == -1 {
        return None;
    }
    let vk = (result & 0xFF) as u16;
    let state = ((result >> 8) & 0xFF) as u8;
    if state & !1 != 0 {
        return None;
    }
    Some((vk, state & 1 != 0))
}

#[cfg(not(windows))]
fn system_key_lookup(_c: char) -> Option<(u16, bool)> {
    None
}

#[cfg(windows)]
fn nvidia_values() -> HashMap<String, u32> {
    const KEY: windows::core::PCWSTR =
        w!("Software\\NVIDIA Corporation\\Global\\ShadowPlay\\NVSPCAPS");
    const ERROR_MORE_DATA: u32 = 234;
    const MAX_VALUES: u32 = 4000;

    let mut values = HashMap::new();
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, KEY, Some(0), KEY_READ, &mut hkey).0 != 0 {
            return values;
        }
        for index in 0..MAX_VALUES {
            let mut name = [0u16; 256];
            let mut name_len = name.len() as u32;
            let status = RegEnumValueW(
                hkey,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut name_len,
                None,
                None,
                None,
                None,
            );
            if status.0 == ERROR_MORE_DATA {
                continue;
            }
            if status.0 != 0 {
                break;
            }
            let name_text = String::from_utf16_lossy(&name[..name_len as usize]);
            if !name_text.contains("HKey") && !name_text.starts_with("IsShadowPlayEnabled") {
                continue;
            }
            let mut data = [0u8; 4];
            let mut data_len = data.len() as u32;
            let status = RegQueryValueExW(
                hkey,
                windows::core::PCWSTR(name.as_ptr()),
                None,
                None,
                Some(data.as_mut_ptr()),
                Some(&mut data_len),
            );
            if status.0 == 0 && data_len == 4 {
                values.insert(name_text, u32::from_le_bytes(data));
            }
        }
        let _ = RegCloseKey(hkey);
    }
    values
}

#[cfg(not(windows))]
fn nvidia_values() -> HashMap<String, u32> {
    HashMap::new()
}

#[cfg(windows)]
fn process_running(exe: &str) -> bool {
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = false;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                if String::from_utf16_lossy(&entry.szExeFile[..len]).eq_ignore_ascii_case(exe) {
                    found = true;
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

#[cfg(not(windows))]
fn process_running(_exe: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcut::parse;

    fn us_lookup(c: char) -> Option<(u16, bool)> {
        match c {
            ')' => Some((0x30, true)),
            '!' => Some((0x31, true)),
            '@' => Some((0x32, true)),
            '#' => Some((0x33, true)),
            '$' => Some((0x34, true)),
            '%' => Some((0x35, true)),
            '(' => Some((0x39, true)),
            '*' => Some((0x38, true)),
            '+' => Some((0xBB, true)),
            ';' => Some((0xBA, false)),
            _ => None,
        }
    }

    fn sc(text: &str) -> Shortcut {
        parse(text).unwrap()
    }

    #[test]
    fn powertoys_enabled_lists_only_true_modules() {
        let json = r#"{"enabled":{"Peek":true,"Awake":false,"PowerToys Run":true,"X":"yes"}}"#;
        assert_eq!(powertoys_enabled(json), vec!["Peek", "PowerToys Run"]);
        assert!(powertoys_enabled("not json").is_empty());
        assert!(powertoys_enabled("{}").is_empty());
    }

    #[test]
    fn powertoys_hotkeys_walks_nested_objects_and_arrays() {
        let json = r#"{
            "name": "AdvancedPaste",
            "properties": {
                "DefaultActivationShortcut": {"win":false,"ctrl":true,"alt":false,"shift":false,"code":32,"key":""},
                "ActivationShortcut": {"win":false,"ctrl":true,"alt":false,"shift":false,"code":32,"key":""},
                "hotkey": {"value": {"win":true,"ctrl":true,"alt":false,"shift":false,"code":84,"key":""}},
                "unset": {"win":false,"ctrl":false,"alt":false,"shift":false,"code":0,"key":""},
                "additional-actions": {
                    "paste-as-file": {
                        "paste-as-txt-file": {
                            "shortcut": {"win":true,"ctrl":false,"alt":true,"shift":false,"code":86,"key":""}
                        }
                    },
                    "list": [{"shortcut": {"win":false,"ctrl":true,"alt":true,"shift":true,"code":75,"key":""}}]
                },
                "DefaultNested": {"x": {"win":true,"ctrl":true,"alt":true,"shift":true,"code":90,"key":""}}
            }
        }"#;
        let found = powertoys_hotkeys(json);
        assert_eq!(found.len(), 4);
        assert!(found.contains(&sc("Ctrl+Space")));
        assert!(found.contains(&sc("Ctrl+Alt+Shift+K")));
        assert!(found.contains(&sc("Ctrl+Win+T")));
        assert!(found.contains(&sc("Alt+Win+V")));
    }

    #[test]
    fn powertoys_hotkeys_ignores_default_keys_and_unset_codes() {
        let json = r#"{"properties":{
            "DefaultActivationShortcut": {"win":true,"ctrl":false,"alt":false,"shift":true,"code":67,"key":""},
            "ActivationShortcut": {"win":false,"ctrl":false,"alt":false,"shift":false,"code":0,"key":""}
        }}"#;
        assert!(powertoys_hotkeys(json).is_empty());
    }

    #[test]
    fn powertoys_hotkeys_reads_direct_wrapped_and_deep_shapes() {
        let json = r#"{"properties":{
            "a": {"win":false,"ctrl":true,"alt":false,"shift":false,"code":32,"key":""},
            "b": {"value": {"win":true,"ctrl":true,"alt":false,"shift":false,"code":84,"key":""}},
            "c": {"deep": {"er": [{"win":true,"ctrl":false,"alt":true,"shift":false,"code":86,"key":""}]}}
        }}"#;
        let found = powertoys_hotkeys(json);
        assert_eq!(found.len(), 3);
        assert!(found.contains(&sc("Ctrl+Space")));
        assert!(found.contains(&sc("Ctrl+Win+T")));
        assert!(found.contains(&sc("Alt+Win+V")));
    }

    #[test]
    fn command_palette_hotkey_reads_the_hotkey_key_only() {
        let json = r#"{"Hotkey":{"win":true,"ctrl":false,"alt":true,"shift":false,"code":32,"key":""},
            "CommandHotkeys":[{"win":true,"ctrl":true,"alt":true,"shift":true,"code":65,"key":""}]}"#;
        assert_eq!(command_palette_hotkey(json), Some(sc("Alt+Win+Space")));
        assert_eq!(command_palette_hotkey("{}"), None);
        assert_eq!(command_palette_hotkey(r#"{"Hotkey":null}"#), None);
    }

    #[test]
    fn powertoys_names_are_prefixed_and_spaced() {
        assert_eq!(powertoys_name("Peek"), "PowerToys Peek");
        assert_eq!(powertoys_name("PowerToys Run"), "PowerToys Run");
        assert_eq!(powertoys_name("AlwaysOnTop"), "PowerToys Always On Top");
        assert_eq!(
            powertoys_name("Keyboard Manager"),
            "PowerToys Keyboard Manager"
        );
        assert_eq!(powertoys_name("FancyZones"), "PowerToys FancyZones");
    }

    #[test]
    fn accelerators_parse_modifiers_in_any_order_and_spelling() {
        assert_eq!(
            parse_accelerator("CommandOrControl+Shift+F5", &us_lookup),
            Some(sc("Ctrl+Shift+F5"))
        );
        assert_eq!(
            parse_accelerator("Control+Alt+Left", &us_lookup),
            Some(sc("Ctrl+Alt+Left"))
        );
        assert_eq!(
            parse_accelerator("Super+Space", &us_lookup),
            Some(sc("Win+Space"))
        );
        assert_eq!(
            parse_accelerator("Ctrl+Alt+Space", &us_lookup),
            Some(sc("Ctrl+Alt+Space"))
        );
        assert_eq!(
            parse_accelerator("Alt+Num5", &us_lookup),
            Some(sc("Alt+Numpad5"))
        );
    }

    #[test]
    fn accelerators_with_shifted_symbols_imply_shift() {
        assert_eq!(
            parse_accelerator("Alt+Shift+)", &us_lookup),
            Some(sc("Alt+Shift+0"))
        );
        assert_eq!(
            parse_accelerator("Shift+Alt+@", &us_lookup),
            Some(sc("Alt+Shift+2"))
        );
        assert_eq!(
            parse_accelerator("Alt+!", &us_lookup),
            Some(sc("Alt+Shift+1"))
        );
        assert_eq!(
            parse_accelerator("Ctrl+Alt+Plus", &us_lookup),
            Some(sc("Ctrl+Alt+Shift+="))
        );
        assert_eq!(
            parse_accelerator("Alt+Shift++", &us_lookup),
            Some(sc("Alt+Shift+="))
        );
    }

    #[test]
    fn invalid_accelerators_are_rejected() {
        assert_eq!(parse_accelerator("", &us_lookup), None);
        assert_eq!(parse_accelerator("Alt+Shift+", &us_lookup), None);
        assert_eq!(parse_accelerator("Alt+Banana", &us_lookup), None);
        assert_eq!(parse_accelerator("Alt+~", &us_lookup), None);
    }

    #[test]
    fn twinkle_tray_reads_only_active_hotkeys_and_ignores_other_keys() {
        let json = r#"{
            "udpKey": "do-not-read-this",
            "theme": "dark",
            "hotkeys": [
                {"accelerator":"Alt+Shift+)","actions":[{"type":"set","value":"0"}],"id":"a","active":true},
                {"accelerator":"Alt+Shift+(","actions":[],"id":"b","active":false},
                {"accelerator":"","actions":[],"id":"c","active":true},
                {"accelerator":"Shift+Alt+@","actions":[],"id":"d","active":true},
                {"accelerator":"Alt+Shift+(","actions":[],"id":"e"}
            ]
        }"#;
        assert_eq!(
            twinkle_tray_hotkeys(json, &us_lookup),
            vec![sc("Alt+Shift+0"), sc("Alt+Shift+2")]
        );
        assert!(twinkle_tray_hotkeys("not json", &us_lookup).is_empty());
        assert!(twinkle_tray_hotkeys(r#"{"udpKey":"x"}"#, &us_lookup).is_empty());
    }

    fn nvidia_fixture(enabled: u32) -> HashMap<String, u32> {
        [
            ("IsShadowPlayEnabled", enabled),
            ("IsShadowPlayEnabledUser", 1),
            ("GFEOverlayHKeyV2Count", 2),
            ("GFEOverlayHKeyV20", 0x12),
            ("GFEOverlayHKeyV21", 0x5A),
            ("PMOCOverlayVisibilityHKeyCount", 3),
            ("PMOCOverlayVisibilityHKey0", 0x12),
            ("PMOCOverlayVisibilityHKey1", 0x11),
            ("PMOCOverlayVisibilityHKey2", 0x52),
            ("FSToggleHKeyCount", 0),
            ("BrokenHKeyCount", 2),
            ("BrokenHKey0", 0x12),
            ("VideoWidthCount", 1),
            ("VideoWidth0", 0x41),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }

    #[test]
    fn nvidia_hotkeys_decode_modifier_and_key_vks() {
        let mut found = nvidia_hotkeys(&nvidia_fixture(1));
        found.sort_by_key(|s| s.vk);
        assert_eq!(found, vec![sc("Ctrl+Alt+R"), sc("Alt+Z")]);
    }

    #[test]
    fn nvidia_hotkeys_are_empty_when_the_overlay_is_disabled() {
        assert!(nvidia_hotkeys(&nvidia_fixture(0)).is_empty());
        assert!(nvidia_hotkeys(&HashMap::new()).is_empty());
    }

    #[test]
    fn claude_quick_entry_preference_is_read_without_touching_other_keys() {
        let secret_laden = r#"{
            "mcpServers": {"x": {"env": {"API_TOKEN": "secret"}}},
            "preferences": {"sidebarMode": "chat", "quickEntryShortcut": {"accelerator": "Ctrl+Alt+K"}}
        }"#;
        assert_eq!(
            parse_claude_quick_entry(secret_laden),
            ClaudePref::Accelerator("Ctrl+Alt+K".to_string())
        );
        assert_eq!(
            parse_claude_quick_entry(r#"{"preferences":{"quickEntryShortcut":"off"}}"#),
            ClaudePref::Off
        );
        assert_eq!(
            parse_claude_quick_entry(r#"{"preferences":{"quickEntryShortcut":"Alt+Space"}}"#),
            ClaudePref::Accelerator("Alt+Space".to_string())
        );
        assert_eq!(
            parse_claude_quick_entry(r#"{"preferences":{"sidebarMode":"chat"}}"#),
            ClaudePref::Absent
        );
        assert_eq!(parse_claude_quick_entry("{}"), ClaudePref::Absent);
        assert_eq!(parse_claude_quick_entry("nope"), ClaudePref::Absent);
    }

    #[test]
    fn built_in_windows_chords_are_named() {
        assert_eq!(windows_builtin(&sc("Win+Left")), Some("owner.snap"));
        assert_eq!(windows_builtin(&sc("Win+Down")), Some("owner.snap"));
        assert_eq!(
            windows_builtin(&sc("Alt+Win+Left")),
            Some("owner.window_management")
        );
        assert_eq!(
            windows_builtin(&sc("Win+Shift+Right")),
            Some("owner.window_management")
        );
        assert_eq!(
            windows_builtin(&sc("Ctrl+Win+Left")),
            Some("owner.virtual_desktops")
        );
        assert_eq!(
            windows_builtin(&sc("Ctrl+Alt+Shift+Win+W")),
            Some("owner.office_key")
        );
        assert_eq!(windows_builtin(&sc("Win+E")), Some("owner.file_explorer"));
        assert_eq!(
            windows_builtin(&sc("Win+Shift+S")),
            Some("owner.snipping_tool")
        );
    }

    #[test]
    fn non_windows_chords_have_no_built_in_owner() {
        assert_eq!(windows_builtin(&sc("Ctrl+Alt+Left")), None);
        assert_eq!(windows_builtin(&sc("Ctrl+Win+Up")), None);
        assert_eq!(windows_builtin(&sc("Alt+Win+F13")), None);
    }

    fn known(apps: Vec<(Shortcut, &str)>, claude_default_possible: bool) -> Known {
        Known {
            apps: apps.into_iter().map(|(s, n)| (s, n.to_string())).collect(),
            claude_default_possible,
        }
    }

    #[test]
    fn app_settings_win_and_are_certain() {
        let k = known(vec![(sc("Ctrl+Space"), "PowerToys Peek")], true);
        let owner = resolve(&sc("Ctrl+Space"), &k, &|_| true).unwrap();
        assert_eq!(owner.name, OwnerName::Product("PowerToys Peek".to_string()));
        assert!(owner.certain);
        assert_eq!(owner.note(), "Overrides PowerToys Peek");
    }

    #[test]
    fn built_in_owner_is_certain() {
        let owner = resolve(&sc("Alt+Win+Right"), &Known::default(), &|_| false).unwrap();
        assert_eq!(owner.note(), "Overrides Windows window management");
    }

    #[test]
    fn magnifier_is_probable_only_while_it_runs() {
        let k = Known::default();
        let running = |exe: &str| exe == "magnify.exe";
        let owner = resolve(&sc("Ctrl+Alt+F"), &k, &running).unwrap();
        assert_eq!(owner.note(), "Overrides Windows Magnifier (probably)");
        assert_eq!(resolve(&sc("Ctrl+Alt+F"), &k, &|_| false), None);
    }

    #[test]
    fn claude_default_is_probable_only_while_it_runs_and_the_preference_is_unset() {
        let running = |exe: &str| exe == "claude.exe";
        let k = known(vec![], true);
        let owner = resolve(&sc("Ctrl+Alt+Space"), &k, &running).unwrap();
        assert_eq!(owner.note(), "Overrides Claude (probably)");
        assert_eq!(resolve(&sc("Ctrl+Alt+Space"), &k, &|_| false), None);

        let moved = known(vec![], false);
        assert_eq!(resolve(&sc("Ctrl+Alt+Space"), &moved, &running), None);
    }

    #[test]
    fn unknown_shortcut_has_no_owner() {
        assert_eq!(
            resolve(&sc("Ctrl+Alt+F11"), &Known::default(), &|_| true),
            None
        );
    }
}
