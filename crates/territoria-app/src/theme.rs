//! Visual design: palettes for dark and light mode, spacing and rounding.

use egui::style::{Selection, WidgetVisuals};
use egui::{Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Theme, Vec2};

/// Warnings, values outside their tolerance.
pub const WARN: Color32 = Color32::from_rgb(0xe0, 0x9b, 0x2d);
/// Destructive actions, errors.
pub const DANGER: Color32 = Color32::from_rgb(0xe5, 0x48, 0x4d);
pub const ACCENT: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
/// Something checked out fine (balanced, matched).
pub const SUCCESS: Color32 = Color32::from_rgb(0x36, 0xa8, 0x5f);

struct Palette {
    /// Window background behind panels (darkest in dark mode).
    base: Color32,
    /// Panels, tab bars, toolbars.
    panel: Color32,
    /// Editors, grids, text fields.
    surface: Color32,
    /// Floating windows, menus, popups.
    raised: Color32,
    /// Text fields; must stand out from `raised` (dialogs) and `panel`.
    field: Color32,
    border: Color32,
    text: Color32,
    text_weak: Color32,
    hover: Color32,
    active: Color32,
    selection: Color32,
    stripe: Color32,
    error: Color32,
    warn: Color32,
    link: Color32,
}

const DARK: Palette = Palette {
    base: Color32::from_rgb(0x17, 0x18, 0x1b),
    panel: Color32::from_rgb(0x1e, 0x1f, 0x23),
    surface: Color32::from_rgb(0x1a, 0x1b, 0x1e),
    raised: Color32::from_rgb(0x27, 0x29, 0x2e),
    field: Color32::from_rgb(0x1a, 0x1b, 0x1e),
    border: Color32::from_rgb(0x32, 0x35, 0x3b),
    text: Color32::from_rgb(0xdc, 0xde, 0xe3),
    text_weak: Color32::from_rgb(0x8b, 0x90, 0x9a),
    hover: Color32::from_rgb(0x2c, 0x2f, 0x35),
    active: Color32::from_rgb(0x35, 0x39, 0x41),
    selection: Color32::from_rgba_premultiplied(0x23, 0x4e, 0x94, 0xb0),
    stripe: Color32::from_rgb(0x1f, 0x20, 0x24),
    error: Color32::from_rgb(0xf2, 0x6d, 0x6d),
    warn: Color32::from_rgb(0xe8, 0xb3, 0x4a),
    link: Color32::from_rgb(0x6c, 0xa6, 0xff),
};

const LIGHT: Palette = Palette {
    base: Color32::from_rgb(0xe9, 0xeb, 0xef),
    panel: Color32::from_rgb(0xf4, 0xf5, 0xf7),
    surface: Color32::from_rgb(0xff, 0xff, 0xff),
    raised: Color32::from_rgb(0xff, 0xff, 0xff),
    field: Color32::from_rgb(0xec, 0xee, 0xf2),
    border: Color32::from_rgb(0xd9, 0xdc, 0xe1),
    text: Color32::from_rgb(0x1f, 0x23, 0x28),
    text_weak: Color32::from_rgb(0x6a, 0x71, 0x7c),
    hover: Color32::from_rgb(0xe6, 0xe8, 0xec),
    active: Color32::from_rgb(0xd8, 0xdc, 0xe3),
    selection: Color32::from_rgba_premultiplied(0xa6, 0xc8, 0xff, 0xc0),
    stripe: Color32::from_rgb(0xf7, 0xf8, 0xfa),
    error: Color32::from_rgb(0xc8, 0x2b, 0x2b),
    warn: Color32::from_rgb(0xa8, 0x62, 0x00),
    link: Color32::from_rgb(0x1d, 0x5f, 0xd6),
};

pub fn install(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    set_fonts(ctx, None);
    ctx.style_mut_of(Theme::Dark, |s| apply(s, &DARK));
    ctx.style_mut_of(Theme::Light, |s| apply(s, &LIGHT));
}

