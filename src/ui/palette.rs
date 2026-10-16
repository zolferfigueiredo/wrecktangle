//! Settings window colors: the Windows 11 Fluent tokens the window was
//! designed with, plus the app's own shades of the foreground. The accent is
//! the one picked in Windows (Fluent's blue when Windows gives none), and
//! "Wrecktangle colors" swaps it for the logo's yellow, as on the website.

use crate::theme::Mode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// `0xRRGGBBAA`.
    pub const fn hex(v: u32) -> Rgba {
        Rgba {
            r: (v >> 24) as u8,
            g: (v >> 16) as u8,
            b: (v >> 8) as u8,
            a: v as u8,
        }
    }

    /// The same color with its alpha replaced, not multiplied.
    pub fn with_alpha(self, alpha: f32) -> Rgba {
        Rgba {
            a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ..self
        }
    }

    pub const TRANSPARENT: Rgba = Rgba::hex(0x00000000);
}

/// The two shades of the Windows accent color that Windows itself uses for
/// accent fills: a lighter one on dark backgrounds, a darker one on light.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accent {
    pub on_dark: Rgba,
    pub on_light: Rgba,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Rgba,
    pub foreground: Rgba,
    /// Fills: the selected tab bar, layout icons, switches, check boxes and
    /// primary buttons.
    pub accent: Rgba,
    /// Accent text and outlines (links, notes, focus rings), which need more
    /// contrast than a yellow fill has on a light background.
    pub accent_text: Rgba,
    /// Thin accent strokes (the display chevrons), a shade darker than the
    /// fill on light backgrounds.
    pub accent_stroke: Rgba,
    /// Text and marks drawn on top of an accent fill.
    pub accent_foreground: Rgba,
    pub accent_disabled: Rgba,
    pub accent_text_pressed: Rgba,
    pub accent_text_disabled: Rgba,
    pub control: Rgba,
    pub control_hover: Rgba,
    pub control_pressed: Rgba,
    pub control_disabled: Rgba,
    pub control_border: Rgba,
    pub control_alt: Rgba,
    pub control_alt_hover: Rgba,
    pub control_alt_pressed: Rgba,
    pub strong_stroke: Rgba,
    pub strong_stroke_disabled: Rgba,
    pub circle_border: Rgba,
    pub fluent_text_secondary: Rgba,
    pub fluent_text_tertiary: Rgba,
    pub text_disabled: Rgba,
    pub focus_outer: Rgba,
    pub focus_inner: Rgba,
    pub scrollbar_track: Rgba,
    pub scrollbar_thumb: Rgba,
    pub text_secondary: Rgba,
    pub text_tertiary: Rgba,
    pub divider: Rgba,
    pub card_border: Rgba,
    pub warning: Rgba,
}

