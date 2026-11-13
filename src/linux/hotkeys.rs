//! Global shortcuts as X11 key grabs on the root window. A grab another
//! client already holds fails with BadAccess, which is how a busy chord
//! shows up here.

use x11rb::connection::Connection;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, ModMask};

use super::x11;
use crate::shortcut::{Shortcut, keysym};

const ALT_L: u32 = 0xFFE9;
const ALT_R: u32 = 0xFFEA;
const SUPER_L: u32 = 0xFFEB;
const SUPER_R: u32 = 0xFFEC;
const NUM_LOCK: u32 = 0xFF7F;

const SHIFT_MASK: u16 = 1;
const LOCK_MASK: u16 = 1 << 1;
const CONTROL_MASK: u16 = 1 << 2;

/// The keyboard mapping: which keycode types which keysym, and which
/// modifier bits Alt, Super and NumLock sit on.
pub struct Keymap {
    min_keycode: u8,
    per_keycode: usize,
    keysyms: Vec<u32>,
    alt: u16,
    win: u16,
    num_lock: u16,
}

impl Keymap {
    pub fn load() -> Keymap {
        let conn = &x11::shared().conn;
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let mapping = conn
            .get_keyboard_mapping(min, max - min + 1)
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        let mut keymap = Keymap {
            min_keycode: min,
            per_keycode: mapping
                .as_ref()
                .map_or(1, |m| usize::from(m.keysyms_per_keycode).max(1)),
            keysyms: mapping.map(|m| m.keysyms).unwrap_or_default(),
            // The usual bits, kept when the modifier map doesn't say.
            alt: 1 << 3,
            win: 1 << 6,
            num_lock: 1 << 4,
        };

        let modifiers = conn
            .get_modifier_mapping()
            .ok()
            .and_then(|cookie| cookie.reply().ok());
        if let Some(modifiers) = modifiers {
            let per = modifiers.keycodes.len() / 8;
            if per > 0 {
                // Mod1 to Mod5 are bits 3 to 7; Shift, Lock and Control
                // come first and never hold these keys.
                for (bit, codes) in modifiers.keycodes.chunks(per).enumerate().skip(3) {
                    let mask = 1u16 << bit;
                    let syms: Vec<u32> = codes
                        .iter()
                        .filter(|&&code| code != 0)
                        .map(|&code| keymap.keysym(code, 0))
                        .collect();
                    let has = |wanted: &[u32]| syms.iter().any(|sym| wanted.contains(sym));
                    if has(&[ALT_L, ALT_R]) {
                        keymap.alt = mask;
                    }
                    if has(&[SUPER_L, SUPER_R]) {
                        keymap.win = mask;
                    }
                    if has(&[NUM_LOCK]) {
                        keymap.num_lock = mask;
                    }
                }
            }
        }
        keymap
    }

    /// The keysym in `column` of `keycode`: 0 unshifted, 1 with Shift.
    pub fn keysym(&self, keycode: u8, column: usize) -> u32 {
        let Some(row) = usize::from(keycode).checked_sub(usize::from(self.min_keycode)) else {
            return 0;
        };
        if column >= self.per_keycode {
            return 0;
        }
        self.keysyms
            .get(row * self.per_keycode + column)
            .copied()
            .unwrap_or(0)
    }

    /// The keycode that types `sym`, preferring a key where it needs no Shift.
    pub fn keycode(&self, sym: u32) -> Option<u8> {
        let rows = self.keysyms.len() / self.per_keycode;
        let code = |row: usize| u8::try_from(row + usize::from(self.min_keycode)).ok();
        for column in [0, 1] {
            if let Some(row) = (0..rows).find(|&row| {
                column < self.per_keycode && self.keysyms[row * self.per_keycode + column] == sym
            }) {
                return code(row);
            }
        }
        None
    }

