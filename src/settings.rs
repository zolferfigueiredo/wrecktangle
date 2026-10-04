use std::cell::{Cell, RefCell};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    COLOR_BTNFACE, CreateFontIndirectW, DeleteObject, GetSysColorBrush, LOGFONTW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, LIM_LARGE, LIM_SMALL,
    LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVCFMT_LEFT, LVCOLUMNW, LVIF_TEXT, LVITEMW,
    LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVM_SETITEMTEXTW, LVNI_SELECTED, LVS_EX_FULLROWSELECT, LVS_REPORT, LVS_SHOWSELALWAYS,
    LVS_SINGLESEL, LoadIconMetric, NM_DBLCLK, NMHDR, NMITEMACTIVATE, WC_LISTVIEW,
};
use windows::Win32::UI::HiDpi::{
    AdjustWindowRectExForDpi, GetDpiForWindow, SystemParametersInfoForDpi,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetFocus, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_CONTROL, VK_ESCAPE, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_MENU, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CW_USEDEFAULT, CreateWindowExW, DefWindowProcW, DestroyWindow, ES_AUTOHSCROLL, GetWindowTextW,
    HMENU, ICON_BIG, ICON_SMALL, IDCANCEL, IDOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, PostMessageW,
    RegisterClassExW, SW_SHOW, SendMessageW, SetForegroundWindow, SetWindowPos, SetWindowTextW,
    ShowWindow, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_DPICHANGED, WM_KEYDOWN, WM_KEYUP, WM_NOTIFY, WM_SETFONT, WM_SETICON, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WNDCLASSEXW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_GROUP, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{Error, HRESULT, PCWSTR, PWSTR, Result, w};

use crate::config::{self, Config};
use crate::layout::Action;
use crate::shortcut::{self, Shortcut};
use crate::{app, startup};

const WINDOW_CLASS_NAME: PCWSTR = w!("WectangleSettingsWindow");

const ID_LISTVIEW: i32 = 101;
const ID_CHANGE: i32 = 102;
const ID_CLEAR: i32 = 103;
const ID_RESTORE: i32 = 104;
const ID_SIZES_EDIT: i32 = 105;
const ID_STARTUP_CHECK: i32 = 106;
const ID_SAVE: i32 = IDOK.0;
const ID_CANCEL: i32 = IDCANCEL.0;
const ID_TEST_HOTKEY: i32 = 999;

const WM_APP_RECORDER_KEY: u32 = WM_APP + 30;

const DUMMY_VK: u16 = 0xFF;
const DUMMY_KEY_MAGIC: usize = 0x5745_4354;

const BASE_W: i32 = 540;
const BASE_H: i32 = 430;
const MARGIN: i32 = 12;
const LIST_Y: i32 = 12;
const LIST_H: i32 = 230;

#[derive(Clone, Copy)]
struct Hwnds {
    main: HWND,
    listview: HWND,
    change_btn: HWND,
    clear_btn: HWND,
    restore_btn: HWND,
    sizes_edit: HWND,
    startup_check: HWND,
    save_btn: HWND,
    cancel_btn: HWND,
    font: windows::Win32::Graphics::Gdi::HFONT,
}

struct Staged {
    shortcuts: crate::config::Shortcuts,
    recording_row: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HookPhase {
    Idle,
    Recording,
    SwallowingKeyUp(u16),
}

thread_local! {
    static HWNDS: Cell<Option<Hwnds>> = const { Cell::new(None) };
    static STAGED: RefCell<Option<Staged>> = const { RefCell::new(None) };
    static HOOK: Cell<Option<windows::Win32::UI::WindowsAndMessaging::HHOOK>> = const { Cell::new(None) };
    static HOOK_PHASE: Cell<HookPhase> = const { Cell::new(HookPhase::Idle) };
    static CLASS_REGISTERED: Cell<bool> = const { Cell::new(false) };
}

pub fn open() {
    if let Some(h) = HWNDS.with(|c| c.get()) {
        unsafe {
            let _ = SetForegroundWindow(h.main);
        }
        return;
    }
    if let Err(_e) = create_window() {
        // Nothing sensible to show if the settings window itself cannot be
        // created; the tray icon and hotkeys keep working regardless.
    }
}

pub fn hwnd() -> Option<HWND> {
    HWNDS.with(|c| c.get()).map(|h| h.main)
}

fn create_window() -> Result<()> {
    unsafe {
        let hinstance = GetModuleHandleW(None)?;

        if !CLASS_REGISTERED.with(|c| c.get()) {
            let _ = InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_LISTVIEW_CLASSES,
            });

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: WINDOW_CLASS_NAME,
                hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
                ..Default::default()
            };
            if RegisterClassExW(&wc) == 0 {
                let code = windows::Win32::Foundation::GetLastError().0;
                return Err(Error::from_hresult(HRESULT::from_win32(code)));
            }
            CLASS_REGISTERED.with(|c| c.set(true));
        }

        let dpi_guess = 96u32;
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: BASE_W,
            bottom: BASE_H,
        };
        let style = WS_CAPTION | WS_SYSMENU;
        let _ = AdjustWindowRectExForDpi(&mut rect, style, false, WINDOW_EX_STYLE(0), dpi_guess);

        let main = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            WINDOW_CLASS_NAME,
            w!("Wectangle Settings"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(hinstance.into()),
            None,
        )?;

        let dpi = GetDpiForWindow(main).max(1);
        let font = make_font(dpi);
        let hwnds = create_controls(main, font, dpi)?;
        HWNDS.with(|c| c.set(Some(hwnds)));

        let config = app::current_config();
        STAGED.with(|s| {
            *s.borrow_mut() = Some(Staged {
                shortcuts: config.shortcuts.clone(),
                recording_row: None,
            });
        });

        populate_listview(&hwnds, &config.shortcuts);
        let sizes_text = config.sizes.join(", ");
        let _ = SetWindowTextW(
            hwnds.sizes_edit,
            PCWSTR::from_raw(to_wide(&sizes_text).as_ptr()),
        );
        let checked = if startup::is_enabled() {
            1usize
        } else {
            0usize
        };
        SendMessageW(
            hwnds.startup_check,
            windows::Win32::UI::WindowsAndMessaging::BM_SETCHECK,
            Some(WPARAM(checked)),
            Some(LPARAM(0)),
        );

        set_window_icons(main);
        let _ = ShowWindow(main, SW_SHOW);
        let _ = SetForegroundWindow(main);
        Ok(())
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

