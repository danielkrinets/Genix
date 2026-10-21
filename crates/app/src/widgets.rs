//! Небольшие виджеты: поле для числа/выражения, подписи.

use eframe::egui::{self, Color32, RichText};
use fusion_core::expr::format_num;
use fusion_core::{Expr, Params};

/// Поле ввода числа или выражения (`40`, `w*2`, `sqrt(a^2+b^2)`).
/// Значение записывается только когда оно корректно; ошибка показывается красной рамкой и подсказкой.
/// Возвращает true, если выражение изменилось.
pub fn expr_field(ui: &mut egui::Ui, id: impl std::hash::Hash, e: &mut Expr, params: &Params, unit: &'static str) -> bool {
    let scrub_id = egui::Id::new(&id).with("scrub");
    let id = ui.make_persistent_id(id);
    // length fields show and accept the unit chosen in the settings; the document keeps millimetres
    let is_len = unit == "мм";
    let show = |s: &str| if is_len { crate::units::field_text(s) } else { s.to_string() };
    let store = |s: &str| if is_len { crate::units::stored_text(s) } else { s.to_string() };
    let step_base = if is_len { crate::units::unit().step() } else if unit == "×" { 0.1 } else { 1.0 };
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| show(&e.0));
    // если выражение поменяли снаружи (например, отменой), а поле не в фокусе — показываем новое
    let focused = ui.memory(|m| m.has_focus(id));
    let spring_id = id.with("spring");
    let mut spring: Option<Spring> = ui.data(|d| d.get_temp(spring_id));
    if !focused && text != show(&e.0) {
        text = show(&e.0);
    }
    let mut changed = false;
    // value changes by arrows, wheel or dragging: the model follows with a spring (a little overshoot, then settles)
    let mut nudge: Option<f64> = None;
    let mut dragged_px = 0.0_f32;
    ui.horizontal(|ui| {
        let ok = params.eval(&store(&text)).is_ok();
        let mut te = egui::TextEdit::singleline(&mut text).id(id).desired_width(ui.spacing().text_edit_width.min(90.0));
        if !ok {
            te = te.background_color(Color32::from_rgb(255, 225, 225)).text_color(Color32::from_rgb(160, 0, 0));
        }
        let r = ui.add(te);
        if r.changed() {
            spring = None;
            if params.eval(&store(&text)).is_ok() && store(text.trim()) != e.0 {
                e.0 = store(text.trim());
                changed = true;
            }
        }
        // on-screen arrows: a click is ±1 step (Shift ±0.1 of it), holding repeats; only for plain numbers
        let number = text.trim().parse::<f64>().ok();
        let fine = ui.input(|i| i.modifiers.shift);
        if let Some(d) = stepper(ui, id.with("stepper"), number.is_some()) {
            nudge = Some(d * step_base * if fine { 1.0 } else { 1.0 });
        }
        if !unit.is_empty() {
            let label = if is_len { crate::units::label() } else { crate::i18n::tr(unit) };
            // the unit is also a drag handle: pull right to grow, left to shrink
            let u = ui.add(egui::Label::new(RichText::new(label).weak()).sense(egui::Sense::drag()));
            let u = u.on_hover_cursor(egui::CursorIcon::ResizeHorizontal).on_hover_text(crate::i18n::tr("Потяните вправо или влево, чтобы менять значение"));
            dragged_px += u.drag_delta().x;
        }
        // показываем вычисленное значение для выражений
        match params.eval(&store(&text)) {
            Ok(v) if text.trim().parse::<f64>().is_err() => {
                let v = if is_len { crate::units::to_display(v) } else { v };
                ui.label(RichText::new(format!("= {}", format_num((v * 1e6).round() / 1e6))).weak());
            }
            Err(msg) => {
                r.on_hover_text(msg);
            }
            _ => {}
        }
        // колёсико над полем: ±1 шаг (с Shift — в 10 раз меньше) для простых чисел
        if ui.rect_contains_pointer(ui.min_rect()) && number.is_some() {
            let dy = ui.input(|i| i.raw_scroll_delta.y);
            if dy != 0.0 {
                nudge = Some(step_base * if fine { 0.1 } else { 1.0 } * dy.signum() as f64);
            }
        }
    });
    // a label elsewhere (the chip caption) may also be dragged: it leaves its pixels under the same key
    dragged_px += ui.data_mut(|d| d.remove_temp::<f32>(scrub_id)).unwrap_or(0.0);
    let now = ui.input(|i| i.time);
    let current = text.trim().parse::<f64>().ok();
    if let (Some(v), true) = (current, dragged_px != 0.0) {
        // every 6 px is one step; Shift makes it ten times finer. No spring while the hand is on it.
        let acc = ui.data_mut(|d| { let a = d.get_temp::<f32>(id.with("px")).unwrap_or(0.0) + dragged_px; d.insert_temp(id.with("px"), a); a });
        let step = step_base * if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
        let n = (acc / 6.0).trunc();
        if n != 0.0 {
            ui.data_mut(|d| d.insert_temp(id.with("px"), acc - n * 6.0));
            let nv = ((v + n as f64 * step) * 1e6).round() / 1e6;
            // far-out values only break the geometry: dragging stops at ±1e5 mm-equivalent
            let nv = nv.clamp(-1e5, 1e5);
            text = format_num(nv);
            e.0 = store(&text);
            spring = None;
            changed = true;
            crate::haptics::tick();
        }
    } else if ui.input(|i| i.pointer.any_released()) {
        ui.data_mut(|d| d.remove_temp::<f32>(id.with("px")));
    }
    if let (Some(d), Some(v)) = (nudge, current) {
        let target = (((v + d) * 1e6).round() / 1e6).clamp(-1e5, 1e5);
        let from = spring.as_ref().map_or(v, |s| s.value(now));
        text = format_num(target);
        spring = if crate::theme::reduced_motion() { None } else { Some(Spring { from, to: target, start: now }) };
        e.0 = store(&text);
        changed = true;
    }
    if let Some(sp) = spring {
        let t = now - sp.start;
        if t >= SPRING_SECONDS {
            spring = None;
            e.0 = store(&format_num(sp.to));
        } else {
            let mut v = sp.value(now);
            // an overshoot must not make a size negative or zero while it bounces
            if sp.from > 0.0 && sp.to > 0.0 { v = v.max(sp.to.min(sp.from) * 0.25); }
            e.0 = store(&format_num((v * 1e5).round() / 1e5));
            ui.ctx().request_repaint();
        }
        changed = true;
    }
    ui.data_mut(|d| {
        d.insert_temp(id, text);
        match spring { Some(s) => d.insert_temp(spring_id, s), None => { d.remove_temp::<Spring>(spring_id); } }
    });
    changed
}

