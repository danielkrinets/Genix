//! Вход по лицензии и в аккаунт (демо) и главная страница со всеми проектами.

use crate::i18n::tr;
use crate::settings::{ControlScheme, Settings, ThemeChoice, VisualStyle};
use crate::tf;

use eframe::egui::{self, Color32, RichText};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const LOGO_PNG: &[u8] = include_bytes!("../assets/logo-small.png");

/// Genix app mark; the decoded texture is created once and kept in egui memory.
pub fn logo(ui: &mut egui::Ui, size: f32) {
    let key = egui::Id::new("genix-logo-texture");
    let texture = match ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        Some(texture) => texture,
        None => {
            let Ok(img) = image::load_from_memory(LOGO_PNG) else { return };
            let img = img.to_rgba8();
            let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            let texture = ui.ctx().load_texture("genix-logo", color, egui::TextureOptions::LINEAR);
            ui.ctx().data_mut(|d| d.insert_temp(key, texture.clone()));
            texture
        }
    };
    ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(size, size)));
}

pub fn unlocked(st: &Settings) -> bool {
    st.account.is_some() || st.account_skipped
}

#[derive(Default)]
pub struct Gate {
    name: String,
    email: String,
    pass: String,
    repeat: String,
    login: bool,
    msg: Option<String>,
}

pub(crate) fn valid_email(email: &str) -> bool {
    let email = email.trim();
    let Some((local, host)) = email.split_once('@') else { return false };
    !local.is_empty() && host.contains('.') && !host.starts_with('.') && !host.ends_with('.')
        && !email.chars().any(char::is_whitespace) && !host.contains('@') && email.len() <= 254
}

impl Gate {
    fn submit(&mut self, st: &mut Settings) -> bool {
        let error = if !self.login && self.name.trim().is_empty() {
            Some(tr("Введите имя"))
        } else if !valid_email(&self.email) {
            Some(tr("Введите корректную электронную почту"))
        } else if self.pass.chars().count() < 8 {
            Some(tr("Пароль должен содержать не менее 8 символов"))
        } else if !self.login && self.pass != self.repeat {
            Some(tr("Пароли не совпадают"))
        } else { None };
        self.msg = error.map(str::to_owned);
        if self.msg.is_some() { return false }
        st.account = Some(self.email.trim().to_lowercase());
        st.account_skipped = false;
        st.license.get_or_insert_with(|| "DEMO".into());
        st.display_name = if self.name.trim().is_empty() {
            self.email.split('@').next().unwrap_or_default().to_owned()
        } else { self.name.trim().to_owned() };
        self.pass.clear();
        self.repeat.clear();
        true
    }
}

/// Локальная демонстрация регистрации: пароли не отправляются и не сохраняются.
pub fn gate_ui(ctx: &egui::Context, st: &mut Settings, g: &mut Gate) -> bool {
    let mut changed = false;
    let mut shy = false;
    egui::CentralPanel::default().show(ctx, |ui| {
        crate::backgrounds::paint(ui, ui.max_rect(), true);
        let available_width = ui.available_width();
        let wide = available_width >= 980.0;
        let card_width = if wide { 500.0 } else { (available_width - 32.0).max(300.0) };
        let card_left = if wide { available_width * 0.75 - card_width * 0.5 } else { (available_width - card_width) * 0.5 };
        egui::ScrollArea::vertical().show(ui, |ui| {
            let top_space = if wide { ((ui.available_height() - 700.0) * 0.5).max(24.0) } else { 20.0 };
            ui.add_space(top_space);
            ui.horizontal(|ui| {
            ui.add_space(card_left.max(0.0));
            ui.allocate_ui_with_layout(egui::vec2(card_width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(26).inner_margin(egui::Margin::same(28)).show(ui, |ui| {
                    ui.set_width((card_width - 56.0).max(240.0));
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.heading(if g.login { tr("С возвращением") } else { tr("Создать аккаунт") });
                    ui.add_space(2.0);
                    ui.label(RichText::new(tr("Мок регистрации · данные остаются на этом устройстве")).color(muted(ui)));
                    ui.add_space(14.0);
                    if !g.login {
                        ui.label(tr("Имя"));
                        ui.add(egui::TextEdit::singleline(&mut g.name).font(egui::FontId::proportional(16.0)).margin(egui::Margin::symmetric(14,12)).char_limit(100).desired_width(f32::INFINITY));
                        ui.add_space(3.0);
                    }
                    ui.label(tr("Электронная почта"));
                    ui.add(egui::TextEdit::singleline(&mut g.email).font(egui::FontId::proportional(16.0)).margin(egui::Margin::symmetric(14,12)).hint_text(RichText::new("you@example.com").color(muted(ui))).char_limit(254).desired_width(f32::INFINITY));
                    ui.add_space(3.0);
                    ui.label(tr("Пароль"));
                    let password = ui.add(egui::TextEdit::singleline(&mut g.pass).font(egui::FontId::proportional(16.0)).margin(egui::Margin::symmetric(14,12)).password(true).char_limit(128).hint_text(RichText::new(tr("Не менее 8 символов")).color(muted(ui))).desired_width(f32::INFINITY));
                    let mut enter = password.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    shy |= password.has_focus();
                    if !g.login {
                        ui.add_space(3.0);
                        ui.label(tr("Повторите пароль"));
                        let repeat = ui.add(egui::TextEdit::singleline(&mut g.repeat).font(egui::FontId::proportional(16.0)).margin(egui::Margin::symmetric(14,12)).password(true).char_limit(128).desired_width(f32::INFINITY));
                        enter = repeat.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        shy |= repeat.has_focus();
                    }
                    if let Some(msg) = &g.msg { ui.colored_label(ui.visuals().error_fg_color, msg); }
                    ui.add_space(8.0);
                    if primary(ui, if g.login { tr("Войти") } else { tr("Зарегистрироваться") }).clicked() || enter {
                        changed = g.submit(st);
                    }
                    ui.add_space(2.0);
                    ui.vertical_centered(|ui| {
                    if crate::ui_kit::b03(ui, if g.login { tr("Нет аккаунта? Зарегистрироваться") } else { tr("Уже есть аккаунт? Войти") }).clicked() {
                        g.login = !g.login;
                        g.msg = None;
                        g.pass.clear();
                        g.repeat.clear();
                    }
                    if crate::ui_kit::b03(ui, tr("Продолжить без аккаунта")).clicked() {
                        st.account = None;
                        st.account_skipped = true;
                        st.license.get_or_insert_with(|| "DEMO".into());
                        g.pass.clear();
                        g.repeat.clear();
                        changed = true;
                    }
                    });
                    ui.add_space(4.0);
                    ui.vertical_centered(|ui| ui.label(RichText::new(tr("Это демонстрация: аккаунт не создаётся на сервере. Используйте тестовый пароль.")).color(muted(ui)).size(12.0)));
                    });
            });
            });
        });
        if wide {
            let rect = ui.max_rect();
            let width = (rect.width() * 0.4).min(rect.height() * 0.7).clamp(220.0, 560.0);
            crate::mascot::paint(ui, egui::pos2(rect.left() + rect.width() * 0.255, rect.bottom() - 28.0), width, shy);
        }
    });
    changed
}

/// Читаемый вторичный текст на светлых и тёмных поверхностях библиотеки.
pub(crate) fn muted(ui: &egui::Ui) -> Color32 { crate::theme::secondary(ui) }

fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    crate::ui_kit::b01(ui, text, ui.available_width())
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
    modified: Option<u64>,
    steps: Option<usize>,
    exists: bool,
    preview_key: String,
}

#[derive(Default)]
struct ProjectFilters {
    in_path: bool,
    available_only: bool,
    readable_only: bool,
    min_steps: usize,
    modified_days: u64,
    sort: u8,
}

impl ProjectFilters {
    fn matches(&self, query: &str, path: &str, info: &ProjInfo, now: u64) -> bool {
        let text = if self.in_path { format!("{} {}", info.name, path) } else { info.name.clone() }.to_lowercase();
        query.split_whitespace().all(|word| text.contains(word))
            && (!self.available_only || info.exists)
            && (!self.readable_only || info.steps.is_some())
            && (self.min_steps == 0 || info.steps.is_some_and(|n| n >= self.min_steps))
            && (self.modified_days == 0 || info.modified.is_some_and(|t| now.saturating_sub(t) <= self.modified_days * 86400))
    }

    fn active(&self) -> bool {
        self.in_path || self.available_only || self.readable_only || self.min_steps > 0 || self.modified_days > 0 || self.sort != 0
    }
}

#[derive(Default)]
pub struct HomeState {
    page: HomePage,
    transition_started: Option<f64>,
    profile: Option<ProfileDraft>,
    checkout: bool,
    updates: crate::updates::Updates,
    search: String,
    filters_open: bool,
    filters: ProjectFilters,
    infos: HashMap<String, ProjInfo>,
    thumbs: HashMap<String, egui::TextureHandle>,
    pending: PendingFrames,
    queued: VecDeque<(String, fusion_core::Document)>,
    inflight: HashSet<String>,
    completed: HashSet<String>,
    busy: Arc<AtomicUsize>,
    new_project_started: Option<f64>,
    sidebar_pinned: bool,
    open_folder: Option<String>,
    focus_rename: bool,
    fab_open: bool,
    fab_toggled_at: f64,
    name_dialog: Option<NameDialog>,
    edit_mode: bool,
    edit_since: f64,
    selected: HashSet<String>,
    hold_since: Option<f64>,
    hover_since: Option<(String, f64)>,
    spin_started: HashSet<String>,
    /// Models whose 360° frames are in memory, oldest first: only the last few are kept.
    spin_order: VecDeque<String>,
    share_path: Option<String>,
    share_email: String,
    share_editor: bool,
    share_msg: Option<&'static str>,
    team_email: String,
    team_editor: bool,
    team_msg: Option<&'static str>,
    folder_rename: String,
}

