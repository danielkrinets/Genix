//! The «Управление» window: keycaps drawn like real keys. A key lights up while it is held down on the real keyboard.
use eframe::egui::{self, Color32, Key, Rect, Sense, Stroke, Vec2};

#[derive(Clone, Copy, PartialEq)]
enum K {
    Key(Key),
    Shift,
    Ctrl,
    Alt,
    Cmd,
}

fn is_down(i: &egui::InputState, k: K) -> bool {
    match k {
        K::Key(key) => i.key_down(key),
        K::Shift => i.modifiers.shift,
        K::Ctrl => i.modifiers.ctrl,
        K::Alt => i.modifiers.alt,
        K::Cmd => i.modifiers.command,
    }
}

/// One physical key: a lower "body" and a face that sinks when pressed.
fn cap(ui: &mut egui::Ui, rect: Rect, label: &str, pressed: bool, used: bool) {
    let accent = crate::theme::accent();
    let dark = ui.visuals().dark_mode;
    let body = if dark { Color32::from_rgb(20, 24, 31) } else { Color32::from_rgb(170, 176, 186) };
    let face = if pressed { accent } else if dark { Color32::from_rgb(58, 66, 80) } else { Color32::from_rgb(244, 246, 249) };
    let ink = if pressed { crate::theme::on_accent() } else { ui.visuals().text_color() };
    let depth = if pressed { 1.0 } else { 4.0 };
    let top = Rect::from_min_size(rect.min + Vec2::new(0.0, 4.0 - depth), rect.size() - Vec2::new(0.0, 4.0));
    ui.painter().rect_filled(Rect::from_min_size(rect.min + Vec2::new(0.0, 4.0), rect.size() - Vec2::new(0.0, 4.0)), 8.0, body);
    ui.painter().rect_filled(top, 8.0, face);
    if used && !pressed {
        ui.painter().rect_stroke(top.shrink(0.5), 8.0, Stroke::new(1.5_f32, accent), egui::StrokeKind::Inside);
    }
    ui.painter().text(top.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(if label.chars().count() > 2 { 11.0 } else { 13.5 }), ink);
}

/// A key in running text, e.g. in a row of the shortcut table.
fn inline(ui: &mut egui::Ui, label: &str, k: K) {
    let w = (ui.painter().layout_no_wrap(label.to_string(), egui::FontId::proportional(13.0), Color32::WHITE).size().x + 18.0).max(30.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::hover());
    let pressed = ui.input(|i| is_down(i, k));
    cap(ui, rect, label, pressed, false);
}

const LETTER_KEYS: [(&str, Key); 26] = [
    ("A", Key::A), ("B", Key::B), ("C", Key::C), ("D", Key::D), ("E", Key::E), ("F", Key::F), ("G", Key::G), ("H", Key::H), ("I", Key::I),
    ("J", Key::J), ("K", Key::K), ("L", Key::L), ("M", Key::M), ("N", Key::N), ("O", Key::O), ("P", Key::P), ("Q", Key::Q), ("R", Key::R),
    ("S", Key::S), ("T", Key::T), ("U", Key::U), ("V", Key::V), ("W", Key::W), ("X", Key::X), ("Y", Key::Y), ("Z", Key::Z),
];

fn letter(c: char) -> K {
    K::Key(LETTER_KEYS.iter().find(|(n, _)| n.starts_with(c)).map(|(_, k)| *k).unwrap_or(Key::A))
}

/// What a key does in Genix (shown under the keyboard while it is held).
fn meaning(c: char) -> Option<&'static str> {
    Some(match c {
        'S' => "Новый чертёж",
        'E' => "Выдавить",
        'M' => "Переместить",
        'F' => "Вписать модель",
        'L' => "В чертеже: линия",
        'R' => "В чертеже: прямоугольник",
        'C' => "В чертеже: окружность",
        'A' => "В чертеже: дуга",
        'P' => "В чертеже: многоугольник",
        'O' => "В чертеже: паз",
        'T' => "В чертеже: обрезать",
        'D' => "В чертеже: размер",
        'X' => "В чертеже: вспомогательная линия",
        'Z' => "⌘Z — отменить, ⇧⌘Z — повторить",
        _ => return None,
    })
}

pub fn window(ctx: &egui::Context, open: &mut bool) {
    egui::Window::new(crate::i18n::tr("Управление")).open(open).default_width(580.0).show(ctx, |ui| {
        let tr = crate::i18n::tr;
        keyboard(ui);
        ui.add_space(14.0);
        mouse_rows(ui);
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tr("Клавиши с рамкой работают в Genix. Нажмите клавишу — она нажмётся и здесь.")).weak().small());
        let _ = ctx;
    });
}