/// Hands a horizontal drag made on some caption to the field that is drawn next with the same id.
pub fn scrub_caption(ui: &mut egui::Ui, text: egui::RichText, id: impl std::hash::Hash) {
    let r = ui.add(egui::Label::new(text).sense(egui::Sense::drag()));
    let r = r.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
    if r.dragged() {
        let key = egui::Id::new(&id).with("scrub");
        ui.data_mut(|d| d.insert_temp(key, r.drag_delta().x));
    }
}

const SPRING_SECONDS: f64 = 0.7;

/// Damped spring from `from` to `to`: overshoots by about 15 % and settles in well under a second.
#[derive(Clone, Copy, Default)]
struct Spring {
    from: f64,
    to: f64,
    start: f64,
}

impl Spring {
    fn value(&self, now: f64) -> f64 {
        let t = (now - self.start).max(0.0);
        let (zeta, omega) = (0.45_f64, 24.0_f64);
        let wd = omega * (1.0 - zeta * zeta).sqrt();
        let k = 1.0 - (-zeta * omega * t).exp() * ((wd * t).cos() + zeta * omega / wd * (wd * t).sin());
        self.from + (self.to - self.from) * k
    }
}

/// Two stacked arrows ▲▼ beside a number field. Returns the step to add this frame.
fn stepper(ui: &mut egui::Ui, id: egui::Id, enabled: bool) -> Option<f64> {
    let h = ui.spacing().interact_size.y.max(20.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, h), egui::Sense::hover());
    let now = ui.input(|i| i.time);
    let step = if ui.input(|i| i.modifiers.shift) { 0.1 } else { 1.0 };
    let mut out = None;
    for (k, sign) in [(0, 1.0), (1, -1.0)] {
        let half = egui::Rect::from_min_size(rect.min + egui::vec2(0.0, k as f32 * h / 2.0), egui::vec2(16.0, h / 2.0));
        let r = ui.interact(half, id.with(k), if enabled { egui::Sense::click_and_drag() } else { egui::Sense::hover() });
        let col = if !enabled { ui.visuals().weak_text_color().gamma_multiply(0.5) } else if r.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().text_color().gamma_multiply(0.75) };
        if r.hovered() && enabled { ui.painter().rect_filled(half.shrink(1.0), 4.0, crate::theme::tint(24)); }
        let c = half.center();
        let tip = egui::vec2(0.0, -3.0 * sign as f32);
        let side = egui::vec2(4.0, 2.0 * sign as f32);
        ui.painter().line(vec![c - side.x * egui::Vec2::X + egui::vec2(0.0, side.y), c + tip, c + side.x * egui::Vec2::X + egui::vec2(0.0, side.y)], egui::Stroke::new(1.6_f32, col));
        // first press steps at once; after 0.4 s of holding it repeats every 0.08 s
        let key = id.with((k, "held"));
        if r.drag_started() || (r.clicked() && ui.data(|d| d.get_temp::<(f64, f64)>(key)).is_none()) {
            ui.data_mut(|d| d.insert_temp(key, (now, now)));
            out = Some(step * sign);
        } else if r.is_pointer_button_down_on() {
            if let Some((start, last)) = ui.data(|d| d.get_temp::<(f64, f64)>(key)) {
                if now - start > 0.4 && now - last > 0.08 {
                    ui.data_mut(|d| d.insert_temp(key, (start, now)));
                    out = Some(step * sign);
                }
                ui.ctx().request_repaint();
            }
        } else {
            ui.data_mut(|d| d.remove::<(f64, f64)>(key));
        }
        r.on_hover_text(if sign > 0.0 { crate::i18n::tr("Больше (Shift — на 0,1)") } else { crate::i18n::tr("Меньше (Shift — на 0,1)") });
    }
    out
}

