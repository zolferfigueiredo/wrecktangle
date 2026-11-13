//! Desktop notifications through `org.freedesktop.Notifications` on the
//! session bus (xfce4-notifyd on Xfce), the Linux stand-in for tray
//! balloons.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::Value;

use super::x11::{self, Message};
use crate::lang;

const DESTINATION: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";

static CONNECTION: OnceLock<Option<Connection>> = OnceLock::new();
// Clicking a notification reports its id, so only a click on the update
// notification opens the release page.
static UPDATE_ID: AtomicU32 = AtomicU32::new(0);

fn connection() -> Option<&'static Connection> {
    CONNECTION
        .get_or_init(|| {
            let connection = Connection::session().ok()?;
            listen_for_clicks(connection.clone());
            Some(connection)
        })
        .as_ref()
}

fn listen_for_clicks(connection: Connection) {
    let _ = std::thread::Builder::new()
        .name("notifications".to_string())
        .spawn(move || {
            let Ok(proxy) = Proxy::new(&connection, DESTINATION, PATH, INTERFACE) else {
                return;
            };
            let Ok(signals) = proxy.receive_signal("ActionInvoked") else {
                return;
            };
            for message in signals {
                if let Ok((id, _action)) = message.body().deserialize::<(u32, String)>()
                    && id != 0
                    && id == UPDATE_ID.load(Ordering::Relaxed)
                {
                    x11::post(Message::UpdateClicked);
                }
            }
        });
}

fn send(body: &str, actions: &[&str]) -> Option<u32> {
    let connection = connection()?;
    let hints: HashMap<&str, Value> =
        HashMap::from([("desktop-entry", Value::from("wrecktangle"))]);
    let reply = connection
        .call_method(
            Some(DESTINATION),
            PATH,
            Some(INTERFACE),
            "Notify",
            &(
                "Wrecktangle",
                0u32,
                "wrecktangle",
                "Wrecktangle",
                body,
                actions,
                hints,
                -1i32,
            ),
        )
        .ok()?;
    reply.body().deserialize::<u32>().ok()
}

pub fn notify(message: &str) {
    let _ = send(message, &[]);
}

/// A notification that opens the new release when clicked.
pub fn notify_update(message: &str) {
    let label = lang::t("button.download");
    if let Some(id) = send(message, &["default", &label]) {
        UPDATE_ID.store(id, Ordering::Relaxed);
    }
}