/// Отрисовать превью документа на фоновом потоке (прозрачный фон, изометрия).
fn spawn_thumb(key: String, doc: fusion_core::Document, pending: PendingFrames, busy: Arc<AtomicUsize>) {
    busy.fetch_add(1, Ordering::SeqCst);
    std::thread::spawn(move || {
        let img = std::panic::catch_unwind(|| {
            let mut ev = fusion_core::Evaluator::default();
            ev.evaluate(&doc);
            let model = ev.model();
            let meshes: Vec<&fusion_core::Mesh> = model.bodies.iter().filter(|b| !doc.hidden_bodies.contains(&b.id)).map(|b| b.mesh()).collect();
            crate::thumb::render(&meshes, crate::camera::Camera::ISO.0, crate::camera::Camera::ISO.1, 528, 352)
        }).ok().flatten();
        pending.lock().unwrap().push((key, img));
        busy.fetch_sub(1, Ordering::SeqCst);
    });
}

/// Rendered preview frames travelling from worker threads to the UI thread.
type PendingFrames = Arc<Mutex<Vec<(String, Option<crate::thumb::Rgba>)>>>;

const SPIN_FRAMES: usize = 24;
const SPIN_MODELS: usize = 4;

/// Окно выбора имени для новой модели или папки.
struct NameDialog {
    folder: bool,
    text: String,
}

/// Where a model for the 360° preview comes from.
enum SpinSource {
    File(String),
    Example(usize),
}

/// Render `SPIN_FRAMES` views around the model on a background thread; frames arrive through `pending` as `key#spinN`.
fn spawn_spin(key: String, source: SpinSource, pending: PendingFrames) {
    std::thread::spawn(move || {
        let doc = match source {
            SpinSource::File(path) => std::fs::read_to_string(path).ok().and_then(|s| fusion_core::Document::from_json(&s).ok()),
            SpinSource::Example(i) => Some(if i == 0 { crate::examples::bracket() } else { crate::examples::bushing() }),
        };
        let Some(doc) = doc else { return };
        let _ = std::panic::catch_unwind(|| {
            let mut ev = fusion_core::Evaluator::default();
            ev.evaluate(&doc);
            let model = ev.model();
            let meshes: Vec<&fusion_core::Mesh> = model.bodies.iter().filter(|b| !doc.hidden_bodies.contains(&b.id)).map(|b| b.mesh()).collect();
            for i in 0..SPIN_FRAMES {
                let yaw = crate::camera::Camera::ISO.0 + std::f64::consts::TAU * i as f64 / SPIN_FRAMES as f64;
                let frame = crate::thumb::render(&meshes, yaw, crate::camera::Camera::ISO.1, 396, 264);
                pending.lock().unwrap().push((format!("{key}#spin{i}"), frame));
            }
        });
    });
}

pub enum HomeAction {
    None,
    New,
    /// Новый проект сразу в выбранной папке.
    NewIn(String),
    /// Новая модель с выбранным именем (и папкой, если создаём внутри неё).
    NewNamed(String, Option<String>),
    Open,
    OpenPath(PathBuf),
    Example(usize),
    Forget(String),
    SettingsChanged,
    SignOut,
}

impl HomeState {
    /// Shows the inside of a folder (used by QA to open one without clicking).
    pub fn open_folder(&mut self, name: &str) {
        self.page = HomePage::Models;
        self.open_folder = Some(name.to_string());
        self.folder_rename = name.to_string();
    }

    /// Opens the name prompt for a new folder or model (used by the «+» button and the File menu).
    pub fn open_name_dialog(&mut self, folder: bool) {
        self.page = HomePage::Models;
        self.name_dialog = Some(NameDialog { folder, text: String::new() });
    }

    /// Перечитать сведения о проектах (при заходе на главную).
    pub fn refresh(&mut self, recent: &[String]) {
        self.infos.clear();
        self.queued.clear();
        self.spin_started.clear();
        self.spin_order.clear();
        let mut current = HashSet::new();
        for p in recent {
            let path = PathBuf::from(p);
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
            let meta = std::fs::metadata(&path).ok();
            let modified = meta.as_ref().and_then(|m| m.modified().ok());
            let time = modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| format_time(d.as_secs())).unwrap_or_default();
            let preview_key = format!("{p}:{modified:?}:{}", meta.as_ref().map_or(0, |m| m.len()));
            let doc = std::fs::read_to_string(&path).ok().and_then(|s| fusion_core::Document::from_json(&s).ok());
            let steps = doc.as_ref().map(|d| d.features.len());
            current.insert(preview_key.clone());
            if !self.thumbs.contains_key(&preview_key) && !self.inflight.contains(&preview_key) {
                if let Some(doc) = doc { self.queued.push_back((preview_key.clone(), doc)); }
                else { self.completed.insert(preview_key.clone()); }
            }
            self.infos.insert(p.clone(), ProjInfo { name, time, modified: modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()), steps, exists: meta.is_some(), preview_key });
        }
        for (i, example) in [crate::examples::bracket as fn() -> fusion_core::Document, crate::examples::bushing].into_iter().enumerate() {
            let key = format!("ex:{i}");
            current.insert(key.clone());
            if !self.thumbs.contains_key(&key) && !self.inflight.contains(&key) {
                self.queued.push_back((key, example()));
            }
        }
        self.thumbs.retain(|key, _| current.contains(key));
        self.completed.retain(|key| current.contains(key));
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HomePage {
    #[default]
    Models,
    Profile,
    Team,
    Updates,
    Server,
    Settings,
}

impl HomePage {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "models" => Self::Models,
            "profile" => Self::Profile,
            "team" => Self::Team,
            "updates" => Self::Updates,
            "server" => Self::Server,
            "settings" => Self::Settings,
            _ => return None,
        })
    }

    fn label(self) -> &'static str {
        match self {
            Self::Models => "Мои модели",
            Self::Profile => "Профиль",
            Self::Team => "Team",
            Self::Updates => "Обновления",
            Self::Server => "Сервер",
            Self::Settings => "Настройки",
        }
    }
}

#[derive(Default)]
struct ProfileDraft {
    name: String,
    email: String,
    message: Option<String>,
}

impl HomeState {
    pub fn navigate(&mut self, page: HomePage) {
        if self.page != page {
            self.profile = None;
            self.checkout = false;
        }
        self.page = page;
        self.transition_started = None;
    }
}

fn nav(ui: &mut egui::Ui, h: &mut HomeState, expanded: bool) {
    for (page, icon) in [(HomePage::Models, "home_models"), (HomePage::Team, "profile"), (HomePage::Updates, "updates"), (HomePage::Server, "server"), (HomePage::Settings, "settings")] {
        let selected = h.page == page;
        let response = crate::widgets::navigation_button_mode(ui, tr(page.label()), icon, selected, expanded);
        if response.clicked() && !selected {
            h.navigate(page);
            if ui.input(|i| i.pointer.any_released()) { h.transition_started = Some(ui.input(|i| i.time)); }
        }
    }
}

pub fn home_ui(ui: &mut egui::Ui, h: &mut HomeState, st: &mut Settings, projects: &[String], viewer: bool) -> HomeAction {
    let mut action = HomeAction::None;
    if ui.available_width() < 700.0 {
        ui.horizontal_wrapped(|ui| nav(ui, h, true));
        ui.separator();
        let content = hub_content(ui, h, st, projects, viewer);
        if !matches!(content, HomeAction::None) { action = content; }
    } else {
        let height = ui.available_height();
        let expanded = h.sidebar_pinned;
        let open = crate::widgets::feedback(ui, ui.id().with("home-sidebar-open"), expanded);
        let nav_width = 64.0 + (190.0 - 64.0) * open;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(nav_width, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(22).inner_margin(egui::Margin::symmetric(10, 14)).show(ui, |ui| {
                    ui.set_width(nav_width - 20.0);
                    ui.set_min_height(height - 28.0);
                    ui.horizontal(|ui| {
                        let (rect, toggle) = ui.allocate_exact_size(egui::vec2(36.0, 34.0), egui::Sense::click());
                        if toggle.hovered() { ui.painter().rect_filled(rect, 12.0, crate::theme::tint(20)); }
                        if let Some(icon) = crate::icons::icon("nav_panel_left", 22.0, ui.visuals().text_color()) {
                            icon.paint_at(ui, egui::Rect::from_center_size(rect.center(), egui::vec2(22.0, 22.0)));
                        }
                        if toggle.on_hover_text(if h.sidebar_pinned { tr("Свернуть панель") } else { tr("Развернуть панель") }).clicked() { h.sidebar_pinned = !h.sidebar_pinned; }
                        if open > 0.72 { ui.label(RichText::new("Genix").size(22.0).strong()); }
                    });
                    ui.add_space(22.0);
                    ui.spacing_mut().item_spacing.y = 6.0;
                    nav(ui, h, open > 0.72);
                    ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                        let name = if st.account.is_some() { st.display_name.clone() } else { tr("Локальное пространство").to_string() };
                        ui.horizontal(|ui| {
                            let (rect, avatar) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
                            ui.painter().circle_filled(rect.center(), 18.0, crate::theme::tint(if avatar.hovered() { 80 } else { 55 }));
                            let initial = name.chars().next().map_or('G', |c| c.to_ascii_uppercase());
                            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(16.0), crate::theme::accent());
                            if avatar.clicked() { h.navigate(HomePage::Profile); }
                            if open > 0.72 {
                                ui.vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label(RichText::new(name).strong());
                                    ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).color(muted(ui)).small());
                                });
                            }
                        });
                    });
                });
            });
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                let content = hub_content(ui, h, st, projects, viewer);
                if !matches!(content, HomeAction::None) { action = content; }
            });
        });
    }
    action
}

