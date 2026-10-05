//! The Settings pages as plain data. `layout` positions everything for one
//! page, measuring text through a `Shaper`; `paint` turns that layout and the
//! current hover, press and focus state into drawing operations. Sizes and
//! colors follow the Slint design this window was first built with.

use super::palette::{Palette, Rgba};
use super::{IconGeometry, IconGlyph, IconShape, Rect, Scroll, Snap, layout_icon};
use crate::lang;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Shortcuts,
    General,
    About,
}

pub const PAGES: [Page; 3] = [Page::Shortcuts, Page::General, Page::About];

impl Page {
    fn title_key(self) -> &'static str {
        match self {
            Page::Shortcuts => "tab.shortcuts",
            Page::General => "tab.general",
            Page::About => "tab.about",
        }
    }
}

/// One shortcut row, in display form.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub label: String,
    pub keys: Vec<String>,
    pub note: String,
    pub warn: bool,
    pub recording: bool,
    pub hint: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SizeOption {
    pub label: String,
    pub checked: bool,
    pub locked: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Model {
    pub page: Page,
    pub rows: Vec<Row>,
    pub sizes: Vec<SizeOption>,
    pub language: usize,
    pub startup: bool,
    pub auto_update: bool,
    pub update_status: String,
    pub update_detail: String,
    pub update_available: bool,
    pub update_busy: bool,
    pub about_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Website,
    Author,
    Rectangle,
}

impl Link {
    pub fn url(self) -> &'static str {
        match self {
            Link::Website => "https://github.com/zolferfigueiredo/wrecktangle",
            Link::Author => "https://zolfer.com",
            Link::Rectangle => "https://rectangleapp.com",
        }
    }
}

/// Everything that can be hovered, pressed, focused or activated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Tab(Page),
    Pill(usize),
    Clear(usize),
    RestoreDefaults,
    Language,
    Size(usize),
    Startup,
    AutoUpdate,
    CheckNow,
    Download,
    Link(Link),
}

