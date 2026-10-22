//! Главное окно: меню, панель инструментов, браузер, 3D-вид, история (timeline), диалоги.

use crate::camera::Camera;
use crate::commands::{self, CmdResult, Command, PickTarget};
use crate::gpu::{FrameUniform, GpuScene, LineInst, Overlay, FaceVertex};
use crate::render::{self, fill_quad, fill_region, fill_tris, nice_step, polyline, RenderCache};
use crate::sketcher::{SketchAction, SketchSession};
use crate::theme::{linear, Theme, accent};
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
    Quit(Vec<usize>),
}

enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

/// Тип устройства: компьютер, планшет (iPad — крупнее и под палец) или телефон (iPhone — только просмотр).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Desktop,
    Tablet,
    Phone,
}

fn detect_device(ctx: &egui::Context) -> Device {
    match std::env::var("GENIX_DEVICE").as_deref() {
        Ok("phone") => return Device::Phone,
        Ok("tablet") => return Device::Tablet,
        _ => {}
    }
    if !cfg!(target_os = "ios") {
        return Device::Desktop;
    }
    let r = ctx.screen_rect();
    if r.width().min(r.height()) < 600.0 { Device::Phone } else { Device::Tablet }
}

/// What a palette row does.
#[derive(Clone, Copy)]
enum PaletteCmd {
    Sketch,
    Start(&'static str),
    Fit,
    Save,
    Export(&'static str),
    Slicer(usize),
    Home,
    Settings,
    NewTab,
    Grid,
}

/// Группа вкладок как в Chrome: общее имя и цвет, можно свернуть.
#[derive(Clone)]
struct TabGroup {
    id: u32,
    name: String,
    color: Color32,
    collapsed: bool,
}

const GROUP_COLORS: [Color32; 8] = [
    Color32::from_rgb(10, 132, 255), Color32::from_rgb(255, 69, 58), Color32::from_rgb(255, 159, 10), Color32::from_rgb(48, 209, 88),
    Color32::from_rgb(255, 55, 140), Color32::from_rgb(175, 82, 222), Color32::from_rgb(50, 173, 230), Color32::from_rgb(142, 142, 147),
];

/// Припаркованное состояние неактивной вкладки-документа.
#[derive(Default)]
struct Tab {
    suggested: Option<String>,
    group: Option<u32>,
    doc: Document,
    ev: Evaluator,
    path: Option<PathBuf>,
    saved: Document,
    undo: Vec<Document>,
    redo: Vec<Document>,
    cam: Camera,
    selection: Vec<u64>,
    folder: Option<String>,
}

struct Materials;
impl Materials {
    /// Название, плотность г/см³, базовый цвет, сила отражения.
    const LIST: [(&'static str, f64, Color32, f32); 8] = [
        ("Сталь", 7.85, Color32::from_rgb(142, 151, 162), 0.72),
        ("Нерж. сталь", 8.0, Color32::from_rgb(190, 198, 207), 0.92),
        ("Алюминий", 2.70, Color32::from_rgb(178, 190, 205), 0.62),
        ("Латунь", 8.5, Color32::from_rgb(191, 145, 52), 0.78),
        ("PLA (3D-печать)", 1.24, Color32::from_rgb(103, 151, 214), 0.16),
        ("ABS/PETG", 1.05, Color32::from_rgb(103, 111, 126), 0.24),
        ("Дерево (сосна)", 0.52, Color32::from_rgb(177, 123, 73), 0.06),
        // reflection 2.0 is the shader's sign for frosted glass
        ("Стекло", 2.5, Color32::from_rgb(170, 220, 236), 2.0),
    ];

    fn get(index: usize) -> (&'static str, f64, Color32, f32) {
        Self::LIST[index.min(Self::LIST.len() - 1)]
    }
}

pub struct App {
    /// Вкладки: неактивные хранят своё состояние, у активной (`active`) слот пустой.
    tabs: Vec<Tab>,
    active: usize,
    gate: crate::home::Gate,
    device: Device,
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
    /// Папка, в которую попадёт этот проект при первом сохранении.
    folder: Option<String>,
    /// Имя, выбранное при создании модели: показывается во вкладке и подставляется в окно «Сохранить».
    suggested: Option<String>,
    group: Option<u32>,
    tab_groups: Vec<TabGroup>,
    next_group: u32,
    /// Короткая плашка «Открываем проект…» после New/Open из меню, Finder и библиотеки.
    busy: Option<(f64, &'static str)>,
    /// Палитра команд (⌘K): текст запроса и выбранная строка; `None` — закрыта.
    palette: Option<(String, usize)>,
    splash_started: Option<f64>,
    hover: Option<Hit>,
    cache: RenderCache,
    /// модель, чьи буферы уже на GPU (держим ссылку, чтобы адрес не достался новой модели)
    shown_model: Option<Arc<Model>>,
    shown_edge_w: u32,
    ortho: bool,
    /// данные тел для GPU и их номер: сцена перезаливает буферы, когда номер меняется
    /// (кадр egui может быть выброшен и перерисован, поэтому данные не «забираем», а держим)
    upload: Arc<(Vec<FaceVertex>, Vec<LineInst>)>,
    upload_gen: u64,
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
    /// Ребро под курсором и тело, которому оно принадлежит.
    hover_edge: Option<(EdgeRef, u64)>,
    edge_cache: std::cell::RefCell<std::collections::HashMap<usize, (Arc<fusion_core::csg::Solid>, Arc<Vec<EdgeRef>>)>>,
    toast: Option<(String, f64, bool)>,
    rename_body: Option<(u64, String)>,
    rename_feature: Option<(u64, String)>,
    params_open: bool,
    help_open: bool,
    report: crate::report::ReportState,
    crash_checked: bool,
    /// sketch picked in the tree: Delete removes it
    tree_sketch: Option<u64>,
    /// wheel zoom (natural log of the factor) still to be applied, and where it is aimed
    zoom_pending: f64,
    zoom_cursor: Option<egui::Pos2>,
    /// Align: the target as it was before hover previews started changing it
    align_saved: Option<(fusion_core::Plane, Option<[f64; 3]>)>,
    confirm: Option<Confirm>,
    allow_close: bool,
    need_fit: bool,
    /// Тело/шаг под правой кнопкой (для контекстного меню).
    context_body: Option<u64>,
    new_param: (String, String),
    viewport: Rect,
    face_groups: std::cell::RefCell<std::collections::HashMap<usize, (Arc<fusion_core::csg::Solid>, Arc<Vec<u32>>)>>,
    /// Вид камеры до входа в чертёж — возвращаемся к нему после.
    pre_sketch_view: Option<(f64, f64)>,
    now: f64,
}

/// Кнопка со значком из набора (нет значка — текстовый символ).
fn icon_button(ui: &mut egui::Ui, id: &str, fallback: &str, selected: bool, tip: &str, min: egui::Vec2) -> bool {
    let enabled = ui.is_enabled();
    let (rect, response) = ui.allocate_exact_size(min, egui::Sense::click());
    let hover = response.hovered() && !ui.input(|i| i.any_touches());
    if selected || (hover && enabled) {
        ui.painter().rect_filled(rect, 12.0, if selected { crate::theme::accent() } else { crate::theme::tint(20) });
    }
    let main = if selected { crate::theme::on_accent() } else if enabled { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };
    // painted at the exact centre of the button: egui's image button lays its content out from the left
    match crate::icons::icon(id, 22.0, main) {
        Some(img) => img.paint_at(ui, Rect::from_center_size(rect.center(), egui::vec2(22.0, 22.0))),
        None => { ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, fallback, egui::FontId::proportional(16.0), main); }
    }
    let response = response.on_hover_cursor(if enabled { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Default });
    crate::widgets::warm_tooltip(&response, tip);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    response.clicked()
}

/// Группа дерева в стиле Fusion: стрелка, значок папки, название; вся строка подсвечивается при наведении.
fn tree_group(ui: &mut egui::Ui, id: &str, icon: &str, title: String, default_open: bool, body: impl FnOnce(&mut egui::Ui)) {
    use egui::collapsing_header::CollapsingState;
    let cid = ui.make_persistent_id(id);
    let mut st = CollapsingState::load_with_default_open(ui.ctx(), cid, default_open);
    let open = st.is_open();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), 12.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let col = ui.visuals().text_color();
    let mut x = rect.left() + 4.0;
    let draw = |name: &str, size: f32, x: &mut f32| {
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
    let rows_id = egui::Id::new("tree-rows");
    ui.data_mut(|d| d.insert_temp(rows_id, Vec::<(f32, bool)>::new()));
    let shown = ui.scope(|ui| {
        ui.spacing_mut().indent = 24.0;
        st.show_body_indented(&resp, ui, body)
    }).inner;
    let rows = ui.data_mut(|d| d.get_temp::<Vec<(f32, bool)>>(rows_id)).unwrap_or_default();
    if let Some(inner) = shown {
        draw_branches(ui, cid, rect, inner.response.rect, &rows);
    }
}

/// Row of a tree group; remembers its vertical centre so the group can draw a branch to it.
fn tree_row(ui: &mut egui::Ui, selected: bool, add: impl FnOnce(&mut egui::Ui)) {
    let y = ui.horizontal(add).response.rect.center().y;
    ui.data_mut(|d| d.get_temp_mut_or_default::<Vec<(f32, bool)>>(egui::Id::new("tree-rows")).push((y, selected)));
}

/// Trunk under the group arrow with a curved branch into every row; the selected row gets an accent line that travels down to it.
fn draw_branches(ui: &egui::Ui, id: egui::Id, header: Rect, body: Rect, rows: &[(f32, bool)]) {
    let Some(&(last, _)) = rows.last() else { return };
    let p = ui.painter().with_clip_rect(Rect::from_min_max(header.left_bottom(), egui::pos2(header.right(), body.bottom() + 2.0)));
    let (x, r) = (header.left() + 11.0, 7.0);
    let end = x + r + 5.0;
    let top = header.bottom() - 4.0;
    let line = egui::Stroke::new(1.5_f32, ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.3));
    let branch = |y: f32| -> Vec<Pos2> {
        let mut pts: Vec<Pos2> = (0..=8).map(|i| {
            let a = std::f32::consts::PI * (1.0 - 0.5 * i as f32 / 8.0);
            egui::pos2(x + r + r * a.cos(), y - r + r * a.sin())
        }).collect();
        pts.push(egui::pos2(end, y));
        pts
    };
    p.line_segment([egui::pos2(x, top), egui::pos2(x, last - r)], line);
    for &(y, _) in rows { p.add(egui::Shape::line(branch(y), line)); }
    if let Some(&(sel, _)) = rows.iter().find(|row| row.1) {
        let glide = if crate::theme::reduced_motion() { 0.0 } else { 0.3 };
        let reach = ui.ctx().animate_value_with_time(id.with("reach"), sel, glide);
        let accent = egui::Stroke::new(1.5_f32, crate::theme::accent());
        p.line_segment([egui::pos2(x, top), egui::pos2(x, (reach - r).max(top))], accent);
        if reach >= sel - 0.5 { p.add(egui::Shape::line(branch(sel), accent)); }
    }
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
pub(crate) fn menu_item(ui: &mut egui::Ui, icon: &str, text: &str) -> bool {
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
        let mut settings = Settings::load();
        // an existing install has already seen the splash: it skips the first-launch setup
        if !settings.splash_seen.is_empty() { settings.setup_done = true; }
        i18n::set_lang(settings.lang);
        let theme = resolve_theme(&settings, &cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        Theme::install_fonts(&cc.egui_ctx);
        crate::theme::configure(settings.accent, theme == Theme::Dark, settings.reduce_motion);
        theme.apply(&cc.egui_ctx);
        cc.egui_ctx.set_zoom_factor(settings.ui_scale);
        let mut app = Self::from_settings(settings, theme);
        if let Some(p) = open {
            app.open_path(p);
        } else if std::env::var("GENIX_DEMO").is_ok() {
            app.load_doc(crate::examples::bracket(), None);
        }
        // QA: start on a given library page or inside a folder without clicking (GENIX_PAGE=team|server|settings|updates|profile)
        if let Ok(page) = std::env::var("GENIX_PAGE") {
            if let Some(page) = crate::home::HomePage::from_name(&page) { app.home.navigate(page); }
        }
        if let Some(folder) = std::env::var("GENIX_FOLDER").ok().filter(|f| !f.is_empty()) { app.home.open_folder(&folder); }
        if std::env::var("GENIX_SKIP_LOGIN").is_ok() {
            app.settings.license.get_or_insert_with(|| "DEMO".into());
            app.settings.account_skipped = true;
        }
        let list = app.project_list();
        app.home.refresh(&list);
        app
    }

    fn from_settings(settings: Settings, theme: Theme) -> Self {
        Self {
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
            device: Device::Desktop,
            home: Default::default(),
            home_active: true,
            theme,
            selection: Vec::new(),
            folder: None,
            suggested: None,
            group: None,
            tab_groups: Vec::new(),
            next_group: 1,
            busy: None,
            palette: None,
            splash_started: None,
            hover: None,
            cache: RenderCache::default(),
            shown_model: None,
            shown_edge_w: 0,
            ortho: false,
            upload: Default::default(),
            upload_gen: 0,
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
            report: Default::default(),
            crash_checked: false,
            tree_sketch: None,
            zoom_pending: 0.0,
            zoom_cursor: None,
            align_saved: None,
            confirm: None,
            allow_close: false,
            need_fit: false,
            context_body: None,
            new_param: (String::new(), String::new()),
            viewport: Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0)),
            face_groups: Default::default(),
            pre_sketch_view: None,
            now: 0.0,
        }
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

    /// Centre and radius of everything that is drawn (or a default sphere around the origin for an empty scene).
    fn scene_sphere(&self) -> (DVec3, f64) {
        let model = self.ev.model();
        let (mut lo, mut hi) = (DVec3::splat(f64::MAX), DVec3::splat(f64::MIN));
        for b in &model.bodies {
            if let Some((a, c)) = b.mesh().bounds() {
                lo = lo.min(a);
                hi = hi.max(c);
            }
        }
        if lo.x > hi.x { return (DVec3::ZERO, 100.0); }
        ((lo + hi) / 2.0, ((hi - lo) / 2.0).length().max(10.0))
    }

    /// Lay on: while the target is being chosen, the body is shown already lying on the face or plane under the cursor.
    /// Moving away gives back the target chosen before; a click keeps the previewed one.
    fn align_hover_preview(&mut self) {
        let uid = match &self.mode {
            Mode::Command(c) if c.pick == PickTarget::AlignTarget => c.uid,
            _ => {
                self.align_saved = None;
                return;
            }
        };
        let model = self.pick_model();
        let (o, d) = self.last_ray;
        let user_planes = self.doc.offset_planes();
        let Some(FeatureKind::PlaceOnPlane { body, target, to, .. }) = self.doc.feature(uid).map(|f| f.kind.clone()) else { return };
        let wanted: Option<(fusion_core::Plane, Option<[f64; 3]>)> = match self.hover {
            Some(Hit::Body { slot, tri, t }) if model.bodies.get(slot).is_some_and(|b| b.id != body) => {
                self.face_plane(&model, slot, tri).map(|(pl, _)| (pl, Some((o + d * t).to_array())))
            }
            Some(Hit::OriginPlane(i, _)) => Some((ORIGIN_PLANES[i], None)),
            Some(Hit::UserPlane(i, _)) => user_planes.get(i).map(|(_, _, pl)| (*pl, None)),
            _ => None,
        };
        let saved = *self.align_saved.get_or_insert((target, to));
        let want = wanted.unwrap_or(saved);
        if (target, to) != want {
            if let Some(FeatureKind::PlaceOnPlane { target, to, .. }) = self.doc.feature_mut(uid).map(|f| &mut f.kind) {
                *target = want.0;
                *to = want.1;
            }
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
        // a loaded document starts clean: no leftover folder, group or name from the tab's previous life
        self.folder = None;
        self.group = None;
        self.suggested = None;
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
        let name = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).or_else(|| self.suggested.clone()).unwrap_or_else(|| tr("Без имени").into());
        format!("{name}{} — Genix", if self.dirty() { " •" } else { "" })
    }

    /// Проекты для главной: недавние и (на iPad/iPhone) все файлы из папки «Документы».
    fn project_list(&self) -> Vec<String> {
        let mut v = self.settings.recent.clone();
        if cfg!(target_os = "ios") || self.device != Device::Desktop {
            for p in crate::files::documents_with(&["genix", "ostov"]) {
                if !v.contains(&p) {
                    v.push(p);
                }
            }
        }
        v
    }

    fn note_recent(&mut self, p: &std::path::Path) {
        let s = p.to_string_lossy().into_owned();
        self.settings.recent.retain(|x| *x != s);
        self.settings.recent.insert(0, s);
        self.settings.recent.truncate(40);
        self.settings.save();
    }

    fn handle_home_action(&mut self, action: crate::home::HomeAction) {
        if matches!(action, crate::home::HomeAction::Report) {
            self.report.show(false);
            return;
        }
        use crate::home::HomeAction;
        match action {
            HomeAction::None | HomeAction::Report => {},
            HomeAction::New => self.new_blank_tab(),
            HomeAction::NewIn(folder) => {
                self.new_blank_tab();
                self.folder = Some(folder);
            }
            HomeAction::NewNamed(name, folder) => {
                self.set_busy(tr("Подготовка проекта…"));
                self.new_blank_tab();
                self.folder = folder;
                self.suggested = Some(name);
            }
            HomeAction::Open => self.open_dialog(),
            HomeAction::OpenPath(path) => self.open_path(path),
            HomeAction::Example(i) => {
                let example: fn() -> Document = if i == 0 { crate::examples::bracket } else { crate::examples::bushing };
                self.open_in_tab(example(), None);
            }
            HomeAction::Forget(path) => {
                self.settings.recent.retain(|p| *p != path);
                self.settings.save();
                let list = self.project_list();
                self.home.refresh(&list);
            }
            HomeAction::SettingsChanged => self.settings.save(),
            HomeAction::SignOut => {
                self.settings.account = None;
                self.settings.account_skipped = false;
                self.settings.save();
                self.gate = Default::default();
                self.home_active = true;
                self.home.navigate(crate::home::HomePage::Models);
            }
        }
    }

    fn open_path(&mut self, p: PathBuf) {
        self.set_busy(tr("Открываем проект…"));
        let canonical=std::fs::canonicalize(&p).ok();
        if let Some(index)=self.tabs.iter().position(|tab| tab.path.as_ref().is_some_and(|other| other==&p || canonical.as_ref().is_some_and(|p|std::fs::canonicalize(other).ok().as_ref()==Some(p)))) {
            self.switch_tab(index); self.home_active=false; return;
        }
        match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|s| Document::from_json(&s)) {
            Ok(d) => {
                self.note_recent(&p);
                self.open_in_tab(d, Some(p));
            }
            Err(e) => self.toast = Some((tf!("Не удалось открыть: {}", e), 0.0, true)),
        }
    }

    /// Opens an older version of the current file in a new tab, unsaved, so the file itself stays untouched.
    fn open_version(&mut self, file: &std::path::Path, stamp: u64, now: f64) {
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match std::fs::read_to_string(file).map_err(|e| e.to_string()).and_then(|s| Document::from_json(&s)) {
            Ok(d) => {
                self.new_blank_tab();
                self.load_doc(d, None);
                self.suggested = Some(format!("{stem} ({})", crate::home::format_time(stamp)));
            }
            Err(e) => self.toast(tf!("Не удалось открыть: {}", e), true, now),
        }
    }

    fn open_dialog(&mut self) {
        if let Some(p) = crate::files::open("Genix", &["genix", "ostov"]) {
            self.open_path(p);
        }
    }

    fn save(&mut self, as_new: bool, now: f64) -> bool {
        self.commit_pending_edits();
        let path = match (&self.path, as_new) {
            (Some(p), false) => Some(p.clone()),
            _ => crate::files::save("Genix", &["genix"], &self.suggested.as_ref().map_or_else(|| tr("деталь.genix").to_string(), |n| format!("{n}.genix"))),
        };
        let Some(mut p) = path else { return false };
        if p.extension().is_none() {
            p.set_extension("genix");
        }
        if let Some(base) = crate::files::versions_base() {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let _ = crate::files::snapshot_version(&base, &p, stamp);
        }
        match crate::files::write_atomic(&p, self.doc.to_json().as_bytes()) {
            Ok(()) => {
                self.saved = self.doc.clone();
                self.last_autosave = now;
                self.note_recent(&p);
                let key = p.to_string_lossy().into_owned();
                if let Some(name) = self.folder.take() {
                    let target = self.settings.folders.iter().position(|f| f.name == name);
                    self.settings.move_to_folder(&key, target);
                    self.settings.save();
                }
                let synced = self.settings.server_active() && self.settings.folder_of(&key).is_some_and(|i| self.settings.folders[i].auto_sync);
                self.path = Some(p);
                self.suggested = None;
                self.toast(if synced { tr("Сохранено · отправлено на сервер (демо)") } else { tr("Сохранено") }, false, now);
                true
            }
            Err(e) => {
                self.toast(tf!("Ошибка сохранения: {}", e), true, now);
                false
            }
        }
    }

    /// Автосохранение всех именованных вкладок без переключения текущей.
    fn autosave(&mut self, now: f64) {
        let mut error = None;
        if self.dirty() {
            if let Some(path) = &self.path {
                match crate::files::write_atomic(path, self.doc.to_json().as_bytes()) {
                    Ok(()) => self.saved = self.doc.clone(),
                    Err(e) => error = Some(format!("{}: {e}", path.display())),
                }
            }
        }
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i == self.active || tab.doc == tab.saved {
                continue;
            }
            if let Some(path) = &tab.path {
                match crate::files::write_atomic(path, tab.doc.to_json().as_bytes()) {
                    Ok(()) => tab.saved = tab.doc.clone(),
                    Err(e) => error = Some(format!("{}: {e}", path.display())),
                }
            }
        }
        if let Some(error) = error {
            self.toast(tf!("Ошибка сохранения: {}", error), true, now);
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
        let Some(p) = crate::files::save("PNG", &["png"], &name) else { return };
        let buf = image::RgbaImage::from_raw(img.w as u32, img.h as u32, img.data).unwrap();
        match buf.save(&p) {
            Ok(()) => self.toast(tr("Снимок сохранён (прозрачный фон)"), false, now),
            Err(e) => self.toast(tf!("Ошибка экспорта: {}", e), true, now),
        }
    }

    /// Selected (or all visible) bodies written in `fmt` ("stl", "3mf" or "obj"); the count of bodies goes along.
    fn model_bytes(&self, fmt: &str) -> Option<(Vec<u8>, usize)> {
        let model = self.ev.model();
        let mut bodies: Vec<&fusion_core::doc::Body> = model.bodies.iter().filter(|b| self.selection.contains(&b.id)).collect();
        if bodies.is_empty() {
            bodies = model.bodies.iter().filter(|b| !self.doc.hidden_bodies.contains(&b.id)).collect();
        }
        if bodies.is_empty() { return None; }
        let named = || -> Vec<(&str, &Mesh)> { bodies.iter().map(|b| (self.doc.body_name(b), b.mesh())).collect() };
        let bytes = match fmt {
            "obj" => Mesh::write_obj(&named()).into_bytes(),
            "3mf" => Mesh::write_3mf(&named()),
            _ => {
                let mut out = Vec::new();
                Mesh::write_stl_binary(&bodies.iter().map(|b| b.mesh()).collect::<Vec<_>>(), &mut out);
                out
            }
        };
        Some((bytes, bodies.len()))
    }

    fn export(&mut self, fmt: &str, now: f64) {
        if self.model_bytes(fmt).is_none() {
            self.toast(tr("Нет тел для экспорта"), true, now);
            return;
        }
        let (ext, label) = match fmt { "obj" => ("obj", "OBJ"), "3mf" => ("3mf", "3MF"), _ => ("stl", tr("STL (3D-печать)")) };
        let name = format!("{}.{ext}", self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("модель").into()));
        let Some(p) = crate::files::save(label, &[ext], &name) else { return };
        let Some((bytes, count)) = self.model_bytes(fmt) else { return };
        match std::fs::write(&p, bytes) {
            Ok(()) => self.toast(tf!("Экспортировано тел: {}", count), false, now),
            Err(e) => self.toast(tf!("Ошибка экспорта: {}", e), true, now),
        }
    }