fn hub_content(ui: &mut egui::Ui, h: &mut HomeState, st: &mut Settings, projects: &[String], viewer: bool) -> HomeAction {
    let progress = if crate::theme::reduced_motion() { 1.0 } else {
        h.transition_started.map(|start| ((ui.input(|i| i.time) - start) / 0.16).clamp(0.0, 1.0) as f32).unwrap_or(1.0)
    };
    if progress < 1.0 { ui.ctx().request_repaint(); }
    ui.scope(|ui| {
        ui.multiply_opacity(0.65 + 0.35 * crate::widgets::ease_out(progress));
        hub_content_inner(ui, h, st, projects, viewer)
    }).inner
}

fn hub_content_inner(ui: &mut egui::Ui, h: &mut HomeState, st: &mut Settings, projects: &[String], viewer: bool) -> HomeAction {
    if h.page == HomePage::Models {
        crate::backgrounds::paint(ui, ui.max_rect(), false);
        return projects_ui(ui, h, st, projects, viewer)
    }
    let mut action = HomeAction::None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(crate::theme::SURFACE_RADIUS).inner_margin(egui::Margin::same(28)).show(ui, |ui| {
            ui.set_max_width(640.0);
            match h.page {
                HomePage::Profile => {
                    let draft = h.profile.get_or_insert_with(|| ProfileDraft {
                        name: st.display_name.clone(), email: st.account.clone().unwrap_or_default(), message: None,
                    });
                    ui.heading(tr("Профиль"));
                    ui.label(RichText::new(tr("Локальный аккаунт · мок")).color(muted(ui)));
                    ui.add_space(24.0);
                    if st.account.is_none() {
                        ui.label(tr("Вы работаете без аккаунта. Модели доступны на этом устройстве."));
                        if ui.button(tr("Зарегистрироваться / войти")).clicked() { action = HomeAction::SignOut; }
                    } else {
                        ui.label(tr("Имя"));
                        ui.add(egui::TextEdit::singleline(&mut draft.name).char_limit(100).desired_width(360.0));
                        ui.add_space(12.0);
                        ui.label(tr("Электронная почта"));
                        ui.add(egui::TextEdit::singleline(&mut draft.email).char_limit(254).desired_width(360.0));
                        ui.add_space(16.0);
                        if ui.button(tr("Сохранить профиль")).clicked() {
                            if draft.name.trim().is_empty() || !valid_email(&draft.email) {
                                draft.message = Some(tr("Введите имя и корректную почту").into());
                            } else {
                                let email = draft.email.trim().to_lowercase();
                                if st.server_demo_account == st.account { st.server_demo_account = Some(email.clone()); }
                                st.display_name = draft.name.trim().to_owned();
                                st.account = Some(email);
                                draft.message = Some(tr("Профиль сохранён на этом устройстве").into());
                                action = HomeAction::SettingsChanged;
                            }
                        }
                        if let Some(message) = &draft.message { ui.label(message); }
                        ui.add_space(24.0);
                        ui.separator();
                        ui.label(tr("Пароль не хранится. Реальная авторизация будет подключена позже."));
                        if ui.button(tr("Выйти из аккаунта")).clicked() { action = HomeAction::SignOut; }
                    }
                }
                HomePage::Team => team_page(ui, h, st, &mut action),
                HomePage::Updates => h.updates.ui(ui),
                HomePage::Server => {
                    ui.heading(tr("Сервер Genix"));
                    ui.label(RichText::new(tr("Демонстрационная подписка")).color(muted(ui)));
                    ui.add_space(24.0);
                    ui.label(RichText::new(tr("Ваше пространство для моделей")).size(20.0).strong());
                    ui.label(tr("Будущий сервер для хранения проектов и работы с ними на разных устройствах."));
                    ui.add_space(12.0);
                    ui.label(tr("Сейчас сервер не подключён. Модели хранятся только на этом устройстве."));
                    ui.label(tr("Цена и тарифы пока не определены. Оплата не производится."));
                    ui.add_space(24.0);
                    let active = st.account.is_some() && st.server_demo_account == st.account;
                    if active {
                        ui.label(RichText::new(tr("Демо-подписка активна")).strong());
                        ui.label(tr("Это локальный статус для проверки интерфейса. Загрузка на сервер недоступна."));
                        ui.add_space(12.0);
                        if st.ticket.is_none() {
                            st.ticket = Some(crate::settings::Ticket { number: crate::ticket::number_for(st.account.as_deref().unwrap_or_default()), activated: false });
                        }
                        if let Some(ticket) = st.ticket.clone() {
                            if crate::ticket::show(ui, &ticket.number, ticket.activated) {
                                st.ticket = Some(crate::settings::Ticket { activated: true, ..ticket });
                                action = HomeAction::SettingsChanged;
                            }
                        }
                        ui.add_space(12.0);
                        if crate::ui_kit::b04(ui, tr("Отключить демо-подписку")).clicked() {
                            st.server_demo_account = None;
                            st.ticket = None;
                            action = HomeAction::SettingsChanged;
                        }
                    } else if st.account.is_none() {
                        ui.label(tr("Для демо-подписки войдите в аккаунт."));
                        if crate::ui_kit::b02(ui, tr("Перейти в профиль")).clicked() { h.navigate(HomePage::Profile); }
                    } else if h.checkout {
                        ui.separator();
                        ui.label(RichText::new(tr("Подтверждение демо-покупки")).strong());
                        ui.label(tr("Деньги не списываются. Банковские данные не требуются."));
                        if crate::ui_kit::b01(ui, tr("Подтвердить демо-подписку"), 260.0).clicked() {
                            st.server_demo_account = st.account.clone();
                            h.checkout = false;
                            action = HomeAction::SettingsChanged;
                        }
                        if crate::ui_kit::b03(ui, tr("Отмена")).clicked() { h.checkout = false; }
                    } else if primary(ui, tr("Купить подписку на сервер (мок)")).clicked() {
                        h.checkout = true;
                    }
                }
                HomePage::Settings => settings_page(ui, st, &mut action),
                HomePage::Models => {},
            }
        });
    });
    action
}

fn settings_page(ui: &mut egui::Ui, st: &mut Settings, action: &mut HomeAction) {
    let before = st.clone();
    ui.heading(tr("Настройки"));
    ui.label(RichText::new(tr("Параметры интерфейса, редактора и локального аккаунта")).color(muted(ui)));
    ui.add_space(22.0);

    crate::widgets::section(ui, tr("ВНЕШНИЙ ВИД"));
    egui::Grid::new("home-settings-appearance").num_columns(2).spacing([28.0, 12.0]).show(ui, |ui| {
        ui.label(tr("Тема"));
        egui::ComboBox::from_id_salt("home-theme").selected_text(match st.theme { ThemeChoice::Auto => tr("Как в системе"), ThemeChoice::Light => tr("Светлая"), ThemeChoice::Dark => tr("Тёмная") }).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.theme, ThemeChoice::Auto, tr("Как в системе"));
            ui.selectable_value(&mut st.theme, ThemeChoice::Light, tr("Светлая"));
            ui.selectable_value(&mut st.theme, ThemeChoice::Dark, tr("Тёмная"));
        });
        ui.end_row();
        ui.label(tr("Акцентный цвет"));
        ui.horizontal(|ui| { for color in crate::theme::Accent::ALL { if crate::widgets::accent_swatch(ui, color, st.accent == color).clicked() { st.accent = color; } } });
        ui.end_row();
        ui.label(tr("Масштаб интерфейса"));
        crate::widgets::scale_slider(ui, &mut st.ui_scale);
        ui.end_row();
        ui.label(tr("Анимации"));
        crate::widgets::toggle(ui, &mut st.reduce_motion, tr("Уменьшить движение"));
        ui.end_row();
    });

    ui.add_space(22.0);
    crate::widgets::section(ui, tr("РЕДАКТОР"));
    egui::Grid::new("home-settings-editor").num_columns(2).spacing([28.0, 12.0]).show(ui, |ui| {
        ui.label(tr("Стиль отображения"));
        egui::ComboBox::from_id_salt("home-vstyle").selected_text(tr(st.visual_style.label())).show_ui(ui, |ui| {
            for style in VisualStyle::ALL { ui.selectable_value(&mut st.visual_style, style, tr(style.label())); }
        });
        ui.end_row();
        ui.label(tr("Схема управления"));
        ui.horizontal(|ui| {
            ui.selectable_value(&mut st.controls, ControlScheme::Standard, tr("Стандартная"));
            ui.selectable_value(&mut st.controls, ControlScheme::Blender, "Blender");
        });
        ui.end_row();
        ui.label(tr("Сетка"));
        crate::widgets::toggle(ui, &mut st.show_grid, tr("показывать"));
        ui.end_row();
        ui.label(tr("Привязка в чертеже"));
        crate::widgets::toggle(ui, &mut st.snap_grid, tr("включена"));
        ui.end_row();
        ui.label(tr("Толщина рёбер"));
        ui.add(egui::Slider::new(&mut st.edge_width, 0.8..=3.0).fixed_decimals(1));
        ui.end_row();
        ui.label(tr("Угол обзора"));
        ui.add(egui::Slider::new(&mut st.fov_deg, 10.0..=70.0).suffix("°").fixed_decimals(0));
        ui.end_row();
        ui.label(tr("Материал новых тел"));
        const MATERIALS: [&str; 7] = ["Сталь", "Нерж. сталь", "Алюминий", "Латунь", "PLA (3D-печать)", "ABS/PETG", "Дерево (сосна)"];
        egui::ComboBox::from_id_salt("home-material").selected_text(tr(MATERIALS[st.material.min(MATERIALS.len()-1)])).show_ui(ui, |ui| {
            for (index, name) in MATERIALS.iter().enumerate() { ui.selectable_value(&mut st.material, index, tr(name)); }
        });
        ui.end_row();
        ui.label(tr("Автосохранение"));
        ui.add(egui::DragValue::new(&mut st.autosave_minutes).range(0..=60).suffix(tr(" мин")));
        ui.end_row();
    });

    ui.add_space(22.0);
    crate::widgets::section(ui, tr("АККАУНТ"));
    ui.label(st.account.as_deref().unwrap_or(tr("Локальное пространство")));
    ui.label(RichText::new(tr("Выход не удаляет локальные проекты.")).color(muted(ui)).small());
    if ui.add(egui::Button::new(RichText::new(tr("Выйти из аккаунта")).color(Color32::from_rgb(220, 70, 70)))).clicked() {
        *action = HomeAction::SignOut;
    }

    if *st != before && matches!(action, HomeAction::None) { *action = HomeAction::SettingsChanged; }
}

