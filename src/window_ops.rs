use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromWindow,
};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GA_ROOT, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowRect,
    GetWindowThreadProcessId, IsHungAppWindow, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
    SW_MAXIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos, ShowWindow,
};
use windows::core::BOOL;

use crate::config::Config;
use crate::layout::{self, Action, Rect};

const CYCLE_TOLERANCE: i32 = 2;

pub enum ActionResult {
    Applied,
    NoOp,
    NoTarget,
    AdminBlocked,
    Failed,
}

enum MoveOutcome {
    Moved,
    NoOp,
    Failed,
}

pub fn apply_action(action: Action, config: &Config) -> ActionResult {
    let Some(hwnd) = foreground_target() else {
        return ActionResult::NoTarget;
    };

    let elevated = is_elevated(hwnd);
    if elevated == Some(true) && !is_self_elevated() {
        return ActionResult::AdminBlocked;
    }

    let outcome = match action {
        Action::Maximize => toggle_maximize(hwnd),
        Action::Center => apply_center(hwnd),
        Action::NextDisplay => move_display(hwnd, config, true),
        Action::PreviousDisplay => move_display(hwnd, config, false),
        _ => apply_directional(hwnd, action, config),
    };

    match outcome {
        MoveOutcome::Moved => ActionResult::Applied,
        MoveOutcome::NoOp => ActionResult::NoOp,
        MoveOutcome::Failed if elevated.is_none() => ActionResult::AdminBlocked,
        MoveOutcome::Failed => ActionResult::Failed,
    }
}

fn foreground_target() -> Option<HWND> {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.is_invalid() {
            return None;
        }
        let root = GetAncestor(fg, GA_ROOT);
        let hwnd = if root.is_invalid() { fg } else { root };
        is_eligible(hwnd).then_some(hwnd)
    }
}

fn is_eligible(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        if IsIconic(hwnd).as_bool() {
            return false;
        }
        if IsHungAppWindow(hwnd).as_bool() {
            return false;
        }

        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut c_void,
            std::mem::size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return false;
        }

        let class = class_name(hwnd);
        !matches!(
            class.as_str(),
            "Progman"
                | "WorkerW"
                | "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "Windows.UI.Core.CoreWindow"
        )
    }
}

fn class_name(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }
}