fn make_font(dpi: u32) -> windows::Win32::Graphics::Gdi::HFONT {
    unsafe {
        let mut ncm = windows::Win32::UI::WindowsAndMessaging::NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<windows::Win32::UI::WindowsAndMessaging::NONCLIENTMETRICSW>(
            ) as u32,
            ..Default::default()
        };
        let ok = SystemParametersInfoForDpi(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETNONCLIENTMETRICS.0,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut core::ffi::c_void),
            0,
            dpi,
        );
        let lf = if ok.is_ok() {
            ncm.lfMessageFont
        } else {
            LOGFONTW::default()
        };
        CreateFontIndirectW(&lf)
    }
}

fn scale(v: i32, dpi: u32) -> i32 {
    (v as f64 * dpi as f64 / 96.0).round() as i32
}

struct Rects {
    listview: RECT,
    change_btn: RECT,
    clear_btn: RECT,
    restore_btn: RECT,
    size_label: RECT,
    sizes_edit: RECT,
    size_hint: RECT,
    startup_check: RECT,
    save_btn: RECT,
    cancel_btn: RECT,
}

fn rect_at(x: i32, y: i32, w: i32, h: i32, dpi: u32) -> RECT {
    RECT {
        left: scale(x, dpi),
        top: scale(y, dpi),
        right: scale(x + w, dpi),
        bottom: scale(y + h, dpi),
    }
}

