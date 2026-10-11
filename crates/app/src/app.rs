//! Главное окно: меню, панель инструментов, браузер, 3D-вид, история (timeline), диалоги.

use crate::camera::Camera;
use crate::commands::{self, CmdResult, Command, PickTarget};
use crate::gpu::{FrameUniform, GpuScene, LineInst, Overlay, FaceVertex};
use crate::render::{self, fill_quad, fill_region, fill_tris, nice_step, polyline, RenderCache};
use crate::sketcher::{SketchAction, SketchSession};
use crate::theme::{linear, Theme, ACCENT};
use crate::viewcube::{self, CubeAction};
use crate::widgets::expr_field;
use crate::i18n::{self, tr, tr_dyn, tr_name, Lang};
use crate::settings::{ControlScheme, Settings, ThemeChoice, VisualStyle};
use crate::tf;
use crate::toolbar;
use eframe::egui::{self, Color32, Pos2, Rect, RichText};
use eframe::egui_wgpu::{self, wgpu};
use fusion_core::doc::{EdgeRef, Model, SketchResult};
use fusion_core::expr::format_num;
use fusion_core::sketch::{region_at, ProfileRef};
use fusion_core::{Document, Evaluator, Expr, FeatureKind, Mesh, Params, Plane, Sketch};
use glam::DVec3;
use std::path::PathBuf;
use std::sync::Arc;

pub const MSAA: u32 = 4;

enum Mode {
    Idle,
    /// Выбор плоскости для нового чертежа.
    PickPlane,
    Sketch(Box<SketchSession>),
    Command(Command),
    /// Измерение: до двух точек на телах.
    Measure(Vec<DVec3>),
}

/// Что под курсором в 3D.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hit {
    Body { slot: usize, tri: usize, t: f64 },
    Profile { sketch: u64, region: usize, local: [f64; 2], t: f64 },
    OriginPlane(usize, f64),
    /// Смещённая плоскость пользователя (индекс в `Document::offset_planes`).
    UserPlane(usize, f64),
}

/// Что делает левая кнопка мыши в пустом месте окна.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavMode {
    Default,
    Orbit,
    Pan,
    Zoom,
}

/// Перетаскивание элемента манипулятора перемещения: стрелка оси или дуга поворота.
struct Gizmo {
    ring: bool,
    axis: usize,
    c0: DVec3,
    grab: f64,
    start: f64,
}

const GIZMO_COLORS: [Color32; 3] = [Color32::from_rgb(235, 60, 60), Color32::from_rgb(60, 200, 60), Color32::from_rgb(80, 110, 255)];

/// Перетаскивание стрелки выдавливания.
struct Manip {
    base: DVec3,
    dir: DVec3,
    grab: f64,
}

impl Hit {
    fn t(&self) -> f64 {
        match *self {
            Hit::Body { t, .. } | Hit::Profile { t, .. } | Hit::OriginPlane(_, t) | Hit::UserPlane(_, t) => t,
        }
    }
}

enum Confirm {
    New,
    Open,
    OpenExample(fn() -> Document),
    CloseTab,
    Quit,
}

/// Припаркованное состояние неактивной вкладки-документа.
#[derive(Default)]
struct Tab {
    doc: Document,
    ev: Evaluator,
    path: Option<PathBuf>,
    saved: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    cam: Camera,
    selection: Vec<u64>,
}

struct Materials;
impl Materials {
    const LIST: [(&'static str, f64); 7] = [
        ("Сталь", 7.85),
        ("Нерж. сталь", 8.0),
        ("Алюминий", 2.70),
        ("Латунь", 8.5),
        ("PLA (3D-печать)", 1.24),
        ("ABS/PETG", 1.05),
        ("Дерево (сосна)", 0.52),
    ];
}

pub struct App {
    /// Вкладки: неактивные хранят своё состояние, у активной (`active`) слот пустой.
    tabs: Vec<Tab>,
    active: usize,
    gate: crate::home::Gate,
    home: crate::home::HomeState,
    home_active: bool,
    doc: Document,
    ev: Evaluator,
    path: Option<PathBuf>,
    saved: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    mode: Mode,
    cam: Camera,
    theme: Theme,
    selection: Vec<u64>,
    hover: Option<Hit>,
    cache: RenderCache,
    shown_model: usize,
    shown_edge_w: u32,
    ortho: bool,
    upload: Option<(Vec<FaceVertex>, Vec<LineInst>)>,
    show_planes: bool,
    settings: Settings,
    settings_open: bool,
    settings_tab: u8,
    last_autosave: f64,
    last_ray: (DVec3, DVec3),
    manip: Option<Manip>,
    gizmo: Option<Gizmo>,
    nav: NavMode,
    playing: bool,
    last_play: f64,
    new_view_name: String,
    hover_edge: Option<EdgeRef>,
    edge_cache: std::cell::RefCell<std::collections::HashMap<usize, Arc<Vec<EdgeRef>>>>,
    toast: Option<(String, f64, bool)>,
    rename_body: Option<(u64, String)>,
    rename_feature: Option<(u64, String)>,
    params_open: bool,
    help_open: bool,
    confirm: Option<Confirm>,
    allow_close: bool,
    need_fit: bool,
    /// Тело/шаг под правой кнопкой (для контекстного меню).
    context_body: Option<u64>,
    new_param: (String, String),
    viewport: Rect,
    face_groups: std::cell::RefCell<std::collections::HashMap<usize, Arc<Vec<u32>>>>,
    /// Вид камеры до входа в чертёж — возвращаемся к нему после.
    pre_sketch_view: Option<(f64, f64)>,
    now: f64,
}

/// Кнопка со значком из набора (нет значка — текстовый символ).
fn icon_button(ui: &mut egui::Ui, id: &str, fallback: &str, selected: bool, tip: &str, min: egui::Vec2) -> bool {
    let main = if selected { Color32::WHITE } else { ui.visuals().text_color() };
    let btn = match crate::icons::icon(id, 22.0, main) {
        Some(img) => egui::Button::image(img),
        None => egui::Button::new(RichText::new(fallback).size(16.0)),
    };
    ui.add(btn.selected(selected).frame(selected).min_size(min)).on_hover_text(tip).clicked()
}

/// Группа дерева в стиле Fusion: стрелка, значок папки, название; вся строка подсвечивается при наведении.
fn tree_group(ui: &mut egui::Ui, id: &str, icon: &str, title: String, default_open: bool, body: impl FnOnce(&mut egui::Ui)) {
    use egui::collapsing_header::CollapsingState;
    let cid = ui.make_persistent_id(id);
    let mut st = CollapsingState::load_with_default_open(ui.ctx(), cid, default_open);
    let open = st.is_open();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), 5.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let col = ui.visuals().text_color();
    let mut x = rect.left() + 4.0;
    let mut draw = |name: &str, size: f32, x: &mut f32| {
        if let Some(img) = crate::icons::icon(name, size, col) {
            img.paint_at(ui, egui::Rect::from_min_size(egui::pos2(*x, rect.center().y - size / 2.0), egui::vec2(size, size)));
        }
        *x += size + 4.0;
    };
    draw(if open { "tree_arrow_down" } else { "tree_arrow_right" }, 14.0, &mut x);
    draw(icon, 18.0, &mut x);
    ui.painter().text(egui::pos2(x + 2.0, rect.center().y), egui::Align2::LEFT_CENTER, title, egui::FontId::proportional(13.5), col);
    if resp.clicked() {
        st.toggle(ui);
    }
    st.show_body_indented(&resp, ui, body);
}

/// Значок глаза (показать/скрыть). true — значение изменилось.
fn eye_toggle(ui: &mut egui::Ui, vis: &mut bool, tip: &str) -> bool {
    if icon_button(ui, if *vis { "tree_eye" } else { "tree_eye_off" }, "👁", false, tip, egui::vec2(24.0, 24.0)) {
        *vis = !*vis;
        return true;
    }
    false
}

/// Пункт меню со значком слева.
fn menu_item(ui: &mut egui::Ui, icon: &str, text: &str) -> bool {
    let col = ui.visuals().text_color();
    let btn = match crate::icons::icon(icon, 16.0, col) {
        Some(img) => egui::Button::image_and_text(img, text),
        None => egui::Button::new(text),
    };
    ui.add(btn.min_size(egui::vec2(220.0, 26.0))).clicked()
}

fn plane_name(i: usize) -> &'static str {
    [tr("XY (верх)"), tr("XZ (перед)"), tr("YZ (право)")][i]
}