struct Insets {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

// The visible DWM frame and how much larger the raw window rect is on each
// edge; SetWindowPos operates on the raw rect, so the insets must be added
// back to land the visible frame on the intended target.
fn get_frame_and_insets(hwnd: HWND) -> Option<(Rect, Insets)> {
    unsafe {
        let mut raw = RECT::default();
        GetWindowRect(hwnd, &mut raw).ok()?;

        let mut frame = RECT::default();
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut _ as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;

        let insets = Insets {
            left: raw.left - frame.left,
            top: raw.top - frame.top,
            right: raw.right - frame.right,
            bottom: raw.bottom - frame.bottom,
        };
        let visible = Rect::new(
            frame.left,
            frame.top,
            frame.right - frame.left,
            frame.bottom - frame.top,
        );
        Some((visible, insets))
    }
}

fn apply_frame(hwnd: HWND, target: Rect, insets: &Insets) -> bool {
    let x = target.x + insets.left;
    let y = target.y + insets.top;
    let cx = target.w + insets.right - insets.left;
    let cy = target.h + insets.bottom - insets.top;
    unsafe { SetWindowPos(hwnd, None, x, y, cx, cy, SWP_NOZORDER | SWP_NOACTIVATE).is_ok() }
}

fn move_to_frame(hwnd: HWND, target: Rect) -> bool {
    match get_frame_and_insets(hwnd) {
        Some((_, insets)) => apply_frame(hwnd, target, &insets),
        None => false,
    }
}

// Applies target, then re-reads the frame. A mismatch there is retried with
// the same target and fresh insets once, since a cross-monitor move can
// trigger WM_DPICHANGED and resize the window on its own. Only a mismatch
// that survives the retry is treated as the app enforcing its own size, and
// gets the recompute callback (reanchor or recenter) instead of another
// identical attempt.
fn settle_frame(hwnd: HWND, target: Rect, recompute: impl Fn(i32, i32) -> Rect) -> bool {
    let mut ok = move_to_frame(hwnd, target);
    let mut verified = match get_frame_and_insets(hwnd) {
        Some((v, _)) => v,
        None => return ok,
    };

    if verified != target {
        ok = move_to_frame(hwnd, target) && ok;
        verified = match get_frame_and_insets(hwnd) {
            Some((v, _)) => v,
            None => return ok,
        };
    }

    if verified.w != target.w || verified.h != target.h {
        let corrected = recompute(verified.w, verified.h);
        ok = move_to_frame(hwnd, corrected) && ok;
    }
    ok
}

fn apply_directional(hwnd: HWND, action: Action, config: &Config) -> MoveOutcome {
    let Some(monitor) = find_monitor(hwnd) else {
        return MoveOutcome::Failed;
    };
    let work = monitor.work;
    let sizes = config.size_fractions();
    if sizes.is_empty() {
        return MoveOutcome::Failed;
    }
    let Some((current_frame, _)) = get_frame_and_insets(hwnd) else {
        return MoveOutcome::Failed;
    };

    let index = layout::next_cycle_index(
        cycle_state::get(hwnd),
        action,
        current_frame,
        sizes.len(),
        CYCLE_TOLERANCE,
    );
    let target = layout::compute(action, sizes[index], work);

    if unsafe { IsZoomed(hwnd).as_bool() } {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
    }

    let ok = settle_frame(hwnd, target, |w, h| layout::reanchor(action, w, h, work));

    if let Some((final_frame, _)) = get_frame_and_insets(hwnd) {
        cycle_state::set(hwnd, action, index, final_frame);
    }
    if ok {
        MoveOutcome::Moved
    } else {
        MoveOutcome::Failed
    }
}

fn apply_center(hwnd: HWND) -> MoveOutcome {
    let Some(monitor) = find_monitor(hwnd) else {
        return MoveOutcome::Failed;
    };
    let work = monitor.work;
    let Some((current_frame, _)) = get_frame_and_insets(hwnd) else {
        return MoveOutcome::Failed;
    };

    let target = layout::center(current_frame.w, current_frame.h, work);
    let ok = settle_frame(hwnd, target, |w, h| layout::center(w, h, work));

    if let Some((final_frame, _)) = get_frame_and_insets(hwnd) {
        cycle_state::set(hwnd, Action::Center, 0, final_frame);
    }
    if ok {
        MoveOutcome::Moved
    } else {
        MoveOutcome::Failed
    }
}

fn toggle_maximize(hwnd: HWND) -> MoveOutcome {
    unsafe {
        if IsZoomed(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }
    MoveOutcome::Moved
}

fn move_display(hwnd: HWND, config: &Config, forward: bool) -> MoveOutcome {
    let monitors = enumerate_monitors();
    if monitors.len() < 2 {
        return MoveOutcome::NoOp;
    }
    let current_handle = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    let Some(current_index) = monitors.iter().position(|m| m.handle == current_handle) else {
        return MoveOutcome::NoOp;
    };
    let target_index = layout::wrap_monitor_index(current_index, monitors.len(), forward);
    if target_index == current_index {
        return MoveOutcome::NoOp;
    }

    let source_work = monitors[current_index].work;
    let target_work = monitors[target_index].work;
    let dpi_ratio = monitors[target_index].dpi as f64 / monitors[current_index].dpi.max(1) as f64;

    let was_maximized = unsafe { IsZoomed(hwnd).as_bool() };
    if was_maximized {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
    }

    let Some((current_frame, _)) = get_frame_and_insets(hwnd) else {
        return MoveOutcome::Failed;
    };

    let remembered: Option<(Action, usize)> = match cycle_state::get(hwnd) {
        Some((action, index, frame)) if frame.close_to(&current_frame, CYCLE_TOLERANCE) => {
            Some((action, index))
        }
        _ if layout::center(current_frame.w, current_frame.h, source_work)
            .close_to(&current_frame, CYCLE_TOLERANCE) =>
        {
            Some((Action::Center, 0))
        }
        _ => None,
    };

    let target = match remembered {
        Some((Action::Center, _)) => layout::center(current_frame.w, current_frame.h, target_work),
        Some((action, index)) => {
            let fraction = config.size_fractions().get(index).copied().unwrap_or(0.5);
            layout::compute(action, fraction, target_work)
        }
        None => layout::scale_to_monitor(current_frame, source_work, target_work, dpi_ratio),
    };

    let ok = settle_frame(hwnd, target, |w, h| match remembered {
        Some((Action::Center, _)) | None => layout::center(w, h, target_work),
        Some((action, _)) => layout::reanchor(action, w, h, target_work),
    });

    if was_maximized {
        unsafe {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }

    match (remembered, get_frame_and_insets(hwnd)) {
        (Some((action, index)), Some((final_frame, _))) => {
            cycle_state::set(hwnd, action, index, final_frame)
        }
        _ => cycle_state::clear(hwnd),
    }

    if ok {
        MoveOutcome::Moved
    } else {
        MoveOutcome::Failed
    }
}

pub struct MonitorInfo {
    pub handle: HMONITOR,
    pub bounds: Rect,
    pub work: Rect,
    pub dpi: u32,
}

extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    unsafe {
        let monitors = &mut *(lparam.0 as *mut Vec<HMONITOR>);
        monitors.push(hmonitor);
    }
    BOOL(1)
}

pub fn enumerate_monitors() -> Vec<MonitorInfo> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut handles as *mut _ as isize),
        );
    }

    let mut infos: Vec<MonitorInfo> = handles
        .into_iter()
        .filter_map(|handle| {
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            if !unsafe { GetMonitorInfoW(handle, &mut mi) }.as_bool() {
                return None;
            }
            let mut dpi_x = 0u32;
            let mut dpi_y = 0u32;
            unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y).ok()? };
            Some(MonitorInfo {
                handle,
                bounds: Rect::new(
                    mi.rcMonitor.left,
                    mi.rcMonitor.top,
                    mi.rcMonitor.right - mi.rcMonitor.left,
                    mi.rcMonitor.bottom - mi.rcMonitor.top,
                ),
                work: Rect::new(
                    mi.rcWork.left,
                    mi.rcWork.top,
                    mi.rcWork.right - mi.rcWork.left,
                    mi.rcWork.bottom - mi.rcWork.top,
                ),
                dpi: dpi_x,
            })
        })
        .collect();

    infos.sort_by_key(|m| (m.bounds.x, m.bounds.y));
    infos
}

