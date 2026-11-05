#[cfg(windows)]
use std::cell::{Cell, RefCell};

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
#[cfg(windows)]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL,
    VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, PostMessageW,
    SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP,
};

use crate::layout::Action;
use crate::shortcut::Shortcut;

/// Tags every key Wrecktangle injects, so its own hooks pass them through.
#[cfg(windows)]
pub const OWN_INPUT_MAGIC: usize = 0x5745_4354;
const DUMMY_VK: u16 = 0xFF;

const VK_L: u16 = 0x4C;
const VK_P: u16 = 0x50;
const VK_DELETE: u16 = 0x2E;

/// The side-specific modifier keys a low-level hook reports.
mod key {
    pub const LSHIFT: u16 = 0xA0;
    pub const RSHIFT: u16 = 0xA1;
    pub const LCTRL: u16 = 0xA2;
    pub const RCTRL: u16 = 0xA3;
    pub const LALT: u16 = 0xA4;
    pub const RALT: u16 = 0xA5;
    pub const LWIN: u16 = 0x5B;
    pub const RWIN: u16 = 0x5C;

    pub const MODIFIERS: [u16; 8] = [LCTRL, RCTRL, LALT, RALT, LSHIFT, RSHIFT, LWIN, RWIN];
}

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

/// One key event, seen by the hook or to be injected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyStroke {
    pub vk: u16,
    pub up: bool,
    pub extended: bool,
}

impl KeyStroke {
    pub fn down(vk: u16) -> KeyStroke {
        KeyStroke {
            vk,
            up: false,
            extended: is_extended(vk),
        }
    }

    pub fn up(vk: u16) -> KeyStroke {
        KeyStroke {
            up: true,
            ..KeyStroke::down(vk)
        }
    }
}

// Right Ctrl, right Alt and both Win keys send an E0 scan code prefix.
fn is_extended(vk: u16) -> bool {
    matches!(vk, key::RCTRL | key::RALT | key::LWIN | key::RWIN)
}

/// A set of side-specific modifier keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModKeys(u8);

impl ModKeys {
    fn bit(vk: u16) -> u8 {
        key::MODIFIERS
            .iter()
            .position(|m| *m == vk)
            .map_or(0, |i| 1 << i)
    }

    pub fn insert(&mut self, vk: u16) {
        self.0 |= Self::bit(vk);
    }

    pub fn remove(&mut self, vk: u16) {
        self.0 &= !Self::bit(vk);
    }

    pub fn contains(&self, vk: u16) -> bool {
        self.0 & Self::bit(vk) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        key::MODIFIERS.into_iter().filter(|vk| self.contains(*vk))
    }

    pub fn mods(&self) -> Mods {
        let any = |a: u16, b: u16| self.contains(a) || self.contains(b);
        Mods {
            ctrl: any(key::LCTRL, key::RCTRL),
            alt: any(key::LALT, key::RALT),
            shift: any(key::LSHIFT, key::RSHIFT),
            lwin: self.contains(key::LWIN),
            rwin: self.contains(key::RWIN),
        }
    }
}

/// Where the Project shortcut's relay to a held Win+P stands. `released`
/// holds the modifiers the user still has down but Windows sees as up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Relay {
    #[default]
    Idle,
    /// Win is held for the Project menu; each press of `key` is a P.
    Relaying {
        key: u16,
        key_down: bool,
        released: ModKeys,
    },
    /// Win is up again; swallows what is left of the chord as it comes up.
    Draining { key: Option<u16>, released: ModKeys },
}