    /// Writes the model to a temporary file and opens it in the chosen slicer app.
    fn open_in_slicer(&mut self, index: usize, now: f64) {
        let slicer = &crate::slicers::SLICERS[index];
        let Some(app) = crate::slicers::installed(slicer) else {
            self.toast(tf!("{} не установлен", slicer.name), true, now);
            return;
        };
        let Some((bytes, _)) = self.model_bytes(slicer.format) else {
            self.toast(tr("Нет тел для экспорта"), true, now);
            return;
        };
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| tr("модель").into());
        let result = crate::slicers::temp_file(&stem, slicer.format).and_then(|file| std::fs::write(&file, bytes).map(|_| file)).map_err(|e| e.to_string()).and_then(|file| crate::slicers::open_in(app, &file));
        match result {
            Ok(()) => self.toast(tf!("Открываю в {}…", slicer.name), false, now),
            Err(e) => self.toast(tf!("Не удалось открыть в слайсере: {}", e), true, now),
        }
    }

    fn commit_pending_edits(&mut self) {
        match &self.mode {
            Mode::Sketch(_) => self.finish_sketch(true),
            Mode::Command(_) => self.finish_command(true),
            _ => self.mode = Mode::Idle,
        }
    }

    fn dirty_tabs(&self) -> Vec<usize> {
        (0..self.tabs.len()).filter(|&i| {
            if i == self.active { self.dirty() } else { self.tabs[i].doc != self.tabs[i].saved }
        }).collect()
    }

    fn request(&mut self, c: Confirm) {
        if self.confirm.is_some() {
            return;
        }
        if matches!(c, Confirm::Quit(_) | Confirm::CloseTab) {
            self.commit_pending_edits();
        }
        if matches!(c, Confirm::Quit(_)) {
            // Каждая изменённая вкладка получает собственное решение.
            let mut pending = self.dirty_tabs();
            pending.reverse();
            self.run_confirmed(Confirm::Quit(pending));
        } else if matches!(c, Confirm::CloseTab) && self.dirty() {
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
            folder: self.folder.take(),
            group: self.group.take(),
            suggested: self.suggested.take(),
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
        self.folder = t.folder;
        self.group = t.group;
        self.suggested = t.suggested;
        self.mode = Mode::Idle;
        self.home_active = false;
        self.shown_model = None;
        self.after_doc_change();
    }

    fn tab_title(&self, i: usize) -> (String, bool) {
        let (path, dirty) = if i == self.active { (&self.path, self.dirty()) } else { (&self.tabs[i].path, self.tabs[i].doc != self.tabs[i].saved) };
        let suggested = if i == self.active { &self.suggested } else { &self.tabs[i].suggested };
        let name = path.as_ref().and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().to_string()).or_else(|| suggested.clone()).unwrap_or_else(|| tr("Без имени").into());
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

    /// Every action the palette can run; `needs_model` ones are greyed out while there is nothing to act on.
    fn palette_items(&self) -> Vec<(&'static str, String, PaletteCmd, bool)> {
        use PaletteCmd::*;
        let mut v = vec![
            ("sketch", tr("Новый чертёж").to_string(), Sketch, false),
            ("extrude", tr("Выдавить").to_string(), Start("extrude"), false),
            ("revolve", tr("Вращение").to_string(), Start("revolve"), false),
            ("hole", tr("Отверстие").to_string(), Start("hole"), true),
            ("fillet", tr("Скругление").to_string(), Start("fillet"), true),
            ("chamfer", tr("Фаска").to_string(), Start("chamfer"), true),
            ("press_pull", tr("Вытянуть грань").to_string(), Start("press_pull"), true),
            ("align", tr("Наложить").to_string(), Start("place"), true),
            ("move", tr("Переместить").to_string(), Start("move"), true),
            ("nav_fit", tr("Вписать модель").to_string(), Fit, true),
            ("file_save", tr("Сохранить").to_string(), Save, false),
            ("file_export_stl", tr("Экспорт STL (3D-печать)…").to_string(), Export("stl"), true),
            ("file_export_3mf", tr("Экспорт 3MF (3D-печать)…").to_string(), Export("3mf"), true),
            ("home_models", tr("Главная").to_string(), Home, false),
            ("settings", tr("Настройки…").to_string(), Settings, false),
            ("add", tr("Новая вкладка").to_string(), NewTab, false),
            ("nav_grid", tr("Сетка").to_string(), Grid, false),
        ];
        // every toolbar tool is searchable too, so nothing hides behind a group dropdown
        for item in [crate::toolbar::CREATE, crate::toolbar::MODIFY, crate::toolbar::REFERENCE, crate::toolbar::INSPECT].into_iter().flatten() {
            if !v.iter().any(|(_, _, c, _)| matches!(c, Start(k) if *k == item.id)) && item.id != "sketch" {
                v.push((item.id, tr(item.label).to_string(), Start(item.id), false));
            }
        }
        for (i, s) in crate::slicers::SLICERS.iter().enumerate() {
            if crate::slicers::installed(s).is_some() { v.push(("file_export_stl", format!("{} {}", tr("Открыть в слайсере"), s.name), Slicer(i), true)); }
        }
        v
    }

    /// Command palette: ⌘K / Ctrl+K opens a search over every action; ↑↓ choose, Enter runs, Esc closes.
    fn palette_ui(&mut self, ctx: &egui::Context, now: f64) {
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K)) {
            self.palette = if self.palette.is_some() { None } else { Some((String::new(), 0)) };
        }
        let Some((mut query, mut selected)) = self.palette.clone() else { return };
        let has_model = !self.home_active && !self.ev.model().bodies.is_empty();
        let needle = query.trim().to_lowercase();
        let items: Vec<_> = self.palette_items().into_iter().filter(|(_, label, _, _)| needle.is_empty() || label.to_lowercase().contains(&needle)).collect();
        selected = selected.min(items.len().saturating_sub(1));
        let (down, up, enter, esc) = ctx.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
        if down { selected = (selected + 1).min(items.len().saturating_sub(1)); }
        if up { selected = selected.saturating_sub(1); }
        let mut run: Option<PaletteCmd> = None;
        egui::Area::new(egui::Id::new("palette")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 110.0)).show(ctx, |ui| {
            egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(22).inner_margin(egui::Margin::same(12)).stroke(egui::Stroke::new(1.0_f32, ui.visuals().window_stroke.color)).show(ui, |ui| {
                ui.set_width(540.0);
                let field = ui.add(egui::TextEdit::singleline(&mut query).hint_text(RichText::new(tr("Что сделать?")).color(crate::theme::secondary(ui))).font(egui::FontId::proportional(18.0)).margin(egui::Margin::symmetric(14, 12)).desired_width(f32::INFINITY));
                field.request_focus();
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                    for (i, (icon, label, cmd, needs_model)) in items.iter().enumerate() {
                        let usable = !needs_model || has_model;
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 38.0), if usable { egui::Sense::click() } else { egui::Sense::hover() });
                        if i == selected || resp.hovered() && usable { ui.painter().rect_filled(rect, 10.0, crate::theme::tint(if i == selected { 46 } else { 22 })); }
                        let ink = if usable { ui.visuals().text_color() } else { ui.visuals().text_color().gamma_multiply(0.4) };
                        if let Some(img) = crate::icons::icon(icon, 18.0, ink) { img.paint_at(ui, egui::Rect::from_center_size(rect.left_center() + egui::vec2(20.0, 0.0), egui::vec2(18.0, 18.0))); }
                        ui.painter().text(rect.left_center() + egui::vec2(42.0, 0.0), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), ink);
                        if usable && resp.clicked() { run = Some(*cmd); }
                        if usable && enter && i == selected { run = Some(*cmd); }
                    }
                    if items.is_empty() { ui.label(RichText::new(tr("Ничего не найдено")).color(crate::theme::secondary(ui))); }
                });
            });
        });
        self.palette = if run.is_some() || esc { None } else { Some((query, selected)) };
        if let Some(cmd) = run {
            use PaletteCmd::*;
            match cmd {
                Sketch => self.start_sketch(),
                Start(kind) => self.run_tool(kind, now),
                Fit => self.need_fit = true,
                Save => { self.save(false, now); }
                Export(fmt) => self.export(fmt, now),
                Slicer(i) => self.open_in_slicer(i, now),
                Home => {
                    self.commit_pending_edits();
                    self.home_active = true;
                    self.home.navigate(crate::home::HomePage::Models);
                    let list = self.project_list();
                    self.home.refresh(&list);
                }
                Settings => {
                    self.commit_pending_edits();
                    self.home_active = true;
                    self.home.navigate(crate::home::HomePage::Settings);
                }
                NewTab => self.new_blank_tab(),
                Grid => { self.settings.show_grid = !self.settings.show_grid; self.settings.save(); }
            }
        }
    }

    fn set_busy(&mut self, label: &'static str) {
        self.busy = Some((self.now + 0.6, label));
    }

    /// Dimmed screen with a spinner card while a project is being prepared or opened.
    fn busy_overlay(&mut self, ctx: &egui::Context, now: f64) {
        let Some((until, label)) = self.busy else { return };
        if now >= until { self.busy = None; return; }
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("busy-overlay")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
            ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(90));
            let card = Rect::from_center_size(rect.center(), egui::vec2(300.0, 76.0));
            ui.painter().rect_filled(card, 22.0, crate::theme::surface(ui));
            let spin = egui::pos2(card.left() + 38.0, card.center().y);
            let t = now as f32 * 5.5;
            let arc: Vec<egui::Pos2> = (0..=14).map(|i| {
                let a = t + i as f32 * std::f32::consts::TAU * 0.75 / 14.0;
                spin + egui::vec2(a.cos(), a.sin()) * 11.0
            }).collect();
            ui.painter().add(egui::Shape::line(arc, egui::Stroke::new(3.0_f32, crate::theme::accent())));
            ui.painter().text(egui::pos2(card.left() + 68.0, card.center().y), egui::Align2::LEFT_CENTER, label, egui::FontId::proportional(16.0), ui.visuals().text_color());
        });
        ctx.request_repaint();
    }

    /// First launch after an install or update: logo, progress and a few steps. Returns true while it is on screen.
    fn splash(&mut self, ctx: &egui::Context, now: f64) -> bool {
        let version = env!("CARGO_PKG_VERSION");
        if self.settings.splash_seen == version { return false; }
        let start = *self.splash_started.get_or_insert(now);
        let t = if crate::theme::reduced_motion() { 1.0 } else { ((now - start) / 2.6).clamp(0.0, 1.0) as f32 };
        let first = self.settings.splash_seen.is_empty();
        let mut skip = false;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill)).show(ctx, |ui| {
            skip = ui.interact(ui.max_rect(), ui.id().with("splash-skip"), egui::Sense::click()).clicked();
            let center = ui.max_rect().center();
            let logo = Rect::from_center_size(center - egui::vec2(0.0, 70.0), egui::vec2(104.0, 104.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(logo), |ui| crate::home::logo(ui, 104.0));
            ui.painter().text(center + egui::vec2(0.0, 12.0), egui::Align2::CENTER_CENTER, "Genix", egui::FontId::proportional(36.0), ui.visuals().strong_text_color());
            let line = if first { tr("Добро пожаловать") } else { tr("Обновление установлено") };
            ui.painter().text(center + egui::vec2(0.0, 46.0), egui::Align2::CENTER_CENTER, format!("{line} · v{version}"), egui::FontId::proportional(14.0), crate::theme::secondary(ui));
            let bar = Rect::from_center_size(center + egui::vec2(0.0, 92.0), egui::vec2(280.0, 6.0));
            ui.painter().rect_filled(bar, 3.0, ui.visuals().extreme_bg_color);
            let eased = 1.0 - (1.0 - t).powi(3);
            ui.painter().rect_filled(Rect::from_min_size(bar.min, egui::vec2(bar.width() * eased, bar.height())), 3.0, crate::theme::accent());
            let step = if t < 0.34 { tr("Готовим рабочее пространство…") } else if t < 0.68 { tr("Загружаем библиотеку моделей…") } else { tr("Почти готово…") };
            ui.painter().text(center + egui::vec2(0.0, 118.0), egui::Align2::CENTER_CENTER, step, egui::FontId::proportional(13.0), crate::theme::secondary(ui));
        });
        if t >= 1.0 || skip {
            self.settings.splash_seen = version.to_string();
            self.settings.save();
            return false;
        }
        ctx.request_repaint();
        true
    }

    /// First launch only: language, theme and units before anything else. Returns true while it is on screen.
    fn first_setup(&mut self, ctx: &egui::Context) -> bool {
        if self.settings.setup_done { return false; }
        let before = self.settings.clone();
        let mut done = false;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(ctx.style().visuals.panel_fill)).show(ctx, |ui| {
            let c = ui.max_rect().center();
            let card = Rect::from_center_size(c, egui::vec2(520.0, 470.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(card), |ui| {
                egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(28).inner_margin(egui::Margin::same(32)).show(ui, |ui| {
                    ui.set_width(card.width() - 64.0);
                    ui.vertical_centered(|ui| { crate::home::logo(ui, 56.0); });
                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| { ui.label(RichText::new(tr("Давайте настроим Genix")).size(24.0).strong()); });
                    ui.vertical_centered(|ui| { ui.label(RichText::new(tr("Это можно изменить позже в настройках")).color(crate::theme::secondary(ui))); });
                    ui.add_space(22.0);
                    crate::widgets::preference_grid(ui, &mut self.settings, "first-setup", true);
                    ui.add_space(26.0);
                    ui.vertical_centered(|ui| { if crate::ui_kit::b01(ui, tr("Продолжить"), 220.0).clicked() { done = true; } });
                });
            });
        });
        if self.settings != before {
            i18n::set_lang(self.settings.lang);
            let theme = resolve_theme(&self.settings, ctx);
            crate::theme::configure(self.settings.accent, theme == Theme::Dark, self.settings.reduce_motion);
            theme.apply(ctx);
            self.theme = theme;
        }
        if done { self.settings.setup_done = true; self.settings.save(); }
        done == false
    }

    fn group_of(&self, i: usize) -> Option<u32> {
        if i == self.active { self.group } else { self.tabs[i].group }
    }

    fn set_group(&mut self, i: usize, g: Option<u32>) {
        if i == self.active { self.group = g } else { self.tabs[i].group = g }
    }

    /// Порядок показа: вкладки одной группы стоят подряд, как в Chrome.
    fn tab_order(&self) -> Vec<usize> {
        let mut order = Vec::new();
        for i in 0..self.tabs.len() {
            match self.group_of(i) {
                Some(g) if order.iter().any(|&k| self.group_of(k) == Some(g)) => {}
                Some(g) => order.extend((0..self.tabs.len()).filter(|&k| self.group_of(k) == Some(g))),
                None => order.push(i),
            }
        }
        order
    }

    fn new_group_for(&mut self, i: usize) {
        let id = self.next_group;
        self.next_group += 1;
        let color = GROUP_COLORS[(id as usize - 1) % GROUP_COLORS.len()];
        self.tab_groups.push(TabGroup { id, name: format!("{} {}", tr("Группа"), id), color, collapsed: false });
        self.set_group(i, Some(id));
    }

    fn tab_bar(&mut self, ui: &mut egui::Ui) {
        enum Act { NewGroup(usize), Join(usize, u32), Leave(usize), Fold(u32), Rename(u32, String), Color(u32, Color32), Ungroup(u32), Name(usize, String), Save(usize), Duplicate(usize) }
        let idle = matches!(self.mode, Mode::Idle);
        let titles: Vec<(String, bool)> = (0..self.tabs.len()).map(|i| self.tab_title(i)).collect();
        let order = self.tab_order();
        let (mut go, mut close, mut add, mut act, mut home) = (None, None, false, None, false);
        ui.add_enabled_ui(idle, |ui| {
            egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if crate::ui_kit::b14(ui, tr("Главная"), self.home_active, None, false).tab.clicked() { home = true; }
                    let mut shown_chip: Option<u32> = None;
                    for &i in &order {
                        let group = self.group_of(i).and_then(|g| self.tab_groups.iter().find(|x| x.id == g).cloned());
                        if let Some(g) = &group {
                            if shown_chip != Some(g.id) {
                                shown_chip = Some(g.id);
                                let count = order.iter().filter(|&&k| self.group_of(k) == Some(g.id)).count();
                                let chip = crate::ui_kit::group_chip(ui, &if g.collapsed { format!("{} · {}", g.name, count) } else { g.name.clone() }, g.color);
                                if chip.clicked() { act = Some(Act::Fold(g.id)); }
                                chip.context_menu(|ui| {
                                    let mut name = g.name.clone();
                                    if ui.add(egui::TextEdit::singleline(&mut name).desired_width(170.0).margin(egui::Margin::symmetric(10, 8))).changed() { act = Some(Act::Rename(g.id, name)); }
                                    ui.horizontal(|ui| {
                                        for c in GROUP_COLORS {
                                            let (rect, r) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::click());
                                            ui.painter().circle_filled(rect.center(), 8.0, c);
                                            if g.color == c { ui.painter().circle_stroke(rect.center(), 10.0, egui::Stroke::new(1.5_f32, ui.visuals().text_color())); }
                                            if r.clicked() { act = Some(Act::Color(g.id, c)); }
                                        }
                                    });
                                    if menu_item(ui, "close", tr("Разгруппировать")) { act = Some(Act::Ungroup(g.id)); ui.close_menu(); }
                                });
                            }
                            if g.collapsed && i != self.active { continue; }
                        }
                        let (name, dirty) = &titles[i];
                        let hit = crate::ui_kit::b14(ui, &format!("{}{}", name, if *dirty { " •" } else { "" }), i == self.active && !self.home_active, group.as_ref().map(|g| g.color), true);
                        if hit.tab.clicked() { go = Some(i); }
                        if hit.tab.middle_clicked() || hit.close { close = Some(i); }
                        hit.tab.context_menu(|ui| {
                            let mut name = if i == self.active { self.suggested.clone() } else { self.tabs[i].suggested.clone() }.unwrap_or_else(|| titles[i].0.clone());
                            ui.label(RichText::new(tr("НАЗВАНИЕ")).small().weak());
                            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(170.0).margin(egui::Margin::symmetric(10, 8))).changed() { act = Some(Act::Name(i, name)); }
                            ui.separator();
                            if menu_item(ui, "file_save", tr("Сохранить")) { act = Some(Act::Save(i)); ui.close_menu(); }
                            if menu_item(ui, "add", tr("Дублировать")) { act = Some(Act::Duplicate(i)); ui.close_menu(); }
                            ui.separator();
                            if menu_item(ui, "add", tr("В новую группу")) { act = Some(Act::NewGroup(i)); ui.close_menu(); }
                            for g in &self.tab_groups {
                                if self.group_of(i) != Some(g.id) && menu_item(ui, "tree_folder", &format!("{} «{}»", tr("В группу"), g.name)) { act = Some(Act::Join(i, g.id)); ui.close_menu(); }
                            }
                            if self.group_of(i).is_some() && menu_item(ui, "tree_folder_open", tr("Убрать из группы")) { act = Some(Act::Leave(i)); ui.close_menu(); }
                            if menu_item(ui, "close", tr("Закрыть вкладку")) { close = Some(i); ui.close_menu(); }
                        });
                    }
                    if ui.add(egui::Button::new(RichText::new("+").size(17.0)).frame(false).min_size(egui::vec2(26.0, 26.0))).on_hover_text(tr("Новая вкладка")).clicked() {
                        add = true;
                    }
                });
            });
        });
        match act {
            Some(Act::NewGroup(i)) => self.new_group_for(i),
            Some(Act::Join(i, g)) => self.set_group(i, Some(g)),
            Some(Act::Leave(i)) => self.set_group(i, None),
            Some(Act::Fold(id)) => if let Some(g) = self.tab_groups.iter_mut().find(|g| g.id == id) { g.collapsed = !g.collapsed },
            Some(Act::Rename(id, name)) => if let Some(g) = self.tab_groups.iter_mut().find(|g| g.id == id) { g.name = name },
            Some(Act::Color(id, c)) => if let Some(g) = self.tab_groups.iter_mut().find(|g| g.id == id) { g.color = c },
            Some(Act::Ungroup(id)) => (0..self.tabs.len()).for_each(|i| if self.group_of(i) == Some(id) { self.set_group(i, None) }),
            Some(Act::Name(i, name)) => {
                let name = if name.trim().is_empty() { None } else { Some(name) };
                if i == self.active { self.suggested = name; } else { self.tabs[i].suggested = name; }
            }
            Some(Act::Save(i)) => {
                self.home_active = false;
                self.switch_tab(i);
                if i == self.active { self.save(false, self.now); }
            }
            Some(Act::Duplicate(i)) => {
                self.home_active = false;
                self.switch_tab(i);
                if i == self.active {
                    let (doc, name) = (self.doc.clone(), self.tab_title(i).0);
                    self.open_in_tab(doc, None);
                    self.suggested = Some(format!("{} {}", name, tr("копия")));
                }
            }
            None => {}
        }
        let used: Vec<Option<u32>> = (0..self.tabs.len()).map(|i| self.group_of(i)).collect();
        self.tab_groups.retain(|g| used.contains(&Some(g.id)));
        if home {
            self.home_active = true;
            self.home.navigate(crate::home::HomePage::Models);
            let list = self.project_list();
            self.home.refresh(&list);
        }
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
            Confirm::New => { self.set_busy(tr("Подготовка проекта…")); self.new_blank_tab() }
            Confirm::Open => self.open_dialog(),
            Confirm::OpenExample(f) => self.open_in_tab(f(), None),
            Confirm::CloseTab => self.close_tab_now(),
            Confirm::Quit(mut pending) => {
                if let Some(i) = pending.pop() {
                    self.switch_tab(i);
                    self.home_active = false;
                    self.confirm = Some(Confirm::Quit(pending));
                } else {
                    self.allow_close = true;
                }
            }
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
        // a feature that moves faces (put on a plane, press-pull) picks its face on the body as it was before the feature,
        // otherwise a second pick lands on the already moved face and the plane comes out wrong
        if let Mode::Command(c) = &self.mode {
            let moves = self.doc.feature(c.uid).is_some_and(|f| match f.kind {
                FeatureKind::PlaceOnPlane { .. } | FeatureKind::PressPull { .. } => matches!(c.pick, PickTarget::Face | PickTarget::AlignTarget),
                _ => false,
            });
            if moves {
                if let Some(i) = self.doc.index_of(c.uid) {
                    return self.ev.model_before(i);
                }
            }
        }
        self.ev.model()
    }

    /// The model that clicks and the pointer hit-test against. An extrude shows its result (so growing it can be watched),
    /// but a face for it is picked on the body as it was before the extrude: the default profile already extrudes the shown one.
    /// Bodies listed in the tree: a command in progress has no body of its own there yet.
    fn tree_model(&self) -> Arc<Model> {
        if let Mode::Command(c) = &self.mode {
            if let Some(i) = self.doc.index_of(c.uid) {
                return self.ev.model_before(i);
            }
        }
        self.shown()
    }

    fn pick_model(&self) -> Arc<Model> {
        if let Mode::Command(c) = &self.mode {
            if c.pick == PickTarget::Profiles && self.doc.feature(c.uid).is_some_and(|f| matches!(f.kind, FeatureKind::Extrude { .. })) {
                if let Some(i) = self.doc.index_of(c.uid) {
                    return self.ev.model_before(i);
                }
            }
        }
        self.shown()
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
                // an existing put-on-plane / press-pull already has its face: show the result, re-pick on request
                let pick = if matches!(kind, FeatureKind::PlaceOnPlane { .. } | FeatureKind::PressPull { .. }) { PickTarget::None } else { Command::pick_for(&kind) };
                self.mode = Mode::Command(Command { uid, before: self.doc.clone(), pick, auto_op: false });
            }
        }
    }

    fn start_command(&mut self, kind: &str, now: f64) {
        crate::crash::note(format!("command {kind}"));
        self.cancel_mode();
        let model = self.ev.model();
        // как в Fusion: если в последнем видимом чертеже ровно один контур, он выбран сразу; иначе ждём щелчка
        let last_sketch = self
            .doc
            .features
            .iter()
            .take(self.doc.rollback)
            .rev()
            .filter(|f| matches!(f.kind, FeatureKind::Sketch { .. }) && self.doc.visible_sketches.contains(&f.uid))
            .find_map(|f| self.ev.sketch(f.uid).map(|s| (f.uid, s)))
            .filter(|(_, s)| s.regions.len() == 1)
            .map(|(uid, s)| (uid, vec![ProfileRef::of(&s.regions[0])]));
        let has_sketch = self.doc.features.iter().take(self.doc.rollback).any(|f| matches!(f.kind, FeatureKind::Sketch { .. }));
        // выдавить можно и грань тела, без чертежа
        if matches!(kind, "revolve" | "sweep" | "loft" | "pipe") && !has_sketch || kind == "extrude" && !has_sketch && model.bodies.is_empty() {
            self.toast(tr("Сначала создайте чертёж (кнопка «Новый чертёж»)"), true, now);
            return;
        }
        if matches!(kind, "hole" | "press_pull" | "place" | "fillet" | "chamfer" | "scale" | "move" | "mirror" | "pattern" | "combine" | "remove") && model.bodies.is_empty() {
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
            top_face,
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
            || matches!(&self.mode, Mode::Command(c) if c.pick == PickTarget::AlignTarget)
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
                let all = matches!(kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. } | FeatureKind::Loft { .. } | FeatureKind::Sweep { .. } | FeatureKind::Pipe { .. });
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

    /// Чертёж, линия которого ближе всего к лучу щелчка (для пути трубы).
    fn nearest_sketch_line(&self, (o, d): (DVec3, DVec3)) -> Option<u64> {
        let tol = self.cam.world_per_pixel(self.viewport) * 10.0;
        let mut best: Option<(f64, u64)> = None;
        for uid in self.sketches_to_show() {
            let Some(sk) = self.ev.sketch(uid) else { continue };
            for i in 0..sk.sketch.entities.len() {
                let pts: Vec<DVec3> = sk.sketch.tessellate(i).iter().map(|p| sk.plane.to_world(*p)).collect();
                for w in pts.windows(2) {
                    let dist = ray_segment_distance(o, d, w[0], w[1]);
                    if dist < tol && best.map_or(true, |(b, _)| dist < b) {
                        best = Some((dist, uid));
                    }
                }
            }
        }
        best.map(|(_, u)| u)
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

    fn hole_target(&self, model: &Model, slot: usize, tri: usize, point: DVec3, rect: Rect) -> Option<(Plane,[f64;2],[f64;2],[f64;2],[bool;2])> {
        let (plane,tris)=self.face_plane(model,slot,tri)?;
        let mesh=model.bodies.get(slot)?.mesh();
        let mut lo=[f64::INFINITY;2]; let mut hi=[f64::NEG_INFINITY;2];
        for t in tris {for v in mesh.triangles[t] {let q=plane.to_local(DVec3::from(mesh.vertices[v as usize])); for a in 0..2 {lo[a]=lo[a].min(q[a]);hi[a]=hi[a].max(q[a]);}}}
        let mut at=plane.to_local(point); let mut snapped=[false;2];
        let tolerance=self.cam.world_per_pixel(rect)*9.0;
        for a in 0..2 {let center=(lo[a]+hi[a])*0.5; if (at[a]-center).abs()<=tolerance {at[a]=center;snapped[a]=true;}}
        Some((plane,at,lo,hi,snapped))
    }

    /// Плоские грани тела (кэш по телу: считаются один раз).
    fn face_groups(&self, b: &fusion_core::doc::Body) -> Arc<Vec<u32>> {
        let key = Arc::as_ptr(&b.solid) as usize;
        let mut cache = self.face_groups.borrow_mut();
        if cache.len() > 512 {
            cache.clear();
        }
        // в записи держим само тело: пока оно живо, его адрес не достанется новому телу
        match cache.get(&key) {
            Some((solid, v)) if Arc::ptr_eq(solid, &b.solid) => v.clone(),
            _ => {
                let v = Arc::new(b.mesh().planar_faces());
                cache.insert(key, (b.solid.clone(), v.clone()));
                v
            }
        }
    }

    fn handle_click(&mut self, hit: Option<Hit>, shift: bool, now: f64) {
        // slots in `hit` index the model that was hit-tested
        let model = self.pick_model();
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
                let extrudes = matches!(self.doc.feature(uid).map(|f| &f.kind), Some(FeatureKind::Extrude { .. }));
                let face = match (pick, hit) {
                    (PickTarget::Face | PickTarget::AlignTarget, Some(Hit::Body { slot, tri, t })) => {
                        let (o,d)=self.last_ray;
                        let is_hole=self.doc.feature(uid).is_some_and(|f|matches!(f.kind,FeatureKind::Hole{..}));
                        if is_hole {self.hole_target(&model,slot,tri,o+d*t,self.viewport).map(|(p,a,_,_,_)|(p,a))}
                        else {self.face_plane(&model,slot,tri).map(|(p,_)|(p,p.to_local(o+d*t)))}
                    },
                    (PickTarget::Profiles, Some(Hit::Body { slot, tri, t })) if extrudes => {
                        let (o,d)=self.last_ray;
                        self.face_plane(&model,slot,tri).map(|(p,_)|(p,p.to_local(o+d*t)))
                    },
                    _ => None,
                };
                let ray = self.last_ray;
                let face_pick = face;
                let hit_body = match hit { Some(Hit::Body { slot, .. }) => model.bodies.get(slot).map(|b| b.id), _ => None };
                let edge = self.hover_edge;
                // one click takes the whole tangent chain (a straight rim and the rounded corners it runs into), as in Fusion
                let chain: Vec<EdgeRef> = match edge {
                    Some((e, owner)) => {
                        let all = model.body(owner).map(|b| self.straight_edges(b));
                        all.map(|all| crate::edges::tangent_chain(&all, &e)).unwrap_or_else(|| vec![e])
                    }
                    None => Vec::new(),
                };
                let path_pick = if pick == PickTarget::Path { self.nearest_sketch_line(self.last_ray) } else { None };
                let user_planes = self.doc.offset_planes();
                let Some(f) = self.doc.feature_mut(uid) else { return };
                let mut changed = false;
                let mut face_done = false;
                let mut next_pick = None;
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
                    (FeatureKind::PlaceOnPlane { body, face, seed, .. }, Some(Hit::Body { t, .. }), PickTarget::Face) => {
                        if let (Some((pl, _)), Some(id)) = (face_pick, hit_body) {
                            *face = pl;
                            *seed = (ray.0 + ray.1 * t).to_array();
                            *body = id;
                            changed = true;
                            next_pick = Some(PickTarget::AlignTarget);
                        }
                    }
                    (FeatureKind::PlaceOnPlane { body, target, to, .. }, h, PickTarget::AlignTarget) => {
                        match h {
                            // the target face's outward normal: the moved face rests against it from outside
                            Some(Hit::Body { t, .. }) if hit_body.is_some_and(|b| b != *body) => {
                                if let Some((pl, _)) = face_pick {
                                    *target = pl;
                                    *to = Some((ray.0 + ray.1 * t).to_array());
                                    changed = true;
                                    face_done = true;
                                }
                            }
                            Some(Hit::OriginPlane(i, _)) => {
                                *target = ORIGIN_PLANES[i];
                                *to = None;
                                changed = true;
                                face_done = true;
                            }
                            Some(Hit::UserPlane(i, _)) => {
                                if let Some((_, _, pl)) = user_planes.get(i) {
                                    *target = *pl;
                                    *to = None;
                                    changed = true;
                                    face_done = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    (FeatureKind::PressPull { plane, seed, .. }, Some(Hit::Body { t, .. }), PickTarget::Face) => {
                        if let Some((pl, _)) = face {
                            *plane = pl;
                            *seed = (ray.0 + ray.1 * t).to_array();
                            changed = true;
                            face_done = true;
                        }
                    }
                    (FeatureKind::Hole { plane, at, .. }, _, PickTarget::Face) => {
                        if let Some((pl, loc)) = face {
                            *plane = pl;
                            at[0] = Expr::num(loc[0]);
                            at[1] = Expr::num(loc[1]);
                            changed = true;
                        }
                    }
                    (FeatureKind::Chamfer { body, edges, .. } | FeatureKind::Fillet { body, edges, .. }, _, PickTarget::Edges) => {
                        // the first picked edge chooses the body; later edges come only from it
                        if let Some((e, owner)) = edge.filter(|(_, owner)| *body == 0 || *body == *owner) {
                            *body = owner;
                            let same = |x: &EdgeRef, y: &EdgeRef| x.circle == y.circle && (DVec3::from(x.a) - DVec3::from(y.a)).length() < 1e-6 && (DVec3::from(x.b) - DVec3::from(y.b)).length() < 1e-6;
                            // a second click on a selected edge lets the whole chain go again
                            if edges.iter().any(|x| same(x, &e)) {
                                edges.retain(|x| !chain.iter().any(|c| same(x, c)));
                            } else {
                                for c in &chain {
                                    if !edges.iter().any(|x| same(x, c)) {
                                        edges.push(*c);
                                    }
                                }
                            }
                            changed = true;
                        }
                    }
                    (FeatureKind::Pipe { path, .. }, _, PickTarget::Path) => {
                        if let Some(u) = path_pick {
                            *path = u;
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
                    (FeatureKind::Extrude { face, profiles, .. }, Some(Hit::Body { t, .. }), PickTarget::Profiles) => {
                        if let Some((pl, _)) = face_pick {
                            *face = Some(fusion_core::doc::FaceRef { plane: pl, seed: (ray.0 + ray.1 * t).to_array() });
                            profiles.clear();
                            changed = true;
                        }
                    }
                    (FeatureKind::Extrude { face, sketch, profiles, .. }, Some(Hit::Profile { sketch: s, region, .. }), _) => {
                        *face = None;
                        if *sketch != s {
                            *sketch = s;
                            profiles.clear();
                        }
                        let regions = self.ev.sketch(s).map(|r| r.regions.clone()).unwrap_or_default();
                        if let Some(i) = profiles.iter().position(|p| p.resolve(&regions) == Some(region)) {
                            profiles.remove(i);
                        } else if let Some(r) = regions.get(region) {
                            profiles.push(ProfileRef::of(r));
                        }
                        changed = true;
                    }
                    (FeatureKind::Revolve { sketch, profiles, .. } | FeatureKind::Sweep { sketch, profiles, .. }, Some(Hit::Profile { sketch: s, region, .. }), _) => {
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
                if face_done || next_pick.is_some() {
                    if let Mode::Command(c) = &mut self.mode { c.pick = next_pick.unwrap_or(PickTarget::None); }
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
        // Compact menu highlights; rounded pills are reserved for larger controls.
        let visuals = &mut ui.style_mut().visuals;
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
        visuals.widgets.open.corner_radius = egui::CornerRadius::same(6);
        let start = ui.cursor().left();
        let has_doc = !self.home_active;
        let has_model = has_doc && !self.ev.model().bodies.is_empty();
        egui::menu::bar(ui, |ui| {
            crate::home::logo(ui, 18.0);
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
                if menu_item(ui, "tree_folder", tr("Новая папка…")) {
                    ui.close_menu();
                    self.commit_pending_edits();
                    self.home_active = true;
                    self.home.open_name_dialog(true);
                }
                if menu_item(ui, "add", tr("Новая модель…")) {
                    ui.close_menu();
                    self.commit_pending_edits();
                    self.home_active = true;
                    self.home.open_name_dialog(false);
                }
                ui.separator();
                if ui.add_enabled_ui(has_doc, |ui| menu_item(ui, "file_save", tr("Сохранить           ⌘S"))).inner {
                    ui.close_menu();
                    self.save(false, now);
                }
                if ui.add_enabled_ui(has_doc, |ui| menu_item(ui, "file_saveas", tr("Сохранить как…    ⇧⌘S"))).inner {
                    ui.close_menu();
                    self.save(true, now);
                }
                let versions = self.path.as_ref().zip(crate::files::versions_base()).map(|(p, base)| crate::files::list_versions(&base, p)).unwrap_or_default();
                ui.add_enabled_ui(has_doc && !versions.is_empty(), |ui| {
                    ui.menu_button(tr("Версии файла"), |ui| {
                        for (stamp, file) in &versions {
                            if menu_item(ui, "tl_prev", &crate::home::format_time(*stamp)) {
                                ui.close_menu();
                                self.open_version(file, *stamp, now);
                            }
                        }
                    });
                });
                ui.separator();
                if ui.add_enabled_ui(has_model, |ui| menu_item(ui, "file_export_stl", tr("Экспорт STL (3D-печать)…"))).inner {
                    ui.close_menu();
                    self.export("stl", now);
                }
                if ui.add_enabled_ui(has_model, |ui| menu_item(ui, "sk_photo", tr("Снимок модели PNG без фона…"))).inner {
                    ui.close_menu();
                    self.export_png(now);
                }
                if ui.add_enabled_ui(has_model, |ui| menu_item(ui, "file_export_3mf", tr("Экспорт 3MF (3D-печать)…"))).inner {
                    ui.close_menu();
                    self.export("3mf", now);
                }
                if ui.add_enabled_ui(has_model, |ui| menu_item(ui, "file_export_obj", tr("Экспорт OBJ…"))).inner {
                    ui.close_menu();
                    self.export("obj", now);
                }
                ui.add_enabled_ui(has_model, |ui| {
                    ui.menu_button(tr("Открыть в слайсере"), |ui| {
                        for (i, slicer) in crate::slicers::SLICERS.iter().enumerate() {
                            let installed = crate::slicers::installed(slicer).is_some();
                            let label = if installed { slicer.name.to_string() } else { format!("{} — {}", slicer.name, tr("не установлен")) };
                            if ui.add_enabled_ui(installed, |ui| menu_item(ui, "file_export_stl", &label)).inner {
                                ui.close_menu();
                                self.open_in_slicer(i, now);
                            }
                        }
                    });
                });
            });
            ui.menu_button(tr("Правка"), |ui| {
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(tr("Отменить        ⌘Z"))).clicked() {
                    ui.close_menu();
                    self.do_undo();
                }
                if ui.add_enabled(!self.redo.is_empty(), egui::Button::new(tr("Повторить     ⌘Y"))).clicked() {
                    ui.close_menu();
                    self.do_redo();
                }
                ui.separator();
                if ui.add_enabled(has_doc, egui::Button::new(tr("Параметры…"))).clicked() {
                    ui.close_menu();
                    self.params_open = true;
                }
                ui.separator();
                if menu_item(ui, "settings", tr("Настройки…")) {
                    ui.close_menu();
                    self.commit_pending_edits();
                    self.home_active = true;
                    self.home.navigate(crate::home::HomePage::Settings);
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
                crate::widgets::checkbox(ui, &mut self.settings.show_left_panel, tr("Левая панель"));
                crate::widgets::checkbox(ui, &mut self.settings.show_right_panel, tr("Правая панель"));
                ui.separator();
                crate::widgets::checkbox(ui, &mut self.settings.show_grid, tr("Сетка"));
                crate::widgets::checkbox(ui, &mut self.show_planes, tr("Плоскости начала координат"));
                if ui.add_enabled(has_model, egui::Button::new(tr("Вписать модель           F"))).clicked() {
                    ui.close_menu();
                    self.need_fit = true;
                }
            });
            ui.menu_button(tr("Справка"), |ui| {
                if ui.button(tr("Управление и горячие клавиши")).clicked() {
                    ui.close_menu();
                    self.help_open = true;
                }
                if ui.button(tr("Сообщить о проблеме…")).clicked() {
                    ui.close_menu();
                    self.report.show(false);
                }
            });
            ui.separator();
            // in a sketch the same two buttons step through the sketch's own history (what was drawn), not the model's
            let editing = !self.home_active;
            let small = egui::vec2(28.0, 26.0);
            let (can_undo, can_redo) = match &self.mode {
                Mode::Sketch(s) => (s.can_undo(), s.can_redo()),
                _ => (editing && !self.undo.is_empty(), editing && !self.redo.is_empty()),
            };
            if ui.add_enabled_ui(can_undo, |ui| icon_button(ui, "undo", "↶", false, tr("Отменить (⌘Z)"), small)).inner {
                let params = self.doc.params.clone();
                if let Mode::Sketch(s) = &mut self.mode { s.undo(&params); } else { self.do_undo(); }
            }
            if ui.add_enabled_ui(can_redo, |ui| icon_button(ui, "redo", "↷", false, tr("Повторить (⌘Y)"), small)).inner {
                let params = self.doc.params.clone();
                if let Mode::Sketch(s) = &mut self.mode { s.redo(&params); } else { self.do_redo(); }
            }
            // egui's menu bar fills the row; remember how much the menus really use so the tabs keep the rest
            let used = ui.cursor().left() - start;
            ui.data_mut(|d| d.insert_temp(egui::Id::new("menu-width"), used));
        });
    }

    fn header_bar(&mut self, ui: &mut egui::Ui, now: f64) {
        ui.horizontal_centered(|ui| {
            // one row height for menus, undo/redo, tabs and «+», so everything shares the same centre line
            ui.spacing_mut().interact_size.y = 26.0;
            // the measured width ends with one item gap after «redo»; drop it so both sides of the arrows match
            let width = ui.data(|d| d.get_temp::<f32>(egui::Id::new("menu-width"))).unwrap_or(330.0) - ui.spacing().item_spacing.x;
            ui.allocate_ui_with_layout(egui::vec2(width, 28.0), egui::Layout::left_to_right(egui::Align::Center), |ui| self.menu_bar(ui, now));
            // the header row's own gap is 3 px wider than the menu bar's, which pushed the arrows off centre
            ui.add_space(-3.0);
            ui.separator();
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 28.0), egui::Layout::left_to_right(egui::Align::Center), |ui| self.tab_bar(ui));
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, now: f64) {
        if let Mode::Sketch(s) = &mut self.mode {
            let full = ui.available_rect_before_wrap();
            // room for «Отмена» + «Завершить чертёж» + margins; the tools scroll in what is left, so nothing hides under the buttons
            let split = full.max.x - 310.0_f32.min(full.width() * 0.45);
            // fixed row: the scroll area would otherwise report the whole panel height and the bar would grow
            let mut tools = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(full.min, egui::pos2(split, full.min.y + 92.0))));
            let row = 92.0;
            egui::ScrollArea::horizontal().auto_shrink([false, true]).max_height(row).show(&mut tools, |ui| s.toolbar(ui));
            let mut done = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(egui::pos2(split, full.min.y), egui::pos2(full.max.x - 20.0, full.min.y + row))).layout(egui::Layout::right_to_left(egui::Align::Center)));
            done.spacing_mut().item_spacing.x = 14.0;
            let act = s.actions(&mut done);
            ui.allocate_rect(Rect::from_min_max(full.min, egui::pos2(full.max.x, full.min.y + row)), egui::Sense::hover());
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
                tree_row(ui, false, |ui| { ui.label(tr("Единицы: мм, г")); });
            });
            tree_group(ui, "t_views", "tree_views", tf!("Именованные виды ({})", self.doc.views.len()), false, |ui| {
                let mut go = None;
                let mut del = None;
                for (i, v) in self.doc.views.iter().enumerate() {
                    tree_row(ui, false, |ui| {
                        if ui.selectable_label(false, format!("▸ {}", v.name)).on_hover_text(tr("Перейти к виду")).clicked() {
                            go = Some(i);
                        }
                        if ui.small_button("×").clicked() {
                            del = Some(i);
                        }
                    });
                }
                tree_row(ui, false, |ui| {
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
                tree_row(ui, false, |ui| { crate::widgets::checkbox(ui, &mut self.show_planes, tr("Показывать плоскости")); });
                tree_row(ui, false, |ui| { crate::widgets::checkbox(ui, &mut self.settings.show_grid, tr("Показывать сетку")); });
            });
            let model = self.tree_model();
            tree_group(ui, "t_bodies", "tree_bodies", tf!("Тела ({})", model.bodies.len()), true, |ui| {
                if model.bodies.is_empty() {
                    tree_row(ui, false, |ui| { ui.label(RichText::new(tr("Пока нет тел")).weak()); });
                }
                for b in &model.bodies {
                    tree_row(ui, self.selection.contains(&b.id), |ui| {
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
                            if menu_item(ui, "rename", tr("Переименовать")) {
                                self.rename_body = Some((b.id, self.doc.body_name(b).to_string()));
                                ui.close_menu();
                            }
                            if menu_item(ui, "delete", tr("Удалить тело")) {
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
                    tree_row(ui, false, |ui| {
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
                        let r = ui.selectable_label(self.tree_sketch == Some(uid), text).on_hover_text(tr("Двойной щелчок — изменить, Delete — удалить"));
                        if r.clicked() {
                            self.tree_sketch = Some(uid);
                        }
                        if r.double_clicked() {
                            self.edit_feature(uid, now);
                        }
                        r.context_menu(|ui| {
                            if ui.button(tr("Изменить")).clicked() {
                                ui.close_menu();
                                self.edit_feature(uid, now);
                            }
                            if ui.button(RichText::new(tr("Удалить")).color(Color32::from_rgb(200, 40, 40))).clicked() {
                                ui.close_menu();
                                self.delete_feature(uid, now);
                            }
                        });
                    });
                }
            });
            tree_group(ui, "t_params", "tree_params", tf!("Параметры ({})", self.doc.params.items.len()), false, |ui| {
                for (k, v) in &self.doc.params.items {
                    let val = self.doc.params.eval(&v.0).map(format_num).unwrap_or_else(|_| "?".into());
                    tree_row(ui, false, |ui| { ui.label(format!("{k} = {val}")); });
                }
                tree_row(ui, false, |ui| {
                    if ui.button(tr("Изменить…")).clicked() {
                        self.params_open = true;
                    }
                });
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
                        let editing = matches!(&self.mode, Mode::Command(c) if c.uid == uid) || matches!(&self.mode, Mode::Sketch(s) if s.uid == Some(uid));
                        let waiting = editing && self.doc.feature(uid).is_some_and(|f| commands::awaiting_pick(&f.kind));
                        let err = self.ev.error(uid).filter(|_| !waiting).map(|s| s.to_string());
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
                        // the step being edited is filled with the accent: its text must be the light "on accent" colour
                        if editing { text = text.color(crate::theme::on_accent()); }
                        if suppressed {
                            text = text.strikethrough().weak();
                        }
                        if !active {
                            text = text.weak();
                        }
                        let mut btn = egui::Button::new(text).corner_radius(12);
                        if editing { btn = btn.fill(crate::theme::accent()); }
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
                            if menu_item(ui, "edit", tr("Изменить")) {
                                ui.close_menu();
                                self.edit_feature(uid, now);
                            }
                            if menu_item(ui, "rename", tr("Переименовать")) {
                                ui.close_menu();
                                self.rename_feature = Some((uid, name.clone()));
                            }
                            if menu_item(ui, "rollback", tr("Откатить историю сюда")) {
                                ui.close_menu();
                                self.cancel_mode();
                                self.doc.rollback = i + 1;
                                self.after_doc_change();
                            }
                            if menu_item(ui, "suppress", if suppressed { tr("Включить") } else { tr("Отключить (подавить)") }) {
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
        crate::keys::window(ctx, &mut open);
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
        let selected_ids = self.selection.clone();
        let mut selected_material = selected_ids.first().and_then(|id| self.doc.body_materials.get(id)).copied().unwrap_or(self.settings.material);
        let mut material_changed = false;
        egui::Grid::new("inspector").num_columns(2).show(ui, |ui| {
            ui.label(tr("Выбрано тел"));
            ui.label(format!("{}", sel.len()));
            ui.end_row();
            ui.label(tr("Габариты"));
            ui.label(format!("{} × {} × {} {}", crate::units::number(crate::units::to_display(size.x)), crate::units::number(crate::units::to_display(size.y)), crate::units::number(crate::units::to_display(size.z)), crate::units::label()));
            ui.end_row();
            ui.label(tr("Объём"));
            ui.label(tf!("{} см³", r1(vol / 1000.0)));
            ui.end_row();
            ui.label(tr("Площадь"));
            ui.label(tf!("{} см²", r1(area / 100.0)));
            ui.end_row();
            ui.label(tr("Материал"));
            egui::ComboBox::from_id_salt("material").selected_text(tr(Materials::get(selected_material).0)).show_ui(ui, |ui| {
                for (i, (n, _, _, _)) in Materials::LIST.iter().enumerate() {
                    material_changed |= ui.selectable_value(&mut selected_material, i, tr(n)).changed();
                }
            });
            ui.end_row();
            ui.label(tr("Масса"));
            ui.label(tf!("{} г", r1(vol / 1000.0 * Materials::get(selected_material).1)));
            ui.end_row();
        });
        if material_changed {
            self.checkpoint();
            for id in &selected_ids { self.doc.body_materials.insert(*id, selected_material); }
            self.settings.material = selected_material;
            self.settings.save();
            self.after_doc_change();
        }
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
        if !resp.hovered() {
            // zoom still owed from a scroll must not fire later, when the pointer comes back
            self.zoom_pending = 0.0;
        }
        if resp.hovered() {
            let cursor = resp.hover_pos();
            let mut zoom = 1.0f64;
            let mut wheel = 0.0f64;
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
                                    // every notch counts at least as one full step of the chosen size, however small the reported delta
                                    let ticks = if delta.y == 0.0 { 0.0 } else { delta.y.signum() as f64 * (delta.y.abs() as f64).max(1.0) };
                                    wheel += 0.88f64.ln() * ticks * zk;
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
            // the wheel's zoom is not applied at once: it is spent over the next frames, so it glides
            self.zoom_pending += wheel;
            if cursor.is_some() {
                self.zoom_cursor = cursor;
            }
            if self.zoom_pending.abs() > 1e-4 {
                let step = if crate::theme::reduced_motion() { self.zoom_pending } else { self.zoom_pending * (1.0 - (-ctx.input(|i| i.stable_dt) as f64 * 14.0).exp()) };
                self.zoom_pending -= step;
                zoom *= step.exp();
                ctx.request_repaint();
            } else {
                self.zoom_pending = 0.0;
            }
            if zoom != 1.0 {
                self.cam.zoom_at(zoom, rect, self.zoom_cursor.or(cursor));
                let (center, radius) = self.scene_sphere();
                self.cam.keep_near(center, radius);
            }
            // два пальца на сенсорном экране (iPad/iPhone) — сдвиг вида
            if let Some(mt) = ctx.multi_touch() {
                if mt.num_touches >= 2 && mt.translation_delta != egui::Vec2::ZERO {
                    self.cam.pan(mt.translation_delta.x as f64, mt.translation_delta.y as f64, rect);
                }
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
                self.hover = if manip_active { None } else { let pick = self.pick_model(); self.hit_test(&pick, o, dir) };
            }
        }
        let (center, radius) = self.scene_sphere();
        self.cam.set_scene(center, radius);
        self.align_hover_preview();
        // ребро под курсором (фаска и скругление)
        self.hover_edge = None;
        if matches!(&self.mode, Mode::Command(c) if c.pick == PickTarget::Edges) && !resp.dragged() {
            if let Some(m) = resp.hover_pos() {
                self.hover_edge = self.pick_edge(&model, rect, m);
            }
        }
        if picking && resp.clicked() {
            self.handle_click(self.hover, shift, now);
            // the click changes the selection after this frame was drawn: draw again at once, so highlights do not wait for the next mouse move
            ui.ctx().request_repaint();
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
                    if menu_item(ui, "hide", tr("Скрыть")) {
                        self.doc.hidden_bodies.insert(id);
                        self.selection.retain(|x| *x != id);
                        ui.close_menu();
                    }
                    if menu_item(ui, "move", tr("Переместить…")) {
                        self.selection = vec![id];
                        ui.close_menu();
                        self.start_command("move", now);
                    }
                    if menu_item(ui, "delete", tr("Удалить")) {
                        ui.close_menu();
                        self.delete_bodies(vec![id]);
                    }
                    ui.separator();
                }
                if menu_item(ui, "sketch", tr("Новый чертёж")) {
                    ui.close_menu();
                    self.start_sketch();
                }
                for (icon, label, kind) in [("extrude", "Выдавить", "extrude"), ("hole", "Отверстие", "hole"), ("fillet", "Скругление", "fillet"), ("chamfer", "Фаска", "chamfer"), ("press_pull", "Вытянуть грань", "press_pull"), ("align", "Наложить", "place")] {
                    if menu_item(ui, icon, tr(label)) {
                        ui.close_menu();
                        self.start_command(kind, now);
                    }
                }
                ui.separator();
                if menu_item(ui, "nav_fit", tr("Вписать модель")) {
                    ui.close_menu();
                    self.need_fit = true;
                }
                if !self.doc.hidden_bodies.is_empty() && menu_item(ui, "show", tr("Показать все тела")) {
                    self.doc.hidden_bodies.clear();
                    ui.close_menu();
                }
            });
        } else if matches!(self.mode, Mode::Sketch(_)) {
            resp.context_menu(|ui| {
                use crate::sketcher::Tool;
                for (icon, label, tool) in [("sk_line", "Линия", Tool::Line), ("sk_rect", "Прямоугольник", Tool::Rect), ("sk_circle", "Окружность", Tool::Circle), ("sk_dim", "Размер", Tool::Dimension)] {
                    if menu_item(ui, icon, tr(label)) {
                        if let Mode::Sketch(s) = &mut self.mode { s.set_tool(tool); }
                        ui.close_menu();
                    }
                }
                ui.separator();
                if menu_item(ui, "sk_finish", tr("Завершить чертёж")) {
                    ui.close_menu();
                    self.finish_sketch(true);
                }
                if menu_item(ui, "close", tr("Отмена")) {
                    ui.close_menu();
                    self.finish_sketch(false);
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
        let ew_bits = self.settings.edge_width.to_bits();
        if !self.shown_model.as_ref().is_some_and(|m| Arc::ptr_eq(m, &model)) || ew_bits != self.shown_edge_w {
            self.shown_model = Some(model.clone());
            self.shown_edge_w = ew_bits;
            self.upload = Arc::new(self.cache.assemble(&model.bodies, self.settings.edge_width));
            self.upload_gen += 1;
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
            let material = Materials::get(self.doc.body_materials.get(&b.id).copied().unwrap_or(self.settings.material));
            let c = if self.doc.hidden_bodies.contains(&b.id) {
                [0.0; 4]
            } else if self.doc.cutter_bodies.contains(&b.id) {
                let mut c = linear(Color32::from_rgb(82, 205, 232));
                c[3] = -0.32;
                c
            } else if self.selection.contains(&b.id) || cmd_bodies.contains(&b.id) {
                let mut c = linear(colors.body_selected); c[3] = material.3; c
            } else if hover_slot == Some(slot) {
                let mut c = linear(colors.body_hover); c[3] = material.3; c
            } else {
                let mut c = linear(material.2); c[3] = material.3; c
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
            glass: {
                let ppp = ui.ctx().pixels_per_point();
                [rect.min.x * ppp, rect.min.y * ppp, rect.width() * ppp, rect.height() * ppp]
            },
        };
        let cb = ViewCallback { frame, colors: body_colors, overlay, upload: self.upload.clone(), upload_gen: self.upload_gen, style: self.settings.visual_style.parts() };
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
                if matches!(self.mode, Mode::PickPlane) {
                    fill_quad(&mut ov.tris_top, pl, [lo, lo], [hi, hi], col);
                } else {
                    fill_quad(&mut ov.tris, pl, [lo, lo], [hi, hi], col);
                }
                let corners = [[lo, lo], [hi, lo], [hi, hi], [lo, hi]].map(|q| pl.to_world(q));
                let line_color = if hov { c.plane_hover.to_opaque() } else { base.to_opaque() };
                if matches!(self.mode, Mode::PickPlane) {
                    polyline(&mut ov.lines_top, &corners, line_color, 2.0, true);
                } else {
                    polyline(&mut ov.lines, &corners, line_color, 1.2, true);
                }
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
            if matches!(self.mode, Mode::PickPlane) { fill_quad(&mut ov.tris_top, pl, a, b, col); }
            else { fill_quad(&mut ov.tris, pl, a, b, col); }
            let corners = [[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]].map(|q| pl.to_world(q));
            if matches!(self.mode, Mode::PickPlane) { polyline(&mut ov.lines_top, &corners, col.to_opaque(), 2.0, true); }
            else { polyline(&mut ov.lines, &corners, col.to_opaque(), 1.2, true); }
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
        let face_pick = matches!(self.mode, Mode::PickPlane) || matches!(&self.mode, Mode::Command(cm) if matches!(cm.pick, PickTarget::Face | PickTarget::AlignTarget));
        let face_pick = face_pick;
        if let (true, Some(Hit::Body { slot, tri, .. })) = (face_pick, self.hover) {
            if let Some((_, tris)) = self.face_plane(model, slot, tri) {
                fill_tris(&mut ov.tris_top, model.bodies[slot].mesh(), tris.into_iter(), c.plane_hover);
            }
        }
        if matches!(&self.mode,Mode::Command(cm) if self.doc.feature(cm.uid).is_some_and(|f|matches!(f.kind,FeatureKind::Hole{..}))) {
            if let Some(Hit::Body{slot,tri,t})=self.hover {
                let (o,d)=self.last_ray;
                if let Some((plane,at,lo,hi,snapped))=self.hole_target(model,slot,tri,o+d*t,rect) {
                    let center=[(lo[0]+hi[0])*0.5,(lo[1]+hi[1])*0.5];
                    let color=crate::theme::accent();
                    let size=self.cam.world_per_pixel(rect)*7.0;
                    for a in 0..2 {
                        let mut start=center; let mut end=center;
                        start[a]-=size;end[a]+=size;
                        ov.lines_top.push(render::line(plane.to_world(start),plane.to_world(end),color,2.0));
                        if snapped[a] {
                            let other=1-a;let mut start=at;let mut end=at;start[other]=lo[other];end[other]=hi[other];
                            ov.lines_top.push(render::line(plane.to_world(start),plane.to_world(end),color.gamma_multiply(0.7),1.5));
                        }
                    }
                }
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
            if let (PickTarget::Edges, Some((e, _))) = (cm.pick, self.hover_edge) {
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
            let col = if hot { Color32::from_rgb(255, 150, 30) } else { crate::theme::accent() };
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
                let enter = if crate::theme::reduced_motion() { 1.0 } else { crate::widgets::ease_out((age / 0.2).clamp(0.0, 1.0) as f32) };
                let alpha = enter * (1.0 - ((age - 3.6).max(0.0) / 0.4).min(1.0) as f32);
                let color = if *err { ui.visuals().error_fg_color } else { accent() };
                let fg = crate::theme::foreground_on(color);
                let galley = painter.layout_no_wrap(msg.clone(), egui::FontId::proportional(14.0), fg.gamma_multiply(alpha));
                let pill = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 32.0), galley.size() + egui::vec2(32.0, 20.0));
                painter.rect_filled(pill, 18.0, color.gamma_multiply(alpha));
                painter.galley(pill.min + egui::vec2(16.0, 10.0), galley, fg.gamma_multiply(alpha));
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
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
                    let text = crate::units::fmt(pts[0].distance(pts[1]));
                    let g = painter.layout_no_wrap(text, egui::FontId::proportional(14.0), Color32::WHITE);
                    let r = Rect::from_min_size(p + egui::vec2(10.0, -22.0), g.size() + egui::vec2(12.0, 6.0));
                    painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(200, 100, 0, 235));
                    painter.galley(r.min + egui::vec2(6.0, 3.0), g, Color32::WHITE);
                }
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

/// Расстояние от луча (o, d) до отрезка a–b.
fn ray_segment_distance(o: DVec3, d: DVec3, a: DVec3, b: DVec3) -> f64 {
    let (v, w) = (b - a, o - a);
    let (aa, bb, cc, dd, ee) = (d.dot(d), d.dot(v), v.dot(v), d.dot(w), v.dot(w));
    let den = aa * cc - bb * bb;
    let t = if den < 1e-12 { 0.0 } else { ((aa * ee - bb * dd) / den).clamp(0.0, 1.0) };
    let s = ((bb * t - dd) / aa).max(0.0);
    (o + d * s - (a + v * t)).length()
}

fn marker(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(6.0, 22.0), egui::Sense::hover());
    ui.painter().rect_filled(Rect::from_center_size(r.center(), egui::vec2(3.0, 22.0)), 1.0, accent());
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
    upload: Arc<(Vec<FaceVertex>, Vec<LineInst>)>,
    upload_gen: u64,
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
        if scene.bodies_gen != self.upload_gen {
            scene.set_bodies(device, queue, &self.upload.0, &self.upload.1);
            scene.bodies_gen = self.upload_gen;
        }
        scene.set_style(self.style.0, self.style.1);
        scene.set_overlay(device, queue, &self.overlay);
        scene.set_frame(device, queue, self.frame, &self.colors);
        let glass_present = self.colors.iter().any(|c| c[3] > 1.5);
        scene.prepare_glass(device, queue, _enc, (self.frame.glass[2] as u32, self.frame.glass[3] as u32), glass_present);
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
        self.busy_overlay(ctx, now);
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
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Confirm::Quit(Vec::new()));
        }
        if self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // тема «как в системе», масштаб интерфейса, автосохранение
        let want = resolve_theme(&self.settings, ctx);
        let appearance_changed = crate::theme::configure(self.settings.accent, want == Theme::Dark, self.settings.reduce_motion);
        if want != self.theme || appearance_changed {
            self.theme = want;
            want.apply(ctx);
        }
        self.device = detect_device(ctx);
        // на планшете и телефоне всё крупнее — под палец
        let scale = self.settings.ui_scale * if self.device == Device::Desktop { 1.0 } else { 1.25 };
        if (ctx.zoom_factor() - scale).abs() > 1e-3 {
            ctx.set_zoom_factor(scale);
        }
        if self.settings.autosave_minutes > 0 && self.confirm.is_none() && matches!(self.mode, Mode::Idle) {
            if self.last_autosave == 0.0 {
                self.last_autosave = now;
            }
            if now - self.last_autosave >= self.settings.autosave_minutes as f64 * 60.0 {
                self.last_autosave = now;
                self.autosave(now);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
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
        for file in ctx.input(|i| i.raw.dropped_files.clone()) {
            if let Some(path)=file.path {crate::desktop::enqueue(path);}
        }
        crate::units::apply(self.settings.length_unit, self.settings.decimals);
        // a click or a key changes state after the widgets were drawn; one more frame right away shows the result
        // (otherwise a toggle such as the grid button only seems to work after the mouse moves again)
        if ctx.input(|i| i.pointer.any_released() || i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. } | egui::Event::Text(_)))) {
            ctx.request_repaint();
        }
        if self.splash(ctx, now) { return; }
        // a log from an earlier crash that has not been offered yet: ask once, right after the start
        if !self.crash_checked && self.settings.setup_done {
            self.crash_checked = true;
            if let Some(newest) = crate::crash::list().first().map(|e| e.when) {
                if newest > self.settings.last_crash_seen {
                    self.settings.last_crash_seen = newest;
                    self.settings.save();
                    self.report.show(true);
                }
            }
        }
        crate::report::window(ctx, &mut self.report);
        if self.first_setup(ctx) { return; }
        // сначала лицензия, затем аккаунт (демо)
        if !crate::home::unlocked(&self.settings) {
            if crate::home::gate_ui(ctx, &mut self.settings, &mut self.gate) {
                self.settings.save();
                self.home_active = true;
                let list = self.project_list();
                self.home.refresh(&list);
            }
            self.confirm_window(ctx, now);
            return;
        }
        self.palette_ui(ctx, now);
        for action in crate::desktop::take_actions() {
            match action {
                crate::desktop::Action::New => { self.set_busy(tr("Подготовка проекта…")); self.request(Confirm::New) }
                crate::desktop::Action::Open => self.request(Confirm::Open),
                crate::desktop::Action::Save => { self.save(false, now); }
                crate::desktop::Action::SaveAs => { self.save(true, now); }
                crate::desktop::Action::Undo => self.do_undo(),
                crate::desktop::Action::Redo => self.do_redo(),
                crate::desktop::Action::Home => {
                    self.commit_pending_edits();
                    self.home_active = true;
                    self.home.navigate(crate::home::HomePage::Models);
                    let list = self.project_list();
                    self.home.refresh(&list);
                }
                crate::desktop::Action::Settings => {
                    self.commit_pending_edits();
                    self.home_active = true;
                    self.home.navigate(crate::home::HomePage::Settings);
                }
            }
        }
        if self.confirm.is_none() && matches!(self.mode,Mode::Idle) {
            for path in crate::desktop::take() {
                if crate::desktop::project_path(&path) {self.open_path(path);}
                else {self.toast(tr("Перетащите файл проекта .genix или .ostov"),true,now);}
            }
        }
        if self.device == Device::Phone {
            self.phone_ui(ctx, now);
            self.show_dialogs(ctx);
            return;
        }
        if !self.home_active && self.confirm.is_none() {
            self.shortcuts(ctx, now);
        }

        egui::TopBottomPanel::top("header").frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(8, 3))).show(ctx, |ui| self.header_bar(ui, now));
        if self.home_active {
            let mut act = crate::home::HomeAction::None;
            let list = self.project_list();
            let viewer = self.device == Device::Phone;
            egui::CentralPanel::default().show(ctx, |ui| act = crate::home::home_ui(ui, &mut self.home, &mut self.settings, &list, viewer));
            self.handle_home_action(act);
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
                ui.label(RichText::new(title).strong().color(accent()));
                ui.separator();
                self.right_panel(ui, now);
            });
        }

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| self.viewport(ui, now));

        self.show_dialogs(ctx);
    }
}

impl App {
    /// iPhone: только просмотр — главная со всеми проектами, 3D-вид (вращение пальцем, щипок — масштаб), аккаунт.
    fn phone_ui(&mut self, ctx: &egui::Context, now: f64) {
        self.mode = Mode::Idle;
        egui::TopBottomPanel::top("phone_bar").frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(10, 8))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                crate::home::logo(ui, 22.0);
                if !self.home_active && ui.add(egui::Button::new(RichText::new(format!("‹ {}", tr("Главная"))).strong()).min_size(egui::vec2(0.0, 34.0))).clicked() {
                    self.home_active = true;
                    let list = self.project_list();
                    self.home.refresh(&list);
                }
                let title = if self.home_active { tr("Главная").to_string() } else { self.tab_title(self.active).0 };
                ui.label(RichText::new(title).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let who = self.settings.account.clone().unwrap_or_else(|| tr("Войти").to_string());
                    ui.menu_button(who, |ui| {
                        if self.settings.account.is_some() {
                            if ui.button(tr("Выйти из аккаунта")).clicked() {
                                self.settings.account = None;
                                self.settings.account_skipped = false;
                                self.settings.save();
                                ui.close_menu();
                            }
                        } else if ui.button(tr("Войти в аккаунт")).clicked() {
                            self.settings.account_skipped = false;
                            self.settings.save();
                            ui.close_menu();
                        }
                    });
                });
            });
        });
        if self.home_active {
            let mut act = crate::home::HomeAction::None;
            let list = self.project_list();
            egui::CentralPanel::default().show(ctx, |ui| act = crate::home::home_ui(ui, &mut self.home, &mut self.settings, &list, true));
            self.handle_home_action(act);
        } else {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| self.viewport(ui, now));
        }
    }

    fn show_dialogs(&mut self, ctx: &egui::Context) {
        let now = self.now;
        if self.params_open {
            if !crate::widgets::text_input_active(ctx) && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.params_open = false;
            } else {
                self.params_window(ctx);
            }
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
                    egui::Grid::new("measure").num_columns(2).show(ui, |ui| {
                        ui.label(tr("Расстояние"));
                        ui.label(RichText::new(crate::units::fmt(d.length())).strong());
                        ui.end_row();
                        for (n, v) in [("ΔX", d.x), ("ΔY", d.y), ("ΔZ", d.z)] {
                            ui.label(n);
                            ui.label(crate::units::fmt(v));
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
                    if crate::ui_kit::b02(ui, &tf!("Плоскость {}", tr(plane_name(i)))).clicked() {
                        self.begin_sketch_on(*pl, now);
                        return;
                    }
                }
                ui.add_space(6.0);
                if crate::ui_kit::b03(ui, tr("Отмена")).clicked() {
                    self.mode = Mode::Idle;
                }
            }
            Mode::Idle => self.inspector(ui),
        }
    }

    fn confirm_window(&mut self, ctx: &egui::Context, now: f64) {
        let Some(c) = self.confirm.take() else { return };
        let mut choice = None;
        egui::Modal::new(egui::Id::new("unsaved_changes")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(tr("Несохранённые изменения"));
            ui.label(tr("Сохранить изменения в документе?"));
            let name = self.tab_title(self.active).0;
            ui.label(RichText::new(name).strong());
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(RichText::new(tr("Сохранить")).strong()).clicked() {
                    choice = Some(SaveChoice::Save);
                }
                if ui.button(tr("Не сохранять")).clicked() {
                    choice = Some(SaveChoice::Discard);
                }
                if crate::ui_kit::b03(ui, tr("Отмена")).clicked() {
                    choice = Some(SaveChoice::Cancel);
                }
            });
        });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            choice = Some(SaveChoice::Cancel);
        }
        match choice {
            Some(choice) => self.resolve_confirmation(c, choice, now),
            None => self.confirm = Some(c),
        }
    }

    fn resolve_confirmation(&mut self, c: Confirm, choice: SaveChoice, now: f64) {
        match choice {
            SaveChoice::Save if !self.save(false, now) => self.confirm = Some(c),
            SaveChoice::Save | SaveChoice::Discard => self.run_confirmed(c),
            SaveChoice::Cancel => {},
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
        // on a Mac ⌘ is the main modifier, but Ctrl+Z / Ctrl+Y are what most people press: both work
        let cmd = |k: egui::Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, k) || i.consume_key(egui::Modifiers::CTRL, k));
        let cmd_shift = |k: egui::Key| ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, k) || i.consume_key(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, k));
        if cmd_shift(egui::Key::S) {
            self.save(true, now);
        } else if cmd(egui::Key::S) {
            self.save(false, now);
        }
        if cmd(egui::Key::O) {
            self.request(Confirm::Open);
        }
        if cmd_shift(egui::Key::P) {
            self.export_png(now);
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
        if pressed(egui::Key::Delete) || pressed(egui::Key::Backspace) {
            if !self.selection.is_empty() {
                let ids = self.selection.clone();
                self.delete_bodies(ids);
            } else if let Some(uid) = self.tree_sketch.take() {
                self.cancel_mode();
                self.delete_feature(uid, now);
            }
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
        FeatureKind::PressPull { .. } => "press_pull",
        FeatureKind::PlaceOnPlane { .. } => "place",
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
    crate::edges::selectable(mesh)
}

impl App {
    /// Прямые рёбра тела (кэш по телу).
    fn straight_edges(&self, b: &fusion_core::doc::Body) -> Arc<Vec<EdgeRef>> {
        let key = Arc::as_ptr(&b.solid) as usize;
        let mut cache = self.edge_cache.borrow_mut();
        if cache.len() > 256 {
            cache.clear();
        }
        match cache.get(&key) {
            Some((solid, v)) if Arc::ptr_eq(solid, &b.solid) => v.clone(),
            _ => {
                let v = Arc::new(compute_straight_edges(b.mesh()));
                cache.insert(key, (b.solid.clone(), v.clone()));
                v
            }
        }
    }

    /// Ребро под курсором: ближайшее к точке экрана и не закрытое телом.
    fn pick_edge(&self, model: &Model, rect: Rect, mouse: Pos2) -> Option<(EdgeRef, u64)> {
        let eye = self.cam.eye();
        let target_body = match &self.mode {
            Mode::Command(c) => match self.doc.feature(c.uid).map(|f| &f.kind) {
                Some(FeatureKind::Chamfer { body, .. } | FeatureKind::Fillet { body, .. }) if *body != 0 => Some(*body),
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
        // visible edges win; an edge hidden behind the body can still be picked when the pointer is right on it
        // (a box always has one vertical edge that no view angle shows, and it must be selectable too)
        let mut hidden: Option<(f32, EdgeRef, u64)> = None;
        for (d, e, p, b) in cands.into_iter().take(6) {
            let to = p - eye;
            let dist = to.length();
            let blocked = b.mesh().raycast(eye, to / dist).is_some_and(|(h, _)| h < dist - 2e-3 * dist.max(1.0));
            if !blocked {
                return Some((e, b.id));
            }
            if d < 6.0 && hidden.is_none() {
                hidden = Some((d, e, b.id));
            }
        }
        hidden.map(|(_, e, id)| (e, id))
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
        let FeatureKind::Extrude { sketch, profiles, extent, face: None, .. } = &self.doc.feature(c.uid)?.kind else { return None };
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

    /// Поля значений над моделью: все главные размеры текущей команды можно вписать прямо в окне (как в Fusion),
    /// не только в правой панели. Длина, радиус, высота, угол, смещение и т. д. — по виду операции.
    fn value_box(&mut self, ui: &mut egui::Ui, rect: Rect) {
        use fusion_core::doc::{Extent, PatternKind, Shape};
        let Mode::Command(cmd) = &self.mode else { return };
        let uid = cmd.uid;
        let anchor = self.extrude_handle().and_then(|(b, d, l)| self.cam.project(rect, b + d * l));
        let params = self.doc.params.clone();
        let Some(f) = self.doc.feature_mut(uid) else { return };
        // поля появляются после выбора, как в Fusion: до щелчка они только мешают
        if commands::awaiting_pick(&f.kind) { return; }
        let mut fields: Vec<(&str, &'static str, &mut Expr)> = match &mut f.kind {
            FeatureKind::Extrude { extent: Extent::OneSide(e) | Extent::Symmetric(e), .. } => vec![("Расстояние", "мм", e)],
            FeatureKind::Extrude { extent: Extent::TwoSides(a, b), .. } => vec![("Вперёд", "мм", a), ("Назад", "мм", b)],
            FeatureKind::Revolve { angle, .. } => vec![("Угол", "°", angle)],
            FeatureKind::Fillet { radius, .. } => vec![("Радиус", "мм", radius)],
            FeatureKind::Chamfer { dist, .. } => vec![("Расстояние", "мм", dist)],
            FeatureKind::Hole { dia, depth, .. } => {
                let mut v = vec![("Диаметр", "мм", dia)];
                if let Some(d) = depth { v.push(("Глубина", "мм", d)); }
                v
            }
            FeatureKind::Shell { thickness, .. } => vec![("Толщина", "мм", thickness)],
            FeatureKind::Pipe { outer, wall, .. } => vec![("Размер", "мм", outer), ("Стенка", "мм", wall)],
            FeatureKind::OffsetPlane { offset, .. } => vec![("Расстояние", "мм", offset)],
            FeatureKind::PressPull { distance, .. } => vec![("Расстояние", "мм", distance)],
            FeatureKind::Primitive { shape: Shape::Box { w, d, h }, .. } => vec![("Ширина", "мм", w), ("Глубина", "мм", d), ("Высота", "мм", h)],
            FeatureKind::Primitive { shape: Shape::Cylinder { r, h }, .. } => vec![("Радиус", "мм", r), ("Высота", "мм", h)],
            FeatureKind::Primitive { shape: Shape::Sphere { r }, .. } => vec![("Радиус", "мм", r)],
            FeatureKind::Primitive { shape: Shape::Torus { major, minor }, .. } => vec![("Большой радиус", "мм", major), ("Малый радиус", "мм", minor)],
            FeatureKind::Scale { factor, uniform: true, .. } => vec![("Масштаб", "×", &mut factor[0])],
            FeatureKind::Scale { factor, .. } => {
                let [x, y, z] = factor;
                vec![("X", "×", x), ("Y", "×", y), ("Z", "×", z)]
            }
            FeatureKind::Move { translate, .. } => {
                let [x, y, z] = translate;
                vec![("X", "мм", x), ("Y", "мм", y), ("Z", "мм", z)]
            }
            FeatureKind::Pattern { kind: PatternKind::Rect { nx, dx, ny, dy }, .. } => vec![("Кол-во X", "", nx), ("Шаг X", "мм", dx), ("Кол-во Y", "", ny), ("Шаг Y", "мм", dy)],
            FeatureKind::Pattern { kind: PatternKind::Circular { count, angle, .. }, .. } => vec![("Кол-во", "", count), ("Угол", "°", angle)],
            _ => return,
        };
        let mut changed = false;
        // синие плашки: у ручки выдавливания, иначе сверху по центру
        let (pos, pivot) = match anchor {
            Some(p) => (p + egui::vec2(16.0, -16.0), egui::Align2::LEFT_BOTTOM),
            None => (Pos2::new(rect.center().x, rect.top() + 14.0), egui::Align2::CENTER_TOP),
        };
        let blue = Color32::from_rgb(37, 99, 176);
        egui::Area::new(egui::Id::new("value_box")).fixed_pos(pos).pivot(pivot).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (i, (label, unit, expr)) in fields.iter_mut().enumerate() {
                    egui::Frame::new().fill(blue).corner_radius(10).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                        let v = ui.visuals_mut();
                        v.override_text_color = Some(Color32::WHITE);
                        v.extreme_bg_color = Color32::from_rgb(20, 66, 128);
                        v.widgets.inactive.bg_stroke = egui::Stroke::NONE;
                        // weak text (the unit) fades towards this colour; keep it readable on blue
                        v.widgets.noninteractive.weak_bg_fill = blue;
                        ui.spacing_mut().text_edit_width = 60.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            crate::widgets::scrub_caption(ui, RichText::new(tr(label)).size(13.0), ("value_box", uid, i));
                            changed |= crate::widgets::expr_field(ui, ("value_box", uid, i), expr, &params, unit);
                        });
                    });
                }
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
            egui::Frame::new().fill(crate::theme::surface(ui)).corner_radius(24).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    let sz = egui::vec2(36.0, 34.0);
                    let b = |ui: &mut egui::Ui, id: &str, icon: &str, tip: &str| icon_button(ui, id, icon, false, tip, sz);
                    for (m, id, icon, tip) in [(NavMode::Orbit, "nav_orbit", "↻", "Вращение левой кнопкой"), (NavMode::Pan, "nav_pan", "↔", "Сдвиг левой кнопкой"), (NavMode::Zoom, "nav_zoom", "🔍", "Масштаб левой кнопкой")] {
                        let on = self.nav == m;
                        if icon_button(ui, id, icon, on, tr(tip), sz) {
                            self.nav = if on { NavMode::Default } else { m };
                        }
                    }
                    ui.add_space(4.0);
                    for (id, icon, tip, on) in [("nav_panel_left", "◧", "Левая панель", &mut self.settings.show_left_panel), ("nav_panel_right", "◨", "Правая панель", &mut self.settings.show_right_panel)] {
                        if icon_button(ui, id, icon, *on, tr(tip), sz) {
                            *on = !*on;
                        }
                    }
                    ui.add_space(4.0);
                    if b(ui, "nav_fit", "⌖", tr("Вписать модель (F)")) {
                        self.fit_view(now, false);
                    }
                    if b(ui, "nav_zoom_in", "+", tr("Приблизить")) {
                        self.cam.zoom_at(0.8, rect, None);
                    }
                    if b(ui, "nav_zoom_out", "−", tr("Отдалить")) {
                        self.cam.zoom_at(1.25, rect, None);
                    }
                    ui.add_space(4.0);
                    let style_icon = crate::icons::icon("nav_style", 22.0, ui.visuals().text_color());
                    let style_button = match style_icon { Some(img) => egui::Button::image(img), None => egui::Button::new("◧") };
                    let style_menu = egui::menu::menu_custom_button(ui, style_button.corner_radius(12).frame(false).min_size(sz), |ui| {
                        for st in VisualStyle::ALL {
                            if ui.radio_value(&mut self.settings.visual_style, st, tr(st.label())).clicked() {
                                self.settings.save();
                                ui.close_menu();
                            }
                        }
                    });
                    crate::widgets::warm_tooltip(&style_menu.response, &format!("{}: {}", tr("Стиль отображения"), tr(self.settings.visual_style.label())));
                    ui.add_space(4.0);
                    if icon_button(ui, "nav_grid", "▦", self.settings.show_grid, tr("Сетка"), sz) {
                        self.settings.show_grid = !self.settings.show_grid;
                        self.settings.save();
                    }
                    if icon_button(ui, "tree_plane", "⊞", self.show_planes, tr("Плоскости начала координат"), sz) {
                        self.show_planes = !self.show_planes;
                    }
                    ui.add_space(4.0);
                    if b(ui, "sk_photo", "📷", tr("Снимок модели PNG без фона (⇧⌘P)")) {
                        self.export_png(now);
                    }
                    if b(ui, "nav_settings", "⚙", tr("Настройки")) {
                        self.commit_pending_edits();
                        self.home_active = true;
                        self.home.navigate(crate::home::HomePage::Settings);
                    }
                });
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.settings_open;
        let before = self.settings.clone();
        let mut navigate = None;
        egui::Window::new(tr("Настройки")).open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in [tr("Основные"), tr("Просмотр"), tr("О программе")].iter().enumerate() {
                    if ui.selectable_label(self.settings_tab == i as u8, RichText::new(*name).strong().color(if self.settings_tab == i as u8 { crate::theme::on_accent() } else { ui.visuals().text_color() })).clicked() {
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
                    row(ui, tr("Акцентный цвет"), &mut |ui| {
                        for color in crate::theme::Accent::ALL {
                            if crate::widgets::accent_swatch(ui, color, st.accent == color).clicked() { st.accent = color; }
                        }
                    });
                    crate::widgets::toggle(ui, &mut st.reduce_motion, tr("Уменьшить движение"));
                    row(ui, tr("Масштаб интерфейса"), &mut |ui| {
                        crate::widgets::scale_slider(ui, &mut st.ui_scale);
                    });
                    crate::widgets::preference_grid(ui, st, "win-units", false);
                    row(ui, tr("Материал по умолчанию"), &mut |ui| {
                        egui::ComboBox::from_id_salt("mat_default").selected_text(tr(Materials::get(st.material).0)).show_ui(ui, |ui| {
                            for (i, (n, _, _, _)) in Materials::LIST.iter().enumerate() {
                                ui.selectable_value(&mut st.material, i, tr(n));
                            }
                        });
                    });
                    row(ui, tr("Автосохранение"), &mut |ui| {
                        let mut on = st.autosave_minutes > 0;
                        if crate::widgets::toggle(ui, &mut on, tr("каждые")).changed() {
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
                        crate::widgets::toggle(ui, &mut st.show_grid, tr("показывать"));
                    });
                    row(ui, tr("Привязка к сетке в чертеже"), &mut |ui| {
                        crate::widgets::toggle(ui, &mut st.snap_grid, tr("включена по умолчанию"));
                    });
                    row(ui, tr("Числа вдоль осей"), &mut |ui| {
                        crate::widgets::toggle(ui, &mut st.grid_numbers, tr("показывать"));
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
                        // one wheel notch scales the view by exactly this much
                        ui.label(RichText::new(format!("×{:.2}", (1.0 / 0.88f64).powf(st.zoom_speed as f64))).weak());
                    });
                    row(ui, tr("Направление масштаба"), &mut |ui| {
                        crate::widgets::toggle(ui, &mut st.invert_zoom, tr("обратное"));
                    });
                }
                _ => {
                    ui.heading("Genix");
                    ui.collapsing("React Bits · License", |ui| {
                        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| { ui.label(include_str!("../../../docs/REACT_BITS_LICENSE.md")); });
                    });
                    ui.label(format!("{} {} — {}", tr("Версия"), crate::version::version(), crate::version::note()));
                    ui.label(tr("Параметрический 3D-CAD: чертежи, твердотельное моделирование, история построения."));
                    ui.add_space(6.0);
                    if let Some(p) = Settings::path() {
                        ui.label(RichText::new(format!("{}: {}", tr("Файл настроек"), p.display())).weak().small());
                    }
                    ui.add_space(6.0);
                    ui.label(RichText::new(tr("Локальный аккаунт · мок")).weak());
                    ui.label(st.account.as_deref().unwrap_or(tr("Локальное пространство")));
                    ui.horizontal(|ui| {
                        if ui.button(tr("Профиль")).clicked() { navigate = Some(crate::home::HomePage::Profile); }
                        if ui.button(tr("Обновления")).clicked() { navigate = Some(crate::home::HomePage::Updates); }
                    });
                    ui.add_space(6.0);
                    if ui.button(tr("Сбросить все настройки")).clicked() {
                        *st = Settings { lang: st.lang, license: st.license.take(), account: st.account.take(), account_skipped: st.account_skipped, display_name: std::mem::take(&mut st.display_name), server_demo_account: st.server_demo_account.take(), recent: std::mem::take(&mut st.recent), ..Settings::default() };
                    }
                }
            }
        });
        self.settings_open = open && navigate.is_none();
        if let Some(page) = navigate {
            self.commit_pending_edits();
            self.home_active = true;
            self.home.navigate(page);
        }
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


