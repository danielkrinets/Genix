//! Оформление: светлая тема и тёмная.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

/// Акцент един для навигации, дока, инструментов и индикаторов.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum Accent {
    #[default]
    Blue,
    Indigo,
    Teal,
    Orange,
    Rose,
}

impl Accent {
    pub const ALL: [Self; 5] = [Self::Blue, Self::Indigo, Self::Teal, Self::Orange, Self::Rose];
    pub fn label(self) -> &'static str {
        match self { Self::Blue => "Синий", Self::Indigo => "Индиго", Self::Teal => "Бирюзовый", Self::Orange => "Оранжевый", Self::Rose => "Розовый" }
    }
    pub fn color(self, dark: bool) -> Color32 {
        let rgb = match (self, dark) {
            (Self::Blue, false) => [0, 101, 212], (Self::Blue, true) => [92, 166, 255],
            (Self::Indigo, false) => [88, 70, 203], (Self::Indigo, true) => [164, 149, 255],
            (Self::Teal, false) => [0, 117, 119], (Self::Teal, true) => [92, 212, 205],
            (Self::Orange, false) => [168, 73, 0], (Self::Orange, true) => [255, 173, 92],
            (Self::Rose, false) => [188, 42, 83], (Self::Rose, true) => [255, 135, 165],
        };
        Color32::from_rgb(rgb[0], rgb[1], rgb[2])
    }
}

static ACCENT_RGB: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x0065D4);
static REDUCED_MOTION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn configure(color: Accent, dark: bool, reduced_motion: bool) -> bool {
    use std::sync::atomic::Ordering::Relaxed;
    let c = color.color(dark);
    let packed = ((c.r() as u32) << 16) | ((c.g() as u32) << 8) | c.b() as u32;
    let old_color = ACCENT_RGB.swap(packed, Relaxed);
    let old_motion = REDUCED_MOTION.swap(reduced_motion, Relaxed);
    old_color != packed || old_motion != reduced_motion
}

