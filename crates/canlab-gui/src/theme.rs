//! Whole colour palettes, not one accent colour.
//!
//! An accent alone is a sticker on someone else's theme: it changes the caret and
//! the selection and leaves the surfaces, the text and the plot's curve wheel
//! exactly where they were. A palette here is a *scheme* — window, panel, raised
//! and sunken surfaces, dividers, text and weak text, the accent, the warning and
//! error colours, and the six colours the plot cycles through — so choosing one
//! changes the whole window coherently.
//!
//! # Why a palette pins light or dark
//!
//! Each scheme is designed for one background. "Solarized Dark" on a white
//! window is not a variation of Solarized, it is a mistake; keeping a separate
//! light/dark toggle would let anyone make it. So the palette *is* the mode, and
//! the Settings menu lists schemes rather than a mode and a sticker.
//!
//! # The one rule
//!
//! The mathematics is the only saturated thing on screen. Every palette keeps its
//! surfaces within a narrow band of near-neutrals and spends its saturation on
//! the accent; the plot is the one place with six colours, and that is because a
//! system of equations needs to be told apart.

use egui::{self, Color32, CornerRadius, FontId, Margin, Stroke, TextStyle, Visuals, vec2};

/// Add a system font that covers the symbols the palette draws.
///
/// egui bundles Ubuntu-Light and its own icon font, and neither has `ϑ`, `∢`,
/// `≢`, `∝` or the typographic ellipsis — so a palette of mathematics renders as
/// boxes. The headless frame test found this by panicking on the first missing
/// glyph; a reader would have found it by staring at `□`.
///
/// The font is read from the operating system rather than bundled, because a
/// redistribution of Segoe UI Symbol is not ours to make. If the file is not
/// there the app still runs, with the same boxes it had before, and that is the
/// only behaviour this function is allowed to promise.
fn install_symbol_fallback(ctx: &egui::Context) {
    // Windows first, then the usual Linux paths. The Linux entries are not
    // decoration: without them the fallback covers nothing off Windows, the
    // headless build server renders every symbol as tofu, and the frames in
    // `tests/snapshots/` cannot be compared on the machine that builds them.
    // `tests` in `snapshot.rs` asks the font system whether each character the
    // app prints has a glyph, which is how this list is known to be enough.
    const CANDIDATES: [(&str, &str); 6] = [
        ("seguisym", r"C:\Windows\Fonts\seguisym.ttf"),
        ("segoeui", r"C:\Windows\Fonts\segoeui.ttf"),
        ("dejavu", r"C:\Windows\Fonts\DejaVuSans.ttf"),
        ("linux-dejavu", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        ("linux-noto-symbols", "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf"),
        ("linux-noto-symbols2", "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf"),
    ];
    let mut fonts = egui::FontDefinitions::default();
    let mut added = false;
    for (name, path) in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        fonts.font_data.insert(
            String::from(name),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        // Last in the chain: the bundled fonts stay primary, so the typography
        // does not change, and only the glyphs they lack come from here.
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push(String::from(name));
        }
        added = true;
    }
    if added {
        ctx.set_fonts(fonts);
    }
}

/// Which background a palette is designed for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Dark surfaces, light text.
    #[default]
    Dark,
    /// Light surfaces, dark text.
    Light,
}

impl Mode {
    /// The word the Settings menu shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// The egui theme this mode installs under.
    pub fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        }
    }
}

/// One complete colour scheme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// The name the Settings menu shows.
    pub name: &'static str,
    /// Which background it is designed for.
    pub mode: Mode,
    /// The shell behind the dock: the menu and status bars.
    pub window: Color32,
    /// A dock panel's background — where the mathematics sits.
    pub panel: Color32,
    /// Buttons, palette keys, and anything that should stand off the panel.
    pub raised: Color32,
    /// Inputs, the plot's backdrop, and the deepest surface.
    pub sunken: Color32,
    /// Faint bands: panel headers, alternating rows.
    pub faint: Color32,
    /// Dividers and outlines.
    pub line: Color32,
    /// Body text.
    pub text: Color32,
    /// Secondary text: captions, units, hints.
    pub weak: Color32,
    /// The one saturated colour: caret, selection, current line, first curve.
    pub accent: Color32,
    /// Links and the "solver is thinking" hint.
    pub link: Color32,
    /// A warning that is not an error.
    pub warn: Color32,
    /// Something failed.
    pub error: Color32,
    /// The plot's curve wheel; the first entry is normally the accent.
    pub curves: [Color32; 6],
}

