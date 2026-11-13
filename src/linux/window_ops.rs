//! Moves and resizes the focused window on X11, through EWMH requests to the
//! window manager. The Linux counterpart of `src/window_ops.rs`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::randr::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt, EventMask, MapState, Window};

use super::x11;
use crate::config::Config;
use crate::layout::{self, Action, Rect};

const CYCLE_TOLERANCE: i32 = 2;
// The window manager applies a request after it returns, so a move is
// followed by polling the window until it stops changing.
const SETTLE_TIMEOUT: Duration = Duration::from_millis(300);
const SETTLE_POLL: Duration = Duration::from_millis(10);

// _NET_MOVERESIZE_WINDOW: StaticGravity (the position is the client
// window's own), x, y, width and height given, sent by a pager-like tool.
const MOVERESIZE_FLAGS: u32 = 10 | (0xF << 8) | (2 << 12);
const STATE_REMOVE: u32 = 0;
const STATE_ADD: u32 = 1;
const SOURCE_PAGER: u32 = 2;

pub enum ActionResult {
    Applied,
    NoOp,
    NoTarget,
    Failed,
}

enum MoveOutcome {
    Moved,
    NoOp,
    Failed,
}

pub fn apply_action(action: Action, config: &Config) -> ActionResult {
    if action == Action::DisplayMode {
        return if super::spawn("xfce4-display-settings", &["--minimal"]) {
            ActionResult::Applied
        } else {
            ActionResult::Failed
        };
    }
    let Some(window) = active_window() else {
        return ActionResult::NoTarget;
    };
    let outcome = match action {
        Action::Maximize => toggle_maximize(window),
        Action::Center => apply_center(window),
        Action::NextDisplay => move_display(window, config, true),
        Action::PreviousDisplay => move_display(window, config, false),
        _ => apply_directional(window, action, config),
    };
    match outcome {
        MoveOutcome::Moved => ActionResult::Applied,
        MoveOutcome::NoOp => ActionResult::NoOp,
        MoveOutcome::Failed => ActionResult::Failed,
    }
}

/// Screens plugged in, whether they show anything or not, as RandR counts
/// connected outputs. A laptop's own panel is one.
pub fn connected_display_count() -> usize {
    let shared = x11::shared();
    let Some(resources) = shared
        .conn
        .randr_get_screen_resources_current(shared.root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        return 1;
    };
    let connected = resources
        .outputs
        .iter()
        .filter_map(|&output| {
            shared
                .conn
                .randr_get_output_info(output, resources.config_timestamp)
                .ok()?
                .reply()
                .ok()
        })
        .filter(|info| info.connection == randr::Connection::CONNECTED)
        .count();
    connected.max(1)
}

fn active_window() -> Option<Window> {
    let shared = x11::shared();
    let window = *x11::values32(shared.root, shared.atoms._NET_ACTIVE_WINDOW).first()?;
    (window != 0 && window != shared.root && is_eligible(window)).then_some(window)
}

fn is_eligible(window: Window) -> bool {
    let shared = x11::shared();
    let atoms = &shared.atoms;
    let viewable = shared
        .conn
        .get_window_attributes(window)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|attributes| attributes.map_state == MapState::VIEWABLE);
    if !viewable {
        return false;
    }
    let types = x11::values32(window, atoms._NET_WM_WINDOW_TYPE);
    if types.contains(&atoms._NET_WM_WINDOW_TYPE_DESKTOP)
        || types.contains(&atoms._NET_WM_WINDOW_TYPE_DOCK)
    {
        return false;
    }
    let state = x11::values32(window, atoms._NET_WM_STATE);
    !state.contains(&atoms._NET_WM_STATE_HIDDEN) && !state.contains(&atoms._NET_WM_STATE_FULLSCREEN)
}

/// What the window manager adds around a client window (`frame`) and what
/// a client-side-decorated window draws inside it as an invisible shadow
/// (`gtk`), each as left, right, top, bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Extents {
    frame: [i32; 4],
    gtk: [i32; 4],
}

// The visible frame: the client window plus the decorations around it,
// minus a CSD window's invisible shadow. This is the rectangle that gets
// tiled, so decorations land inside the tile and shadows outside it.
fn visible_from(client: Rect, e: &Extents) -> Rect {
    let [fl, fr, ft, fb] = e.frame;
    let [gl, gr, gt, gb] = e.gtk;
    Rect::new(
        client.x - fl + gl,
        client.y - ft + gt,
        client.w + fl + fr - gl - gr,
        client.h + ft + fb - gt - gb,
    )
}