pub fn palette(mode: Mode, wrecktangle_colors: bool, accent: Option<Accent>) -> Palette {
    let dark = mode == Mode::Dark;
    let pick = |d: u32, l: u32| Rgba::hex(if dark { d } else { l });
    let foreground = pick(0xFFFFFFFF, 0x000000E6);
    // Fluent's blue is what Windows' default accent gives in these two shades.
    let windows = match accent {
        Some(a) if dark => a.on_dark,
        Some(a) => a.on_light,
        None => pick(0x60CDFFFF, 0x005FB8FF),
    };
    let (accent, accent_text, accent_stroke, accent_foreground, accent_text_pressed, warning) =
        if wrecktangle_colors {
            // The website's yellow, with its dark ink on top and its darker
            // text yellow; warnings turn orange so notes stay distinct.
            (
                Rgba::hex(0xF0B000FF),
                pick(0xFFD84AFF, 0x8A5A00FF),
                pick(0xF0B000FF, 0xD49A00FF),
                Rgba::hex(0x1A1D21FF),
                Rgba::hex(0x1A1D2199),
                pick(0xFF9F43FF, 0xB54708FF),
            )
        } else {
            (
                windows,
                windows,
                windows,
                pick(0x000000FF, 0xFFFFFFFF),
                pick(0x00000080, 0xFFFFFFB3),
                pick(0xFFC83DFF, 0x9D5D00FF),
            )
        };
    Palette {
        background: pick(0x1C1C1CFF, 0xFAFAFAFF),
        foreground,
        accent,
        accent_text,
        accent_stroke,
        accent_foreground,
        accent_disabled: pick(0xFFFFFF29, 0x00000038),
        accent_text_pressed,
        accent_text_disabled: pick(0xFFFFFF87, 0xFFFFFFFF),
        control: pick(0xFFFFFF0F, 0xFFFFFFB3),
        control_hover: pick(0xFFFFFF14, 0xF9F9F980),
        control_pressed: pick(0xFFFFFF08, 0xF9F9F94D),
        control_disabled: pick(0xFFFFFF0A, 0xF9F9F94D),
        control_border: pick(0xFFFFFF14, 0x0000001A),
        control_alt: pick(0x0000001A, 0x00000005),
        control_alt_hover: pick(0xFFFFFF0A, 0x0000000F),
        control_alt_pressed: pick(0xFFFFFF12, 0x00000017),
        strong_stroke: pick(0xFFFFFF99, 0x00000099),
        strong_stroke_disabled: pick(0xFFFFFF29, 0x00000038),
        circle_border: pick(0xFFFFFF14, 0x0000001C),
        fluent_text_secondary: pick(0xFFFFFFC9, 0x00000099),
        fluent_text_tertiary: pick(0xFFFFFF8A, 0x00000073),
        text_disabled: pick(0xFFFFFF5E, 0x0000005E),
        focus_outer: pick(0xFFFFFFFF, 0x000000E6),
        focus_inner: pick(0x000000B3, 0xFFFFFFFF),
        scrollbar_track: pick(0x2C2C2CFF, 0xF0F0F0FF),
        scrollbar_thumb: pick(0xFFFFFF8A, 0x00000073),
        text_secondary: foreground.with_alpha(0.72),
        text_tertiary: foreground.with_alpha(0.55),
        divider: foreground.with_alpha(0.08),
        card_border: foreground.with_alpha(0.1),
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_unpacks_rgba() {
        assert_eq!(
            Rgba::hex(0x12345678),
            Rgba {
                r: 0x12,
                g: 0x34,
                b: 0x56,
                a: 0x78
            }
        );
    }

    #[test]
    fn with_alpha_replaces_the_alpha() {
        let light_fg = palette(Mode::Light, false, None).foreground;
        assert_eq!(light_fg.a, 0xE6);
        assert_eq!(light_fg.with_alpha(0.72), Rgba::hex(0x000000B8));
        assert_eq!(Rgba::hex(0xFFFFFF00).with_alpha(2.0).a, 255);
    }

    #[test]
    fn themes_differ_where_it_matters() {
        let dark = palette(Mode::Dark, false, None);
        let light = palette(Mode::Light, false, None);
        assert_ne!(dark.background, light.background);
        assert_ne!(dark.accent, light.accent);
        assert_eq!(dark.text_secondary, Rgba::hex(0xFFFFFFB8));
        assert_eq!(dark.background.a, 255);
        assert_eq!(light.background.a, 255);
    }

    fn red() -> Accent {
        Accent {
            on_dark: Rgba::hex(0xFF99A4FF),
            on_light: Rgba::hex(0xC42B1CFF),
        }
    }

    #[test]
    fn the_accent_follows_windows_unless_wrecktangle_colors_are_on() {
        let dark = palette(Mode::Dark, false, Some(red()));
        let light = palette(Mode::Light, false, Some(red()));
        assert_eq!(
            (dark.accent, dark.accent_text),
            (red().on_dark, red().on_dark)
        );
        assert_eq!(
            (light.accent, light.accent_stroke),
            (red().on_light, red().on_light)
        );
        // Without one from Windows it is Fluent's blue, which is also what
        // Windows' default accent gives.
        assert_eq!(
            palette(Mode::Dark, false, None).accent,
            Rgba::hex(0x60CDFFFF)
        );
        assert_eq!(
            palette(Mode::Light, false, None).accent,
            Rgba::hex(0x005FB8FF)
        );
        for mode in [Mode::Dark, Mode::Light] {
            assert_eq!(
                palette(mode, true, Some(red())).accent,
                Rgba::hex(0xF0B000FF)
            );
        }
    }

    #[test]
    fn wrecktangle_colors_swap_only_the_accent_set() {
        for mode in [Mode::Dark, Mode::Light] {
            let fluent = palette(mode, false, None);
            let yellow = palette(mode, true, Some(red()));
            assert_eq!(fluent.accent, fluent.accent_text);
            assert_eq!(yellow.accent, Rgba::hex(0xF0B000FF));
            assert_ne!(yellow.accent_text, fluent.accent_text);
            assert_ne!(yellow.warning, yellow.accent_text);
            assert_eq!(yellow.background, fluent.background);
            assert_eq!(yellow.foreground, fluent.foreground);
        }
    }
}