/// Advances the relay by one key event; returns the next state, whether to
/// swallow the event, and the keys to inject in its place. `held` reads the
/// modifiers down right now, and is only called for the chord's own key.
pub fn relay_step(
    state: Relay,
    shortcut: &Shortcut,
    event: KeyStroke,
    held: impl FnOnce() -> ModKeys,
) -> (Relay, bool, Vec<KeyStroke>) {
    match state {
        Relay::Idle => {
            if event.up || event.vk != shortcut.vk {
                return (state, false, Vec::new());
            }
            let released = held();
            let mods = released.mods();
            if !matches(shortcut, mods, event.vk) {
                return (state, false, Vec::new());
            }
            let mut inject = Vec::new();
            if mods.alt || mods.win() {
                inject.extend([KeyStroke::down(DUMMY_VK), KeyStroke::up(DUMMY_VK)]);
            }
            inject.extend(released.iter().map(KeyStroke::up));
            inject.extend([KeyStroke::down(key::LWIN), KeyStroke::down(VK_P)]);
            let next = Relay::Relaying {
                key: event.vk,
                key_down: true,
                released,
            };
            (next, true, inject)
        }
        Relay::Relaying {
            key,
            key_down,
            mut released,
        } => {
            if event.vk == key {
                let next = Relay::Relaying {
                    key,
                    key_down: !event.up,
                    released,
                };
                let p = if event.up {
                    KeyStroke::up(VK_P)
                } else {
                    KeyStroke::down(VK_P)
                };
                return (next, true, vec![p]);
            }
            let lifted = released.contains(event.vk);
            if lifted && !event.up {
                return (state, true, Vec::new());
            }
            // Releasing a lifted modifier picks the highlighted option. Any
            // other key ends the cycle first, so it does not arrive as Win+key.
            let mut inject = relay_end(state);
            if lifted {
                released.remove(event.vk);
            } else {
                inject.push(event);
            }
            (drain(key_down.then_some(key), released), true, inject)
        }
        Relay::Draining { key, mut released } => {
            if key == Some(event.vk) {
                let key = if event.up { None } else { key };
                return (drain(key, released), true, Vec::new());
            }
            if !released.contains(event.vk) {
                return (state, false, Vec::new());
            }
            if event.up {
                released.remove(event.vk);
            }
            (drain(key, released), true, Vec::new())
        }
    }
}

fn drain(key: Option<u16>, released: ModKeys) -> Relay {
    if key.is_none() && released.is_empty() {
        Relay::Idle
    } else {
        Relay::Draining { key, released }
    }
}

/// The keys that end a running cycle: P if it is down, then Win.
pub fn relay_end(state: Relay) -> Vec<KeyStroke> {
    match state {
        Relay::Relaying { key_down, .. } => {
            let mut strokes = Vec::new();
            if key_down {
                strokes.push(KeyStroke::up(VK_P));
            }
            strokes.push(KeyStroke::up(key::LWIN));
            strokes
        }
        Relay::Idle | Relay::Draining { .. } => Vec::new(),
    }
}

#[cfg(windows)]
thread_local! {
    static ENTRIES: RefCell<Vec<(Action, Shortcut)>> = const { RefCell::new(Vec::new()) };
    static TARGET: Cell<Option<HWND>> = const { Cell::new(None) };
    static HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static HELD: Cell<Option<u16>> = const { Cell::new(None) };
    static RELAY: Cell<Option<Shortcut>> = const { Cell::new(None) };
    static RELAY_STATE: Cell<Relay> = const { Cell::new(Relay::Idle) };
}

/// Routes `entries` through the global hook, and `relay` (the Project
/// shortcut) as a held Win+P, installing the hook if needed, or removing it
/// when there is nothing to take over. Returns false when the hook could not
/// be installed.
#[cfg(windows)]
pub fn start(hwnd: HWND, entries: &[(Action, Shortcut)], relay: Option<Shortcut>) -> bool {
    end_relay();
    ENTRIES.with(|e| *e.borrow_mut() = entries.to_vec());
    RELAY.with(|r| r.set(relay));
    TARGET.with(|t| t.set(Some(hwnd)));
    HELD.with(|h| h.set(None));
    if entries.is_empty() && relay.is_none() {
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
    end_relay();
    ENTRIES.with(|e| e.borrow_mut().clear());
    RELAY.with(|r| r.set(None));
    HELD.with(|h| h.set(None));
}

// Lets go of Win (and P) if a cycle is running, so neither sticks once the
// hook stops seeing the chord's release.
#[cfg(windows)]
fn end_relay() {
    let state = RELAY_STATE.with(|s| s.replace(Relay::Idle));
    send_strokes(&relay_end(state));
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
    send_strokes(&[KeyStroke::down(DUMMY_VK), KeyStroke::up(DUMMY_VK)]);
}

/// Injects `strokes` in one SendInput call, so nothing comes between them.
#[cfg(windows)]
fn send_strokes(strokes: &[KeyStroke]) {
    if strokes.is_empty() {
        return;
    }
    let inputs: Vec<INPUT> = strokes
        .iter()
        .map(|stroke| {
            let mut flags = KEYBD_EVENT_FLAGS(0);
            if stroke.up {
                flags |= KEYEVENTF_KEYUP;
            }
            if stroke.extended {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            let scan = unsafe { MapVirtualKeyW(stroke.vk as u32, MAPVK_VK_TO_VSC) };
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(stroke.vk),
                        wScan: scan as u16,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: OWN_INPUT_MAGIC,
                    },
                },
            }
        })
        .collect();
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(windows)]
fn read_mod_keys() -> ModKeys {
    let mut keys = ModKeys::default();
    for vk in key::MODIFIERS {
        if unsafe { GetAsyncKeyState(vk as i32) } < 0 {
            keys.insert(vk);
        }
    }
    keys
}

