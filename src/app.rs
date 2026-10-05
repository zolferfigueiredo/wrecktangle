use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, KillTimer, RegisterClassExW,
    RegisterWindowMessageW, SetTimer, WINDOW_STYLE, WM_APP, WM_DESTROY, WM_HOTKEY,
    WM_SETTINGCHANGE, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW,
};
use windows::core::PCWSTR;
use windows::core::w;

use crate::config::{self, Config};
use crate::lang;
use crate::layout::Action;
use crate::shortcut::{self, Shortcut};
use crate::theme;
use crate::{owners, takeover, tray, update, window_ops};

pub const WINDOW_CLASS_NAME: PCWSTR = w!("WrecktangleMainWindow");
pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_OPEN_SETTINGS: u32 = WM_APP + 2;
pub const WM_APP_RECORDER_KEY: u32 = WM_APP + 3;
pub const WM_APP_TAKEOVER: u32 = WM_APP + 4;
pub const WM_APP_UPDATE_DONE: u32 = WM_APP + 5;

const TIMER_UPDATE_STARTUP: usize = 1;
const TIMER_UPDATE_DAILY: usize = 2;
const UPDATE_STARTUP_DELAY_MS: u32 = 30_000;
const UPDATE_DAILY_INTERVAL_MS: u32 = 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Claim {
    TakenOver,
    Reserved,
    HookFailed,
}

struct State {
    hwnd: HWND,
    config: Config,
    hotkeys: HashMap<i32, Action>,
    claims: HashMap<Action, Claim>,
    taskbar_created: u32,
    admin_notice_shown: bool,
    config_invalid: bool,
    update_notified: Option<String>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn init() -> windows::core::Result<HWND> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: WINDOW_CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            let code = windows::Win32::Foundation::GetLastError().0;
            return Err(windows::core::Error::from_hresult(
                windows::core::HRESULT::from_win32(code),
            ));
        }

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            WINDOW_CLASS_NAME,
            w!("Wrecktangle"),
            WINDOW_STYLE(0),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;

        let path = config::config_path();
        let _ = config::migrate_legacy(&config::legacy_config_path(), &path);
        crate::startup::migrate_legacy();
        let (mut config, source) = config::load(&path);
        let language_unset = config.language.is_empty();
        if language_unset {
            config.language = lang::detect_system().to_string();
        }
        lang::set(&config.language);
        if source == config::Source::Missing || (source == config::Source::Loaded && language_unset)
        {
            let _ = config::save(&path, &config);
        }

        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));

        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                hwnd,
                config,
                hotkeys: HashMap::new(),
                claims: HashMap::new(),
                taskbar_created,
                admin_notice_shown: false,
                config_invalid: source == config::Source::Invalid,
                update_notified: None,
            });
        });

        theme::apply_dark_menu(theme::current_mode() == theme::Mode::Dark);

        let failed = register_all(hwnd);
        tray::add(hwnd)?;
        SetTimer(
            Some(hwnd),
            TIMER_UPDATE_STARTUP,
            UPDATE_STARTUP_DELAY_MS,
            None,
        );
        SetTimer(
            Some(hwnd),
            TIMER_UPDATE_DAILY,
            UPDATE_DAILY_INTERVAL_MS,
            None,
        );

        if source == config::Source::Invalid {
            tray::notify(hwnd, "Wrecktangle", &lang::t("notify.config_invalid"));
        }
        if !failed.is_empty() {
            tray::notify(hwnd, "Wrecktangle", &failed_message(&failed));
        }

        Ok(hwnd)
    }
}

pub fn main_hwnd() -> HWND {
    STATE.with(|s| s.borrow().as_ref().expect("state initialized").hwnd)
}

pub fn current_config() -> Config {
    STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| st.config.clone())
            .unwrap_or_else(Config::defaults)
    })
}

/// Status text for an action whose shortcut RegisterHotKey refused, or None
/// when it is registered normally.
pub fn shortcut_note(action: Action) -> Option<String> {
    let (claim, sc) = STATE.with(|s| {
        let st = s.borrow();
        let state = st.as_ref()?;
        Some((
            state.claims.get(&action).copied()?,
            state.config.shortcuts.get(action)?,
        ))
    })?;
    Some(match claim {
        Claim::TakenOver => takeover_note(&sc),
        Claim::Reserved => lang::t("status.reserved"),
        Claim::HookFailed => lang::t("status.in_use"),
    })
}

fn takeover_note(sc: &Shortcut) -> String {
    match owners::find_owner(sc) {
        Some(owner) => owner.note(),
        None => lang::t("status.overrides_generic"),
    }
}

pub fn suspend_hotkeys() {
    unregister_all(main_hwnd());
}

pub fn resume_hotkeys() -> Vec<Action> {
    register_all(main_hwnd())
}

pub fn apply_new_config(new_config: Config) -> Vec<Action> {
    let hwnd = main_hwnd();
    let _ = config::save(&config::config_path(), &new_config);
    unregister_all(hwnd);
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.config = new_config;
            state.config_invalid = false;
        }
    });
    register_all(hwnd)
}

// Changes one setting without touching the hotkeys. While the config file on
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

fn notify_update_available(hwnd: HWND) {
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
        tray::notify_update(
            hwnd,
            "Wrecktangle",
            &lang::format("notify.update_available", &[("version", &version)]),
        );
    }
}

fn failed_message(failed: &[Action]) -> String {
    let config = current_config();
    let names: Vec<String> = failed
        .iter()
        .map(|&action| match config.shortcuts.get(action) {
            Some(sc) => format!("{} ({})", lang::t(action.key()), shortcut::format(&sc)),
            None => lang::t(action.key()),
        })
        .collect();
    lang::format("notify.shortcuts_failed", &[("list", &names.join(", "))])
}