const ORIGIN_PLANES: [Plane; 3] = [Plane::XY, Plane::XZ, Plane::YZ];

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open: Option<PathBuf>) -> Self {
        let rs = cc.wgpu_render_state.as_ref().expect("нужен wgpu-рендер");
        rs.renderer.write().callback_resources.insert(GpuScene::new(&rs.device, rs.target_format, MSAA));
        let settings = Settings::load();
        i18n::set_lang(settings.lang);
        let theme = resolve_theme(&settings, &cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Theme::install_fonts(&cc.egui_ctx);
        theme.apply(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(settings.ui_scale);
        let mut app = App {
            doc: Document::default(),
            ev: Evaluator::default(),
            path: None,
            saved: Document::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            mode: Mode::Idle,
            cam: Camera::default(),
            tabs: vec![Tab::default()],
            active: 0,
            gate: Default::default(),
            home: Default::default(),
            home_active: true,
            theme,
            selection: Vec::new(),
            hover: None,
            cache: RenderCache::default(),
            shown_model: usize::MAX,
            shown_edge_w: 0,
            ortho: false,
            upload: None,
            show_planes: false,
            settings,
            settings_open: false,
            settings_tab: 0,
            last_autosave: 0.0,
            last_ray: (DVec3::ZERO, DVec3::Z),
            manip: None,
            gizmo: None,
            nav: NavMode::Default,
            playing: false,
            last_play: 0.0,
            new_view_name: String::new(),
            hover_edge: None,
            edge_cache: Default::default(),
            toast: None,
            rename_body: None,
            rename_feature: None,
            params_open: false,
            help_open: false,
            confirm: None,
            allow_close: false,
            need_fit: false,
            context_body: None,
            new_param: (String::new(), String::new()),
            viewport: Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)),
            face_groups: Default::default(),
            pre_sketch_view: None,
            now: 0.0,
        };
        if let Some(p) = open {
            app.open_path(p);
        } else if std::env::var("GENIX_DEMO").is_ok() {
            app.load_doc(crate::examples::bracket(), None);
        }
        if std::env::var("GENIX_SKIP_LOGIN").is_ok() {
            app.settings.license.get_or_insert_with(|| "DEMO".into());
            app.settings.account_skipped = true;
        }
        app.home.refresh(&app.settings.recent);
        app
    }

    // ------------------------------------------------------------ документ и история

    fn dirty(&self) -> bool {
        self.doc != self.saved
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.doc.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn do_undo(&mut self) {
        if let Some(d) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, d));
            self.after_doc_change();
        }
    }

    fn do_redo(&mut self) {
        if let Some(d) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, d));
            self.after_doc_change();
        }
    }

    fn after_doc_change(&mut self) {
        self.ev.evaluate(&self.doc);
        let model = self.ev.model();
        self.selection.retain(|id| model.body(*id).is_some());
    }

    fn load_doc(&mut self, d: Document, path: Option<PathBuf>) {
        self.mode = Mode::Idle;
        self.home_active = false;
        self.doc = d;
        self.saved = if path.is_some() { self.doc.clone() } else { Document::default() };
        self.path = path;
        self.undo.clear();
        self.redo.clear();
        self.selection.clear();
        self.ev = Evaluator::default();
        self.after_doc_change();
        self.need_fit = true;
    }

    fn toast(&mut self, msg: impl Into<String>, error: bool, now: f64) {
        self.toast = Some((msg.into(), now, error));
    }

    fn title(&self) -> String {
        if self.home_active {
            return format!("{} — Genix", tr("Главная"));
        }
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("Без имени").into());
        format!("{name}{} — Genix", if self.dirty() { " •" } else { "" })
    }

    fn note_recent(&mut self, p: &std::path::Path) {
        let s = p.to_string_lossy().into_owned();
        self.settings.recent.retain(|x| *x != s);
        self.settings.recent.insert(0, s);
        self.settings.recent.truncate(40);
        self.settings.save();
    }

    fn open_path(&mut self, p: PathBuf) {
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| Document::from_json(&s)) {
            Ok(d) => {
                self.note_recent(&p);
                self.open_in_tab(d, Some(p));
            }
            Err(e) => self.toast = Some((tf!("Не удалось открыть: {}", e), 0.0, true)),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Genix", &["genix", "ostov"]).pick_file() {
            self.open_path(p);
        }
    }

    fn save(&mut self, as_new: bool, now: f64) -> bool {
        let path = match (&self.path, as_new) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new().add_filter("Genix", &["genix"]).set_file_name(tr("деталь.genix")).save_file(),
        };
        let Some(mut p) = path else { return false };
        if p.extension().is_none() {
            p.set_extension("genix");
        }
        match std::fs::write(&p, self.doc.to_json()) {
            Ok(()) => {
                self.saved = self.doc.clone();
                self.note_recent(&p);
                self.path = Some(p);
                self.toast(tr("Сохранено"), false, now);
                true
            }
            Err(e) => {
                self.toast(tf!("Ошибка сохранения: {}", e), true, now);
                false
            }
        }
    }

    /// Снимок выбранных (или всех видимых) тел с текущего ракурса — PNG с прозрачным фоном.
    fn export_png(&mut self, now: f64) {
        let model = self.ev.model();
        let mut bodies: Vec<&fusion_core::doc::Body> = model.bodies.iter().filter(|b| self.selection.contains(&b.id)).collect();
        if bodies.is_empty() {
            bodies = model.bodies.iter().filter(|b| !self.doc.hidden_bodies.contains(&b.id)).collect();
        }
        let meshes: Vec<&Mesh> = bodies.iter().map(|b| b.mesh()).collect();
        let Some(img) = crate::thumb::render(&meshes, self.cam.yaw, self.cam.pitch, 1800, 1350) else {
            self.toast(tr("Нет тел для экспорта"), true, now);
            return;
        };
        let name = format!("{}.png", self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("модель").into()));
        let Some(p) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name(&name).save_file() else { return };
        let buf = image::RgbaImage::from_raw(img.w as u32, img.h as u32, img.data).unwrap();
        match buf.save(&p) {
            Ok(()) => self.toast(tr("Снимок сохранён (прозрачный фон)"), false, now),
            Err(e) => self.toast(tf!("Ошибка экспорта: {}", e), true, now),
        }
    }

    fn export(&mut self, obj: bool, now: f64) {
        let model = self.ev.model();
        let mut bodies: Vec<&fusion_core::doc::Body> = model.bodies.iter().filter(|b| self.selection.contains(&b.id)).collect();
        if bodies.is_empty() {
            bodies = model.bodies.iter().filter(|b| !self.doc.hidden_bodies.contains(&b.id)).collect();
        }
        if bodies.is_empty() {
            self.toast(tr("Нет тел для экспорта"), true, now);
            return;
        }
        let (ext, label) = if obj { ("obj", "OBJ") } else { ("stl", tr("STL (3D-печать)")) };
        let name = format!("{}.{ext}", self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("модель").into()));
        let Some(p) = rfd::FileDialog::new().add_filter(label, &[ext]).set_file_name(&name).save_file() else { return };
        let res = if obj {
            let named: Vec<(&str, &Mesh)> = bodies.iter().map(|b| (self.doc.body_name(b), b.mesh())).collect();
            std::fs::write(&p, Mesh::write_obj(&named))
        } else {
            let mut out = Vec::new();
            Mesh::write_stl_binary(&bodies.iter().map(|b| b.mesh()).collect::<Vec<_>>(), &mut out);
            std::fs::write(&p, out)
        };
        match res {
            Ok(()) => self.toast(tf!("Экспортировано тел: {}", bodies.len()), false, now),
            Err(e) => self.toast(tf!("Ошибка экспорта: {}", e), true, now),
        }
    }

    fn request(&mut self, c: Confirm) {
        let ask = match c {
            Confirm::Quit => self.dirty() || self.tabs.iter().enumerate().any(|(i, t)| i != self.active && t.doc != t.saved),
            Confirm::CloseTab => self.dirty(),
            _ => false, // новый документ и открытие идут во вкладку — ничего не теряется
        };
        if ask {
            self.confirm = Some(c);
        } else {
            self.run_confirmed(c);
        }
    }

    // ---------------------------------------------------------- вкладки

    fn park(&mut self) -> Tab {
        Tab {
            doc: std::mem::take(&mut self.doc),
            ev: std::mem::take(&mut self.ev),
            path: self.path.take(),
            saved: std::mem::take(&mut self.saved),
            undo: std::mem::take(&mut self.undo),
            redo: std::mem::take(&mut self.redo),
            cam: self.cam,
            selection: std::mem::take(&mut self.selection),
        }
    }

    fn restore(&mut self, t: Tab) {
        self.doc = t.doc;
        self.ev = t.ev;
        self.path = t.path;
        self.saved = t.saved;
        self.undo = t.undo;
        self.redo = t.redo;
        self.cam = t.cam;
        self.selection = t.selection;
        self.mode = Mode::Idle;
        self.home_active = false;
        self.shown_model = usize::MAX;
        self.after_doc_change();
    }

    fn tab_title(&self, i: usize) -> (String, bool) {
        let (path, dirty) = if i == self.active { (&self.path, self.dirty()) } else { (&self.tabs[i].path, self.tabs[i].doc != self.tabs[i].saved) };
        let name = path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("Без имени").into());
        (name, dirty)
    }

    fn switch_tab(&mut self, i: usize) {
        if i == self.active || i >= self.tabs.len() || !matches!(self.mode, Mode::Idle) {
            return;
        }
        let old = self.park();
        self.tabs[self.active] = old;
        let t = std::mem::take(&mut self.tabs[i]);
        self.active = i;
        self.restore(t);
    }

    /// Открыть документ в новой вкладке (пустая безымянная вкладка используется повторно).
    fn open_in_tab(&mut self, d: Document, path: Option<PathBuf>) {
        if !matches!(self.mode, Mode::Idle) {
            return;
        }
        if let Some(p) = &path {
            let own = self.path.as_ref() == Some(p);
            if own {
                self.home_active = false;
                return;
            }
            if let Some(i) = self.tabs.iter().enumerate().position(|(k, t)| k != self.active && t.path.as_ref() == Some(p)) {
                self.switch_tab(i);
                return;
            }
        }
        let pristine = self.path.is_none() && self.doc == Document::default() && self.undo.is_empty();
        if pristine {
            self.load_doc(d, path);
            return;
        }
        let old = self.park();
        self.tabs[self.active] = old;
        self.tabs.push(Tab::default());
        self.active = self.tabs.len() - 1;
        self.restore(Tab { saved: if path.is_some() { d.clone() } else { Document::default() }, doc: d, path, ..Tab::default() });
        self.need_fit = true;
    }

    fn new_blank_tab(&mut self) {
        if !matches!(self.mode, Mode::Idle) {
            return;
        }
        let old = self.park();
        self.tabs[self.active] = old;
        self.tabs.push(Tab::default());
        self.active = self.tabs.len() - 1;
        self.restore(Tab::default());
    }

    fn close_tab_now(&mut self) {
        if self.tabs.len() == 1 {
            self.load_doc(Document::default(), None);
            return;
        }
        self.tabs.remove(self.active);
        let ni = self.active.min(self.tabs.len() - 1);
        let t = std::mem::take(&mut self.tabs[ni]);
        self.active = ni;
        self.restore(t);
        self.need_fit = false;
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        let idle = matches!(self.mode, Mode::Idle);
        let titles: Vec<(String, bool)> = (0..self.tabs.len()).map(|i| self.tab_title(i)).collect();
        let (mut go, mut close, mut add) = (None, None, false);
        ui.add_enabled_ui(idle, |ui| {
            egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if ui.add(egui::Button::new(RichText::new(format!("⌂ {}", tr("Главная"))).strong().size(12.5)).selected(self.home_active).frame(self.home_active).min_size(egui::vec2(90.0, 24.0))).clicked() {
                        self.home_active = true;
                        self.home.refresh(&self.settings.recent);
                    }
                    ui.add_space(6.0);
                    for (i, (name, dirty)) in titles.iter().enumerate() {
                        let sel = i == self.active && !self.home_active;
                        let label = format!("{}{}", name, if *dirty { " •" } else { "" });
                        let r = ui.add(egui::Button::new(RichText::new(label).strong().size(12.5)).selected(sel).frame(sel).min_size(egui::vec2(90.0, 24.0)));
                        if r.clicked() {
                            go = Some(i);
                        }
                        if r.middle_clicked() {
                            close = Some(i);
                        }
                        if ui.add(egui::Button::new("✕").small().frame(false)).on_hover_text(tr("Закрыть вкладку")).clicked() {
                            close = Some(i);
                        }
                        ui.add_space(6.0);
                    }
                    if ui.add(egui::Button::new(RichText::new("+").size(17.0)).frame(false)).on_hover_text(tr("Новая вкладка")).clicked() {
                        add = true;
                    }
                });
            });
        });
        if let Some(i) = go {
            self.home_active = false;
            self.switch_tab(i);
        }
        if let Some(i) = close {
            self.home_active = false;
            self.switch_tab(i);
            if i == self.active {
                self.request(Confirm::CloseTab);
            }
        }
        if add {
            self.new_blank_tab();
        }
    }

    fn run_confirmed(&mut self, c: Confirm) {
        match c {
            Confirm::New => self.new_blank_tab(),
            Confirm::Open => self.open_dialog(),
            Confirm::OpenExample(f) => self.open_in_tab(f(), None),
            Confirm::CloseTab => self.close_tab_now(),
            Confirm::Quit => self.allow_close = true,
        }
    }

    // ------------------------------------------------------------ команды

    /// Модель, которую сейчас показываем (при правке чертежа — до этого чертежа).
    fn shown(&self) -> Arc<Model> {
        if let Mode::Sketch(s) = &self.mode {
            if let Some(uid) = s.uid {
                if let Some(i) = self.doc.index_of(uid) {
                    return self.ev.model_before(i);
                }
            }
        }
        self.ev.model()
    }

    fn body_name(&self, id: u64) -> String {
        let m = self.ev.model();
        m.body(id).map(|b| self.doc.body_name(b).to_string()).unwrap_or_else(|| tr("(нет тела)").into())
    }

    fn start_sketch(&mut self) {
        self.cancel_mode();
        self.mode = Mode::PickPlane;
    }

    fn begin_sketch_on(&mut self, plane: Plane, now: f64) {
        let s = SketchSession::new(None, plane, Sketch::default(), &self.doc.params);
        self.look_at_plane(&plane, now);
        self.mode = Mode::Sketch(Box::new(s));
    }

    fn look_at_plane(&mut self, plane: &Plane, now: f64) {
        if self.pre_sketch_view.is_none() {
            self.pre_sketch_view = Some((self.cam.yaw, self.cam.pitch));
        }
        let (yaw, pitch) = Camera::angles_for(plane.normal(), plane.xa());
        // центр взгляда — проекция текущей цели на плоскость
        let t = plane.to_world(plane.to_local(self.cam.target));
        self.cam.animate_to(yaw, pitch, None, Some(t), now);
    }

    fn edit_feature(&mut self, uid: u64, now: f64) {
        self.cancel_mode();
        let Some(f) = self.doc.feature(uid).cloned() else { return };
        match f.kind {
            FeatureKind::Sketch { plane, sketch } => {
                let s = SketchSession::new(Some(uid), plane, sketch, &self.doc.params);
                self.look_at_plane(&plane, now);
                self.mode = Mode::Sketch(Box::new(s));
            }
            kind => {
                let pick = Command::pick_for(&kind);
                self.mode = Mode::Command(Command { uid, before: self.doc.clone(), pick, auto_op: false });
            }
        }
    }

    fn start_command(&mut self, kind: &str, now: f64) {
        self.cancel_mode();
        let model = self.ev.model();
        // последний видимый чертёж с одним профилем — выбираем сразу
        let last_sketch = self
            .doc
            .features
            .iter()
            .take(self.doc.rollback)
            .rev()
            .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }) && self.doc.visible_sketches.contains(&f.uid))
            .find_map(|f| self.ev.sketch(f.uid).map(|s| (f.uid, s)))
            .map(|(uid, s)| (uid, if s.regions.len() == 1 { vec![ProfileRef::of(&s.regions[0])] } else { vec![] }));
        let sketches: Vec<u64> = self.doc.features.iter().take(self.doc.rollback).filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.uid).collect();
        if matches!(kind, "extrude" | "revolve" | "sweep" | "loft" | "pipe") && sketches.is_empty() {
            self.toast(tr("Сначала создайте чертёж (кнопка «Новый чертёж»)"), true, now);
            return;
        }
        if matches!(kind, "hole" | "fillet" | "chamfer" | "scale" | "move" | "mirror" | "pattern" | "combine" | "remove") && model.bodies.is_empty() {
            self.toast(tr("Сначала создайте тело"), true, now);
            return;
        }
        // заготовка отверстия: верхняя грань первого тела
        let top_face = model.bodies.iter().find_map(|b| b.mesh().bounds().map(|(lo, hi)| {
            let c = (lo + hi) / 2.0;
            (Plane { origin: [0.0, 0.0, hi.z], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] }, [c.x, c.y])
        }));
        let ctx = commands::Ctx {
            has_bodies: !model.bodies.is_empty(),
            selected: self.selection.clone(),
            last_sketch,
            sketches,
            top_face,
            first_body: model.bodies.first().map(|b| b.id),
        };
        let feature = commands::default_feature(kind, &ctx);
        let before = self.doc.clone();
        let pick = Command::pick_for(&feature);
        let uid = self.doc.insert(feature);
        for sk in used_sketches(&self.doc.feature(uid).map(|f| f.kind.clone()).unwrap()) {
            self.doc.visible_sketches.insert(sk);
        }
        self.mode = Mode::Command(Command { uid, before, pick, auto_op: true });
        self.after_doc_change();
    }

    /// Нажатие на кнопку панели инструментов.
    fn run_tool(&mut self, id: &str, now: f64) {
        match id {
            "sketch" => self.start_sketch(),
            "measure" => {
                self.cancel_mode();
                self.mode = Mode::Measure(Vec::new());
            }
            "remove" => {
                if self.selection.is_empty() {
                    self.start_command("remove", now);
                } else {
                    let ids = self.selection.clone();
                    self.delete_bodies(ids);
                }
            }
            other => self.start_command(other, now),
        }
    }

    fn finish_command(&mut self, ok: bool) {
        let Mode::Command(cmd) = std::mem::replace(&mut self.mode, Mode::Idle) else { return };
        if ok {
            // использованный чертёж прячем
            if let Some(kind) = self.doc.feature(cmd.uid).map(|f| f.kind.clone()) {
                for sk in used_sketches(&kind) {
                    self.doc.visible_sketches.remove(&sk);
                }
            }
            self.undo.push(cmd.before);
            self.redo.clear();
        } else {
            self.doc = cmd.before;
        }
        self.after_doc_change();
    }

    fn finish_sketch(&mut self, keep: bool) {
        let Mode::Sketch(s) = std::mem::replace(&mut self.mode, Mode::Idle) else { return };
        if let Some((yaw, pitch)) = self.pre_sketch_view.take() {
            let now = self.now;
            self.cam.animate_to(yaw, pitch, None, None, now);
        }
        if !keep {
            return;
        }
        self.checkpoint();
        match s.uid {
            Some(uid) => {
                if let Some(f) = self.doc.feature_mut(uid) {
                    f.kind = FeatureKind::Sketch { plane: s.plane, sketch: s.sketch };
                }
            }
            None => {
                if s.sketch.entities.is_empty() {
                    self.undo.pop();
                    return;
                }
                let uid = self.doc.insert(FeatureKind::Sketch { plane: s.plane, sketch: s.sketch });
                self.doc.visible_sketches.insert(uid);
            }
        }
        self.after_doc_change();
    }

    fn cancel_mode(&mut self) {
        match &self.mode {
            Mode::Command(_) => self.finish_command(false),
            Mode::Sketch(_) => self.finish_sketch(true),
            _ => self.mode = Mode::Idle,
        }
    }

    fn delete_bodies(&mut self, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        self.cancel_mode();
        self.checkpoint();
        self.doc.insert(FeatureKind::Remove { bodies: ids });
        self.after_doc_change();
    }

    fn delete_feature(&mut self, uid: u64, now: f64) {
        self.checkpoint();
        let broken = self.doc.delete(uid);
        self.after_doc_change();
        if !broken.is_empty() {
            self.toast(tf!("Шагов, ссылавшихся на удалённый чертёж: {} — они помечены ошибкой", broken.len()), true, now);
        }
    }

    // ------------------------------------------------------------ выбор в 3D

    fn hit_test(&self, model: &Model, origin: DVec3, dir: DVec3) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut consider = |h: Hit| {
            if best.is_none_or(|b| h.t() < b.t()) {
                best = Some(h);
            }
        };
        let picking_profiles = matches!(&self.mode, Mode::Command(c) if c.pick == PickTarget::Profiles);
        let picking_plane = self.plane_pick_mode();
        let mut body_hit = false;
        for (slot, b) in model.bodies.iter().enumerate() {
            if self.doc.hidden_bodies.contains(&b.id) {
                continue;
            }
            if let Some((lo, hi)) = b.mesh().bounds() {
                if !ray_box(origin, dir, lo, hi) {
                    continue;
                }
            }
            if let Some((t, tri)) = b.mesh().raycast(origin, dir) {
                body_hit = true;
                consider(Hit::Body { slot, tri, t });
            }
        }
        if picking_profiles {
            let skip = self.path_sketch();
            for uid in self.sketches_to_show() {
                if Some(uid) == skip {
                    continue;
                }
                let Some(sk) = self.ev.sketch(uid) else { continue };
                if let Some(t) = sk.plane.ray_hit(origin, dir) {
                    if t > 0.0 {
                        let local = sk.plane.to_local(origin + dir * t);
                        if let Some(region) = region_at(&sk.regions, local) {
                            // профиль на грани тела выигрывает у самой грани
                            let h = Hit::Profile { sketch: uid, region, local, t: t - 1e-6 * t.abs().max(1.0) };
                            consider(h);
                        }
                    }
                }
            }
        }
        // плоскости начала координат выбираются только там, где под курсором нет тела (иначе они перехватывают грани)
        if picking_plane && !body_hit && self.show_origin_planes() {
            let s = self.origin_plane_size();
            for (i, pl) in ORIGIN_PLANES.iter().enumerate() {
                if let Some(t) = pl.ray_hit(origin, dir) {
                    let l = pl.to_local(origin + dir * t);
                    if t > 0.0 && (0.1 * s..=1.1 * s).contains(&l[0]) && (0.1 * s..=1.1 * s).contains(&l[1]) {
                        consider(Hit::OriginPlane(i, t));
                    }
                }
            }
            for (i, (_, _, pl)) in self.doc.offset_planes().iter().enumerate() {
                let c = pl.to_local(self.cam.target);
                if let Some(t) = pl.ray_hit(origin, dir) {
                    let l = pl.to_local(origin + dir * t);
                    if t > 0.0 && (l[0] - c[0]).abs() <= s && (l[1] - c[1]).abs() <= s {
                        consider(Hit::UserPlane(i, t));
                    }
                }
            }
        }
        best
    }

    /// Выбираем плоскость: для нового чертежа или как основу смещённой плоскости.
    fn plane_pick_mode(&self) -> bool {
        matches!(self.mode, Mode::PickPlane)
            || matches!(&self.mode, Mode::Command(c) if self.doc.feature(c.uid).is_some_and(|f| matches!(f.kind, FeatureKind::OffsetPlane { .. })))
    }

    fn show_origin_planes(&self) -> bool {
        self.show_planes || self.plane_pick_mode()
    }

    fn origin_plane_size(&self) -> f64 {
        nice_step(self.cam.distance * 0.15)
    }

    fn sketches_to_show(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.doc.features.iter().take(self.doc.rollback).filter(|f| self.doc.visible_sketches.contains(&f.uid)).map(|f| f.uid).collect();
        if let Mode::Command(c) = &self.mode {
            if let Some(kind) = self.doc.feature(c.uid).map(|f| &f.kind) {
                let all = matches!(kind, FeatureKind::Loft { .. } | FeatureKind::Sweep { .. } | FeatureKind::Pipe { .. });
                if all {
                    for f in self.doc.features.iter().take(self.doc.rollback).filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })) {
                        if !v.contains(&f.uid) {
                            v.push(f.uid);
                        }
                    }
                }
                for sk in used_sketches(kind) {
                    if !v.contains(&sk) {
                        v.push(sk);
                    }
                }
            }
        }
        v
    }

    /// Плоскость грани тела в точке попадания.
    fn face_plane(&self, model: &Model, slot: usize, tri: usize) -> Option<(Plane, Vec<usize>)> {
        let b = model.bodies.get(slot)?;
        let mesh = b.mesh();
        let n = mesh.tri_normal(&mesh.triangles[tri]);
        let groups = self.face_groups(b);
        let g = groups[tri];
        let tris: Vec<usize> = groups.iter().enumerate().filter(|(_, x)| **x == g).map(|(i, _)| i).collect();
        let p0 = DVec3::from(mesh.vertices[mesh.triangles[tri][0] as usize]);
        // плоская ли грань (все треугольники группы с той же нормалью)
        let planar = tris.iter().all(|&t| mesh.tri_normal(&mesh.triangles[t]).dot(n) > 0.9999);
        if !planar {
            return None;
        }
        Some((Plane::from_point_normal(p0, n), tris))
    }

    /// Плоские грани тела (кэш по телу: считаются один раз).
    fn face_groups(&self, b: &fusion_core::doc::Body) -> Arc<Vec<u32>> {
        let key = Arc::as_ptr(&b.solid) as usize;
        let mut cache = self.face_groups.borrow_mut();
        if cache.len() > 512 {
            cache.clear();
        }
        cache.entry(key).or_insert_with(|| Arc::new(b.mesh().planar_faces())).clone()
    }

    fn handle_click(&mut self, hit: Option<Hit>, shift: bool, now: f64) {
        let model = self.ev.model();
        match &mut self.mode {
            Mode::Idle => match hit {
                Some(Hit::Body { slot, .. }) => {
                    let id = model.bodies[slot].id;
                    if shift {
                        if let Some(i) = self.selection.iter().position(|x| *x == id) {
                            self.selection.remove(i);
                        } else {
                            self.selection.push(id);
                        }
                    } else {
                        self.selection = vec![id];
                    }
                }
                _ => {
                    if !shift {
                        self.selection.clear();
                    }
                }
            },
            Mode::Measure(pts) => {
                if let Some(Hit::Body { slot, t, .. }) = hit {
                    let (o, d) = self.last_ray;
                    let mut p = o + d * t;
                    let snap = self.cam.world_per_pixel(self.viewport) * 12.0;
                    if let Some(v) = model.bodies[slot].mesh().vertices.iter().map(|v| DVec3::from(*v)).filter(|v| v.distance(p) < snap).min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))) {
                        p = v;
                    }
                    if pts.len() >= 2 {
                        pts.clear();
                    }
                    pts.push(p);
                }
            }
            Mode::PickPlane => match hit {
                Some(Hit::UserPlane(i, _)) => {
                    if let Some((_, _, pl)) = self.doc.offset_planes().get(i) {
                        let pl = *pl;
                        self.begin_sketch_on(pl, now)
                    }
                }
                Some(Hit::OriginPlane(i, _)) => self.begin_sketch_on(ORIGIN_PLANES[i], now),
                Some(Hit::Body { slot, tri, .. }) => match self.face_plane(&model, slot, tri) {
                    Some((pl, _)) => self.begin_sketch_on(pl, now),
                    None => self.toast(tr("Чертёж можно создать только на плоской грани"), true, now),
                },
                _ => {}
            },
            Mode::Command(cmd) => {
                let uid = cmd.uid;
                let pick = cmd.pick;
                // грань для отверстия и ребро для фаски определяем до заимствования документа
                let face = match (pick, hit) {
                    (PickTarget::Face, Some(Hit::Body { slot, tri, t })) => self.face_plane(&model, slot, tri).map(|(pl, _)| {
                        let (o, d) = self.last_ray;
                        (pl, pl.to_local(o + d * t))
                    }),
                    _ => None,
                };
                let edge = self.hover_edge;
                let user_planes = self.doc.offset_planes();
                let Some(f) = self.doc.feature_mut(uid) else { return };
                let mut changed = false;
                match (&mut f.kind, hit, pick) {
                    (FeatureKind::OffsetPlane { base, .. }, _, PickTarget::Face) => {
                        if let Some((pl, _)) = face {
                            *base = pl;
                            changed = true;
                        } else if let Some(Hit::OriginPlane(i, _)) = hit {
                            *base = ORIGIN_PLANES[i];
                            changed = true;
                        } else if let Some(Hit::UserPlane(i, _)) = hit {
                            if let Some((_, _, pl)) = user_planes.get(i) {
                                *base = *pl;
                                changed = true;
                            }
                        }
                    }
                    (FeatureKind::Shell { plane, .. }, _, PickTarget::Face) => {
                        if let Some((pl, _)) = face {
                            *plane = pl;
                            changed = true;
                        }
                    }
                    (FeatureKind::Hole { plane, at, .. }, _, PickTarget::Face) => {
                        if let Some((pl, loc)) = face {
                            *plane = pl;
                            at[0] = Expr::num((loc[0] * 10.0).round() / 10.0);
                            at[1] = Expr::num((loc[1] * 10.0).round() / 10.0);
                            changed = true;
                        }
                    }
                    (FeatureKind::Chamfer { edges, .. } | FeatureKind::Fillet { edges, .. }, _, PickTarget::Edges) => {
                        if let Some(e) = edge {
                            let same = |x: &EdgeRef| x.circle == e.circle && (DVec3::from(x.a) - DVec3::from(e.a)).length() < 1e-6 && (DVec3::from(x.b) - DVec3::from(e.b)).length() < 1e-6;
                            if let Some(i) = edges.iter().position(same) {
                                edges.remove(i);
                            } else {
                                edges.push(e);
                            }
                            changed = true;
                        }
                    }
                    (FeatureKind::Loft { profiles, .. }, Some(Hit::Profile { sketch: s, region, .. }), _) => {
                        let regions = self.ev.sketch(s).map(|r| r.regions.clone()).unwrap_or_default();
                        if let Some(i) = profiles.iter().position(|(u, p)| *u == s && p.resolve(&regions) == Some(region)) {
                            profiles.remove(i);
                        } else if let Some(r) = regions.get(region) {
                            profiles.retain(|(u, _)| *u != s);
                            profiles.push((s, ProfileRef::of(r)));
                        }
                        changed = true;
                    }
                    (FeatureKind::Extrude { sketch, profiles, .. } | FeatureKind::Revolve { sketch, profiles, .. } | FeatureKind::Sweep { sketch, profiles, .. }, Some(Hit::Profile { sketch: s, region, .. }), _) => {
                        if *sketch != s {
                            *sketch = s;
                            profiles.clear();
                        }
                        // переключаем: повторный щелчок по профилю снимает выбор
                        let regions = self.ev.sketch(s).map(|r| r.regions.clone()).unwrap_or_default();
                        if let Some(i) = profiles.iter().position(|p| p.resolve(&regions) == Some(region)) {
                            profiles.remove(i);
                        } else if let Some(r) = regions.get(region) {
                            profiles.push(ProfileRef::of(r));
                        }
                        changed = true;
                    }
                    (FeatureKind::Move { bodies, .. } | FeatureKind::Mirror { bodies, .. } | FeatureKind::Pattern { bodies, .. } | FeatureKind::Remove { bodies } | FeatureKind::Scale { bodies, .. }, Some(Hit::Body { slot, .. }), _) => {
                        let id = model.bodies[slot].id;
                        if let Some(i) = bodies.iter().position(|x| *x == id) {
                            bodies.remove(i);
                        } else {
                            bodies.push(id);
                        }
                        changed = true;
                    }
                    (FeatureKind::Combine { target, tools, .. }, Some(Hit::Body { slot, .. }), p) => {
                        let id = model.bodies[slot].id;
                        if p == PickTarget::CombineTarget {
                            *target = id;
                            tools.retain(|t| *t != id);
                            if let Mode::Command(c) = &mut self.mode {
                                c.pick = PickTarget::CombineTools;
                            }
                        } else if id != *target {
                            if let Some(i) = tools.iter().position(|x| *x == id) {
                                tools.remove(i);
                            } else {
                                tools.push(id);
                            }
                        }
                        changed = true;
                    }
                    _ => {}
                }
                if changed {
                    self.after_doc_change();
                }
            }
            Mode::Sketch(_) => {}
        }
    }

    // ------------------------------------------------------------ интерфейс

    fn menu_bar(&mut self, ui: &mut egui::Ui, now: f64) {
        egui::menu::bar(ui, |ui| {
            ui.add(egui::Image::from_bytes("bytes://genix/logo.svg", crate::home::LOGO_SVG.as_bytes().to_vec()).fit_to_exact_size(egui::vec2(18.0, 18.0)));
            ui.menu_button(tr("Файл"), |ui| {
                if menu_item(ui, "file_new", tr("Новый                ⌘N")) {
                    ui.close_menu();
                    self.request(Confirm::New);
                }
                if menu_item(ui, "file_open", tr("Открыть…            ⌘O")) {
                    ui.close_menu();
                    self.request(Confirm::Open);
                }
                ui.menu_button(tr("Открыть пример"), |ui| {
                    if ui.button(tr("Кронштейн (параметрический)")).clicked() {
                        ui.close_menu();
                        self.request(Confirm::OpenExample(crate::examples::bracket));
                    }
                    if ui.button(tr("Втулка (вращение)")).clicked() {
                        ui.close_menu();
                        self.request(Confirm::OpenExample(crate::examples::bushing));
                    }
                });
                ui.separator();
                if menu_item(ui, "file_save", tr("Сохранить           ⌘S")) {
                    ui.close_menu();
                    self.save(false, now);
                }
                if menu_item(ui, "file_saveas", tr("Сохранить как…    ⇧⌘S")) {
                    ui.close_menu();
                    self.save(true, now);
                }
                ui.separator();
                if menu_item(ui, "file_export_stl", tr("Экспорт STL (3D-печать)…")) {
                    ui.close_menu();
                    self.export(false, now);
                }
                if menu_item(ui, "sk_photo", tr("Снимок модели PNG без фона…")) {
                    ui.close_menu();
                    self.export_png(now);
                }
                if menu_item(ui, "file_export_obj", tr("Экспорт OBJ…")) {
                    ui.close_menu();
                    self.export(true, now);
                }
            });
            ui.menu_button(tr("Правка"), |ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(tr("Отменить        ⌘Z"))).clicked() {
                    ui.close_menu();
                    self.do_undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new(tr("Повторить     ⇧⌘Z"))).clicked() {
                    ui.close_menu();
                    self.do_redo();
                }
                ui.separator();
                if ui.button(tr("Параметры…")).clicked() {
                    ui.close_menu();
                    self.params_open = true;
                }
                ui.separator();
                if menu_item(ui, "settings", tr("Настройки…")) {
                    ui.close_menu();
                    self.settings_open = true;
                }
            });
            ui.menu_button(tr("Вид"), |ui| {
                for (t, name) in [(ThemeChoice::Auto, "Тема: как в системе"), (ThemeChoice::Light, "Тема: светлая"), (ThemeChoice::Dark, "Тема: тёмная")] {
                    if ui.radio_value(&mut self.settings.theme, t, tr(name)).clicked() {
                        self.settings.save();
                        ui.close_menu();
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.settings.show_left_panel, tr("Левая панель"));
                ui.checkbox(&mut self.settings.show_right_panel, tr("Правая панель"));
                ui.separator();
                ui.checkbox(&mut self.settings.show_grid, tr("Сетка"));
                ui.checkbox(&mut self.show_planes, tr("Плоскости начала координат"));
                if ui.button(tr("Вписать модель           F")).clicked() {
                    ui.close_menu();
                    self.need_fit = true;
                }
            });
            ui.menu_button(tr("Справка"), |ui| {
                if ui.button(tr("Управление и горячие клавиши")).clicked() {
                    ui.close_menu();
                    self.help_open = true;
                }
            });
            ui.separator();
            if ui.add_enabled(!self.undo.is_empty() && !matches!(self.mode, Mode::Sketch(_)), egui::Button::new("↶")).on_hover_text(tr("Отменить (⌘Z)")).clicked() {
                self.do_undo();
            }
            if ui.add_enabled(!self.redo.is_empty() && !matches!(self.mode, Mode::Sketch(_)), egui::Button::new("↷")).on_hover_text(tr("Повторить (⇧⌘Z)")).clicked() {
                self.do_redo();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(self.title()).strong());
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, now: f64) {
        if let Mode::Sketch(s) = &mut self.mode {
            let act = egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| s.toolbar(ui)).inner;
            match act {
                SketchAction::Finish => self.finish_sketch(true),
                SketchAction::Cancel => self.finish_sketch(false),
                SketchAction::None => {}
            }
            return;
        }
        let busy = matches!(self.mode, Mode::Command(_) | Mode::PickPlane | Mode::Measure(_));
        let active = match &self.mode {
            Mode::Command(c) => self.doc.feature(c.uid).map(|f| tool_id(&f.kind)),
            Mode::PickPlane => Some("sketch"),
            Mode::Measure(_) => Some("measure"),
            _ => None,
        };
        ui.horizontal(|ui| {
            let chosen = toolbar::solid_toolbar(ui, !busy, active);
            ui.separator();
            ui.vertical(|ui| {
                ui.add_space(2.0);
                if toolbar::tool_button(ui, "params", "ƒx", tr("Параметры"), true, false).on_hover_text(tr("Именованные размеры, которые можно использовать в любых полях")).clicked() {
                    self.params_open = true;
                }
            });
            if let Some(id) = chosen {
                self.run_tool(id, now);
            }
        });
    }

    fn browser(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.add_space(4.0);
        ui.label(RichText::new(tr("ДЕРЕВО")).small().strong().weak());
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            tree_group(ui, "t_doc", "tree_doc_settings", tr("Настройки документа").to_string(), false, |ui| {
                ui.label(tr("Единицы: мм, г"));
            });
            tree_group(ui, "t_views", "tree_views", tf!("Именованные виды ({})", self.doc.views.len()), false, |ui| {
                let mut go = None;
                let mut del = None;
                for (i, v) in self.doc.views.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.selectable_label(false, format!("▸ {}", v.name)).on_hover_text(tr("Перейти к виду")).clicked() {
                            go = Some(i);
                        }
                        if ui.small_button("×").clicked() {
                            del = Some(i);
                        }
                    });
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_view_name).hint_text(tr("имя вида")).desired_width(110.0));
                    if ui.button(tr("Сохранить вид")).clicked() {
                        let name = if self.new_view_name.trim().is_empty() { format!("{} {}", tr("Вид"), self.doc.views.len() + 1) } else { self.new_view_name.trim().to_string() };
                        self.checkpoint();
                        self.doc.views.push(fusion_core::doc::NamedView { name, yaw: self.cam.yaw, pitch: self.cam.pitch, distance: self.cam.distance, target: self.cam.target.to_array() });
                        self.new_view_name.clear();
                    }
                });
                if let Some(i) = go {
                    let v = self.doc.views[i].clone();
                    self.cam.animate_to(v.yaw, v.pitch, Some(v.distance), Some(DVec3::from(v.target)), now);
                }
                if let Some(i) = del {
                    self.checkpoint();
                    self.doc.views.remove(i);
                }
            });
            tree_group(ui, "t_origin", "tree_origin", tr("Начало координат").to_string(), false, |ui| {
                ui.checkbox(&mut self.show_planes, tr("Показывать плоскости"));
                ui.checkbox(&mut self.settings.show_grid, tr("Показывать сетку"));
            });
            let model = self.shown();
            tree_group(ui, "t_bodies", "tree_bodies", tf!("Тела ({})", model.bodies.len()), true, |ui| {
                if model.bodies.is_empty() {
                    ui.label(RichText::new(tr("Пока нет тел")).weak());
                }
                for b in &model.bodies {
                    ui.horizontal(|ui| {
                        let mut vis = !self.doc.hidden_bodies.contains(&b.id);
                        if eye_toggle(ui, &mut vis, tr("Показать/скрыть")) {
                            if vis {
                                self.doc.hidden_bodies.remove(&b.id);
                            } else {
                                self.doc.hidden_bodies.insert(b.id);
                            }
                        }
                        if let Some((id, text)) = &mut self.rename_body {
                            if *id == b.id {
                                let r = ui.text_edit_singleline(text);
                                r.request_focus();
                                if r.lost_focus() {
                                    let t = text.trim().to_string();
                                    let id = *id;
                                    self.rename_body = None;
                                    if !t.is_empty() {
                                        self.checkpoint();
                                        self.doc.body_names.insert(id, t);
                                    }
                                }
                                return;
                            }
                        }
                        let sel = self.selection.contains(&b.id);
                        let r = ui.selectable_label(sel, tr_name(self.doc.body_name(b)));
                        if r.clicked() {
                            if ui.input(|i| i.modifiers.shift) {
                                if sel {
                                    self.selection.retain(|x| *x != b.id);
                                } else {
                                    self.selection.push(b.id);
                                }
                            } else {
                                self.selection = vec![b.id];
                            }
                        }
                        if r.double_clicked() {
                            self.rename_body = Some((b.id, self.doc.body_name(b).to_string()));
                        }
                        r.context_menu(|ui| {
                            if ui.button(tr("Переименовать")).clicked() {
                                self.rename_body = Some((b.id, self.doc.body_name(b).to_string()));
                                ui.close_menu();
                            }
                            if ui.button(tr("Удалить тело")).clicked() {
                                ui.close_menu();
                                self.delete_bodies(vec![b.id]);
                            }
                        });
                    });
                }
            });
            let sketches: Vec<(u64, String, bool)> = self
                .doc
                .features
                .iter()
                .take(self.doc.rollback)
                .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }))
                .map(|f| (f.uid, f.name.clone(), self.ev.error(f.uid).is_some()))
                .collect();
            tree_group(ui, "t_sketches", "tree_sketches", tf!("Чертежи ({})", sketches.len()), true, |ui| {
                for (uid, name, err) in sketches {
                    ui.horizontal(|ui| {
                        let mut vis = self.doc.visible_sketches.contains(&uid);
                        if eye_toggle(ui, &mut vis, tr("Показать/скрыть чертёж")) {
                            if vis {
                                self.doc.visible_sketches.insert(uid);
                            } else {
                                self.doc.visible_sketches.remove(&uid);
                            }
                        }
                        let name = tr_name(&name);
                        let text = if err { RichText::new(format!("{name} ⚠")).color(Color32::from_rgb(200, 40, 40)) } else { RichText::new(name.to_string()) };
                        let r = ui.selectable_label(false, text).on_hover_text(tr("Двойной щелчок — изменить чертёж"));
                        if r.double_clicked() {
                            self.edit_feature(uid, now);
                        }
                    });
                }
            });
            tree_group(ui, "t_params", "tree_params", tf!("Параметры ({})", self.doc.params.items.len()), false, |ui| {
                for (k, v) in &self.doc.params.items {
                    let val = self.doc.params.eval(&v.0).map(format_num).unwrap_or_else(|_| "?".into());
                    ui.label(format!("{k} = {val}"));
                }
                if ui.button(tr("Изменить…")).clicked() {
                    self.params_open = true;
                }
            });
        });
    }

    fn timeline(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("ИСТОРИЯ")).small().strong().weak());
            let n = self.doc.features.len();
            let enabled = matches!(self.mode, Mode::Idle);
            let roll = |ui: &mut egui::Ui, id: &str, tip: &str, to: usize, me: &mut Self| {
                let btn = match crate::icons::icon(id, 16.0, ui.visuals().text_color()) {
                    Some(img) => egui::Button::image(img),
                    None => egui::Button::new(id).small(),
                };
                if ui.add_enabled(enabled, btn).on_hover_text(tip).clicked() {
                    me.doc.rollback = to.min(n);
                    me.after_doc_change();
                }
            };
            let r = self.doc.rollback;
            roll(ui, "tl_first", tr("В начало истории"), 0, self);
            roll(ui, "tl_prev", tr("Шаг назад"), r.saturating_sub(1), self);
            roll(ui, "tl_next", tr("Шаг вперёд"), r + 1, self);
            roll(ui, "tl_last", tr("В конец истории"), n, self);
            let play_btn = match crate::icons::icon(if self.playing { "tl_pause" } else { "tl_play" }, 16.0, ui.visuals().text_color()) {
                Some(img) => egui::Button::image_and_text(img, if self.playing { tr("Пауза") } else { tr("Воспроизвести") }),
                None => egui::Button::new(if self.playing { tr("Пауза") } else { tr("▶ Воспроизвести") }).small(),
            };
            if ui.add_enabled(enabled && n > 0, play_btn).on_hover_text(tr("Показать построение шаг за шагом")).clicked() {
                self.playing = !self.playing;
                if self.playing {
                    if self.doc.rollback >= n {
                        self.doc.rollback = 0;
                        self.after_doc_change();
                    }
                    self.last_play = self.now;
                }
            }
            ui.separator();
            egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let features: Vec<(u64, String, bool, &'static str)> = self.doc.features.iter().map(|f| (f.uid, f.name.clone(), f.suppressed, f.kind.type_label())).collect();
                    for (i, (uid, name, suppressed, typ)) in features.into_iter().enumerate() {
                        if i == self.doc.rollback {
                            marker(ui);
                        }
                        let active = i < self.doc.rollback;
                        let err = self.ev.error(uid).map(|s| s.to_string());
                        let editing = matches!(&self.mode, Mode::Command(c) if c.uid == uid) || matches!(&self.mode, Mode::Sketch(s) if s.uid == Some(uid));
                        let icon = match typ {
                            "Чертёж" => "✏",
                            "Выдавливание" => "⬆",
                            "Вращение" => "⟳",
                            "Перемещение" => "✚",
                            "Зеркало" => "⇋",
                            "Массив" => "▦",
                            "Комбинирование" => "∪",
                            "Удаление" => "✖",
                            "Сфера" => "●",
                            "Цилиндр" => "▮",
                            "Тор" => "◯",
                            "Отверстие" => "◎",
                            "Спираль" => "≋",
                            "Труба" => "⊡",
                            "По траектории" => "↝",
                            "Переход" => "◣",
                            "Масштаб" => "⇲",
                            "Фаска" => "◢",
                            "Скругление" => "◠",
                            _ => "◼",
                        };
                        if let Some((ruid, text)) = &mut self.rename_feature {
                            if *ruid == uid {
                                let r = ui.add(egui::TextEdit::singleline(text).desired_width(120.0));
                                r.request_focus();
                                if r.lost_focus() {
                                    let t = text.trim().to_string();
                                    self.rename_feature = None;
                                    if !t.is_empty() {
                                        self.checkpoint();
                                        if let Some(f) = self.doc.feature_mut(uid) {
                                            f.name = t;
                                        }
                                    }
                                }
                                continue;
                            }
                        }
                        let mut text = RichText::new(format!("{icon} {}", tr_name(&name)));
                        if suppressed {
                            text = text.strikethrough().weak();
                        }
                        if !active {
                            text = text.weak();
                        }
                        let mut btn = egui::Button::new(text).selected(editing);
                        if err.is_some() && active && !editing {
                            btn = btn.fill(Color32::from_rgb(250, 205, 205));
                        }
                        let tip = match &err {
                            Some(e) => tf!("{}: ошибка — {}", tr(typ), tr_dyn(e)),
                            None => {
                                let ms = self.ev.steps().get(i).map(|s| s.millis).unwrap_or(0.0);
                                tf!("{} · пересчёт {} мс\nДвойной щелчок — изменить, правая кнопка — меню", tr(typ), format!("{ms:.1}"))
                            }
                        };
                        let r = ui.add(btn).on_hover_text(tip);
                        if r.double_clicked() && enabled {
                            self.edit_feature(uid, now);
                        }
                        r.context_menu(|ui| {
                            if ui.button(tr("Изменить")).clicked() {
                                ui.close_menu();
                                self.edit_feature(uid, now);
                            }
                            if ui.button(tr("Переименовать")).clicked() {
                                ui.close_menu();
                                self.rename_feature = Some((uid, name.clone()));
                            }
                            if ui.button(tr("Откатить историю сюда")).clicked() {
                                ui.close_menu();
                                self.cancel_mode();
                                self.doc.rollback = i + 1;
                                self.after_doc_change();
                            }
                            if ui.button(if suppressed { tr("Включить") } else { tr("Отключить (подавить)") }).clicked() {
                                ui.close_menu();
                                self.cancel_mode();
                                self.checkpoint();
                                if let Some(f) = self.doc.feature_mut(uid) {
                                    f.suppressed = !f.suppressed;
                                }
                                self.after_doc_change();
                            }
                            ui.separator();
                            if ui.button(RichText::new(tr("Удалить")).color(Color32::from_rgb(200, 40, 40))).clicked() {
                                ui.close_menu();
                                self.cancel_mode();
                                self.delete_feature(uid, now);
                            }
                        });
                    }
                    if self.doc.rollback >= self.doc.features.len() {
                        marker(ui);
                    }
                    if self.doc.features.is_empty() {
                        ui.label(RichText::new(tr("Здесь будут шаги построения. Начните с кнопки «✏ Чертёж».")).weak());
                    }
                });
            });
        });
    }

    fn params_window(&mut self, ctx: &egui::Context) {
        let mut open = self.params_open;
        egui::Window::new(tr("Параметры")).open(&mut open).resizable(true).default_width(380.0).show(ctx, |ui| {
            ui.label(RichText::new(tr("Имена можно использовать в любом поле: например «ширина/2 + 5».")).weak());
            ui.separator();
            let mut remove = None;
            let mut changed = None;
            egui::Grid::new("params").num_columns(4).striped(true).show(ui, |ui| {
                ui.strong(tr("Имя"));
                ui.strong(tr("Выражение"));
                ui.strong(tr("Значение"));
                ui.label("");
                ui.end_row();
                let params = self.doc.params.clone();
                for (k, v) in &params.items {
                    ui.label(k);
                    let mut e = v.clone();
                    if expr_field(ui, ("param", k), &mut e, &params, "") {
                        changed = Some((k.clone(), e));
                    }
                    ui.label(params.eval(&v.0).map(format_num).unwrap_or_else(|e| tf!("ошибка: {}", tr_dyn(&e))));
                    if ui.small_button("×").on_hover_text(tr("Удалить параметр")).clicked() {
                        remove = Some(k.clone());
                    }
                    ui.end_row();
                }
            });
            if let Some((k, e)) = changed {
                self.checkpoint();
                self.doc.params.items.insert(k, e);
                self.after_doc_change();
            }
            if let Some(k) = remove {
                self.checkpoint();
                self.doc.params.items.remove(&k);
                self.after_doc_change();
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_param.0).hint_text(tr("имя")).desired_width(90.0));
                ui.label("=");
                ui.add(egui::TextEdit::singleline(&mut self.new_param.1).hint_text(tr("значение")).desired_width(110.0));
                let name = self.new_param.0.trim().to_string();
                let valid = Params::valid_name(&name) && !self.doc.params.items.contains_key(&name) && self.doc.params.eval(&self.new_param.1).is_ok();
                if ui.add_enabled(valid, egui::Button::new(tr("Добавить"))).clicked() {
                    self.checkpoint();
                    self.doc.params.set(&name, Expr(self.new_param.1.trim().to_string()));
                    self.new_param = (String::new(), String::new());
                    self.after_doc_change();
                }
            });
        });
        self.params_open = open;
    }

    fn help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.help_open;
        egui::Window::new(tr("Управление")).open(&mut open).default_width(420.0).show(ctx, |ui| {
            let rows = [
                (tr("Вращать вид"), tr("Левая кнопка мыши по пустому месту / по телу")),
                (tr("Сдвинуть вид"), tr("Правая или средняя кнопка, Shift+левая, два пальца на трекпаде")),
                (tr("Масштаб"), tr("Колесо мыши, щипок на трекпаде, ⌘+прокрутка")),
                (tr("Вписать модель"), tr("F или 🏠 над кубом видов")),
                (tr("Стандартный вид"), tr("Щелчок по грани куба видов")),
                (tr("Выбрать тело"), tr("Щелчок; Shift — добавить к выбору")),
                (tr("Чертёж / Выдавить / Переместить"), "S / E / M"),
                (tr("В чертеже: линия, прямоугольник, окружность, дуга"), "L / R / C / A"),
                (tr("В чертеже: размер"), tr("D, затем щелчок по линии/окружности")),
                (tr("Точная длина при рисовании"), tr("Наберите число и Enter")),
                (tr("Отменить / повторить"), "⌘Z / ⇧⌘Z"),
                (tr("Сохранить / открыть"), "⌘S / ⌘O"),
                (tr("Удалить выбранное"), "Delete"),
                (tr("Отменить действие"), "Esc"),
            ];
            egui::Grid::new("help").striped(true).num_columns(2).show(ui, |ui| {
                for (a, b) in rows {
                    ui.label(a);
                    ui.label(RichText::new(b).weak());
                    ui.end_row();
                }
            });
        });
        self.help_open = open;
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let model = self.ev.model();
        let sel: Vec<&fusion_core::doc::Body> = model.bodies.iter().filter(|b| self.selection.contains(&b.id)).collect();
        if sel.is_empty() {
            return;
        }
        let vol: f64 = sel.iter().map(|b| b.solid.volume()).sum();
        let area: f64 = sel.iter().map(|b| b.mesh().area()).sum();
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for b in &sel {
            if let Some((l, h)) = b.mesh().bounds() {
                lo = lo.min(l);
                hi = hi.max(h);
            }
        }
        let size = hi - lo;
        let r1 = |v: f64| format_num((v * 100.0).round() / 100.0);
        egui::Grid::new("inspector").num_columns(2).show(ui, |ui| {
            ui.label(tr("Выбрано тел"));
            ui.label(format!("{}", sel.len()));
            ui.end_row();
            ui.label(tr("Габариты"));
            ui.label(tf!("{} × {} × {} мм", r1(size.x), r1(size.y), r1(size.z)));
            ui.end_row();
            ui.label(tr("Объём"));
            ui.label(tf!("{} см³", r1(vol / 1000.0)));
            ui.end_row();
            ui.label(tr("Площадь"));
            ui.label(tf!("{} см²", r1(area / 100.0)));
            ui.end_row();
            ui.label(tr("Материал"));
            egui::ComboBox::from_id_salt("material").selected_text(tr(Materials::LIST[self.settings.material].0)).show_ui(ui, |ui| {
                for (i, (n, _)) in Materials::LIST.iter().enumerate() {
                    ui.selectable_value(&mut self.settings.material, i, tr(n));
                }
            });
            ui.end_row();
            ui.label(tr("Масса"));
            ui.label(tf!("{} г", r1(vol / 1000.0 * Materials::LIST[self.settings.material].1)));
            ui.end_row();
        });
        ui.horizontal(|ui| {
            if ui.button(tr("Удалить")).clicked() {
                let ids = self.selection.clone();
                self.delete_bodies(ids);
            }
            if ui.button(tr("Скрыть")).clicked() {
                for id in self.selection.clone() {
                    self.doc.hidden_bodies.insert(id);
                }
                self.selection.clear();
            }
        });
    }

    // ------------------------------------------------------------ 3D-вид

    fn viewport(&mut self, ui: &mut egui::Ui, now: f64) {
        let rect = ui.available_rect_before_wrap();
        self.viewport = rect;
        let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
        let ctx = ui.ctx().clone();
        let colors = self.theme.view();
        let model = self.shown();

        if self.need_fit {
            self.fit_view(now, true);
            self.need_fit = false;
        }
        if self.cam.update(now) {
            ctx.request_repaint();
        }

        // ---- ввод режима чертежа
        if let Mode::Sketch(s) = &mut self.mode {
            match s.handle(ui, &resp, &mut self.cam, rect, &self.doc.params) {
                SketchAction::Finish => self.finish_sketch(true),
                SketchAction::Cancel => self.finish_sketch(false),
                SketchAction::None => {}
            }
        }
        let sketch_dragging = matches!(&self.mode, Mode::Sketch(s) if s.tool == crate::sketcher::Tool::Select && s.is_busy());

        // ---- стрелка выдавливания: потянуть за кончик
        self.drive_manip(&resp, rect, &ctx);
        let manip_active = self.manip.is_some() || self.gizmo.is_some();

        // ---- навигация
        let shift = ctx.input(|i| i.modifiers.shift);
        let ctrl = ctx.input(|i| i.modifiers.ctrl || i.modifiers.command);
        let d = resp.drag_delta();
        if !sketch_dragging && !manip_active {
            let k = self.settings.orbit_speed as f64;
            let zoom_drag = |me: &mut Self| me.cam.zoom_at((1.0 + d.y as f64 * 0.01 * me.settings.zoom_speed as f64).clamp(0.5, 2.0), rect, resp.hover_pos());
            if self.settings.controls == ControlScheme::Blender {
                // как в Blender: средняя кнопка вращает, Shift — сдвиг, Ctrl — масштаб; левая — только выбор
                if resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Secondary) && shift) {
                    if shift {
                        self.cam.pan(d.x as f64, d.y as f64, rect);
                    } else if ctrl {
                        zoom_drag(self);
                    } else {
                        self.cam.orbit(d.x as f64 * k, d.y as f64 * k);
                    }
                }
            } else if resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) || (resp.dragged_by(egui::PointerButton::Primary) && shift) {
                self.cam.pan(d.x as f64, d.y as f64, rect);
            } else if resp.dragged_by(egui::PointerButton::Primary) {
                match self.nav {
                    NavMode::Pan => self.cam.pan(d.x as f64, d.y as f64, rect),
                    NavMode::Zoom => zoom_drag(self),
                    _ => self.cam.orbit(d.x as f64 * k, d.y as f64 * k),
                }
            }
        }
        if resp.hovered() {
            let cursor = resp.hover_pos();
            let mut zoom = 1.0f64;
            let zk = self.settings.zoom_speed as f64 * if self.settings.invert_zoom { -1.0 } else { 1.0 };
            let mut pan = egui::Vec2::ZERO;
            ctx.input(|i| {
                for e in &i.events {
                    if let egui::Event::MouseWheel { unit, delta, modifiers } = e {
                        match unit {
                            // колесо мыши — зум
                            egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                                if self.settings.controls == ControlScheme::Blender && modifiers.ctrl {
                                    pan.x += delta.y * 40.0; // Ctrl + колесо — сдвиг по горизонтали
                                } else if self.settings.controls == ControlScheme::Blender && modifiers.shift {
                                    pan.y += delta.y * 40.0; // Shift + колесо — по вертикали
                                } else {
                                    zoom *= 0.88f64.powf(delta.y as f64 * zk)
                                }
                            }
                            // трекпад: два пальца — пан, с ⌘/Ctrl — зум
                            egui::MouseWheelUnit::Point => {
                                if modifiers.command || modifiers.ctrl {
                                    zoom *= (1.0 - delta.y as f64 * 0.004 * zk).clamp(0.5, 2.0);
                                } else {
                                    pan += *delta;
                                }
                            }
                        }
                    }
                }
                // щипок
                let z = i.zoom_delta();
                if z != 1.0 {
                    zoom /= z as f64;
                }
            });
            if zoom != 1.0 {
                self.cam.zoom_at(zoom, rect, cursor);
            }
            if pan != egui::Vec2::ZERO {
                self.cam.pan(pan.x as f64, pan.y as f64, rect);
            }
        }

        // ---- выбор
        let picking = !matches!(self.mode, Mode::Sketch(_));
        self.hover = None;
        if picking && !resp.dragged() {
            if let Some(m) = resp.hover_pos() {
                let (o, dir) = self.cam.ray(rect, m);
                self.last_ray = (o, dir);
                self.hover = if manip_active { None } else { self.hit_test(&model, o, dir) };
            }
        }
        // ребро под курсором (фаска и скругление)
        self.hover_edge = None;
        if matches!(&self.mode, Mode::Command(c) if c.pick == PickTarget::Edges) && !resp.dragged() {
            if let Some(m) = resp.hover_pos() {
                self.hover_edge = self.pick_edge(&model, rect, m);
            }
        }
        if picking && resp.clicked() {
            self.handle_click(self.hover, shift, now);
        }
        if picking && resp.secondary_clicked() {
            self.context_body = match self.hover {
                Some(Hit::Body { slot, .. }) => model.bodies.get(slot).map(|b| b.id),
                _ => None,
            };
        }
        if matches!(self.mode, Mode::Idle) {
            let cb = self.context_body;
            resp.context_menu(|ui| {
                if let Some(id) = cb {
                    ui.label(RichText::new(self.body_name(id)).strong());
                    if ui.button(tr("Скрыть")).clicked() {
                        self.doc.hidden_bodies.insert(id);
                        self.selection.retain(|x| *x != id);
                        ui.close_menu();
                    }
                    if ui.button(tr("Переместить…")).clicked() {
                        self.selection = vec![id];
                        ui.close_menu();
                        self.start_command("move", now);
                    }
                    if ui.button(tr("Удалить")).clicked() {
                        ui.close_menu();
                        self.delete_bodies(vec![id]);
                    }
                    ui.separator();
                }
                if ui.button(tr("Новый чертёж")).clicked() {
                    ui.close_menu();
                    self.start_sketch();
                }
                if ui.button(tr("Вписать модель")).clicked() {
                    ui.close_menu();
                    self.need_fit = true;
                }
                if !self.doc.hidden_bodies.is_empty() && ui.button(tr("Показать все тела")).clicked() {
                    self.doc.hidden_bodies.clear();
                    ui.close_menu();
                }
            });
        }
        if resp.hovered() {
            let cursor = match (&self.mode, self.hover) {
                (Mode::Sketch(_), _) => egui::CursorIcon::Crosshair,
                _ if manip_active => egui::CursorIcon::Grabbing,
                (_, Some(_)) => egui::CursorIcon::PointingHand,
                _ if self.hover_edge.is_some() || self.handle_hot(rect, &ctx) => egui::CursorIcon::PointingHand,
                _ => egui::CursorIcon::Default,
            };
            ctx.set_cursor_icon(cursor);
        }

        self.cam.fov_deg = if self.ortho { 5.0 } else { self.settings.fov_deg as f64 };
        // ---- данные для GPU
        let model_ptr = Arc::as_ptr(&model) as usize;
        let ew_bits = self.settings.edge_width.to_bits();
        if model_ptr != self.shown_model || ew_bits != self.shown_edge_w {
            self.shown_model = model_ptr;
            self.shown_edge_w = ew_bits;
            self.upload = Some(self.cache.assemble(&model.bodies, self.settings.edge_width));
        }
        let overlay = self.build_overlay(&model, rect, &colors);
        let mut body_colors = Vec::with_capacity(model.bodies.len());
        let hover_slot = match self.hover {
            Some(Hit::Body { slot, .. }) if matches!(self.mode, Mode::Idle) || matches!(&self.mode, Mode::Command(c) if c.pick != PickTarget::Profiles) => Some(slot),
            _ => None,
        };
        let cmd_bodies: Vec<u64> = match &self.mode {
            Mode::Command(c) => match self.doc.feature(c.uid).map(|f| &f.kind) {
                Some(FeatureKind::Move { bodies, .. } | FeatureKind::Mirror { bodies, .. } | FeatureKind::Pattern { bodies, .. } | FeatureKind::Remove { bodies }) => bodies.clone(),
                Some(FeatureKind::Combine { target, tools, .. }) => std::iter::once(*target).chain(tools.iter().copied()).collect(),
                _ => vec![],
            },
            _ => vec![],
        };
        for (slot, b) in model.bodies.iter().enumerate() {
            let c = if self.doc.hidden_bodies.contains(&b.id) {
                [0.0; 4]
            } else if self.selection.contains(&b.id) || cmd_bodies.contains(&b.id) {
                linear(colors.body_selected)
            } else if hover_slot == Some(slot) {
                linear(colors.body_hover)
            } else {
                linear(colors.body)
            };
            body_colors.push(c);
        }
        let eye = self.cam.eye().as_vec3();
        let light = self.cam.light_dir().as_vec3();
        let frame = FrameUniform {
            view_proj: self.cam.view_proj_f32(rect).to_cols_array_2d(),
            eye: [eye.x, eye.y, eye.z, 1.0],
            light: [light.x, light.y, light.z, 0.0],
            viewport: [rect.width(), rect.height(), 0.0, 0.0],
            bg_top: linear(colors.bg_top),
            bg_bottom: linear(colors.bg_bottom),
            edge_color: linear(colors.edge),
        };
        let cb = ViewCallback { frame, colors: body_colors, overlay, upload: self.upload.take(), style: self.settings.visual_style.parts() };
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(rect, cb));

        // ---- 2D поверх 3D
        if let Mode::Sketch(s) = &mut self.mode {
            s.paint(ui, &self.cam, rect, &colors, &self.doc.params);
        }
        self.paint_labels(ui, rect, &model);
        self.nav_bar(ui, rect, now);
        self.value_box(ui, rect);
        match viewcube::show(ui, rect, &self.cam, self.theme == Theme::Dark) {
            CubeAction::View(n, r) => {
                let (yaw, pitch) = Camera::angles_for(n, r);
                self.cam.animate_to(yaw, pitch, None, None, now);
            }
            CubeAction::Home => self.fit_view(now, true),
            CubeAction::None => {}
        }
    }

    fn fit_view(&mut self, now: f64, iso: bool) {
        let model = self.shown();
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for b in model.bodies.iter().filter(|b| !self.doc.hidden_bodies.contains(&b.id)) {
            if let Some((l, h)) = b.mesh().bounds() {
                lo = lo.min(l);
                hi = hi.max(h);
            }
        }
        for uid in self.sketches_to_show() {
            if let Some(s) = self.ev.sketch(uid) {
                for p in &s.sketch.points {
                    let w = s.plane.to_world(p.pos);
                    lo = lo.min(w);
                    hi = hi.max(w);
                }
            }
        }
        if lo.x > hi.x {
            lo = DVec3::splat(-50.0);
            hi = DVec3::splat(50.0);
        }
        let (dist, center) = self.cam.fit_params(lo, hi, self.viewport);
        let (yaw, pitch) = if iso { Camera::ISO } else { (self.cam.yaw, self.cam.pitch) };
        self.cam.animate_to(yaw, pitch, Some(dist), Some(center), now);
    }

    fn build_overlay(&self, model: &Model, rect: Rect, c: &crate::theme::ViewColors) -> Overlay {
        let mut ov = Overlay::default();
        let in_sketch = matches!(self.mode, Mode::Sketch(_));
        // сетка на XY (в чертеже — своя сетка на плоскости чертежа)
        if self.settings.show_grid && !in_sketch {
            let wpp = self.cam.world_per_pixel(rect);
            let center = Plane::XY.to_local(self.cam.target);
            let ext = (self.cam.distance * 1.5).max(wpp * 800.0);
            render::grid_adaptive(&mut ov.lines, &Plane::XY, center, wpp, ext, c.grid_minor, c.grid_major);
            ov.lines.push(render::line(DVec3::new(-ext, 0.0, 0.0), DVec3::new(ext, 0.0, 0.0), Color32::from_rgb(210, 70, 70), 1.4));
            ov.lines.push(render::line(DVec3::new(0.0, -ext, 0.0), DVec3::new(0.0, ext, 0.0), Color32::from_rgb(70, 170, 70), 1.4));
        }
        // плоскости начала координат
        if self.show_origin_planes() {
            let s = self.origin_plane_size();
            for (i, pl) in ORIGIN_PLANES.iter().enumerate() {
                let hov = matches!(self.hover, Some(Hit::OriginPlane(k, _)) if k == i);
                // как в Fusion: «пол» светлый, две стенки оранжевые; квадраты стоят в углу осей
                let base = if i == 0 { Color32::from_rgba_unmultiplied(225, 225, 232, 150) } else { Color32::from_rgba_unmultiplied(235, 150, 70, 95) };
                let col = if hov { c.plane_hover } else { base };
                let (lo, hi) = (0.1 * s, 1.1 * s);
                fill_quad(&mut ov.tris, pl, [lo, lo], [hi, hi], col);
                let corners = [[lo, lo], [hi, lo], [hi, hi], [lo, hi]].map(|q| pl.to_world(q));
                polyline(&mut ov.lines, &corners, if hov { c.plane_hover.to_opaque() } else { base.to_opaque() }, 1.2, true);
            }
            // яркие оси из начала координат
            for (d, col) in [(DVec3::X, Color32::from_rgb(235, 40, 40)), (DVec3::Y, Color32::from_rgb(40, 210, 40)), (DVec3::Z, Color32::from_rgb(50, 80, 255))] {
                ov.lines.push(render::line(DVec3::ZERO, d * s * 1.5, col, 2.6));
            }
        }
        // смещённые плоскости
        for (i, (_, _, pl)) in self.doc.offset_planes().iter().enumerate() {
            let s = self.origin_plane_size();
            let cl = pl.to_local(self.cam.target);
            let hov = matches!(self.hover, Some(Hit::UserPlane(k, _)) if k == i);
            let col = if hov { c.plane_hover } else { c.plane.gamma_multiply(0.7) };
            let (a, b) = ([cl[0] - s, cl[1] - s], [cl[0] + s, cl[1] + s]);
            fill_quad(&mut ov.tris, pl, a, b, col);
            let corners = [[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]].map(|q| pl.to_world(q));
            polyline(&mut ov.lines, &corners, col.to_opaque(), 1.2, true);
        }
        // измерение
        if let Mode::Measure(pts) = &self.mode {
            let col = Color32::from_rgb(255, 140, 0);
            let r = self.cam.world_per_pixel(rect) * 7.0;
            for p in pts {
                for d in [DVec3::X, DVec3::Y, DVec3::Z] {
                    ov.lines_top.push(render::line(*p - d * r, *p + d * r, col, 2.5));
                }
            }
            if pts.len() == 2 {
                ov.lines_top.push(render::line(pts[0], pts[1], col, 2.5));
            } else if let (1, Some(Hit::Body { t, .. })) = (pts.len(), self.hover) {
                let (o, d) = self.last_ray;
                ov.lines_top.push(render::line(pts[0], o + d * t, col.gamma_multiply(0.6), 1.5));
            }
        }
        // подсветка грани при выборе плоскости чертежа или места отверстия
        let face_pick = matches!(self.mode, Mode::PickPlane) || matches!(&self.mode, Mode::Command(cm) if cm.pick == PickTarget::Face);
        let face_pick = face_pick;
        if let (true, Some(Hit::Body { slot, tri, .. })) = (face_pick, self.hover) {
            if let Some((_, tris)) = self.face_plane(model, slot, tri) {
                fill_tris(&mut ov.tris, model.bodies[slot].mesh(), tris.into_iter(), c.plane_hover);
            }
        }
        // выбранные и подсвеченные рёбра (фаска, скругление)
        if let Mode::Command(cm) = &self.mode {
            if let Some(FeatureKind::Chamfer { edges, .. } | FeatureKind::Fillet { edges, .. }) = self.doc.feature(cm.uid).map(|f| &f.kind) {
                for e in edges {
                    for w in e.polyline().windows(2) {
                        ov.lines_top.push(render::line(w[0], w[1], c.sketch_selected, 4.5));
                    }
                }
            }
            if let (PickTarget::Edges, Some(e)) = (cm.pick, self.hover_edge) {
                for w in e.polyline().windows(2) {
                    ov.lines_top.push(render::line(w[0], w[1], c.sketch_hover, 4.0));
                }
            }
        }
        // манипулятор перемещения: стрелки по осям и дуги поворота
        if let Some((c0, t, r, len)) = self.move_gizmo(rect) {
            let c = c0 + DVec3::from(t);
            let _ = r;
            for (i, dir) in [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate() {
                let hot = self.gizmo.as_ref().is_some_and(|g| g.axis == i && !g.ring);
                let col = if hot { Color32::from_rgb(255, 200, 60) } else { GIZMO_COLORS[i] };
                let tip = c + dir * len;
                ov.lines_top.push(render::line(c, tip, col, 3.0));
                let (x, y) = fusion_core::geom::perp_frame(dir);
                let head = len * 0.16;
                let base = tip - dir * head;
                let cone: Vec<DVec3> = (0..8).map(|k| base + (x * (std::f64::consts::TAU * k as f64 / 8.0).cos() + y * (std::f64::consts::TAU * k as f64 / 8.0).sin()) * head * 0.45).collect();
                for (k, q) in cone.iter().enumerate() {
                    ov.lines_top.push(render::line(tip, *q, col, 2.4));
                    ov.lines_top.push(render::line(*q, cone[(k + 1) % cone.len()], col, 2.0));
                }
                // дуга поворота вокруг этой оси
                let hotr = self.gizmo.as_ref().is_some_and(|g| g.axis == i && g.ring);
                let rc = if hotr { Color32::from_rgb(255, 200, 60) } else { col.gamma_multiply(0.85) };
                let pts: Vec<DVec3> = (0..=40).map(|k| {
                    let a = std::f64::consts::TAU * k as f64 / 40.0;
                    c + (x * a.cos() + y * a.sin()) * len * 0.62
                }).collect();
                for w in pts.windows(2) {
                    ov.lines_top.push(render::line(w[0], w[1], rc, 2.2));
                }
            }
        }
        // стрелка выдавливания
        if let Some((base, dir, dist)) = self.extrude_handle() {
            let tip = base + dir * dist;
            let wpp = self.cam.world_per_pixel(rect);
            let hot = self.manip.is_some();
            let col = if hot { Color32::from_rgb(255, 150, 30) } else { crate::theme::ACCENT };
            ov.lines_top.push(render::line(base, tip, col, 3.0));
            let (x, y) = fusion_core::geom::perp_frame(dir);
            let head = wpp * 16.0 * dist.signum();
            let ring: Vec<DVec3> = (0..10).map(|i| {
                let a = std::f64::consts::TAU * i as f64 / 10.0;
                tip - dir * head + (x * a.cos() + y * a.sin()) * head.abs() * 0.38
            }).collect();
            for (i, q) in ring.iter().enumerate() {
                ov.lines_top.push(render::line(tip, *q, col, 2.5));
                ov.lines_top.push(render::line(*q, ring[(i + 1) % ring.len()], col, 2.0));
            }
        }
        // видимые чертежи (не активный)
        let (cmd_sel, cmd_pick): (Vec<(u64, ProfileRef)>, bool) = match &self.mode {
            Mode::Command(cmd) => match self.doc.feature(cmd.uid).map(|f| &f.kind) {
                Some(FeatureKind::Extrude { sketch, profiles, .. } | FeatureKind::Revolve { sketch, profiles, .. } | FeatureKind::Sweep { sketch, profiles, .. }) => {
                    (profiles.iter().map(|p| (*sketch, p.clone())).collect(), true)
                }
                Some(FeatureKind::Loft { profiles, .. }) => (profiles.clone(), true),
                _ => (vec![], false),
            },
            _ => (vec![], false),
        };
        let editing = if let Mode::Sketch(s) = &self.mode { s.uid } else { None };
        let no_profiles = self.path_sketch();
        for uid in self.sketches_to_show() {
            if Some(uid) == editing {
                continue;
            }
            let Some(sk) = self.ev.sketch(uid) else { continue };
            draw_sketch(&mut ov, &sk, c);
            if cmd_pick && Some(uid) != no_profiles {
                for (i, r) in sk.regions.iter().enumerate() {
                    let selected = cmd_sel.iter().any(|(u, p)| *u == uid && p.resolve(&sk.regions) == Some(i));
                    let hov = matches!(self.hover, Some(Hit::Profile { sketch, region, .. }) if sketch == uid && region == i);
                    let col = if selected {
                        c.profile_selected
                    } else if hov {
                        c.profile_hover
                    } else {
                        c.profile
                    };
                    fill_region(&mut ov.tris, &sk.plane, r, col);
                }
            }
        }
        if let Mode::Sketch(s) = &self.mode {
            s.overlay(&mut ov, &self.cam, rect, c);
        }
        ov
    }

    fn paint_labels(&self, ui: &mut egui::Ui, rect: Rect, model: &Model) {
        let painter = ui.painter_at(rect);
        // подсказка режима внизу 3D-вида
        let hint = match &self.mode {
            Mode::Idle if model.bodies.is_empty() && self.doc.features.is_empty() => {
                Some(tr("Начните с «✏ Чертёж»: выберите плоскость и нарисуйте контур. Или Файл → Открыть пример.").to_string())
            }
            Mode::Idle => None,
            Mode::PickPlane => Some(tr("Выберите плоскость (XY, XZ, YZ) или плоскую грань тела. Esc — отмена").into()),
            Mode::Sketch(s) => Some(s.hint()),
            Mode::Command(c) => Some(c.hint().into()),
            Mode::Measure(p) => Some(if p.len() == 1 { tr("Щёлкните вторую точку (вершина прилипает). Esc — выйти") } else { tr("Щёлкните точку на теле или его вершину. Esc — выйти") }.into()),
        };
        if let Some(h) = hint {
            let g = painter.layout(h, egui::FontId::proportional(13.0), Color32::WHITE, rect.width() - 40.0);
            let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.bottom() - 72.0), g.size() + egui::vec2(18.0, 10.0));
            painter.rect_filled(r, 6.0, Color32::from_rgba_unmultiplied(25, 35, 50, 200));
            painter.galley(r.min + egui::vec2(9.0, 5.0), g, Color32::WHITE);
        }
        if let Some((msg, t, err)) = &self.toast {
            let age = ui.input(|i| i.time) - t;
            if age < 4.0 {
                let a = (1.0 - ((age - 3.0).max(0.0))) as f32;
                let col = if *err { Color32::from_rgb(190, 40, 40) } else { Color32::from_rgb(30, 130, 60) };
                let g = painter.layout_no_wrap(msg.clone(), egui::FontId::proportional(13.5), Color32::WHITE);
                let r = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 30.0), g.size() + egui::vec2(20.0, 12.0));
                painter.rect_filled(r, 6.0, col.gamma_multiply(a));
                painter.galley(r.min + egui::vec2(10.0, 6.0), g, Color32::WHITE.gamma_multiply(a));
                ui.ctx().request_repaint();
            }
        }
        // числа вдоль осей сетки, как линейка
        if self.settings.show_grid && self.settings.grid_numbers {
            let col = if self.theme == Theme::Dark { Color32::from_rgb(170, 180, 195) } else { Color32::from_rgb(90, 98, 110) };
            match &self.mode {
                Mode::Sketch(s) => render::axis_labels(&painter, &self.cam, rect, &s.plane, render::decade_step(self.cam.world_per_pixel(rect), 55.0), col.gamma_multiply(0.55)),
                _ => render::axis_labels(&painter, &self.cam, rect, &Plane::XY, render::decade_step(self.cam.world_per_pixel(rect), 55.0), col),
            }
        }
        // расстояние между измеряемыми точками
        if let Mode::Measure(pts) = &self.mode {
            if pts.len() == 2 {
                if let Some(p) = self.cam.project(rect, (pts[0] + pts[1]) / 2.0) {
                    let text = format!("{} {}", format_num((pts[0].distance(pts[1]) * 1000.0).round() / 1000.0), tr("мм"));
                    let g = painter.layout_no_wrap(text, egui::FontId::proportional(14.0), Color32::WHITE);
                    let r = Rect::from_min_size(p + egui::vec2(10.0, -22.0), g.size() + egui::vec2(12.0, 6.0));
                    painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(200, 100, 0, 235));
                    painter.galley(r.min + egui::vec2(6.0, 3.0), g, Color32::WHITE);
                }
            }
        }
        // значение у стрелки выдавливания
        if let Some((base, dir, dist)) = self.extrude_handle() {
            if let Some(p) = self.cam.project(rect, base + dir * dist) {
                let text = format!("{} {}", format_num((dist.abs() * 100.0).round() / 100.0), tr("мм"));
                let g = painter.layout_no_wrap(text, egui::FontId::proportional(13.0), Color32::WHITE);
                let r = Rect::from_min_size(p + egui::vec2(14.0, -26.0), g.size() + egui::vec2(12.0, 6.0));
                painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(20, 90, 160, 235));
                painter.galley(r.min + egui::vec2(6.0, 3.0), g, Color32::WHITE);
            }
        }
        // подписи плоскостей
        if self.show_origin_planes() {
            let s = self.origin_plane_size();
            for (i, pl) in ORIGIN_PLANES.iter().enumerate() {
                if let Some(p) = self.cam.project(rect, pl.to_world([s, s])) {
                    painter.text(p, egui::Align2::LEFT_BOTTOM, plane_name(i), egui::FontId::proportional(11.0), Color32::from_rgb(160, 100, 20));
                }
            }
        }
    }
}

