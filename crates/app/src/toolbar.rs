//! Панель инструментов: группы с быстрыми кнопками и выпадающим списком.

use crate::i18n::tr;
use crate::theme::accent;
use eframe::egui::{self, Align2, Color32, FontId, Response, RichText, Sense, Stroke};

pub struct Item {
    pub id: &'static str,
    pub icon: &'static str,
    pub label: &'static str,
    pub key: &'static str,
    pub tip: &'static str,
}

const fn it(id: &'static str, icon: &'static str, label: &'static str, key: &'static str, tip: &'static str) -> Item {
    Item { id, icon, label, key, tip }
}

pub const CREATE: &[Item] = &[
    it("sketch", "✏", "Новый чертёж", "S", "2D-чертёж на плоскости или грани"),
    it("extrude", "⬆", "Выдавить", "E", "Выдавить профиль чертежа"),
    it("revolve", "⟳", "Вращение", "", "Повернуть профиль вокруг оси"),
    it("sweep", "↝", "По траектории", "", "Протянуть профиль вдоль пути"),
    it("loft", "◣", "Переход", "", "Плавный переход между профилями"),
    it("hole", "◎", "Отверстие", "H", "Простое, с цековкой или зенковкой"),
    it("box", "■", "Параллелепипед", "", "Прямоугольный блок"),
    it("cylinder", "▮", "Цилиндр", "", "Цилиндр по оси Z"),
    it("sphere", "●", "Сфера", "", "Шар"),
    it("torus", "◯", "Тор", "", "Бублик"),
    it("coil", "≋", "Спираль", "", "Пружина: сечение вдоль винтовой линии"),
    it("pipe", "⊡", "Труба", "", "Труба вдоль пути из чертежа"),
    it("mirror", "⇋", "Зеркало", "", "Зеркальная копия тел"),
    it("pattern", "▦", "Массив", "", "Прямоугольный или круговой массив копий"),
];

pub const MODIFY: &[Item] = &[
    it("move", "✚", "Переместить", "M", "Сдвиг/поворот/копирование тел"),
    it("press_pull", "⇅", "Вытянуть грань", "", "Вытянуть или вдавить плоскую грань тела в любую сторону"),
    it("place", "⤓", "Выровнять", "", "Как Align во Fusion: грань тела ложится на грань другого тела или на плоскость"),
    it("scale", "⇲", "Масштаб", "", "Изменить размер тел"),
    it("fillet", "◠", "Скругление", "", "Скруглить прямые рёбра"),
    it("shell", "◰", "Оболочка", "", "Выдолбить тело, оставив стенки"),
    it("chamfer", "◢", "Фаска", "", "Снять фаску с прямых рёбер"),
    it("combine", "∪", "Комбинировать", "", "Объединить, вычесть или пересечь тела"),
    it("cutter", "", "Тело-отверстие", "", "Сделать выбранное тело прозрачным инструментом для вычитания"),
    it("remove", "✖", "Удалить", "Del", "Удалить тела"),
];

pub const REFERENCE: &[Item] = &[it("plane", "▱", "Смещённая плоскость", "", "Плоскость на расстоянии от грани или плоскости")];

pub const INSPECT: &[Item] = &[it("measure", "↔", "Измерить", "", "Расстояние между двумя точками тела")];

const QUICK_CREATE: &[&str] = &["sketch", "extrude", "revolve", "hole"];
const QUICK_MODIFY: &[&str] = &["move", "place", "fillet", "chamfer", "cutter"];