#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn app() -> App {
        App::from_settings(Settings::default(), Theme::Light)
    }

    #[test]
    fn the_place_on_plane_command_waits_for_the_user_to_pick_a_face() {
        use fusion_core::doc::Shape;
        let mut app = app();
        let n = Expr::num;
        app.doc.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(30.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: fusion_core::Op::NewBody });
        app.after_doc_change();
        app.start_command("place", 0.0);
        let uid = match &app.mode { Mode::Command(c) => c.uid, _ => panic!("the command did not start") };
        assert!(matches!(app.doc.feature(uid).unwrap().kind, FeatureKind::PlaceOnPlane { .. }));
        assert!(matches!(app.doc.feature(uid).unwrap().kind, FeatureKind::PlaceOnPlane { body: 0, .. }), "no body may be chosen for the user");
    }

    #[test]
    fn an_extrude_shows_its_result_while_faces_are_picked_on_the_body_before_it() {
        let mut app = app();
        app.load_doc(crate::examples::bracket(), None);
        app.start_command("extrude", 0.0);
        let Mode::Command(c) = &app.mode else { panic!("the command did not start") };
        let i = app.doc.index_of(c.uid).unwrap();
        assert!(Arc::ptr_eq(&app.shown(), &app.ev.model()), "growing the extrude must be visible");
        assert!(Arc::ptr_eq(&app.pick_model(), &app.ev.model_before(i)), "a face is picked on the unextruded body");
    }

    #[test]
    fn a_face_for_put_on_plane_is_picked_on_the_body_before_it_moved() {
        use fusion_core::doc::Shape;
        let mut app = app();
        let n = Expr::num;
        app.doc.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(30.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: fusion_core::Op::NewBody });
        app.after_doc_change();
        let id = app.ev.model().bodies[0].id;
        let side = Plane { origin: [0.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };
        let uid = app.doc.insert(FeatureKind::PlaceOnPlane { body: id, face: side, seed: [0.0, 15.0, 5.0], target: Plane::XY, to: None });
        app.after_doc_change();
        app.edit_feature(uid, 0.0);
        let i = app.doc.index_of(uid).unwrap();
        assert!(Arc::ptr_eq(&app.shown(), &app.ev.model()), "editing starts on the result");
        if let Mode::Command(c) = &mut app.mode { c.pick = PickTarget::Face; }
        assert!(Arc::ptr_eq(&app.shown(), &app.ev.model_before(i)), "a new face is picked on the unmoved body");
    }

    #[test]
    fn a_fillet_waits_for_an_edge_and_takes_its_body_from_it() {
        let mut app = app();
        app.load_doc(crate::examples::bracket(), None);
        app.start_command("fillet", 0.0);
        let Mode::Command(c) = &app.mode else { panic!("the command did not start") };
        let kind = app.doc.feature(c.uid).unwrap().kind.clone();
        assert!(matches!(kind, FeatureKind::Fillet { body: 0, .. }));
        assert!(commands::awaiting_pick(&kind), "no red error before the first pick");
    }

    #[test]
    fn the_pipe_command_does_not_pick_a_sketch_by_itself() {
        let mut app = app();
        app.doc.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: Default::default() });
        app.after_doc_change();
        app.start_command("pipe", 0.0);
        let Mode::Command(c) = &app.mode else { panic!("the command did not start") };
        assert!(c.pick == PickTarget::Path);
        assert!(matches!(app.doc.feature(c.uid).unwrap().kind, FeatureKind::Pipe { path: 0, .. }));
    }

    #[test]
    fn a_click_ray_finds_the_nearest_segment() {
        let (a, b) = (DVec3::new(0.0, 0.0, 0.0), DVec3::new(10.0, 0.0, 0.0));
        let d = ray_segment_distance(DVec3::new(5.0, 3.0, 10.0), DVec3::new(0.0, 0.0, -1.0), a, b);
        assert!((d - 3.0).abs() < 1e-9, "{d}");
        assert!((ray_segment_distance(DVec3::new(15.0, 0.0, 10.0), DVec3::new(0.0, 0.0, -1.0), a, b) - 5.0).abs() < 1e-9);
    }

    #[test]
    fn a_loaded_document_does_not_inherit_the_name_folder_or_group_of_the_tab() {
        let mut app = app();
        app.suggested = Some("Подставка".into());
        app.folder = Some("Работа".into());
        app.group = Some(7);
        assert_eq!(app.tab_title(0).0, "Подставка");
        app.load_doc(Document::default(), None);
        assert_eq!((app.suggested.clone(), app.folder.clone(), app.group), (None, None, None));
        assert_eq!(app.tab_title(0).0, tr("Без имени"));
    }

    #[test]
    fn tab_groups_keep_their_tabs_together_and_vanish_when_empty() {
        let mut app = app();
        app.new_blank_tab();
        app.new_blank_tab();
        // tabs 0 and 2 go into one group: the display order puts them side by side
        app.new_group_for(0);
        let id = app.group_of(0).unwrap();
        app.set_group(2, Some(id));
        assert_eq!(app.tab_order(), vec![0, 2, 1]);
        // the group data survives switching the active tab (the active tab's group lives in the App, the others in Tab)
        app.switch_tab(1);
        assert_eq!((app.group_of(0), app.group_of(2)), (Some(id), Some(id)));
        app.set_group(0, None);
        app.set_group(2, None);
        assert_eq!(app.tab_order(), vec![0, 1, 2]);
    }

    #[test]
    fn quit_checks_inactive_tabs_and_cancel_keeps_changes() {
        let mut app = app();
        app.doc.params.set("width", "10");
        app.new_blank_tab();
        assert!(!app.dirty());
        app.request(Confirm::Quit(Vec::new()));
        assert_eq!(app.active, 0);
        assert!(!app.allow_close);
        let confirm = app.confirm.take().unwrap();
        app.resolve_confirmation(confirm, SaveChoice::Cancel, 1.0);
        assert!(app.confirm.is_none());
        assert!(app.dirty());
        assert!(!app.allow_close);
    }

    #[test]
    fn quit_asks_for_every_dirty_tab_without_marking_discarded_as_saved() {
        let mut app = app();
        app.doc.params.set("first", "10");
        app.new_blank_tab();
        app.doc.params.set("second", "20");
        app.request(Confirm::Quit(Vec::new()));
        assert_eq!(app.active, 0);
        let confirm = app.confirm.take().unwrap();
        app.resolve_confirmation(confirm, SaveChoice::Discard, 1.0);
        assert_eq!(app.active, 1);
        assert!(!app.allow_close);
        let confirm = app.confirm.take().unwrap();
        app.resolve_confirmation(confirm, SaveChoice::Cancel, 1.0);
        assert_eq!(app.dirty_tabs(), vec![0, 1]);
        app.request(Confirm::Quit(Vec::new()));
        for _ in 0..2 {
            let confirm = app.confirm.take().unwrap();
            app.resolve_confirmation(confirm, SaveChoice::Discard, 1.0);
        }
        assert!(app.allow_close);
    }

    #[test]
    fn failed_save_keeps_confirmation_and_document() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.path = Some(dir.path().join("missing/project.genix"));
        app.doc.params.set("width", "10");
        app.request(Confirm::CloseTab);
        let before = app.doc.clone();
        let confirm = app.confirm.take().unwrap();
        app.resolve_confirmation(confirm, SaveChoice::Save, 1.0);
        assert_eq!(app.doc, before);
        assert!(app.dirty());
        assert!(matches!(app.confirm, Some(Confirm::CloseTab)));
        assert!(!app.allow_close);
    }

    #[test]
    fn hole_placement_snaps_to_actual_face_center_and_axes() {
        let app=app();
        let mesh=fusion_core::geom::box_mesh(DVec3::ZERO,DVec3::new(40.0,20.0,10.0));
        let solid=Arc::new(fusion_core::csg::Solid::from_mesh(&mesh).unwrap());
        let model=Model {bodies:vec![fusion_core::doc::Body{id:1,name:String::new(),solid}],..Default::default()};
        let tri=model.bodies[0].mesh().triangles.iter().position(|t| model.bodies[0].mesh().tri(t).iter().all(|p|(p.z-10.0).abs()<1e-6)).unwrap();
        let rect=Rect::from_min_size(Pos2::ZERO,egui::vec2(1000.0,800.0));
        let (plane,at,lo,hi,snapped)=app.hole_target(&model,0,tri,DVec3::new(20.01,10.01,10.0),rect).unwrap();
        assert_eq!(snapped,[true,true]);
        assert!((plane.to_world(at)-DVec3::new(20.0,10.0,10.0)).length()<1e-6);
        assert_eq!(at,[(lo[0]+hi[0])*0.5,(lo[1]+hi[1])*0.5]);
        let (_,_,_,_,snapped)=app.hole_target(&model,0,tri,DVec3::new(2.0,2.0,10.0),rect).unwrap();
        assert_eq!(snapped,[false,false]);
    }

    #[test]
    fn reopening_same_project_selects_existing_tab() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("finder.genix");
        std::fs::write(&path,Document::default().to_json()).unwrap();
        let mut app=app();app.open_path(path.clone());let count=app.tabs.len();
        app.open_path(path);assert_eq!(app.tabs.len(),count);assert!(!app.home_active);
    }

    #[test]
    fn closing_commits_pending_sketch_and_can_be_cancelled() {
        let mut app = app();
        app.begin_sketch_on(Plane::XY, 0.0);
        let Mode::Sketch(session) = &mut app.mode else { panic!("sketch") };
        session.sketch.add_rect([0.0, 0.0], [10.0, 20.0]);
        assert!(!app.dirty()); // чертёж ещё не перенесён в документ
        app.request(Confirm::Quit(Vec::new()));
        assert!(app.dirty());
        assert_eq!(app.doc.features.len(), 1);
        assert!(!app.allow_close);
        let confirm = app.confirm.take().unwrap();
        app.resolve_confirmation(confirm, SaveChoice::Cancel, 1.0);
        assert_eq!(app.doc.features.len(), 1);
        app.do_undo();
        assert!(app.doc.features.is_empty());
    }

    #[test]
    fn autosave_writes_parked_tabs_and_keeps_unnamed_documents_dirty() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        let path = dir.path().join("first.genix");
        app.path = Some(path.clone());
        app.doc.params.set("width", "42");
        let expected = app.doc.clone();
        app.new_blank_tab();
        app.doc.params.set("height", "7");
        app.autosave(10.0);
        assert_eq!(app.active, 1);
        assert!(app.dirty());
        assert_eq!(app.tabs[0].doc, app.tabs[0].saved);
        let restored = Document::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(restored, expected);
    }

    #[test]
    fn clean_session_quits_immediately() {
        let mut app = app();
        app.request(Confirm::Quit(Vec::new()));
        assert!(app.allow_close);
        assert!(app.confirm.is_none());
    }
}