impl Target {
    /// Tabs and links show the hand cursor, like the Slint version did.
    pub fn uses_hand_cursor(self) -> bool {
        matches!(self, Target::Tab(_) | Target::Link(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    SemiBold,
    Bold,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: Weight,
}

const fn style(size: f32, weight: Weight) -> TextStyle {
    TextStyle { size, weight }
}

/// The default text size of the Fluent widgets (buttons, switches, check
/// boxes, the combo box), from the Windows message font.
const BODY: TextStyle = style(12.0, Weight::Regular);
const SMALL: TextStyle = style(11.0, Weight::Regular);
const KEY: TextStyle = style(11.0, Weight::SemiBold);
const LABEL: TextStyle = style(13.0, Weight::Regular);
const SECTION: TextStyle = style(13.0, Weight::SemiBold);
const SETTING: TextStyle = style(14.0, Weight::Regular);
const TAB: TextStyle = style(14.0, Weight::Regular);
const TAB_SELECTED: TextStyle = style(14.0, Weight::Bold);
const APP_NAME: TextStyle = style(24.0, Weight::Bold);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    /// One line at its natural width.
    Line,
    /// One line, cut with an ellipsis past the given width.
    Ellipsis,
    /// Wrapped at the given width, left aligned.
    Words,
    /// Wrapped at the given width, each line centered in it.
    WordsCentered,
}

/// Measures text. `shape` returns the shaped text and its width and height
/// in DIPs; for `WordsCentered` the width is the full given width.
pub trait Shaper {
    type Text: Clone;
    fn shape(
        &mut self,
        text: &str,
        style: TextStyle,
        max_width: f32,
        wrap: Wrap,
    ) -> (Self::Text, f32, f32);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Foreground,
    Secondary,
    Tertiary,
    Accent,
    Warning,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item<T> {
    Text {
        text: T,
        x: f32,
        y: f32,
        ink: Ink,
    },
    Card(Rect),
    Divider(Rect),
    Icon {
        x: f32,
        y: f32,
        geometry: IconGeometry,
    },
    Tab {
        page: Page,
        rect: Rect,
        label: T,
        label_rect: Rect,
        selected: bool,
    },
    Pill {
        index: usize,
        rect: Rect,
        recording: bool,
    },
    Chip(Rect),
    Clear {
        index: usize,
        rect: Rect,
    },
    Button {
        target: Target,
        rect: Rect,
        label: T,
        label_x: f32,
        label_y: f32,
        primary: bool,
        enabled: bool,
    },
    Switch {
        target: Target,
        rail: Rect,
        on: bool,
        label: T,
        label_x: f32,
        label_y: f32,
    },
    Check {
        index: usize,
        rect: Rect,
        mark: Rect,
        checked: bool,
        enabled: bool,
        label: T,
        label_x: f32,
        label_y: f32,
    },
    Combo {
        rect: Rect,
        label: T,
        label_x: f32,
        label_y: f32,
    },
    Link {
        link: Link,
        rect: Rect,
        text: T,
    },
    Logo(Rect),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub target: Target,
    pub rect: Rect,
    pub enabled: bool,
}

/// One laid out page. Header coordinates are window coordinates; content
/// coordinates start at the top of `viewport` and scroll.
#[derive(Debug, Clone)]
pub struct Layout<T> {
    pub header: Vec<Item<T>>,
    pub content: Vec<Item<T>>,
    pub viewport: Rect,
    pub content_height: f32,
    pub header_hits: Vec<Hit>,
    pub content_hits: Vec<Hit>,
}

impl<T> Layout<T> {
    /// The enabled target under a window point, given the snapped scroll
    /// offset the content was painted with.
    pub fn hit(&self, x: f32, y: f32, offset: f32) -> Option<Target> {
        if let Some(hit) = self.header_hits.iter().find(|h| h.rect.contains(x, y)) {
            return hit.enabled.then_some(hit.target);
        }
        if !self.viewport.contains(x, y) {
            return None;
        }
        let cy = y - self.viewport.y + offset;
        self.content_hits
            .iter()
            .find(|h| h.rect.contains(x, cy))
            .filter(|h| h.enabled)
            .map(|h| h.target)
    }

    /// Every target in visual order, with whether it can take focus.
    pub fn focus_order(&self) -> Vec<(Target, bool)> {
        self.header_hits
            .iter()
            .chain(&self.content_hits)
            .map(|h| (h.target, h.enabled))
            .collect()
    }

    pub fn has_target(&self, target: Target) -> bool {
        self.header_hits
            .iter()
            .chain(&self.content_hits)
            .any(|h| h.target == target && h.enabled)
    }

    /// A content target's rectangle in content coordinates; None for header
    /// targets and targets not on this page.
    pub fn content_rect(&self, target: Target) -> Option<Rect> {
        self.content_hits
            .iter()
            .find(|h| h.target == target)
            .map(|h| h.rect)
    }

    /// A target's rectangle in window coordinates for a scroll offset.
    pub fn window_rect(&self, target: Target, offset: f32) -> Option<Rect> {
        if let Some(hit) = self.header_hits.iter().find(|h| h.target == target) {
            return Some(hit.rect);
        }
        self.content_rect(target)
            .map(|r| r.offset(0.0, self.viewport.y - offset))
    }
}

const TAB_BAR_HEIGHT: f32 = 40.0;
const TAB_HEIGHT: f32 = 34.0;
const PAGE_PADDING: f32 = 24.0;

struct Builder<'a, S: Shaper> {
    shaper: &'a mut S,
    snap: Snap,
    items: Vec<Item<S::Text>>,
    hits: Vec<Hit>,
}

impl<S: Shaper> Builder<'_, S> {
    fn shape(
        &mut self,
        text: &str,
        style: TextStyle,
        max_width: f32,
        wrap: Wrap,
    ) -> Shaped<S::Text> {
        let (text, w, h) = self.shaper.shape(text, style, max_width.max(0.0), wrap);
        Shaped { text, w, h }
    }

    fn text(&mut self, shaped: &Shaped<S::Text>, x: f32, y: f32, ink: Ink) {
        self.items.push(Item::Text {
            text: shaped.text.clone(),
            x,
            y,
            ink,
        });
    }

    fn hit(&mut self, target: Target, rect: Rect, enabled: bool) {
        self.hits.push(Hit {
            target,
            rect,
            enabled,
        });
    }

    fn hairline(&self) -> f32 {
        self.snap.hairline()
    }

    /// A 13 px semibold group title; returns where the card below it starts.
    fn section_title(&mut self, key: &str, x: f32, y: f32, w: f32) -> f32 {
        let title = self.shape(&lang::t(key), SECTION, w, Wrap::Ellipsis);
        self.text(&title, x, y, Ink::Foreground);
        self.snap.round(y + title.h) + 4.0
    }

    /// Lays out a card's rows with `rows` (given the inner left, top and
    /// width, returning the bottom), then puts the card behind them.
    fn card(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        rows: impl FnOnce(&mut Self, f32, f32, f32) -> f32,
    ) -> f32 {
        let behind = self.items.len();
        let hair = self.hairline();
        let bottom = rows(self, x + hair, y + hair, w - 2.0 * hair);
        let card = Rect::new(x, y, w, bottom + hair - y);
        self.items.insert(behind, Item::Card(card));
        card.bottom()
    }

    fn divider(&mut self, x: f32, y: f32, w: f32) -> f32 {
        let hair = self.hairline();
        self.items.push(Item::Divider(Rect::new(x, y, w, hair)));
        y + hair
    }

    fn button(&mut self, key: &str) -> ButtonSize<S::Text> {
        let label = self.shape(&lang::t(key), BODY, f32::MAX, Wrap::Line);
        ButtonSize {
            w: (label.w + 24.0).max(32.0).ceil(),
            h: (label.h + 10.0).max(32.0),
            label,
        }
    }

    fn place_button(
        &mut self,
        size: &ButtonSize<S::Text>,
        target: Target,
        x: f32,
        y: f32,
        primary: bool,
        enabled: bool,
    ) {
        let rect = self.snap.rect(Rect::new(x, y, size.w, size.h));
        self.items.push(Item::Button {
            target,
            rect,
            label: size.label.text.clone(),
            label_x: rect.x + (rect.w - size.label.w) / 2.0,
            label_y: rect.y + (rect.h - size.label.h) / 2.0,
            primary,
            enabled,
        });
        self.hit(target, rect, enabled);
    }
}

struct Shaped<T> {
    text: T,
    w: f32,
    h: f32,
}

struct ButtonSize<T> {
    label: Shaped<T>,
    w: f32,
    h: f32,
}

/// Lays out the current page of `model` for a client area of `width` x
/// `height` DIPs.
pub fn layout<S: Shaper>(
    model: &Model,
    shaper: &mut S,
    width: f32,
    height: f32,
    snap: Snap,
) -> Layout<S::Text> {
    let mut b = Builder {
        shaper,
        snap,
        items: Vec::new(),
        hits: Vec::new(),
    };
    tab_bar(&mut b, model, width);
    let header = std::mem::take(&mut b.items);
    let header_hits = std::mem::take(&mut b.hits);

    let top = TAB_BAR_HEIGHT + snap.hairline();
    let viewport = Rect::new(0.0, top, width, (height - top).max(0.0));
    let content_height = match model.page {
        Page::Shortcuts => shortcuts_page(&mut b, model, width),
        Page::General => general_page(&mut b, model, width),
        Page::About => about_page(&mut b, model, width, viewport.h),
    };
    Layout {
        header,
        content: b.items,
        viewport,
        content_height: snap.round(content_height),
        header_hits,
        content_hits: b.hits,
    }
}

fn tab_bar<S: Shaper>(b: &mut Builder<S>, model: &Model, width: f32) {
    let mut x = 12.0;
    let y = 6.0;
    for page in PAGES {
        let title = lang::t(page.title_key());
        let selected = model.page == page;
        let bold = b.shape(&title, TAB_SELECTED, f32::MAX, Wrap::Line);
        let regular = b.shape(&title, TAB, f32::MAX, Wrap::Line);
        let tab_w = bold.w.max(regular.w).ceil() + 28.0;
        let label = if selected { bold } else { regular };
        let rect = Rect::new(x, y, tab_w, TAB_HEIGHT);
        b.items.push(Item::Tab {
            page,
            rect,
            label_rect: Rect::new(
                x + (tab_w - label.w) / 2.0,
                y + (TAB_HEIGHT - label.h) / 2.0 - 1.0,
                label.w,
                label.h,
            ),
            label: label.text,
            selected,
        });
        b.hit(Target::Tab(page), rect, true);
        x += tab_w + 4.0;
    }
    b.divider(0.0, TAB_BAR_HEIGHT, width);
}

const fn area(fx: f32, fy: f32, fw: f32, fh: f32) -> IconGlyph {
    IconGlyph::Area { fx, fy, fw, fh }
}

/// The Shortcuts page groups, as (title key, [(row index, icon)]). The row
/// indices follow `Action::ALL`.
const GROUPS: [(&str, &[(usize, IconGlyph)]); 4] = [
    (
        "group.halves",
        &[
            (0, area(0.0, 0.0, 0.5, 1.0)),
            (1, area(0.5, 0.0, 0.5, 1.0)),
            (2, area(0.0, 0.0, 1.0, 0.5)),
            (3, area(0.0, 0.5, 1.0, 0.5)),
        ],
    ),
    (
        "group.corners",
        &[
            (4, area(0.0, 0.0, 0.5, 0.5)),
            (5, area(0.5, 0.0, 0.5, 0.5)),
            (6, area(0.0, 0.5, 0.5, 0.5)),
            (7, area(0.5, 0.5, 0.5, 0.5)),
        ],
    ),
    (
        "group.window",
        &[(8, area(0.0, 0.0, 1.0, 1.0)), (9, IconGlyph::Center)],
    ),
    (
        "group.displays",
        &[(10, IconGlyph::Next), (11, IconGlyph::Previous)],
    ),
];

fn shortcuts_page<S: Shaper>(b: &mut Builder<S>, model: &Model, width: f32) -> f32 {
    let x = PAGE_PADDING;
    let w = (width - 2.0 * PAGE_PADDING).max(0.0);
    let mut y = 8.0;
    let intro = b.shape(&lang::t("shortcuts.intro"), BODY, w, Wrap::Words);
    b.text(&intro, x, y, Ink::Secondary);
    y = b.snap.round(y + intro.h);

    let empty = Row::default();
    for (title, specs) in GROUPS {
        y = b.section_title(title, x, y + 8.0, w);
        y = b.card(x, y, w, |b, x, mut y, w| {
            for (i, &(index, glyph)) in specs.iter().enumerate() {
                if i > 0 {
                    y = b.divider(x, y, w);
                }
                let row = model.rows.get(index).unwrap_or(&empty);
                y += shortcut_row(b, index, glyph, row, x, y, w);
            }
            y
        });
    }

    y += 8.0;
    let restore = b.button("button.restore_defaults");
    b.place_button(&restore, Target::RestoreDefaults, x, y, false, true);
    y + restore.h + 8.0
}

/// One shortcut row; returns its height.
fn shortcut_row<S: Shaper>(
    b: &mut Builder<S>,
    index: usize,
    glyph: IconGlyph,
    row: &Row,
    x: f32,
    y: f32,
    w: f32,
) -> f32 {
    let snap = b.snap;
    let note = if row.recording { &row.hint } else { &row.note };
    let min_h = snap.even(if note.is_empty() { 36.0 } else { 40.0 });
    let geometry = layout_icon(glyph, snap.scale);
    let icon_w = geometry.w / snap.scale;
    let icon_h = geometry.h / snap.scale;

    let pill = measure_pill(b, row);
    let right = x + w - 8.0;
    let clear_x = right - 24.0;
    let pill_x = clear_x - 12.0 - pill.w;
    let icon_x = snap.round(x + snap.round(14.0));
    let text_x = icon_x + icon_w + 12.0;
    let text_w = (pill_x - 12.0 - text_x).max(0.0);

    let label = b.shape(&row.label, LABEL, text_w, Wrap::Ellipsis);
    let sub = (!note.is_empty()).then(|| b.shape(note, SMALL, text_w, Wrap::Ellipsis));
    let block_h = label.h + sub.as_ref().map_or(0.0, |s| s.h);
    let tallest = icon_h.max(block_h).max(pill.h).max(24.0);
    let h = min_h.max(snap.round(tallest + 6.0));

    b.items.push(Item::Icon {
        x: icon_x,
        y: snap.round(y + (h - icon_h) / 2.0),
        geometry,
    });
    let label_y = y + (h - block_h) / 2.0;
    b.text(&label, text_x, label_y, Ink::Foreground);
    if let Some(sub) = &sub {
        let ink = if row.recording || row.warn {
            Ink::Warning
        } else {
            Ink::Accent
        };
        b.text(sub, text_x, label_y + label.h, ink);
    }

    let pill_rect = snap.rect(Rect::new(pill_x, y + (h - pill.h) / 2.0, pill.w, pill.h));
    b.items.push(Item::Pill {
        index,
        rect: pill_rect,
        recording: row.recording,
    });
    b.hit(Target::Pill(index), pill_rect, true);
    place_pill_content(b, pill, pill_rect);

    if !row.keys.is_empty() && !row.recording {
        let rect = snap.rect(Rect::new(clear_x, y + (h - 24.0) / 2.0, 24.0, 24.0));
        b.items.push(Item::Clear { index, rect });
        b.hit(Target::Clear(index), rect, true);
    }
    h
}

enum PillContent<T> {
    Prompt(Shaped<T>),
    Empty(Shaped<T>),
    Keys(Vec<(Shaped<T>, f32, f32)>),
}

struct PillSize<T> {
    content: PillContent<T>,
    w: f32,
    h: f32,
}

const PILL_PAD_X: f32 = 6.0;
const PILL_PAD_Y: f32 = 3.0;
const CHIP_GAP: f32 = 3.0;

fn measure_pill<S: Shaper>(b: &mut Builder<S>, row: &Row) -> PillSize<S::Text> {
    let (content, w, h) = if row.recording {
        let text = b.shape(&lang::t("recorder.prompt"), SMALL, f32::MAX, Wrap::Line);
        let (w, h) = (text.w, text.h);
        (PillContent::Prompt(text), w, h)
    } else if row.keys.is_empty() {
        let text = b.shape(
            &lang::t("recorder.click_to_set"),
            SMALL,
            f32::MAX,
            Wrap::Line,
        );
        let (w, h) = (text.w, text.h);
        (PillContent::Empty(text), w, h)
    } else {
        let chips: Vec<_> = row
            .keys
            .iter()
            .map(|key| {
                let text = b.shape(key, KEY, f32::MAX, Wrap::Line);
                let chip_w = (text.w + 12.0).max(22.0).ceil();
                let chip_h = text.h + 4.0;
                (text, chip_w, chip_h)
            })
            .collect();
        let w = chips.iter().map(|c| c.1).sum::<f32>() + CHIP_GAP * (chips.len() - 1) as f32;
        let h = chips.iter().map(|c| c.2).fold(0.0, f32::max);
        (PillContent::Keys(chips), w, h)
    };
    PillSize {
        content,
        w: (w + 2.0 * PILL_PAD_X).ceil(),
        h: h + 2.0 * PILL_PAD_Y,
    }
}

fn place_pill_content<S: Shaper>(b: &mut Builder<S>, pill: PillSize<S::Text>, rect: Rect) {
    let x = rect.x + PILL_PAD_X;
    let inner_h = rect.h - 2.0 * PILL_PAD_Y;
    let top = rect.y + PILL_PAD_Y;
    match pill.content {
        PillContent::Prompt(text) => b.text(&text, x, top + (inner_h - text.h) / 2.0, Ink::Accent),
        PillContent::Empty(text) => b.text(&text, x, top + (inner_h - text.h) / 2.0, Ink::Tertiary),
        PillContent::Keys(chips) => {
            let mut cx = x;
            for (text, w, h) in chips {
                let chip = b.snap.rect(Rect::new(cx, top + (inner_h - h) / 2.0, w, h));
                b.items.push(Item::Chip(chip));
                b.text(
                    &text,
                    chip.x + (chip.w - text.w) / 2.0,
                    chip.y + (chip.h - text.h) / 2.0,
                    Ink::Foreground,
                );
                cx += w + CHIP_GAP;
            }
        }
    }
}

/// What sits at the right end of a setting row.
enum Trailing {
    Nothing,
    Language,
    Switch(Target, bool),
    UpdateButtons,
}

const COMBO_W: f32 = 190.0;
const COMBO_H: f32 = 32.0;

fn general_page<S: Shaper>(b: &mut Builder<S>, model: &Model, width: f32) -> f32 {
    let x = PAGE_PADDING;
    let w = (width - 2.0 * PAGE_PADDING).max(0.0);
    let mut y = 14.0;

    y = b.card(x, y, w, |b, x, y, w| {
        y + setting_row(
            b,
            model,
            x,
            y,
            w,
            &lang::t("general.language"),
            &lang::t("general.language_desc"),
            Trailing::Language,
        )
    });

    y = b.section_title("group.window_sizes", x, y + 16.0, w);
    y = b.card(x, y, w, |b, x, mut y, w| {
        y += setting_row(
            b,
            model,
            x,
            y,
            w,
            &lang::t("general.size_cycle"),
            &lang::t("general.size_cycle_desc"),
            Trailing::Nothing,
        );
        y + size_checks(b, model, x, y)
    });

    y = b.section_title("group.startup", x, y + 16.0, w);
    y = b.card(x, y, w, |b, x, y, w| {
        y + setting_row(
            b,
            model,
            x,
            y,
            w,
            &lang::t("general.startup"),
            &lang::t("general.startup_desc"),
            Trailing::Switch(Target::Startup, model.startup),
        )
    });

    y = b.section_title("group.updates", x, y + 16.0, w);
    y = b.card(x, y, w, |b, x, mut y, w| {
        y += setting_row(
            b,
            model,
            x,
            y,
            w,
            &lang::t("general.auto_update"),
            &lang::t("general.auto_update_desc"),
            Trailing::Switch(Target::AutoUpdate, model.auto_update),
        );
        y = b.divider(x, y, w);
        y + setting_row(
            b,
            model,
            x,
            y,
            w,
            &model.update_status,
            "",
            Trailing::UpdateButtons,
        )
    });

    y + 20.0
}

/// A title, an optional description and a trailing control; returns the
/// row height.
#[allow(clippy::too_many_arguments)]
fn setting_row<S: Shaper>(
    b: &mut Builder<S>,
    model: &Model,
    x: f32,
    y: f32,
    w: f32,
    title: &str,
    description: &str,
    trailing: Trailing,
) -> f32 {
    let switch_label = match trailing {
        Trailing::Switch(_, on) => {
            let key = if on { "switch.on" } else { "switch.off" };
            Some(b.shape(&lang::t(key), BODY, f32::MAX, Wrap::Line))
        }
        _ => None,
    };
    let download = (matches!(trailing, Trailing::UpdateButtons) && model.update_available)
        .then(|| b.button("button.download"));
    let check_now =
        matches!(trailing, Trailing::UpdateButtons).then(|| b.button("button.check_now"));

    let (trailing_w, trailing_h) = match trailing {
        Trailing::Nothing => (0.0, 0.0),
        Trailing::Language => (COMBO_W, COMBO_H),
        Trailing::Switch(..) => {
            let label = switch_label.as_ref().map_or((0.0, 0.0), |l| (l.w, l.h));
            ((40.0 + 12.0 + label.0).ceil(), label.1.max(20.0))
        }
        Trailing::UpdateButtons => {
            let check = check_now.as_ref().map_or((0.0, 0.0), |c| (c.w, c.h));
            match &download {
                Some(d) => (d.w + 8.0 + check.0, d.h.max(check.1)),
                None => check,
            }
        }
    };

    let text_w = w - 32.0 - 16.0 - trailing_w;
    let title = b.shape(title, SETTING, text_w, Wrap::Words);
    let description =
        (!description.is_empty()).then(|| b.shape(description, BODY, text_w, Wrap::Words));
    let text_h = title.h + description.as_ref().map_or(0.0, |d| 2.0 + d.h);
    let h = b.snap.round((text_h.max(trailing_h) + 18.0).max(58.0));

    let text_y = y + (h - text_h) / 2.0;
    b.text(&title, x + 16.0, text_y, Ink::Foreground);
    if let Some(description) = &description {
        b.text(
            description,
            x + 16.0,
            text_y + title.h + 2.0,
            Ink::Secondary,
        );
    }

    let tx = x + w - 16.0 - trailing_w;
    let ty = y + (h - trailing_h) / 2.0;
    match trailing {
        Trailing::Nothing => {}
        Trailing::Language => {
            let rect = b.snap.rect(Rect::new(tx, ty, COMBO_W, COMBO_H));
            let name = lang::LANGUAGES
                .get(model.language)
                .map_or("", |language| language.name);
            let label = b.shape(name, BODY, COMBO_W - 22.0 - 8.0 - 12.0, Wrap::Ellipsis);
            b.items.push(Item::Combo {
                rect,
                label: label.text,
                label_x: rect.x + 11.0,
                label_y: rect.y + (rect.h - label.h) / 2.0,
            });
            b.hit(Target::Language, rect, true);
        }
        Trailing::Switch(target, on) => {
            let label = switch_label.expect("switch label");
            let rect = b.snap.rect(Rect::new(tx, ty, trailing_w, trailing_h));
            let rail = b.snap.rect(Rect::new(
                rect.x,
                rect.y + (rect.h - 20.0) / 2.0,
                40.0,
                20.0,
            ));
            b.items.push(Item::Switch {
                target,
                rail,
                on,
                label_x: rail.right() + 12.0,
                label_y: rect.y + (rect.h - label.h) / 2.0,
                label: label.text,
            });
            b.hit(target, rect, true);
        }
        Trailing::UpdateButtons => {
            let mut bx = tx;
            if let Some(download) = &download {
                b.place_button(download, Target::Download, bx, ty, true, true);
                bx += download.w + 8.0;
            }
            if let Some(check) = &check_now {
                b.place_button(check, Target::CheckNow, bx, ty, false, !model.update_busy);
            }
        }
    }
    h
}

/// The size cycle check boxes under their setting row; returns the height.
fn size_checks<S: Shaper>(b: &mut Builder<S>, model: &Model, x: f32, y: f32) -> f32 {
    let measured: Vec<_> = model
        .sizes
        .iter()
        .map(|option| b.shape(&option.label, BODY, f32::MAX, Wrap::Line))
        .collect();
    let h = measured.iter().map(|l| l.h).fold(18.0, f32::max);
    let mut cx = x + 16.0;
    for (index, (option, label)) in model.sizes.iter().zip(measured).enumerate() {
        let w = (18.0 + 12.0 + label.w).ceil();
        let rect = b.snap.rect(Rect::new(cx, y, w, h));
        let mark = b.snap.rect(Rect::new(cx, y + (h - 18.0) / 2.0, 18.0, 18.0));
        b.items.push(Item::Check {
            index,
            rect,
            mark,
            checked: option.checked,
            enabled: !option.locked,
            label_x: mark.right() + 12.0,
            label_y: y + (h - label.h) / 2.0,
            label: label.text,
        });
        b.hit(Target::Size(index), rect, !option.locked);
        cx += w + 20.0;
    }
    h + 12.0
}

enum Block<T> {
    Logo,
    Space(f32),
    Centered(Shaped<T>, Ink),
    Buttons(Vec<(ButtonSize<T>, Target, bool, bool)>),
    Links(Option<Shaped<T>>, Shaped<T>, Link),
}

const LOGO_SIZE: f32 = 128.0;
const ABOUT_SPACING: f32 = 6.0;

fn about_page<S: Shaper>(b: &mut Builder<S>, model: &Model, width: f32, viewport_h: f32) -> f32 {
    let w = (width - 2.0 * PAGE_PADDING).max(0.0);
    let mut blocks = vec![
        Block::Logo,
        Block::Space(8.0),
        Block::Centered(
            b.shape("Wrecktangle", APP_NAME, f32::MAX, Wrap::Line),
            Ink::Foreground,
        ),
        Block::Centered(
            b.shape(&model.about_version, BODY, f32::MAX, Wrap::Line),
            Ink::Secondary,
        ),
        Block::Space(16.0),
    ];
    let mut buttons = vec![(
        b.button("about.check"),
        Target::CheckNow,
        false,
        !model.update_busy,
    )];
    if model.update_available {
        buttons.push((b.button("button.download"), Target::Download, true, true));
    }
    blocks.push(Block::Buttons(buttons));
    if !model.update_detail.is_empty() {
        blocks.push(Block::Centered(
            b.shape(&model.update_detail, BODY, w, Wrap::WordsCentered),
            Ink::Secondary,
        ));
    }
    blocks.push(Block::Space(16.0));
    blocks.push(Block::Links(
        None,
        b.shape(&lang::t("about.website"), LABEL, f32::MAX, Wrap::Line),
        Link::Website,
    ));
    blocks.push(Block::Space(6.0));
    for (key, name, link) in [
        ("about.made_by", "zolfer.com", Link::Author),
        ("about.inspired_by", "Rectangle", Link::Rectangle),
    ] {
        let lead = b.shape(&lang::t(key), LABEL, f32::MAX, Wrap::Line);
        let text = b.shape(name, LABEL, f32::MAX, Wrap::Line);
        blocks.push(Block::Links(Some(lead), text, link));
    }

    let block_h = |block: &Block<S::Text>| match block {
        Block::Logo => LOGO_SIZE,
        Block::Space(h) => *h,
        Block::Centered(text, _) => text.h,
        Block::Buttons(buttons) => buttons.iter().map(|b| b.0.h).fold(0.0, f32::max),
        Block::Links(lead, text, _) => lead.as_ref().map_or(text.h, |l| l.h.max(text.h)),
    };
    let total = 2.0 * PAGE_PADDING
        + blocks.iter().map(block_h).sum::<f32>()
        + ABOUT_SPACING * (blocks.len() - 1) as f32;
    let content_h = total.max(viewport_h);
    let mut y = b.snap.round((content_h - total) / 2.0 + PAGE_PADDING);

    for block in blocks {
        let h = block_h(&block);
        match block {
            Block::Logo => {
                b.items.push(Item::Logo(b.snap.rect(Rect::new(
                    (width - LOGO_SIZE) / 2.0,
                    y,
                    LOGO_SIZE,
                    LOGO_SIZE,
                ))));
            }
            Block::Space(_) => {}
            Block::Centered(text, ink) => {
                let x = if text.w >= w {
                    PAGE_PADDING
                } else {
                    (width - text.w) / 2.0
                };
                b.text(&text, x, y, ink);
            }
            Block::Buttons(buttons) => {
                let total_w =
                    buttons.iter().map(|b| b.0.w).sum::<f32>() + 8.0 * (buttons.len() - 1) as f32;
                let mut x = (width - total_w) / 2.0;
                for (size, target, primary, enabled) in buttons {
                    b.place_button(&size, target, x, y, primary, enabled);
                    x += size.w + 8.0;
                }
            }
            Block::Links(lead, text, link) => {
                let lead_w = lead.as_ref().map_or(0.0, |l| l.w + 4.0);
                let mut x = (width - lead_w - text.w) / 2.0;
                if let Some(lead) = &lead {
                    b.text(lead, x, y, Ink::Secondary);
                    x += lead_w;
                }
                let rect = Rect::new(x, y, text.w, text.h);
                b.items.push(Item::Link {
                    link,
                    rect,
                    text: text.text,
                });
                b.hit(Target::Link(link), rect, true);
            }
        }
        y = b.snap.round(y + h + ABOUT_SPACING);
    }
    content_h
}

/// Hover, press and focus state, which only changes how things are painted.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Interaction {
    pub hover: Option<Target>,
    pub pressed: Option<Target>,
    pub focus: Option<Target>,
    /// Focus rings show only after keyboard use, while the window is active.
    pub focus_visible: bool,
    pub bar_hover: bool,
    pub bar_drag: bool,
}