/// Большая кнопка: значок над подписью.
pub fn tool_button(ui: &mut egui::Ui, id: &str, icon: &str, label: &str, enabled: bool, selected: bool) -> Response {
    let font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE);
    let w = (galley.size().x + 12.0).max(58.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 66.0), if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let hover = resp.hovered() && enabled && !ui.input(|i| i.any_touches());
        let press = crate::widgets::feedback(ui, resp.id.with("press"), resp.is_pointer_button_down_on());
        let fill = if selected { crate::theme::tint(30) } else if hover { v.widgets.hovered.weak_bg_fill } else { Color32::TRANSPARENT };
        let p = ui.painter();
        let paint_rect = rect.shrink(press * 1.5);
        p.rect_filled(paint_rect, 6.0, fill);
        if resp.has_focus() { p.rect_stroke(paint_rect, 6.0, Stroke::new(1.5_f32, accent()), egui::StrokeKind::Inside); }
        let col = if enabled { v.text_color() } else { v.weak_text_color().gamma_multiply(0.6) };
        let icon_rect = egui::Rect::from_center_size(rect.center_top() + egui::vec2(0.0, 24.0), egui::vec2(34.0, 34.0) * (1.0 - press * 0.03));
        match crate::icons::icon(id, 32.0, if selected { accent() } else { col }) {
            Some(img) => img.paint_at(ui, icon_rect),
            None => {
                ui.painter().text(rect.center_top() + egui::vec2(0.0, 5.0), Align2::CENTER_TOP, icon, FontId::proportional(24.0), col);
            }
        }
        p.text(rect.center_bottom() - egui::vec2(0.0, 5.0), Align2::CENTER_BOTTOM, label, font, col);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    resp
}

/// Группа кнопок с названием-выпадающим списком. Возвращает идентификатор выбранного действия.
pub fn group(ui: &mut egui::Ui, title: &'static str, quick: &[&str], all: &'static [Item], enabled: bool, active: Option<&str>) -> Option<&'static str> {
    let mut chosen = None;
    ui.vertical(|ui| {
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for id in quick {
                if let Some(item) = all.iter().find(|i| i.id == *id) {
                    let r = tool_button(ui, item.id, item.icon, tr(item.label), enabled, active == Some(item.id)).on_hover_ui(|ui| rich_tip(ui, item));
                    if r.clicked() {
                        chosen = Some(item.id);
                    }
                }
            }
        }).response.rect.width();
        let popup_id = ui.make_persistent_id(("toolbar_group", title));
        let open = ui.memory(|m| m.is_popup_open(popup_id));
        let (title_rect, title_resp) = ui.allocate_exact_size(egui::vec2(row.max(92.0), 22.0), if enabled { Sense::click() } else { Sense::hover() });
        if title_resp.hovered() && enabled { ui.painter().rect_filled(title_rect, 6.0, ui.visuals().widgets.hovered.weak_bg_fill); }
        let col = if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
        ui.painter().text(title_rect.left_center() + egui::vec2(8.0, 0.0), Align2::LEFT_CENTER, tr(title), FontId::proportional(12.5), col);
        let turn = crate::widgets::feedback(ui, title_resp.id.with("chevron"), open);
        let center = title_rect.right_center() - egui::vec2(13.0, 0.0);
        let rot = egui::emath::Rot2::from_angle(std::f32::consts::PI * turn);
        let points = [egui::vec2(-5.0, -2.0), egui::vec2(0.0, 3.0), egui::vec2(5.0, -2.0)].map(|p| center + rot * p);
        ui.painter().line(points.to_vec(), Stroke::new(2.0_f32, col));
        // one physical click can arrive twice within a few frames; a short lock keeps the list from opening and closing at once
        let now = ui.input(|i| i.time);
        let last = ui.data(|d| d.get_temp::<f64>(popup_id.with("toggled"))).unwrap_or(f64::NEG_INFINITY);
        if title_resp.clicked() && now - last > 0.35 {
            ui.memory_mut(|m| m.toggle_popup(popup_id));
            ui.data_mut(|d| d.insert_temp(popup_id.with("toggled"), now));
        }
        egui::popup::popup_below_widget(ui, popup_id, &title_resp, egui::popup::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(250.0);
            for item in all {
                // строка на всю ширину списка: подсветка при наведении не вылезает и не обрезается
                let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(250.0), 30.0), Sense::click());
                let r = r.on_hover_ui(|ui| rich_tip(ui, item));
                let p = ui.painter();
                if r.hovered() {
                    p.rect_filled(rect.shrink2(egui::vec2(2.0, 1.0)), 6.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let col = ui.visuals().text_color();
                match crate::icons::icon(item.id, 20.0, col) {
                    Some(img) => img.paint_at(ui, egui::Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), egui::vec2(20.0, 20.0))),
                    None => {
                        p.text(egui::pos2(rect.left() + 18.0, rect.center().y), Align2::CENTER_CENTER, item.icon, FontId::proportional(15.0), col);
                    }
                }
                p.text(egui::pos2(rect.left() + 36.0, rect.center().y), Align2::LEFT_CENTER, tr(item.label), FontId::proportional(13.5), col);
                if !item.key.is_empty() {
                    p.text(egui::pos2(rect.right() - 12.0, rect.center().y), Align2::RIGHT_CENTER, item.key, FontId::proportional(12.0), ui.visuals().weak_text_color());
                }
                if r.clicked() {
                    chosen = Some(item.id);
                }
            }
        });
    });
    chosen
}