// The client rectangle that gives `target` as the visible frame.
fn client_for(target: Rect, e: &Extents) -> Rect {
    let [fl, fr, ft, fb] = e.frame;
    let [gl, gr, gt, gb] = e.gtk;
    Rect::new(
        target.x + fl - gl,
        target.y + ft - gt,
        target.w - fl - fr + gl + gr,
        target.h - ft - fb + gt + gb,
    )
}

struct Frame {
    visible: Rect,
    client: Rect,
    extents: Extents,
}

fn read_frame(window: Window) -> Option<Frame> {
    let shared = x11::shared();
    let geometry = shared.conn.get_geometry(window).ok()?.reply().ok()?;
    let origin = shared
        .conn
        .translate_coordinates(window, shared.root, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    let client = Rect::new(
        i32::from(origin.dst_x),
        i32::from(origin.dst_y),
        i32::from(geometry.width),
        i32::from(geometry.height),
    );
    let extents = Extents {
        frame: x11::extents(window, shared.atoms._NET_FRAME_EXTENTS),
        gtk: x11::extents(window, shared.atoms._GTK_FRAME_EXTENTS),
    };
    Some(Frame {
        visible: visible_from(client, &extents),
        client,
        extents,
    })
}

fn visible_frame(window: Window) -> Option<Rect> {
    read_frame(window).map(|frame| frame.visible)
}

fn send_to_root(window: Window, kind: u32, data: [u32; 5]) -> bool {
    let shared = x11::shared();
    let event = ClientMessageEvent::new(32, window, kind, data);
    shared
        .conn
        .send_event(
            false,
            shared.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )
        .is_ok()
        && shared.conn.flush().is_ok()
}

// Polls until the client window has moved away from `before` and then held
// still for two polls, or reached `expected`, or the timeout passed.
fn wait_settled(window: Window, before: Rect, expected: Rect) {
    let start = Instant::now();
    let mut last = before;
    let mut still = 0;
    while start.elapsed() < SETTLE_TIMEOUT {
        std::thread::sleep(SETTLE_POLL);
        let Some(frame) = read_frame(window) else {
            return;
        };
        if frame.client == expected {
            return;
        }
        if frame.client != last {
            last = frame.client;
            still = 0;
        } else if last != before {
            still += 1;
            if still >= 2 {
                return;
            }
        }
    }
}

fn move_to_frame(window: Window, target: Rect) -> bool {
    let Some(frame) = read_frame(window) else {
        return false;
    };
    if frame.visible == target {
        return true;
    }
    let client = client_for(target, &frame.extents);
    if client.w < 1 || client.h < 1 {
        return false;
    }
    let shared = x11::shared();
    let sent = send_to_root(
        window,
        shared.atoms._NET_MOVERESIZE_WINDOW,
        [
            MOVERESIZE_FLAGS,
            client.x as u32,
            client.y as u32,
            client.w as u32,
            client.h as u32,
        ],
    );
    if sent {
        wait_settled(window, frame.client, client);
    }
    sent
}

// Applies `target`, then re-reads the frame. A window that keeps another
// size (a terminal sizes itself in whole characters) gets the recompute
// callback (reanchor or recenter) for its actual size. Unlike on Windows
// there is no identical retry: X11 has no per-monitor DPI change that could
// resize the window on its own.
fn settle_frame(window: Window, target: Rect, recompute: impl Fn(i32, i32) -> Rect) -> bool {
    let mut ok = move_to_frame(window, target);
    let Some(verified) = visible_frame(window) else {
        return ok;
    };
    if verified.w != target.w || verified.h != target.h {
        let corrected = recompute(verified.w, verified.h);
        ok = move_to_frame(window, corrected) && ok;
    }
    ok
}

fn is_maximized(window: Window) -> bool {
    let atoms = &x11::shared().atoms;
    let state = x11::values32(window, atoms._NET_WM_STATE);
    state.contains(&atoms._NET_WM_STATE_MAXIMIZED_VERT)
        || state.contains(&atoms._NET_WM_STATE_MAXIMIZED_HORZ)
}

fn set_maximized(window: Window, on: bool) {
    let atoms = &x11::shared().atoms;
    let before = read_frame(window).map(|frame| frame.client);
    let sent = send_to_root(
        window,
        atoms._NET_WM_STATE,
        [
            if on { STATE_ADD } else { STATE_REMOVE },
            atoms._NET_WM_STATE_MAXIMIZED_VERT,
            atoms._NET_WM_STATE_MAXIMIZED_HORZ,
            SOURCE_PAGER,
            0,
        ],
    );
    if !sent {
        return;
    }
    let start = Instant::now();
    while is_maximized(window) != on && start.elapsed() < SETTLE_TIMEOUT {
        std::thread::sleep(SETTLE_POLL);
    }
    if let Some(before) = before {
        wait_settled(window, before, before);
    }
}

fn restore_if_maximized(window: Window) -> bool {
    let maximized = is_maximized(window);
    if maximized {
        set_maximized(window, false);
    }
    maximized
}

pub struct Monitor {
    pub bounds: Rect,
    pub work: Rect,
}

pub fn monitors() -> Vec<Monitor> {
    let shared = x11::shared();
    let screen = shared
        .conn
        .get_geometry(shared.root)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|g| Rect::new(0, 0, i32::from(g.width), i32::from(g.height)))
        .unwrap_or_else(|| Rect::new(0, 0, 1, 1));
    let mut bounds: Vec<Rect> = shared
        .conn
        .randr_get_monitors(shared.root, true)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .map(|reply| {
            reply
                .monitors
                .iter()
                .map(|m| {
                    Rect::new(
                        i32::from(m.x),
                        i32::from(m.y),
                        i32::from(m.width),
                        i32::from(m.height),
                    )
                })
                .filter(|r| r.w > 0 && r.h > 0)
                .collect()
        })
        .unwrap_or_default();
    if bounds.is_empty() {
        bounds.push(screen);
    }
    bounds.sort_by_key(|r| (r.x, r.y));

    let struts = panel_struts();
    bounds
        .into_iter()
        .map(|bounds| Monitor {
            bounds,
            work: layout::work_area(bounds, screen.w, screen.h, &struts),
        })
        .collect()
}