// Copies out what to register before calling RegisterHotKey, and only takes
// the borrow again afterward to store the results, per the re-entrancy rule.
// Shortcuts RegisterHotKey refuses go to the takeover hook; the returned
// actions are the ones that still do not work.
fn register_all(hwnd: HWND) -> Vec<Action> {
    let entries: Vec<(i32, Action, Shortcut)> = STATE.with(|s| {
        let st = s.borrow();
        let state = st.as_ref().expect("state initialized");
        state
            .config
            .shortcuts
            .entries()
            .into_iter()
            .enumerate()
            .filter_map(|(index, (action, shortcut))| {
                shortcut.map(|sc| ((index + 1) as i32, action, sc))
            })
            .collect()
    });

    let mut hotkeys = HashMap::new();
    let mut refused: Vec<(Action, Shortcut)> = Vec::new();
    for (id, action, sc) in entries {
        let modifiers = shortcut::hotkey_modifiers(&sc);
        let registered = unsafe { RegisterHotKey(Some(hwnd), id, modifiers, sc.vk as u32) };
        if registered.is_ok() {
            hotkeys.insert(id, action);
        } else {
            refused.push((action, sc));
        }
    }

    let hookable: Vec<(Action, Shortcut)> = refused
        .iter()
        .filter(|(_, sc)| !takeover::is_reserved(sc))
        .copied()
        .collect();
    let hooked = takeover::start(hwnd, &hookable);

    let mut claims = HashMap::new();
    let mut unavailable = Vec::new();
    for (action, sc) in &refused {
        let claim = if takeover::is_reserved(sc) {
            Claim::Reserved
        } else if hooked {
            Claim::TakenOver
        } else {
            Claim::HookFailed
        };
        claims.insert(*action, claim);
        if claim != Claim::TakenOver {
            unavailable.push(*action);
        }
    }

    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.hotkeys = hotkeys;
            state.claims = claims;
        }
    });

    unavailable
}

fn unregister_all(hwnd: HWND) {
    let ids: Vec<i32> = STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|state| state.hotkeys.keys().copied().collect())
            .unwrap_or_default()
    });
    for id in ids {
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), id);
        }
    }
    takeover::stop();
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.hotkeys.clear();
            state.claims.clear();
        }
    });
}

// True if this is the first admin-blocked notification this session; also
// marks it shown, so the caller knows whether to notify without a second
// borrow.
fn mark_admin_notice_shown() -> bool {
    STATE.with(|s| match s.borrow_mut().as_mut() {
        Some(state) => {
            let already = state.admin_notice_shown;
            state.admin_notice_shown = true;
            already
        }
        None => true,
    })
}

fn run_action(hwnd: HWND, action: Action) {
    let config = STATE.with(|s| s.borrow().as_ref().map(|st| st.config.clone()));
    if let Some(config) = config
        && let window_ops::ActionResult::AdminBlocked = window_ops::apply_action(action, &config)
        && !mark_admin_notice_shown()
    {
        tray::notify(hwnd, "Wrecktangle", &lang::t("notify.admin_blocked"));
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let action = STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .and_then(|st| st.hotkeys.get(&(wparam.0 as i32)).copied())
            });
            if let Some(action) = action {
                run_action(hwnd, action);
            }
            LRESULT(0)
        }
        WM_APP_TAKEOVER => {
            let action = takeover::action_from_id(wparam.0).filter(|action| {
                STATE.with(|s| {
                    s.borrow()
                        .as_ref()
                        .is_some_and(|st| st.claims.get(action) == Some(&Claim::TakenOver))
                })
            });
            if let Some(action) = action {
                run_action(hwnd, action);
            }
            LRESULT(0)
        }
        WM_TIMER => {
            match wparam.0 {
                TIMER_UPDATE_STARTUP => {
                    unsafe {
                        let _ = KillTimer(Some(hwnd), TIMER_UPDATE_STARTUP);
                    }
                    let config = current_config();
                    if config.check_updates
                        && update::auto_check_due(config.last_update_check, update::now_unix())
                    {
                        update::check_auto();
                    }
                }
                TIMER_UPDATE_DAILY if current_config().check_updates => update::check_auto(),
                _ => {}
            }
            LRESULT(0)
        }
        WM_APP_UPDATE_DONE => {
            let checked_at = lparam.0.max(0) as u64;
            if checked_at > 0 {
                update_config(|config| config.last_update_check = checked_at);
            }
            crate::settings::on_update_state_changed();
            if wparam.0 != 0 {
                notify_update_available(hwnd);
            }
            LRESULT(0)
        }
        WM_APP_TRAY => {
            tray::handle_callback(hwnd, wparam, lparam);
            LRESULT(0)
        }
        WM_APP_OPEN_SETTINGS => {
            crate::settings::open();
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            if theme::is_immersive_color_set_change(lparam) {
                theme::apply_dark_menu(theme::current_mode() == theme::Mode::Dark);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_APP_RECORDER_KEY => {
            crate::settings::on_recorder_key(wparam.0 as u16, lparam.0);
            LRESULT(0)
        }
        WM_DESTROY => {
            unregister_all(hwnd);
            tray::remove(hwnd);
            let _ = slint::quit_event_loop();
            LRESULT(0)
        }
        _ => {
            let is_taskbar_created = STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .map(|st| st.taskbar_created != 0 && st.taskbar_created == msg)
                    .unwrap_or(false)
            });
            if is_taskbar_created {
                let _ = tray::add(hwnd);
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}
