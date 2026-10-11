//! Небольшие виджеты: поле для числа/выражения, подписи.

use eframe::egui::{self, Color32, RichText};
use fusion_core::expr::format_num;
use fusion_core::{Expr, Params};

/// Поле ввода числа или выражения (`40`, `w*2`, `sqrt(a^2+b^2)`).
/// Значение записывается только когда оно корректно; ошибка показывается красной рамкой и подсказкой.
/// Возвращает true, если выражение изменилось.
pub fn expr_field(ui: &mut egui::Ui, id: impl std::hash::Hash, e: &mut Expr, params: &Params, unit: &'static str) -> bool {
    let id = ui.make_persistent_id(id);
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| e.0.clone());
    // если выражение поменяли снаружи (например, отменой), а поле не в фокусе — показываем новое
    let focused = ui.memory(|m| m.has_focus(id));
    if !focused && text != e.0 {
        text = e.0.clone();
    }
    let result = params.eval(&text);
    let ok = result.is_ok();
    let mut changed = false;
    ui.horizontal(|ui| {
        let mut te = egui::TextEdit::singleline(&mut text).id(id).desired_width(90.0);
        if !ok {
            te = te.background_color(Color32::from_rgb(255, 225, 225)).text_color(Color32::from_rgb(160, 0, 0));
        }
        let r = ui.add(te);
        if r.changed() {
            if params.eval(&text).is_ok() && text.trim() != e.0 {
                e.0 = text.trim().to_string();
                changed = true;
            }
        }
        if !unit.is_empty() {
            ui.label(RichText::new(crate::i18n::tr(unit)).weak());
        }
        // показываем вычисленное значение для выражений
        match params.eval(&text) {
            Ok(v) if text.trim().parse::<f64>().is_err() => {
                ui.label(RichText::new(format!("= {}", format_num(v))).weak());
            }
            Err(msg) => {
                r.on_hover_text(msg);
            }
            _ => {}
        }
        // колёсико над полем: ±1 (с Shift — ±0.1) для простых чисел
        if ui.rect_contains_pointer(ui.min_rect()) {
            if let Ok(v) = text.trim().parse::<f64>() {
                let dy = ui.input(|i| i.raw_scroll_delta.y);
                if dy != 0.0 {
                    let step = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
                    let nv = v + step * dy.signum() as f64;
                    text = format_num(nv);
                    e.0 = text.clone();
                    changed = true;
                }
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, text));
    changed
}

/// Набирается ли сейчас текст в поле ввода (тогда горячие клавиши не срабатывают).
/// В отличие от `wants_keyboard_input`, фокус на кнопке (после Tab) не считается.
pub fn text_input_active(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.focused()).is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some())
}

/// Снять фокус с кнопки/меню, чтобы клавиши снова шли в 3D-вид.
pub fn release_button_focus(ctx: &egui::Context) {
    if let Some(id) = ctx.memory(|m| m.focused()) {
        if egui::TextEdit::load_state(ctx, id).is_none() {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
    }
}

/// Подпись поля фиксированной ширины (для ровных колонок в диалогах).
pub fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.add_sized([110.0, 20.0], egui::Label::new(text).truncate());
}

pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).small().strong().color(ui.visuals().weak_text_color()));
}
