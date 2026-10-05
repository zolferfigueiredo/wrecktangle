//! The Settings controller: keeps the window's model in sync with the
//! config, runs what the window asks for, and records shortcuts with a
//! low-level keyboard hook. The window itself is in `window`.

mod gfx;
mod window;

use std::cell::Cell;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, PostMessageW, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::config::{self, Config};
use crate::layout::Action;
use crate::shortcut::{self, Shortcut};
use crate::ui::view::{Page, Row, SizeOption};
use crate::{app, lang, startup, takeover, update};

const MOD_BIT_CTRL: isize = 0x1;
const MOD_BIT_ALT: isize = 0x2;
const MOD_BIT_SHIFT: isize = 0x4;
const MOD_BIT_WIN: isize = 0x8;

#[derive(Clone, Copy, PartialEq, Eq)]
enum HookPhase {
    Idle,
    Recording,
    SwallowingKeyUp(u16),
}

/// What the window asks for, run once it has let go of its state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Page(Page),
    Record(usize),
    Clear(usize),
    RestoreDefaults,
    LanguageMenu,
    Language(usize),
    ToggleSize(usize),
    SetStartup(bool),
    SetAutoUpdate(bool),
    CheckNow,
    Download,
    OpenUrl(&'static str),
}

thread_local! {
    static RECORDING: Cell<Option<usize>> = const { Cell::new(None) };
    static HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static HOOK_PHASE: Cell<HookPhase> = const { Cell::new(HookPhase::Idle) };
    static HOOK_TARGET: Cell<Option<HWND>> = const { Cell::new(None) };
}

pub fn open() {
    if !window::exists() && window::create().is_err() {
        return;
    }
    if !window::is_visible() {
        refresh_all();
    }
    window::show();
}

fn run(command: Command) {
    match command {
        Command::Page(page) => {
            if window::set_page(page) {
                cancel_recording();
            }
        }
        Command::Record(index) => start_recording(index),
        Command::Clear(index) => {
            let mut config = app::current_config();
            set_shortcut_field(&mut config.shortcuts, Action::ALL[index], None);
            commit(config);
        }
        Command::RestoreDefaults => {
            let mut config = app::current_config();
            config.shortcuts = Config::defaults().shortcuts;
            commit(config);
        }
        Command::LanguageMenu => {
            // The recorder hook would swallow the menu's keys.
            cancel_recording();
            if let Some(index) = window::pick_language(lang::current_index()) {
                set_language(index);
            }
        }
        Command::Language(index) => set_language(index),
        Command::ToggleSize(index) => on_size_toggled(index),
        Command::SetStartup(enable) => set_startup(enable),
        Command::SetAutoUpdate(enable) => {
            update::set_auto(enable);
            let enabled = update::auto_enabled();
            window::update(|m| m.auto_update = enabled);
        }
        Command::CheckNow => {
            update::check_now();
            on_update_state_changed();
        }
        Command::Download => update::open_download(),
        Command::OpenUrl(url) => update::open_url(url),
    }
}

/// The window was closed; it only hides.
fn on_close() {
    cancel_recording();
}

fn refresh_all() {
    show_sizes();
    let startup = startup::is_enabled();
    let auto_update = update::auto_enabled();
    let language = lang::current_index();
    window::update(|m| {
        m.startup = startup;
        m.auto_update = auto_update;
        m.language = language;
    });
    show_update_state();
    refresh_rows();
}

fn set_language(index: usize) {
    let Some(language) = lang::LANGUAGES.get(index) else {
        return;
    };
    if lang::current_index() == index {
        return;
    }
    lang::set(language.code);
    let mut config = app::current_config();
    config.language = language.code.to_string();
    commit(config);
    window::update(|m| m.language = index);
    window::language_changed();
    show_update_state();
}

fn show_update_state() {
    let state = update::state();
    let version = lang::format("about.version", &[("version", update::current_version())]);
    let status = update::status_line(&state);
    let detail = update::status_detail(&state);
    let available = matches!(state, update::State::Available { .. });
    let busy = matches!(state, update::State::Checking);
    window::update(|m| {
        m.about_version = version;
        m.update_status = status;
        m.update_detail = detail;
        m.update_available = available;
        m.update_busy = busy;
    });
}