fn marker(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(6.0, 22.0), egui::Sense::hover());
    ui.painter().rect_filled(Rect::from_center_size(r.center(), egui::vec2(3.0, 22.0)), 1.0, ACCENT);
}

fn draw_sketch(ov: &mut Overlay, sk: &SketchResult, c: &crate::theme::ViewColors) {
    for (i, e) in sk.sketch.entities.iter().enumerate() {
        let pts: Vec<DVec3> = sk.sketch.tessellate(i).iter().map(|p| sk.plane.to_world(*p)).collect();
        let col = if e.construction() { c.sketch_construction } else { c.sketch };
        polyline(&mut ov.lines, &pts, col, if e.construction() { 1.0 } else { 1.6 }, false);
    }
}

fn ray_box(o: DVec3, d: DVec3, lo: DVec3, hi: DVec3) -> bool {
    let inv = d.recip();
    let t1 = (lo - o) * inv;
    let t2 = (hi - o) * inv;
    let tmin = t1.min(t2).max_element();
    let tmax = t1.max(t2).min_element();
    tmax >= tmin.max(0.0) - 1e-9
}

struct ViewCallback {
    frame: FrameUniform,
    colors: Vec<[f32; 4]>,
    overlay: Overlay,
    upload: Option<(Vec<FaceVertex>, Vec<LineInst>)>,
    style: (bool, bool),
}

