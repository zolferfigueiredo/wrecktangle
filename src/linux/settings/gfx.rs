//! Draws the Settings window's operations into a tiny-skia pixmap and shapes
//! its text with cosmic-text. The Linux counterpart of `src/settings/gfx.rs`.
//! Everything here is built when Settings opens and dropped when it closes,
//! so the tray app never keeps the fonts in memory.

use std::cell::RefCell;
use std::rc::Rc;

use cosmic_text::{
    Align, Attrs, Buffer, Color, Ellipsize, EllipsizeHeightLimit, Family, FontSystem, Metrics,
    Shaping, SwashCache, Weight as FontWeight, Wrap as TextWrap,
};
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Mask, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, Stroke, Transform,
};

use crate::lang;
use crate::ui::Rect;
use crate::ui::palette::Rgba;
use crate::ui::view::{Op, Shaper, TextStyle, Weight, Wrap};

const LOGO_PNG: &[u8] = include_bytes!("../../../assets/wrecktangle-256.png");
/// Segoe UI's line height, which the Settings layout was tuned to.
const LINE_HEIGHT: f32 = 1.33;

/// Shaped text: cheap to clone, as the layout clones it into every frame.
#[derive(Clone)]
pub struct Text(Rc<RefCell<Buffer>>);

pub struct Gfx {
    fonts: FontSystem,
    glyphs: SwashCache,
    scale: f32,
    /// The desktop's UI font family, when it is installed.
    ui_family: Option<String>,
    family: Option<String>,
    logo: Option<Pixmap>,
    logo_scaled: Option<Pixmap>,
}

fn installed(fonts: &FontSystem, name: &str) -> bool {
    fonts
        .db()
        .faces()
        .any(|face| face.families.iter().any(|(family, _)| family == name))
}

impl Gfx {
    pub fn new(scale: f32, ui_family: Option<String>) -> Gfx {
        let fonts = FontSystem::new();
        let ui_family = ui_family.filter(|name| installed(&fonts, name));
        let mut gfx = Gfx {
            fonts,
            glyphs: SwashCache::new(),
            scale,
            ui_family,
            family: None,
            logo: Pixmap::decode_png(LOGO_PNG).ok(),
            logo_scaled: None,
        };
        gfx.language_changed();
        gfx
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
        self.logo_scaled = None;
    }

    /// Each CJK language gets its own font, so Han characters never come out
    /// of another language's font; the rest use the desktop's UI font.
    pub fn language_changed(&mut self) {
        let cjk = match lang::LANGUAGES[lang::current_index()].code {
            "zh" => Some("Noto Sans CJK SC"),
            "ja" => Some("Noto Sans CJK JP"),
            "ko" => Some("Noto Sans CJK KR"),
            _ => None,
        };
        let candidates = cjk
            .into_iter()
            .map(str::to_string)
            .chain(self.ui_family.clone())
            .chain(["Noto Sans".to_string(), "DejaVu Sans".to_string()]);
        let fonts = &self.fonts;
        self.family = candidates.into_iter().find(|name| installed(fonts, name));
    }