/// Refreshes the update row of an open Settings window.
pub fn on_update_state_changed() {
    if window::exists() {
        show_update_state();
    }
}

fn shortcut_row(action: Action, hint: Option<&str>) -> Row {
    let sc = app::current_config().shortcuts.get(action);
    let (note, warn) = match (app::shortcut_note(action), sc) {
        (Some(note), _) => (note, false),
        (None, Some(sc)) => match shortcut::altgr_char(&sc) {
            Some(ch) => (
                lang::format(
                    "status.altgr",
                    &[("char", &ch.to_string()), ("key", &shortcut::key_name(&sc))],
                ),
                true,
            ),
            None => (String::new(), false),
        },
        (None, None) => (String::new(), false),
    };
    Row {
        label: lang::t(action.key()),
        keys: sc.map(|sc| shortcut::key_caps(&sc)).unwrap_or_default(),
        note,
        warn,
        recording: hint.is_some(),
        hint: hint.unwrap_or_default().to_string(),
    }
}

fn refresh_rows() {
    let rows: Vec<Row> = Action::ALL
        .iter()
        .map(|action| shortcut_row(*action, None))
        .collect();
    window::update(|m| m.rows = rows);
}

fn show_row(index: usize, hint: Option<&str>) {
    let row = shortcut_row(Action::ALL[index], hint);
    window::update(|m| {
        if let Some(slot) = m.rows.get_mut(index) {
            *slot = row;
        }
    });
}

fn show_sizes() {
    let sizes = app::current_config().sizes;
    let checks = config::size_checks(&sizes);
    let checked_count = checks.iter().filter(|checked| **checked).count();
    let options: Vec<SizeOption> = config::SIZE_OPTIONS
        .iter()
        .enumerate()
        .map(|(index, label)| SizeOption {
            label: label.to_string(),
            checked: checks[index],
            locked: checks[index] && checked_count == 1,
        })
        .collect();
    window::update(|m| m.sizes = options);
}

fn on_size_toggled(index: usize) {
    let mut config = app::current_config();
    let updated = config::toggle_size(&config.sizes, index);
    if updated != config.sizes {
        config.sizes = updated;
        commit(config);
    }
    show_sizes();
}

fn set_startup(enable: bool) {
    if enable != startup::is_enabled() {
        let _ = startup::set_enabled(enable);
    }
    let enabled = startup::is_enabled();
    window::update(|m| m.startup = enabled);
}

// Applies and saves right away. A recording in progress ends first, because
// applying re-registers the hotkeys that recording had suspended.
fn commit(config: Config) {
    finish_recording(false);
    app::apply_new_config(config);
    if window::exists() {
        refresh_rows();
    }
}

fn start_recording(index: usize) {
    if !window::exists() || index >= Action::ALL.len() {
        return;
    }
    let previous = RECORDING.with(|r| r.replace(Some(index)));
    match previous {
        Some(previous) if previous == index => return,
        Some(previous) => show_row(previous, None),
        None => {
            app::suspend_hotkeys();
            HOOK_PHASE.with(|p| p.set(HookPhase::Recording));
            install_hook();
        }
    }
    show_row(index, Some(""));
}

fn cancel_recording() {
    if finish_recording(true).is_some() && window::exists() {
        refresh_rows();
    }
}

// Ends a recording, if one is running. `resume` re-registers the hotkeys that
// start_recording suspended; commit() skips it because applying does that.
fn finish_recording(resume: bool) -> Option<usize> {
    let row = RECORDING.with(|r| r.take())?;
    remove_hook();
    if resume {
        app::resume_hotkeys();
    }
    Some(row)
}

fn install_hook() {
    HOOK_TARGET.with(|t| t.set(Some(app::main_hwnd())));
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        if let Ok(h) = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_hook_proc),
            Some(hinstance.into()),
            0,
        ) {
            HOOK.with(|c| c.set(Some(h)));
        }
    }
}

fn remove_hook() {
    if let Some(h) = HOOK.with(|c| c.take()) {
        unsafe {
            let _ = UnhookWindowsHookEx(h);
        }
    }
    HOOK_PHASE.with(|p| p.set(HookPhase::Idle));
}