fn projects_ui(ui: &mut egui::Ui, h: &mut HomeState, st: &mut Settings, projects: &[String], viewer: bool) -> HomeAction {
    let mut action = HomeAction::None;
    let mut move_to: Option<(String, Option<usize>)> = None;
    let mut share_req: Option<String> = None;
    let mut toggle: Option<String> = None;
    let mut hovered: Option<(String, SpinSource)> = None;
    let folder_idx = h.open_folder.as_ref().and_then(|n| st.folders.iter().position(|f| &f.name == n));
    if folder_idx.is_none() { h.open_folder = None; }
    let ready: Vec<_> = std::mem::take(&mut *h.pending.lock().unwrap());
    for (k, img) in ready {
        h.inflight.remove(&k);
        h.completed.insert(k.clone());
        if let Some(img) = img {
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.w, img.h], &img.data);
            h.thumbs.insert(k.clone(), ui.ctx().load_texture(format!("thumb-{k}"), ci, egui::TextureOptions::LINEAR));
        }
    }
    while h.busy.load(Ordering::SeqCst) < 2 {
        let Some((key, doc)) = h.queued.pop_front() else { break };
        h.inflight.insert(key.clone());
        spawn_thumb(key, doc, h.pending.clone(), h.busy.clone());
    }
    if h.busy.load(Ordering::SeqCst) > 0 {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
    }
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let now_t = ui.input(|i| i.time);
        // blank space: a long press enters the move mode (as on iPhone), a right click opens the library menu
        let backdrop = ui.interact(ui.max_rect(), ui.id().with("library-backdrop"), egui::Sense::click_and_drag());
        if !h.edit_mode && backdrop.is_pointer_button_down_on() && backdrop.drag_delta().length() < 3.0 && !viewer {
            let started = *h.hold_since.get_or_insert(now_t);
            if now_t - started >= 0.6 { h.edit_mode = true; h.edit_since = now_t; h.hold_since = None; } else { ui.ctx().request_repaint(); }
        } else {
            h.hold_since = None;
        }
        if h.edit_mode && (ui.input(|i| i.key_pressed(egui::Key::Escape)) || backdrop.clicked() && now_t - h.edit_since > 0.5) { h.edit_mode = false; h.selected.clear(); }
        backdrop.context_menu(|ui| {
            if !viewer && crate::app::menu_item(ui, "add", tr("Новый проект")) { action = HomeAction::New; ui.close_menu(); }
            if !viewer && !cfg!(target_os = "ios") && crate::app::menu_item(ui, "file_open", tr("Открыть файл…")) { action = HomeAction::Open; ui.close_menu(); }
            if !viewer && crate::app::menu_item(ui, "tree_folder", tr("Создать папку")) {
                st.folders.push(crate::settings::ProjectFolder { name: unused_folder_name(&st.folders, tr("Новая папка")), ..Default::default() });
                action = HomeAction::SettingsChanged;
                ui.close_menu();
            }
            if !viewer && crate::app::menu_item(ui, "move", tr("Режим перемещения")) { h.edit_mode = true; h.edit_since = now_t; ui.close_menu(); }
            if crate::app::menu_item(ui, "search", tr("Фильтры")) { h.filters_open = !h.filters_open; ui.close_menu(); }
            if crate::app::menu_item(ui, "settings", tr("Настройки")) { h.navigate(HomePage::Settings); ui.close_menu(); }
        });
        ui.add_space(16.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.vertical(|ui| {
                if let Some(i) = folder_idx {
                    if crate::ui_kit::b02(ui, &format!("←  {}", tr("Все модели"))).clicked() { h.open_folder = None; }
                    ui.add_space(8.0);
                    ui.label(RichText::new(st.folders[i].name.as_str()).size(26.0).strong().background_color(ui.visuals().panel_fill));
                    ui.label(RichText::new(tf!("Моделей в папке: {}", st.folders[i].projects.len())).color(muted(ui)).background_color(ui.visuals().panel_fill));
                } else {
                    ui.label(RichText::new(tr("Мои модели")).size(26.0).strong().background_color(ui.visuals().panel_fill));
                    let who = tr("Локальные проекты · выберите модель, чтобы открыть редактор");
                    ui.label(RichText::new(who).color(muted(ui)).background_color(ui.visuals().panel_fill));
                }
            });
        });
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(24.0);
            // на iPhone только просмотр: создавать и открывать через диалоги нечего
            if !viewer {
                if let Some(started) = h.new_project_started {
                    new_project_button(ui, true);
                    if ui.input(|i| i.time) - started >= 0.55 {
                        h.new_project_started = None;
                        action = match &h.open_folder { Some(name) => HomeAction::NewIn(name.clone()), None => HomeAction::New };
                    } else {
                        ui.ctx().request_repaint();
                    }
                } else if new_project_button(ui, false).clicked() {
                    h.new_project_started = Some(ui.input(|i| i.time));
                    ui.ctx().request_repaint();
                }
                plus_button(ui, h);
            }
        });
        ui.add_space(16.0);
        ui.horizontal_wrapped(|ui| {
            ui.add_space(24.0);
            let search_width = (ui.available_width() - 24.0).clamp(220.0, 720.0);
            let right_pad: i8 = if h.search.is_empty() { 112 } else { 124 };
            let search = ui.add(egui::TextEdit::singleline(&mut h.search).font(egui::FontId::proportional(16.0)).margin(egui::Margin { left: 44, right: right_pad, top: 13, bottom: 13 }).hint_text(RichText::new(tr("Поиск проектов")).color(muted(ui))).desired_width(search_width));
            // the response rect is the text area only: rebuild the visible field from the margins
            let field = egui::Rect::from_min_max(egui::pos2(search.rect.left() - 44.0, search.rect.top() - 13.0), egui::pos2(search.rect.right() + f32::from(right_pad), search.rect.bottom() + 13.0));
            let center = field.left_center() + egui::vec2(24.0, -1.0);
            let stroke = egui::Stroke::new(1.8_f32, muted(ui));
            ui.painter().circle_stroke(center, 7.0, stroke);
            ui.painter().line_segment([center + egui::vec2(5.0, 5.0), center + egui::vec2(10.0, 10.0)], stroke);
            let active = h.filters_open || h.filters.active();
            let filters_label = if h.filters.active() { format!("{} •", tr("Фильтры")) } else { tr("Фильтры").to_string() };
            let zone = egui::Rect::from_min_max(egui::pos2(field.right() - 108.0, field.top() + 6.0), egui::pos2(field.right() - 8.0, field.bottom() - 6.0));
            let filters = ui.interact(zone, ui.id().with("filters-in-search"), egui::Sense::click());
            let ink = if active { crate::theme::accent() } else if filters.hovered() { ui.visuals().text_color() } else { muted(ui) };
            ui.painter().text(zone.center(), egui::Align2::CENTER_CENTER, format!("{filters_label} ▾"), egui::FontId::proportional(14.0), ink);
            if filters.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { h.filters_open = !h.filters_open; }
            if !h.search.is_empty() {
                let clear = ui.interact(egui::Rect::from_center_size(egui::pos2(zone.left() - 18.0, field.center().y), egui::vec2(24.0, 24.0)), ui.id().with("search-clear"), egui::Sense::click());
                ui.painter().text(clear.rect.center(), egui::Align2::CENTER_CENTER, "✕", egui::FontId::proportional(13.0), if clear.hovered() { ui.visuals().text_color() } else { muted(ui) });
                if clear.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { h.search.clear(); }
            }
        });
        let openness = crate::widgets::feedback(ui, ui.id().with("project-filters"), h.filters_open);
        if openness > 0.0 {
            let height_id = ui.id().with("filters-height");
            let height = ui.data(|d| d.get_temp::<f32>(height_id)).unwrap_or(200.0);
            let mut filter_ui = ui.new_child(egui::UiBuilder::new().id_salt("filter-body").max_rect(ui.available_rect_before_wrap()));
            {
                let ui = &mut filter_ui;
                let top = ui.next_widget_position().y;
                let mut clip = ui.clip_rect();
                clip.max.y = clip.max.y.min(top + height * openness);
                ui.set_clip_rect(clip);
                ui.set_opacity(openness);
                if !h.filters_open { ui.disable(); }
                egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(20).inner_margin(16).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        crate::widgets::checkbox(ui, &mut h.filters.in_path, tr("Искать в пути файла"));
                        crate::widgets::checkbox(ui, &mut h.filters.available_only, tr("Только доступные файлы"));
                        crate::widgets::checkbox(ui, &mut h.filters.readable_only, tr("Только читаемые проекты"));
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label(tr("Минимум операций"));
                        ui.add(egui::DragValue::new(&mut h.filters.min_steps).range(0..=100000));
                        egui::ComboBox::from_id_salt("modified-filter").selected_text(tr(match h.filters.modified_days { 7 => "За неделю", 30 => "За месяц", _ => "За всё время" })).show_ui(ui, |ui| {
                            for (days, label) in [(0,"За всё время"),(7,"За неделю"),(30,"За месяц")] { ui.selectable_value(&mut h.filters.modified_days, days, tr(label)); }
                        });
                        egui::ComboBox::from_id_salt("project-sort").selected_text(tr(match h.filters.sort { 1 => "Сначала новые", 2 => "По названию", 3 => "Больше операций", _ => "Недавно открытые" })).show_ui(ui, |ui| {
                            for (sort,label) in [(0,"Недавно открытые"),(1,"Сначала новые"),(2,"По названию"),(3,"Больше операций")] { ui.selectable_value(&mut h.filters.sort, sort, tr(label)); }
                        });
                        if ui.add_enabled(h.filters.active(), egui::Button::new(tr("Сбросить фильтры"))).clicked() { h.filters = ProjectFilters::default(); }
                    });
                });
                let rect = ui.min_rect();
                ui.data_mut(|d| d.insert_temp(height_id, rect.height()));

            }
            let mut rect = filter_ui.min_rect();
            rect.max.y = rect.min.y + rect.height() * openness;
            ui.allocate_rect(rect, egui::Sense::hover());
        }
        ui.add_space(14.0);
        if h.busy.load(Ordering::SeqCst) > 0 || !h.queued.is_empty() {
            ui.horizontal(|ui| { ui.add_space(24.0); crate::widgets::loading(ui, tr("Подготавливаем предпросмотр…")); });
        }
        if h.edit_mode { edit_bar(ui, h, st, &mut action); }
        if let Some(i) = folder_idx {
            folder_settings(ui, h, st, i, &mut action);
        } else if !viewer {
            folders_section(ui, h, st, &mut action);
        }
        let thumbs = &h.thumbs;
        let card = |ui: &mut egui::Ui, key: &str, title: &str, sub: &str, people: usize, edit: bool, checked: bool, tex_key: &str| -> egui::Response {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(240.0, 160.0), if edit { egui::Sense::click_and_drag() } else { egui::Sense::click() });
            // move mode: the card trembles like an icon on the iPhone home screen
            let rect = if edit {
                let t = ui.input(|i| i.time) as f32;
                let phase = key.len() as f32;
                ui.ctx().request_repaint();
                rect.translate(egui::vec2((t * 28.0 + phase).sin() * 1.1, (t * 31.0 + phase * 1.7).cos() * 0.9))
            } else { rect };
            let v = ui.visuals();
            let hover = crate::widgets::feedback(ui, resp.id.with("hover"), resp.hovered() && !ui.input(|i| i.any_touches()));
            let press = crate::widgets::feedback(ui, resp.id.with("press"), resp.is_pointer_button_down_on());
            let (base, accent) = (crate::theme::surface(ui), crate::theme::accent());
            let rect = rect.translate(egui::vec2(0.0, -2.0 * hover + 1.0 * press));
            let p = ui.painter().with_clip_rect(rect.expand(8.0));
            // a soft shadow lifts the card on hover instead of an outline
            p.rect_filled(rect.translate(egui::vec2(0.0, 4.0)).expand(1.0), 24.0, Color32::from_black_alpha((38.0 * hover) as u8));
            let body = base.lerp_to_gamma(ui.visuals().extreme_bg_color, 0.55);
            p.rect_filled(rect, 24.0, body);
            p.rect_stroke(rect, 24.0, egui::Stroke::new(1.0_f32, crate::theme::tint(30)), egui::StrokeKind::Inside);
            let area = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 106.0));
            p.rect_filled(area, egui::CornerRadius { nw: 24, ne: 24, sw: 0, se: 0 }, body.lerp_to_gamma(accent, 0.10 + 0.04 * hover));
            if let Some(t) = thumbs.get(tex_key) {
                let s = t.size_vec2();
                let k = ((area.width() - 28.0) / s.x).min((area.height() - 16.0) / s.y);
                let r = egui::Rect::from_center_size(area.center(), s * k);
                p.image(t.id(), r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            } else {
                p.text(area.center(), egui::Align2::CENTER_CENTER, if h.completed.contains(key) { tr("Предпросмотр недоступен") } else { tr("Предпросмотр…") }, egui::FontId::proportional(12.0), muted(ui));
            }
            if people > 0 {
                let chip = egui::Rect::from_min_size(area.right_top() + egui::vec2(-52.0, 10.0), egui::vec2(42.0, 20.0));
                p.rect_filled(chip, 10.0, crate::theme::accent());
                if let Some(icon) = crate::icons::icon("profile", 13.0, crate::theme::on_accent()) {
                    icon.paint_at(ui, egui::Rect::from_center_size(chip.left_center() + egui::vec2(11.0, 0.0), egui::vec2(13.0, 13.0)));
                }
                p.text(chip.right_center() - egui::vec2(10.0, 0.0), egui::Align2::CENTER_CENTER, people.to_string(), egui::FontId::proportional(12.0), crate::theme::on_accent());
            }
            let text = |text: &str, y: f32, size: f32, color: Color32| {
                let mut job = egui::text::LayoutJob::simple(text.into(), egui::FontId::proportional(size), color, rect.width() - 24.0);
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
                let galley = p.layout_job(job);
                p.galley(rect.left_top() + egui::vec2(16.0, y), galley, color);
            };
            text(title, 114.0, 15.0, v.text_color());
            text(sub, 135.0, 11.5, muted(ui));
            if edit {
                let dot = rect.left_top() + egui::vec2(24.0, 24.0);
                p.circle_filled(dot, 11.0, if checked { crate::theme::accent() } else { Color32::from_black_alpha(110) });
                p.circle_stroke(dot, 11.0, egui::Stroke::new(1.6_f32, Color32::WHITE));
                if checked {
                    let w = Color32::WHITE;
                    p.add(egui::Shape::line(vec![dot + egui::vec2(-4.5, 0.5), dot + egui::vec2(-1.2, 3.8), dot + egui::vec2(5.0, -3.4)], egui::Stroke::new(2.0_f32, w)));
                }
            }
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
            resp
        };
        let q = h.search.trim().to_lowercase();
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new(if folder_idx.is_some() { tr("Модели в папке") } else { tr("Недавние модели") }).size(16.0).strong().color(muted(ui)).background_color(ui.visuals().panel_fill));
        });
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let visible: Vec<&String> = projects.iter().filter(|p| match folder_idx { Some(i) => st.folders[i].projects.contains(*p), None => st.folder_of(p).is_none() }).collect();
        let mut shown: Vec<&String> = visible.iter().copied().filter(|p| h.infos.get(*p).is_some_and(|i| h.filters.matches(&q, p, i, now))).collect();
        shown.sort_by(|a,b| {
            let (a,b) = (&h.infos[*a], &h.infos[*b]);
            match h.filters.sort { 1 => b.modified.cmp(&a.modified), 2 => a.name.to_lowercase().cmp(&b.name.to_lowercase()), 3 => b.steps.cmp(&a.steps), _ => std::cmp::Ordering::Equal }
        });
        ui.horizontal(|ui| { ui.add_space(24.0); ui.label(RichText::new(tf!("Найдено: {} из {}", shown.len(), visible.len())).size(14.0).color(muted(ui)).background_color(ui.visuals().panel_fill)); });
        if shown.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(24.0);
                ui.label(RichText::new(if visible.is_empty() { if viewer { tr("Положите файлы .genix в папку Genix в приложении «Файлы»") } else { tr("Пока нет проектов — создайте новый или откройте файл") } } else { tr("Ничего не найдено") }).color(muted(ui)));
            });
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(16.0, 16.0);
            ui.add_space(22.0);
            for p in shown {
                let Some(info) = h.infos.get(p).cloned() else { continue };
                let sub = if info.exists { format!("{} · {}", info.time, info.steps.map(|n| tf!("шагов: {}", n)).unwrap_or_else(|| tr("не читается").into())) } else { tr("файл не найден").into() };
                let spinning = h.hover_since.as_ref().filter(|(k, t0)| *k == info.preview_key && now_t - t0 >= 3.0).map(|(_, t0)| now_t - t0 - 3.0);
                let tex_key = match spinning {
                    Some(t) if h.thumbs.contains_key(&format!("{}#spin0", info.preview_key)) => format!("{}#spin{}", info.preview_key, (t * 12.0) as usize % SPIN_FRAMES),
                    _ => info.preview_key.clone(),
                };
                let checked = h.selected.contains(p);
                let r = card(ui, &info.preview_key, &info.name, &sub, st.people_of(p).len(), h.edit_mode, checked, &tex_key).on_hover_text(p.as_str());
                if r.hovered() && !h.edit_mode { hovered = Some((info.preview_key.clone(), SpinSource::File(p.clone()))); }
                if r.clicked() {
                    if h.edit_mode { toggle = Some(p.clone()); }
                    else if ui.input(|i| i.modifiers.shift) { h.edit_mode = true; h.edit_since = now_t; toggle = Some(p.clone()); }
                    else if info.exists { action = HomeAction::OpenPath(PathBuf::from(p)); }
                }
                if h.edit_mode && r.drag_started() {
                    r.dnd_set_drag_payload(if checked { h.selected.iter().cloned().collect::<Vec<String>>() } else { vec![p.clone()] });
                }
                r.context_menu(|ui| {
                    if cfg!(target_os="macos") && info.exists && crate::app::menu_item(ui, "search", tr("Показать в Finder")) {crate::desktop::reveal(std::path::Path::new(p)); ui.close_menu();}
                    if info.exists && crate::app::menu_item(ui, "profile", tr("Поделиться…")) { share_req = Some(p.clone()); ui.close_menu(); }
                    ui.menu_button(tr("Переместить в папку"), |ui| {
                        if st.folders.is_empty() { ui.label(RichText::new(tr("Папок пока нет")).color(muted(ui))); }
                        for (i, f) in st.folders.iter().enumerate() {
                            if crate::app::menu_item(ui, "tree_folder", f.name.as_str()) { move_to = Some((p.clone(), Some(i))); ui.close_menu(); }
                        }
                    });
                    if st.folder_of(p).is_some() && crate::app::menu_item(ui, "tree_folder_open", tr("Убрать из папки")) { move_to = Some((p.clone(), None)); ui.close_menu(); }
                    if crate::app::menu_item(ui, "close", tr("Убрать из списка")) {
                        action = HomeAction::Forget(p.clone());
                        ui.close_menu();
                    }
                });
            }
        });
        if folder_idx.is_some() { return; }
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new(tr("Начните с примера")).size(16.0).strong().color(muted(ui)).background_color(ui.visuals().panel_fill));
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(16.0, 16.0);
            ui.add_space(22.0);
            for (i, (name, sub)) in [(tr("Кронштейн (параметрический)"), tr("пластина с отверстиями и стойка")), (tr("Втулка (вращение)"), tr("деталь вращения"))].into_iter().enumerate() {
                let key = format!("ex:{i}");
                let spinning = h.hover_since.as_ref().filter(|(k, t0)| *k == key && now_t - t0 >= 3.0).map(|(_, t0)| now_t - t0 - 3.0);
                let tex_key = match spinning {
                    Some(t) if h.thumbs.contains_key(&format!("{key}#spin0")) => format!("{key}#spin{}", (t * 12.0) as usize % SPIN_FRAMES),
                    _ => key.clone(),
                };
                let r = card(ui, &key, name, sub, 0, false, false, &tex_key);
                if r.hovered() { hovered = Some((key.clone(), SpinSource::Example(i))); }
                if r.clicked() {
                    action = HomeAction::Example(i);
                }
            }
        });
    });
    if let Some((path, to)) = move_to {
        st.move_to_folder(&path, to);
        action = HomeAction::SettingsChanged;
    }
    if let Some(path) = toggle {
        if !h.selected.remove(&path) { h.selected.insert(path); }
    }
    // hovering one model for 3 s starts its 360° preview
    let now = ui.input(|i| i.time);
    match (hovered, h.hover_since.clone()) {
        (Some((key, _)), Some((current, _))) if key == current => {}
        (Some((key, _)), _) => h.hover_since = Some((key, now)),
        (None, _) => h.hover_since = None,
    }
    if let Some((key, t0)) = h.hover_since.clone() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
        if now - t0 >= 3.0 && h.spin_started.insert(key.clone()) {
            h.spin_order.push_back(key.clone());
            while h.spin_order.len() > SPIN_MODELS {
                if let Some(old) = h.spin_order.pop_front() {
                    h.spin_started.remove(&old);
                    for i in 0..SPIN_FRAMES { h.thumbs.remove(&format!("{old}#spin{i}")); }
                }
            }
            let source = if let Some(i) = key.strip_prefix("ex:").and_then(|i| i.parse().ok()) { Some(SpinSource::Example(i)) } else { h.infos.iter().find(|(_, info)| info.preview_key == key).map(|(p, _)| SpinSource::File(p.clone())) };
            if let Some(source) = source { spawn_spin(key, source, h.pending.clone()); }
        }
    }
    if egui::DragAndDrop::has_payload_of_type::<Vec<String>>(ui.ctx()) {
        if let Some(at) = ui.ctx().pointer_latest_pos() {
            let count = egui::DragAndDrop::payload::<Vec<String>>(ui.ctx()).map_or(0, |p| p.len());
            egui::Area::new(egui::Id::new("drag-ghost")).order(egui::Order::Tooltip).fixed_pos(at + egui::vec2(14.0, 14.0)).interactable(false).show(ui.ctx(), |ui| {
                egui::Frame::new().fill(crate::theme::accent()).corner_radius(14).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                    ui.label(RichText::new(tf!("Моделей: {}", count)).color(crate::theme::on_accent()).size(13.0));
                });
            });
        }
    }
    if share_req.is_some() {
        h.share_path = share_req;
        h.share_msg = None;
    }
    share_dialog(ui.ctx(), h, st, &mut action);
    if !viewer {
        name_dialog(ui.ctx(), h, st, &mut action);
    }
    action
}

