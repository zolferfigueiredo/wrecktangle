//! The Linux build: an X11 client for Xfce and other EWMH window managers.
//! Wayland is not supported, since it doesn't let one app move another
//! app's windows.

pub mod app;
pub mod desktop;
pub mod hotkeys;
pub mod notify;
pub mod settings;
pub mod tray;
pub mod window_ops;
pub mod x11;
pub mod xfconf;

use std::process::{Command, Stdio};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, WindowClass};
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

use self::x11::{Atoms, Message, Shared};

pub fn run() {
    // The uninstall script's last step: put back the Xfce shortcuts a copy
    // that crashed or was killed left removed. A copy still running holds
    // those keys, so it is asked to quit instead, which puts them back once
    // it has let go of them.
    if std::env::args().any(|arg| arg == "--restore-xfce-shortcuts") {
        let running = x11rb::connect(None).ok().and_then(|(conn, _)| {
            let atoms = Atoms::new(&conn).ok()?.reply().ok()?;
            let owner = instance_owner(&conn, &atoms)?;
            tell(&conn, owner, &atoms, Message::Quit);
            Some(())
        });
        if running.is_none() {
            xfconf::restore_all();
        }
        return;
    }
    let quit = std::env::args().any(|arg| arg == "--quit");
    let wayland =
        std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t.eq_ignore_ascii_case("wayland"));
    if wayland || std::env::var_os("DISPLAY").is_none() {
        eprintln!("Wrecktangle needs an X11 session; Wayland is not supported.");
        std::process::exit(1);
    }
    let Ok((conn, screen_num)) = x11rb::connect(None) else {
        eprintln!("Wrecktangle could not connect to the X server.");
        std::process::exit(1);
    };
    let root = conn.setup().roots[screen_num].root;
    let Some(atoms) = Atoms::new(&conn)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
    else {
        std::process::exit(1);
    };

    // A second start only tells the running copy to open Settings (or to
    // quit, with --quit).
    let message = if quit {
        Message::Quit
    } else {
        Message::OpenSettings
    };
    if let Some(owner) = instance_owner(&conn, &atoms) {
        tell(&conn, owner, &atoms, message);
        return;
    }
    if quit {
        return;
    }

    let Ok(window) = conn.generate_id() else {
        std::process::exit(1);
    };
    let created = conn
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            -1,
            -1,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            COPY_FROM_PARENT,
            &CreateWindowAux::new(),
        )
        .map(|cookie| cookie.check());
    if !matches!(created, Ok(Ok(()))) {
        std::process::exit(1);
    }
    let _ = conn.set_selection_owner(window, atoms.WRECKTANGLE_INSTANCE, CURRENT_TIME);
    // Two copies started at once: the one that lost hands over and leaves.
    match instance_owner(&conn, &atoms) {
        Some(owner) if owner == window => {}
        Some(owner) => {
            tell(&conn, owner, &atoms, message);
            return;
        }
        None => std::process::exit(1),
    }

    x11::init(Shared {
        conn,
        atoms,
        screen: screen_num,
        root,
        window,
    });
    watch_signals();
    app::init();

    // An error means the X server went away, at the end of the session.
    let conn = &x11::shared().conn;
    while let Ok(event) = conn.wait_for_event() {
        if !app::handle_event(event) {
            break;
        }
    }
    app::shutdown();
}

// The X server can drop requests a client sent just before it disconnects,
// so a round trip makes sure the running copy got the message before this
// one exits.
fn tell(
    conn: &x11rb::rust_connection::RustConnection,
    owner: u32,
    atoms: &Atoms,
    message: Message,
) {
    if x11::send_to(conn, owner, atoms.WRECKTANGLE_MESSAGE, message).is_ok() {
        let _ = conn.get_input_focus().map(|cookie| cookie.reply());
    }
}

fn instance_owner(conn: &impl Connection, atoms: &Atoms) -> Option<u32> {
    let owner = conn
        .get_selection_owner(atoms.WRECKTANGLE_INSTANCE)
        .ok()?
        .reply()
        .ok()?
        .owner;
    (owner != NONE).then_some(owner)
}

// Quitting puts Xfce's shortcuts back, so a logout or `kill` quits the same
// way the tray's Quit does.
fn watch_signals() {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    let Ok(mut signals) = Signals::new([SIGTERM, SIGINT, SIGHUP]) else {
        return;
    };
    let _ = std::thread::Builder::new()
        .name("signals".to_string())
        .spawn(move || {
            for _ in signals.forever() {
                x11::post(Message::Quit);
            }
        });
}

/// Starts a program without waiting for it; a thread reaps it when it exits.
pub fn spawn(program: &str, args: &[&str]) -> bool {
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => {
            let _ = std::thread::Builder::new()
                .name("child".to_string())
                .spawn(move || child.wait());
            true
        }
        Err(_) => false,
    }
}
