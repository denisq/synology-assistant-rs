//! macOS-like look: white content area, light-grey chrome (toolbar, status bar,
//! side panels), system-blue accent with white text on selection. Light and dark
//! variants follow the OS appearance.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, Visuals};

pub struct Palette {
    pub content: Color32,
    pub chrome: Color32,
    pub stripe: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub text_secondary: Color32,
    pub accent: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub control_active: Color32,
    pub green: Color32,
    pub orange: Color32,
    pub red: Color32,
    pub gray: Color32,
}

pub const LIGHT: Palette = Palette {
    content: Color32::WHITE,
    chrome: Color32::from_rgb(246, 246, 246),
    stripe: Color32::from_rgb(244, 245, 247),
    separator: Color32::from_rgb(222, 222, 222),
    text: Color32::from_rgb(29, 29, 31),
    text_secondary: Color32::from_rgb(110, 110, 115),
    accent: Color32::from_rgb(0, 122, 255),
    control: Color32::WHITE,
    control_hover: Color32::from_rgb(240, 240, 242),
    control_active: Color32::from_rgb(226, 226, 230),
    green: Color32::from_rgb(40, 167, 69),
    orange: Color32::from_rgb(230, 130, 0),
    red: Color32::from_rgb(215, 45, 40),
    gray: Color32::from_rgb(142, 142, 147),
};

pub const DARK: Palette = Palette {
    content: Color32::from_rgb(30, 30, 30),
    chrome: Color32::from_rgb(40, 40, 42),
    stripe: Color32::from_rgb(37, 37, 39),
    separator: Color32::from_rgb(58, 58, 60),
    text: Color32::from_rgb(235, 235, 240),
    text_secondary: Color32::from_rgb(152, 152, 157),
    accent: Color32::from_rgb(10, 132, 255),
    control: Color32::from_rgb(58, 58, 60),
    control_hover: Color32::from_rgb(72, 72, 74),
    control_active: Color32::from_rgb(88, 88, 90),
    green: Color32::from_rgb(50, 205, 90),
    orange: Color32::from_rgb(255, 159, 10),
    red: Color32::from_rgb(255, 69, 58),
    gray: Color32::from_rgb(142, 142, 147),
};

pub fn palette(ui: &egui::Ui) -> &'static Palette {
    if ui.visuals().dark_mode { &DARK } else { &LIGHT }
}

fn visuals(p: &Palette, dark: bool) -> Visuals {
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    let r6 = CornerRadius::same(6);
    v.panel_fill = p.content;
    v.window_fill = p.content;
    v.window_stroke = Stroke::new(1.0, p.separator);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.faint_bg_color = p.stripe;
    v.extreme_bg_color = p.content;
    v.text_edit_bg_color = Some(p.control);
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.orange;
    v.error_fg_color = p.red;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.content;
    w.noninteractive.weak_bg_fill = p.content;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r6;
    for (st, fill) in [
        (&mut w.inactive, p.control),
        (&mut w.hovered, p.control_hover),
        (&mut w.active, p.control_active),
        (&mut w.open, p.control_active),
    ] {
        st.bg_fill = fill;
        st.weak_bg_fill = fill;
        st.bg_stroke = Stroke::new(1.0, p.separator);
        st.fg_stroke = Stroke::new(1.0, p.text);
        st.corner_radius = r6;
        st.expansion = 0.0;
    }
    v
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals_of(Theme::Light, visuals(&LIGHT, false));
    ctx.set_visuals_of(Theme::Dark, visuals(&DARK, true));
    ctx.global_style_mut(|s| {
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.interact_size.y = 24.0;
    });
}

/// Frame for toolbar, status bar and side panels.
pub fn chrome_frame(ctx: &egui::Context, margin: egui::Margin) -> egui::Frame {
    let p = if ctx.theme() == Theme::Dark { &DARK } else { &LIGHT };
    egui::Frame::new().fill(p.chrome).inner_margin(margin)
}