impl Palette {
    /// A colour for the `index`-th curve, cycling through the wheel.
    pub fn curve(&self, index: usize) -> Color32 {
        self.curves[index % self.curves.len()]
    }

    /// True for a scheme designed for a dark background.
    pub fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }

    /// A slightly lifted version of a surface, for hover states.
    ///
    /// Derived rather than listed: a hover state that does not follow its surface
    /// is how a theme ends up with one widget that looks like it belongs to a
    /// different theme.
    fn hover(&self, surface: Color32) -> Color32 {
        blend(surface, self.text, 0.12)
    }
}

/// The schemes, in the order the Settings menu offers them. The first is the
/// default.
pub const PALETTES: [Palette; 7] = [
    Palette {
        name: "Midnight",
        mode: Mode::Dark,
        window: Color32::from_rgb(0x1e, 0x1e, 0x1e),
        panel: Color32::from_rgb(0x23, 0x23, 0x23),
        raised: Color32::from_rgb(0x3a, 0x3a, 0x3a),
        sunken: Color32::from_rgb(0x18, 0x18, 0x18),
        faint: Color32::from_rgb(0x2b, 0x2b, 0x2b),
        line: Color32::from_rgb(0x3a, 0x3a, 0x3a),
        text: Color32::from_rgb(0xdc, 0xdc, 0xdc),
        weak: Color32::from_rgb(0x8c, 0x8c, 0x8c),
        accent: Color32::from_rgb(0xe8, 0x7d, 0x0d),
        link: Color32::from_rgb(0x3d, 0x7e, 0xff),
        warn: Color32::from_rgb(0xff, 0xa9, 0x4d),
        error: Color32::from_rgb(0xff, 0x6b, 0x6b),
        curves: [
            Color32::from_rgb(0xe8, 0x7d, 0x0d),
            Color32::from_rgb(0x62, 0xb0, 0xf5),
            Color32::from_rgb(0x8b, 0xc3, 0x4a),
            Color32::from_rgb(0xe5, 0x7a, 0xc8),
            Color32::from_rgb(0xf2, 0xc9, 0x4c),
            Color32::from_rgb(0x7a, 0xe0, 0xd4),
        ],
    },
    Palette {
        name: "Graphite",
        mode: Mode::Dark,
        window: Color32::from_rgb(0x17, 0x18, 0x1a),
        panel: Color32::from_rgb(0x1d, 0x1f, 0x22),
        raised: Color32::from_rgb(0x2a, 0x2d, 0x31),
        sunken: Color32::from_rgb(0x10, 0x11, 0x13),
        faint: Color32::from_rgb(0x23, 0x26, 0x2a),
        line: Color32::from_rgb(0x33, 0x37, 0x3c),
        text: Color32::from_rgb(0xe3, 0xe5, 0xe8),
        weak: Color32::from_rgb(0x9a, 0xa0, 0xa6),
        accent: Color32::from_rgb(0x4d, 0xd0, 0xe1),
        link: Color32::from_rgb(0x64, 0xb5, 0xf6),
        warn: Color32::from_rgb(0xff, 0xb7, 0x4d),
        error: Color32::from_rgb(0xef, 0x53, 0x50),
        curves: [
            Color32::from_rgb(0x4d, 0xd0, 0xe1),
            Color32::from_rgb(0xff, 0xb7, 0x4d),
            Color32::from_rgb(0x81, 0xc7, 0x84),
            Color32::from_rgb(0xf0, 0x62, 0x92),
            Color32::from_rgb(0xba, 0x68, 0xc8),
            Color32::from_rgb(0x90, 0xa4, 0xae),
        ],
    },
    Palette {
        name: "Nord",
        mode: Mode::Dark,
        window: Color32::from_rgb(0x2e, 0x34, 0x40),
        panel: Color32::from_rgb(0x35, 0x3c, 0x4a),
        raised: Color32::from_rgb(0x43, 0x4c, 0x5e),
        sunken: Color32::from_rgb(0x27, 0x2c, 0x36),
        faint: Color32::from_rgb(0x3b, 0x42, 0x52),
        line: Color32::from_rgb(0x4c, 0x56, 0x6a),
        text: Color32::from_rgb(0xec, 0xef, 0xf4),
        weak: Color32::from_rgb(0xa3, 0xaa, 0xb8),
        accent: Color32::from_rgb(0x88, 0xc0, 0xd0),
        link: Color32::from_rgb(0x8f, 0xbc, 0xbb),
        warn: Color32::from_rgb(0xeb, 0xcb, 0x8b),
        error: Color32::from_rgb(0xbf, 0x61, 0x6a),
        curves: [
            Color32::from_rgb(0x88, 0xc0, 0xd0),
            Color32::from_rgb(0x81, 0xa1, 0xc1),
            Color32::from_rgb(0xa3, 0xbe, 0x8c),
            Color32::from_rgb(0xb4, 0x8e, 0xad),
            Color32::from_rgb(0xeb, 0xcb, 0x8b),
            Color32::from_rgb(0xd0, 0x87, 0x70),
        ],
    },
    Palette {
        name: "Solarized Dark",
        mode: Mode::Dark,
        window: Color32::from_rgb(0x00, 0x2b, 0x36),
        panel: Color32::from_rgb(0x07, 0x36, 0x42),
        raised: Color32::from_rgb(0x0e, 0x44, 0x52),
        sunken: Color32::from_rgb(0x00, 0x24, 0x2d),
        faint: Color32::from_rgb(0x0a, 0x3a, 0x47),
        line: Color32::from_rgb(0x18, 0x4e, 0x5c),
        text: Color32::from_rgb(0x93, 0xa1, 0xa1),
        weak: Color32::from_rgb(0x65, 0x7b, 0x83),
        accent: Color32::from_rgb(0xb5, 0x89, 0x00),
        link: Color32::from_rgb(0x26, 0x8b, 0xd2),
        warn: Color32::from_rgb(0xcb, 0x4b, 0x16),
        error: Color32::from_rgb(0xdc, 0x32, 0x2f),
        curves: [
            Color32::from_rgb(0xb5, 0x89, 0x00),
            Color32::from_rgb(0x26, 0x8b, 0xd2),
            Color32::from_rgb(0x85, 0x99, 0x00),
            Color32::from_rgb(0x2a, 0xa1, 0x98),
            Color32::from_rgb(0xd3, 0x36, 0x82),
            Color32::from_rgb(0xcb, 0x4b, 0x16),
        ],
    },
    Palette {
        name: "Contrast",
        mode: Mode::Dark,
        window: Color32::from_rgb(0x00, 0x00, 0x00),
        panel: Color32::from_rgb(0x0a, 0x0a, 0x0a),
        raised: Color32::from_rgb(0x1c, 0x1c, 0x1c),
        sunken: Color32::from_rgb(0x00, 0x00, 0x00),
        faint: Color32::from_rgb(0x14, 0x14, 0x14),
        line: Color32::from_rgb(0x5a, 0x5a, 0x5a),
        text: Color32::from_rgb(0xff, 0xff, 0xff),
        weak: Color32::from_rgb(0xb4, 0xb4, 0xb4),
        accent: Color32::from_rgb(0xff, 0xd4, 0x00),
        link: Color32::from_rgb(0x82, 0xb1, 0xff),
        warn: Color32::from_rgb(0xff, 0xab, 0x40),
        error: Color32::from_rgb(0xff, 0x52, 0x52),
        curves: [
            Color32::from_rgb(0xff, 0xd4, 0x00),
            Color32::from_rgb(0x00, 0xe5, 0xff),
            Color32::from_rgb(0x76, 0xff, 0x03),
            Color32::from_rgb(0xff, 0x40, 0x81),
            Color32::from_rgb(0xe0, 0x40, 0xfb),
            Color32::from_rgb(0xff, 0xff, 0xff),
        ],
    },
    Palette {
        name: "Solstice",
        mode: Mode::Light,
        window: Color32::from_rgb(0xf1, 0xef, 0xea),
        panel: Color32::from_rgb(0xfa, 0xf9, 0xf6),
        raised: Color32::from_rgb(0xe8, 0xe5, 0xde),
        sunken: Color32::from_rgb(0xff, 0xff, 0xff),
        faint: Color32::from_rgb(0xf0, 0xed, 0xe6),
        line: Color32::from_rgb(0xd2, 0xcd, 0xc2),
        text: Color32::from_rgb(0x24, 0x22, 0x1e),
        weak: Color32::from_rgb(0x6d, 0x68, 0x5f),
        accent: Color32::from_rgb(0xc9, 0x64, 0x00),
        link: Color32::from_rgb(0x1b, 0x5c, 0xd6),
        warn: Color32::from_rgb(0xa8, 0x5a, 0x00),
        error: Color32::from_rgb(0xc0, 0x39, 0x2b),
        curves: [
            Color32::from_rgb(0xc9, 0x64, 0x00),
            Color32::from_rgb(0x1b, 0x5c, 0xd6),
            Color32::from_rgb(0x2e, 0x7d, 0x32),
            Color32::from_rgb(0xad, 0x14, 0x57),
            Color32::from_rgb(0x9a, 0x6d, 0x00),
            Color32::from_rgb(0x00, 0x83, 0x8f),
        ],
    },
    Palette {
        name: "Solarized Light",
        mode: Mode::Light,
        window: Color32::from_rgb(0xf3, 0xee, 0xda),
        panel: Color32::from_rgb(0xfd, 0xf6, 0xe3),
        raised: Color32::from_rgb(0xee, 0xe8, 0xd5),
        sunken: Color32::from_rgb(0xff, 0xfb, 0xef),
        faint: Color32::from_rgb(0xf5, 0xf0, 0xde),
        line: Color32::from_rgb(0xd9, 0xd2, 0xbc),
        text: Color32::from_rgb(0x3f, 0x50, 0x54),
        weak: Color32::from_rgb(0x7a, 0x8b, 0x8d),
        accent: Color32::from_rgb(0x26, 0x8b, 0xd2),
        link: Color32::from_rgb(0x1c, 0x6f, 0xa8),
        warn: Color32::from_rgb(0xb1, 0x6d, 0x00),
        error: Color32::from_rgb(0xd8, 0x2b, 0x28),
        curves: [
            Color32::from_rgb(0x26, 0x8b, 0xd2),
            Color32::from_rgb(0x85, 0x99, 0x00),
            Color32::from_rgb(0xb5, 0x89, 0x00),
            Color32::from_rgb(0x2a, 0xa1, 0x98),
            Color32::from_rgb(0xd3, 0x36, 0x82),
            Color32::from_rgb(0xcb, 0x4b, 0x16),
        ],
    },
];

