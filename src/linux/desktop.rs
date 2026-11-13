//! The desktop's light or dark preference and accent color, which the
//! System theme and the accent follow. They come from the xdg-desktop-portal
//! Settings interface (`org.freedesktop.appearance`) and, where it gives
//! none, from the GTK theme named in XSETTINGS (`Net/ThemeName`). Two
//! threads watch both for changes and tell the tray and Settings.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use x11rb::NONE;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt, EventMask, Window,
};
use x11rb::rust_connection::RustConnection;
use zbus::blocking::{Connection as Bus, Proxy};
use zbus::zvariant::Value;

use super::{settings, tray, x11};
use crate::ui::palette::{Accent, Rgba};

const APPEARANCE: &str = "org.freedesktop.appearance";

#[derive(Debug, Clone, Default, PartialEq)]
struct Desktop {
    /// The GTK theme, such as "Mint-L-Dark-Blue".
    theme_name: Option<String>,
    /// The theme's selection color from its gtk.css.
    theme_accent: Option<Rgba>,
    /// The portal's `color-scheme`: 0 no preference, 1 dark, 2 light.
    color_scheme: Option<u32>,
    portal_accent: Option<Rgba>,
}

impl Desktop {
    fn prefers_light(&self) -> bool {
        match self.color_scheme {
            Some(1) => false,
            Some(2) => true,
            // Xfce's portal says "no preference" even with a dark theme,
            // so the theme's name decides. Nothing known at all: dark.
            _ => self
                .theme_name
                .as_ref()
                .is_some_and(|name| !name.to_ascii_lowercase().contains("dark")),
        }
    }

    fn accent(&self) -> Option<Accent> {
        self.portal_accent
            .or(self.theme_accent)
            .map(Accent::from_color)
    }

    /// What the tray and Settings show from this.
    fn look(&self) -> (bool, Option<Accent>) {
        (self.prefers_light(), self.accent())
    }
}

static DESKTOP: Mutex<Option<Desktop>> = Mutex::new(None);

fn read<R>(f: impl FnOnce(&Desktop) -> R) -> R {
    let state = DESKTOP.lock().unwrap_or_else(|e| e.into_inner());
    f(state.as_ref().unwrap_or(&Desktop::default()))
}

/// Applies a change and, if the tray or Settings would look different,
/// tells them. Called on the watcher threads.
fn change(f: impl FnOnce(&mut Desktop)) {
    let changed = {
        let mut state = DESKTOP.lock().unwrap_or_else(|e| e.into_inner());
        let desktop = state.get_or_insert_with(Desktop::default);
        let before = desktop.look();
        f(desktop);
        desktop.look() != before
    };
    if changed {
        tray::desktop_changed();
        settings::desktop_changed();
    }
}

/// Whether the desktop uses a light theme. Safe from any thread.
pub fn prefers_light() -> bool {
    read(Desktop::prefers_light)
}

/// The desktop's accent color, or None to keep Fluent's blue.
pub fn accent() -> Option<Accent> {
    read(Desktop::accent)
}

/// Reads the GTK theme now, so the first tray icon and Settings window
/// match it, and starts watching. The portal can take a while to answer,
/// so it is only read on its thread.
pub fn start() {
    let shared = x11::shared();
    let theme_name = XSettings::new(&shared.conn, shared.screen)
        .and_then(|xs| xs.theme_name(&shared.conn, xs.owner(&shared.conn)?));
    let theme_accent = theme_name.as_deref().and_then(theme_accent);
    {
        let mut state = DESKTOP.lock().unwrap_or_else(|e| e.into_inner());
        let desktop = state.get_or_insert_with(Desktop::default);
        desktop.theme_name = theme_name;
        desktop.theme_accent = theme_accent;
    }
    let _ = std::thread::Builder::new()
        .name("xsettings".to_string())
        .spawn(watch_xsettings);
    let _ = std::thread::Builder::new()
        .name("portal-settings".to_string())
        .spawn(watch_portal);
}

/// The atoms of the XSETTINGS protocol for one screen.
struct XSettings {
    selection: u32,
    settings: u32,
    manager: u32,
}