/// Installs the fonts, with `code_font` (font file bytes) first for code.
pub fn set_fonts(ctx: &egui::Context, code_font: Option<Vec<u8>>) {
    // The UI font lacks arrows and some symbols; fall back to the
    // monospace font (Hack) before the emoji fonts.
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        family.insert(1.min(family.len()), "Hack".to_owned());
    }
    if let Some(bytes) = code_font {
        fonts.font_data.insert(
            "code".to_owned(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            family.insert(0, "code".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

fn apply(style: &mut Style, p: &Palette) {
    let radius = CornerRadius::same(5);
    let spacing = &mut style.spacing;
    spacing.item_spacing = Vec2::new(8.0, 5.0);
    spacing.button_padding = Vec2::new(8.0, 3.0);
    spacing.interact_size = Vec2::new(36.0, 22.0);
    spacing.menu_margin = Margin::same(6);
    spacing.window_margin = Margin::same(14);
    spacing.indent = 16.0;
    spacing.scroll.bar_width = 8.0;
    spacing.scroll.floating = true;

    let v = &mut style.visuals;
    v.panel_fill = p.panel;
    v.window_fill = p.raised;
    v.extreme_bg_color = p.surface;
    v.text_edit_bg_color = Some(p.field);
    v.code_bg_color = p.surface;
    v.faint_bg_color = p.stripe;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    let shadow_alpha = if v.dark_mode { 110 } else { 40 };
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(shadow_alpha),
    };
    v.selection = Selection {
        bg_fill: p.selection,
        stroke: Stroke::new(1.0, p.text),
    };
    v.hyperlink_color = p.link;
    v.error_fg_color = p.error;
    v.warn_fg_color = p.warn;
    v.striped = true;
    v.text_cursor.stroke = Stroke::new(2.0, ACCENT);

    let widget = |bg: Color32, stroke: Color32, fg: Color32| WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: Stroke::new(1.0, stroke),
        corner_radius: radius,
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    };
    let w = &mut v.widgets;
    w.noninteractive = widget(p.panel, p.border, p.text);
    w.noninteractive.weak_bg_fill = p.panel;
    w.inactive = widget(p.hover, Color32::TRANSPARENT, p.text);
    // No stroke rather than a transparent one: egui takes the stroke width
    // off the button padding, and buttons without an idle frame (icon
    // buttons) only draw the stroke on hover, so they would grow by it.
    w.inactive.bg_stroke = Stroke::NONE;
    w.hovered = widget(p.active, p.border, p.text);
    w.active = widget(p.active, ACCENT, p.text);
    w.open = widget(p.active, p.border, p.text);
    // Weak text (labels marked .weak()) uses this.
    v.weak_text_color = Some(p.text_weak);
    let _ = p.base;
}

/// Frame of the toolbar strip below the menu.
pub fn toolbar_frame(style: &Style) -> Frame {
    Frame::new()
        .fill(style.visuals.panel_fill)
        .inner_margin(Margin::symmetric(8, 4))
}

/// Frame of the status bar.
pub fn status_frame(style: &Style) -> Frame {
    Frame::new()
        .fill(style.visuals.window_fill)
        .inner_margin(Margin::symmetric(10, 3))
        .stroke(Stroke::new(
            1.0,
            style.visuals.widgets.noninteractive.bg_stroke.color,
        ))
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    /// Buttons without an idle frame (icon buttons) keep their size on hover.
    #[test]
    fn frameless_button_keeps_size_on_hover() {
        let mut h = Harness::new_ui(|ui| {
            ui.add(egui::Button::new("Run").frame_when_inactive(false));
        });
        super::install(&h.ctx);
        h.run();
        let idle = h.get_by_label("Run").rect();
        h.get_by_label("Run").hover();
        h.run();
        assert_eq!(h.get_by_label("Run").rect(), idle);
    }
}