/// Language, theme, accent and measurement units as one grid: used by the first-launch setup and by the settings pages.
/// `appearance` adds language, theme and accent above the units.
pub fn preference_grid(ui: &mut egui::Ui, st: &mut crate::settings::Settings, id: &str, appearance: bool) {
    use crate::settings::ThemeChoice;
    egui::Grid::new(id).num_columns(2).spacing([28.0, 12.0]).show(ui, |ui| {
        if appearance {
            ui.label(crate::i18n::tr("Язык"));
            egui::ComboBox::from_id_salt((id, "lang")).selected_text(st.lang.name()).show_ui(ui, |ui| {
                for l in crate::i18n::Lang::ALL { ui.selectable_value(&mut st.lang, l, l.name()); }
            });
            ui.end_row();
            ui.label(crate::i18n::tr("Тема"));
            ui.horizontal(|ui| {
                for (t, n) in [(ThemeChoice::Auto, "Как в системе"), (ThemeChoice::Light, "Светлая"), (ThemeChoice::Dark, "Тёмная")] {
                    ui.selectable_value(&mut st.theme, t, crate::i18n::tr(n));
                }
            });
            ui.end_row();
            ui.label(crate::i18n::tr("Акцентный цвет"));
            ui.horizontal(|ui| { for color in crate::theme::Accent::ALL { if accent_swatch(ui, color, st.accent == color).clicked() { st.accent = color; } } });
            ui.end_row();
        }
        ui.label(crate::i18n::tr("Единицы длины"));
        egui::ComboBox::from_id_salt((id, "unit")).selected_text(crate::i18n::tr(st.length_unit.name())).show_ui(ui, |ui| {
            for u in crate::units::LengthUnit::ALL { ui.selectable_value(&mut st.length_unit, u, crate::i18n::tr(u.name())); }
        });
        ui.end_row();
        ui.label(crate::i18n::tr("Знаков после запятой"));
        ui.horizontal(|ui| { for n in 0..=4u8 { ui.selectable_value(&mut st.decimals, n, n.to_string()); } });
        ui.end_row();
    });
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

/// Shared strong ease-out: cubic-bezier(0.23, 1, 0.32, 1).
pub fn ease_out(t: f32) -> f32 {
    if t <= 0.0 { return 0.0 }
    if t >= 1.0 { return 1.0 }
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..16 {
        let u = (lo + hi) * 0.5;
        let x = 3.0 * (1.0 - u).powi(2) * u * 0.23 + 3.0 * (1.0 - u) * u * u * 0.32 + u.powi(3);
        if x < t { lo = u } else { hi = u }
    }
    1.0 - (1.0 - (lo + hi) * 0.5).powi(3)
}

#[derive(Clone, Copy)]
struct FeedbackTween { from: f32, to: f32, started: f64 }

/// Retarget from the currently painted value, so quick reversals never jump.
pub fn feedback(ui: &egui::Ui, id: egui::Id, active: bool) -> f32 {
    feedback_duration(ui, id, active, 0.16, false)
}

fn feedback_duration(ui: &egui::Ui, id: egui::Id, active: bool, duration: f64, linear: bool) -> f32 {
    let now = ui.input(|i| i.time);
    let target = if active { 1.0 } else { 0.0 };
    let keyboard = ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. })));
    let mut tween = ui.data(|d| d.get_temp::<FeedbackTween>(id)).unwrap_or(FeedbackTween { from: target, to: target, started: now });
    let progress = ((now - tween.started) / duration).clamp(0.0, 1.0) as f32;
    let mut value = tween.from + (tween.to - tween.from) * if linear { progress } else { ease_out(progress) };
    if crate::theme::reduced_motion() || keyboard {
        value = target;
        tween = FeedbackTween { from: target, to: target, started: now };
    } else if tween.to != target {
        tween = FeedbackTween { from: value, to: target, started: now };
    }
    if (value - target).abs() > 0.001 { ui.ctx().request_repaint(); }
    ui.data_mut(|d| d.insert_temp(id, tween));
    value
}