fn panel_struts() -> Vec<layout::Strut> {
    let shared = x11::shared();
    let atoms = &shared.atoms;
    x11::values32(shared.root, atoms._NET_CLIENT_LIST)
        .into_iter()
        .flat_map(|window| {
            let partial = x11::values32(window, atoms._NET_WM_STRUT_PARTIAL);
            let values = if partial.is_empty() {
                x11::values32(window, atoms._NET_WM_STRUT)
            } else {
                partial
            };
            layout::struts_from(&values)
        })
        .collect()
}

// The monitor holding the frame's center, else the one it overlaps most.
fn monitor_index(monitors: &[Monitor], frame: Rect) -> usize {
    let (cx, cy) = frame.center();
    if let Some(index) = monitors.iter().position(|m| {
        cx >= m.bounds.x && cx < m.bounds.right() && cy >= m.bounds.y && cy < m.bounds.bottom()
    }) {
        return index;
    }
    let overlap = |m: &Monitor| {
        let w = frame.right().min(m.bounds.right()) - frame.x.max(m.bounds.x);
        let h = frame.bottom().min(m.bounds.bottom()) - frame.y.max(m.bounds.y);
        i64::from(w.max(0)) * i64::from(h.max(0))
    };
    monitors
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| overlap(m))
        .map_or(0, |(index, _)| index)
}

fn work_area_of(window: Window) -> Option<Rect> {
    let frame = visible_frame(window)?;
    let monitors = monitors();
    monitors
        .get(monitor_index(&monitors, frame))
        .map(|m| m.work)
}

fn apply_directional(window: Window, action: Action, config: &Config) -> MoveOutcome {
    let sizes = config.size_fractions();
    if sizes.is_empty() {
        return MoveOutcome::Failed;
    }
    let Some(current_frame) = visible_frame(window) else {
        return MoveOutcome::Failed;
    };
    let Some(work) = work_area_of(window) else {
        return MoveOutcome::Failed;
    };

    let index = layout::next_cycle_index(
        cycle_state::get(window),
        action,
        current_frame,
        sizes.len(),
        CYCLE_TOLERANCE,
    );
    let target = layout::compute(action, sizes[index], work);

    restore_if_maximized(window);
    let ok = settle_frame(window, target, |w, h| layout::reanchor(action, w, h, work));

    if let Some(final_frame) = visible_frame(window) {
        cycle_state::set(window, action, index, final_frame);
    }
    if ok {
        MoveOutcome::Moved
    } else {
        MoveOutcome::Failed
    }
}

fn apply_center(window: Window) -> MoveOutcome {
    let Some(work) = work_area_of(window) else {
        return MoveOutcome::Failed;
    };
    let Some(current_frame) = visible_frame(window) else {
        return MoveOutcome::Failed;
    };
    let target = layout::center(current_frame.w, current_frame.h, work);
    let ok = settle_frame(window, target, |w, h| layout::center(w, h, work));
    if let Some(final_frame) = visible_frame(window) {
        cycle_state::set(window, Action::Center, 0, final_frame);
    }
    if ok {
        MoveOutcome::Moved
    } else {
        MoveOutcome::Failed
    }
}

fn toggle_maximize(window: Window) -> MoveOutcome {
    let maximized = is_maximized(window);
    set_maximized(window, !maximized);
    MoveOutcome::Moved
}