impl Interaction {
    fn hovered(&self, target: Target) -> bool {
        self.hover == Some(target)
    }

    fn pressed(&self, target: Target) -> bool {
        self.pressed == Some(target) && self.hover == Some(target)
    }

    fn focused(&self, target: Target) -> bool {
        self.focus_visible && self.focus == Some(target)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op<T> {
    Clear(Rgba),
    Fill {
        rect: Rect,
        radius: f32,
        color: Rgba,
    },
    /// A border drawn inside `rect`.
    Stroke {
        rect: Rect,
        radius: f32,
        width: f32,
        color: Rgba,
    },
    Text {
        text: T,
        x: f32,
        y: f32,
        color: Rgba,
    },
    /// Open polylines stroked with round caps and joins as one shape.
    Lines {
        figures: Vec<Vec<(f32, f32)>>,
        width: f32,
        color: Rgba,
    },
    Logo(Rect),
    Clip(Rect),
    Unclip,
    /// Moves everything after it down by this much (content scrolling).
    Shift(f32),
}

struct Painter<'a, T> {
    ops: Vec<Op<T>>,
    ui: &'a Interaction,
    p: &'a Palette,
    snap: Snap,
}

impl<T: Clone> Painter<'_, T> {
    fn fill(&mut self, rect: Rect, radius: f32, color: Rgba) {
        if color.a > 0 {
            self.ops.push(Op::Fill {
                rect,
                radius,
                color,
            });
        }
    }