/// The palette in force, for the painting code that has no `CasApp` to ask.
///
/// The dock chrome draws inside `layout.rs`, which is handed rectangles, not the
/// app. One shared index keeps every accent — panel dots, dividers, caret,
/// selection, plot — the same colour instead of half of them following Settings.
static CURRENT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// The palette every widget should paint with right now.
pub fn current() -> &'static Palette {
    let index = CURRENT.load(std::sync::atomic::Ordering::Relaxed) as usize;
    PALETTES.get(index).unwrap_or(&PALETTES[0])
}

/// The accent of the palette in force.
pub fn accent() -> Color32 {
    current().accent
}

/// Secondary text — captions, units, hints — from the palette in force.
///
/// egui derives `weak_text_color` by gamma-multiplying the body colour, which in
/// a light scheme produces text *darker* than the body it is supposed to defer
/// to. A palette lists its own weak colour, so the app asks here instead.
pub fn weak() -> Color32 {
    current().weak
}

/// Body text, from the palette in force.
pub fn text() -> Color32 {
    current().text
}

/// The scrim the plot's controls sit on.
///
/// The panel colour with a little of its opacity removed, rather than a new
/// colour: the grid shows through and the strip still belongs to the scheme it
/// is drawn in. A literal here would look right in one palette and wrong in the
/// other six.
pub fn overlay() -> Color32 {
    // Translucent enough that the grid reads through it, opaque enough that a
    // number on top stays legible.
    const ALPHA: u8 = 236;
    let panel = current().panel;
    Color32::from_rgba_unmultiplied(panel.r(), panel.g(), panel.b(), ALPHA)
}