impl XSettings {
    fn new(conn: &RustConnection, screen: usize) -> Option<XSettings> {
        let atom = |name: &str| {
            conn.intern_atom(false, name.as_bytes())
                .ok()?
                .reply()
                .ok()
                .map(|reply| reply.atom)
        };
        Some(XSettings {
            selection: atom(&format!("_XSETTINGS_S{screen}"))?,
            settings: atom("_XSETTINGS_SETTINGS")?,
            manager: atom("MANAGER")?,
        })
    }

    /// The XSETTINGS manager's window (xfsettingsd on Xfce).
    fn owner(&self, conn: &RustConnection) -> Option<Window> {
        let owner = conn
            .get_selection_owner(self.selection)
            .ok()?
            .reply()
            .ok()?
            .owner;
        (owner != NONE).then_some(owner)
    }

    fn theme_name(&self, conn: &RustConnection, owner: Window) -> Option<String> {
        let reply = conn
            .get_property(false, owner, self.settings, AtomEnum::ANY, 0, u32::MAX / 4)
            .ok()?
            .reply()
            .ok()?;
        xsettings_string(&reply.value, "Net/ThemeName")
    }
}

/// Follows theme changes on its own X connection, so the main loop sees
/// none of these events.
fn watch_xsettings() {
    let Ok((conn, screen)) = x11rb::connect(None) else {
        return;
    };
    let Some(xs) = XSettings::new(&conn, screen) else {
        return;
    };
    let root = conn.setup().roots[screen].root;
    // A manager that starts later (xfsettingsd at sign-in) announces itself
    // with a MANAGER message on the root window.
    let _ = conn.change_window_attributes(
        root,
        &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
    );
    let follow = |conn: &RustConnection| {
        let owner = xs.owner(conn)?;
        let _ = conn.change_window_attributes(
            owner,
            &ChangeWindowAttributesAux::new()
                .event_mask(EventMask::PROPERTY_CHANGE | EventMask::STRUCTURE_NOTIFY),
        );
        Some(owner)
    };
    let refresh = |conn: &RustConnection, owner: Option<Window>| {
        let name = owner.and_then(|owner| xs.theme_name(conn, owner));
        if name.is_some() && read(|d| d.theme_name != name) {
            let accent = name.as_deref().and_then(theme_accent);
            change(|d| {
                d.theme_name = name;
                d.theme_accent = accent;
            });
        }
    };
    let mut owner = follow(&conn);
    let _ = conn.flush();
    // A change between `start`'s read and the watch above.
    refresh(&conn, owner);
    while let Ok(event) = conn.wait_for_event() {
        match event {
            Event::PropertyNotify(e) if Some(e.window) == owner && e.atom == xs.settings => {
                refresh(&conn, owner);
            }
            Event::ClientMessage(e)
                if e.window == root
                    && e.type_ == xs.manager
                    && e.data.as_data32()[1] == xs.selection =>
            {
                owner = follow(&conn);
                let _ = conn.flush();
                refresh(&conn, owner);
            }
            Event::DestroyNotify(e) if Some(e.window) == owner => owner = None,
            _ => {}
        }
    }
}

/// Reads the portal's appearance settings, then follows its changes.
fn watch_portal() {
    let Ok(bus) = Bus::session() else {
        return;
    };
    let Ok(proxy) = Proxy::new(
        &bus,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Settings",
    ) else {
        return;
    };
    // Listening first, so a change during the reads below is not missed.
    let signals = proxy.receive_signal("SettingChanged").ok();
    let scheme = read_portal(&proxy, "color-scheme");
    let accent = read_portal(&proxy, "accent-color");
    change(|d| {
        d.color_scheme = scheme.as_ref().and_then(|v| color_scheme(v));
        d.portal_accent = accent.as_ref().and_then(|v| portal_accent(v));
    });
    for message in signals.into_iter().flatten() {
        let Ok((namespace, key, value)) =
            message
                .body()
                .deserialize::<(String, String, zbus::zvariant::OwnedValue)>()
        else {
            continue;
        };
        if namespace != APPEARANCE {
            continue;
        }
        match key.as_str() {
            "color-scheme" => change(|d| d.color_scheme = color_scheme(&value)),
            "accent-color" => change(|d| d.portal_accent = portal_accent(&value)),
            _ => {}
        }
    }
}

