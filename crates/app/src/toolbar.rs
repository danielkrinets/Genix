//! Панель инструментов: группы с быстрыми кнопками и выпадающим списком.

use crate::i18n::tr;
use crate::theme::ACCENT;
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
    it("scale", "⇲", "Масштаб", "", "Изменить размер тел"),
    it("fillet", "◠", "Скругление", "", "Скруглить прямые рёбра"),
    it("shell", "◰", "Оболочка", "", "Выдолбить тело, оставив стенки"),
    it("chamfer", "◢", "Фаска", "", "Снять фаску с прямых рёбер"),
    it("combine", "∪", "Комбинировать", "", "Объединить, вычесть или пересечь тела"),
    it("remove", "✖", "Удалить", "Del", "Удалить тела"),
];

pub const REFERENCE: &[Item] = &[it("plane", "▱", "Смещённая плоскость", "", "Плоскость на расстоянии от грани или плоскости")];

pub const INSPECT: &[Item] = &[it("measure", "↔", "Измерить", "", "Расстояние между двумя точками тела")];

const QUICK_CREATE: &[&str] = &["sketch", "extrude", "revolve", "hole"];
const QUICK_MODIFY: &[&str] = &["move", "fillet", "chamfer", "combine"];

/// Большая кнопка: значок над подписью.
pub fn tool_button(ui: &mut egui::Ui, id: &str, icon: &str, label: &str, enabled: bool, selected: bool) -> Response {
    let font = FontId::proportional(11.5);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE);
    let w = (galley.size().x + 12.0).max(58.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 66.0), if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let fill = if selected {
            ACCENT.gamma_multiply(0.25)
        } else if resp.hovered() && enabled {
            v.widgets.hovered.weak_bg_fill
        } else {
            Color32::TRANSPARENT
        };
        let p = ui.painter();
        p.rect(rect, 5.0, fill, if resp.hovered() && enabled { Stroke::new(1.0_f32, v.widgets.hovered.bg_stroke.color) } else { Stroke::NONE }, egui::StrokeKind::Inside);
        let col = if enabled { v.text_color() } else { v.weak_text_color().gamma_multiply(0.6) };
        let icon_rect = egui::Rect::from_center_size(rect.center_top() + egui::vec2(0.0, 24.0), egui::vec2(34.0, 34.0));
        match crate::icons::icon(id, 32.0, if selected { ACCENT } else { col }) {
            Some(img) => img.paint_at(ui, icon_rect),
            None => {
                ui.painter().text(rect.center_top() + egui::vec2(0.0, 5.0), Align2::CENTER_TOP, icon, FontId::proportional(24.0), col);
            }
        }
        p.text(rect.center_bottom() - egui::vec2(0.0, 5.0), Align2::CENTER_BOTTOM, label, font, col);
    }
    resp
}

/// Группа кнопок с названием-выпадающим списком. Возвращает идентификатор выбранного действия.
pub fn group(ui: &mut egui::Ui, title: &'static str, quick: &[&str], all: &'static [Item], enabled: bool, active: Option<&str>) -> Option<&'static str> {
    let mut chosen = None;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for id in quick {
                if let Some(item) = all.iter().find(|i| i.id == *id) {
                    let r = tool_button(ui, item.id, item.icon, tr(item.label), enabled, active == Some(item.id)).on_hover_text(tip(item));
                    if r.clicked() {
                        chosen = Some(item.id);
                    }
                }
            }
        });
        let title_resp = ui.add_enabled(enabled, egui::Button::new(RichText::new(format!("{} ▾", tr(title))).small().strong()).frame(false));
        let popup_id = ui.make_persistent_id(("toolbar_group", title));
        if title_resp.clicked() {
            ui.memory_mut(|m| m.toggle_popup(popup_id));
        }
        egui::popup::popup_below_widget(ui, popup_id, &title_resp, egui::popup::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(250.0);
            for item in all {
                let key = if item.key.is_empty() { String::new() } else { item.key.to_string() };
                let btn = match crate::icons::icon(item.id, 16.0, ui.visuals().text_color()) {
                    Some(img) => egui::Button::image_and_text(img, tr(item.label)),
                    None => egui::Button::new(format!("{}  {}", item.icon, tr(item.label))),
                };
                let r = ui.add(btn.min_size(egui::vec2(230.0, 24.0)).shortcut_text(key).frame(false)).on_hover_text(tip(item));
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

/// Все группы вкладки «Твердотельное моделирование». Возвращает выбранный инструмент.
pub fn solid_toolbar(ui: &mut egui::Ui, enabled: bool, active: Option<&str>) -> Option<&'static str> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.add_space(2.0);
            ui.label(RichText::new(tr("ДЕТАЛЬ")).small().strong().color(ACCENT));
            ui.add_space(30.0);
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
