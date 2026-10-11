//! Вход по лицензии и в аккаунт (демо) и главная страница со всеми проектами.

use crate::i18n::tr;
use crate::settings::Settings;
use crate::tf;
use crate::theme::ACCENT;
use eframe::egui::{self, Color32, RichText};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub const LOGO_SVG: &str = include_str!("../assets/logo.svg");

// ------------------------------------------------------------------ лицензия

fn checksum(body: &str) -> String {
    let mut h: u32 = 0x811c9dc5;
    for b in body.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{:04X}", (h ^ (h >> 16)) & 0xFFFF)
}

/// Ключ вида `XXXX-XXXX-XXXX-CCCC`, где CCCC — контрольная сумма первых трёх групп.
/// Это демо-проверка без сервера: она отличает опечатки, но не защищает от подделки.
pub fn license_valid(key: &str) -> bool {
    let key = key.trim().to_uppercase();
    if key == "DEMO" {
        return true;
    }
    let groups: Vec<&str> = key.split('-').collect();
    groups.len() == 4 && groups.iter().all(|g| g.len() == 4 && g.chars().all(|c| c.is_ascii_alphanumeric())) && checksum(&groups[..3].join("-")) == groups[3]
}

/// Выпустить ключ для трёх групп (используется в тестах и при выдаче демо-ключей).
#[allow(dead_code)]
pub fn make_key(body: &str) -> String {
    format!("{body}-{}", checksum(body))
}

pub fn unlocked(st: &Settings) -> bool {
    st.license.is_some() && (st.account.is_some() || st.account_skipped)
}

#[derive(Default)]
pub struct Gate {
    key: String,
    email: String,
    pass: String,
    msg: Option<String>,
}

/// Экран входа: сначала лицензия, потом аккаунт. Возвращает true, если настройки изменились.
pub fn gate_ui(ctx: &egui::Context, st: &mut Settings, g: &mut Gate) -> bool {
    let mut changed = false;
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.08).max(16.0));
            ui.add(egui::Image::from_bytes("bytes://genix/logo.svg", LOGO_SVG.as_bytes().to_vec()).fit_to_exact_size(egui::vec2(96.0, 96.0)));
            ui.add_space(6.0);
            ui.label(RichText::new("Genix").size(34.0).strong());
            ui.add_space(4.0);
            let step = if st.license.is_none() { 1 } else { 2 };
            ui.label(RichText::new(tf!("Шаг {} из 2", step)).weak());
            ui.add_space(16.0);
            ui.allocate_ui(egui::vec2(380.0, 0.0), |ui| {
                egui::Frame::group(ui.style()).inner_margin(egui::Margin::same(18)).show(ui, |ui| {
                    ui.set_width(344.0);
                    if st.license.is_none() {
                        ui.label(RichText::new(tr("Лицензия")).size(18.0).strong());
                        ui.label(RichText::new(tr("Введите ключ лицензии, например XXXX-XXXX-XXXX-XXXX")).weak());
                        ui.add_space(8.0);
                        let r = ui.add(egui::TextEdit::singleline(&mut g.key).hint_text("XXXX-XXXX-XXXX-XXXX").desired_width(f32::INFINITY));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.add_space(8.0);
                        let go = egui::Button::new(RichText::new(tr("Активировать")).strong().color(Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(344.0, 30.0));
                        if ui.add(go).clicked() || enter {
                            if license_valid(&g.key) {
                                st.license = Some(g.key.trim().to_uppercase());
                                g.msg = None;
                                changed = true;
                            } else {
                                g.msg = Some(tr("Неверный ключ лицензии").into());
                            }
                        }
                        if ui.add(egui::Button::new(tr("Демо-режим (без ключа)")).min_size(egui::vec2(344.0, 28.0))).clicked() {
                            st.license = Some("DEMO".into());
                            g.msg = None;
                            changed = true;
                        }
                    } else {
                        ui.label(RichText::new(tr("Вход в аккаунт")).size(18.0).strong());
                        ui.label(RichText::new(tr("Демо: данные хранятся только на этом компьютере")).weak());
                        ui.add_space(8.0);
                        ui.add(egui::TextEdit::singleline(&mut g.email).hint_text(tr("Электронная почта")).desired_width(f32::INFINITY));
                        let r = ui.add(egui::TextEdit::singleline(&mut g.pass).password(true).hint_text(tr("Пароль")).desired_width(f32::INFINITY));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.add_space(8.0);
                        let go = egui::Button::new(RichText::new(tr("Войти")).strong().color(Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(344.0, 30.0));
                        if ui.add(go).clicked() || enter {
                            let email = g.email.trim();
                            if email.contains('@') && email.contains('.') && g.pass.len() >= 4 {
                                st.account = Some(email.to_string());
                                g.pass.clear();
                                g.msg = None;
                                changed = true;
                            } else {
                                g.msg = Some(tr("Проверьте почту и пароль (не короче 4 символов)").into());
                            }
                        }
                        if ui.add(egui::Button::new(tr("Продолжить без аккаунта")).min_size(egui::vec2(344.0, 28.0))).clicked() {
                            st.account_skipped = true;
                            changed = true;
                        }
                        if ui.small_button(tr("← Другой ключ лицензии")).clicked() {
                            st.license = None;
                            changed = true;
                        }
                    }
                    if let Some(m) = &g.msg {
                        ui.add_space(6.0);
                        ui.label(RichText::new(m).color(Color32::from_rgb(220, 70, 70)));
                    }
                });
            });
        });
    });
    changed
}