/// Страница Team: люди рабочего пространства. Демо — приглашения не отправляются.
fn team_page(ui: &mut egui::Ui, h: &mut HomeState, st: &mut Settings, action: &mut HomeAction) {
    ui.heading(tr("Team"));
    ui.label(RichText::new(tr("Демо: приглашения не отправляются, список людей хранится только на этом устройстве.")).color(muted(ui)));
    ui.add_space(20.0);
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut h.team_email).hint_text(RichText::new(tr("Почта человека")).color(muted(ui))).margin(egui::Margin::symmetric(12, 10)).desired_width(230.0).char_limit(254));
        egui::ComboBox::from_id_salt("team-role").selected_text(if h.team_editor { tr("Может править") } else { tr("Только просмотр") }).show_ui(ui, |ui| {
            ui.selectable_value(&mut h.team_editor, true, tr("Может править"));
            ui.selectable_value(&mut h.team_editor, false, tr("Только просмотр"));
        });
        if crate::ui_kit::b02(ui, tr("Добавить")).clicked() {
            match st.add_team_member(&h.team_email, h.team_editor) {
                Ok(()) => { h.team_email.clear(); h.team_msg = None; *action = HomeAction::SettingsChanged; }
                Err(e) => h.team_msg = Some(e),
            }
        }
    });
    if let Some(msg) = h.team_msg { ui.colored_label(ui.visuals().error_fg_color, tr(msg)); }
    ui.add_space(16.0);
    let mut remove: Option<String> = None;
    let owner = st.account.clone().unwrap_or_else(|| tr("Вы").to_string());
    let rows: Vec<(String, String, bool)> = std::iter::once((owner, tr("Владелец").to_string(), false)).chain(st.team.iter().map(|p| (p.email.clone(), tr(if p.editor { "Может править" } else { "Только просмотр" }).to_string(), true))).collect();
    for (email, role, removable) in rows {
        ui.horizontal(|ui| {
            person_row(ui, &email, &role);
            if removable {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::ui_kit::b03(ui, tr("Убрать")).clicked() { remove = Some(email.clone()); }
                });
            }
        });
        ui.add_space(4.0);
    }
    if let Some(email) = remove {
        st.team.retain(|p| p.email != email);
        *action = HomeAction::SettingsChanged;
    }
}