/// One appearance setting: `ReadOne` from version 2 of the interface,
/// `Read` (which wraps the value in one more variant) before that.
fn read_portal(proxy: &Proxy, key: &str) -> Option<zbus::zvariant::OwnedValue> {
    proxy
        .call("ReadOne", &(APPEARANCE, key))
        .or_else(|_| proxy.call("Read", &(APPEARANCE, key)))
        .ok()
}

fn unwrap_variant<'a>(value: &'a Value<'a>) -> &'a Value<'a> {
    match value {
        Value::Value(inner) => unwrap_variant(inner),
        other => other,
    }
}

fn color_scheme(value: &Value) -> Option<u32> {
    match unwrap_variant(value) {
        Value::U32(scheme) => Some(*scheme),
        _ => None,
    }
}

/// `(ddd)`, each channel from 0 to 1; anything outside means "not set".
fn portal_accent(value: &Value) -> Option<Rgba> {
    let Value::Structure(fields) = unwrap_variant(value) else {
        return None;
    };
    let channel = |field: &Value| match unwrap_variant(field) {
        Value::F64(v) if (0.0..=1.0).contains(v) => Some((v * 255.0).round() as u8),
        _ => None,
    };
    match fields.fields() {
        [r, g, b] => Some(Rgba {
            r: channel(r)?,
            g: channel(g)?,
            b: channel(b)?,
            a: 255,
        }),
        _ => None,
    }
}

/// A string setting from an `_XSETTINGS_SETTINGS` property, as the
/// XSETTINGS specification lays it out.
fn xsettings_string(data: &[u8], wanted: &str) -> Option<String> {
    let big_endian = match data.first()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let u16_at = |at: usize| {
        let bytes: [u8; 2] = data.get(at..at + 2)?.try_into().ok()?;
        Some(if big_endian {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        })
    };
    let u32_at = |at: usize| {
        let bytes: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        Some(if big_endian {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        })
    };
    let padded = |len: usize| len.div_ceil(4) * 4;
    let count = u32_at(8)?;
    let mut at = 12;
    for _ in 0..count {
        let kind = *data.get(at)?;
        let name_len = usize::from(u16_at(at + 2)?);
        let name = data.get(at + 4..at + 4 + name_len)?;
        // The name, then the serial of its last change.
        at += 4 + padded(name_len) + 4;
        match kind {
            0 => at += 4,
            1 => {
                let len = usize::try_from(u32_at(at)?).ok()?;
                let value = data.get(at + 4..at + 4 + len)?;
                if name == wanted.as_bytes() {
                    return String::from_utf8(value.to_vec()).ok();
                }
                at += 4 + padded(len);
            }
            2 => at += 8,
            _ => return None,
        }
    }
    None
}

/// Where GTK looks for themes, most personal first.
fn theme_dirs() -> Vec<PathBuf> {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let home = env("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    if let Some(home) = &home {
        dirs.push(home.join(".themes"));
    }
    match env("XDG_DATA_HOME") {
        Some(data) => dirs.push(PathBuf::from(data).join("themes")),
        None => dirs.extend(home.map(|h| h.join(".local/share/themes"))),
    }
    let data_dirs = env("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(
        data_dirs
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(dir).join("themes")),
    );
    dirs
}

/// The selection color of a GTK 3 theme, read from its gtk.css.
fn theme_accent(theme_name: &str) -> Option<Rgba> {
    if theme_name.is_empty() || theme_name.contains('/') || theme_name.starts_with('.') {
        return None;
    }
    theme_dirs().into_iter().find_map(|dir| {
        let css = std::fs::read_to_string(dir.join(theme_name).join("gtk-3.0/gtk.css")).ok()?;
        selection_color(&css)
    })
}