    fn stroke(&mut self, rect: Rect, radius: f32, width: f32, color: Rgba) {
        if color.a > 0 && width > 0.0 {
            self.ops.push(Op::Stroke {
                rect,
                radius,
                width,
                color,
            });
        }
    }

    fn text(&mut self, text: &T, x: f32, y: f32, color: Rgba) {
        self.ops.push(Op::Text {
            text: text.clone(),
            x,
            y,
            color,
        });
    }

    /// The Fluent focus border: a 2 px outer ring and a 1 px inner one.
    fn focus_border(&mut self, rect: Rect, radius: f32) {
        self.stroke(rect, radius, 2.0, self.p.focus_outer);
        self.stroke(
            rect.inset(2.0),
            (radius - 2.0).max(0.0),
            1.0,
            self.p.focus_inner,
        );
    }

    fn ink(&self, ink: Ink) -> Rgba {
        match ink {
            Ink::Foreground => self.p.foreground,
            Ink::Secondary => self.p.text_secondary,
            Ink::Tertiary => self.p.text_tertiary,
            Ink::Accent => self.p.accent,
            Ink::Warning => self.p.warning,
        }
    }

    fn item(&mut self, item: &Item<T>) {
        let p = self.p;
        let fg = p.foreground;
        match item {
            Item::Text { text, x, y, ink } => self.text(text, *x, *y, self.ink(*ink)),
            Item::Card(rect) => {
                self.fill(*rect, 6.0, p.control);
                self.stroke(*rect, 6.0, self.snap.hairline(), p.card_border);
            }
            Item::Divider(rect) => self.fill(*rect, 0.0, p.divider),
            Item::Icon { x, y, geometry } => self.icon(*x, *y, geometry),
            Item::Tab {
                page,
                rect,
                label,
                label_rect,
                selected,
            } => {
                let target = Target::Tab(*page);
                let color = if *selected || self.ui.hovered(target) {
                    fg
                } else {
                    p.text_secondary
                };
                self.text(label, label_rect.x, label_rect.y, color);
                if *selected {
                    let bar = Rect::new(label_rect.x, rect.bottom() - 3.0, label_rect.w, 3.0);
                    self.fill(bar, 1.5, p.accent);
                }
                if self.ui.focused(target) {
                    self.stroke(*rect, 4.0, 2.0, p.accent);
                }
            }
            Item::Pill {
                index,
                rect,
                recording,
            } => {
                let target = Target::Pill(*index);
                let (bg, border, width) = if *recording {
                    (p.accent.with_alpha(0.12), p.accent, 2.0)
                } else if self.ui.hovered(target) {
                    (fg.with_alpha(0.09), fg.with_alpha(0.14), 1.0)
                } else {
                    (fg.with_alpha(0.04), fg.with_alpha(0.14), 1.0)
                };
                self.fill(*rect, 5.0, bg);
                self.stroke(*rect, 5.0, width, border);
                if !*recording && self.ui.focused(target) {
                    self.stroke(*rect, 5.0, 2.0, p.accent);
                }
            }
            Item::Chip(rect) => {
                self.fill(*rect, 3.0, fg.with_alpha(0.07));
                self.stroke(*rect, 3.0, 1.0, fg.with_alpha(0.14));
            }
            Item::Clear { index, rect } => {
                let target = Target::Clear(*index);
                if self.ui.hovered(target) {
                    self.fill(*rect, 4.0, fg.with_alpha(0.1));
                }
                let x = rect.x + (rect.w - 8.0) / 2.0;
                let y = rect.y + (rect.h - 8.0) / 2.0;
                self.ops.push(Op::Lines {
                    figures: vec![
                        vec![(x + 1.0, y + 1.0), (x + 7.0, y + 7.0)],
                        vec![(x + 7.0, y + 1.0), (x + 1.0, y + 7.0)],
                    ],
                    width: 1.3,
                    color: p.text_secondary,
                });
                if self.ui.focused(target) {
                    self.stroke(*rect, 4.0, 2.0, p.accent);
                }
            }
            Item::Button {
                target,
                rect,
                label,
                label_x,
                label_y,
                primary,
                enabled,
            } => self.button(
                *target, *rect, label, *label_x, *label_y, *primary, *enabled,
            ),
            Item::Switch {
                target,
                rail,
                on,
                label,
                label_x,
                label_y,
            } => {
                self.switch(*target, *rail, *on);
                self.text(label, *label_x, *label_y, fg);
            }
            Item::Check {
                index,
                rect,
                mark,
                checked,
                enabled,
                label,
                label_x,
                label_y,
            } => self.check(
                *index, *rect, *mark, *checked, *enabled, label, *label_x, *label_y,
            ),
            Item::Combo {
                rect,
                label,
                label_x,
                label_y,
            } => {
                let pressed = self.ui.pressed(Target::Language);
                let (bg, text, chevron) = if pressed {
                    (
                        p.control_alt_hover,
                        p.fluent_text_secondary,
                        p.fluent_text_tertiary,
                    )
                } else if self.ui.hovered(Target::Language) {
                    (p.control_hover, fg, p.fluent_text_secondary)
                } else {
                    (p.control, fg, p.fluent_text_secondary)
                };
                self.fill(*rect, 3.0, bg);
                self.stroke(*rect, 3.0, 1.0, p.control_border);
                self.text(label, *label_x, *label_y, text);
                let cx = rect.right() - 11.0 - 12.0;
                let cy = rect.y + (rect.h - 6.0) / 2.0;
                self.ops.push(Op::Lines {
                    figures: vec![vec![(cx + 0.5, cy), (cx + 6.0, cy + 5.5), (cx + 11.5, cy)]],
                    width: 1.0,
                    color: chevron,
                });
                if self.ui.focused(Target::Language) {
                    self.focus_border(*rect, 3.0);
                }
            }
            Item::Link { link, rect, text } => {
                let target = Target::Link(*link);
                self.text(text, rect.x, rect.y, p.accent);
                if self.ui.hovered(target) || self.ui.focused(target) {
                    self.fill(
                        Rect::new(rect.x, rect.bottom() - 2.0, rect.w, 1.0),
                        0.0,
                        p.accent,
                    );
                }
            }
            Item::Logo(rect) => self.ops.push(Op::Logo(*rect)),
        }
    }