/// Dividers and outlines, from the palette in force.
pub fn line() -> Color32 {
    current().line
}

/// The `index`-th plot curve colour, from the palette in force.
pub fn curve_color(index: usize) -> Color32 {
    current().curve(index)
}

/// Find a palette by name, for a restored session.
pub fn by_name(name: &str) -> Option<&'static Palette> {
    PALETTES.iter().find(|palette| palette.name == name)
}

/// The closest palette to a light/dark flag and an accent, for a session file
/// written before palettes existed.
pub fn nearest(dark: bool, accent: Color32) -> &'static Palette {
    let distance = |a: Color32, b: Color32| {
        let channel = |x: u8, y: u8| {
            let difference = i32::from(x) - i32::from(y);
            difference * difference
        };
        channel(a.r(), b.r()) + channel(a.g(), b.g()) + channel(a.b(), b.b())
    };
    PALETTES
        .iter()
        .filter(|palette| palette.is_dark() == dark)
        .min_by_key(|palette| distance(palette.accent, accent))
        .unwrap_or(&PALETTES[0])
}

/// Install the fonts, sizes and the default palette.
pub fn install(ctx: &egui::Context) {
    install_symbol_fallback(ctx);
    // Both themes get the typography: a palette pins one of them, but the style
    // store is per theme and a switch must not fall back to egui's defaults.
    for mode in [Mode::Dark, Mode::Light] {
        let theme = mode.egui_theme();
        ctx.style_mut_of(theme, |style| {
            style
                .text_styles
                .insert(TextStyle::Heading, FontId::proportional(26.0));
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(15.0));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(14.5));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(11.5));
            style
                .text_styles
                .insert(TextStyle::Monospace, FontId::monospace(14.0));
            style.spacing.item_spacing = vec2(8.0, 6.0);
            style.spacing.button_padding = vec2(10.0, 4.0);
            style.spacing.menu_margin = Margin::same(8);
            style.spacing.interact_size.y = 26.0;
            style.visuals.window_corner_radius = CornerRadius::same(6);
            style.visuals.menu_corner_radius = CornerRadius::same(6);
        });
    }
    apply(ctx, &PALETTES[0]);
}

