//! The Linux app: config, shortcut grabs, the Xfce takeover and what each
//! message runs. The counterpart of `src/app.rs`, with the same functions
//! for Settings to call.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ConnectionExt, Mapping};

use super::hotkeys::{self, Grab, Keymap};
use super::x11::{self, Message};
use super::{notify, settings, tray, window_ops, xfconf};
use crate::config::{self, Config};
use crate::layout::Action;
use crate::owners::Owner;
use crate::shortcut::{self, Shortcut};
use crate::xfce::{self, Binding};
use crate::{lang, startup, takeover, update};

// Xfce's window manager and settings daemon let go of a key a moment after
// its binding is removed, so a taken-over chord is grabbed again for up to
// a second.
const RELEASE_WAIT: Duration = Duration::from_millis(50);
const RELEASE_TRIES: usize = 20;
const UPDATE_STARTUP_DELAY: Duration = Duration::from_secs(30);

#[derive(Clone, PartialEq, Eq)]
enum Claim {
    /// Grabbed after removing this Xfce binding.
    TakenOver(Binding),
    Reserved,
    InUse,
}

struct State {
    config: Config,
    keymap: Keymap,
    grabbed: Vec<(Action, Shortcut)>,
    claims: HashMap<Action, Claim>,
    /// The key of the shortcut being held, so its repeats run it once.
    held: Option<u8>,
    config_invalid: bool,
    update_notified: Option<String>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn init() {
    let path = config::config_path();
    let (mut config, source) = config::load(&path);
    let language_unset = config.language.is_empty();
    if language_unset {
        config.language = lang::detect_system().to_string();
    }
    lang::set(&config.language);
    if source == config::Source::Missing || (source == config::Source::Loaded && language_unset) {
        let _ = config::save(&path, &config);
    }

    hotkeys::detect_autorepeat();
    // Display mode's Settings row follows the screens plugged in.
    let shared = x11::shared();
    let _ = shared
        .conn
        .randr_select_input(shared.root, randr::NotifyMask::SCREEN_CHANGE);
    STATE.with(|s| {
        *s.borrow_mut() = Some(State {
            config,
            keymap: Keymap::load(),
            grabbed: Vec::new(),
            claims: HashMap::new(),
            held: None,
            config_invalid: source == config::Source::Invalid,
            update_notified: None,
        });
    });

    let failed = register_all();
    tray::start();
    if source == config::Source::Invalid {
        notify::notify(&lang::t("notify.config_invalid"));
    }
    if !failed.is_empty() {
        notify::notify(&failed_message(&failed));
    }
    start_update_timer();
}

/// Handles one X event; false once the app should quit.
pub fn handle_event(event: Event) -> bool {
    let keep_running = settings::handle_event(&event) || dispatch(event);
    settings::paint_if_dirty();
    keep_running
}

fn dispatch(event: Event) -> bool {
    match event {
        Event::KeyPress(press) => {
            let action = STATE.with(|s| {
                let mut st = s.borrow_mut();
                let st = st.as_mut()?;
                if st.held == Some(press.detail) {
                    return None;
                }
                let state = u16::from(press.state);
                let action = st
                    .grabbed
                    .iter()
                    .find(|(_, sc)| hotkeys::matches(&st.keymap, sc, press.detail, state))
                    .map(|(action, _)| *action)?;
                st.held = Some(press.detail);
                Some(action)
            });
            if let Some(action) = action {
                run_action(action);
            }
        }
        Event::KeyRelease(release) => STATE.with(|s| {
            if let Some(st) = s.borrow_mut().as_mut()
                && st.held == Some(release.detail)
            {
                st.held = None;
            }
        }),
        Event::RandrScreenChangeNotify(_) => settings::show_displays(),
        Event::MappingNotify(notify) if notify.request != Mapping::POINTER => {
            // The old keymap still knows which keycodes were grabbed.
            unregister_all();
            STATE.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    st.keymap = Keymap::load();
                }
            });
            let _ = register_all();
        }
        Event::ClientMessage(message)
            if message.type_ == x11::shared().atoms.WRECKTANGLE_MESSAGE =>
        {
            if let Some(message) = Message::decode(message.data.as_data32()) {
                return handle_message(message);
            }
        }
        _ => {}
    }
    true
}

