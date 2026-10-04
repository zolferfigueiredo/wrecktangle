#[cfg(windows)]
use std::cell::{Cell, RefCell};

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, PostMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::layout::Action;
use crate::shortcut::Shortcut;

#[cfg(windows)]
pub const DUMMY_KEY_MAGIC: usize = 0x5745_4354;
#[cfg(windows)]
const DUMMY_VK: u16 = 0xFF;

const VK_L: u16 = 0x4C;
const VK_DELETE: u16 = 0x2E;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub lwin: bool,
    pub rwin: bool,
}

impl Mods {
    pub fn win(&self) -> bool {
        self.lwin || self.rwin
    }
}

pub fn matches(shortcut: &Shortcut, mods: Mods, vk: u16) -> bool {
    shortcut.vk == vk
        && shortcut.ctrl == mods.ctrl
        && shortcut.alt == mods.alt
        && shortcut.shift == mods.shift
        && shortcut.win == mods.win()
}

// Win+L and Ctrl+Alt+Del are acted on by Windows before any keyboard hook
// sees them, so a hook cannot take them over.
pub fn is_reserved(shortcut: &Shortcut) -> bool {
    let lock =
        shortcut.win && !shortcut.ctrl && !shortcut.alt && !shortcut.shift && shortcut.vk == VK_L;
    let secure_attention = shortcut.ctrl && shortcut.alt && shortcut.vk == VK_DELETE;
    lock || secure_attention
}

pub fn action_id(action: Action) -> usize {
    Action::ALL
        .iter()
        .position(|a| *a == action)
        .unwrap_or_default()
}

pub fn action_from_id(id: usize) -> Option<Action> {
    Action::ALL.get(id).copied()
}

#[cfg(windows)]
thread_local! {
    static ENTRIES: RefCell<Vec<(Action, Shortcut)>> = const { RefCell::new(Vec::new()) };
    static TARGET: Cell<Option<HWND>> = const { Cell::new(None) };
    static HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static HELD: Cell<Option<u16>> = const { Cell::new(None) };
}

/// Routes `entries` through the global hook, installing it if needed, or
/// removing it when there is nothing to take over. Returns false when the
/// hook could not be installed.
#[cfg(windows)]
pub fn start(hwnd: HWND, entries: &[(Action, Shortcut)]) -> bool {
    ENTRIES.with(|e| *e.borrow_mut() = entries.to_vec());
    TARGET.with(|t| t.set(Some(hwnd)));
    HELD.with(|h| h.set(None));
    if entries.is_empty() {
        remove_hook();
        return true;
    }
    let installed = HOOK.with(|h| {
        let current = h.take();
        let present = current.is_some();
        h.set(current);
        present
    });
    installed || install_hook()
}

#[cfg(windows)]
pub fn stop() {
    remove_hook();
    ENTRIES.with(|e| e.borrow_mut().clear());
    HELD.with(|h| h.set(None));
}

#[cfg(windows)]
fn install_hook() -> bool {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), Some(hinstance.into()), 0) {
            Ok(h) => {
                HOOK.with(|c| c.set(Some(h)));
                true
            }
            Err(_) => false,
        }
    }
}

#[cfg(windows)]
fn remove_hook() {
    if let Some(h) = HOOK.with(|c| c.take()) {
        unsafe {
            let _ = UnhookWindowsHookEx(h);
        }
    }
}

#[cfg(windows)]
pub fn is_modifier_vk(vk: u16) -> bool {
    matches!(
        VIRTUAL_KEY(vk),
        VK_CONTROL
            | VK_MENU
            | VK_SHIFT
            | VK_LWIN
            | VK_RWIN
            | VK_LCONTROL
            | VK_RCONTROL
            | VK_LMENU
            | VK_RMENU
            | VK_LSHIFT
            | VK_RSHIFT
    )
}

/// Presses and releases an unassigned key so a held Win or Alt is not
/// released "alone", which would open the Start menu or an app's menu bar.
#[cfg(windows)]
pub fn inject_dummy_key() {
    let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(DUMMY_VK),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: DUMMY_KEY_MAGIC,
            },
        },
    };
    let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(windows)]
fn read_mods() -> Mods {
    let down = |vk: VIRTUAL_KEY| unsafe { GetAsyncKeyState(vk.0 as i32) } < 0;
    Mods {
        ctrl: down(VK_CONTROL),
        alt: down(VK_MENU),
        shift: down(VK_SHIFT),
        lwin: down(VK_LWIN),
        rwin: down(VK_RWIN),
    }
}