fn compute_layout(dpi: u32) -> Rects {
    let row_btn_y = LIST_Y + LIST_H + 8;
    let row_btn_h = 24;
    let size_label_y = row_btn_y + row_btn_h + 12;
    let size_edit_y = size_label_y + 18;
    let size_hint_y = size_edit_y + 24;
    let startup_y = size_hint_y + 22;
    let bottom_btn_y = startup_y + 28;
    let bottom_btn_h = 26;
    let content_w = BASE_W - 2 * MARGIN;

    Rects {
        listview: rect_at(MARGIN, LIST_Y, content_w, LIST_H, dpi),
        change_btn: rect_at(MARGIN, row_btn_y, 90, row_btn_h, dpi),
        clear_btn: rect_at(MARGIN + 98, row_btn_y, 70, row_btn_h, dpi),
        restore_btn: rect_at(MARGIN + 176, row_btn_y, 120, row_btn_h, dpi),
        size_label: rect_at(MARGIN, size_label_y, content_w, 16, dpi),
        sizes_edit: rect_at(MARGIN, size_edit_y, content_w, 22, dpi),
        size_hint: rect_at(MARGIN, size_hint_y, content_w, 16, dpi),
        startup_check: rect_at(MARGIN, startup_y, content_w, 20, dpi),
        save_btn: rect_at(BASE_W - MARGIN - 168, bottom_btn_y, 80, bottom_btn_h, dpi),
        cancel_btn: rect_at(BASE_W - MARGIN - 80, bottom_btn_y, 80, bottom_btn_h, dpi),
    }
}

fn create_controls(
    main: HWND,
    font: windows::Win32::Graphics::Gdi::HFONT,
    dpi: u32,
) -> Result<Hwnds> {
    let r = compute_layout(dpi);
    unsafe {
        let hinstance = GetModuleHandleW(None)?;

        let listview = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            WC_LISTVIEW,
            w!(""),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WINDOW_STYLE(LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS),
            r.listview.left,
            r.listview.top,
            r.listview.right - r.listview.left,
            r.listview.bottom - r.listview.top,
            Some(main),
            Some(HMENU(ID_LISTVIEW as *mut core::ffi::c_void)),
            Some(hinstance.into()),
            None,
        )?;
        SendMessageW(
            listview,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(LVS_EX_FULLROWSELECT as usize)),
            Some(LPARAM(LVS_EX_FULLROWSELECT as isize)),
        );
        let content_w = r.listview.right - r.listview.left;
        let action_w = scale(185, dpi);
        let shortcut_w = scale(125, dpi);
        let status_w = (content_w - action_w - shortcut_w).max(scale(80, dpi));
        insert_column(listview, 0, "Action", action_w);
        insert_column(listview, 1, "Shortcut", shortcut_w);
        insert_column(listview, 2, "Status", status_w);

        let change_btn =
            create_button(main, hinstance, "Change...", ID_CHANGE, r.change_btn, false)?;
        let clear_btn = create_button(main, hinstance, "Clear", ID_CLEAR, r.clear_btn, false)?;
        let restore_btn = create_button(
            main,
            hinstance,
            "Restore defaults",
            ID_RESTORE,
            r.restore_btn,
            false,
        )?;

        let size_label = create_static(
            main,
            hinstance,
            "Size cycle (repeat a shortcut to step through)",
            r.size_label,
        )?;

        let sizes_edit = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            windows::Win32::UI::Controls::WC_EDIT,
            w!(""),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            r.sizes_edit.left,
            r.sizes_edit.top,
            r.sizes_edit.right - r.sizes_edit.left,
            r.sizes_edit.bottom - r.sizes_edit.top,
            Some(main),
            Some(HMENU(ID_SIZES_EDIT as *mut core::ffi::c_void)),
            Some(hinstance.into()),
            None,
        )?;

        let hint_label = create_static(
            main,
            hinstance,
            "Fractions or percentages, separated by commas.",
            r.size_hint,
        )?;

        let startup_check = create_button(
            main,
            hinstance,
            "Launch Wectangle when Windows starts",
            ID_STARTUP_CHECK,
            r.startup_check,
            true,
        )?;

        let save_btn = create_default_button(main, hinstance, "Save", ID_SAVE, r.save_btn)?;
        let cancel_btn = create_button(main, hinstance, "Cancel", ID_CANCEL, r.cancel_btn, false)?;

        for ctl in [
            listview,
            change_btn,
            clear_btn,
            restore_btn,
            size_label,
            sizes_edit,
            hint_label,
            startup_check,
            save_btn,
            cancel_btn,
        ] {
            SendMessageW(
                ctl,
                WM_SETFONT,
                Some(WPARAM(font.0 as usize)),
                Some(LPARAM(1)),
            );
        }

        Ok(Hwnds {
            main,
            listview,
            change_btn,
            clear_btn,
            restore_btn,
            sizes_edit,
            startup_check,
            save_btn,
            cancel_btn,
            font,
        })
    }
}

