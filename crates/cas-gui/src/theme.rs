//! The two looks the app wears.
//!
//! The palette is Blender-ish on purpose: a dark grey shell, flat mid-grey
//! widgets, and one warm accent used only for selection and the current
//! action. Giving the mathematics the only saturated colour on screen is what
//! makes the editor read as the centre of the window.
//!
//! Both themes are installed once at start-up, so switching is a preference
//! change rather than a rebuild of every style field.

use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, Stroke, TextStyle, Visuals, vec2};

/// Warm orange — selection, the active tab, the plotted curve.
pub const ACCENT: Color32 = Color32::from_rgb(0xe8, 0x7d, 0x0d);
/// Cool blue — links and the "solver is thinking" hint.
pub const BLUE: Color32 = Color32::from_rgb(0x3d, 0x7e, 0xff);

/// Which palette the app is wearing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Dark,
    Light,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    pub fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Dark => egui::Theme::Dark,
            Self::Light => egui::Theme::Light,
        }
    }

    fn visuals(self) -> Visuals {
        match self {
            Self::Dark => dark_visuals(),
            Self::Light => light_visuals(),
        }
    }
}

/// Install both themes and start in dark mode.
pub fn install(ctx: &egui::Context) {
    for mode in [Mode::Dark, Mode::Light] {
        let theme = mode.egui_theme();
        ctx.set_visuals_of(theme, mode.visuals());
        ctx.style_mut_of(theme, |style| {
            style.text_styles.insert(
                TextStyle::Heading,
                FontId::proportional(26.0),
            );
            style.text_styles.insert(TextStyle::Body, FontId::proportional(15.0));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(14.5));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(11.5));
            style.text_styles.insert(
                TextStyle::Monospace,
                FontId::monospace(14.0),
            );
            style.spacing.item_spacing = vec2(8.0, 6.0);
            style.spacing.button_padding = vec2(10.0, 4.0);
            style.spacing.menu_margin = Margin::same(8);
            style.spacing.interact_size.y = 26.0;
            style.visuals.window_corner_radius = CornerRadius::same(6);
            style.visuals.menu_corner_radius = CornerRadius::same(6);
        });
    }
    apply(ctx, Mode::Dark);
}

/// Make `mode` the active theme.
///
/// The tuned visuals are re-applied here as well as in [`install`] so a change
/// made through this function is complete even if some other code replaced the
/// style in between.
pub fn apply(ctx: &egui::Context, mode: Mode) {
    let theme = mode.egui_theme();
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, mode.visuals());
    ctx.set_visuals(mode.visuals());
    ctx.request_repaint();
}

fn dark_visuals() -> Visuals {
    let mut visuals = Visuals::dark();

    visuals.window_fill = Color32::from_rgb(0x1e, 0x1e, 0x1e);
    visuals.panel_fill = Color32::from_rgb(0x23, 0x23, 0x23);
    visuals.faint_bg_color = Color32::from_rgb(0x2b, 0x2b, 0x2b);
    visuals.extreme_bg_color = Color32::from_rgb(0x16, 0x16, 0x16);
    visuals.code_bg_color = Color32::from_rgb(0x1a, 0x1a, 0x1a);
    visuals.text_edit_bg_color = Some(Color32::from_rgb(0x18, 0x18, 0x18));
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3a, 0x3a));
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [4, 8],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };

    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(0x23, 0x23, 0x23);
    visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(0x23, 0x23, 0x23);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x3a, 0x3a, 0x3a));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0xc8, 0xc8, 0xc8));

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0x3a, 0x3a, 0x3a);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x33, 0x33, 0x33);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x45, 0x45, 0x45));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0xe6, 0xe6, 0xe6));

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x4a, 0x4a, 0x4a);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x4a, 0x4a, 0x4a);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x5a, 0x5a, 0x5a));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);

    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xff, 0xa0, 0x3a));
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = BLUE;
    visuals.warn_fg_color = Color32::from_rgb(0xff, 0xa9, 0x4d);
    visuals.error_fg_color = Color32::from_rgb(0xff, 0x6b, 0x6b);
    visuals
}

fn light_visuals() -> Visuals {
    let mut visuals = Visuals::light();

    visuals.window_fill = Color32::from_rgb(0xf1, 0xf1, 0xf1);
    visuals.panel_fill = Color32::from_rgb(0xf8, 0xf8, 0xf8);
    visuals.faint_bg_color = Color32::from_rgb(0xec, 0xec, 0xec);
    visuals.extreme_bg_color = Color32::from_rgb(0xff, 0xff, 0xff);
    visuals.code_bg_color = Color32::from_rgb(0xef, 0xef, 0xef);
    visuals.text_edit_bg_color = Some(Color32::from_rgb(0xff, 0xff, 0xff));
    visuals.window_stroke = Stroke::new(1.0, Color32::from_rgb(0xd0, 0xd0, 0xd0));
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [4, 8],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(40),
    };

    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(0xf8, 0xf8, 0xf8);
    visuals.widgets.noninteractive.weak_bg_fill = Color32::from_rgb(0xf8, 0xf8, 0xf8);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xd5, 0xd5, 0xd5));
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0x33, 0x33, 0x33));

    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0xe6, 0xe6, 0xe6);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xe6, 0xe6, 0xe6);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xcf, 0xcf, 0xcf));
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0x1f, 0x1f, 0x1f));

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0xd8, 0xd8, 0xd8);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xd8, 0xd8, 0xd8);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xb9, 0xb9, 0xb9));
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::from_rgb(0x00, 0x00, 0x00));

    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xc9, 0x64, 0x00));
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);

    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.hyperlink_color = Color32::from_rgb(0x1b, 0x5c, 0xd6);
    visuals.warn_fg_color = Color32::from_rgb(0xa8, 0x5a, 0x00);
    visuals.error_fg_color = Color32::from_rgb(0xc0, 0x39, 0x2b);
    visuals
}