impl egui_wgpu::CallbackTrait for ViewCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _enc: &mut wgpu::CommandEncoder,
        res: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let scene: &mut GpuScene = res.get_mut().unwrap();
        if let Some((f, e)) = &self.upload {
            scene.set_bodies(device, queue, f, e);
        }
        scene.set_style(self.style.0, self.style.1);
        scene.set_overlay(device, queue, &self.overlay);
        scene.set_frame(device, queue, self.frame, &self.colors);
        Vec::new()
    }

    fn paint(&self, _info: egui::PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>, res: &egui_wgpu::CallbackResources) {
        let scene: &GpuScene = res.get().unwrap();
        scene.draw(pass);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = ctx.input(|i| i.time);
        self.now = now;
        if std::env::var("GENIX_DEBUG_EVENTS").is_ok() {
            ctx.input(|i| {
                for e in &i.events {
                    if !matches!(e, egui::Event::PointerMoved(_)) {
                        eprintln!("event: {e:?}");
                    }
                }
            });
        }
        if let Some((_, t, _)) = &mut self.toast {
            if *t == 0.0 {
                *t = now;
            }
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title()));

        // закрытие окна с несохранёнными изменениями
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close && self.dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::Quit);
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // тема «как в системе», масштаб интерфейса, автосохранение
        let want = resolve_theme(&self.settings, ctx);
        if want != self.theme {
            self.theme = want;
            want.apply(ctx);
        }
        if (ctx.zoom_factor() - self.settings.ui_scale).abs() > 1e-3 {
            ctx.set_zoom_factor(self.settings.ui_scale);
        }
        if self.settings.autosave_minutes > 0 && self.path.is_some() && self.dirty() && matches!(self.mode, Mode::Idle) {
            if self.last_autosave == 0.0 {
                self.last_autosave = now;
            }
            if now - self.last_autosave > self.settings.autosave_minutes as f64 * 60.0 {
                self.last_autosave = now;
                self.save(false, now);
            }
        }
        if self.playing {
            if !matches!(self.mode, Mode::Idle) {
                self.playing = false;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                if now - self.last_play > 0.8 {
                    self.last_play = now;
                    if self.doc.rollback < self.doc.features.len() {
                        self.doc.rollback += 1;
                        self.after_doc_change();
                    } else {
                        self.playing = false;
                    }
                }
            }
        }
        // сначала лицензия, затем аккаунт (демо)
        if !crate::home::unlocked(&self.settings) {
            if crate::home::gate_ui(ctx, &mut self.settings, &mut self.gate) {
                self.settings.save();
            }
            return;
        }
        if !self.home_active {
            self.shortcuts(ctx, now);
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu_bar(ui, now));
        egui::TopBottomPanel::top("tabs").frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(8, 3))).show(ctx, |ui| self.tab_bar(ui));
        if self.home_active {
            let mut act = crate::home::HomeAction::None;
            egui::CentralPanel::default().show(ctx, |ui| act = crate::home::home_ui(ui, &mut self.home, &self.settings));
            match act {
                crate::home::HomeAction::None => {}
                crate::home::HomeAction::New => self.new_blank_tab(),
                crate::home::HomeAction::Open => self.open_dialog(),
                crate::home::HomeAction::OpenPath(p) => self.open_path(p),
                crate::home::HomeAction::Example(i) => {
                    let f: fn() -> Document = if i == 0 { crate::examples::bracket } else { crate::examples::bushing };
                    self.open_in_tab(f(), None);
                }
                crate::home::HomeAction::Forget(p) => {
                    self.settings.recent.retain(|x| *x != p);
                    self.settings.save();
                    self.home.refresh(&self.settings.recent);
                }
            }
            self.show_dialogs(ctx);
            return;
        }
        egui::TopBottomPanel::top("toolbar").frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(10, 10))).show(ctx, |ui| self.toolbar(ui, now));
        egui::TopBottomPanel::bottom("timeline").frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(8, 6))).show(ctx, |ui| self.timeline(ui, now));
        if self.settings.show_left_panel {
            egui::SidePanel::left("browser").default_width(210.0).width_range(150.0..=420.0).resizable(true).show(ctx, |ui| self.browser(ui, now));
        }

        // панель справа: диалог команды, свойства чертежа или выбранных тел
        let right_title: Option<String> = match &self.mode {
            Mode::Command(c) => self.doc.feature(c.uid).map(|f| tr(f.kind.type_label()).to_uppercase()),
            Mode::Sketch(_) => Some(tr("ЧЕРТЁЖ").into()),
            Mode::PickPlane => Some(tr("НОВЫЙ ЧЕРТЁЖ").into()),
            Mode::Measure(_) => Some(tr("ИЗМЕРЕНИЕ").into()),
            Mode::Idle if !self.selection.is_empty() => Some(tr("СВОЙСТВА").into()),
            Mode::Idle => None,
        };
        if let (Some(title), true) = (right_title, self.settings.show_right_panel) {
            egui::SidePanel::right("props").default_width(250.0).width_range(180.0..=480.0).resizable(true).show(ctx, |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(title).strong().color(ACCENT));
                ui.separator();
                self.right_panel(ui, now);
            });
        }

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| self.viewport(ui, now));

        self.show_dialogs(ctx);
    }
}