fn create_button(
    parent: HWND,
    hinstance: windows::Win32::Foundation::HMODULE,
    text: &str,
    id: i32,
    r: RECT,
    checkbox: bool,
) -> Result<HWND> {
    let style_bits = if checkbox {
        windows::Win32::UI::WindowsAndMessaging::BS_AUTOCHECKBOX as u32
    } else {
        windows::Win32::UI::WindowsAndMessaging::BS_PUSHBUTTON as u32
    };
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::Win32::UI::Controls::WC_BUTTON,
            PCWSTR::from_raw(to_wide(text).as_ptr()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_GROUP | WINDOW_STYLE(style_bits),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            Some(parent),
            Some(HMENU(id as *mut core::ffi::c_void)),
            Some(hinstance.into()),
            None,
        )
    }
}

fn create_default_button(
    parent: HWND,
    hinstance: windows::Win32::Foundation::HMODULE,
    text: &str,
    id: i32,
    r: RECT,
) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::Win32::UI::Controls::WC_BUTTON,
            PCWSTR::from_raw(to_wide(text).as_ptr()),
            WS_CHILD
                | WS_VISIBLE
                | WS_TABSTOP
                | WS_GROUP
                | WINDOW_STYLE(windows::Win32::UI::WindowsAndMessaging::BS_DEFPUSHBUTTON as u32),
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            Some(parent),
            Some(HMENU(id as *mut core::ffi::c_void)),
            Some(hinstance.into()),
            None,
        )
    }
}

fn create_static(
    parent: HWND,
    hinstance: windows::Win32::Foundation::HMODULE,
    text: &str,
    r: RECT,
) -> Result<HWND> {
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::Win32::UI::Controls::WC_STATIC,
            PCWSTR::from_raw(to_wide(text).as_ptr()),
            WS_CHILD | WS_VISIBLE,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            Some(parent),
            None,
            Some(hinstance.into()),
            None,
        )
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn insert_column(listview: HWND, index: i32, title: &str, width: i32) {
    let mut wide = to_wide(title);
    let col = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
        fmt: LVCFMT_LEFT,
        cx: width,
        pszText: PWSTR(wide.as_mut_ptr()),
        iSubItem: index,
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            listview,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(index as usize)),
            Some(LPARAM(&col as *const _ as isize)),
        );
    }
}

fn insert_row(listview: HWND, row: i32, text: &str) {
    let mut wide = to_wide(text);
    let item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: row,
        iSubItem: 0,
        pszText: PWSTR(wide.as_mut_ptr()),
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            listview,
            LVM_INSERTITEMW,
            Some(WPARAM(0)),
            Some(LPARAM(&item as *const _ as isize)),
        );
    }
}

fn set_item_text(listview: HWND, row: i32, col: i32, text: &str) {
    let mut wide = to_wide(text);
    let item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: row,
        iSubItem: col,
        pszText: PWSTR(wide.as_mut_ptr()),
        ..Default::default()
    };
    unsafe {
        SendMessageW(
            listview,
            LVM_SETITEMTEXTW,
            Some(WPARAM(row as usize)),
            Some(LPARAM(&item as *const _ as isize)),
        );
    }
}

fn selected_row(listview: HWND) -> Option<usize> {
    let result = unsafe {
        SendMessageW(
            listview,
            LVM_GETNEXTITEM,
            Some(WPARAM(usize::MAX)),
            Some(LPARAM(LVNI_SELECTED as isize)),
        )
    };
    if result.0 < 0 {
        None
    } else {
        Some(result.0 as usize)
    }
}

fn status_text(sc: Option<Shortcut>, in_use: bool) -> String {
    if in_use {
        return "In use by another app".to_string();
    }
    match sc.and_then(|s| shortcut::altgr_char(&s)) {
        Some(ch) => format!(
            "Also blocks typing {ch} (AltGr+{})",
            shortcut::key_name(&sc.unwrap())
        ),
        None => String::new(),
    }
}

fn populate_listview(h: &Hwnds, shortcuts: &crate::config::Shortcuts) {
    let failed = app::registration_failed();
    for (row, action) in Action::ALL.iter().enumerate() {
        insert_row(h.listview, row as i32, action.label());
        let sc = shortcuts.get(*action);
        set_item_text(
            h.listview,
            row as i32,
            1,
            &sc.map(|s| shortcut::format(&s)).unwrap_or_default(),
        );
        set_item_text(
            h.listview,
            row as i32,
            2,
            &status_text(sc, failed.contains(action)),
        );
    }
}

