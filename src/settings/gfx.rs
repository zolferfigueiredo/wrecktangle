//! Direct2D and DirectWrite for the Settings window: measures text for
//! `ui::view::layout` and draws the operations `ui::view::paint` returns.
//! Drawing is in DIPs; the render target carries the window's DPI.

use std::collections::HashMap;

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F,
    D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES,
    D2D1_CAP_STYLE_FLAT, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINE_JOIN_ROUND, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1CreateFactory, ID2D1Bitmap, ID2D1Factory,
    ID2D1HwndRenderTarget, ID2D1RenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_REGULAR,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS,
    DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER, DWRITE_WORD_WRAPPING_EMERGENCY_BREAK,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory, IDWriteInlineObject,
    IDWriteTextFormat, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory,
    WICBitmapDitherTypeNone, WICBitmapInterpolationModeHighQualityCubic,
    WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::{PCWSTR, Result};
use windows_numerics::{Matrix3x2, Vector2};

use crate::lang;
use crate::ui::Rect;
use crate::ui::palette::Rgba;
use crate::ui::view::{Op, Shaper, TextStyle, Weight, Wrap};

const LOGO_PNG: &[u8] = include_bytes!("../../assets/wrecktangle-256.png");

/// Shaped text; None only if DirectWrite failed to make the layout.
pub type Text = Option<IDWriteTextLayout>;

struct Format {
    format: IDWriteTextFormat,
    ellipsis: Option<IDWriteInlineObject>,
}

/// Resources tied to the render target, rebuilt after device loss.
struct Device {
    target: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
    logo: Option<(u32, ID2D1Bitmap)>,
}

pub struct Gfx {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    round: ID2D1StrokeStyle,
    formats: HashMap<(u32, Weight), Format>,
    device: Option<Device>,
    dpi: u32,
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: f32::from(c.r) / 255.0,
        g: f32::from(c.g) / 255.0,
        b: f32::from(c.b) / 255.0,
        a: f32::from(c.a) / 255.0,
    }
}

fn rect_f(r: Rect) -> D2D_RECT_F {
    D2D_RECT_F {
        left: r.x,
        top: r.y,
        right: r.right(),
        bottom: r.bottom(),
    }
}

fn font_weight(weight: Weight) -> DWRITE_FONT_WEIGHT {
    match weight {
        Weight::Regular => DWRITE_FONT_WEIGHT_REGULAR,
        Weight::SemiBold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        Weight::Bold => DWRITE_FONT_WEIGHT_BOLD,
    }
}