// Reads modifier state here, synchronously at the key-down, and packs it
// into the posted message. Reading it later in the window procedure would
// race a fast release of the modifiers (for example a SendInput sequence
// that sends all key-downs and then all key-ups with no delay).
extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        let is_keydown = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_keyup = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        let vk = kb.vkCode as u16;
        let is_our_dummy =
            kb.flags.contains(LLKHF_INJECTED) && kb.dwExtraInfo == takeover::DUMMY_KEY_MAGIC;

        if !is_our_dummy && !takeover::is_modifier_vk(vk) {
            let phase = HOOK_PHASE.with(|p| p.get());
            match phase {
                HookPhase::Idle => {}
                HookPhase::Recording => {
                    if is_keydown {
                        let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0;
                        let alt = unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } < 0;
                        let shift = unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0;
                        let win = unsafe { GetAsyncKeyState(VK_LWIN.0 as i32) } < 0
                            || unsafe { GetAsyncKeyState(VK_RWIN.0 as i32) } < 0;
                        let mut mods: isize = 0;
                        if ctrl {
                            mods |= MOD_BIT_CTRL;
                        }
                        if alt {
                            mods |= MOD_BIT_ALT;
                        }
                        if shift {
                            mods |= MOD_BIT_SHIFT;
                        }
                        if win {
                            mods |= MOD_BIT_WIN;
                        }

                        HOOK_PHASE.with(|p| p.set(HookPhase::SwallowingKeyUp(vk)));
                        if let Some(target) = HOOK_TARGET.with(|t| t.get()) {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(target),
                                    app::WM_APP_RECORDER_KEY,
                                    WPARAM(vk as usize),
                                    LPARAM(mods),
                                );
                            }
                        }
                        return LRESULT(1);
                    }
                    if is_keyup {
                        return LRESULT(1);
                    }
                }
                HookPhase::SwallowingKeyUp(expected) => {
                    if vk == expected && is_keyup {
                        HOOK_PHASE.with(|p| p.set(HookPhase::Recording));
                        return LRESULT(1);
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Handles a key captured by the recorder hook, posted to the main window.
pub fn on_recorder_key(vk: u16, mods: isize) {
    let Some(index) = RECORDING.with(|r| r.get()) else {
        return;
    };
    if vk == VK_ESCAPE.0 {
        cancel_recording();
        return;
    }

    let ctrl = mods & MOD_BIT_CTRL != 0;
    let alt = mods & MOD_BIT_ALT != 0;
    let shift = mods & MOD_BIT_SHIFT != 0;
    let win = mods & MOD_BIT_WIN != 0;

    if win {
        takeover::inject_dummy_key();
    }

    if !(ctrl || alt || win) {
        show_row(index, Some(&lang::t("recorder.needs_modifier")));
        return;
    }

    let candidate = Shortcut {
        ctrl,
        alt,
        shift,
        win,
        vk,
    };

    let mut config = app::current_config();
    let duplicate = Action::ALL
        .iter()
        .enumerate()
        .find(|&(i, a)| i != index && config.shortcuts.get(*a) == Some(candidate))
        .map(|(_, a)| lang::t(a.key()));
    if let Some(label) = duplicate {
        show_row(
            index,
            Some(&lang::format("recorder.duplicate", &[("action", &label)])),
        );
        return;
    }

    set_shortcut_field(&mut config.shortcuts, Action::ALL[index], Some(candidate));
    commit(config);
}

fn set_shortcut_field(
    shortcuts: &mut crate::config::Shortcuts,
    action: Action,
    value: Option<Shortcut>,
) {
    match action {
        Action::Left => shortcuts.left = value,
        Action::Right => shortcuts.right = value,
        Action::Top => shortcuts.top = value,
        Action::Bottom => shortcuts.bottom = value,
        Action::TopLeft => shortcuts.top_left = value,
        Action::TopRight => shortcuts.top_right = value,
        Action::BottomLeft => shortcuts.bottom_left = value,
        Action::BottomRight => shortcuts.bottom_right = value,
        Action::Maximize => shortcuts.maximize = value,
        Action::Center => shortcuts.center = value,
        Action::NextDisplay => shortcuts.next_display = value,
        Action::PreviousDisplay => shortcuts.previous_display = value,
    }
}