fn refresh_row(h: &Hwnds, row: usize) {
    let sc = STAGED.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| st.shortcuts.get(Action::ALL[row]))
    });
    let Some(sc) = sc else { return };
    set_item_text(
        h.listview,
        row as i32,
        1,
        &sc.map(|s| shortcut::format(&s)).unwrap_or_default(),
    );
    set_item_text(h.listview, row as i32, 2, &status_text(sc, false));
}

fn set_row_text(h: &Hwnds, row: usize, shortcut_col: &str, status_col: &str) {
    set_item_text(h.listview, row as i32, 1, shortcut_col);
    set_item_text(h.listview, row as i32, 2, status_col);
}

fn start_recording(h: &Hwnds, row: usize) {
    let already_recording = STAGED.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| st.recording_row.is_some())
            .unwrap_or(true)
    });
    if already_recording {
        return;
    }
    app::suspend_hotkeys();
    STAGED.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.recording_row = Some(row);
        }
    });
    set_row_text(h, row, "Press the new shortcut... (Esc cancels)", "");
    HOOK_PHASE.with(|p| p.set(HookPhase::Recording));
    install_hook();
}

fn stop_recording(h: &Hwnds) -> Option<usize> {
    remove_hook();
    let row = STAGED.with(|s| {
        s.borrow_mut()
            .as_mut()
            .and_then(|st| st.recording_row.take())
    });
    if row.is_some() {
        app::resume_hotkeys();
    }
    let _ = h;
    row
}

fn install_hook() {
    unsafe {
        let hinstance = GetModuleHandleW(None).unwrap_or_default();
        if let Ok(h) = windows::Win32::UI::WindowsAndMessaging::SetWindowsHookExW(
            windows::Win32::UI::WindowsAndMessaging::WH_KEYBOARD_LL,
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
            let _ = windows::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(h);
        }
    }
    HOOK_PHASE.with(|p| p.set(HookPhase::Idle));
}

fn is_modifier_vk(vk: u16) -> bool {
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

fn inject_dummy_key() {
    let down = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(DUMMY_VK),
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: DUMMY_KEY_MAGIC,
            },
        },
    };
    let up = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(DUMMY_VK),
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: DUMMY_KEY_MAGIC,
            },
        },
    };
    unsafe {
        SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
    }
}

extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        let is_keydown = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let is_keyup = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        let vk = kb.vkCode as u16;
        let is_our_dummy = kb.flags.contains(LLKHF_INJECTED) && kb.dwExtraInfo == DUMMY_KEY_MAGIC;

        if !is_our_dummy && !is_modifier_vk(vk) {
            let phase = HOOK_PHASE.with(|p| p.get());
            match phase {
                HookPhase::Idle => {}
                HookPhase::Recording => {
                    if is_keydown {
                        HOOK_PHASE.with(|p| p.set(HookPhase::SwallowingKeyUp(vk)));
                        if let Some(h) = HWNDS.with(|c| c.get()) {
                            unsafe {
                                let _ = PostMessageW(
                                    Some(h.main),
                                    WM_APP_RECORDER_KEY,
                                    WPARAM(vk as usize),
                                    LPARAM(0),
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
    unsafe { windows::Win32::UI::WindowsAndMessaging::CallNextHookEx(None, code, wparam, lparam) }
}

fn on_recorder_key(h: &Hwnds, vk: u16) {
    let Some(row) = STAGED.with(|s| s.borrow().as_ref().and_then(|st| st.recording_row)) else {
        return;
    };

    if vk == VK_ESCAPE.0 {
        stop_recording(h);
        refresh_row(h, row);
        return;
    }

    let ctrl = unsafe { GetAsyncKeyState(VK_CONTROL.0 as i32) } < 0;
    let alt = unsafe { GetAsyncKeyState(VK_MENU.0 as i32) } < 0;
    let shift = unsafe { GetAsyncKeyState(VK_SHIFT.0 as i32) } < 0;
    let win =
        unsafe { GetAsyncKeyState(VK_LWIN.0 as i32) < 0 || GetAsyncKeyState(VK_RWIN.0 as i32) < 0 };

    if win {
        inject_dummy_key();
    }

    if !(ctrl || alt || win) {
        set_row_text(
            h,
            row,
            "Press the new shortcut... (Esc cancels)",
            "Needs at least one of Ctrl, Alt or Win.",
        );
        return;
    }

    let candidate = Shortcut {
        ctrl,
        alt,
        shift,
        win,
        vk,
    };

    let duplicate = STAGED.with(|s| {
        s.borrow().as_ref().and_then(|st| {
            Action::ALL
                .iter()
                .enumerate()
                .find(|&(i, a)| i != row && st.shortcuts.get(*a) == Some(candidate))
                .map(|(_, a)| a.label())
        })
    });
    if let Some(label) = duplicate {
        set_row_text(
            h,
            row,
            "Press the new shortcut... (Esc cancels)",
            &format!("Already used by {label}."),
        );
        return;
    }

    let in_use = !test_register(h.main, candidate);
    stage_shortcut(row, Some(candidate));
    stop_recording(h);
    set_row_text(
        h,
        row,
        &shortcut::format(&candidate),
        &status_text(Some(candidate), in_use),
    );
}

fn test_register(hwnd: HWND, sc: Shortcut) -> bool {
    let modifiers = shortcut::hotkey_modifiers(&sc);
    unsafe {
        let ok = windows::Win32::UI::Input::KeyboardAndMouse::RegisterHotKey(
            Some(hwnd),
            ID_TEST_HOTKEY,
            modifiers,
            sc.vk as u32,
        )
        .is_ok();
        if ok {
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey(
                Some(hwnd),
                ID_TEST_HOTKEY,
            );
        }
        ok
    }
}

fn stage_shortcut(row: usize, value: Option<Shortcut>) {
    STAGED.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            set_shortcut_field(&mut st.shortcuts, Action::ALL[row], value);
        }
    });
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

fn do_change(h: &Hwnds) {
    if let Some(row) = selected_row(h.listview) {
        start_recording(h, row);
    }
}

fn do_clear(h: &Hwnds) {
    if let Some(row) = selected_row(h.listview) {
        stage_shortcut(row, None);
        set_row_text(h, row, "", "");
    }
}

fn do_restore_defaults(h: &Hwnds) {
    let defaults = Config::defaults();
    STAGED.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            st.shortcuts = defaults.shortcuts.clone();
        }
    });
    populate_listview(h, &defaults.shortcuts);
    unsafe {
        let _ = SetWindowTextW(
            h.sizes_edit,
            PCWSTR::from_raw(to_wide(&defaults.sizes.join(", ")).as_ptr()),
        );
    }
}

fn read_edit_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

fn parse_sizes(text: &str) -> std::result::Result<Vec<String>, &'static str> {
    let entries: Vec<String> = text
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if entries.is_empty() || entries.len() > 8 {
        return Err("Enter 1 to 8 fractions or percentages, separated by commas.");
    }
    for entry in &entries {
        if config::parse_size(entry).is_err() {
            return Err("Each size must be a fraction or percentage between 0 and 1.");
        }
    }
    Ok(entries)
}

fn do_save(h: &Hwnds) {
    let sizes_text = read_edit_text(h.sizes_edit);
    let sizes = match parse_sizes(&sizes_text) {
        Ok(sizes) => sizes,
        Err(message) => {
            show_message(h.main, message);
            return;
        }
    };

    let shortcuts = STAGED.with(|s| s.borrow().as_ref().map(|st| st.shortcuts.clone()));
    let Some(shortcuts) = shortcuts else { return };

    let new_config = Config { sizes, shortcuts };
    let failed = app::apply_new_config(new_config);

    if !failed.is_empty() {
        for (row, action) in Action::ALL.iter().enumerate() {
            if failed.contains(action) {
                set_item_text(h.listview, row as i32, 2, "In use by another app");
            }
        }
        return;
    }

    let checked = unsafe {
        SendMessageW(
            h.startup_check,
            windows::Win32::UI::WindowsAndMessaging::BM_GETCHECK,
            None,
            None,
        )
    };
    let wants_startup = checked.0 != 0;
    if wants_startup != startup::is_enabled() {
        let _ = startup::set_enabled(wants_startup);
    }

    close_window(h);
}

fn do_cancel(h: &Hwnds) {
    close_window(h);
}