impl Gfx {
    pub fn new(dpi: u32) -> Result<Gfx> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let round = d2d.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_FLAT,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )?;
            Ok(Gfx {
                d2d,
                dwrite,
                round,
                formats: HashMap::new(),
                device: None,
                dpi,
            })
        }
    }

    /// Drops the text formats so the next layout picks up the current
    /// language's font and locale.
    pub fn language_changed(&mut self) {
        self.formats.clear();
    }

    pub fn set_dpi(&mut self, dpi: u32) {
        self.dpi = dpi;
        if let Some(device) = &mut self.device {
            unsafe { device.target.SetDpi(dpi as f32, dpi as f32) };
            device.logo = None;
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if let Some(device) = &self.device {
            let size = D2D_SIZE_U { width, height };
            if unsafe { device.target.Resize(&size) }.is_err() {
                self.device = None;
            }
        }
    }

    fn format(&mut self, style: TextStyle) -> Option<&Format> {
        let key = ((style.size * 10.0).round() as u32, style.weight);
        if !self.formats.contains_key(&key) {
            let language = &lang::LANGUAGES[lang::current_index()];
            let family = match lang::ui_font() {
                "" => "Segoe UI",
                family => family,
            };
            let family = wide(family);
            let locale = wide(language.locale);
            let format = unsafe {
                self.dwrite.CreateTextFormat(
                    PCWSTR(family.as_ptr()),
                    None,
                    font_weight(style.weight),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    style.size,
                    PCWSTR(locale.as_ptr()),
                )
            }
            .ok()?;
            let ellipsis = unsafe { self.dwrite.CreateEllipsisTrimmingSign(&format) }.ok();
            self.formats.insert(key, Format { format, ellipsis });
        }
        self.formats.get(&key)
    }

    fn device(&mut self, hwnd: HWND) -> Result<&Device> {
        if self.device.is_none() {
            let mut client = RECT::default();
            unsafe { GetClientRect(hwnd, &mut client)? };
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                // An explicit DPI: 0 would mean the system DPI, which is wrong
                // on a second monitor under per-monitor awareness.
                dpiX: self.dpi as f32,
                dpiY: self.dpi as f32,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hwnd_properties = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D_SIZE_U {
                    width: (client.right - client.left).max(1) as u32,
                    height: (client.bottom - client.top).max(1) as u32,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            unsafe {
                let target = self
                    .d2d
                    .CreateHwndRenderTarget(&properties, &hwnd_properties)?;
                let brush = target.CreateSolidColorBrush(&color(Rgba::TRANSPARENT), None)?;
                self.device = Some(Device {
                    target,
                    brush,
                    logo: None,
                });
            }
        }
        Ok(self.device.as_ref().expect("device just created"))
    }

    /// Draws one frame. Device loss drops the render target, so the next
    /// paint rebuilds it.
    pub fn draw(&mut self, hwnd: HWND, ops: &[Op<Text>]) -> Result<()> {
        self.device(hwnd)?;
        self.prepare_logo(ops);
        let device = self.device.as_ref().expect("device created above");
        let target = &device.target;
        unsafe {
            target.BeginDraw();
            let logo = device.logo.as_ref().map(|(_, bitmap)| bitmap);
            for op in ops {
                self.op(target, &device.brush, logo, op);
            }
            target.SetTransform(&Matrix3x2::identity());
            let result = target.EndDraw(None, None);
            if let Err(error) = &result
                && error.code() == D2DERR_RECREATE_TARGET
            {
                self.device = None;
            }
            result
        }
    }

    fn op(
        &self,
        target: &ID2D1RenderTarget,
        brush: &ID2D1SolidColorBrush,
        logo: Option<&ID2D1Bitmap>,
        op: &Op<Text>,
    ) {
        unsafe {
            match op {
                Op::Clear(c) => target.Clear(Some(&color(*c))),
                Op::Fill {
                    rect,
                    radius,
                    color: c,
                } => {
                    brush.SetColor(&color(*c));
                    if *radius > 0.0 {
                        target.FillRoundedRectangle(
                            &D2D1_ROUNDED_RECT {
                                rect: rect_f(*rect),
                                radiusX: *radius,
                                radiusY: *radius,
                            },
                            brush,
                        );
                    } else {
                        target.FillRectangle(&rect_f(*rect), brush);
                    }
                }
                Op::Stroke {
                    rect,
                    radius,
                    width,
                    color: c,
                } => {
                    brush.SetColor(&color(*c));
                    let inner = rect.inset(width / 2.0);
                    let radius = (radius - width / 2.0).max(0.0);
                    target.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT {
                            rect: rect_f(inner),
                            radiusX: radius,
                            radiusY: radius,
                        },
                        brush,
                        *width,
                        None,
                    );
                }
                Op::Text {
                    text,
                    x,
                    y,
                    color: c,
                } => {
                    if let Some(layout) = text {
                        brush.SetColor(&color(*c));
                        target.DrawTextLayout(
                            Vector2::new(*x, *y),
                            layout,
                            brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                        );
                    }
                }
                Op::Lines {
                    figures,
                    width,
                    color: c,
                } => {
                    let Ok(geometry) = self.d2d.CreatePathGeometry() else {
                        return;
                    };
                    let Ok(sink) = geometry.Open() else {
                        return;
                    };
                    for figure in figures {
                        let points: Vec<Vector2> =
                            figure.iter().map(|&(x, y)| Vector2::new(x, y)).collect();
                        let Some((first, rest)) = points.split_first() else {
                            continue;
                        };
                        sink.BeginFigure(*first, D2D1_FIGURE_BEGIN_HOLLOW);
                        sink.AddLines(rest);
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    if sink.Close().is_ok() {
                        brush.SetColor(&color(*c));
                        target.DrawGeometry(&geometry, brush, *width, &self.round);
                    }
                }
                Op::Logo(rect) => {
                    if let Some(bitmap) = logo {
                        target.DrawBitmap(
                            bitmap,
                            Some(&rect_f(*rect)),
                            1.0,
                            D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                            None,
                        );
                    }
                }
                Op::Clip(rect) => {
                    target.PushAxisAlignedClip(&rect_f(*rect), D2D1_ANTIALIAS_MODE_ALIASED)
                }
                Op::Unclip => target.PopAxisAlignedClip(),
                Op::Shift(dy) => target.SetTransform(&Matrix3x2::translation(0.0, *dy)),
            }
        }
    }

    /// Makes the logo bitmap at the physical size it is about to be drawn
    /// at, pre-scaled by WIC, so Direct2D draws it pixel for pixel.
    fn prepare_logo(&mut self, ops: &[Op<Text>]) {
        let Some(px) = logo_px(ops, self.dpi) else {
            return;
        };
        let Some(device) = &mut self.device else {
            return;
        };
        if device.logo.as_ref().is_some_and(|(size, _)| *size == px) {
            return;
        }
        device.logo = logo_bitmap(&device.target, px, self.dpi).map(|bitmap| (px, bitmap));
    }
}

fn logo_px(ops: &[Op<Text>], dpi: u32) -> Option<u32> {
    ops.iter().find_map(|op| match op {
        Op::Logo(rect) => Some((rect.w * dpi as f32 / 96.0).round().max(1.0) as u32),
        _ => None,
    })
}

fn logo_bitmap(target: &ID2D1RenderTarget, px: u32, dpi: u32) -> Option<ID2D1Bitmap> {
    let pixels = decode_logo(px).ok()?;
    let properties = D2D1_BITMAP_PROPERTIES {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
        },
        dpiX: dpi as f32,
        dpiY: dpi as f32,
    };
    unsafe {
        target.CreateBitmap(
            D2D_SIZE_U {
                width: px,
                height: px,
            },
            Some(pixels.as_ptr().cast()),
            px * 4,
            &properties,
        )
    }
    .ok()
}