/// Interface-scale slider: the zoom changes once, on release; while dragging only the number moves.
/// Re-zooming the whole UI on every pointer move made everything stutter.
pub fn scale_slider(ui: &mut egui::Ui, value: &mut f32) -> egui::Response {
    let id = egui::Id::new("scale-slider-draft");
    let mut draft = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(*value);
    let response = ui.add(egui::Slider::new(&mut draft, 0.8..=1.6).step_by(0.05).fixed_decimals(2));
    if response.dragged() {
        ui.data_mut(|d| d.insert_temp(id, draft));
    } else {
        ui.data_mut(|d| d.remove::<f32>(id));
        if response.drag_stopped() || response.changed() { *value = draft; }
    }
    response
}

#[derive(Clone, Default)]
struct WarmTip {
    open: Option<egui::Id>,
    warm_until: f64,
    hover: Option<(egui::Id, f64)>,
    session: u64,
}

const TIP_DELAY: f64 = 0.4;
const TIP_WARM_WINDOW: f64 = 0.3;
const TIP_GLIDE: f32 = 0.32;

/// "Bold (⌘B)" -> ("Bold", Some("⌘B")).
pub fn split_shortcut(tip: &str) -> (&str, Option<&str>) {
    match tip.rfind(" (") {
        Some(i) if tip.ends_with(')') => (&tip[..i], Some(&tip[i + 2..tip.len() - 1])),
        _ => (tip, None),
    }
}