impl App {
    fn show_dialogs(&mut self, ctx: &egui::Context) {
        let now = self.now;
        if self.params_open {
            self.params_window(ctx);
        }
        if self.help_open {
            self.help_window(ctx);
        }
        if self.settings_open {
            self.settings_window(ctx);
        }
        self.confirm_window(ctx, now);
    }
}

impl App {
    fn right_panel(&mut self, ui: &mut egui::Ui, now: f64) {
        let names: std::collections::HashMap<u64, String> = self.ev.model().bodies.iter().map(|b| (b.id, tr_name(self.doc.body_name(b)))).collect();
        let body_name = |id: u64| names.get(&id).cloned().unwrap_or_else(|| tr("(нет тела)").into());
        match &mut self.mode {
            Mode::Command(cmd) => {
                let sketch_list: Vec<(u64, String)> = self.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| (f.uid, tr_name(&f.name))).collect();
                let r = commands::dialog_ui(ui, &mut self.doc, &self.ev, cmd, &body_name, &sketch_list);
                match r {
                    CmdResult::Changed => self.after_doc_change(),
                    CmdResult::Ok => self.finish_command(true),
                    CmdResult::Cancel => self.finish_command(false),
                    CmdResult::None => {}
                }
                let _ = now;
            }
            Mode::Sketch(s) => s.status_ui(ui, &self.doc.params),
            Mode::Measure(pts) => {
                if pts.len() == 2 {
                    let d = pts[1] - pts[0];
                    let r = |v: f64| format_num((v * 1000.0).round() / 1000.0);
                    egui::Grid::new("measure").num_columns(2).show(ui, |ui| {
                        ui.label(tr("Расстояние"));
                        ui.label(RichText::new(tf!("{} мм", r(d.length()))).strong());
                        ui.end_row();
                        for (n, v) in [("ΔX", d.x), ("ΔY", d.y), ("ΔZ", d.z)] {
                            ui.label(n);
                            ui.label(tf!("{} мм", r(v)));
                            ui.end_row();
                        }
                    });
                } else {
                    ui.label(RichText::new(tr("Щёлкните две точки на теле")).weak());
                }
                ui.add_space(6.0);
                let (mut reset, mut close) = (false, false);
                ui.horizontal(|ui| {
                    reset = ui.button(tr("Сбросить")).clicked();
                    close = ui.button(tr("Закрыть")).clicked();
                });
                if reset {
                    pts.clear();
                }
                if close {
                    self.mode = Mode::Idle;
                }
            }
            Mode::PickPlane => {
                ui.label(tr("Щёлкните плоскость начала координат или плоскую грань тела."));
                ui.add_space(6.0);
                for (i, pl) in ORIGIN_PLANES.iter().enumerate() {
                    if ui.button(tf!("Плоскость {}", tr(plane_name(i)))).clicked() {
                        self.begin_sketch_on(*pl, now);
                        return;
                    }
                }
                ui.add_space(6.0);
                if ui.button(tr("Отмена")).clicked() {
                    self.mode = Mode::Idle;
                }
            }
            Mode::Idle => self.inspector(ui),
        }
    }

    fn confirm_window(&mut self, ctx: &egui::Context, now: f64) {
        let Some(c) = self.confirm.take() else { return };
        let mut keep = true;
        let mut run = false;
        egui::Window::new(tr("Несохранённые изменения")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(tr("Сохранить изменения в документе?"));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(RichText::new(tr("Сохранить")).strong()).clicked() {
                    keep = false;
                    run = self.save(false, now);
                }
                if ui.button(tr("Не сохранять")).clicked() {
                    keep = false;
                    run = true;
                }
                if ui.button(tr("Отмена")).clicked() {
                    keep = false;
                }
            });
        });
        if run {
            self.saved = self.doc.clone();
            self.run_confirmed(c);
        } else if keep {
            self.confirm = Some(c);
        }
    }

    /// Виды цифровой клавиатуры, как в Blender: 1 — спереди, 3 — справа, 7 — сверху (с Ctrl — с обратной стороны),
    /// 5 — перспектива/ортогональная проекция, 4/6/8/2 — повернуть на 15°.
    fn blender_views(&mut self, ctx: &egui::Context, now: f64) {
        use egui::Key::*;
        let (key, ctrl) = ctx.input(|i| (
            [Num1, Num3, Num7, Num5, Num4, Num6, Num8, Num2].into_iter().find(|k| i.key_pressed(*k)),
            i.modifiers.ctrl || i.modifiers.command,
        ));
        let Some(key) = key else { return };
        let (y, x, z) = (DVec3::Y, DVec3::X, DVec3::Z);
        let step = 15f64.to_radians();
        let (cy, cp) = (self.cam.yaw, self.cam.pitch);
        match key {
            Num1 => { let (yaw, pitch) = if ctrl { Camera::angles_for(y, -x) } else { Camera::angles_for(-y, x) }; self.cam.animate_to(yaw, pitch, None, None, now); }
            Num3 => { let (yaw, pitch) = if ctrl { Camera::angles_for(-x, -y) } else { Camera::angles_for(x, y) }; self.cam.animate_to(yaw, pitch, None, None, now); }
            Num7 => { let (yaw, pitch) = if ctrl { Camera::angles_for(-z, x) } else { Camera::angles_for(z, x) }; self.cam.animate_to(yaw, pitch, None, None, now); }
            Num4 => self.cam.animate_to(cy + step, cp, None, None, now),
            Num6 => self.cam.animate_to(cy - step, cp, None, None, now),
            Num8 => self.cam.animate_to(cy, cp - step, None, None, now),
            Num2 => self.cam.animate_to(cy, cp + step, None, None, now),
            _ => {
                // 5: тот же масштаб на экране при смене угла обзора
                let (from, to) = (self.cam.fov_deg.to_radians() / 2.0, if self.ortho { self.settings.fov_deg as f64 } else { 5.0 }.to_radians() / 2.0);
                self.cam.distance *= from.tan() / to.tan();
                self.ortho = !self.ortho;
            }
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context, now: f64) {
        let cmd = |k: egui::Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, k));
        let cmd_shift = |k: egui::Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, k));
        if cmd_shift(egui::Key::S) {
            self.save(true, now);
        } else if cmd(egui::Key::S) {
            self.save(false, now);
        }
        if cmd(egui::Key::O) {
            self.request(Confirm::Open);
        }
        if cmd(egui::Key::N) || cmd(egui::Key::T) {
            self.request(Confirm::New);
        }
        if cmd(egui::Key::W) {
            self.request(Confirm::CloseTab);
        }
        let in_sketch = matches!(self.mode, Mode::Sketch(_));
        if !in_sketch {
            if cmd_shift(egui::Key::Z) || cmd(egui::Key::Y) {
                if matches!(self.mode, Mode::Idle) {
                    self.do_redo();
                }
            } else if cmd(egui::Key::Z) && matches!(self.mode, Mode::Idle) {
                self.do_undo();
            }
        }
        if crate::widgets::text_input_active(ctx) || in_sketch {
            return;
        }
        crate::widgets::release_button_focus(ctx);
        let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k) && !i.modifiers.command && !i.modifiers.alt);
        if pressed(egui::Key::Escape) {
            match self.mode {
                Mode::Command(_) => self.finish_command(false),
                Mode::PickPlane | Mode::Measure(_) => self.mode = Mode::Idle,
                _ => self.selection.clear(),
            }
        }
        if pressed(egui::Key::Enter) && matches!(self.mode, Mode::Command(_)) {
            self.finish_command(true);
        }
        let blender = self.settings.controls == ControlScheme::Blender;
        // панели: N — свойства справа, T — дерево слева (как в Blender)
        if blender && pressed(egui::Key::N) {
            self.settings.show_right_panel = !self.settings.show_right_panel;
        }
        if blender && pressed(egui::Key::T) {
            self.settings.show_left_panel = !self.settings.show_left_panel;
        }
        if blender {
            self.blender_views(ctx, now);
        }
        if !matches!(self.mode, Mode::Idle) {
            return;
        }
        if blender {
            if pressed(egui::Key::G) || pressed(egui::Key::R) {
                self.start_command("move", now);
            }
            if pressed(egui::Key::S) {
                self.start_command("scale", now);
            }
            if pressed(egui::Key::Q) {
                self.start_sketch();
            }
            if pressed(egui::Key::X) && !self.selection.is_empty() {
                let ids = self.selection.clone();
                self.delete_bodies(ids);
            }
            if pressed(egui::Key::Home) || pressed(egui::Key::Period) {
                self.fit_view(now, false);
            }
        } else if pressed(egui::Key::S) {
            self.start_sketch();
        }
        if pressed(egui::Key::E) {
            self.start_command("extrude", now);
        }
        if !blender && pressed(egui::Key::M) {
            self.start_command("move", now);
        }
        if pressed(egui::Key::H) {
            self.start_command("hole", now);
        }
        if pressed(egui::Key::F) {
            self.fit_view(now, false);
        }
        if (pressed(egui::Key::Delete) || pressed(egui::Key::Backspace)) && !self.selection.is_empty() {
            let ids = self.selection.clone();
            self.delete_bodies(ids);
        }
    }
}