/// Decodes the embedded PNG to premultiplied BGRA at `px` x `px`. Converting
/// before scaling keeps the transparent edge from darkening.
fn decode_logo(px: u32) -> Result<Vec<u8>> {
    unsafe {
        let wic: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = wic.CreateStream()?;
        stream.InitializeFromMemory(LOGO_PNG)?;
        let decoder =
            wic.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let converter = wic.CreateFormatConverter()?;
        converter.Initialize(
            &frame,
            &GUID_WICPixelFormat32bppPBGRA,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        let scaler = wic.CreateBitmapScaler()?;
        scaler.Initialize(
            &converter,
            px,
            px,
            WICBitmapInterpolationModeHighQualityCubic,
        )?;
        let mut pixels = vec![0u8; (px * px * 4) as usize];
        scaler.CopyPixels(std::ptr::null(), px * 4, &mut pixels)?;
        Ok(pixels)
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
        let dwrite = self.dwrite.clone();
        let Some(format) = self.format(style) else {
            return (None, 0.0, 0.0);
        };
        let units: Vec<u16> = text.encode_utf16().collect();
        let width = match wrap {
            Wrap::Line => 100_000.0,
            _ => max_width.min(100_000.0),
        };
        unsafe {
            let Ok(layout) = dwrite.CreateTextLayout(&units, &format.format, width, 100_000.0)
            else {
                return (None, 0.0, 0.0);
            };
            match wrap {
                Wrap::Line => {
                    let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                }
                Wrap::Ellipsis => {
                    let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    let trimming = DWRITE_TRIMMING {
                        granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                        delimiter: 0,
                        delimiterCount: 0,
                    };
                    let _ = layout.SetTrimming(&trimming, format.ellipsis.as_ref());
                }
                Wrap::Words => {
                    let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_EMERGENCY_BREAK);
                }
                Wrap::WordsCentered => {
                    let _ = layout.SetWordWrapping(DWRITE_WORD_WRAPPING_EMERGENCY_BREAK);
                    let _ = layout.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                }
            }
            let mut metrics = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut metrics).is_err() {
                return (None, 0.0, 0.0);
            }
            let w = match wrap {
                Wrap::WordsCentered => width,
                Wrap::Ellipsis => metrics.widthIncludingTrailingWhitespace.min(width),
                _ => metrics.widthIncludingTrailingWhitespace,
            };
            (Some(layout), w, metrics.height)
        }
    }
}