/// `theme_selected_bg_color`, or the older `selected_bg_color`, from a
/// stylesheet's `@define-color` rules, following references to other
/// defined colors.
fn selection_color(css: &str) -> Option<Rgba> {
    let defines: HashMap<&str, &str> = css
        .split("@define-color")
        .skip(1)
        .filter_map(|rule| {
            let rule = rule.split(';').next()?.trim();
            let (name, value) = rule.split_once(char::is_whitespace)?;
            Some((name, value.trim()))
        })
        .collect();
    let resolve = |name: &str| {
        let mut value = *defines.get(name)?;
        for _ in 0..8 {
            match value.strip_prefix('@') {
                Some(other) => value = defines.get(other)?,
                None => return parse_color(value),
            }
        }
        None
    };
    resolve("theme_selected_bg_color").or_else(|| resolve("selected_bg_color"))
}

/// `#rgb`, `#rrggbb`, `rgb(…)` and `rgba(…)`; GTK's color functions
/// (`shade`, `mix`, …) are not worked out.
fn parse_color(value: &str) -> Option<Rgba> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let digit = |i: usize| u8::from_str_radix(hex.get(i..i + 1)?, 16).ok();
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        return match hex.len() {
            3 => Some(Rgba {
                r: digit(0)? * 17,
                g: digit(1)? * 17,
                b: digit(2)? * 17,
                a: 255,
            }),
            6 | 8 => Some(Rgba {
                r: byte(0)?,
                g: byte(2)?,
                b: byte(4)?,
                a: 255,
            }),
            _ => None,
        };
    }
    let inner = value
        .strip_prefix("rgba(")
        .or_else(|| value.strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let channel = |part: &str| {
        let part = part.trim();
        match part.strip_suffix('%') {
            Some(percent) => {
                let v: f32 = percent.trim().parse().ok()?;
                Some((v.clamp(0.0, 100.0) * 2.55).round() as u8)
            }
            None => {
                let v: f32 = part.parse().ok()?;
                Some(v.clamp(0.0, 255.0).round() as u8)
            }
        }
    };
    let parts: Vec<&str> = inner.split(',').collect();
    match parts.as_slice() {
        [r, g, b] | [r, g, b, _] => Some(Rgba {
            r: channel(r)?,
            g: channel(g)?,
            b: channel(b)?,
            a: 255,
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An `_XSETTINGS_SETTINGS` value in the given byte order.
    fn settings(big_endian: bool, entries: &[(u8, &str, &[u8])]) -> Vec<u8> {
        let u16b = |v: u16| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let u32b = |v: u32| {
            if big_endian {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let pad = |out: &mut Vec<u8>| {
            while !out.len().is_multiple_of(4) {
                out.push(0);
            }
        };
        let mut out = vec![u8::from(big_endian), 0, 0, 0];
        out.extend(u32b(7));
        out.extend(u32b(entries.len() as u32));
        for (kind, name, value) in entries {
            out.extend([*kind, 0]);
            out.extend(u16b(name.len() as u16));
            out.extend(name.as_bytes());
            pad(&mut out);
            out.extend(u32b(3));
            if *kind == 1 {
                out.extend(u32b(value.len() as u32));
            }
            out.extend(*value);
            pad(&mut out);
        }
        out
    }

    #[test]
    fn xsettings_finds_a_string_after_other_kinds() {
        for big_endian in [false, true] {
            let data = settings(
                big_endian,
                &[
                    (0, "Net/DoubleClickTime", &[0, 0, 1, 144]),
                    (2, "Gtk/Color", &[0; 8]),
                    (1, "Gtk/FontName", b"Ubuntu 10"),
                    (1, "Net/ThemeName", b"Mint-L-Dark-Blue"),
                ],
            );
            assert_eq!(
                xsettings_string(&data, "Net/ThemeName").as_deref(),
                Some("Mint-L-Dark-Blue")
            );
            assert_eq!(
                xsettings_string(&data, "Gtk/FontName").as_deref(),
                Some("Ubuntu 10")
            );
            assert_eq!(xsettings_string(&data, "Net/IconThemeName"), None);
        }
    }

    #[test]
    fn xsettings_rejects_cut_or_unknown_data() {
        let data = settings(false, &[(1, "Net/ThemeName", b"Adwaita")]);
        assert_eq!(
            xsettings_string(&data[..data.len() - 4], "Net/ThemeName"),
            None
        );
        assert_eq!(xsettings_string(&[2, 0, 0, 0], "Net/ThemeName"), None);
        assert_eq!(xsettings_string(&[], "Net/ThemeName"), None);
    }

    #[test]
    fn the_portal_scheme_wins_and_the_theme_name_decides_otherwise() {
        let mut d = Desktop {
            theme_name: Some("Mint-L-Dark-Blue".into()),
            ..Desktop::default()
        };
        assert!(!d.prefers_light());
        d.color_scheme = Some(0);
        assert!(!d.prefers_light());
        d.color_scheme = Some(2);
        assert!(d.prefers_light());
        d.color_scheme = None;
        d.theme_name = Some("Mint-L-Blue".into());
        assert!(d.prefers_light());
        d.color_scheme = Some(1);
        assert!(!d.prefers_light());
        assert!(!Desktop::default().prefers_light());
    }

    #[test]
    fn the_portal_accent_comes_before_the_theme_one() {
        let mint = Rgba::hex(0x596EB5FF);
        let mut d = Desktop {
            theme_accent: Some(Rgba::hex(0x9C7E65FF)),
            ..Desktop::default()
        };
        assert_eq!(d.accent(), Some(Accent::from_color(Rgba::hex(0x9C7E65FF))));
        d.portal_accent = Some(mint);
        assert_eq!(d.accent(), Some(Accent::from_color(mint)));
        assert_eq!(Desktop::default().accent(), None);
    }

    #[test]
    fn portal_values_unwrap_and_validate() {
        let scheme = Value::from(1u32);
        assert_eq!(color_scheme(&scheme), Some(1));
        assert_eq!(
            color_scheme(&Value::Value(Box::new(Value::from(2u32)))),
            Some(2)
        );
        assert_eq!(color_scheme(&Value::from("dark")), None);

        let mint = Value::from((
            0.34901960784313724f64,
            0.43137254901960786f64,
            0.7098039215686275f64,
        ));
        assert_eq!(portal_accent(&mint), Some(Rgba::hex(0x596EB5FF)));
        let wrapped = Value::Value(Box::new(mint.try_clone().expect("clone")));
        assert_eq!(portal_accent(&wrapped), Some(Rgba::hex(0x596EB5FF)));
        let unset = Value::from((-1.0f64, -1.0f64, -1.0f64));
        assert_eq!(portal_accent(&unset), None);
    }

    #[test]
    fn gtk_css_selection_colors_resolve() {
        let css = "@define-color fg_color #eeeeee;\n\
                   @define-color theme_selected_bg_color #596eb5;\n\
                   @define-color selected_bg_color #5b73c4;";
        assert_eq!(selection_color(css), Some(Rgba::hex(0x596EB5FF)));
        let css = "@define-color accent rgb(53, 132, 228);\n\
                   @define-color selected_bg_color @accent;";
        assert_eq!(selection_color(css), Some(Rgba::hex(0x3584E4FF)));
        let css = "@define-color theme_selected_bg_color #abc ;";
        assert_eq!(selection_color(css), Some(Rgba::hex(0xAABBCCFF)));
        let css = "@define-color selected_bg_color shade(#596eb5, 0.9);";
        assert_eq!(selection_color(css), None);
        let css = "@define-color a @b; @define-color b @a; @define-color selected_bg_color @a;";
        assert_eq!(selection_color(css), None);
        assert_eq!(selection_color(""), None);
    }

    #[test]
    fn colors_parse_in_the_forms_gtk_themes_use() {
        assert_eq!(parse_color("#596EB5"), Some(Rgba::hex(0x596EB5FF)));
        assert_eq!(parse_color("#596eb5cc"), Some(Rgba::hex(0x596EB5FF)));
        assert_eq!(
            parse_color("rgba(89, 110, 181, 0.5)"),
            Some(Rgba::hex(0x596EB5FF))
        );
        assert_eq!(
            parse_color("rgb(100%, 0%, 50%)"),
            Some(Rgba::hex(0xFF0080FF))
        );
        assert_eq!(parse_color("#59"), None);
        assert_eq!(parse_color("blue"), None);
    }

    #[test]
    fn theme_names_cannot_leave_the_theme_folders() {
        assert_eq!(theme_accent("../../etc"), None);
        assert_eq!(theme_accent(".hidden"), None);
        assert_eq!(theme_accent(""), None);
    }
}
