use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, PostQuitMessage, RegisterClassExW,
    RegisterWindowMessageW, WINDOW_STYLE, WM_APP, WM_DESTROY, WM_HOTKEY, WNDCLASSEXW,
    WS_EX_TOOLWINDOW,
};
use windows::core::PCWSTR;
use windows::core::w;

use crate::config::{self, Config};
use crate::layout::Action;
use crate::shortcut::{self, Shortcut};
use crate::{tray, window_ops};

pub const WINDOW_CLASS_NAME: PCWSTR = w!("WectangleMainWindow");
pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_OPEN_SETTINGS: u32 = WM_APP + 2;

struct State {
    hwnd: HWND,
    config: Config,
    hotkeys: HashMap<i32, Action>,
    registration_failed: Vec<Action>,
    taskbar_created: u32,
    admin_notice_shown: bool,
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
            w!("Wectangle"),
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
        let (config, source) = config::load(&path);
        if source == config::Source::Missing {
            let _ = config::save(&path, &config);
        }

        let taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));

        STATE.with(|s| {
            *s.borrow_mut() = Some(State {
                hwnd,
                config,
                hotkeys: HashMap::new(),
                registration_failed: Vec::new(),
                taskbar_created,
                admin_notice_shown: false,
            });
        });

        let failed = register_all(hwnd);
        tray::add(hwnd)?;

        if source == config::Source::Invalid {
            tray::notify(
                hwnd,
                "Wectangle",
                "config.json was invalid. Using default settings until you save changes in Settings.",
            );
        }
        if !failed.is_empty() {
            tray::notify(hwnd, "Wectangle", &failed_message(&failed));
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

pub fn registration_failed() -> Vec<Action> {
    STATE.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| st.registration_failed.clone())
            .unwrap_or_default()
    })
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
        }
    });
    register_all(hwnd)
}

fn failed_message(failed: &[Action]) -> String {
    let config = current_config();
    let names: Vec<String> = failed
        .iter()
        .map(|&action| match config.shortcuts.get(action) {
            Some(sc) => format!("{} ({})", action.label(), shortcut::format(&sc)),
            None => action.label().to_string(),
        })
        .collect();
    format!("Already in use by another app: {}", names.join(", "))
}

// Copies out what to register before calling RegisterHotKey, and only takes
// the borrow again afterward to store the results, per the re-entrancy rule.
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
    let mut failed = Vec::new();
    for (id, action, sc) in entries {
        let modifiers = shortcut::hotkey_modifiers(&sc);
        let registered = unsafe { RegisterHotKey(Some(hwnd), id, modifiers, sc.vk as u32) };
        if registered.is_ok() {
            hotkeys.insert(id, action);
        } else {
            failed.push(action);
        }
    }

    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.hotkeys = hotkeys;
            state.registration_failed = failed.clone();
        }
    });

    failed
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
    STATE.with(|s| {
        if let Some(state) = s.borrow_mut().as_mut() {
            state.hotkeys.clear();
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

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => {
            let action = STATE.with(|s| {
                s.borrow()
                    .as_ref()
                    .and_then(|st| st.hotkeys.get(&(wparam.0 as i32)).copied())
            });
            let config = STATE.with(|s| s.borrow().as_ref().map(|st| st.config.clone()));
            if let (Some(action), Some(config)) = (action, config)
                && let window_ops::ActionResult::AdminBlocked =
                    window_ops::apply_action(action, &config)
                && !mark_admin_notice_shown()
            {
                tray::notify(
                    hwnd,
                    "Wectangle",
                    "That window is running as administrator and cannot be moved.",
                );
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
        WM_DESTROY => {
            unregister_all(hwnd);
            tray::remove(hwnd);
            unsafe {
                PostQuitMessage(0);
            }
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