// Runs on the UI thread inside the message loop and must stay fast: Windows
// stops calling a hook that is slow to answer.
#[cfg(windows)]
extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && on_key(wparam, lparam) {
        return LRESULT(1);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

// True when the event belongs to a taken-over shortcut and must be swallowed.
// A swallowed key is remembered until its key-up so that auto-repeats and the
// release are swallowed too, and apps never see half a key press.
#[cfg(windows)]
fn on_key(wparam: WPARAM, lparam: LPARAM) -> bool {
    let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    if kb.flags.contains(LLKHF_INJECTED) && kb.dwExtraInfo == DUMMY_KEY_MAGIC {
        return false;
    }
    let vk = kb.vkCode as u16;
    if is_modifier_vk(vk) {
        return false;
    }

    let msg = wparam.0 as u32;
    if msg == WM_KEYUP || msg == WM_SYSKEYUP {
        return HELD.with(|h| {
            let held = h.get() == Some(vk);
            if held {
                h.set(None);
            }
            held
        });
    }
    if msg != WM_KEYDOWN && msg != WM_SYSKEYDOWN {
        return false;
    }
    if HELD.with(|h| h.get()) == Some(vk) {
        return true;
    }

    let mut mods = None;
    let hit = ENTRIES.with(|e| {
        e.try_borrow().ok().and_then(|entries| {
            entries
                .iter()
                .find(|(_, sc)| sc.vk == vk && matches(sc, *mods.get_or_insert_with(read_mods), vk))
                .map(|(action, _)| *action)
        })
    });
    let (Some(action), Some(mods)) = (hit, mods) else {
        return false;
    };

    HELD.with(|h| h.set(Some(vk)));
    if mods.win() || mods.alt {
        inject_dummy_key();
    }
    if let Some(hwnd) = TARGET.with(|t| t.get()) {
        unsafe {
            let _ = PostMessageW(
                Some(hwnd),
                crate::app::WM_APP_TAKEOVER,
                WPARAM(action_id(action)),
                LPARAM(0),
            );
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcut::parse;

    fn mods(ctrl: bool, alt: bool, shift: bool, lwin: bool, rwin: bool) -> Mods {
        Mods {
            ctrl,
            alt,
            shift,
            lwin,
            rwin,
        }
    }

    #[test]
    fn exact_modifiers_and_key_match() {
        let sc = parse("Ctrl+Alt+Left").unwrap();
        assert!(matches(&sc, mods(true, true, false, false, false), 0x25));
    }

    #[test]
    fn different_key_does_not_match() {
        let sc = parse("Ctrl+Alt+Left").unwrap();
        assert!(!matches(&sc, mods(true, true, false, false, false), 0x27));
    }

    #[test]
    fn missing_modifier_does_not_match() {
        let sc = parse("Ctrl+Alt+Left").unwrap();
        assert!(!matches(&sc, mods(true, false, false, false, false), 0x25));
        assert!(!matches(&sc, mods(false, false, false, false, false), 0x25));
    }

    #[test]
    fn extra_modifier_does_not_match() {
        let sc = parse("Ctrl+Alt+Left").unwrap();
        assert!(!matches(&sc, mods(true, true, true, false, false), 0x25));
        assert!(!matches(&sc, mods(true, true, false, true, false), 0x25));
        assert!(!matches(&sc, mods(true, true, false, false, true), 0x25));
    }

    #[test]
    fn win_matches_from_either_side() {
        let sc = parse("Alt+Win+Left").unwrap();
        assert!(matches(&sc, mods(false, true, false, true, false), 0x25));
        assert!(matches(&sc, mods(false, true, false, false, true), 0x25));
        assert!(matches(&sc, mods(false, true, false, true, true), 0x25));
        assert!(!matches(&sc, mods(false, true, false, false, false), 0x25));
    }

    #[test]
    fn win_held_does_not_match_a_shortcut_without_win() {
        let sc = parse("Ctrl+Alt+Left").unwrap();
        assert!(!matches(&sc, mods(true, true, false, true, false), 0x25));
    }

    #[test]
    fn shift_must_match_exactly() {
        let sc = parse("Win+Shift+Left").unwrap();
        assert!(matches(&sc, mods(false, false, true, true, false), 0x25));
        assert!(!matches(&sc, mods(false, false, false, true, false), 0x25));
    }

    #[test]
    fn win_l_and_ctrl_alt_del_are_reserved() {
        assert!(is_reserved(&parse("Win+L").unwrap()));
        assert!(is_reserved(&parse("Ctrl+Alt+Delete").unwrap()));
    }

    #[test]
    fn other_chords_are_not_reserved() {
        assert!(!is_reserved(&parse("Win+Shift+L").unwrap()));
        assert!(!is_reserved(&parse("Ctrl+Win+L").unwrap()));
        assert!(!is_reserved(&parse("Alt+Win+Left").unwrap()));
        assert!(!is_reserved(&parse("Ctrl+Alt+Left").unwrap()));
        assert!(!is_reserved(&parse("Ctrl+Delete").unwrap()));
    }

    #[test]
    fn action_ids_round_trip() {
        for action in Action::ALL {
            assert_eq!(action_from_id(action_id(action)), Some(action));
        }
        assert_eq!(action_from_id(Action::ALL.len()), None);
    }
}
