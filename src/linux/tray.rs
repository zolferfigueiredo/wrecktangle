//! The tray icon, a StatusNotifierItem that Xfce's panel shows in its
//! Status Tray. Its menu runs on ksni's own thread, so each item only posts
//! a message to the main loop. The icon is one color, like the panel's own
//! status icons: white for the dark theme, black for the light one.

use std::sync::OnceLock;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, ToolTip};

use super::x11::{self, Message};
use super::{app, desktop};
use crate::theme::{self, Mode, Theme};
use crate::{lang, startup};

/// The Settings window's icon, in the logo's colors.
const WINDOW_PNGS: [&[u8]; 3] = [
    include_bytes!("../../assets/wrecktangle-32.png"),
    include_bytes!("../../assets/wrecktangle-48.png"),
    include_bytes!("../../assets/wrecktangle-64.png"),
];

const WHITE_PNGS: [&[u8]; 6] = [
    include_bytes!("../../assets/wrecktangle-tray-white-16.png"),
    include_bytes!("../../assets/wrecktangle-tray-white-20.png"),
    include_bytes!("../../assets/wrecktangle-tray-white-24.png"),
    include_bytes!("../../assets/wrecktangle-tray-white-32.png"),
    include_bytes!("../../assets/wrecktangle-tray-white-48.png"),
    include_bytes!("../../assets/wrecktangle-tray-white-64.png"),
];

const BLACK_PNGS: [&[u8]; 6] = [
    include_bytes!("../../assets/wrecktangle-tray-black-16.png"),
    include_bytes!("../../assets/wrecktangle-tray-black-20.png"),
    include_bytes!("../../assets/wrecktangle-tray-black-24.png"),
    include_bytes!("../../assets/wrecktangle-tray-black-32.png"),
    include_bytes!("../../assets/wrecktangle-tray-black-48.png"),
    include_bytes!("../../assets/wrecktangle-tray-black-64.png"),
];

/// The Theme setting, kept here because ksni asks for the icon on its own
/// thread, away from the config.
struct Tray {
    theme: Theme,
}

static HANDLE: OnceLock<Handle<Tray>> = OnceLock::new();
static WHITE: OnceLock<Vec<Icon>> = OnceLock::new();
static BLACK: OnceLock<Vec<Icon>> = OnceLock::new();
static WINDOW: OnceLock<Vec<Icon>> = OnceLock::new();

fn decode_all(pngs: &[&[u8]]) -> Vec<Icon> {
    pngs.iter().filter_map(|png| decode(png)).collect()
}

// The panel picks the size it needs, so every pixel-snapped size goes along.
fn tray_icons(mode: Mode) -> &'static [Icon] {
    match mode {
        Mode::Dark => WHITE.get_or_init(|| decode_all(&WHITE_PNGS)),
        Mode::Light => BLACK.get_or_init(|| decode_all(&BLACK_PNGS)),
    }
}

/// Decodes a PNG into the ARGB32, big-endian pixels StatusNotifierItem wants.
fn decode(png: &[u8]) -> Option<Icon> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buffer = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buffer).ok()?;
    let pixels = &buffer[..info.buffer_size()];
    let data: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| [a, r, g, b])
            .collect(),
        png::ColorType::Rgb => pixels
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [255, r, g, b])
            .collect(),
        _ => return None,
    };
    Some(Icon {
        width: i32::try_from(info.width).ok()?,
        height: i32::try_from(info.height).ok()?,
        data,
    })
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "wrecktangle".to_string()
    }

    fn title(&self) -> String {
        "Wrecktangle".to_string()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        tray_icons(theme::current_mode(self.theme)).to_vec()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "Wrecktangle".to_string(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        x11::post(Message::OpenSettings);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: lang::t("tray.settings"),
                activate: Box::new(|_: &mut Self| x11::post(Message::OpenSettings)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: lang::t("tray.check_updates"),
                activate: Box::new(|_: &mut Self| x11::post(Message::CheckUpdates)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: lang::t("tray.startup"),
                checked: startup::is_enabled(),
                activate: Box::new(|_: &mut Self| x11::post(Message::ToggleStartup)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: lang::t("tray.quit"),
                activate: Box::new(|_: &mut Self| x11::post(Message::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Shows the icon. At sign-in the panel can start after Wrecktangle, so the
/// item waits for a tray to appear instead of giving up. Also starts
/// following the desktop's theme, which the System theme uses.
pub fn start() -> bool {
    desktop::start();
    let tray = Tray {
        theme: app::current_config().theme,
    };
    match tray.assume_sni_available(true).spawn() {
        Ok(handle) => HANDLE.set(handle).is_ok(),
        Err(_) => false,
    }
}

/// The Theme setting changed.
pub fn set_theme(theme: Theme) {
    if let Some(handle) = HANDLE.get() {
        handle.update(|tray| tray.theme = theme);
    }
}

/// The desktop's theme changed; with the System theme the icon follows.
/// ksni sends the panel the new icon only if it differs. Safe from any
/// thread.
pub fn desktop_changed() {
    refresh();
}

/// `_NET_WM_ICON` for the Settings window: width, height and ARGB pixels for
/// each size.
pub fn window_icon() -> Vec<u32> {
    WINDOW
        .get_or_init(|| decode_all(&WINDOW_PNGS))
        .iter()
        .flat_map(|icon| {
            [icon.width as u32, icon.height as u32].into_iter().chain(
                icon.data
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|&argb| u32::from_be_bytes(argb)),
            )
        })
        .collect()
}

/// Rebuilds the menu, after the language or launch at startup changed.
pub fn refresh() {
    if let Some(handle) = HANDLE.get() {
        handle.update(|_| {});
    }
}

pub fn stop() {
    if let Some(handle) = HANDLE.get() {
        handle.shutdown().wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tray_png_decodes_to_argb() {
        for pngs in [WHITE_PNGS, BLACK_PNGS] {
            let icons = decode_all(&pngs);
            assert_eq!(icons.len(), pngs.len());
            for (icon, size) in icons.iter().zip([16, 20, 24, 32, 48, 64]) {
                assert_eq!((icon.width, icon.height), (size, size));
                assert_eq!(icon.data.len(), (size * size * 4) as usize);
            }
        }
        let window = decode_all(&WINDOW_PNGS);
        assert_eq!(
            window.iter().map(|i| i.width).collect::<Vec<_>>(),
            [32, 48, 64]
        );
    }

    #[test]
    fn tray_icons_are_one_color_with_the_same_shape() {
        for (white, black) in tray_icons(Mode::Dark).iter().zip(tray_icons(Mode::Light)) {
            let mut drawn = 0;
            for (w, b) in white
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .zip(black.data.as_chunks::<4>().0)
            {
                // ARGB: the same coverage, white versus black.
                assert_eq!(w[0], b[0]);
                if w[0] > 0 {
                    drawn += 1;
                    assert_eq!(&w[1..], &[255, 255, 255]);
                    assert_eq!(&b[1..], &[0, 0, 0]);
                }
            }
            assert!(drawn > 0);
        }
    }
}