// ------------------------------------------------------------------ главная

/// UTC-дата из секунд Unix: «ГГГГ-ММ-ДД ЧЧ:ММ».
pub fn format_time(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

#[derive(Clone)]
struct ProjInfo {
    name: String,
    time: String,
    steps: Option<usize>,
    exists: bool,
}

#[derive(Default)]
pub struct HomeState {
    search: String,
    infos: HashMap<String, ProjInfo>,
    thumbs: HashMap<String, egui::TextureHandle>,
    pending: Arc<Mutex<Vec<(String, crate::thumb::Rgba)>>>,
    busy: Arc<AtomicUsize>,
}

/// Отрисовать превью документа на фоновом потоке (прозрачный фон, изометрия).
fn spawn_thumb(key: String, doc: fusion_core::Document, pending: Arc<Mutex<Vec<(String, crate::thumb::Rgba)>>>, busy: Arc<AtomicUsize>) {
    busy.fetch_add(1, Ordering::SeqCst);
    std::thread::spawn(move || {
        let mut ev = fusion_core::Evaluator::default();
        ev.evaluate(&doc);
        let model = ev.model();
        let meshes: Vec<&fusion_core::Mesh> = model.bodies.iter().filter(|b| !doc.hidden_bodies.contains(&b.id)).map(|b| b.mesh()).collect();
        if let Some(img) = crate::thumb::render(&meshes, crate::camera::Camera::ISO.0, crate::camera::Camera::ISO.1, 264, 176) {
            pending.lock().unwrap().push((key, img));
        }
        busy.fetch_sub(1, Ordering::SeqCst);
    });
}

pub enum HomeAction {
    None,
    New,
    Open,
    OpenPath(PathBuf),
    Example(usize),
    Forget(String),
}

impl HomeState {
    /// Перечитать сведения о проектах (при заходе на главную).
    pub fn refresh(&mut self, recent: &[String]) {
        self.infos.clear();
        for p in recent {
            let path = PathBuf::from(p);
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
            let meta = std::fs::metadata(&path).ok();
            let time = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| format_time(d.as_secs())).unwrap_or_default();
            let steps = std::fs::read_to_string(&path).ok().and_then(|s| fusion_core::Document::from_json(&s).ok()).map(|d| d.features.len());
            self.infos.insert(p.clone(), ProjInfo { name, time, steps, exists: meta.is_some() });
        }
        for p in recent.iter().take(16) {
            if self.thumbs.contains_key(p) {
                continue;
            }
            if let Some(doc) = std::fs::read_to_string(p).ok().and_then(|s| fusion_core::Document::from_json(&s).ok()) {
                spawn_thumb(p.clone(), doc, self.pending.clone(), self.busy.clone());
            }
        }
        for (i, f) in [crate::examples::bracket as fn() -> fusion_core::Document, crate::examples::bushing].into_iter().enumerate() {
            let key = format!("ex:{i}");
            if !self.thumbs.contains_key(&key) {
                spawn_thumb(key, f(), self.pending.clone(), self.busy.clone());
            }
        }
    }
}