fn tip(item: &Item) -> String {
    let t = tr(item.tip);
    if item.key.is_empty() { t.to_string() } else { format!("{t} ({})", item.key) }
}

fn rich_tip(ui: &mut egui::Ui, item: &Item) {
    visual_tip(ui, tr(item.label), &tip(item), item.id);
}

pub fn visual_tip(ui: &mut egui::Ui, title: &str, body: &str, kind: &str) {
    ui.set_max_width(270.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.label(RichText::new(body).color(crate::theme::secondary(ui)));
    ui.add_space(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(250.0, 112.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
    let ink = ui.visuals().text_color().gamma_multiply(0.72);
    let acc = accent();
    let cell = egui::vec2(84.0, 84.0);
    let before = egui::Rect::from_center_size(rect.center() + egui::vec2(-68.0, 0.0), cell);
    let after = egui::Rect::from_center_size(rect.center() + egui::vec2(68.0, 0.0), cell);
    tip_shape(&p, before, kind, false, ink, acc);
    tip_shape(&p, after, kind, true, ink, acc);
    let y = rect.center().y;
    let (x0, x1) = (rect.center().x - 16.0, rect.center().x + 16.0);
    p.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], Stroke::new(2.0_f32, acc));
    p.line_segment([egui::pos2(x1 - 6.0, y - 5.0), egui::pos2(x1, y)], Stroke::new(2.0_f32, acc));
    p.line_segment([egui::pos2(x1 - 6.0, y + 5.0), egui::pos2(x1, y)], Stroke::new(2.0_f32, acc));
}

/// Схема «до / после» для подсказки: вид сбоку на брусок, над которым выполняется действие.
fn tip_shape(p: &egui::Painter, r: egui::Rect, kind: &str, done: bool, ink: Color32, acc: Color32) {
    use egui::{pos2, Pos2, Shape};
    let fill = ink.gamma_multiply(0.16);
    let line = Stroke::new(1.5_f32, ink);
    let hot = Stroke::new(3.0_f32, acc);
    let b = egui::Rect::from_min_max(r.min + egui::vec2(10.0, 24.0), r.max - egui::vec2(10.0, 12.0));
    // сплошной контур; углы по часовой стрелке от левого верхнего
    let poly = |pts: Vec<Pos2>, stroke: Stroke| {
        p.add(Shape::convex_polygon(pts.clone(), fill, Stroke::NONE));
        let mut closed = pts;
        closed.push(closed[0]);
        p.add(Shape::line(closed, stroke));
    };
    let arc = |c: Pos2, rad: f32, a0: f32, a1: f32| -> Vec<Pos2> {
        (0..=10).map(|i| { let a = a0 + (a1 - a0) * i as f32 / 10.0; pos2(c.x + rad * a.cos(), c.y + rad * a.sin()) }).collect()
    };
    match kind {
        "fillet" | "chamfer" => {
            let k = 20.0;
            let mut pts = vec![b.left_top()];
            let mut edge = Vec::new();
            if !done {
                pts.push(b.right_top());
            } else if kind == "fillet" {
                pts.push(pos2(b.right() - k, b.top()));
                edge = arc(pos2(b.right() - k, b.top() + k), k, -std::f32::consts::FRAC_PI_2, 0.0);
                pts.extend(edge.iter().copied());
            } else {
                pts.push(pos2(b.right() - k, b.top()));
                edge = vec![pos2(b.right() - k, b.top()), pos2(b.right(), b.top() + k)];
                pts.push(pos2(b.right(), b.top() + k));
            }
            pts.push(b.right_bottom());
            pts.push(b.left_bottom());
            poly(pts, line);
            if done { p.add(Shape::line(edge, hot)); } else { p.circle_filled(b.right_top(), 4.0, acc); }
        }
        "hole" | "cutter" => {
            poly(vec![b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom()], line);
            let w = 22.0;
            let slot = egui::Rect::from_min_max(pos2(b.center().x - w / 2.0, b.top()), pos2(b.center().x + w / 2.0, b.bottom()));
            if done {
                p.rect_filled(slot, 0.0, p.ctx().style().visuals.extreme_bg_color);
                p.line_segment([slot.left_top(), slot.left_bottom()], hot);
                p.line_segment([slot.right_top(), slot.right_bottom()], hot);
            } else {
                p.circle_stroke(pos2(b.center().x, b.top()), 6.0, hot);
                p.line_segment([pos2(b.center().x, b.top() - 14.0), pos2(b.center().x, b.top() - 3.0)], hot);
            }
        }
        "sketch" => {
            let s = egui::Rect::from_center_size(r.center(), egui::vec2(52.0, 40.0));
            p.rect_stroke(s, 2.0, if done { hot } else { Stroke::new(2.0_f32, acc) }, egui::StrokeKind::Middle);
            if done { p.circle_stroke(s.center(), 9.0, hot); }
        }
        "extrude" | "revolve" | "sweep" | "loft" | "pipe" => {
            let s = egui::Rect::from_center_size(r.center() + egui::vec2(-6.0, 8.0), egui::vec2(46.0, 30.0));
            if !done {
                p.rect_stroke(s, 2.0, hot, egui::StrokeKind::Middle);
            } else {
                let o = egui::vec2(14.0, -14.0);
                p.add(Shape::convex_polygon(vec![s.left_top(), s.left_top() + o, s.right_top() + o, s.right_top()], fill.gamma_multiply(1.6), line));
                p.add(Shape::convex_polygon(vec![s.right_top(), s.right_top() + o, s.right_bottom() + o, s.right_bottom()], fill.gamma_multiply(2.4), line));
                p.add(Shape::convex_polygon(vec![s.left_top(), s.right_top(), s.right_bottom(), s.left_bottom()], fill, hot));
            }
        }
        _ => {
            poly(vec![b.left_top(), b.right_top(), b.right_bottom(), b.left_bottom()], line);
            if done { p.rect_stroke(b.shrink(1.5), 2.0, hot, egui::StrokeKind::Inside); }
        }
    }
}

/// Все группы вкладки «Твердотельное моделирование». Возвращает выбранный инструмент.
pub fn solid_toolbar(ui: &mut egui::Ui, enabled: bool, active: Option<&str>) -> Option<&'static str> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.add_space(2.0);
            ui.label(RichText::new(tr("ДЕТАЛЬ")).small().strong().color(accent()));
            ui.add_space(22.0);
        });
        ui.separator();
        out = out.or(group(ui, "СТРОИТЬ", QUICK_CREATE, CREATE, enabled, active));
        ui.separator();
        out = out.or(group(ui, "ПРАВИТЬ", QUICK_MODIFY, MODIFY, enabled, active));
        ui.separator();
        out = out.or(group(ui, "ОПОРЫ", &["plane"], REFERENCE, enabled, active));
        ui.separator();
        out = out.or(group(ui, "ПРОВЕРКА", &["measure"], INSPECT, enabled, active));
    });
    out
}