/// Строка человека: круглый аватар с буквой, почта и роль.
fn person_row(ui: &mut egui::Ui, email: &str, role: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(32.0, 32.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 16.0, crate::theme::tint(55));
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, email.chars().next().map_or('?', |c| c.to_ascii_uppercase()), egui::FontId::proportional(14.0), crate::theme::accent());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(email);
        ui.label(RichText::new(role).color(muted(ui)).small());
    });
}

/// Big round «+» on the library background: opens the choice «new model / new folder», each asks for a name.
/// «+» next to «Новый проект»: round 48 px button, opens a small menu with a new model or a new folder.
fn plus_button(ui: &mut egui::Ui, h: &mut HomeState) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::click());
    let open = crate::widgets::feedback(ui, response.id.with("open"), h.fab_open);
    let hover = crate::widgets::feedback(ui, response.id.with("hover"), response.hovered());
    let press = crate::widgets::feedback(ui, response.id.with("press"), response.is_pointer_button_down_on());
    let (base, accent) = (crate::theme::surface(ui), crate::theme::accent());
    ui.painter().circle_filled(rect.center(), 24.0 - press, base.lerp_to_gamma(accent, 0.16 + 0.10 * hover));
    let (c, k, turn) = (rect.center(), 8.5, open * std::f32::consts::FRAC_PI_4);
    let stroke = egui::Stroke::new(2.4_f32, accent);
    for a in [turn, turn + std::f32::consts::FRAC_PI_2] {
        let d = egui::vec2(a.cos(), a.sin()) * k;
        ui.painter().line_segment([c - d, c + d], stroke);
    }
    // one physical click can arrive twice within a few frames; a short lock keeps the menu from opening and closing at once
    let now = ui.input(|i| i.time);
    let tip = tr("Новая модель или папка");
    if response.clone().on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip).clicked() && now - h.fab_toggled_at > 0.35 {
        h.fab_open = !h.fab_open;
        h.fab_toggled_at = now;
    }
    if !h.fab_open { return; }
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { h.fab_open = false; }
    let menu = egui::Area::new(egui::Id::new("library-fab-menu")).order(egui::Order::Foreground).fixed_pos(rect.left_bottom() + egui::vec2(0.0, 8.0)).show(&ctx, |ui| {
        egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(22).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            for (icon, label, folder) in [("add", tr("Новая модель"), false), ("tree_folder", tr("Новая папка"), true)] {
                let (rect, row) = ui.allocate_exact_size(egui::vec2(230.0, 48.0), egui::Sense::click());
                if row.hovered() { ui.painter().rect_filled(rect, 14.0, crate::theme::tint(30)); }
                if let Some(img) = crate::icons::icon(icon, 22.0, crate::theme::accent()) { img.paint_at(ui, egui::Rect::from_center_size(rect.left_center() + egui::vec2(26.0, 0.0), egui::vec2(22.0, 22.0))); }
                ui.painter().text(rect.left_center() + egui::vec2(52.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(16.0), ui.visuals().text_color());
                if row.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    h.name_dialog = Some(NameDialog { folder, text: String::new() });
                    h.fab_open = false;
                }
            }
        });
    }).response.rect;
    if ctx.input(|i| i.pointer.any_pressed()) && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|at| !rect.contains(at) && !menu.contains(at)) { h.fab_open = false; }
}