pub fn home_ui(ui: &mut egui::Ui, h: &mut HomeState, st: &Settings) -> HomeAction {
    let mut action = HomeAction::None;
    let ready: Vec<_> = std::mem::take(&mut *h.pending.lock().unwrap());
    for (k, img) in ready {
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.w, img.h], &img.data);
        h.thumbs.insert(k.clone(), ui.ctx().load_texture(format!("thumb-{k}"), ci, egui::TextureOptions::LINEAR));
    }
    if h.busy.load(Ordering::SeqCst) > 0 {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(tr("Главная")).size(26.0).strong());
                let who = match (&st.account, st.license.as_deref()) {
                    (Some(a), Some("DEMO")) => tf!("{} · демо-лицензия", a),
                    (Some(a), _) => tf!("{} · лицензия активна", a),
                    (None, Some("DEMO")) => tr("Без аккаунта · демо-лицензия").into(),
                    _ => tr("Без аккаунта · лицензия активна").into(),
                };
                ui.label(RichText::new(who).weak());
            });
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            let new = egui::Button::new(RichText::new(tr("+ Новый проект")).strong().color(Color32::WHITE)).fill(ACCENT).min_size(egui::vec2(150.0, 32.0));
            if ui.add(new).clicked() {
                action = HomeAction::New;
            }
            if ui.add(egui::Button::new(tr("Открыть файл…")).min_size(egui::vec2(130.0, 32.0))).clicked() {
                action = HomeAction::Open;
            }
            ui.add_space(12.0);
            ui.add(egui::TextEdit::singleline(&mut h.search).hint_text(tr("Поиск проектов")).desired_width(240.0));
        });
        ui.add_space(14.0);
        let thumbs = &h.thumbs;
        let card = |ui: &mut egui::Ui, key: &str, title: &str, sub: &str| -> egui::Response {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(240.0, 160.0), egui::Sense::click());
            let v = ui.visuals();
            let fill = if resp.hovered() { v.widgets.hovered.weak_bg_fill } else { v.widgets.inactive.weak_bg_fill };
            let stroke = if resp.hovered() { v.widgets.hovered.bg_stroke } else { v.widgets.inactive.bg_stroke };
            let p = ui.painter_at(rect);
            p.rect(rect, 8.0, fill, stroke, egui::StrokeKind::Inside);
            let area = egui::Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(rect.width() - 16.0, 94.0));
            if let Some(t) = thumbs.get(key) {
                let s = t.size_vec2();
                let k = (area.width() / s.x).min(area.height() / s.y);
                let r = egui::Rect::from_center_size(area.center(), s * k);
                p.image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            } else {
                p.text(area.center(), egui::Align2::CENTER_CENTER, "…", egui::FontId::proportional(22.0), v.weak_text_color());
            }
            p.text(rect.left_top() + egui::vec2(12.0, 112.0), egui::Align2::LEFT_TOP, title, egui::FontId::proportional(14.0), v.text_color());
            p.text(rect.left_top() + egui::vec2(12.0, 134.0), egui::Align2::LEFT_TOP, sub, egui::FontId::proportional(11.5), v.weak_text_color());
            resp
        };
        let q = h.search.trim().to_lowercase();
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new(tr("НЕДАВНИЕ ПРОЕКТЫ")).small().weak());
        });
        let shown: Vec<&String> = st.recent.iter().filter(|p| h.infos.get(*p).map(|i| q.is_empty() || i.name.to_lowercase().contains(&q)).unwrap_or(true)).collect();
        if shown.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new(if st.recent.is_empty() { tr("Пока нет проектов — создайте новый или откройте файл") } else { tr("Ничего не найдено") }).weak());
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.add_space(24.0);
            for p in shown {
                let Some(info) = h.infos.get(p).cloned() else { continue };
                let sub = if info.exists { format!("{} · {}", info.time, info.steps.map(|n| tf!("шагов: {}", n)).unwrap_or_else(|| tr("не читается").into())) } else { tr("файл не найден").into() };
                let r = card(ui, p.as_str(), &info.name, &sub).on_hover_text(p.as_str());
                if r.clicked() && info.exists {
                    action = HomeAction::OpenPath(PathBuf::from(p));
                }
                r.context_menu(|ui| {
                    if ui.button(tr("Убрать из списка")).clicked() {
                        action = HomeAction::Forget(p.clone());
                        ui.close_menu();
                    }
                });
            }
        });
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new(tr("ПРИМЕРЫ")).small().weak());
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_space(24.0);
            for (i, (name, sub)) in [(tr("Кронштейн (параметрический)"), tr("пластина с отверстиями и стойка")), (tr("Втулка (вращение)"), tr("деталь вращения"))].into_iter().enumerate() {
                if card(ui, &format!("ex:{i}"), name, sub).clicked() {
                    action = HomeAction::Example(i);
                }
            }
        });
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_keys() {
        let k = make_key("OSTV-DEMO-2026");
        assert!(license_valid(&k), "{k}");
        assert!(license_valid(&k.to_lowercase()));
        assert!(!license_valid("OSTV-DEMO-2026-0000"));
        assert!(!license_valid("abc"));
        assert!(license_valid("DEMO"));
        println!("DEMO KEY: {k}");
    }

    #[test]
    fn time_format() {
        assert_eq!(format_time(0), "1970-01-01 00:00");
        assert_eq!(format_time(1_700_000_000), "2023-11-14 22:13");
    }
}