/// Wear `palette`.
pub fn apply(ctx: &egui::Context, palette: &'static Palette) {
    CURRENT.store(
        index_of(palette) as u8,
        std::sync::atomic::Ordering::Relaxed,
    );

    let theme = palette.mode.egui_theme();
    let visuals = visuals(palette);
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, visuals.clone());
    ctx.set_visuals(visuals);
    ctx.request_repaint();
}

/// Where `palette` sits in [`PALETTES`].
///
/// By name rather than by address: `PALETTES` is a `const`, so two references to
/// the "same" element are two separate constant promotions and comparing their
/// addresses is not meaningful. Names are unique and checked by a test.
fn index_of(palette: &Palette) -> usize {
    PALETTES
        .iter()
        .position(|candidate| candidate.name == palette.name)
        .unwrap_or(0)
}

/// Build egui's visuals out of a palette.
fn visuals(palette: &Palette) -> Visuals {
    let mut visuals = match palette.mode {
        Mode::Dark => Visuals::dark(),
        Mode::Light => Visuals::light(),
    };

    visuals.window_fill = palette.window;
    visuals.panel_fill = palette.panel;
    visuals.faint_bg_color = palette.faint;
    visuals.extreme_bg_color = palette.sunken;
    visuals.code_bg_color = palette.sunken;
    visuals.text_edit_bg_color = Some(palette.sunken);
    visuals.window_stroke = Stroke::new(1.0, palette.line);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [4, 8],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if palette.is_dark() { 120 } else { 40 }),
    };

    visuals.widgets.noninteractive.bg_fill = palette.panel;
    visuals.widgets.noninteractive.weak_bg_fill = palette.panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, palette.line);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, palette.text);

    visuals.widgets.inactive.bg_fill = palette.raised;
    visuals.widgets.inactive.weak_bg_fill = palette.raised;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, palette.line);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, palette.text);

    let hovered = palette.hover(palette.raised);
    visuals.widgets.hovered.bg_fill = hovered;
    visuals.widgets.hovered.weak_bg_fill = hovered;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, blend(palette.line, palette.text, 0.3));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, palette.text);

    // A pressed or selected widget is *tinted* with the accent, not flooded by
    // it. A solid accent fill forces the text on top to be white, and egui also
    // uses that foreground as `strong_text_color()` — which is how a light theme
    // ends up with white headings on a near-white panel.
    let tint = |amount: f32| blend(palette.panel, palette.accent, amount);
    let active = tint(if palette.is_dark() { 0.30 } else { 0.20 });
    visuals.widgets.active.bg_fill = active;
    visuals.widgets.active.weak_bg_fill = active;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, palette.accent);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, palette.text);

    visuals.selection.bg_fill = tint(0.55);
    visuals.selection.stroke = Stroke::new(1.0, palette.text);
    visuals.hyperlink_color = palette.link;
    visuals.warn_fg_color = palette.warn;
    visuals.error_fg_color = palette.error;
    visuals.override_text_color = None;
    visuals
}

