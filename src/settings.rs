use std::cell::{Cell, RefCell};
use std::time::Duration;

use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Controls::{LIM_LARGE, LIM_SMALL, LoadIconMetric};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, HHOOK, ICON_BIG, ICON_SMALL, KBDLLHOOKSTRUCT, LLKHF_INJECTED, PostMessageW,
    SendMessageW, SetForegroundWindow, SetWindowsHookExW, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_SETICON, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::{PCWSTR, w};

use crate::config::{self, Config};
use crate::layout::Action;
use crate::shortcut::{self, Shortcut};
use crate::theme;
use crate::{app, startup, takeover, update};

slint::include_modules!();

const WINDOW_TITLE: PCWSTR = w!("Wectangle Settings");

const MOD_BIT_CTRL: isize = 0x1;
const MOD_BIT_ALT: isize = 0x2;
const MOD_BIT_SHIFT: isize = 0x4;
const MOD_BIT_WIN: isize = 0x8;

const RAISE_RETRY_MS: u64 = 25;
const RAISE_MAX_ATTEMPTS: u32 = 40;

#[derive(Clone, Copy, PartialEq, Eq)]
enum HookPhase {
    Idle,
    Recording,
    SwallowingKeyUp(u16),
}

thread_local! {
    static WINDOW: RefCell<Option<SettingsWindow>> = const { RefCell::new(None) };
    static RECORDING: Cell<Option<usize>> = const { Cell::new(None) };
    static HOOK: Cell<Option<HHOOK>> = const { Cell::new(None) };
    static HOOK_PHASE: Cell<HookPhase> = const { Cell::new(HookPhase::Idle) };
    static HOOK_TARGET: Cell<Option<HWND>> = const { Cell::new(None) };
}

fn window() -> Option<SettingsWindow> {
    WINDOW.with(|w| w.borrow().as_ref().map(|ui| ui.clone_strong()))
}

pub fn open() {
    let ui = match window() {
        Some(ui) => ui,
        None => {
            let Ok(ui) = create() else { return };
            WINDOW.with(|w| *w.borrow_mut() = Some(ui.clone_strong()));
            ui
        }
    };
    if !ui.window().is_visible() {
        refresh_all(&ui);
    }
    let _ = ui.show();
    raise(0);
}

fn create() -> Result<SettingsWindow, slint::PlatformError> {
    let ui = SettingsWindow::new()?;
    ui.set_rows(ModelRc::new(VecModel::from(vec![
        ShortcutRow::default();
        Action::ALL.len()
    ])));
    ui.set_theme_override(match theme::override_mode() {
        None => 0,
        Some(theme::Mode::Light) => 1,
        Some(theme::Mode::Dark) => 2,
    });

    ui.on_record(|index| start_recording(index as usize));
    ui.on_cancel_recording(cancel_recording);
    ui.on_clear(|index| {
        let mut config = app::current_config();
        set_shortcut_field(&mut config.shortcuts, Action::ALL[index as usize], None);
        commit(config);
    });
    ui.on_restore_defaults(|| {
        let mut config = app::current_config();
        config.shortcuts = Config::defaults().shortcuts;
        commit(config);
    });
    ui.on_apply_sizes(|text| apply_sizes(&text).into());
    ui.on_set_startup(set_startup);
    ui.on_set_auto_update(update::set_auto);
    ui.on_check_now(|| {
        update::check_now();
        on_update_state_changed();
    });
    ui.on_download(update::open_download);
    ui.window().on_close_requested(|| {
        cancel_recording();
        CloseRequestResponse::HideWindow
    });
    Ok(ui)
}

fn refresh_all(ui: &SettingsWindow) {
    let config = app::current_config();
    ui.set_sizes_text(config.sizes.join(", ").into());
    ui.set_sizes_error(SharedString::new());
    ui.set_startup_enabled(startup::is_enabled());
    ui.set_auto_update(update::auto_enabled());
    ui.set_version_label(format!("Wectangle {}", update::current_version()).into());
    show_update_state(ui);
    refresh_rows(ui);
}

fn show_update_state(ui: &SettingsWindow) {
    let state = update::state();
    ui.set_update_status(update::status_line(&state).into());
    ui.set_update_available(matches!(state, update::State::Available { .. }));
    ui.set_update_busy(matches!(state, update::State::Checking));
}

/// Refreshes the update row of an open Settings window.
pub fn on_update_state_changed() {
    if let Some(ui) = window() {
        show_update_state(&ui);
    }
}