    pub fn draw(&mut self, target: &mut Pixmap, ops: &[Op<Text>]) {
        let s = self.scale;
        let mut shift = 0.0;
        let mut mask: Option<Mask> = None;
        let mut clip_px: Option<(i32, i32, i32, i32)> = None;
        for op in ops {
            let transform = Transform::from_row(s, 0.0, 0.0, s, 0.0, shift * s);
            match op {
                Op::Clear(color) => target.fill(skia_color(*color)),
                Op::Fill {
                    rect,
                    radius,
                    color,
                } => {
                    if let Some(path) = rounded(*rect, *radius) {
                        target.fill_path(
                            &path,
                            &paint(*color),
                            FillRule::Winding,
                            transform,
                            mask.as_ref(),
                        );
                    }
                }
                Op::Stroke {
                    rect,
                    radius,
                    width,
                    color,
                } => {
                    let inner = rect.inset(width / 2.0);
                    if let Some(path) = rounded(inner, (radius - width / 2.0).max(0.0)) {
                        let stroke = Stroke {
                            width: *width,
                            ..Stroke::default()
                        };
                        target.stroke_path(
                            &path,
                            &paint(*color),
                            &stroke,
                            transform,
                            mask.as_ref(),
                        );
                    }
                }
                Op::Text { text, x, y, color } => {
                    self.draw_text(target, text, *x, *y + shift, *color, clip_px);
                }
                Op::Lines {
                    figures,
                    width,
                    color,
                } => {
                    let mut builder = PathBuilder::new();
                    for figure in figures {
                        let mut points = figure.iter();
                        if let Some(&(x, y)) = points.next() {
                            builder.move_to(x, y);
                            for &(x, y) in points {
                                builder.line_to(x, y);
                            }
                        }
                    }
                    if let Some(path) = builder.finish() {
                        let stroke = Stroke {
                            width: *width,
                            line_cap: LineCap::Round,
                            line_join: LineJoin::Round,
                            ..Stroke::default()
                        };
                        target.stroke_path(
                            &path,
                            &paint(*color),
                            &stroke,
                            transform,
                            mask.as_ref(),
                        );
                    }
                }
                Op::Logo(rect) => self.draw_logo(target, rect.offset(0.0, shift), mask.as_ref()),
                Op::Clip(rect) => {
                    let mut clip = Mask::new(target.width(), target.height());
                    if let (Some(clip), Some(path)) = (clip.as_mut(), rounded(*rect, 0.0)) {
                        clip.fill_path(
                            &path,
                            FillRule::Winding,
                            false,
                            Transform::from_scale(s, s),
                        );
                    }
                    mask = clip;
                    clip_px = Some((
                        (rect.x * s).round() as i32,
                        (rect.y * s).round() as i32,
                        (rect.right() * s).round() as i32,
                        (rect.bottom() * s).round() as i32,
                    ));
                }
                Op::Unclip => {
                    mask = None;
                    clip_px = None;
                }
                Op::Shift(dy) => shift = *dy,
            }
        }
    }

    fn draw_text(
        &mut self,
        target: &mut Pixmap,
        text: &Text,
        x: f32,
        y: f32,
        color: Rgba,
        clip: Option<(i32, i32, i32, i32)>,
    ) {
        let origin_x = (x * self.scale).round() as i32;
        let origin_y = (y * self.scale).round() as i32;
        let (width, height) = (target.width() as i32, target.height() as i32);
        let (left, top, right, bottom) = clip.unwrap_or((0, 0, width, height));
        let (left, top) = (left.max(0), top.max(0));
        let (right, bottom) = (right.min(width), bottom.min(height));
        let pixels = target.data_mut();
        let mut buffer = text.0.borrow_mut();
        buffer.draw(
            &mut self.fonts,
            &mut self.glyphs,
            Color::rgba(color.r, color.g, color.b, color.a),
            |gx, gy, w, h, c| {
                let alpha = u32::from(c.a());
                if alpha == 0 {
                    return;
                }
                for py in origin_y + gy..origin_y + gy + h as i32 {
                    if py < top || py >= bottom {
                        continue;
                    }
                    for px in origin_x + gx..origin_x + gx + w as i32 {
                        if px < left || px >= right {
                            continue;
                        }
                        let i = (py as usize * width as usize + px as usize) * 4;
                        let src = [c.r(), c.g(), c.b()];
                        for (channel, value) in src.into_iter().enumerate() {
                            let dst = u32::from(pixels[i + channel]);
                            pixels[i + channel] =
                                ((u32::from(value) * alpha + dst * (255 - alpha) + 127) / 255)
                                    as u8;
                        }
                        let dst_a = u32::from(pixels[i + 3]);
                        pixels[i + 3] = (alpha + dst_a * (255 - alpha) / 255).min(255) as u8;
                    }
                }
            },
        );
    }