    fn icon(&mut self, x: f32, y: f32, g: &IconGeometry) {
        let s = self.snap.scale;
        let fg = self.p.foreground;
        let frame = Rect::new(x, y, g.w / s, g.h / s);
        self.fill(frame, g.radius / s, fg.with_alpha(0.04));
        self.stroke(frame, g.radius / s, g.stroke / s, fg.with_alpha(0.5));
        match g.shape {
            IconShape::Fill(r) => {
                let rect = Rect::new(x + r.x / s, y + r.y / s, r.w / s, r.h / s);
                self.fill(rect, g.glyph_radius / s, self.p.accent);
            }
            IconShape::Chevron(points) => self.ops.push(Op::Lines {
                figures: vec![
                    points
                        .iter()
                        .map(|&(px, py)| (x + px / s, y + py / s))
                        .collect(),
                ],
                width: g.stroke / s,
                color: self.p.accent,
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn button(
        &mut self,
        target: Target,
        rect: Rect,
        label: &T,
        lx: f32,
        ly: f32,
        primary: bool,
        enabled: bool,
    ) {
        let p = self.p;
        let (bg, border, text) = if !enabled {
            if primary {
                (p.accent_disabled, Rgba::TRANSPARENT, p.accent_text_disabled)
            } else {
                (p.control_disabled, p.control_border, p.text_disabled)
            }
        } else if self.ui.pressed(target) {
            if primary {
                (
                    p.accent.with_alpha(0.8),
                    Rgba::TRANSPARENT,
                    p.accent_text_pressed,
                )
            } else {
                (p.control_pressed, p.control_border, p.fluent_text_secondary)
            }
        } else if primary {
            let bg = if self.ui.hovered(target) {
                p.accent.with_alpha(0.9)
            } else {
                p.accent
            };
            (bg, Rgba::TRANSPARENT, p.accent_foreground)
        } else {
            let bg = if self.ui.hovered(target) {
                p.control_hover
            } else {
                p.control
            };
            (bg, p.control_border, p.foreground)
        };
        self.fill(rect, 4.0, bg);
        self.stroke(rect, 4.0, 1.0, border);
        self.text(label, lx, ly, text);
        if enabled && self.ui.focused(target) {
            self.focus_border(rect, 4.0);
        }
    }

    fn switch(&mut self, target: Target, rail: Rect, on: bool) {
        let p = self.p;
        let pressed = self.ui.pressed(target);
        let hovered = self.ui.hovered(target);
        let (thumb_w, thumb_h) = if pressed {
            (17.0, 14.0)
        } else if hovered {
            (14.0, 14.0)
        } else {
            (12.0, 12.0)
        };
        let thumb_x = if on {
            rail.right() - thumb_w - 4.0
        } else {
            rail.x + 4.0
        };
        let thumb = Rect::new(thumb_x, rail.y + (rail.h - thumb_h) / 2.0, thumb_w, thumb_h);
        if on {
            let rail_bg = if pressed {
                p.accent.with_alpha(0.8)
            } else if hovered {
                p.accent.with_alpha(0.9)
            } else {
                p.accent
            };
            self.fill(rail, 10.0, rail_bg);
            self.fill(thumb, thumb_h / 2.0, p.accent_foreground);
            self.stroke(thumb, thumb_h / 2.0, 1.0, p.circle_border);
        } else {
            let rail_bg = if pressed {
                p.control_alt_pressed
            } else if hovered {
                p.control_alt_hover
            } else {
                p.control_alt
            };
            self.fill(rail, 10.0, rail_bg);
            self.stroke(rail, 10.0, 1.0, p.strong_stroke);
            self.fill(thumb, thumb_h / 2.0, p.fluent_text_secondary);
        }
        if self.ui.focused(target) {
            self.focus_border(rail, 10.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check(
        &mut self,
        index: usize,
        rect: Rect,
        mark: Rect,
        checked: bool,
        enabled: bool,
        label: &T,
        lx: f32,
        ly: f32,
    ) {
        let p = self.p;
        let target = Target::Size(index);
        let (bg, border, tick, text) = if !enabled {
            let bg = if checked {
                p.accent_disabled
            } else {
                Rgba::TRANSPARENT
            };
            (
                bg,
                p.strong_stroke_disabled,
                p.accent_text_disabled,
                p.text_disabled,
            )
        } else if self.ui.pressed(target) {
            let bg = if checked {
                p.accent.with_alpha(0.8)
            } else {
                p.control_alt_pressed
            };
            (
                bg,
                p.strong_stroke_disabled,
                p.accent_text_pressed,
                p.foreground,
            )
        } else {
            let bg = match (checked, self.ui.hovered(target)) {
                (true, true) => p.accent.with_alpha(0.9),
                (true, false) => p.accent,
                (false, true) => p.control_alt_hover,
                (false, false) => p.control_alt,
            };
            (bg, p.strong_stroke, p.accent_foreground, p.foreground)
        };
        self.fill(mark, 2.0, bg);
        if checked {
            let k = 12.0 / 11.0;
            let ox = mark.x + (mark.w - 12.0) / 2.0;
            let oy = mark.y + (mark.h - 8.0 * k) / 2.0;
            let at = |x: f32, y: f32| (ox + x * k, oy + y * k);
            self.ops.push(Op::Lines {
                figures: vec![vec![at(0.5, 4.0), at(3.5, 7.0), at(9.5, 1.0)]],
                width: k,
                color: tick,
            });
        } else {
            self.stroke(mark, 2.0, 1.0, border);
        }
        self.text(label, lx, ly, text);
        if enabled && self.ui.focused(target) {
            self.focus_border(rect, 4.0);
        }
    }
}

/// The drawing operations for one frame.
pub fn paint<T: Clone>(
    layout: &Layout<T>,
    ui: &Interaction,
    scroll: &Scroll,
    palette: &Palette,
    snap: Snap,
) -> Vec<Op<T>> {
    let mut painter = Painter {
        ops: vec![Op::Clear(palette.background)],
        ui,
        p: palette,
        snap,
    };
    for item in &layout.header {
        painter.item(item);
    }
    painter.ops.push(Op::Clip(layout.viewport));
    painter
        .ops
        .push(Op::Shift(layout.viewport.y - snap.round(scroll.offset)));
    for item in &layout.content {
        painter.item(item);
    }
    painter.ops.push(Op::Shift(0.0));
    if let Some(bar) = scroll.bar(layout.viewport) {
        let wide = ui.bar_hover || ui.bar_drag;
        if wide {
            painter.fill(bar, 7.0, palette.scrollbar_track);
        }
        if let Some(thumb) = scroll.thumb(layout.viewport, wide) {
            painter.fill(thumb, thumb.w / 2.0, palette.scrollbar_thumb);
        }
    }
    painter.ops.push(Op::Unclip);
    painter.ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Mode;
    use crate::ui::palette::palette;

    /// Monospaced stand-in for DirectWrite: half an em per character and a
    /// 1.33 em line.
    struct FakeShaper;

    impl Shaper for FakeShaper {
        type Text = String;

        fn shape(
            &mut self,
            text: &str,
            style: TextStyle,
            max_width: f32,
            wrap: Wrap,
        ) -> (String, f32, f32) {
            let natural = text.chars().count() as f32 * style.size * 0.5;
            let line = (style.size * 1.33).round();
            match wrap {
                Wrap::Line => (text.to_string(), natural, line),
                Wrap::Ellipsis => (text.to_string(), natural.min(max_width), line),
                Wrap::Words | Wrap::WordsCentered => {
                    let lines = (natural / max_width.max(1.0)).ceil().max(1.0);
                    let w = if wrap == Wrap::WordsCentered {
                        max_width
                    } else {
                        natural.min(max_width)
                    };
                    (text.to_string(), w, lines * line)
                }
            }
        }
    }

    fn model() -> Model {
        Model {
            rows: (0..12)
                .map(|i| Row {
                    label: format!("Action {i}"),
                    keys: vec!["Ctrl".into(), "Alt".into(), "\u{2190}".into()],
                    ..Row::default()
                })
                .collect(),
            sizes: ["1/2", "2/3", "3/4", "1/4", "1/3"]
                .iter()
                .enumerate()
                .map(|(i, label)| SizeOption {
                    label: label.to_string(),
                    checked: i == 0,
                    locked: i == 0,
                })
                .collect(),
            update_status: "Version 1.0.0".into(),
            about_version: "Version 1.0.0".into(),
            ..Model::default()
        }
    }

    fn lay(model: &Model) -> Layout<String> {
        layout(model, &mut FakeShaper, 640.0, 682.0, Snap::new(1.0))
    }

    fn row_heights(layout: &Layout<String>) -> Vec<f32> {
        layout
            .content
            .iter()
            .filter_map(|item| match item {
                Item::Divider(r) => Some(r.y),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn tabs_come_first_in_focus_order() {
        let order = lay(&model()).focus_order();
        assert_eq!(order[0].0, Target::Tab(Page::Shortcuts));
        assert_eq!(order[1].0, Target::Tab(Page::General));
        assert_eq!(order[2].0, Target::Tab(Page::About));
        assert_eq!(order[3].0, Target::Pill(0));
        assert_eq!(order[4].0, Target::Clear(0));
        assert_eq!(order.last().map(|o| o.0), Some(Target::RestoreDefaults));
    }

    #[test]
    fn shortcut_rows_are_36_or_40_with_a_note() {
        let mut m = model();
        let plain = row_heights(&lay(&m));
        // Halves: dividers sit after rows 0, 1 and 2.
        assert_eq!(plain[1] - plain[0], 36.0 + 1.0);
        m.rows[1].note = "Overrides something".into();
        let noted = row_heights(&lay(&m));
        assert_eq!(noted[1] - noted[0], 40.0 + 1.0);
    }

    #[test]
    fn recording_row_shows_the_prompt_and_no_clear_button() {
        let mut m = model();
        m.rows[2].recording = true;
        let layout = lay(&m);
        assert!(!layout.has_target(Target::Clear(2)));
        assert!(layout.has_target(Target::Clear(3)));
        assert!(layout.content.iter().any(|item| matches!(
            item,
            Item::Pill {
                index: 2,
                recording: true,
                ..
            }
        )));
        let prompt = lang::t("recorder.prompt");
        assert!(layout.content.iter().any(|item| matches!(
            item,
            Item::Text { text, ink: Ink::Accent, .. } if *text == prompt
        )));
    }

    fn overlap(a: &Rect, b: &Rect) -> bool {
        a.x < b.right() && b.x < a.right() && a.y < b.bottom() && b.y < a.bottom()
    }

    #[test]
    fn content_targets_do_not_overlap() {
        for page in PAGES {
            let mut m = model();
            m.page = page;
            m.update_available = true;
            let layout = lay(&m);
            for (i, a) in layout.content_hits.iter().enumerate() {
                for b in &layout.content_hits[i + 1..] {
                    assert!(!overlap(&a.rect, &b.rect), "{page:?}: {a:?} and {b:?}");
                }
            }
        }
    }

    #[test]
    fn download_shows_only_when_an_update_is_available() {
        for page in [Page::General, Page::About] {
            let mut m = model();
            m.page = page;
            assert!(!lay(&m).has_target(Target::Download));
            m.update_available = true;
            assert!(lay(&m).has_target(Target::Download));
        }
    }

    #[test]
    fn locked_size_and_busy_check_are_not_focusable() {
        let mut m = model();
        m.page = Page::General;
        m.update_busy = true;
        let order = lay(&m).focus_order();
        assert!(order.contains(&(Target::Size(0), false)));
        assert!(order.contains(&(Target::Size(1), true)));
        assert!(order.contains(&(Target::CheckNow, false)));
    }

    #[test]
    fn about_page_centers_and_links_its_credits() {
        let mut m = model();
        m.page = Page::About;
        let layout = lay(&m);
        assert_eq!(layout.content_height, layout.viewport.h);
        for link in [Link::Website, Link::Author, Link::Rectangle] {
            assert!(layout.has_target(Target::Link(link)));
        }
        assert!(!layout.content.iter().any(|item| matches!(
            item,
            Item::Text { text, .. } | Item::Link { text, .. } if text.contains("Slint")
        )));
    }

    #[test]
    fn hit_maps_window_points_through_the_scroll_offset() {
        let mut m = model();
        m.page = Page::General;
        let layout = lay(&m);
        let combo = layout.content_rect(Target::Language).expect("combo");
        let (x, y) = (combo.x + 5.0, combo.y + 5.0);
        assert_eq!(
            layout.hit(x, layout.viewport.y + y, 0.0),
            Some(Target::Language)
        );
        assert_eq!(
            layout.hit(x, layout.viewport.y + y - 30.0, 30.0),
            Some(Target::Language)
        );
        assert_eq!(
            layout.hit(20.0, 20.0, 0.0),
            Some(Target::Tab(Page::Shortcuts))
        );
        assert_eq!(layout.hit(-5.0, -5.0, 0.0), None);
        let locked = layout.content_rect(Target::Size(0)).expect("size");
        assert_eq!(
            layout.hit(locked.x + 1.0, layout.viewport.y + locked.y + 1.0, 0.0),
            None
        );
    }

    fn has_focus_ring(ops: &[Op<String>], palette: &Palette) -> bool {
        ops.iter().any(|op| match op {
            Op::Stroke { color, width, .. } => {
                *width == 2.0 && (*color == palette.focus_outer || *color == palette.accent)
            }
            _ => false,
        })
    }

    #[test]
    fn focus_rings_need_keyboard_cues() {
        let m = model();
        let layout = lay(&m);
        let palette = palette(Mode::Dark);
        let scroll = Scroll::default();
        let mut ui = Interaction {
            focus: Some(Target::Pill(0)),
            ..Interaction::default()
        };
        assert!(!has_focus_ring(
            &paint(&layout, &ui, &scroll, &palette, Snap::new(1.0)),
            &palette
        ));
        ui.focus_visible = true;
        assert!(has_focus_ring(
            &paint(&layout, &ui, &scroll, &palette, Snap::new(1.0)),
            &palette
        ));
    }

    #[test]
    fn paint_balances_clip_and_shift() {
        let mut m = model();
        m.page = Page::General;
        let layout = layout(&m, &mut FakeShaper, 480.0, 320.0, Snap::new(1.5));
        let scroll = Scroll {
            offset: 10.0,
            content: layout.content_height,
            viewport: layout.viewport.h,
        };
        let ops = paint(
            &layout,
            &Interaction::default(),
            &scroll,
            &palette(Mode::Light),
            Snap::new(1.5),
        );
        assert!(matches!(ops[0], Op::Clear(_)));
        let clips = ops.iter().filter(|op| matches!(op, Op::Clip(_))).count();
        let unclips = ops.iter().filter(|op| matches!(op, Op::Unclip)).count();
        assert_eq!(clips, 1);
        assert_eq!(unclips, 1);
        assert!(matches!(ops.last(), Some(Op::Unclip)));
        assert!(layout.content_height > layout.viewport.h);
    }
}
