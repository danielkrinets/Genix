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

/// Filled flat pill (B01, B15): one solid accent fill that lightens on hover and darkens on press. No gradients, shadows or overlays.
pub fn pill_fill(ui: &Ui, rect: Rect, radius: f32, hover: f32, press: f32) {
    let fill = crate::theme::accent().lerp_to_gamma(Color32::WHITE, 0.10 * hover).lerp_to_gamma(Color32::BLACK, 0.16 * press);
    ui.painter().rect_filled(rect, radius, fill);
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

/// What a tab pill reports: the pill itself and whether its close cross was pressed.
pub struct TabHit {
    pub tab: Response,
    pub close: bool,
}

/// B14 — document tab, 26 px. Selected = solid accent; a group colour shows as a thin bar under the label.
pub fn b14(ui: &mut Ui, text: &str, selected: bool, group: Option<Color32>, closable: bool) -> TabHit {
    let width = tr_width(ui, text, 13.0) + 28.0 + if closable { 18.0 } else { 0.0 };
    let (rect, tab) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click());
    let hover = crate::widgets::feedback(ui, tab.id.with("hover"), tab.hovered() && !ui.input(|i| i.any_touches()));
    let close_rect = Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(18.0));
    let close = closable && (selected || tab.hovered());
    let close_resp = if close { Some(ui.interact(close_rect, tab.id.with("close"), Sense::click())) } else { None };
    let (ink, fill) = if selected { (crate::theme::on_accent(), crate::theme::accent()) } else { (ui.visuals().text_color(), ui.visuals().text_color().gamma_multiply(0.09 * hover)) };
    ui.painter().rect_filled(rect, 13.0, fill);
    ui.painter().text(egui::pos2(rect.left() + 14.0, rect.center().y), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(13.0), ink);
    if let Some(color) = group { ui.painter().rect_filled(Rect::from_min_size(egui::pos2(rect.left() + 10.0, rect.bottom() - 3.0), Vec2::new(rect.width() - 20.0, 2.0)), 1.0, color); }
    let mut pressed = false;
    if let Some(x) = close_resp {
        let c = close_rect.center();
        let stroke = egui::Stroke::new(1.6_f32, ink.gamma_multiply(if x.hovered() { 1.0 } else { 0.65 }));
        ui.painter().line_segment([c - Vec2::splat(3.5), c + Vec2::splat(3.5)], stroke);
        ui.painter().line_segment([c + Vec2::new(-3.5, 3.5), c + Vec2::new(3.5, -3.5)], stroke);
        pressed = x.clicked();
    }
    tab.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, text));
    TabHit { tab: tab.on_hover_cursor(egui::CursorIcon::PointingHand), close: pressed }
}

/// Group chip of the tab bar: coloured pill with the group name; click folds or unfolds the group.
pub fn group_chip(ui: &mut Ui, text: &str, color: Color32) -> Response {
    flat(ui, text, Vec2::new(tr_width(ui, text, 12.5) + 20.0, 22.0), |ui, rect, hover, _| {
        ui.painter().rect_filled(rect, 11.0, color.lerp_to_gamma(Color32::WHITE, 0.12 * hover));
        label(ui, rect, text, 12.5, Color32::WHITE);
    })
}
