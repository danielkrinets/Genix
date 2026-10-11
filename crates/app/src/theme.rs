//! Оформление: светлая тема и тёмная.

use eframe::egui::{self, Color32, CornerRadius, Stroke, Visuals};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

/// Фирменный синий акцент.
pub const ACCENT: Color32 = Color32::from_rgb(24, 150, 224);

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
            body: if self == Theme::Light { Color32::from_rgb(160, 168, 180) } else { Color32::from_rgb(176, 184, 198) },
            body_selected: Color32::from_rgb(110, 170, 235),
            body_hover: Color32::from_rgb(178, 196, 218),
            edge,
            grid_minor,
            grid_major,
            sketch,
            sketch_constrained: if self == Theme::Light { Color32::from_rgb(20, 20, 24) } else { Color32::from_rgb(235, 235, 240) },
            sketch_construction: Color32::from_rgb(230, 140, 40),
            sketch_selected: Color32::from_rgb(0, 190, 255),
            sketch_hover: Color32::from_rgb(90, 210, 255),
            profile: Color32::from_rgba_unmultiplied(255, 190, 80, 70),
            profile_selected: Color32::from_rgba_unmultiplied(40, 140, 255, 120),
            profile_hover: Color32::from_rgba_unmultiplied(80, 170, 255, 90),
            plane: Color32::from_rgba_unmultiplied(230, 160, 60, 60),
            plane_hover: Color32::from_rgba_unmultiplied(40, 140, 255, 110),
            dim_text,
        };
        match self {
            Theme::Light => common(
                Color32::from_rgb(255, 255, 255),
                Color32::from_rgb(222, 228, 235),
                Color32::from_rgb(30, 32, 36),
                Color32::from_rgba_unmultiplied(80, 90, 105, 40),
                Color32::from_rgba_unmultiplied(70, 80, 95, 90),
                Color32::from_rgb(20, 90, 220),
                Color32::from_rgb(20, 20, 30),
            ),
            Theme::Dark => common(
                Color32::from_rgb(70, 84, 102),
                Color32::from_rgb(40, 48, 60),
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
        ctx.set_fonts(fonts);
    }

    /// Оформление в духе GitHub Primer: плоские панели, тонкие рамки, синий акцент, радиус 6.
    pub fn apply(self, ctx: &egui::Context) {
        let dark = self == Theme::Dark;
        let c = |r, g, b| Color32::from_rgb(r, g, b);
        // (холст, панели, поля ввода, рамка, текст, приглушённый текст, кнопка, кнопка при наведении, кнопка нажата)
        let (canvas, subtle, inset, border, fg, muted, btn, btn_hover, btn_active) = if dark {
            (c(47, 56, 68), c(55, 67, 81), c(36, 43, 53), c(78, 91, 108), c(240, 244, 248), c(160, 172, 186), c(66, 79, 95), c(84, 99, 118), c(98, 114, 134))
        } else {
            (c(255, 255, 255), c(246, 248, 250), c(255, 255, 255), c(208, 215, 222), c(31, 35, 40), c(101, 109, 118), c(246, 248, 250), c(234, 238, 242), c(221, 227, 234))
        };
        let mut v = if dark { Visuals::dark() } else { Visuals::light() };
        let r = CornerRadius::same(6);
        v.panel_fill = subtle;
        v.window_fill = if dark { c(60, 73, 89) } else { canvas };
        v.extreme_bg_color = inset;
        v.faint_bg_color = if dark { c(50, 61, 74) } else { subtle };
        v.window_stroke = Stroke::new(1.0_f32, border);
        v.override_text_color = Some(fg);
        v.widgets.noninteractive.bg_fill = subtle;
        v.widgets.noninteractive.weak_bg_fill = subtle;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, border);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, fg);
        for (w, fill) in [(&mut v.widgets.inactive, btn), (&mut v.widgets.hovered, btn_hover), (&mut v.widgets.active, btn_active), (&mut v.widgets.open, btn_hover)] {
            w.corner_radius = r;
            w.bg_fill = fill;
            w.weak_bg_fill = fill;
            w.fg_stroke = Stroke::new(1.0_f32, fg);
        }
        v.widgets.noninteractive.corner_radius = r;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, border);
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, if dark { c(139, 148, 158) } else { c(175, 184, 193) });
        v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        v.selection.bg_fill = ACCENT;
        v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
        v.hyperlink_color = if dark { c(88, 166, 255) } else { c(9, 105, 218) };
        v.window_corner_radius = CornerRadius::same(8);
        v.menu_corner_radius = r;
        let shadow = egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 120 } else { 40 }) };
        v.window_shadow = shadow;
        v.popup_shadow = shadow;
        let _ = muted;
        ctx.set_visuals(v);
        ctx.style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(6.0, 5.0);
            s.spacing.button_padding = egui::vec2(8.0, 4.0);
            s.spacing.interact_size.y = 24.0;
        });
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
