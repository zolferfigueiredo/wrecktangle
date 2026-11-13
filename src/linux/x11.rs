//! The X connection the Linux build shares between its threads, and the
//! messages other threads post to the main loop. Only the main thread reads
//! events or touches app state; the tray, notification, update and timer
//! threads send a ClientMessage to the hidden main window instead, the way
//! the Windows build posts window messages.

use std::sync::OnceLock;

use x11rb::connection::Connection;
use x11rb::errors::ReplyOrIdError;
use x11rb::protocol::xproto::{AtomEnum, ClientMessageEvent, ConnectionExt, EventMask, Window};
use x11rb::rust_connection::RustConnection;

x11rb::atom_manager! {
    pub Atoms: AtomsCookie {
        _GTK_FRAME_EXTENTS,
        _NET_ACTIVE_WINDOW,
        _NET_CLIENT_LIST,
        _NET_FRAME_EXTENTS,
        _NET_MOVERESIZE_WINDOW,
        _NET_WM_STATE,
        _NET_WM_STATE_FULLSCREEN,
        _NET_WM_STATE_HIDDEN,
        _NET_WM_STATE_MAXIMIZED_HORZ,
        _NET_WM_STATE_MAXIMIZED_VERT,
        _NET_WM_STRUT,
        _NET_WM_STRUT_PARTIAL,
        _NET_WM_WINDOW_TYPE,
        _NET_WM_WINDOW_TYPE_DESKTOP,
        _NET_WM_WINDOW_TYPE_DOCK,
        _NET_WM_WINDOW_TYPE_NORMAL,
        _NET_WM_ICON,
        _NET_WM_NAME,
        UTF8_STRING,
        WM_DELETE_WINDOW,
        WM_PROTOCOLS,
        WRECKTANGLE_INSTANCE,
        WRECKTANGLE_MESSAGE,
    }
}

pub struct Shared {
    pub conn: RustConnection,
    pub atoms: Atoms,
    pub screen: usize,
    pub root: Window,
    /// The hidden window that owns the single-instance selection and
    /// receives posted messages.
    pub window: Window,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

pub fn init(shared: Shared) {
    if SHARED.set(shared).is_err() {
        panic!("the X connection is set up once");
    }
}

pub fn shared() -> &'static Shared {
    SHARED.get().expect("the X connection is set up first")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    OpenSettings,
    Quit,
    CheckUpdates,
    ToggleStartup,
    UpdateDone { auto: bool, checked_at: u64 },
    UpdateClicked,
    UpdateTimer { startup: bool },
}

impl Message {
    pub fn encode(self) -> [u32; 5] {
        match self {
            Message::OpenSettings => [1, 0, 0, 0, 0],
            Message::Quit => [2, 0, 0, 0, 0],
            Message::CheckUpdates => [3, 0, 0, 0, 0],
            Message::ToggleStartup => [4, 0, 0, 0, 0],
            Message::UpdateDone { auto, checked_at } => [
                5,
                u32::from(auto),
                (checked_at >> 32) as u32,
                checked_at as u32,
                0,
            ],
            Message::UpdateClicked => [6, 0, 0, 0, 0],
            Message::UpdateTimer { startup } => [7, u32::from(startup), 0, 0, 0],
        }
    }

    pub fn decode(data: [u32; 5]) -> Option<Message> {
        Some(match data[0] {
            1 => Message::OpenSettings,
            2 => Message::Quit,
            3 => Message::CheckUpdates,
            4 => Message::ToggleStartup,
            5 => Message::UpdateDone {
                auto: data[1] != 0,
                checked_at: (u64::from(data[2]) << 32) | u64::from(data[3]),
            },
            6 => Message::UpdateClicked,
            7 => Message::UpdateTimer {
                startup: data[1] != 0,
            },
            _ => return None,
        })
    }
}

/// Sends `message` to `window`, the main window of this or another
/// running copy.
pub fn send_to(
    conn: &RustConnection,
    window: Window,
    kind: u32,
    message: Message,
) -> Result<(), ReplyOrIdError> {
    let event = ClientMessageEvent::new(32, window, kind, message.encode());
    conn.send_event(false, window, EventMask::NO_EVENT, event)?;
    conn.flush()?;
    Ok(())
}

/// Posts `message` to the main loop. Safe from any thread.
pub fn post(message: Message) {
    let Some(shared) = SHARED.get() else {
        return;
    };
    let _ = send_to(
        &shared.conn,
        shared.window,
        shared.atoms.WRECKTANGLE_MESSAGE,
        message,
    );
}

/// A window property of 32-bit values (CARDINAL, ATOM or WINDOW); empty when
/// it is missing.
pub fn values32(window: Window, property: u32) -> Vec<u32> {
    let conn = &shared().conn;
    conn.get_property(false, window, property, AtomEnum::ANY, 0, 4096)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().map(Iterator::collect))
        .unwrap_or_default()
}

/// A four-value property such as `_NET_FRAME_EXTENTS`, as left, right, top,
/// bottom; zeros when it is missing or malformed.
pub fn extents(window: Window, property: u32) -> [i32; 4] {
    let values = values32(window, property);
    if values.len() < 4 {
        return [0; 4];
    }
    std::array::from_fn(|i| i32::try_from(values[i]).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_survive_encoding() {
        for message in [
            Message::OpenSettings,
            Message::Quit,
            Message::CheckUpdates,
            Message::ToggleStartup,
            Message::UpdateDone {
                auto: true,
                checked_at: 0x1_2345_6789,
            },
            Message::UpdateDone {
                auto: false,
                checked_at: 0,
            },
            Message::UpdateClicked,
            Message::UpdateTimer { startup: true },
            Message::UpdateTimer { startup: false },
        ] {
            assert_eq!(Message::decode(message.encode()), Some(message));
        }
        assert_eq!(Message::decode([99, 0, 0, 0, 0]), None);
    }
}
