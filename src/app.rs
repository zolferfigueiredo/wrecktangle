use std::cell::RefCell;
use std::collections::HashMap;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey,
};
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
    config: Config,
    hotkeys: HashMap<i32, Action>,
    taskbar_created: u32,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Creates the hidden main window, loads config, registers hotkeys and adds
/// the tray icon. Returns the window handle for the caller's message loop.
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
                config,
                hotkeys: HashMap::new(),
                taskbar_created,
            });
        });

        let failed = register_hotkeys(hwnd);
        tray::add(hwnd)?;

        if source == config::Source::Invalid {
            tray::notify(
                hwnd,
                "Wectangle",
                "config.json was invalid. Using default settings until you save changes in Settings.",
            );
        }
        if !failed.is_empty() {
            tray::notify(
                hwnd,
                "Wectangle",
                &format!("Already in use by another app: {}", failed.join(", ")),
            );
        }

        Ok(hwnd)
    }
}

fn modifiers_for(sc: &Shortcut) -> HOT_KEY_MODIFIERS {
    let mut m = MOD_NOREPEAT;
    if sc.ctrl {
        m |= MOD_CONTROL;
    }
    if sc.alt {
        m |= MOD_ALT;
    }
    if sc.shift {
        m |= MOD_SHIFT;
    }
    if sc.win {
        m |= MOD_WIN;
    }
    m
}

// Called only during init, before the message loop runs, so no re-entrancy
// concern arises from registering while holding the state borrow.
fn register_hotkeys(hwnd: HWND) -> Vec<String> {
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        let state = st
            .as_mut()
            .expect("state initialized before register_hotkeys");
        let mut hotkeys = HashMap::new();
        let mut failed = Vec::new();

        for (index, (action, shortcut)) in state.config.shortcuts.entries().into_iter().enumerate()
        {
            let Some(sc) = shortcut else { continue };
            let id = (index + 1) as i32;
            let modifiers = modifiers_for(&sc);
            let registered = unsafe { RegisterHotKey(Some(hwnd), id, modifiers, sc.vk as u32) };
            if registered.is_ok() {
                hotkeys.insert(id, action);
            } else {
                failed.push(format!("{} ({})", action.label(), shortcut::format(&sc)));
            }
        }

        state.hotkeys = hotkeys;
        failed
    })
}

fn unregister_hotkeys(hwnd: HWND) {
    STATE.with(|s| {
        if let Some(state) = s.borrow().as_ref() {
            for id in state.hotkeys.keys() {
                unsafe {
                    let _ = UnregisterHotKey(Some(hwnd), *id);
                }
            }
        }
    });
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
            unregister_hotkeys(hwnd);
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