fn find_monitor(hwnd: HWND) -> Option<MonitorInfo> {
    let handle = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    enumerate_monitors()
        .into_iter()
        .find(|m| m.handle == handle)
}

// PROCESS_QUERY_LIMITED_INFORMATION can read another process's token even
// across an elevation boundary, which is why it is used here rather than
// PROCESS_QUERY_INFORMATION. Returns None when the query itself is denied,
// so the caller can fall back to attempting the move.
fn is_elevated(hwnd: HWND) -> Option<bool> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut token = HANDLE::default();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        let _ = CloseHandle(process);
        opened.ok()?;

        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let got = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        got.ok()?;

        Some(elevation.TokenIsElevated != 0)
    }
}

fn is_self_elevated() -> bool {
    unsafe {
        let process = GetCurrentProcess();
        let mut token = HANDLE::default();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let got = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        );
        let _ = CloseHandle(token);
        got.is_ok() && elevation.TokenIsElevated != 0
    }
}

// Per-window remembered layout, read and written only from the UI thread.
// Entries are pruned on every insert so dead HWNDs do not accumulate.
mod cycle_state {
    use super::*;

    // HWND wraps a raw pointer and does not implement Hash, so the window
    // handle's pointer value is used as the map key instead.
    fn key(hwnd: HWND) -> isize {
        hwnd.0 as isize
    }

    thread_local! {
        static STATE: RefCell<HashMap<isize, (Action, usize, Rect)>> = RefCell::new(HashMap::new());
    }

    pub fn get(hwnd: HWND) -> Option<(Action, usize, Rect)> {
        STATE.with(|s| s.borrow().get(&key(hwnd)).copied())
    }

    pub fn set(hwnd: HWND, action: Action, index: usize, frame: Rect) {
        STATE.with(|s| {
            let mut map = s.borrow_mut();
            map.retain(|&raw, _| unsafe { IsWindow(Some(HWND(raw as *mut c_void))).as_bool() });
            map.insert(key(hwnd), (action, index, frame));
        });
    }

    pub fn clear(hwnd: HWND) {
        STATE.with(|s| {
            s.borrow_mut().remove(&key(hwnd));
        });
    }
}