/// Native port of the React Bits WarmTooltip: 400 ms cold delay, no delay for 300 ms after
/// another label closed, and one label that glides from button to button.
/// shortcut: no pop-in blur/scale and no touch long-press, add them if the tooltip is used on touch.
pub fn warm_tooltip(response: &egui::Response, tip: &str) {
    let (label, shortcut) = split_shortcut(tip);
    let ctx = &response.ctx;
    let now = ctx.input(|i| i.time);
    let key = egui::Id::new("warm-tooltip-state");
    let mut st: WarmTip = ctx.data(|d| d.get_temp(key)).unwrap_or_default();
    let id = response.id;
    let hovered = response.hovered() && !response.is_pointer_button_down_on() && !ctx.input(|i| i.any_touches());
    let mut show = false;
    if hovered {
        if st.hover.map(|h| h.0) != Some(id) { st.hover = Some((id, now)); }
        let since = now - st.hover.map_or(now, |h| h.1);
        let warm = st.open.is_some() || now < st.warm_until;
        let delay = if warm { 0.0 } else { TIP_DELAY };
        if since >= delay {
            if !warm { st.session += 1; }
            st.open = Some(id);
            show = true;
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(delay - since));
        }
    } else {
        if st.hover.map(|h| h.0) == Some(id) { st.hover = None; }
        if st.open == Some(id) { st.open = None; st.warm_until = now + TIP_WARM_WINDOW; }
    }
    let session = st.session;
    ctx.data_mut(|d| d.insert_temp(key, st));
    if !show { return; }
    let glide = if crate::theme::reduced_motion() { 0.0 } else { TIP_GLIDE };
    let sid = egui::Id::new(("warm-tip", session));
    let anchor = response.rect.center_top() - egui::vec2(0.0, 5.0);
    let x = ctx.animate_value_with_time(sid.with("x"), anchor.x, glide);
    let y = ctx.animate_value_with_time(sid.with("y"), anchor.y, glide);
    let (surface, ink) = (crate::theme::accent(), crate::theme::on_accent());
    egui::Area::new(sid.with("area")).order(egui::Order::Tooltip).fixed_pos(egui::pos2(x, y)).pivot(egui::Align2::CENTER_BOTTOM).interactable(false).show(ctx, |ui| {
        egui::Frame::new().fill(surface).corner_radius(16).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                ui.label(RichText::new(label).size(11.5).color(ink));
                if let Some(keys) = shortcut {
                    egui::Frame::new().fill(ink.gamma_multiply(0.15)).corner_radius(5).inner_margin(egui::Margin::symmetric(5, 1)).show(ui, |ui| {
                        ui.label(RichText::new(keys).size(10.0).color(ink.gamma_multiply(0.85)));
                    });
                }
            });
        });
    });
}

/// Animated checkmark with native checkbox semantics and an unchanged hit target.
pub fn checkbox(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.into(), egui::FontId::proportional(14.0), ui.visuals().text_color());
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(30.0 + galley.size().x, 32.0), egui::Sense::click());
    if response.clicked() { *value = !*value; response.mark_changed(); }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label));
    let t = feedback_duration(ui, response.id.with("checked"), *value, 0.32, true);
    let press = feedback(ui, response.id.with("press"), response.is_pointer_button_down_on());
    let box_rect = egui::Rect::from_center_size(rect.left_center() + egui::vec2(11.0,0.0), egui::vec2(22.0,22.0) * (1.0 - press * 0.08));
    ui.painter().rect_filled(box_rect, 7.0, ui.visuals().extreme_bg_color.lerp_to_gamma(crate::theme::accent(), t));
    ui.painter().rect_stroke(box_rect, 7.0, egui::Stroke::new(1.5_f32, crate::theme::secondary(ui).lerp_to_gamma(crate::theme::accent(),t)), egui::StrokeKind::Inside);
    paint_check(ui.painter(), box_rect.center(), t, crate::theme::on_accent());
    if response.has_focus() { ui.painter().rect_stroke(box_rect.expand(3.0), 10.0, egui::Stroke::new(1.5_f32, crate::theme::accent()), egui::StrokeKind::Outside); }
    ui.painter().galley(rect.left_center() + egui::vec2(30.0,-galley.size().y*0.5), galley, ui.visuals().text_color());
    response
}

fn paint_check(painter: &egui::Painter, center: egui::Pos2, progress: f32, color: Color32) {
    if progress <= 0.0 { return; }
    let a = center + egui::vec2(-5.0,0.0);
    let b = center + egui::vec2(-1.0,4.0);
    let c = center + egui::vec2(6.0,-5.0);
    let stroke = egui::Stroke::new(2.2_f32,color);
    let first = (progress / 0.35).min(1.0);
    painter.line_segment([a, a + (b-a)*first],stroke);
    if progress > 0.35 { painter.line_segment([b,b+(c-b)*((progress-0.35)/0.65)],stroke); }
}

