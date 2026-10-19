//! Genix button kit. One function per numbered variant of `docs/design/genix-design-system.html`.
//! A button that is not listed there is not added here without the user's approval first.
use eframe::egui::{self, Color32, Rect, Response, Sense, Ui, Vec2};

fn tr_width(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter().layout_no_wrap(text.to_string(), egui::FontId::proportional(size), Color32::WHITE).size().x
}

/// Accent colour for text on tinted surfaces: lifted on dark themes so it stays readable.
fn ink(ui: &Ui) -> Color32 {
    let accent = crate::theme::accent();
    if ui.visuals().dark_mode { accent.lerp_to_gamma(Color32::WHITE, 0.45) } else { accent }
}

/// Filled flat pill (B01, B15): accent fill, a lighter hover, a darker press. No gradients or shadows.
pub fn pill_fill(ui: &Ui, rect: Rect, radius: f32, hover: f32, press: f32) {
    let p = ui.painter();
    p.rect_filled(rect, radius, crate::theme::accent());
    if hover > 0.0 { p.rect_filled(rect, radius, Color32::from_white_alpha((hover * 22.0) as u8)); }
    if press > 0.0 { p.rect_filled(rect, radius, Color32::from_black_alpha((press * 40.0) as u8)); }
}

/// Flat button: allocate, track hover/press, paint through `draw(rect, hover, press)`.
fn flat(ui: &mut Ui, text: &str, size: Vec2, draw: impl FnOnce(&Ui, Rect, f32, f32)) -> Response {
    let enabled = ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let hover = crate::widgets::feedback(ui, response.id.with("hover"), response.hovered() && enabled && !ui.input(|i| i.any_touches()));
    let press = crate::widgets::feedback(ui, response.id.with("press"), response.is_pointer_button_down_on());
    if ui.is_rect_visible(rect) { draw(ui, rect.shrink(press), hover, press); }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    if enabled { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

fn label(ui: &Ui, rect: Rect, text: &str, size: f32, color: Color32) {
    let color = if ui.is_enabled() { color } else { color.gamma_multiply(0.4) };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(size), color);
}

/// B01 — main pill, 46 px. The one primary action of a screen.
pub fn b01(ui: &mut Ui, text: &str, width: f32) -> Response {
    flat(ui, text, Vec2::new(width, 46.0), |ui, rect, hover, press| {
        pill_fill(ui, rect, 23.0, hover, press);
        label(ui, rect, text, 15.5, crate::theme::on_accent());
    })
}

/// Tinted button of one colour: B02 (accent) and B04 (red).
fn tinted(ui: &mut Ui, text: &str, color: Color32, text_color: Color32) -> Response {
    flat(ui, text, Vec2::new(tr_width(ui, text, 14.5) + 36.0, 40.0), |ui, rect, hover, _| {
        ui.painter().rect_filled(rect, 14.0, color.gamma_multiply(0.18 + 0.10 * hover));
        label(ui, rect, text, 14.5, text_color);
    })
}

/// B02 — secondary, accent tint, 40 px. Next to a B01 or alone.
pub fn b02(ui: &mut Ui, text: &str) -> Response {
    let text_color = ink(ui);
    tinted(ui, text, crate::theme::accent(), text_color)
}

/// B03 — text button without fill, 34 px. Links, "Отмена".
pub fn b03(ui: &mut Ui, text: &str) -> Response {
    flat(ui, text, Vec2::new(tr_width(ui, text, 14.5) + 24.0, 34.0), |ui, rect, hover, _| {
        ui.painter().rect_filled(rect, 12.0, crate::theme::accent().gamma_multiply(0.12 * hover));
        label(ui, rect, text, 14.5, ink(ui));
    })
}

/// B04 — destructive, red tint, 40 px. Only behind a confirmation.
pub fn b04(ui: &mut Ui, text: &str) -> Response {
    let red = ui.visuals().error_fg_color;
    tinted(ui, text, red, red)
}

/// B08 — chip, 28 px. Filters, tags; selected = accent fill.
pub fn b08(ui: &mut Ui, text: &str, selected: bool) -> Response {
    flat(ui, text, Vec2::new(tr_width(ui, text, 13.0) + 24.0, 28.0), |ui, rect, hover, _| {
        let fill = if selected { crate::theme::accent() } else { ui.visuals().text_color().gamma_multiply(0.09 + 0.06 * hover) };
        ui.painter().rect_filled(rect, 14.0, fill);
        label(ui, rect, text, 13.0, if selected { crate::theme::on_accent() } else { ui.visuals().text_color() });
    })
}
