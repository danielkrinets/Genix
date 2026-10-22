//! «Сообщить о проблеме»: a short form plus the crash logs found on this computer, with open / copy / send.
use crate::i18n::tr;
use eframe::egui::{self, Color32, RichText};

pub struct ReportState {
    pub open: bool,
    /// Opened right after the app found a log from a previous crash.
    pub after_crash: bool,
    text: String,
    attach_system: bool,
    selected: Option<usize>,
    note: Option<String>,
}

impl Default for ReportState {
    fn default() -> Self {
        ReportState { open: false, after_crash: false, text: String::new(), attach_system: true, selected: Some(0), note: None }
    }
}

impl ReportState {
    pub fn show(&mut self, after_crash: bool) {
        self.open = true;
        self.after_crash = after_crash;
        self.selected = Some(0);
        self.note = None;
    }
}

fn full_text(st: &ReportState, crashes: &[crate::crash::Entry]) -> String {
    let mut out = String::new();
    if !st.text.trim().is_empty() {
        out.push_str(st.text.trim());
        out.push_str("\n\n");
    }
    if st.attach_system {
        out.push_str(&crate::crash::system_info());
        out.push_str("\n\n");
    }
    if let Some(e) = st.selected.and_then(|i| crashes.get(i)) {
        out.push_str(&format!("--- {} ---\n{}", e.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), crate::crash::read(&e.path)));
    }
    out
}

/// Draws the window when it is open. Returns nothing; sending opens the browser and copies the full text.
pub fn window(ctx: &egui::Context, st: &mut ReportState) {
    if !st.open { return; }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { st.open = false; return; }
    let crashes = crate::crash::list();
    // a soft dim behind the card
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("report-dim")).order(egui::Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
        let (rect, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(110));
        if resp.clicked() { st.open = false; }
    });
    let surface = crate::theme::surface_for(ctx);
    egui::Window::new("report-window").title_bar(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).resizable(false).collapsible(false).order(egui::Order::Foreground)
        .frame(egui::Frame::new().fill(surface).corner_radius(26).inner_margin(egui::Margin::same(26)).shadow(egui::Shadow { offset: [0, 10], blur: 40, spread: 0, color: Color32::from_black_alpha(90) }))
        .show(ctx, |ui| {
            ui.set_width(520.0);
            ui.horizontal(|ui| {
                if let Some(img) = crate::icons::icon("report", 28.0, crate::theme::accent()) { ui.add(img); }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(tr("Сообщить о проблеме")).size(22.0).strong());
                    ui.label(RichText::new(tr("Опишите, что пошло не так — мы разберёмся")).color(crate::theme::secondary(ui)));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame(false)).on_hover_text("Esc").clicked() { st.open = false; }
                });
            });
            if st.after_crash {
                ui.add_space(12.0);
                egui::Frame::new().fill(crate::theme::accent().gamma_multiply(0.16)).corner_radius(14).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(tr("Genix закрылась неожиданно. Отчёт уже сохранён — отправьте его, чтобы мы это исправили.")).color(ui.visuals().text_color()));
                });
            }
            ui.add_space(16.0);
            ui.label(RichText::new(tr("ЧТО ПРОИЗОШЛО")).small().strong().color(crate::theme::secondary(ui)));
            ui.add_space(4.0);
            ui.add(egui::TextEdit::multiline(&mut st.text).desired_rows(4).desired_width(f32::INFINITY).margin(egui::Margin::symmetric(12, 10)).hint_text(tr("Что вы делали и что ожидали увидеть?")));
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(tr("ОТЧЁТЫ О СБОЯХ")).small().strong().color(crate::theme::secondary(ui)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(dir) = crate::crash::dir() {
                        if crate::ui_kit::b03(ui, tr("Открыть папку")).clicked() {
                            let _ = std::fs::create_dir_all(&dir);
                            crate::crash::open_with_system(&dir.to_string_lossy());
                        }
                    }
                });
            });
            ui.add_space(4.0);
            if crashes.is_empty() {
                ui.label(RichText::new(tr("Сбоев не было — приложать нечего.")).color(crate::theme::secondary(ui)));
            }
            for (i, e) in crashes.iter().take(5).enumerate() {
                let selected = st.selected == Some(i);
                let fill = if selected { crate::theme::accent().gamma_multiply(0.16) } else { ui.visuals().extreme_bg_color };
                let row = egui::Frame::new().fill(fill).corner_radius(14).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (dot, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        ui.painter().circle_stroke(dot.center(), 7.0, egui::Stroke::new(1.6_f32, if selected { crate::theme::accent() } else { crate::theme::secondary(ui) }));
                        let t = crate::widgets::feedback_duration(ui, egui::Id::new(("report-radio", i)), selected, 0.2, true);
                        if t > 0.0 { ui.painter().circle_filled(dot.center(), 4.0 * t, crate::theme::accent()); }
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let title: String = e.title.chars().take(54).collect();
                            ui.label(RichText::new(if title.is_empty() { "—".into() } else { title }).strong());
                            ui.label(RichText::new(crate::crash::format_when(e.when)).small().color(crate::theme::secondary(ui)));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::ui_kit::b03(ui, tr("Открыть")).on_hover_text(tr("Открыть файл отчёта")).clicked() {
                                crate::crash::open_with_system(&e.path.to_string_lossy());
                            }
                        });
                    });
                });
                if ui.interact(row.response.rect, ui.id().with(("crash-row", i)), egui::Sense::click()).clicked() {
                    st.selected = if selected { None } else { Some(i) };
                }
                ui.add_space(4.0);
            }
            ui.add_space(10.0);
            crate::widgets::toggle(ui, &mut st.attach_system, tr("Приложить сведения о системе"));
            ui.label(RichText::new(tr("Версия программы, система и язык. Ваши модели не отправляются.")).small().color(crate::theme::secondary(ui)));
            if let Some(n) = &st.note {
                ui.add_space(8.0);
                ui.label(RichText::new(n.as_str()).color(crate::theme::accent()));
            }
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                if crate::ui_kit::b01(ui, tr("Отправить на GitHub"), 200.0).on_hover_text(tr("Откроется страница нового обращения, полный отчёт будет в буфере обмена")).clicked() {
                    let full = full_text(st, &crashes);
                    let title = match st.selected.and_then(|i| crashes.get(i)) {
                        Some(e) => format!("Crash: {}", e.title.chars().take(70).collect::<String>()),
                        None => format!("Problem: {}", st.text.lines().next().unwrap_or("").chars().take(70).collect::<String>()),
                    };
                    ctx.copy_text(full.clone());
                    crate::crash::open_with_system(&crate::crash::issue_url(&title, &full));
                    st.note = Some(tr("Открыл GitHub. Полный отчёт скопирован — вставьте его, если текст обрезан.").to_string());
                }
                if crate::ui_kit::b02(ui, tr("Скопировать отчёт")).clicked() {
                    ctx.copy_text(full_text(st, &crashes));
                    st.note = Some(tr("Отчёт скопирован в буфер обмена.").to_string());
                }
                if crate::ui_kit::b03(ui, tr("Закрыть")).clicked() { st.open = false; }
            });
        });
}