fn show_message(owner: HWND, text: &str) {
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::MessageBoxW(
            Some(owner),
            PCWSTR::from_raw(to_wide(text).as_ptr()),
            w!("Wectangle Settings"),
            windows::Win32::UI::WindowsAndMessaging::MB_OK,
        );
    }
}

fn close_window(h: &Hwnds) {
    if STAGED.with(|s| {
        s.borrow()
            .as_ref()
            .map(|st| st.recording_row.is_some())
            .unwrap_or(false)
    }) {
        stop_recording(h);
    }
    unsafe {
        let _ = DestroyWindow(h.main);
    }
}

fn reposition_controls(h: &Hwnds, dpi: u32) {
    let r = compute_layout(dpi);
    unsafe {
        let _ = SetWindowPos(
            h.listview,
            None,
            r.listview.left,
            r.listview.top,
            r.listview.right - r.listview.left,
            r.listview.bottom - r.listview.top,
            windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
        );
        let moves: [(HWND, RECT); 8] = [
            (h.change_btn, r.change_btn),
            (h.clear_btn, r.clear_btn),
            (h.restore_btn, r.restore_btn),
            (h.sizes_edit, r.sizes_edit),
            (h.startup_check, r.startup_check),
            (h.save_btn, r.save_btn),
            (h.cancel_btn, r.cancel_btn),
            (h.sizes_edit, r.sizes_edit),
        ];
        for (ctl, rc) in moves {
            let _ = SetWindowPos(
                ctl,
                None,
                rc.left,
                rc.top,
                rc.right - rc.left,
                rc.bottom - rc.top,
                windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
            );
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_APP_RECORDER_KEY => {
            if let Some(h) = HWNDS.with(|c| c.get()) {
                on_recorder_key(&h, wparam.0 as u16);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let notify_code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if let Some(h) = HWNDS.with(|c| c.get()) {
                match id {
                    ID_CHANGE => do_change(&h),
                    ID_CLEAR => do_clear(&h),
                    ID_RESTORE => do_restore_defaults(&h),
                    ID_SAVE => {
                        let focus = unsafe { GetFocus() };
                        if focus == h.listview {
                            do_change(&h);
                        } else {
                            do_save(&h);
                        }
                    }
                    ID_CANCEL => do_cancel(&h),
                    _ => {}
                }
            }
            let _ = notify_code;
            LRESULT(0)
        }
        WM_NOTIFY => {
            let nmhdr = unsafe { &*(lparam.0 as *const NMHDR) };
            if nmhdr.code == NM_DBLCLK
                && let Some(h) = HWNDS.with(|c| c.get())
                && nmhdr.hwndFrom == h.listview
            {
                let activate = unsafe { &*(lparam.0 as *const NMITEMACTIVATE) };
                if activate.iItem >= 0 {
                    start_recording(&h, activate.iItem as usize);
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            if let Some(mut h) = HWNDS.with(|c| c.get()) {
                let dpi = (wparam.0 & 0xFFFF) as u32;
                let suggested = unsafe { &*(lparam.0 as *const RECT) };
                unsafe {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        suggested.left,
                        suggested.top,
                        suggested.right - suggested.left,
                        suggested.bottom - suggested.top,
                        windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
                    );
                }
                let old_font = h.font;
                h.font = make_font(dpi);
                HWNDS.with(|c| c.set(Some(h)));
                reposition_controls(&h, dpi);
                for ctl in [
                    h.listview,
                    h.change_btn,
                    h.clear_btn,
                    h.restore_btn,
                    h.sizes_edit,
                    h.startup_check,
                    h.save_btn,
                    h.cancel_btn,
                ] {
                    unsafe {
                        SendMessageW(
                            ctl,
                            WM_SETFONT,
                            Some(WPARAM(h.font.0 as usize)),
                            Some(LPARAM(1)),
                        );
                    }
                }
                unsafe {
                    let _ = DeleteObject(old_font.into());
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if let Some(h) = HWNDS.with(|c| c.get()) {
                do_cancel(&h);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            if let Some(h) = HWNDS.with(|c| c.take()) {
                if STAGED.with(|s| {
                    s.borrow()
                        .as_ref()
                        .map(|st| st.recording_row.is_some())
                        .unwrap_or(false)
                }) {
                    stop_recording(&h);
                }
                unsafe {
                    let _ = DeleteObject(h.font.into());
                }
            }
            STAGED.with(|s| *s.borrow_mut() = None);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