/// Name prompt for a new model or folder. Empty name gives a default one; a model made inside an open folder goes into it.
fn name_dialog(ctx: &egui::Context, h: &mut HomeState, st: &mut Settings, action: &mut HomeAction) {
    let Some(dialog) = h.name_dialog.as_mut() else { return };
    let (folder, mut create, mut cancel) = (dialog.folder, false, false);
    let title = if folder { tr("Новая папка") } else { tr("Новая модель") };
    egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).fixed_size([380.0, 0.0]).show(ctx, |ui| {
        let field = ui.add(egui::TextEdit::singleline(&mut dialog.text).hint_text(RichText::new(tr("Название")).color(crate::theme::secondary(ui))).margin(egui::Margin::symmetric(14, 12)).font(egui::FontId::proportional(16.0)).desired_width(f32::INFINITY).char_limit(80));
        if !field.has_focus() && !field.lost_focus() { field.request_focus(); }
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if crate::ui_kit::b01(ui, tr("Создать"), 150.0).clicked() || enter { create = true; }
            if crate::ui_kit::b03(ui, tr("Отмена")).clicked() { cancel = true; }
        });
    });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) { cancel = true; }
    if create {
        let typed = dialog.text.trim().to_string();
        if folder {
            let base = if typed.is_empty() { tr("Новая папка").to_string() } else { typed };
            st.folders.push(crate::settings::ProjectFolder { name: unused_folder_name(&st.folders, &base), ..Default::default() });
            *action = HomeAction::SettingsChanged;
        } else {
            *action = HomeAction::NewNamed(if typed.is_empty() { tr("Новая модель").to_string() } else { typed }, h.open_folder.clone());
        }
    }
    if create || cancel { h.name_dialog = None; }
}

/// Окно «Поделиться»: добавить человека в проект, сменить права, убрать. Демо — ничего не отправляется.
fn share_dialog(ctx: &egui::Context, h: &mut HomeState, st: &mut Settings, action: &mut HomeAction) {
    let Some(path) = h.share_path.clone() else { return };
    let name = h.infos.get(&path).map_or_else(|| path.clone(), |i| i.name.clone());
    let mut open = true;
    let mut remove: Option<String> = None;
    egui::Window::new(tf!("Доступ к проекту: {}", name)).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).fixed_size([440.0, 0.0]).show(ctx, |ui| {
        ui.label(RichText::new(tr("Демо: приглашения не отправляются, список людей хранится только на этом устройстве.")).color(muted(ui)).small());
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut h.share_email).hint_text(RichText::new(tr("Почта человека")).color(muted(ui))).margin(egui::Margin::symmetric(12, 10)).desired_width(190.0).char_limit(254));
            egui::ComboBox::from_id_salt("share-role").selected_text(if h.share_editor { tr("Может править") } else { tr("Только просмотр") }).show_ui(ui, |ui| {
                ui.selectable_value(&mut h.share_editor, true, tr("Может править"));
                ui.selectable_value(&mut h.share_editor, false, tr("Только просмотр"));
            });
            if crate::ui_kit::b02(ui, tr("Добавить")).clicked() {
                match st.add_person(&path, &h.share_email, h.share_editor) {
                    Ok(()) => { h.share_email.clear(); h.share_msg = None; *action = HomeAction::SettingsChanged; }
                    Err(e) => h.share_msg = Some(e),
                }
            }
        });
        if let Some(msg) = h.share_msg { ui.colored_label(ui.visuals().error_fg_color, tr(msg)); }
        ui.add_space(10.0);
        let owner = st.account.clone().unwrap_or_else(|| tr("Вы").to_string());
        let rows: Vec<(String, String, bool)> = std::iter::once((owner, tr("Владелец").to_string(), false)).chain(st.people_of(&path).iter().map(|p| (p.email.clone(), tr(if p.editor { "Может править" } else { "Только просмотр" }).to_string(), true))).collect();
        for (email, role, removable) in rows {
            ui.horizontal(|ui| {
                person_row(ui, &email, &role);
                if removable {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::ui_kit::b03(ui, tr("Убрать")).clicked() { remove = Some(email.clone()); }
                    });
                }
            });
        }
    });
    if let Some(email) = remove {
        st.remove_person(&path, &email);
        *action = HomeAction::SettingsChanged;
    }
    if !open {
        h.share_path = None;
        h.share_email.clear();
        h.share_msg = None;
    }
}

/// Режим перемещения: сколько выбрано, куда переложить, выход.
fn edit_bar(ui: &mut egui::Ui, h: &mut HomeState, st: &mut Settings, action: &mut HomeAction) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(24.0);
        ui.label(RichText::new(tf!("Выбрано: {}", h.selected.len())).strong());
        ui.add_space(6.0);
        let any = !h.selected.is_empty();
        for i in 0..st.folders.len() {
            let target = format!("→ {}", st.folders[i].name);
            if ui.add_enabled_ui(any, |ui| crate::ui_kit::b08(ui, &target, false)).inner.clicked() {
                for p in h.selected.drain() { st.move_to_folder(&p, Some(i)); }
                *action = HomeAction::SettingsChanged;
            }
        }
        if ui.add_enabled_ui(any, |ui| crate::ui_kit::b08(ui, tr("Вынуть из папки"), false)).inner.clicked() {
            for p in h.selected.drain() { st.move_to_folder(&p, None); }
            *action = HomeAction::SettingsChanged;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui_kit::b01(ui, tr("Готово"), 110.0).clicked() { h.edit_mode = false; h.selected.clear(); }
        });
    });
    ui.add_space(10.0);
}

/// Папки над списком моделей: карточки и создание новой.
fn folders_section(ui: &mut egui::Ui, h: &mut HomeState, st: &mut Settings, action: &mut HomeAction) {
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.label(RichText::new(tr("Папки")).size(16.0).strong().color(muted(ui)).background_color(ui.visuals().panel_fill));
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(16.0, 16.0);
        ui.add_space(22.0);
        let mut dropped: Option<(usize, Vec<String>)> = None;
        let mut deleted: Option<usize> = None;
        for (i, f) in st.folders.iter().enumerate() {
            let pills: Vec<String> = f.projects.iter().filter_map(|p| h.infos.get(p).map(|info| info.name.clone())).take(3).collect();
            let r = folder_card(ui, &f.name, &tf!("моделей: {}", f.projects.len()), &pills, f.auto_sync, h.edit_mode);
            if r.clicked() {
                h.open_folder = Some(f.name.clone());
                h.folder_rename = f.name.clone();
            }
            if let Some(paths) = r.dnd_release_payload::<Vec<String>>() { dropped = Some((i, paths.as_ref().clone())); }
            r.context_menu(|ui| {
                if crate::app::menu_item(ui, "tree_folder_open", tr("Открыть")) { h.open_folder = Some(f.name.clone()); h.folder_rename = f.name.clone(); ui.close_menu(); }
                if crate::app::menu_item(ui, "rename", tr("Переименовать…")) { h.open_folder = Some(f.name.clone()); h.folder_rename = f.name.clone(); h.focus_rename = true; ui.close_menu(); }
                if crate::app::menu_item(ui, "delete", tr("Удалить папку")) { deleted = Some(i); ui.close_menu(); }
            });
        }
        if let Some((i, paths)) = dropped {
            for p in &paths { st.move_to_folder(p, Some(i)); h.selected.remove(p); }
            *action = HomeAction::SettingsChanged;
        }
        if let Some(i) = deleted {
            st.folders.remove(i);
            *action = HomeAction::SettingsChanged;
        }
    });
    if st.folders.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.label(RichText::new(tr("Папок пока нет — создайте первую кнопкой «+» или через меню «Файл».")).color(muted(ui)));
        });
    }
    ui.add_space(14.0);
}

/// `base`, а если такая папка уже есть — `base 2`, `base 3`…
fn unused_folder_name(folders: &[crate::settings::ProjectFolder], base: &str) -> String {
    let mut name = base.to_string();
    let mut n = 2;
    while folders.iter().any(|f| f.name == name) {
        name = format!("{base} {n}");
        n += 1;
    }
    name
}

/// Строка настроек: слева название и пояснение, справа элемент управления.
/// The two columns get fixed widths: a wrapping label would otherwise take the whole row and squeeze the control to nothing.
fn settings_row(ui: &mut egui::Ui, title: &str, note: Option<&str>, control: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(20, 14)).show(ui, |ui| {
        let total = ui.available_width();
        let right = 260.0_f32.min(total * 0.45);
        let left = total - right - 16.0;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(left, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.set_width(left);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(title).size(15.0));
                if let Some(note) = note { ui.add(egui::Label::new(RichText::new(note).color(muted(ui)).size(12.5)).wrap()); }
            });
            ui.allocate_ui_with_layout(egui::vec2(right, 36.0), egui::Layout::right_to_left(egui::Align::Center), control);
        });
    });
}

/// Настройки открытой папки: имя, автосохранение на сервер (демо) и удаление. Модели при удалении остаются.
fn folder_settings(ui: &mut egui::Ui, h: &mut HomeState, st: &mut Settings, i: usize, action: &mut HomeAction) {
    let server = st.server_active();
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(22).show(ui, |ui| ui.vertical(|ui| {
            ui.set_width(ui.available_width().min(620.0));
            settings_row(ui, tr("Название"), None, |ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut h.folder_rename).hint_text(RichText::new(tr("Название папки")).color(muted(ui))).margin(egui::Margin::symmetric(12, 8)).font(egui::FontId::proportional(15.0)).desired_width(240.0).char_limit(60));
                if std::mem::take(&mut h.focus_rename) { field.request_focus(); }
                let name = h.folder_rename.trim().to_string();
                if (field.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) && field.has_focus()) && !name.is_empty() && name != st.folders[i].name && !st.folders.iter().any(|f| f.name == name) {
                    st.folders[i].name = name.clone();
                    h.open_folder = Some(name);
                    *action = HomeAction::SettingsChanged;
                } else if field.lost_focus() {
                    h.folder_rename = st.folders[i].name.clone();
                }
            });
            ui.separator();
            let note = if server { tr("Подписка активна (демо): при сохранении модель отправляется в эту папку на сервере.") } else { tr("Нужна подписка на сервер (демо) — оформите её в разделе «Сервер». Пока сохранение только локальное.") };
            settings_row(ui, tr("Автосохранение на сервер"), Some(note), |ui| {
                let mut sync = st.folders[i].auto_sync;
                if crate::widgets::toggle(ui, &mut sync, "").changed() {
                    st.folders[i].auto_sync = sync;
                    *action = HomeAction::SettingsChanged;
                }
            });
            ui.separator();
            settings_row(ui, tr("Удалить папку"), Some(tr("Модели останутся в библиотеке")), |ui| {
                if crate::ui_kit::b04(ui, tr("Удалить")).clicked() {
                    st.folders.remove(i);
                    h.open_folder = None;
                    *action = HomeAction::SettingsChanged;
                }
            });
        }));
    });
    ui.add_space(14.0);
}