// True when the relay swallows the event. While no cycle runs, any key but
// the chord's returns at once, which keeps the hook fast.
#[cfg(windows)]
fn relay_key(event: KeyStroke) -> bool {
    let Some(shortcut) = RELAY.with(|r| r.get()) else {
        return false;
    };
    let state = RELAY_STATE.with(|s| s.get());
    let (next, swallow, inject) = relay_step(state, &shortcut, event, read_mod_keys);
    RELAY_STATE.with(|s| s.set(next));
    send_strokes(&inject);
    swallow
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
    if kb.flags.contains(LLKHF_INJECTED) && kb.dwExtraInfo == OWN_INPUT_MAGIC {
        return false;
    }
    let vk = kb.vkCode as u16;
    let msg = wparam.0 as u32;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
    if !is_up && msg != WM_KEYDOWN && msg != WM_SYSKEYDOWN {
        return false;
    }
    // Before the modifier check: the relay follows the chord's modifiers too.
    let event = KeyStroke {
        vk,
        up: is_up,
        extended: kb.flags.contains(LLKHF_EXTENDED),
    };
    if relay_key(event) {
        return true;
    }
    if is_modifier_vk(vk) {
        return false;
    }

    if is_up {
        return HELD.with(|h| {
            let held = h.get() == Some(vk);
            if held {
                h.set(None);
            }
            held
        });
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

    const F9: u16 = 0x78;
    const A: u16 = 0x41;

    fn down(vk: u16) -> KeyStroke {
        KeyStroke::down(vk)
    }

    fn up(vk: u16) -> KeyStroke {
        KeyStroke::up(vk)
    }

    fn held(keys: &[u16]) -> ModKeys {
        let mut held = ModKeys::default();
        for vk in keys {
            held.insert(*vk);
        }
        held
    }

    /// Feeds `events` through the relay with `keys` held at the chord press,
    /// returning the state left and, per event, (swallowed, injected).
    fn run(
        shortcut: &str,
        keys: &[u16],
        start: Relay,
        events: &[KeyStroke],
    ) -> (Relay, Vec<(bool, Vec<KeyStroke>)>) {
        let sc = parse(shortcut).unwrap();
        let mut state = start;
        let mut steps = Vec::new();
        for event in events {
            let (next, swallow, inject) = relay_step(state, &sc, *event, || held(keys));
            state = next;
            steps.push((swallow, inject));
        }
        (state, steps)
    }

    fn dummy() -> [KeyStroke; 2] {
        [down(DUMMY_VK), up(DUMMY_VK)]
    }

    #[test]
    fn relay_cycles_like_a_held_win_p_and_picks_on_release() {
        let (state, steps) = run(
            "Ctrl+Alt+F9",
            &[key::LCTRL, key::LALT],
            Relay::Idle,
            &[
                down(F9),
                up(F9),
                down(F9),
                up(F9),
                up(key::LALT),
                up(key::LCTRL),
            ],
        );
        let mut press = dummy().to_vec();
        press.extend([up(key::LCTRL), up(key::LALT), down(key::LWIN), down(VK_P)]);
        assert_eq!(
            steps,
            [
                (true, press),
                (true, vec![up(VK_P)]),
                (true, vec![down(VK_P)]),
                (true, vec![up(VK_P)]),
                (true, vec![up(key::LWIN)]),
                (true, vec![]),
            ]
        );
        assert_eq!(state, Relay::Idle);
    }

    #[test]
    fn relay_lifts_a_held_win_and_presses_its_own() {
        let (state, steps) = run(
            "Ctrl+Win+F9",
            &[key::LCTRL, key::RWIN],
            Relay::Idle,
            &[down(F9), up(F9), up(key::RWIN), up(key::LCTRL)],
        );
        let mut press = dummy().to_vec();
        press.extend([up(key::LCTRL), up(key::RWIN), down(key::LWIN), down(VK_P)]);
        assert_eq!(steps[0], (true, press));
        assert_eq!(steps[2], (true, vec![up(key::LWIN)]));
        assert_eq!(steps[3], (true, vec![]));
        assert_eq!(state, Relay::Idle);
    }

    #[test]
    fn relay_skips_the_dummy_key_without_alt_or_win() {
        let (_, steps) = run(
            "Ctrl+Shift+F9",
            &[key::LCTRL, key::RSHIFT],
            Relay::Idle,
            &[down(F9)],
        );
        assert_eq!(
            steps[0].1,
            [up(key::LCTRL), up(key::RSHIFT), down(key::LWIN), down(VK_P)]
        );
    }

    #[test]
    fn relay_repeats_p_while_the_key_auto_repeats() {
        let (state, steps) = run(
            "Ctrl+Alt+F9",
            &[key::LCTRL, key::LALT],
            Relay::Idle,
            &[
                down(F9),
                down(F9),
                down(F9),
                up(key::LCTRL),
                down(F9),
                up(F9),
                up(key::LALT),
            ],
        );
        assert_eq!(steps[1], (true, vec![down(VK_P)]));
        assert_eq!(steps[2], (true, vec![down(VK_P)]));
        // Releasing a modifier with the key still down lets go of P first.
        assert_eq!(steps[3], (true, vec![up(VK_P), up(key::LWIN)]));
        // The key's own repeats and release are swallowed until it comes up.
        assert_eq!(steps[4], (true, vec![]));
        assert_eq!(steps[5], (true, vec![]));
        assert_eq!(steps[6], (true, vec![]));
        assert_eq!(state, Relay::Idle);
    }

    #[test]
    fn relay_swallows_repeats_of_the_lifted_modifiers() {
        let (state, steps) = run(
            "Ctrl+Alt+F9",
            &[key::LCTRL, key::LALT],
            Relay::Idle,
            &[down(F9), up(F9), down(key::LCTRL), down(key::LALT)],
        );
        assert_eq!(steps[2], (true, vec![]));
        assert_eq!(steps[3], (true, vec![]));
        assert!(matches!(state, Relay::Relaying { .. }));
    }

    #[test]
    fn another_key_mid_cycle_comes_after_win_is_released() {
        let (state, steps) = run(
            "Ctrl+Alt+F9",
            &[key::LCTRL, key::LALT],
            Relay::Idle,
            &[
                down(F9),
                up(F9),
                down(A),
                up(A),
                up(key::LCTRL),
                up(key::LALT),
            ],
        );
        assert_eq!(steps[2], (true, vec![up(key::LWIN), down(A)]));
        assert_eq!(steps[3], (false, vec![]));
        assert_eq!(steps[4], (true, vec![]));
        assert_eq!(steps[5], (true, vec![]));
        assert_eq!(state, Relay::Idle);
    }

    #[test]
    fn another_key_while_the_chord_key_is_down_lets_go_of_p_too() {
        let (_, steps) = run(
            "Ctrl+Alt+F9",
            &[key::LCTRL, key::LALT],
            Relay::Idle,
            &[down(F9), down(A)],
        );
        assert_eq!(steps[1], (true, vec![up(VK_P), up(key::LWIN), down(A)]));
    }

    #[test]
    fn draining_swallows_only_the_lifted_modifiers() {
        let start = Relay::Draining {
            key: None,
            released: held(&[key::LCTRL]),
        };
        let (state, steps) = run(
            "Ctrl+Alt+F9",
            &[],
            start,
            &[
                down(key::RSHIFT),
                down(key::LCTRL),
                up(key::LCTRL),
                down(key::LCTRL),
            ],
        );
        assert_eq!(steps[0], (false, vec![]));
        assert_eq!(steps[1], (true, vec![]));
        assert_eq!(steps[2], (true, vec![]));
        assert_eq!(steps[3], (false, vec![]));
        assert_eq!(state, Relay::Idle);
    }

    #[test]
    fn ending_mid_cycle_releases_win() {
        let held_key = Relay::Relaying {
            key: F9,
            key_down: true,
            released: held(&[key::LCTRL]),
        };
        assert_eq!(relay_end(held_key), [up(VK_P), up(key::LWIN)]);
        let released_key = Relay::Relaying {
            key: F9,
            key_down: false,
            released: held(&[key::LCTRL]),
        };
        assert_eq!(relay_end(released_key), [up(key::LWIN)]);
        let draining = Relay::Draining {
            key: Some(F9),
            released: held(&[key::LCTRL]),
        };
        assert_eq!(relay_end(draining), []);
        assert_eq!(relay_end(Relay::Idle), []);
    }

    #[test]
    fn relay_ignores_other_chords_and_reads_modifiers_only_for_its_key() {
        let sc = parse("Ctrl+Alt+F9").unwrap();
        let unread = || -> ModKeys { panic!("modifiers read for another key") };
        assert_eq!(
            relay_step(Relay::Idle, &sc, down(A), unread),
            (Relay::Idle, false, vec![])
        );
        for keys in [&[key::LCTRL][..], &[key::LCTRL, key::LALT, key::LSHIFT]] {
            assert_eq!(
                relay_step(Relay::Idle, &sc, down(F9), || held(keys)),
                (Relay::Idle, false, vec![]),
                "{keys:?}"
            );
        }
        assert_eq!(
            relay_step(Relay::Idle, &sc, up(F9), || held(&[key::LCTRL, key::LALT])),
            (Relay::Idle, false, vec![])
        );
    }

    #[test]
    fn relay_relays_p_when_the_chord_key_is_p() {
        let (_, steps) = run(
            "Ctrl+Alt+P",
            &[key::RCTRL, key::RALT],
            Relay::Idle,
            &[down(VK_P), up(VK_P)],
        );
        assert_eq!(steps[0].1.last(), Some(&down(VK_P)));
        assert_eq!(steps[1], (true, vec![up(VK_P)]));
    }

    #[test]
    fn right_ctrl_right_alt_and_win_are_extended_keys() {
        for vk in [key::RCTRL, key::RALT, key::LWIN, key::RWIN] {
            assert!(down(vk).extended, "{vk:#x}");
        }
        for vk in [
            key::LCTRL,
            key::LALT,
            key::LSHIFT,
            key::RSHIFT,
            VK_P,
            DUMMY_VK,
        ] {
            assert!(!up(vk).extended, "{vk:#x}");
        }
    }

    #[test]
    fn mod_keys_report_their_chord_modifiers() {
        assert_eq!(
            held(&[key::RCTRL, key::LALT, key::RWIN]).mods(),
            mods(true, true, false, false, true)
        );
        assert_eq!(
            held(&[key::LSHIFT]).mods(),
            mods(false, false, true, false, false)
        );
        let keys = held(&[key::LCTRL, key::RSHIFT]);
        assert_eq!(keys.iter().collect::<Vec<_>>(), [key::LCTRL, key::RSHIFT]);
        assert!(keys.contains(key::LCTRL) && !keys.contains(key::RCTRL));
    }

    #[test]
    fn action_ids_round_trip() {
        for action in Action::ALL {
            assert_eq!(action_from_id(action_id(action)), Some(action));
        }
        assert_eq!(action_from_id(Action::ALL.len()), None);
    }
}