fn resolve_theme(st: &Settings, ctx: &egui::Context) -> Theme {
    match st.theme {
        ThemeChoice::Light => Theme::Light,
        ThemeChoice::Dark => Theme::Dark,
        ThemeChoice::Auto => match ctx.input(|i| i.raw.system_theme) {
            Some(egui::Theme::Dark) => Theme::Dark,
            _ => Theme::Light,
        },
    }
}

/// Чертежи, на которые опирается операция (они скрываются после её завершения).
fn used_sketches(kind: &FeatureKind) -> Vec<u64> {
    match kind {
        FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } => vec![*sketch],
        FeatureKind::Sweep { sketch, path, .. } => vec![*sketch, *path],
        FeatureKind::Pipe { path, .. } => vec![*path],
        FeatureKind::Loft { profiles, .. } => profiles.iter().map(|(s, _)| *s).collect(),
        _ => vec![],
    }
}

/// Идентификатор кнопки панели, соответствующей операции.
fn tool_id(kind: &FeatureKind) -> &'static str {
    use fusion_core::doc::Shape;
    match kind {
        FeatureKind::Sketch { .. } => "sketch",
        FeatureKind::Extrude { .. } => "extrude",
        FeatureKind::Revolve { .. } => "revolve",
        FeatureKind::Sweep { .. } => "sweep",
        FeatureKind::Loft { .. } => "loft",
        FeatureKind::Hole { .. } => "hole",
        FeatureKind::Primitive { shape: Shape::Box { .. }, .. } => "box",
        FeatureKind::Primitive { shape: Shape::Cylinder { .. }, .. } => "cylinder",
        FeatureKind::Primitive { shape: Shape::Sphere { .. }, .. } => "sphere",
        FeatureKind::Primitive { shape: Shape::Torus { .. }, .. } => "torus",
        FeatureKind::Coil { .. } => "coil",
        FeatureKind::Pipe { .. } => "pipe",
        FeatureKind::Mirror { .. } => "mirror",
        FeatureKind::Pattern { .. } => "pattern",
        FeatureKind::Move { .. } => "move",
        FeatureKind::Scale { .. } => "scale",
        FeatureKind::Fillet { .. } => "fillet",
        FeatureKind::Chamfer { .. } => "chamfer",
        FeatureKind::Combine { .. } => "combine",
        FeatureKind::Remove { .. } => "remove",
        FeatureKind::OffsetPlane { .. } => "plane",
        FeatureKind::Shell { .. } => "shell",
    }
}