pub fn accent() -> Color32 {
    let rgb = ACCENT_RGB.load(std::sync::atomic::Ordering::Relaxed);
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

pub fn reduced_motion() -> bool { REDUCED_MOTION.load(std::sync::atomic::Ordering::Relaxed) }

fn luminance(c: Color32) -> f32 {
    let linear = |v: u8| { let v = v as f32 / 255.0; if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

pub fn foreground_on(color: Color32) -> Color32 {
    if luminance(color) > 0.179 { Color32::BLACK } else { Color32::WHITE }
}

pub fn on_accent() -> Color32 { foreground_on(accent()) }

pub fn surface(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode { Color32::from_rgb(28, 28, 30) } else { Color32::WHITE }
}

pub fn secondary(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode { Color32::from_rgb(174, 174, 184) } else { Color32::from_rgb(99, 99, 109) }
}

pub fn tint(alpha: u8) -> Color32 {
    let c = accent(); Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

pub const CONTROL_RADIUS: u8 = 12;
pub const SURFACE_RADIUS: u8 = 22;

/// Цвета 3D-вида для текущей темы (в sRGB).
pub struct ViewColors {
    pub bg_top: Color32,
    pub bg_bottom: Color32,
    pub body: Color32,
    pub body_selected: Color32,
    pub body_hover: Color32,
    pub edge: Color32,
    pub grid_minor: Color32,
    pub grid_major: Color32,
    pub sketch: Color32,
    pub sketch_constrained: Color32,
    pub sketch_construction: Color32,
    pub sketch_selected: Color32,
    pub sketch_hover: Color32,
    pub profile: Color32,
    pub profile_selected: Color32,
    pub profile_hover: Color32,
    pub plane: Color32,
    pub plane_hover: Color32,
    pub dim_text: Color32,
}

impl Theme {
    pub fn view(self) -> ViewColors {
        let common = |bg_top, bg_bottom, edge, grid_minor, grid_major, sketch, dim_text| ViewColors {
            bg_top,
            bg_bottom,
            body: if self == Theme::Light { Color32::from_rgb(174, 174, 181) } else { Color32::from_rgb(142, 142, 147) },
            body_selected: accent(),
            body_hover: Color32::from_rgb(174, 174, 181).lerp_to_gamma(accent(), 0.25),
            edge,
            grid_minor,
            grid_major,
            sketch,
            sketch_constrained: if self == Theme::Light { Color32::from_rgb(20, 20, 24) } else { Color32::from_rgb(235, 235, 240) },
            sketch_construction: Color32::from_rgb(230, 140, 40),
            sketch_selected: accent(),
            sketch_hover: accent(),
            profile: Color32::from_rgba_unmultiplied(255, 190, 80, 70),
            profile_selected: tint(100),
            profile_hover: tint(60),
            plane: Color32::from_rgba_unmultiplied(230, 160, 60, 60),
            plane_hover: tint(90),
            dim_text,
        };
        match self {
            Theme::Light => common(
                Color32::from_rgb(245, 245, 247),
                Color32::from_rgb(245, 245, 247),
                Color32::from_rgb(30, 32, 36),
                Color32::from_rgba_unmultiplied(80, 90, 105, 40),
                Color32::from_rgba_unmultiplied(70, 80, 95, 90),
                Color32::from_rgb(20, 90, 220),
                Color32::from_rgb(20, 20, 30),
            ),
            Theme::Dark => common(
                Color32::from_rgb(20, 20, 22),
                Color32::from_rgb(20, 20, 22),
                Color32::from_rgb(12, 13, 16),
                Color32::from_rgba_unmultiplied(220, 230, 245, 34),
                Color32::from_rgba_unmultiplied(220, 230, 245, 80),
                Color32::from_rgb(90, 170, 255),
                Color32::from_rgb(235, 238, 245),
            ),
        }
    }

    /// Шрифты: к основному добавляем Hack как запасной — в нём есть символы для значков (↶ ∪ ▦ ⟂ …).
    pub fn install_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        // DejaVu Sans — запасной шрифт: в нём есть иврит и редкие символы
        fonts.font_data.insert("DejaVu".to_owned(), std::sync::Arc::new(egui::FontData::from_static(include_bytes!("../assets/fonts/DejaVuSans.ttf"))));
        if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            fam.push("DejaVu".to_owned());
            fam.push("Hack".to_owned());
        }
        // Системный SF Pro читается с Mac, не распространяется вместе с приложением.
        #[cfg(target_os = "macos")]
        if let Ok(data) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
            fonts.font_data.insert("System UI".into(), std::sync::Arc::new(egui::FontData::from_owned(data)));
            fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "System UI".into());
        }
        ctx.set_fonts(fonts);
    }

    /// Flat rounded: системные нейтральные поверхности и согласованный акцент.
    pub fn apply(self, ctx: &egui::Context) {
        let dark = self == Self::Dark;
        // Select the style slot before mutating it; egui defaults to following the OS.
        ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
        let c = Color32::from_rgb;
        let (canvas, panel, inset, border, fg, muted) = if dark {
            (c(0, 0, 0), c(28, 28, 30), c(44, 44, 46), c(64, 64, 68), c(245, 245, 247), c(174, 174, 184))
        } else {
            (c(242, 242, 247), c(255, 255, 255), c(235, 235, 240), c(220, 220, 225), c(28, 28, 30), c(99, 99, 109))
        };
        let mut v = if dark { Visuals::dark() } else { Visuals::light() };
        let radius = CornerRadius::same(CONTROL_RADIUS);
        v.panel_fill = canvas;
        v.window_fill = panel;
        v.extreme_bg_color = inset;
        v.faint_bg_color = inset;
        v.window_stroke = Stroke::NONE;
        v.override_text_color = None;
        v.widgets.noninteractive.bg_fill = panel;
        v.widgets.noninteractive.weak_bg_fill = inset;
        v.widgets.noninteractive.bg_stroke = Stroke::new(0.5_f32, border);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, fg);
        v.widgets.noninteractive.corner_radius = radius;
        for (w, fill) in [(&mut v.widgets.inactive, inset), (&mut v.widgets.hovered, if dark { c(58, 58, 62) } else { c(223, 223, 230) }), (&mut v.widgets.active, if dark { c(68, 68, 74) } else { c(212, 212, 222) }), (&mut v.widgets.open, inset)] {
            w.corner_radius = radius;
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.bg_stroke = Stroke::NONE;
            w.fg_stroke = Stroke::new(1.0_f32, fg);
            w.expansion = 0.0;
        }
        v.widgets.active.bg_stroke = Stroke::new(1.5_f32, accent());
        v.selection.bg_fill = accent();
        v.selection.stroke = Stroke::new(1.0_f32, on_accent());
        v.hyperlink_color = accent();
        v.window_corner_radius = CornerRadius::same(SURFACE_RADIUS);
        v.menu_corner_radius = CornerRadius::same(16);
        v.window_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 64 } else { 18 }) };
        v.popup_shadow = v.window_shadow;
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        v.indent_has_left_vline = false;
        ctx.set_visuals(v);
        ctx.style_mut(|s| {
            s.animation_time = if reduced_motion() { 0.0 } else { 0.16 };
            s.spacing.item_spacing = egui::vec2(8.0, 7.0);
            s.spacing.button_padding = egui::vec2(12.0, 7.0);
            s.spacing.interact_size.y = 30.0;
            s.spacing.slider_rail_height = 6.0;
            s.spacing.slider_width = 140.0;
            s.spacing.window_margin = egui::Margin::same(20);
            s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
            // Weak text remains readable across panels; local semantic labels use secondary().

        });
        let _ = muted;
    }

}

/// Цвет egui (sRGB, без предумножения) → линейный RGBA для шейдера.
pub fn linear(c: Color32) -> [f32; 4] {
    let r = egui::Rgba::from(c);
    // Rgba хранит предумноженные значения — для полупрозрачных возвращаем исходный цвет
    let a = r.a();
    if a > 0.0 {
        [r.r() / a, r.g() / a, r.b() / a, a]
    } else {
        [0.0; 4]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_theme_survives_system_theme_on_the_next_frame() {
        for theme in [Theme::Light, Theme::Dark] {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let mut input = egui::RawInput::default();
            input.system_theme = Some(if theme == Theme::Light { egui::Theme::Dark } else { egui::Theme::Light });
            let _ = ctx.run(input, |ctx| {
                assert_eq!(ctx.style().visuals.dark_mode, theme == Theme::Dark);
                assert_eq!(ctx.style().visuals.widgets.inactive.corner_radius, egui::CornerRadius::same(CONTROL_RADIUS));
                assert_eq!(ctx.style().visuals.selection.bg_fill, accent());
            });
        }
    }

    #[test]
    fn every_accent_has_readable_button_and_surface_contrast() {
        for dark in [false, true] {
            let surface = if dark { Color32::from_rgb(28, 28, 30) } else { Color32::WHITE };
            for accent in Accent::ALL {
                let color = accent.color(dark);
                let contrast = |a: Color32, b: Color32| {
                    let (a, b) = (luminance(a), luminance(b));
                    (a.max(b) + 0.05) / (a.min(b) + 0.05)
                };
                assert!(contrast(color, foreground_on(color)) >= 4.5, "{accent:?} button, dark={dark}");
                assert!(contrast(color, surface) >= 4.5, "{accent:?} text, dark={dark}");
            }
        }
    }
}