fn keyboard(ui: &mut egui::Ui) {
    let tr = crate::i18n::tr;
    let unit = 36.0_f32;
    let gap = 5.0_f32;
    let rows: [(f32, Vec<(&str, K, f32)>); 5] = [
        (0.0, vec![("Esc", K::Key(Key::Escape), 1.3), ("1", K::Key(Key::Num1), 1.0), ("2", K::Key(Key::Num2), 1.0), ("3", K::Key(Key::Num3), 1.0), ("4", K::Key(Key::Num4), 1.0), ("5", K::Key(Key::Num5), 1.0), ("6", K::Key(Key::Num6), 1.0), ("7", K::Key(Key::Num7), 1.0), ("8", K::Key(Key::Num8), 1.0), ("9", K::Key(Key::Num9), 1.0), ("0", K::Key(Key::Num0), 1.0), ("⌫", K::Key(Key::Backspace), 1.4)]),
        (0.0, vec![("Tab", K::Key(Key::Tab), 1.6), ("Q", letter('Q'), 1.0), ("W", letter('W'), 1.0), ("E", letter('E'), 1.0), ("R", letter('R'), 1.0), ("T", letter('T'), 1.0), ("Y", letter('Y'), 1.0), ("U", letter('U'), 1.0), ("I", letter('I'), 1.0), ("O", letter('O'), 1.0), ("P", letter('P'), 1.0)]),
        (0.0, vec![("A", letter('A'), 1.0), ("S", letter('S'), 1.0), ("D", letter('D'), 1.0), ("F", letter('F'), 1.0), ("G", letter('G'), 1.0), ("H", letter('H'), 1.0), ("J", letter('J'), 1.0), ("K", letter('K'), 1.0), ("L", letter('L'), 1.0), ("⏎", K::Key(Key::Enter), 2.2)]),
        (0.0, vec![("⇧", K::Shift, 2.2), ("Z", letter('Z'), 1.0), ("X", letter('X'), 1.0), ("C", letter('C'), 1.0), ("V", letter('V'), 1.0), ("B", letter('B'), 1.0), ("N", letter('N'), 1.0), ("M", letter('M'), 1.0), ("⇧", K::Shift, 2.2)]),
        (0.0, vec![("⌃", K::Ctrl, 1.3), ("⌥", K::Alt, 1.3), ("⌘", K::Cmd, 1.6), ("", K::Key(Key::Space), 5.2), ("⌘", K::Cmd, 1.6), ("⌥", K::Alt, 1.3)]),
    ];
    let mut held: Option<char> = None;
    let used_keys = ['S', 'E', 'M', 'F', 'L', 'R', 'C', 'A', 'P', 'O', 'T', 'D', 'X', 'Z'];
    for (_, row) in rows.iter() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::splat(gap);
            for (label, k, w) in row {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(unit * w + gap * (w - 1.0).max(0.0), unit + 4.0), Sense::hover());
                let pressed = ui.input(|i| is_down(i, *k));
                let ch = label.chars().next().filter(|c| c.is_ascii_uppercase() && label.chars().count() == 1);
                let used = ch.is_some_and(|c| used_keys.contains(&c)) || matches!(k, K::Key(Key::Escape | Key::Backspace | Key::Tab | Key::Enter) | K::Cmd | K::Shift);
                cap(ui, rect, label, pressed, used);
                if let Some(c) = ch {
                    if pressed { held = Some(c); }
                    if let Some(text) = meaning(c) { resp.on_hover_text(tr(text)); }
                }
            }
        });
    }
    ui.add_space(8.0);
    let line = match held.and_then(meaning) {
        Some(text) => format!("{}: {}", held.unwrap(), tr(text)),
        None => String::new(),
    };
    ui.label(egui::RichText::new(if line.is_empty() { tr("Нажмите клавишу, чтобы увидеть её действие") .to_string() } else { line }).color(crate::theme::secondary(ui)));
}

fn mouse_rows(ui: &mut egui::Ui) {
    let tr = crate::i18n::tr;
    egui::Grid::new("help-keys").num_columns(2).spacing([22.0, 10.0]).show(ui, |ui| {
        let text_row = |ui: &mut egui::Ui, a: &str, b: &str| {
            ui.label(a);
            ui.label(egui::RichText::new(b).weak());
            ui.end_row();
        };
        text_row(ui, tr("Вращать вид"), tr("Левая кнопка мыши по пустому месту / по телу"));
        text_row(ui, tr("Сдвинуть вид"), tr("Правая или средняя кнопка, Shift+левая, два пальца на трекпаде"));
        text_row(ui, tr("Масштаб"), tr("Колесо мыши, щипок на трекпаде, ⌘+прокрутка"));
        text_row(ui, tr("Выбрать тело"), tr("Щелчок; Shift — добавить к выбору"));
        text_row(ui, tr("Точная длина при рисовании"), tr("Наберите число и Enter"));
        ui.label(tr("Отменить / повторить"));
        ui.horizontal(|ui| { inline(ui, "⌘", K::Cmd); inline(ui, "Z", letter('Z')); ui.label("/"); inline(ui, "⇧", K::Shift); inline(ui, "⌘", K::Cmd); inline(ui, "Z", letter('Z')); });
        ui.end_row();
        ui.label(tr("Сохранить / открыть"));
        ui.horizontal(|ui| { inline(ui, "⌘", K::Cmd); inline(ui, "S", letter('S')); ui.label("/"); inline(ui, "⌘", K::Cmd); inline(ui, "O", letter('O')); });
        ui.end_row();
        ui.label(tr("Удалить выбранное"));
        inline(ui, "⌫", K::Key(Key::Backspace));
        ui.end_row();
        ui.label(tr("Отменить действие"));
        inline(ui, "Esc", K::Key(Key::Escape));
        ui.end_row();
        ui.label(tr("Переключить поле при вводе размеров"));
        inline(ui, "Tab", K::Key(Key::Tab));
        ui.end_row();
    });
}