/// Рёбра тела для выбора: прямые (коллинеарные куски склеены) и круглые (замкнутые окружности).
pub(crate) fn compute_straight_edges(mesh: &Mesh) -> Vec<EdgeRef> {
    use std::collections::HashMap;
    let segs = mesh.sharp_edges(30.0);
    let pos = |i: u32| DVec3::from(mesh.vertices[i as usize]);
    let mut at: HashMap<u32, Vec<usize>> = HashMap::new();
    for (k, (a, b)) in segs.iter().enumerate() {
        at.entry(*a).or_default().push(k);
        at.entry(*b).or_default().push(k);
    }
    let mut parent: Vec<usize> = (0..segs.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    let mut curved = vec![false; segs.len()];
    let mut cparent: Vec<usize> = (0..segs.len()).collect();
    for (v, list) in &at {
        if list.len() != 2 {
            continue;
        }
        let other = |k: usize| if segs[k].0 == *v { segs[k].1 } else { segs[k].0 };
        let (d1, d2) = ((pos(other(list[0])) - pos(*v)).normalize_or_zero(), (pos(other(list[1])) - pos(*v)).normalize_or_zero());
        if d1.dot(d2) < -1.0 + 1e-7 {
            let (a, b) = (find(&mut parent, list[0]), find(&mut parent, list[1]));
            parent[a] = b;
        } else if (-d1).dot(d2) > 0.7 {
            curved[list[0]] = true;
            curved[list[1]] = true;
            let (a, b) = (find(&mut cparent, list[0]), find(&mut cparent, list[1]));
            cparent[a] = b;
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for k in 0..segs.len() {
        let r = find(&mut parent, k);
        groups.entry(r).or_default().push(k);
    }
    let mut out = Vec::new();
    for g in groups.values() {
        if g.iter().any(|k| curved[*k]) {
            continue;
        }
        let dir = (pos(segs[g[0]].1) - pos(segs[g[0]].0)).normalize_or_zero();
        let mut lo = (f64::MAX, DVec3::ZERO);
        let mut hi = (f64::MIN, DVec3::ZERO);
        for k in g {
            for v in [segs[*k].0, segs[*k].1] {
                let t = pos(v).dot(dir);
                if t < lo.0 {
                    lo = (t, pos(v));
                }
                if t > hi.0 {
                    hi = (t, pos(v));
                }
            }
        }
        out.push(EdgeRef::line(lo.1.to_array(), hi.1.to_array()));
    }
    // замкнутые плавные цепочки, лежащие на окружности
    let mut circles: HashMap<usize, Vec<usize>> = HashMap::new();
    for k in 0..segs.len() {
        if curved[k] {
            let r = find(&mut cparent, k);
            circles.entry(r).or_default().push(k);
        }
    }
    for g in circles.values() {
        let mut verts: Vec<u32> = g.iter().flat_map(|k| [segs[*k].0, segs[*k].1]).collect();
        verts.sort_unstable();
        verts.dedup();
        // цепочка замкнута, если у каждой вершины ровно два отрезка группы
        let closed = verts.iter().all(|v| g.iter().filter(|k| segs[**k].0 == *v || segs[**k].1 == *v).count() == 2);
        if !closed || verts.len() < 8 {
            continue;
        }
        let pts: Vec<DVec3> = verts.iter().map(|v| pos(*v)).collect();
        let c = pts.iter().fold(DVec3::ZERO, |a, p| a + *p) / pts.len() as f64;
        // порядок вершин в `verts` произвольный, поэтому нормаль берём по трём далёким точкам
        let p0 = pts[0];
        let p1 = pts.iter().copied().max_by(|a, b| a.distance(p0).total_cmp(&b.distance(p0))).unwrap();
        let p2 = pts.iter().copied().max_by(|a, b| (a.distance(p0) + a.distance(p1)).total_cmp(&(b.distance(p0) + b.distance(p1)))).unwrap();
        let axis = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        if axis == DVec3::ZERO {
            continue;
        }
        let r = pts.iter().map(|p| ((*p - c) - axis * (*p - c).dot(axis)).length()).collect::<Vec<_>>();
        let rm = r.iter().sum::<f64>() / r.len() as f64;
        let flat = pts.iter().all(|p| (*p - c).dot(axis).abs() < 1e-4 * (1.0 + rm));
        if flat && r.iter().all(|x| (x - rm).abs() < 1e-3 * (1.0 + rm)) {
            out.push(EdgeRef::round(pts[0].to_array(), c.to_array(), axis.to_array()));
        }
    }
    out.sort_by(|x, y| x.a.partial_cmp(&y.a).unwrap().then(x.b.partial_cmp(&y.b).unwrap()));
    out
}

impl App {
    /// Прямые рёбра тела (кэш по телу).
    fn straight_edges(&self, b: &fusion_core::doc::Body) -> Arc<Vec<EdgeRef>> {
        let key = Arc::as_ptr(&b.solid) as usize;
        let mut cache = self.edge_cache.borrow_mut();
        if cache.len() > 256 {
            cache.clear();
        }
        cache.entry(key).or_insert_with(|| Arc::new(compute_straight_edges(b.mesh()))).clone()
    }

    /// Ребро под курсором: ближайшее к точке экрана и не закрытое телом.
    fn pick_edge(&self, model: &Model, rect: Rect, mouse: Pos2) -> Option<EdgeRef> {
        let eye = self.cam.eye();
        let target_body = match &self.mode {
            Mode::Command(c) => match self.doc.feature(c.uid).map(|f| &f.kind) {
                Some(FeatureKind::Chamfer { body, .. } | FeatureKind::Fillet { body, .. }) => Some(*body),
                _ => None,
            },
            _ => None,
        };
        let mut cands: Vec<(f32, EdgeRef, DVec3, &fusion_core::doc::Body)> = Vec::new();
        for b in &model.bodies {
            if self.doc.hidden_bodies.contains(&b.id) || target_body.is_some_and(|t| t != b.id) {
                continue;
            }
            for e in self.straight_edges(b).iter() {
                let pts = e.polyline();
                let mut best: Option<(f32, DVec3)> = None;
                for w in pts.windows(2) {
                    let (Some(pa), Some(pb)) = (self.cam.project(rect, w[0]), self.cam.project(rect, w[1])) else { continue };
                    let ab = pb - pa;
                    let t = ((mouse - pa).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
                    let dist = mouse.distance(pa + ab * t);
                    if best.is_none_or(|x| dist < x.0) {
                        best = Some((dist, w[0].lerp(w[1], t as f64)));
                    }
                }
                if let Some((dist, p)) = best {
                    if dist < 8.0 {
                        cands.push((dist, *e, p, b));
                    }
                }
            }
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, e, p, b) in cands.into_iter().take(4) {
            let to = p - eye;
            let dist = to.length();
            let blocked = b.mesh().raycast(eye, to / dist).is_some_and(|(h, _)| h < dist - 2e-3 * dist.max(1.0));
            if !blocked {
                return Some(e);
            }
        }
        None
    }

    /// Чертёж пути (его области не выбираются как профили).
    fn path_sketch(&self) -> Option<u64> {
        match &self.mode {
            Mode::Command(c) => match self.doc.feature(c.uid).map(|f| &f.kind) {
                Some(FeatureKind::Sweep { path, .. } | FeatureKind::Pipe { path, .. }) => Some(*path),
                _ => None,
            },
            _ => None,
        }
    }

    /// Стрелка выдавливания: начало, направление и текущая длина (в мире).
    fn extrude_handle(&self) -> Option<(DVec3, DVec3, f64)> {
        let Mode::Command(c) = &self.mode else { return None };
        let FeatureKind::Extrude { sketch, profiles, extent, .. } = &self.doc.feature(c.uid)?.kind else { return None };
        let sk = self.ev.sketch(*sketch)?;
        let i = profiles.first()?.resolve(&sk.regions)?;
        let base = sk.plane.to_world(sk.regions[i].interior_point());
        let params = &self.doc.params;
        let dist = match extent {
            fusion_core::doc::Extent::OneSide(d) | fusion_core::doc::Extent::TwoSides(d, _) => d.eval(params).ok()?,
            fusion_core::doc::Extent::Symmetric(d) => d.eval(params).ok()?.abs() / 2.0,
        };
        Some((base, sk.plane.normal(), dist))
    }

    fn handle_hot(&self, rect: Rect, ctx: &egui::Context) -> bool {
        let (Some((b, d, l)), Some(m)) = (self.extrude_handle(), ctx.pointer_hover_pos()) else { return false };
        self.cam.project(rect, b + d * l).is_some_and(|p| p.distance(m) < 16.0)
    }

    /// Тянем кончик стрелки: расстояние выдавливания следует за курсором (шаг 1 мм, с Alt — свободно).
    /// Центр перемещаемых тел, текущий сдвиг, поворот и длина стрелок (для манипулятора перемещения).
    fn move_gizmo(&self, rect: Rect) -> Option<(DVec3, [f64; 3], [f64; 3], f64)> {
        let Mode::Command(c) = &self.mode else { return None };
        let f = self.doc.feature(c.uid)?;
        let FeatureKind::Move { bodies, translate, rotate, .. } = &f.kind else { return None };
        let idx = self.doc.index_of(c.uid)?;
        let model = self.ev.model_before(idx);
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for b in model.bodies.iter().filter(|b| bodies.contains(&b.id)) {
            if let Some((l, h)) = b.mesh().bounds() {
                lo = lo.min(l);
                hi = hi.max(h);
            }
        }
        if lo.x > hi.x {
            return None;
        }
        let ev = |e: &Expr| self.doc.params.eval(&e.0).unwrap_or(0.0);
        let len = self.cam.world_per_pixel(rect) * 90.0;
        Some(((lo + hi) * 0.5, [ev(&translate[0]), ev(&translate[1]), ev(&translate[2])], [ev(&rotate[0]), ev(&rotate[1]), ev(&rotate[2])], len))
    }

    fn drive_gizmo(&mut self, resp: &egui::Response, rect: Rect, ctx: &egui::Context) {
        let dirs = [DVec3::X, DVec3::Y, DVec3::Z];
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let (Some((c0, t, r, len)), Some(press)) = (self.move_gizmo(rect), ctx.input(|i| i.pointer.press_origin())) {
                let c = c0 + DVec3::from(t);
                let (o, d) = self.cam.ray(rect, press);
                let mut best: Option<(f64, Gizmo)> = None;
                for i in 0..3 {
                    // кончик стрелки
                    if let Some(p) = self.cam.project(rect, c + dirs[i] * len) {
                        let dist = p.distance(press) as f64;
                        if dist < 18.0 && best.as_ref().is_none_or(|b| dist < b.0) {
                            let grab = line_param(c0, dirs[i], o, d) - t[i];
                            best = Some((dist, Gizmo { ring: false, axis: i, c0, grab, start: t[i] }));
                        }
                    }
                    // дуга поворота: ближайшая точка кольца к курсору
                    let (x, y) = fusion_core::geom::perp_frame(dirs[i]);
                    for k in 0..48 {
                        let a = std::f64::consts::TAU * k as f64 / 48.0;
                        if let Some(p) = self.cam.project(rect, c + (x * a.cos() + y * a.sin()) * len * 0.62) {
                            let dist = p.distance(press) as f64 + 4.0; // стрелки важнее дуг
                            if dist < 12.0 && best.as_ref().is_none_or(|b| dist < b.0) {
                                best = Some((dist, Gizmo { ring: true, axis: i, c0, grab: a, start: r[i] }));
                            }
                        }
                    }
                }
                if let Some((_, mut g)) = best {
                    if g.ring {
                        g.grab = self.ring_angle(rect, c, g.axis, press);
                    }
                    self.gizmo = Some(g);
                }
            }
        }
        let Some(g) = &self.gizmo else { return };
        if !resp.dragged_by(egui::PointerButton::Primary) {
            self.gizmo = None;
            return;
        }
        let Some(pos) = ctx.pointer_interact_pos() else { return };
        let alt = ctx.input(|i| i.modifiers.alt);
        let (axis, ring, c0, grab, start) = (g.axis, g.ring, g.c0, g.grab, g.start);
        let value = if ring {
            let Some((cc0, t, _, _)) = self.move_gizmo(rect) else { return };
            let c = cc0 + DVec3::from(t);
            let delta = (self.ring_angle(rect, c, axis, pos) - grab).to_degrees();
            let v = start + delta;
            if alt { (v * 10.0).round() / 10.0 } else { (v / 5.0).round() * 5.0 }
        } else {
            let (o, d) = self.cam.ray(rect, pos);
            let v = line_param(c0, dirs[axis], o, d) - grab;
            if alt { (v * 100.0).round() / 100.0 } else { v.round() }
        };
        let uid = match &self.mode {
            Mode::Command(c) => c.uid,
            _ => return,
        };
        if let Some(f) = self.doc.feature_mut(uid) {
            if let FeatureKind::Move { translate, rotate, .. } = &mut f.kind {
                let target = if ring { &mut rotate[axis] } else { &mut translate[axis] };
                let new = Expr::num(value);
                if *target != new {
                    *target = new;
                    self.after_doc_change();
                }
            }
        }
    }

    /// Угол курсора вокруг оси `axis` через центр `c` (луч пересекается с плоскостью кольца).
    fn ring_angle(&self, rect: Rect, c: DVec3, axis: usize, pos: Pos2) -> f64 {
        let n = [DVec3::X, DVec3::Y, DVec3::Z][axis];
        let (o, d) = self.cam.ray(rect, pos);
        let den = d.dot(n);
        if den.abs() < 1e-9 {
            return 0.0;
        }
        let t = (c - o).dot(n) / den;
        let p = o + d * t - c;
        let (x, y) = fusion_core::geom::perp_frame(n);
        p.dot(y).atan2(p.dot(x))
    }

    fn drive_manip(&mut self, resp: &egui::Response, rect: Rect, ctx: &egui::Context) {
        self.drive_gizmo(resp, rect, ctx);
        if self.gizmo.is_some() {
            return;
        }
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let (Some((base, dir, dist)), Some(press)) = (self.extrude_handle(), ctx.input(|i| i.pointer.press_origin())) {
                if self.cam.project(rect, base + dir * dist).is_some_and(|p| p.distance(press) < 18.0) {
                    let (o, d) = self.cam.ray(rect, press);
                    let t = line_param(base, dir, o, d);
                    self.manip = Some(Manip { base, dir, grab: t - dist });
                }
            }
        }
        let Some(m) = &self.manip else { return };
        if !resp.dragged_by(egui::PointerButton::Primary) {
            self.manip = None;
            return;
        }
        let Some(pos) = ctx.pointer_interact_pos() else { return };
        let (o, d) = self.cam.ray(rect, pos);
        let mut v = line_param(m.base, m.dir, o, d) - m.grab;
        if !ctx.input(|i| i.modifiers.alt) {
            v = v.round();
        } else {
            v = (v * 100.0).round() / 100.0;
        }
        let uid = match &self.mode {
            Mode::Command(c) => c.uid,
            _ => return,
        };
        let has_bodies = self.doc.index_of(uid).is_some_and(|i| !self.ev.model_before(i).bodies.is_empty());
        let auto = matches!(&self.mode, Mode::Command(c) if c.auto_op);
        if let Some(f) = self.doc.feature_mut(uid) {
            if let FeatureKind::Extrude { extent, op, .. } = &mut f.kind {
                let new = match extent {
                    fusion_core::doc::Extent::OneSide(_) => fusion_core::doc::Extent::OneSide(Expr::num(v)),
                    fusion_core::doc::Extent::Symmetric(_) => fusion_core::doc::Extent::Symmetric(Expr::num((v * 2.0).abs())),
                    fusion_core::doc::Extent::TwoSides(_, b) => fusion_core::doc::Extent::TwoSides(Expr::num(v), b.clone()),
                };
                if *extent != new {
                    *extent = new;
                    if auto && has_bodies {
                        if let fusion_core::doc::Extent::OneSide(_) = extent {
                            *op = if v < 0.0 { fusion_core::Op::Cut } else { fusion_core::Op::Join };
                        }
                    }
                    self.after_doc_change();
                }
            }
        }
    }

    /// Поле значения над моделью: главный размер текущей команды можно вписать прямо в окне (как в Fusion).
    fn value_box(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Mode::Command(cmd) = &self.mode else { return };
        let uid = cmd.uid;
        let params = self.doc.params.clone();
        let Some(f) = self.doc.feature_mut(uid) else { return };
        let (label, unit, expr): (&str, &'static str, &mut Expr) = match &mut f.kind {
            FeatureKind::Extrude { extent: fusion_core::doc::Extent::OneSide(e) | fusion_core::doc::Extent::Symmetric(e), .. } => ("Расстояние", "мм", e),
            FeatureKind::Fillet { radius, .. } => ("Радиус", "мм", radius),
            FeatureKind::Chamfer { dist, .. } => ("Расстояние", "мм", dist),
            FeatureKind::Hole { dia, .. } => ("Диаметр", "мм", dia),
            FeatureKind::Shell { thickness, .. } => ("Толщина", "мм", thickness),
            FeatureKind::OffsetPlane { offset, .. } => ("Расстояние", "мм", offset),
            _ => return,
        };
        let mut changed = false;
        egui::Area::new(egui::Id::new("value_box")).fixed_pos(Pos2::new(rect.center().x, rect.top() + 14.0)).pivot(egui::Align2::CENTER_TOP).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr(label)).strong());
                    changed = crate::widgets::expr_field(ui, ("value_box", uid), expr, &params, unit);
                });
            });
        });
        if changed {
            self.after_doc_change();
        }
    }

    /// Нижняя панель вида: вписать, масштаб, стиль отображения, сетка, плоскости, настройки.
    fn nav_bar(&mut self, ui: &mut egui::Ui, rect: Rect, now: f64) {
        let pos = Pos2::new(rect.center().x, rect.bottom() - 12.0);
        egui::Area::new(egui::Id::new("navbar")).fixed_pos(pos).pivot(egui::Align2::CENTER_BOTTOM).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let sz = egui::vec2(30.0, 28.0);
                    let b = |ui: &mut egui::Ui, id: &str, icon: &str, tip: &str| icon_button(ui, id, icon, false, tip, sz);
                    for (m, id, icon, tip) in [(NavMode::Orbit, "nav_orbit", "↻", "Вращение левой кнопкой"), (NavMode::Pan, "nav_pan", "↔", "Сдвиг левой кнопкой"), (NavMode::Zoom, "nav_zoom", "🔍", "Масштаб левой кнопкой")] {
                        let on = self.nav == m;
                        if icon_button(ui, id, icon, on, tr(tip), sz) {
                            self.nav = if on { NavMode::Default } else { m };
                        }
                    }
                    ui.separator();
                    for (id, icon, tip, on) in [("nav_panel_left", "◧", "Левая панель", &mut self.settings.show_left_panel), ("nav_panel_right", "◨", "Правая панель", &mut self.settings.show_right_panel)] {
                        if icon_button(ui, id, icon, *on, tr(tip), sz) {
                            *on = !*on;
                        }
                    }
                    ui.separator();
                    if b(ui, "nav_fit", "⌖", tr("Вписать модель (F)")) {
                        self.fit_view(now, false);
                    }
                    if b(ui, "nav_zoom_in", "+", tr("Приблизить")) {
                        self.cam.zoom_at(0.8, rect, None);
                    }
                    if b(ui, "nav_zoom_out", "−", tr("Отдалить")) {
                        self.cam.zoom_at(1.25, rect, None);
                    }
                    ui.separator();
                    ui.menu_button(RichText::new(format!("◧ {} ▾", tr(self.settings.visual_style.label()))).size(13.0), |ui| {
                        for st in VisualStyle::ALL {
                            if ui.radio_value(&mut self.settings.visual_style, st, tr(st.label())).clicked() {
                                self.settings.save();
                                ui.close_menu();
                            }
                        }
                    });
                    ui.separator();
                    let mut g = self.settings.show_grid;
                    if ui.add(egui::Button::new(RichText::new("▦").size(16.0)).selected(g).frame(false).min_size(egui::vec2(28.0, 26.0))).on_hover_text(tr("Сетка")).clicked() {
                        g = !g;
                        self.settings.show_grid = g;
                        self.settings.save();
                    }
                    if icon_button(ui, "tree_plane", "⊞", self.show_planes, tr("Плоскости начала координат"), sz) {
                        self.show_planes = !self.show_planes;
                    }
                    ui.separator();
                    if b(ui, "nav_settings", "⚙", tr("Настройки")) {
                        self.settings_open = true;
                    }
                });
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open;
        let before = self.settings.clone();
        egui::Window::new(tr("Настройки")).open(&mut open).collapsible(false).default_width(440.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in [tr("Основные"), tr("Просмотр"), tr("О программе")].iter().enumerate() {
                    if ui.selectable_label(self.settings_tab == i as u8, RichText::new(*name).strong()).clicked() {
                        self.settings_tab = i as u8;
                    }
                }
            });
            ui.separator();
            let st = &mut self.settings;
            let row = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui)| {
                ui.horizontal(|ui| {
                    ui.add_sized([170.0, 22.0], egui::Label::new(label).truncate());
                    add(ui);
                });
            };
            match self.settings_tab {
                0 => {
                    row(ui, tr("Язык"), &mut |ui| {
                        egui::ComboBox::from_id_salt("lang").selected_text(st.lang.name()).show_ui(ui, |ui| {
                            for l in Lang::ALL {
                                ui.selectable_value(&mut st.lang, l, l.name());
                            }
                        });
                    });
                    row(ui, tr("Схема управления"), &mut |ui| {
                        ui.selectable_value(&mut st.controls, ControlScheme::Standard, tr("Стандартная"));
                        ui.selectable_value(&mut st.controls, ControlScheme::Blender, "Blender");
                    });
                    if st.controls == ControlScheme::Blender {
                        ui.label(RichText::new(tr("Blender: средняя кнопка — вращение, Shift+средняя — сдвиг, Ctrl+средняя — масштаб; цифры 1/3/7 — виды (Ctrl — с обратной стороны), 5 — ортогональная проекция, 4/6/8/2 — поворот; G — переместить, S — масштаб, X — удалить, Q — новый чертёж, N/T — панели.")).weak().small());
                    }
                    row(ui, tr("Тема"), &mut |ui| {
                        for (t, n) in [(ThemeChoice::Auto, "Как в системе"), (ThemeChoice::Light, "Светлая"), (ThemeChoice::Dark, "Тёмная")] {
                            ui.selectable_value(&mut st.theme, t, tr(n));
                        }
                    });
                    row(ui, tr("Масштаб интерфейса"), &mut |ui| {
                        ui.add(egui::Slider::new(&mut st.ui_scale, 0.8..=1.6).step_by(0.05).fixed_decimals(2));
                    });
                    row(ui, tr("Материал по умолчанию"), &mut |ui| {
                        egui::ComboBox::from_id_salt("mat_default").selected_text(tr(Materials::LIST[st.material].0)).show_ui(ui, |ui| {
                            for (i, (n, _)) in Materials::LIST.iter().enumerate() {
                                ui.selectable_value(&mut st.material, i, tr(n));
                            }
                        });
                    });
                    row(ui, tr("Автосохранение"), &mut |ui| {
                        let mut on = st.autosave_minutes > 0;
                        if ui.checkbox(&mut on, tr("каждые")).changed() {
                            st.autosave_minutes = if on { 5 } else { 0 };
                        }
                        if st.autosave_minutes > 0 {
                            ui.add(egui::DragValue::new(&mut st.autosave_minutes).range(1..=60).suffix(tr(" мин")));
                        }
                    });
                    ui.label(RichText::new(tr("Автосохранение пишет в уже сохранённый файл.")).weak().small());
                }
                1 => {
                    row(ui, tr("Стиль отображения"), &mut |ui| {
                        egui::ComboBox::from_id_salt("vstyle").selected_text(tr(st.visual_style.label())).show_ui(ui, |ui| {
                            for v in VisualStyle::ALL {
                                ui.selectable_value(&mut st.visual_style, v, tr(v.label()));
                            }
                        });
                    });
                    row(ui, tr("Сетка"), &mut |ui| {
                        ui.checkbox(&mut st.show_grid, tr("показывать"));
                    });
                    row(ui, tr("Привязка к сетке в чертеже"), &mut |ui| {
                        ui.checkbox(&mut st.snap_grid, tr("включена по умолчанию"));
                    });
                    row(ui, tr("Числа вдоль осей"), &mut |ui| {
                        ui.checkbox(&mut st.grid_numbers, tr("показывать"));
                    });
                    row(ui, tr("Толщина линий рёбер"), &mut |ui| {
                        ui.add(egui::Slider::new(&mut st.edge_width, 0.8..=3.0).fixed_decimals(1));
                    });
                    row(ui, tr("Перспектива (угол обзора)"), &mut |ui| {
                        ui.add(egui::Slider::new(&mut st.fov_deg, 10.0..=70.0).suffix("°").fixed_decimals(0));
                    });
                    row(ui, tr("Скорость вращения"), &mut |ui| {
                        ui.add(egui::Slider::new(&mut st.orbit_speed, 0.3..=3.0).fixed_decimals(1));
                    });
                    row(ui, tr("Скорость масштаба"), &mut |ui| {
                        ui.add(egui::Slider::new(&mut st.zoom_speed, 0.3..=3.0).fixed_decimals(1));
                    });
                    row(ui, tr("Направление масштаба"), &mut |ui| {
                        ui.checkbox(&mut st.invert_zoom, tr("обратное"));
                    });
                }
                _ => {
                    ui.heading("Genix");
                    ui.label(format!("{} {}", tr("Версия"), env!("CARGO_PKG_VERSION")));
                    ui.label(tr("Параметрический 3D-CAD: чертежи, твердотельное моделирование, история построения."));
                    ui.add_space(6.0);
                    if let Some(p) = Settings::path() {
                        ui.label(RichText::new(format!("{}: {}", tr("Файл настроек"), p.display())).weak().small());
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("ЛИЦЕНЗИЯ И АККАУНТ")).small().weak());
                    let lic = match st.license.as_deref() {
                        Some("DEMO") => tr("Демо-лицензия").to_string(),
                        Some(k) => format!("{} {}…{}", tr("Ключ"), &k[..4.min(k.len())], &k[k.len().saturating_sub(4)..]),
                        None => tr("Нет лицензии").to_string(),
                    };
                    ui.label(lic);
                    ui.label(match &st.account {
                        Some(a) => tf!("Аккаунт: {}", a),
                        None => tr("Аккаунт: не выполнен вход").to_string(),
                    });
                    ui.horizontal(|ui| {
                        if ui.button(tr("Выйти из аккаунта")).clicked() {
                            st.account = None;
                            st.account_skipped = false;
                        }
                        if ui.button(tr("Сменить лицензию")).clicked() {
                            st.license = None;
                        }
                    });
                    ui.add_space(6.0);
                    if ui.button(tr("Сбросить все настройки")).clicked() {
                        *st = Settings { lang: st.lang, license: st.license.take(), account: st.account.take(), account_skipped: st.account_skipped, recent: std::mem::take(&mut st.recent), ..Settings::default() };
                    }
                }
            }
        });
        self.settings_open = open;
        if self.settings != before {
            i18n::set_lang(self.settings.lang);
            self.settings.save();
        }
    }
}

/// Параметр точки на прямой (base + dir·t), ближайшей к лучу (o + d·s).
fn line_param(base: DVec3, dir: DVec3, o: DVec3, d: DVec3) -> f64 {
    let w0 = base - o;
    let (b, dd, e) = (dir.dot(d), dir.dot(w0), d.dot(w0));
    let den = 1.0 - b * b;
    if den.abs() < 1e-9 { 0.0 } else { (b * e - dd) / den }
}