    // The 256 px logo, scaled once per size with a smooth filter.
    fn draw_logo(&mut self, target: &mut Pixmap, rect: Rect, mask: Option<&Mask>) {
        let s = self.scale;
        let size = (rect.w * s).round().max(1.0) as u32;
        if self.logo_scaled.as_ref().is_none_or(|p| p.width() != size) {
            self.logo_scaled = self.logo.as_ref().and_then(|logo| {
                let mut scaled = Pixmap::new(size, size)?;
                let factor = size as f32 / logo.width() as f32;
                scaled.draw_pixmap(
                    0,
                    0,
                    logo.as_ref(),
                    &PixmapPaint {
                        quality: FilterQuality::Bicubic,
                        ..PixmapPaint::default()
                    },
                    Transform::from_scale(factor, factor),
                    None,
                );
                Some(scaled)
            });
        }
        if let Some(scaled) = &self.logo_scaled {
            target.draw_pixmap(
                (rect.x * s).round() as i32,
                (rect.y * s).round() as i32,
                scaled.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                mask,
            );
        }
    }
}

impl Shaper for Gfx {
    type Text = Text;

    fn shape(
        &mut self,
        text: &str,
        style: TextStyle,
        max_width: f32,
        wrap: Wrap,
    ) -> (Text, f32, f32) {
        let s = self.scale;
        let metrics = Metrics::new(style.size * s, style.size * LINE_HEIGHT * s);
        let mut buffer = Buffer::new(&mut self.fonts, metrics);
        let limit = (max_width * s)
            .is_finite()
            .then_some(max_width * s)
            .filter(|w| *w < 1.0e6);
        let (text_wrap, width, align) = match wrap {
            Wrap::Line => (TextWrap::None, None, None),
            Wrap::Ellipsis => (TextWrap::None, limit, None),
            Wrap::Words => (TextWrap::WordOrGlyph, limit, None),
            Wrap::WordsCentered => (TextWrap::WordOrGlyph, limit, Some(Align::Center)),
        };
        buffer.set_wrap(text_wrap);
        if wrap == Wrap::Ellipsis {
            buffer.set_ellipsize(Ellipsize::End(EllipsizeHeightLimit::Lines(1)));
        }
        buffer.set_size(width, None);
        let family = match &self.family {
            Some(name) => Family::Name(name),
            None => Family::SansSerif,
        };
        let weight = match style.weight {
            Weight::Regular => FontWeight::NORMAL,
            Weight::SemiBold => FontWeight::SEMIBOLD,
            Weight::Bold => FontWeight::BOLD,
        };
        let attrs = Attrs::new().family(family).weight(weight);
        buffer.set_text(text, &attrs, Shaping::Advanced, align);
        buffer.shape_until_scroll(&mut self.fonts, false);

        let mut natural = 0.0f32;
        let mut lines = 0usize;
        for run in buffer.layout_runs() {
            natural = natural.max(run.line_w);
            lines += 1;
        }
        let h = lines.max(1) as f32 * metrics.line_height / s;
        let w = match wrap {
            Wrap::WordsCentered => max_width,
            Wrap::Line => natural / s,
            Wrap::Ellipsis | Wrap::Words => (natural / s).min(max_width),
        };
        (Text(Rc::new(RefCell::new(buffer))), w, h)
    }
}

fn skia_color(c: Rgba) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn paint(color: Rgba) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color(skia_color(color));
    paint.anti_alias = true;
    paint
}

/// A rectangle with corners of radius `r`, or a plain one when `r` is 0.
fn rounded(rect: Rect, r: f32) -> Option<Path> {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let r = r.min(rect.w / 2.0).min(rect.h / 2.0).max(0.0);
    if r <= 0.0 {
        let rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.w, rect.h)?;
        return Some(PathBuilder::from_rect(rect));
    }
    // A quarter circle as a cubic Bezier: control points at this fraction of r.
    let k = 0.552_284_8 * r;
    let (x, y, right, bottom) = (rect.x, rect.y, rect.right(), rect.bottom());
    let mut b = PathBuilder::new();
    b.move_to(x + r, y);
    b.line_to(right - r, y);
    b.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    b.line_to(right, bottom - r);
    b.cubic_to(
        right,
        bottom - r + k,
        right - r + k,
        bottom,
        right - r,
        bottom,
    );
    b.line_to(x + r, bottom);
    b.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
    b.line_to(x, y + r);
    b.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    b.close();
    b.finish()
}