/// Карточка-папка: при наведении створка приоткрывается и из неё выглядывают первые модели.
fn folder_card(ui: &mut egui::Ui, name: &str, sub: &str, pills: &[String], sync: bool, edit: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(240.0, 160.0), egui::Sense::click());
    let receiving = resp.dnd_hover_payload::<Vec<String>>().is_some();
    let rect = if edit {
        let t = ui.input(|i| i.time) as f32;
        let phase = name.len() as f32;
        ui.ctx().request_repaint();
        rect.translate(egui::vec2((t * 28.0 + phase).sin() * 1.1, (t * 31.0 + phase * 1.7).cos() * 0.9))
    } else { rect };
    let hover = crate::widgets::feedback(ui, resp.id.with("hover"), (resp.hovered() || receiving) && !ui.input(|i| i.any_touches()));
    let press = crate::widgets::feedback(ui, resp.id.with("press"), resp.is_pointer_button_down_on());
    let (accent, base) = (crate::theme::accent(), crate::theme::surface(ui));
    let (back, front) = (base.lerp_to_gamma(accent, 0.22), base.lerp_to_gamma(accent, 0.10));
    let p = ui.painter_at(rect);
    p.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(96.0, 34.0)), egui::CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 }, back);
    p.rect_filled(egui::Rect::from_min_max(rect.min + egui::vec2(0.0, 18.0), rect.max), 20.0, back);
    for (k, label) in pills.iter().enumerate() {
        let rise = (hover * 1.5 - 0.15 * k as f32).clamp(0.0, 1.0);
        let top = rect.top() + 76.0 + (24.0 + 17.0 * k as f32 - 76.0) * rise;
        let slot = egui::Rect::from_min_max(egui::pos2(rect.left() + 18.0 + 6.0 * k as f32, top), egui::pos2(rect.right() - 18.0 - 6.0 * k as f32, top + 22.0));
        p.rect_filled(slot, 11.0, ui.visuals().extreme_bg_color);
        let mut job = egui::text::LayoutJob::simple(label.clone(), egui::FontId::proportional(12.0), ui.visuals().text_color(), slot.width() - 20.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        let galley = p.layout_job(job);
        p.galley(slot.left_center() + egui::vec2(10.0, -galley.size().y / 2.0), galley, ui.visuals().text_color());
    }
    let flap = egui::Rect::from_min_max(rect.min + egui::vec2(0.0, 70.0 + 6.0 * hover + 2.0 * press), rect.max);
    p.rect_filled(flap, 20.0, front);
    p.rect_stroke(flap, 20.0, egui::Stroke::new(if receiving { 2.5_f32 } else { 1.0_f32 }, accent.gamma_multiply(if receiving { 1.0 } else { 0.25 })), egui::StrokeKind::Inside);
    let ink = ui.visuals().text_color();
    p.text(flap.left_top() + egui::vec2(16.0, 18.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(15.0), ink);
    p.text(flap.left_top() + egui::vec2(16.0, 40.0), egui::Align2::LEFT_CENTER, sub, egui::FontId::proportional(12.0), muted(ui));
    if sync {
        if let Some(icon) = crate::icons::icon("server", 16.0, accent) {
            icon.paint_at(ui, egui::Rect::from_center_size(flap.right_top() + egui::vec2(-24.0, 18.0), egui::vec2(16.0, 16.0)));
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// B15 — «Новый проект»: плоская таблетка 48 px, значок и подпись по центру; при загрузке вместо плюса дуга-спиннер.
fn new_project_button(ui: &mut egui::Ui, loading: bool) -> egui::Response {
    let text = if loading { tr("Подготовка проекта…") } else { tr("Новый проект") };
    let font = egui::FontId::proportional(15.5);
    let text_width = ui.painter().layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE).size().x;
    let (rect, response) = ui.allocate_exact_size(egui::vec2((text_width + 18.0 + 10.0 + 52.0).max(200.0), 48.0), if loading { egui::Sense::hover() } else { egui::Sense::click() });
    let hover = crate::widgets::feedback(ui, response.id.with("hover"), response.hovered() && !loading);
    let press = crate::widgets::feedback(ui, response.id.with("press"), response.is_pointer_button_down_on());
    let paint = rect.shrink(press);
    crate::ui_kit::pill_fill(ui, paint, 24.0, hover, press);
    let color = crate::theme::on_accent();
    let left = paint.center().x - (18.0 + 10.0 + text_width) / 2.0;
    let icon = egui::pos2(left + 9.0, paint.center().y);
    let stroke = egui::Stroke::new(2.4_f32, color);
    if loading && !crate::theme::reduced_motion() {
        let t = ui.input(|i| i.time) as f32 * 5.5;
        let arc: Vec<egui::Pos2> = (0..=14).map(|i| {
            let a = t + i as f32 * std::f32::consts::TAU * 0.75 / 14.0;
            icon + egui::vec2(a.cos(), a.sin()) * 7.5
        }).collect();
        ui.painter().add(egui::Shape::line(arc, stroke));
        ui.ctx().request_repaint();
    } else if loading {
        ui.painter().circle_filled(icon, 4.0, color);
    } else {
        let k = 6.0 * (1.0 - press * 0.1);
        ui.painter().line_segment([icon - egui::vec2(k, 0.0), icon + egui::vec2(k, 0.0)], stroke);
        ui.painter().line_segment([icon - egui::vec2(0.0, k), icon + egui::vec2(0.0, k)], stroke);
    }
    ui.painter().text(egui::pos2(left + 28.0, paint.center().y), egui::Align2::LEFT_CENTER, text, font, color);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !loading, tr("Новый проект")));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_search_combines_words_path_and_metadata_filters() {
        let now = 40 * 86400;
        let mut info = ProjInfo { name: "Кронштейн большой".into(), time: String::new(), modified: Some(now-86400), steps: Some(4), exists: true, preview_key: String::new() };
        let mut f = ProjectFilters::default();
        assert!(f.matches("большой крон", "/models/print.genix", &info, now));
        assert!(!f.matches("print", "/models/print.genix", &info, now));
        f.in_path = true;
        assert!(f.matches("print крон", "/models/print.genix", &info, now));
        f.available_only = true; f.readable_only = true; f.min_steps = 4; f.modified_days = 7;
        assert!(f.matches("", "", &info, now));
        info.exists = false; assert!(!f.matches("", "", &info, now)); info.exists = true;
        info.steps = None; assert!(!f.matches("", "", &info, now)); info.steps = Some(3);
        assert!(!f.matches("", "", &info, now)); info.steps = Some(4);
        info.modified = Some(now-8*86400); assert!(!f.matches("", "", &info, now));
        info.modified = None; assert!(!f.matches("", "", &info, now));
        assert!(ProjectFilters::default().matches("", "", &info, now));
    }

    #[test]
    fn mock_registration_validates_and_does_not_store_passwords() {
        let mut st = Settings::default();
        let mut gate = Gate { name: "Даниэль".into(), email: "daniel@example.com".into(), pass: "test1234".into(), repeat: "different".into(), ..Default::default() };
        assert!(!gate.submit(&mut st));
        assert!(st.account.is_none());
        gate.repeat = gate.pass.clone();
        assert!(gate.submit(&mut st));
        assert!(unlocked(&st));
        assert_eq!(st.display_name, "Даниэль");
        assert!(gate.pass.is_empty() && gate.repeat.is_empty());
        let saved = serde_json::to_string(&st).unwrap();
        assert!(!saved.contains("test1234"));
        assert!(!saved.contains("password"));
        let restored: Settings = serde_json::from_str(&saved).unwrap();
        assert!(unlocked(&restored));
    }

    #[test]
    fn email_validation_and_navigation_reset_transient_forms() {
        for bad in ["", "a@", "@example.com", "a@.com", "a@b.", "a b@example.com", "a@b@c.com"] { assert!(!valid_email(bad), "{bad}"); }
        assert!(valid_email("daniel@example.com"));
        let mut home = HomeState::default();
        home.navigate(HomePage::Server);
        home.checkout = true;
        home.navigate(HomePage::Models);
        assert!(!home.checkout);
        assert_eq!(home.page, HomePage::Models);
    }

    #[test]
    fn refresh_invalidates_preview_when_model_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("project.genix");
        let mut doc = fusion_core::Document::default();
        std::fs::write(&path, doc.to_json()).unwrap();
        let recent = vec![path.to_string_lossy().into_owned()];
        let mut home = HomeState::default();
        home.refresh(&recent);
        let first = home.infos[&recent[0]].preview_key.clone();
        doc.params.set("width", "12345");
        std::fs::write(&path, doc.to_json()).unwrap();
        home.refresh(&recent);
        assert_ne!(first, home.infos[&recent[0]].preview_key);
        assert_eq!(home.queued.len(), 3); // модель и два примера
        home.refresh(&recent);
        assert_eq!(home.queued.len(), 3); // повторный вход не дублирует задания
    }

    #[test]
    fn folder_names_never_collide() {
        let named = |n: &str| crate::settings::ProjectFolder { name: n.into(), ..Default::default() };
        assert_eq!(unused_folder_name(&[], "Новая папка"), "Новая папка");
        assert_eq!(unused_folder_name(&[named("Новая папка")], "Новая папка"), "Новая папка 2");
        assert_eq!(unused_folder_name(&[named("A"), named("A 2")], "A"), "A 3");
    }

    #[test]
    fn time_format() {
        assert_eq!(format_time(0), "1970-01-01 00:00");
        assert_eq!(format_time(1_700_000_000), "2023-11-14 22:13");
    }
}