pub fn toggle(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.into(), egui::FontId::proportional(14.0), ui.visuals().text_color());
    let (rect, mut response) = ui.allocate_exact_size(egui::vec2(52.0 + galley.size().x, 32.0), egui::Sense::click());
    if response.clicked() { *value = !*value; response.mark_changed(); }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label));
    let t = feedback(ui, response.id.with("switch"), *value);
    let press = feedback(ui, response.id.with("press"), response.is_pointer_button_down_on());
    let track = egui::Rect::from_center_size(rect.left_center() + egui::vec2(21.0, 0.0), egui::vec2(42.0, 26.0));
    let off = if ui.visuals().dark_mode { Color32::from_rgb(112, 112, 120) } else { Color32::from_rgb(138, 138, 143) };
    let fill = off.lerp_to_gamma(crate::theme::accent(), t);
    ui.painter().rect_filled(track, 13.0, fill);
    ui.painter().circle_filled(egui::pos2(track.left() + 13.0 + 16.0 * t, track.center().y), 10.0 - press, Color32::WHITE.lerp_to_gamma(crate::theme::on_accent(), t));
    if response.has_focus() { ui.painter().rect_stroke(track.expand(3.0), 16.0, egui::Stroke::new(1.5_f32, crate::theme::accent()), egui::StrokeKind::Outside); }
    ui.painter().galley(rect.left_center() + egui::vec2(52.0, -galley.size().y * 0.5), galley, ui.visuals().text_color());
    response
}

pub fn accent_swatch(ui: &mut egui::Ui, color: crate::theme::Accent, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::click());
    let color_value = color.color(ui.visuals().dark_mode);
    if response.has_focus() {
        ui.painter().rect_stroke(rect.expand(2.0), 10.0, egui::Stroke::new(2.0_f32, ui.visuals().text_color()), egui::StrokeKind::Outside);
    }
    ui.painter().circle_filled(rect.center(), 12.0, color_value);
    let t = feedback_duration(ui, response.id.with("selection"), selected, 0.32, true);
    let press = feedback(ui, response.id.with("press"), response.is_pointer_button_down_on());
    if t > 0.0 {
        ui.painter().circle_stroke(rect.center(), 12.0 + 3.0*t - press, egui::Stroke::new(1.5_f32, color_value.gamma_multiply(t)));
        paint_check(ui.painter(), rect.center(), t, crate::theme::foreground_on(color_value));
    }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, crate::i18n::tr(color.label())));
    response.on_hover_text(crate::i18n::tr(color.label()))
}

pub fn navigation_button_mode(ui: &mut egui::Ui, label: &str, icon: &str, selected: bool, expanded: bool) -> egui::Response {
    let text_width = ui.painter().layout_no_wrap(label.into(), egui::FontId::proportional(14.0), ui.visuals().text_color()).size().x;
    let width = if ui.layout().main_dir().is_vertical() { ui.available_width() } else if expanded { text_width + 60.0 } else { 48.0 };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 46.0), egui::Sense::click());
    let hover = response.hovered() && !ui.input(|i| i.any_touches());
    let t = feedback(ui, response.id.with("selected"), selected);
    let press = feedback(ui, response.id.with("press"), response.is_pointer_button_down_on());
    if t > 0.0 { ui.painter().rect_filled(rect, 14.0, crate::theme::accent().gamma_multiply(t)); }
    if hover && !selected { ui.painter().rect_filled(rect, 14.0, crate::theme::tint(18)); }
    if response.has_focus() { ui.painter().rect_stroke(rect, 14.0, egui::Stroke::new(1.5_f32, crate::theme::accent()), egui::StrokeKind::Inside); }
    let color = if selected { crate::theme::on_accent() } else { ui.visuals().text_color() };
    if let Some(img) = crate::icons::icon(icon, 25.0, color) {
        let center = if expanded { rect.left_center() + egui::vec2(24.0, 0.0) } else { rect.center() };
        img.paint_at(ui, egui::Rect::from_center_size(center, egui::vec2(25.0, 25.0) * (1.0 - press * 0.10)));
    }
    if expanded { ui.painter().text(rect.left_center() + egui::vec2(48.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(14.0), color); }
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label));
    if expanded { response } else { response.on_hover_text(label) }
}

pub fn loading(ui: &mut egui::Ui, label: &str) {
    ui.horizontal(|ui| {
        if !crate::theme::reduced_motion() { ui.add(egui::Spinner::new().color(crate::theme::accent()).size(18.0)); }
        else { ui.painter().circle_filled(ui.next_widget_position() + egui::vec2(5.0, 10.0), 3.0, crate::theme::accent()); ui.add_space(14.0); }
        ui.label(RichText::new(label).color(crate::theme::secondary(ui)));
    });
}