/// Linear blend of two colours, used to derive hover states from surfaces.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| {
        (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_palette_is_a_complete_scheme() {
        for palette in &PALETTES {
            assert!(!palette.name.is_empty());
            // A scheme needs its surfaces to be distinguishable, or it is one
            // colour with extra words.
            assert_ne!(palette.window, palette.panel, "{}", palette.name);
            assert_ne!(palette.panel, palette.raised, "{}", palette.name);
            assert_ne!(palette.panel, palette.sunken, "{}", palette.name);
            assert_ne!(palette.text, palette.weak, "{}", palette.name);
            assert_eq!(palette.curves.len(), 6);
        }
    }

    #[test]
    fn text_reads_against_its_background() {
        // Every palette must clear 4:1 between body text and the panel it sits
        // on, and 3:1 for text on a button. This is the check that catches a
        // light scheme with white headings.
        for palette in &PALETTES {
            let ratio = contrast(palette.text, palette.panel);
            assert!(ratio >= 4.0, "{} has {ratio:.1}:1 text contrast", palette.name);
            let ratio = contrast(palette.weak, palette.panel);
            assert!(
                ratio >= 2.5,
                "{} has {ratio:.1}:1 weak-text contrast",
                palette.name
            );
            let ratio = contrast(palette.accent, palette.panel);
            assert!(
                ratio >= 2.0,
                "{} has {ratio:.1}:1 accent contrast",
                palette.name
            );
            let ratio = contrast(palette.text, palette.raised);
            assert!(
                ratio >= 3.0,
                "{} has {ratio:.1}:1 text-on-widget contrast",
                palette.name
            );
        }
    }

    /// Relative luminance, for contrast ratios.
    fn luminance(colour: Color32) -> f32 {
        let channel = |value: u8| {
            let v = f32::from(value) / 255.0;
            if v <= 0.039_28 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(colour.r()) + 0.7152 * channel(colour.g()) + 0.0722 * channel(colour.b())
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn every_text_style_egui_derives_is_readable() {
        // egui derives `strong_text_color` from the *active widget* foreground and
        // `weak_text_color` from the non-interactive one. A palette that fills
        // widgets with its accent therefore puts white headings on a white panel.
        for palette in &PALETTES {
            let visuals = visuals(palette);
            for (name, colour) in [
                ("text", visuals.text_color()),
                ("strong", visuals.strong_text_color()),
            ] {
                assert_eq!(
                    colour, palette.text,
                    "{}: {name} text drifted from the palette",
                    palette.name
                );
            }
            // The palette's own weak colour is what the app paints with: dimmer
            // than body text, still readable on the panel.
            let weak_ratio = contrast(palette.weak, palette.panel);
            assert!(
                weak_ratio >= 2.5,
                "{} has {weak_ratio:.1}:1 weak contrast",
                palette.name
            );
            assert!(
                weak_ratio < contrast(palette.text, palette.panel),
                "{}: weak text is not dimmer than body text",
                palette.name
            );
            // egui's own derived weak colour is used by widgets this app does not
            // paint; it must at least stay readable.
            let derived = contrast(visuals.weak_text_color(), palette.panel);
            assert!(
                derived >= 2.0,
                "{}: egui's weak text has {derived:.1}:1",
                palette.name
            );
            // A selected row is tinted, not flooded: text on it must still read.
            let ratio = contrast(visuals.widgets.active.fg_stroke.color, visuals.widgets.active.bg_fill);
            assert!(
                ratio >= 2.5,
                "{} has {ratio:.1}:1 selected-row contrast",
                palette.name
            );
        }
    }

    #[test]
    fn a_palette_can_be_found_by_name() {
        assert_eq!(by_name("Nord").map(|palette| palette.name), Some("Nord"));
        assert_eq!(by_name("Midnight").map(|palette| palette.mode), Some(Mode::Dark));
        assert!(by_name("No Such Scheme").is_none());
    }

    #[test]
    fn an_old_accent_maps_to_a_whole_palette() {
        // A session from before palettes existed has a mode and an accent; it
        // must still open, in the closest scheme.
        let dark = nearest(true, Color32::from_rgb(0x88, 0xc0, 0xd0));
        assert_eq!(dark.name, "Nord");
        assert!(dark.is_dark());

        let light = nearest(false, Color32::from_rgb(0xc9, 0x64, 0x00));
        assert_eq!(light.name, "Solstice");
        assert!(!light.is_dark());

        // An accent no palette has still lands somewhere sensible.
        let odd = nearest(true, Color32::from_rgb(0x12, 0x34, 0x56));
        assert!(odd.is_dark());
    }

    #[test]
    fn every_palette_can_be_found_again() {
        // `apply` remembers which palette is in force by index, and the rest of
        // the app reads the colours from that index. If a palette cannot be found
        // again, the window is painted in one scheme and the plot in another.
        for (index, palette) in PALETTES.iter().enumerate() {
            assert_eq!(index_of(palette), index, "{}", palette.name);
            assert_eq!(PALETTES[index_of(palette)].name, palette.name);
        }
        // Names are the key, so they must be unique.
        for (index, palette) in PALETTES.iter().enumerate() {
            for other in &PALETTES[index + 1..] {
                assert_ne!(palette.name, other.name);
            }
        }
    }

    #[test]
    fn the_curve_wheel_cycles() {
        let palette = &PALETTES[0];
        assert_eq!(palette.curve(0), palette.accent);
        assert_eq!(palette.curve(6), palette.accent);
        assert_ne!(palette.curve(1), palette.curve(2));
    }

    #[test]
    fn hover_is_derived_and_stays_close_to_its_surface() {
        for palette in &PALETTES {
            let hover = palette.hover(palette.raised);
            assert_ne!(hover, palette.raised, "{} has no hover state", palette.name);
            // Lifted towards the text colour, never away from the surface by more
            // than a step.
            let distance = |a: Color32, b: Color32| {
                (i32::from(a.r()) - i32::from(b.r())).abs()
                    + (i32::from(a.g()) - i32::from(b.g())).abs()
                    + (i32::from(a.b()) - i32::from(b.b())).abs()
            };
            assert!(distance(hover, palette.raised) < 120, "{}", palette.name);
        }
    }
}