    pub fn modifiers(&self, sc: &Shortcut) -> u16 {
        let mut mask = 0;
        if sc.ctrl {
            mask |= CONTROL_MASK;
        }
        if sc.alt {
            mask |= self.alt;
        }
        if sc.shift {
            mask |= SHIFT_MASK;
        }
        if sc.win {
            mask |= self.win;
        }
        mask
    }

    /// Which of Ctrl, Alt, Shift and Win a key event's state holds.
    pub fn modifier_flags(&self, state: u16) -> (bool, bool, bool, bool) {
        (
            state & CONTROL_MASK != 0,
            state & self.alt != 0,
            state & SHIFT_MASK != 0,
            state & self.win != 0,
        )
    }

    /// The modifier bits a shortcut can use; CapsLock, NumLock and the rest
    /// are ignored when matching a key press.
    pub fn relevant(&self) -> u16 {
        CONTROL_MASK | SHIFT_MASK | self.alt | self.win
    }

    // A grab needs the exact modifier state, so each shortcut is grabbed with
    // CapsLock and NumLock on and off.
    fn lock_variants(&self) -> Vec<u16> {
        let mut variants = vec![0, LOCK_MASK, self.num_lock, LOCK_MASK | self.num_lock];
        variants.dedup();
        variants
    }

    pub fn keycode_of(&self, sc: &Shortcut) -> Option<u8> {
        keysym::from_vk(sc.vk).and_then(|sym| self.keycode(sym))
    }
}

pub enum Grab {
    Grabbed,
    /// Another client holds the chord.
    Busy,
    /// The keyboard layout has no key for it.
    NoKey,
}

pub fn grab(keymap: &Keymap, sc: &Shortcut) -> Grab {
    let Some(code) = keymap.keycode_of(sc) else {
        return Grab::NoKey;
    };
    let shared = x11::shared();
    let mods = keymap.modifiers(sc);
    let mut grabbed = Vec::new();
    for extra in keymap.lock_variants() {
        let checked = shared
            .conn
            .grab_key(
                false,
                shared.root,
                ModMask::from(mods | extra),
                code,
                GrabMode::ASYNC,
                GrabMode::ASYNC,
            )
            .map(|cookie| cookie.check());
        if let Ok(Ok(())) = checked {
            grabbed.push(extra);
        } else {
            for extra in grabbed {
                let _ = shared
                    .conn
                    .ungrab_key(code, shared.root, ModMask::from(mods | extra));
            }
            let _ = shared.conn.flush();
            return Grab::Busy;
        }
    }
    Grab::Grabbed
}

pub fn ungrab(keymap: &Keymap, sc: &Shortcut) {
    let Some(code) = keymap.keycode_of(sc) else {
        return;
    };
    let shared = x11::shared();
    let mods = keymap.modifiers(sc);
    for extra in keymap.lock_variants() {
        let _ = shared
            .conn
            .ungrab_key(code, shared.root, ModMask::from(mods | extra));
    }
}

pub fn matches(keymap: &Keymap, sc: &Shortcut, keycode: u8, state: u16) -> bool {
    keymap.keycode_of(sc) == Some(keycode) && state & keymap.relevant() == keymap.modifiers(sc)
}

/// Asks the server to report a held key as repeated presses without the
/// releases in between, so holding a shortcut runs it once.
pub fn detect_autorepeat() -> bool {
    let conn = &x11::shared().conn;
    let supported = conn
        .xkb_use_extension(1, 0)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| reply.supported);
    if !supported {
        return false;
    }
    let flag = xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT;
    conn.xkb_per_client_flags(
        xkb::ID::USE_CORE_KBD.into(),
        flag,
        flag,
        xkb::BoolCtrl::from(0u32),
        xkb::BoolCtrl::from(0u32),
        xkb::BoolCtrl::from(0u32),
    )
    .ok()
    .and_then(|cookie| cookie.reply().ok())
    .is_some_and(|reply| u32::from(reply.value) & u32::from(flag) != 0)
}