fn handle_message(message: Message) -> bool {
    match message {
        Message::Quit => return false,
        Message::OpenSettings => settings::open(),
        Message::CheckUpdates => update::check_now(),
        Message::ToggleStartup => {
            let _ = startup::toggle();
            tray::refresh();
        }
        Message::UpdateDone { auto, checked_at } => {
            if checked_at > 0 {
                update_config(|config| config.last_update_check = checked_at);
            }
            settings::on_update_state_changed();
            if auto {
                notify_update_available();
            }
        }
        Message::UpdateClicked => update::install(),
        Message::UpdateTimer { startup } => {
            let config = current_config();
            if config.check_updates
                && (!startup
                    || update::auto_check_due(config.last_update_check, update::now_unix()))
            {
                update::check_auto();
            }
        }
    }
    true
}

/// Ungrabs every shortcut and puts back the Xfce shortcuts it took over.
pub fn shutdown() {
    unregister_all();
    // The server must have dropped the grabs before Xfce grabs those keys
    // again; a round trip makes sure it has.
    let conn = &x11::shared().conn;
    let _ = conn.get_input_focus().map(|cookie| cookie.reply());
    xfconf::restore_all();
    tray::stop();
}

fn start_update_timer() {
    let _ = std::thread::Builder::new()
        .name("update-timer".to_string())
        .spawn(|| {
            std::thread::sleep(UPDATE_STARTUP_DELAY);
            x11::post(Message::UpdateTimer { startup: true });
            loop {
                std::thread::sleep(Duration::from_secs(update::AUTO_CHECK_INTERVAL_SECS));
                x11::post(Message::UpdateTimer { startup: false });
            }
        });
}

/// A key event read through the keyboard map: its unshifted keysym and the
/// modifiers held.
pub struct KeyInfo {
    pub keysym: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

pub fn key_info(keycode: u8, state: u16) -> KeyInfo {
    with_keymap(|keymap| {
        let mods = keymap.modifier_flags(state);
        KeyInfo {
            keysym: keymap.keysym(keycode, 0),
            ctrl: mods.0,
            alt: mods.1,
            shift: mods.2,
            win: mods.3,
        }
    })
    .unwrap_or(KeyInfo {
        keysym: 0,
        ctrl: false,
        alt: false,
        shift: false,
        win: false,
    })
}

pub fn current_config() -> Config {
    STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| st.config.clone())
            .unwrap_or_else(Config::defaults)
    })
}

/// Status text for an action whose shortcut could not simply be grabbed,
/// or None when it was.
pub fn shortcut_note(action: Action) -> Option<String> {
    let claim = STATE.with(|s| s.borrow().as_ref()?.claims.get(&action).cloned())?;
    Some(match claim {
        Claim::TakenOver(binding) => Owner {
            name: binding.owner(),
            certain: true,
        }
        .note(),
        Claim::Reserved => lang::format("status.reserved", &[("system", lang::SYSTEM)]),
        Claim::InUse => lang::t("status.in_use"),
    })
}

pub fn suspend_hotkeys() {
    unregister_all();
}

pub fn resume_hotkeys() -> Vec<Action> {
    register_all()
}

pub fn apply_new_config(new_config: Config) -> Vec<Action> {
    let _ = config::save(&config::config_path(), &new_config);
    unregister_all();
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.config = new_config;
            state.config_invalid = false;
        }
    });
    let failed = register_all();
    tray::refresh();
    failed
}

// Changes one setting without touching the grabs. While the config file on
// disk is invalid it stays untouched until Settings saves, so a hand edit is
// not overwritten behind the user's back.
pub fn update_config(change: impl FnOnce(&mut Config)) {
    let to_save = STATE.with(|s| {
        let mut state = s.borrow_mut();
        let state = state.as_mut()?;
        change(&mut state.config);
        (!state.config_invalid).then(|| state.config.clone())
    });
    if let Some(config) = to_save {
        let _ = config::save(&config::config_path(), &config);
    }
}

fn notify_update_available() {
    let update::State::Available { version, .. } = update::state() else {
        return;
    };
    let already_told = STATE.with(|s| match s.borrow_mut().as_mut() {
        Some(state) => {
            let same = state.update_notified.as_deref() == Some(version.as_str());
            state.update_notified = Some(version.clone());
            same
        }
        None => true,
    });
    if !already_told {
        notify::notify_update(&lang::format(
            "notify.update_available",
            &[("version", &version)],
        ));
    }
}

fn failed_message(failed: &[Action]) -> String {
    let config = current_config();
    let names: Vec<String> = failed
        .iter()
        .map(|&action| match config.shortcuts.get(action) {
            Some(sc) => format!("{} ({})", lang::t(action.key()), shortcut::display(&sc)),
            None => lang::t(action.key()),
        })
        .collect();
    lang::format("notify.shortcuts_failed", &[("list", &names.join(", "))])
}