fn shortcut_row(action: Action, hint: Option<&str>) -> ShortcutRow {
    let sc = app::current_config().shortcuts.get(action);
    let (note, warn) = match (app::shortcut_note(action), sc) {
        (Some(note), _) => (note, false),
        (None, Some(sc)) => match shortcut::altgr_char(&sc) {
            Some(ch) => (
                format!(
                    "Also blocks typing {ch} (AltGr+{})",
                    shortcut::key_name(&sc)
                ),
                true,
            ),
            None => (String::new(), false),
        },
        (None, None) => (String::new(), false),
    };
    let caps: Vec<SharedString> = sc
        .map(|sc| shortcut::key_caps(&sc))
        .unwrap_or_default()
        .into_iter()
        .map(SharedString::from)
        .collect();
    ShortcutRow {
        keys: ModelRc::new(VecModel::from(caps)),
        note: note.into(),
        warn,
        recording: hint.is_some(),
        hint: hint.unwrap_or_default().into(),
    }
}

fn refresh_rows(ui: &SettingsWindow) {
    let model = ui.get_rows();
    for (index, action) in Action::ALL.iter().enumerate() {
        model.set_row_data(index, shortcut_row(*action, None));
    }
}

fn show_row(ui: &SettingsWindow, index: usize, hint: Option<&str>) {
    ui.get_rows()
        .set_row_data(index, shortcut_row(Action::ALL[index], hint));
}

// Slint creates the native window once the event loop is back in control, so
// the handle may not exist yet right after show().
fn raise(attempt: u32) {
    match find_window() {
        Some(hwnd) => {
            set_window_icons(hwnd);
            unsafe {
                let _ = SetForegroundWindow(hwnd);
            }
        }
        None if attempt < RAISE_MAX_ATTEMPTS => {
            Timer::single_shot(Duration::from_millis(RAISE_RETRY_MS), move || {
                raise(attempt + 1)
            });
        }
        None => {}
    }
}

fn find_window() -> Option<HWND> {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
    unsafe {
        let hwnd = FindWindowW(PCWSTR::null(), WINDOW_TITLE).ok()?;
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (pid == GetCurrentProcessId()).then_some(hwnd)
    }
}

fn set_window_icons(hwnd: HWND) {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        // MAKEINTRESOURCEW(1): the pointer value itself is resource id 1
        // (assets/wectangle.rc), not a real memory address.
        #[allow(clippy::manual_dangling_ptr)]
        let name = PCWSTR(1usize as *const u16);
        if let Ok(big) = LoadIconMetric(Some(hinstance.into()), name, LIM_LARGE) {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_BIG as usize)),
                Some(LPARAM(big.0 as isize)),
            );
        }
        if let Ok(small) = LoadIconMetric(Some(hinstance.into()), name, LIM_SMALL) {
            SendMessageW(
                hwnd,
                WM_SETICON,
                Some(WPARAM(ICON_SMALL as usize)),
                Some(LPARAM(small.0 as isize)),
            );
        }
    }
}

fn apply_sizes(text: &str) -> String {
    let sizes = match config::parse_size_list(text) {
        Ok(sizes) => sizes,
        Err(message) => return message.to_string(),
    };
    let mut config = app::current_config();
    if config.sizes != sizes {
        config.sizes = sizes;
        commit(config);
    }
    String::new()
}

fn set_startup(enable: bool) {
    if enable != startup::is_enabled() {
        let _ = startup::set_enabled(enable);
    }
    if let Some(ui) = window() {
        ui.set_startup_enabled(startup::is_enabled());
    }
}

// Applies and saves right away. A recording in progress ends first, because
// applying re-registers the hotkeys that recording had suspended.
fn commit(config: Config) {
    finish_recording(false);
    app::apply_new_config(config);
    if let Some(ui) = window() {
        refresh_rows(&ui);
    }
}

fn start_recording(index: usize) {
    let Some(ui) = window() else { return };
    let previous = RECORDING.with(|r| r.replace(Some(index)));
    match previous {
        Some(previous) if previous == index => return,
        Some(previous) => show_row(&ui, previous, None),
        None => {
            app::suspend_hotkeys();
            HOOK_PHASE.with(|p| p.set(HookPhase::Recording));
            install_hook();
        }
    }
    show_row(&ui, index, Some(""));
}

fn cancel_recording() {
    if finish_recording(true).is_some()
        && let Some(ui) = window()
    {
        refresh_rows(&ui);
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
    let Some(ui) = window() else { return };

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
        show_row(&ui, index, Some("Needs at least one of Ctrl, Alt or Win."));
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
        .map(|(_, a)| a.label());
    if let Some(label) = duplicate {
        show_row(&ui, index, Some(&format!("Already used by {label}.")));
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