fn move_display(window: Window, config: &Config, forward: bool) -> MoveOutcome {
    let monitors = monitors();
    if monitors.len() < 2 {
        return MoveOutcome::NoOp;
    }
    let was_maximized = restore_if_maximized(window);
    let Some(current_frame) = visible_frame(window) else {
        return MoveOutcome::Failed;
    };
    let current_index = monitor_index(&monitors, current_frame);
    let target_index = layout::wrap_monitor_index(current_index, monitors.len(), forward);
    if target_index == current_index {
        return MoveOutcome::NoOp;
    }
    let source_work = monitors[current_index].work;
    let target_work = monitors[target_index].work;

    let remembered: Option<(Action, usize)> = match cycle_state::get(window) {
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

    // X11 has one scale for every screen.
    let target = match remembered {
        Some((Action::Center, _)) => layout::center(current_frame.w, current_frame.h, target_work),
        Some((action, index)) => {
            let fraction = config.size_fractions().get(index).copied().unwrap_or(0.5);
            layout::compute(action, fraction, target_work)
        }
        None => layout::scale_to_monitor(current_frame, source_work, target_work, 1.0),
    };

    let ok = settle_frame(window, target, |w, h| match remembered {
        Some((Action::Center, _)) | None => layout::center(w, h, target_work),
        Some((action, _)) => layout::reanchor(action, w, h, target_work),
    });

    if was_maximized {
        set_maximized(window, true);
    }

    match (remembered, visible_frame(window)) {
        (Some((action, index)), Some(final_frame)) => {
            cycle_state::set(window, action, index, final_frame)
        }
        _ => cycle_state::clear(window),
    }

    if ok {
        MoveOutcome::Moved
    } else {
        MoveOutcome::Failed
    }
}

// Per-window remembered layout, read and written only from the main
// thread. Entries are pruned on every insert so closed windows do not
// accumulate.
mod cycle_state {
    use super::*;

    thread_local! {
        static STATE: RefCell<HashMap<Window, (Action, usize, Rect)>> = RefCell::new(HashMap::new());
    }

    fn exists(window: Window) -> bool {
        x11::shared()
            .conn
            .get_window_attributes(window)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some()
    }

    pub fn get(window: Window) -> Option<(Action, usize, Rect)> {
        STATE.with(|s| s.borrow().get(&window).copied())
    }

    pub fn set(window: Window, action: Action, index: usize, frame: Rect) {
        let known: Vec<Window> = STATE.with(|s| s.borrow().keys().copied().collect());
        let gone: Vec<Window> = known
            .into_iter()
            .filter(|&w| w != window && !exists(w))
            .collect();
        STATE.with(|s| {
            let mut map = s.borrow_mut();
            for w in gone {
                map.remove(&w);
            }
            map.insert(window, (action, index, frame));
        });
    }

    pub fn clear(window: Window) {
        STATE.with(|s| {
            s.borrow_mut().remove(&window);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorations_are_inside_the_visible_frame() {
        // xfwm4 frames Thunar with a 24 px title bar and 3 px borders.
        let e = Extents {
            frame: [3, 3, 24, 3],
            gtk: [0; 4],
        };
        let client = Rect::new(103, 124, 600, 400);
        assert_eq!(visible_from(client, &e), Rect::new(100, 100, 606, 427));
        assert_eq!(client_for(Rect::new(100, 100, 606, 427), &e), client);
    }

    #[test]
    fn a_csd_shadow_is_outside_the_visible_frame() {
        // A GTK headerbar window draws a 26 px shadow (and 23 px on top).
        let e = Extents {
            frame: [0; 4],
            gtk: [26, 26, 23, 29],
        };
        let client = Rect::new(0, 0, 852, 652);
        assert_eq!(visible_from(client, &e), Rect::new(26, 23, 800, 600));
        assert_eq!(
            client_for(Rect::new(0, 0, 683, 735), &e),
            Rect::new(-26, -23, 735, 787)
        );
    }

    #[test]
    fn a_window_belongs_to_the_monitor_holding_its_center() {
        let monitors = [
            Monitor {
                bounds: Rect::new(0, 0, 1920, 1080),
                work: Rect::new(0, 0, 1920, 1050),
            },
            Monitor {
                bounds: Rect::new(1920, 0, 1366, 768),
                work: Rect::new(1920, 0, 1366, 768),
            },
        ];
        assert_eq!(monitor_index(&monitors, Rect::new(1800, 100, 400, 300)), 1);
        assert_eq!(monitor_index(&monitors, Rect::new(100, 100, 400, 300)), 0);
        // Center off every screen: the one it overlaps most.
        assert_eq!(monitor_index(&monitors, Rect::new(1900, 700, 200, 800)), 1);
    }
}