fn with_keymap<R>(f: impl FnOnce(&Keymap) -> R) -> Option<R> {
    STATE.with(|s| s.borrow().as_ref().map(|st| f(&st.keymap)))
}

fn grab(sc: &Shortcut) -> Grab {
    with_keymap(|keymap| hotkeys::grab(keymap, sc)).unwrap_or(Grab::NoKey)
}

// Grabs every bound shortcut. One another client holds is taken over when
// Xfce holds it: its binding is removed, recorded, and the chord grabbed
// again once Xfce lets go. Recorded bindings whose chord is no longer bound
// are put back first. Returns the actions whose shortcut still doesn't work.
fn register_all() -> Vec<Action> {
    let entries: Vec<(Action, Shortcut)> = STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| {
                st.config
                    .shortcuts
                    .entries()
                    .into_iter()
                    .filter_map(|(action, sc)| sc.map(|sc| (action, sc)))
                    .collect()
            })
            .unwrap_or_default()
    });
    let bound: Vec<Shortcut> = entries.iter().map(|(_, sc)| *sc).collect();

    let mut record = xfconf::load_record();
    let (kept, unneeded): (Vec<Binding>, Vec<Binding>) = record
        .removed
        .into_iter()
        .partition(|binding| binding.shortcut().is_some_and(|sc| bound.contains(&sc)));
    record.removed = kept;
    for binding in unneeded {
        if !xfconf::restore(&binding) {
            record.removed.push(binding);
        }
    }

    // Every Xfce binding on a bound chord goes, not only the ones whose grab
    // fails: right after a restart Xfce may not have grabbed its key back
    // yet, and a binding left in place would then stay dead after Quit.
    let usable: Vec<Shortcut> = bound
        .iter()
        .copied()
        .filter(|sc| !takeover::is_reserved(sc))
        .collect();
    let mut removed_any = false;
    if let Some(list) = xfconf::list() {
        for binding in xfce::clashing(&usable, &list) {
            if xfconf::remove(binding) {
                if !record.removed.contains(binding) {
                    record.removed.push(binding.clone());
                }
                removed_any = true;
            }
        }
    }
    // Saved before grabbing, so a crash from here on still restores them.
    xfconf::save_record(&record);

    let mut claims = HashMap::new();
    let mut grabbed = Vec::new();
    let mut busy = Vec::new();
    for (action, sc) in entries {
        if takeover::is_reserved(&sc) {
            claims.insert(action, Claim::Reserved);
            continue;
        }
        match grab(&sc) {
            Grab::Grabbed => grabbed.push((action, sc)),
            Grab::Busy => busy.push((action, sc)),
            Grab::NoKey => {
                claims.insert(action, Claim::InUse);
            }
        }
    }
    if removed_any {
        for _ in 0..RELEASE_TRIES {
            if busy.is_empty() {
                break;
            }
            std::thread::sleep(RELEASE_WAIT);
            busy.retain(|&(action, sc)| {
                let now = matches!(grab(&sc), Grab::Grabbed);
                if now {
                    grabbed.push((action, sc));
                }
                !now
            });
        }
    }
    xfconf::save_record(&record);
    let _ = x11::shared().conn.flush();

    for (action, sc) in &grabbed {
        if let Some(binding) = record
            .removed
            .iter()
            .find(|binding| binding.shortcut() == Some(*sc))
        {
            claims.insert(*action, Claim::TakenOver(binding.clone()));
        }
    }
    for (action, _) in &busy {
        claims.insert(*action, Claim::InUse);
    }
    let unavailable = Action::ALL
        .into_iter()
        .filter(|action| {
            claims
                .get(action)
                .is_some_and(|claim| !matches!(claim, Claim::TakenOver(_)))
        })
        .collect();

    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.grabbed = grabbed;
            state.claims = claims;
            state.held = None;
        }
    });
    unavailable
}

fn unregister_all() {
    let grabbed: Vec<Shortcut> = STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| st.grabbed.iter().map(|(_, sc)| *sc).collect())
            .unwrap_or_default()
    });
    for sc in &grabbed {
        with_keymap(|keymap| hotkeys::ungrab(keymap, sc));
    }
    let _ = x11::shared().conn.flush();
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.grabbed.clear();
            state.claims.clear();
            state.held = None;
        }
    });
}

fn run_action(action: Action) {
    let config = current_config();
    let _ = window_ops::apply_action(action, &config);
}