#[cfg(test)]
mod rounded_tests {
    use super::*;
    #[test]
    fn feedback_reverses_from_its_current_value_without_jumping() {
        let ctx = egui::Context::default();
        let sample = |time: f64, active: bool| {
            let mut input = egui::RawInput::default(); input.time = Some(time);
            let mut value = 0.0;
            let _ = ctx.run(input, |ctx| { egui::CentralPanel::default().show(ctx, |ui| {
                value = feedback(ui, egui::Id::new("test-feedback"), active);
            }); });
            value
        };
        assert_eq!(sample(0.0,false),0.0);
        assert_eq!(sample(0.01,true),0.0);
        let halfway = sample(0.05,true);
        assert!(halfway > 0.0 && halfway < 1.0);
        let reversed = sample(0.05,false);
        assert!((reversed-halfway).abs() < 0.001);
        assert!(sample(0.10,false) < reversed);
        assert_eq!(sample(0.30,false),0.0);
    }

    #[test]
    fn checkmark_is_half_drawn_after_160ms_and_finishes_after_320ms() {
        let ctx = egui::Context::default();
        let sample = |time: f64, selected: bool| {
            let mut input = egui::RawInput::default(); input.time = Some(time);
            let mut value = 0.0;
            let _ = ctx.run(input, |ctx| { egui::CentralPanel::default().show(ctx, |ui| {
                value = feedback_duration(ui, egui::Id::new("slow-check"), selected, 0.32, true);
            }); });
            value
        };
        assert_eq!(sample(0.0,false),0.0);
        sample(0.01,true);
        assert!((sample(0.17,true)-0.5).abs()<0.001);
        assert_eq!(sample(0.34,true),1.0);
    }

    #[test]
    fn switch_supports_pointer_and_keyboard_activation() {
        assert_activation(toggle);
    }

    #[test]
    fn checkbox_supports_pointer_and_keyboard_activation() {
        assert_activation(checkbox);
    }

    fn assert_activation(widget: fn(&mut egui::Ui, &mut bool, &str) -> egui::Response) {
        let ctx = egui::Context::default();
        let mut value = false;
        let mut rect = egui::Rect::NOTHING;
        let mut id = egui::Id::NULL;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = widget(ui, &mut value, "Test"); rect = r.rect; id = r.id;
            });
        });
        let mut input = egui::RawInput::default();
        input.events = vec![egui::Event::PointerMoved(rect.center()),
            egui::Event::PointerButton { pos: rect.center(), button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            egui::Event::PointerButton { pos: rect.center(), button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }];
        let _ = ctx.run(input, |ctx| { egui::CentralPanel::default().show(ctx, |ui| { widget(ui, &mut value, "Test"); }); });
        assert!(value);
        ctx.memory_mut(|m| m.request_focus(id));
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
        let _ = ctx.run(input, |ctx| { egui::CentralPanel::default().show(ctx, |ui| { widget(ui, &mut value, "Test"); }); });
        assert!(!value);
    }
}

#[cfg(test)]
mod warm_tip_tests {
    use super::split_shortcut;

    #[test]
    fn shortcut_is_split_from_the_label() {
        assert_eq!(split_shortcut("Вписать модель (F)"), ("Вписать модель", Some("F")));
        assert_eq!(split_shortcut("Снимок (⇧⌘P)"), ("Снимок", Some("⇧⌘P")));
        assert_eq!(split_shortcut("Сетка"), ("Сетка", None));
    }
}

#[cfg(test)]
mod header_width_tests {
    use super::*;

    #[test]
    fn menu_bar_fills_the_row_but_its_real_content_width_is_measurable() {
        let ctx = egui::Context::default();
        let mut used = 0.0;
        let mut row = 0.0;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let start = ui.cursor().left();
                let bar = egui::menu::bar(ui, |ui| {
                    ui.label("a");
                    ui.label("b");
                    used = ui.cursor().left() - start;
                });
                row = bar.response.rect.width();
            });
        });
        assert!(used > 0.0 && used < 100.0, "{used}");
        assert!(row > 500.0, "the bar was expected to take the whole row, got {row}");
    }
}
