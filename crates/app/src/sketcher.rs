//! Режим чертежа: инструменты рисования, ограничения, размеры, отображение.

use crate::i18n::{tr, tr_dyn};
use crate::tf;
use crate::camera::Camera;
use crate::gpu::Overlay;
use crate::render::{self, dashed, fill_region, polyline};
use crate::theme::ViewColors;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Stroke};
use fusion_core::expr::format_num;
use fusion_core::sketch::{Constraint, Entity, Plane, Region, SolveStatus, Sketch, P2};
use fusion_core::{Expr, Params};
use glam::{DVec2, DVec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsKind {
    Horizontal,
    Vertical,
    Parallel,
    Perpendicular,
    Tangent,
    Equal,
    Concentric,
    Coincident,
    Midpoint,
    Fix,
}

impl ConsKind {
    pub const ALL: [ConsKind; 10] = [
        ConsKind::Horizontal,
        ConsKind::Vertical,
        ConsKind::Parallel,
        ConsKind::Perpendicular,
        ConsKind::Tangent,
        ConsKind::Equal,
        ConsKind::Concentric,
        ConsKind::Coincident,
        ConsKind::Midpoint,
        ConsKind::Fix,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ConsKind::Horizontal => tr("Горизонт."),
            ConsKind::Vertical => tr("Вертик."),
            ConsKind::Parallel => tr("Параллельно"),
            ConsKind::Perpendicular => tr("Перпендик."),
            ConsKind::Tangent => tr("Касание"),
            ConsKind::Equal => tr("Равно"),
            ConsKind::Concentric => tr("Концентрич."),
            ConsKind::Coincident => tr("Совпадение"),
            ConsKind::Midpoint => tr("Середина"),
            ConsKind::Fix => tr("Фиксация"),
        }
    }
    fn hint(self) -> &'static str {
        match self {
            ConsKind::Horizontal | ConsKind::Vertical => tr("Щёлкните линию"),
            ConsKind::Parallel | ConsKind::Perpendicular => tr("Щёлкните две линии"),
            ConsKind::Tangent => tr("Щёлкните линию и окружность/дугу (или две окружности)"),
            ConsKind::Equal => tr("Щёлкните две линии или две окружности"),
            ConsKind::Concentric => tr("Щёлкните две окружности/дуги"),
            ConsKind::Coincident => tr("Щёлкните точку, затем точку или кривую"),
            ConsKind::Midpoint => tr("Щёлкните точку, затем линию"),
            ConsKind::Fix => tr("Щёлкните точку, чтобы закрепить или открепить её"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Line,
    Rect,
    Circle,
    Arc,
    Polygon,
    Slot,
    CenterRect,
    Dimension,
    Angle,
    Trim,
    Offset,
    Corner,
    Cons(ConsKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    Point(usize),
    Entity(usize),
    /// Ограничение или размер (по индексу в списке ограничений).
    Cons(usize),
}

/// Точка курсора с привязкой: координаты и (если есть) существующая точка чертежа.
#[derive(Debug, Clone, Copy)]
struct Snap {
    pos: P2,
    point: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Pending {
    None,
    /// Линия: текущее начало и первая точка цепочки.
    Line { start: usize, first: usize },
    Rect { p0: P2, pt: Option<usize> },
    Circle { c: P2, pt: Option<usize> },
    Arc1 { a: P2, pa: Option<usize> },
    Arc2 { a: P2, pa: Option<usize>, b: P2, pb: Option<usize> },
    Polygon { c: P2, pt: Option<usize> },
    Slot1 { a: P2, pa: Option<usize> },
    Slot2 { a: P2, pa: Option<usize>, b: P2, pb: Option<usize> },
    CRect { c: P2, pt: Option<usize> },
    /// Ограничение/размер ждёт второй объект.
    First(Pick),
    Drag { points: [Option<(usize, P2)>; 2], grab: P2 },
}

pub enum SketchAction {
    None,
    Finish,
    Cancel,
}

pub struct SketchSession {
    /// uid редактируемого чертежа (None — новый).
    pub uid: Option<u64>,
    pub plane: Plane,
    pub sketch: Sketch,
    pub tool: Tool,
    pub snap_grid: bool,
    pub status: SolveStatus,
    pub dof: usize,
    /// Число сторон правильного многоугольника.
    pub poly_sides: u32,
    pub trace: Option<crate::trace_ui::TraceDialog>,
    pub show_constraints: bool,
    pending_resolve: bool,
    pub offset_dist: f64,
    pub corner_r: f64,
    pub selection: Vec<Pick>,
    pub message: Option<String>,
    regions: Vec<Region>,
    hover: Option<Pick>,
    cursor: Option<Snap>,
    pending: Pending,
    /// Значения, набранные с клавиатуры во время рисования (длина/диаметр, ширина и высота).
    typed: [String; 2],
    typed_field: usize,
    history: Vec<Sketch>,
    future: Vec<Sketch>,
    edit_dim: Option<(usize, String, bool)>,
    label_rects: Vec<(usize, Rect)>,
    grid_step: f64,
}

const PICK_PX: f32 = 7.0;

impl SketchSession {
    pub fn new(uid: Option<u64>, plane: Plane, sketch: Sketch, params: &Params) -> Self {
        let mut s = SketchSession {
            uid,
            plane,
            sketch,
            tool: Tool::Line,
            snap_grid: true,
            poly_sides: 6,
            trace: None,
            show_constraints: false,
            pending_resolve: false,
            offset_dist: 5.0,
            corner_r: 5.0,
            status: SolveStatus::Ok,
            dof: 0,
            selection: Vec::new(),
            message: None,
            regions: Vec::new(),
            hover: None,
            cursor: None,
            pending: Pending::None,
            typed: Default::default(),
            typed_field: 0,
            history: Vec::new(),
            future: Vec::new(),
            edit_dim: None,
            label_rects: Vec::new(),
            grid_step: 1.0,
        };
        if uid.is_some() {
            s.tool = Tool::Select;
        }
        s.resolve(params);
        s
    }

    pub fn grid_step(&self) -> f64 {
        self.grid_step
    }

    pub fn is_busy(&self) -> bool {
        !matches!(self.pending, Pending::None) || self.edit_dim.is_some()
    }

    pub fn hint(&self) -> String {
        if let Some(m) = &self.message {
            return m.clone();
        }
        match (self.tool, self.pending) {
            (Tool::Select, _) => tr("Щёлкните объект для выбора, тяните точки. Delete — удалить, X — вспомогательная линия, двойной щелчок по размеру — изменить").into(),
            (Tool::Line, Pending::None) => tr("Линия: щёлкните начальную точку").into(),
            (Tool::Line, _) => tr("Щёлкните следующую точку или наберите длину и Enter. Esc — закончить").into(),
            (Tool::Rect, Pending::None) => tr("Прямоугольник: щёлкните первый угол").into(),
            (Tool::Rect, _) => tr("Щёлкните противоположный угол или наберите ширину, Tab, высоту, Enter").into(),
            (Tool::Circle, Pending::None) => tr("Окружность: щёлкните центр").into(),
            (Tool::Circle, _) => tr("Щёлкните точку на окружности или наберите диаметр и Enter").into(),
            (Tool::Arc, Pending::None) => tr("Дуга: щёлкните начальную точку").into(),
            (Tool::Arc, Pending::Arc1 { .. }) => tr("Щёлкните конечную точку дуги").into(),
            (Tool::Arc, _) => tr("Щёлкните точку, через которую проходит дуга").into(),
            (Tool::Polygon, Pending::None) => tr("Многоугольник: щёлкните центр").into(),
            (Tool::Polygon, _) => tr("Щёлкните вершину — задаёт размер и поворот. Число сторон справа").into(),
            (Tool::Trim, _) => tr("Обрезка: щёлкните часть линии, которую нужно убрать (до ближайших пересечений)").into(),
            (Tool::Offset, _) => tr("Смещение: щёлкните линию или окружность с той стороны, куда сместить. Расстояние справа").into(),
            (Tool::Corner, _) => tr("Скругление угла: щёлкните вершину, где сходятся две линии. Радиус справа").into(),
            (Tool::Slot, Pending::None) => tr("Паз: щёлкните центр первого края").into(),
            (Tool::Slot, Pending::Slot1 { .. }) => tr("Щёлкните центр второго края").into(),
            (Tool::Slot, _) => tr("Щёлкните, чтобы задать ширину паза").into(),
            (Tool::CenterRect, Pending::None) => tr("Прямоугольник от центра: щёлкните центр").into(),
            (Tool::CenterRect, _) => tr("Щёлкните угол прямоугольника").into(),
            (Tool::Dimension, Pending::None) => tr("Размер: щёлкните линию (длина), окружность (диаметр) или две точки (расстояние)").into(),
            (Tool::Dimension, _) => tr("Щёлкните вторую точку").into(),
            (Tool::Angle, Pending::None) => tr("Угол: щёлкните первую линию").into(),
            (Tool::Angle, _) => tr("Щёлкните вторую линию").into(),
            (Tool::Cons(k), _) => k.hint().into(),
        }
    }

    fn resolve(&mut self, params: &Params) {
        match self.sketch.solve(params, &[]) {
            Ok(st) => self.status = st,
            Err(e) => {
                self.status = SolveStatus::Conflict(f64::NAN);
                self.message = Some(tf!("Ошибка в размере: {}", tr_dyn(&e)));
            }
        }
        self.regions = self.sketch.regions();
        self.dof = self.sketch.free_dof(params);
    }

    fn checkpoint(&mut self) {
        self.history.push(self.sketch.clone());
        if self.history.len() > 200 {
            self.history.remove(0);
        }
        self.future.clear();
    }

    pub fn undo(&mut self, params: &Params) {
        self.cancel_pending();
        if let Some(s) = self.history.pop() {
            self.future.push(std::mem::replace(&mut self.sketch, s));
            self.selection.clear();
            self.resolve(params);
        }
    }

    pub fn redo(&mut self, params: &Params) {
        self.cancel_pending();
        if let Some(s) = self.future.pop() {
            self.history.push(std::mem::replace(&mut self.sketch, s));
            self.selection.clear();
            self.resolve(params);
        }
    }

    pub fn set_tool(&mut self, t: Tool) {
        self.cancel_pending();
        self.tool = t;
        self.message = None;
    }

    fn cancel_pending(&mut self) {
        if matches!(self.pending, Pending::Line { .. }) {
            // начальная точка линии могла остаться одна — убираем её
            self.sketch.gc_points();
        }
        self.pending = Pending::None;
        self.typed = Default::default();
        self.typed_field = 0;
    }

    /// Применить изменение с проверкой: если ограничения противоречат — откатить.
    fn commit_checked(&mut self, params: &Params, what: &str) -> bool {
        self.resolve(params);
        if let SolveStatus::Conflict(_) = self.status {
            if let Some(prev) = self.history.pop() {
                self.sketch = prev;
                self.resolve(params);
            }
            self.message = Some(tf!("{}: противоречит существующим ограничениям — не добавлено", what));
            return false;
        }
        self.message = None;
        true
    }

    // -------------------------------------------------------------- ввод

    fn local(&self, cam: &Camera, rect: Rect, p: Pos2) -> Option<P2> {
        let (o, d) = cam.ray(rect, p);
        let t = self.plane.ray_hit(o, d)?;
        Some(self.plane.to_local(o + d * t))
    }

    fn screen(&self, cam: &Camera, rect: Rect, p: P2) -> Option<Pos2> {
        cam.project(rect, self.plane.to_world(p))
    }

    fn pick(&self, cam: &Camera, rect: Rect, mouse: Pos2) -> Option<Pick> {
        // размеры и значки ограничений (по прямоугольникам подписей)
        for (i, r) in self.label_rects.iter().rev() {
            if r.expand(2.0).contains(mouse) {
                return Some(Pick::Cons(*i));
            }
        }
        let mut best: Option<(f32, Pick)> = None;
        for (i, p) in self.sketch.points.iter().enumerate() {
            if let Some(s) = self.screen(cam, rect, p.pos) {
                let d = s.distance(mouse);
                if d < PICK_PX && best.is_none_or(|b| d < b.0) {
                    best = Some((d, Pick::Point(i)));
                }
            }
        }
        if best.is_some() {
            return best.map(|b| b.1);
        }
        for i in 0..self.sketch.entities.len() {
            let pts: Vec<Pos2> = self.sketch.tessellate(i).iter().filter_map(|p| self.screen(cam, rect, *p)).collect();
            for w in pts.windows(2) {
                let d = seg_dist(mouse, w[0], w[1]);
                if d < PICK_PX && best.is_none_or(|b| d < b.0) {
                    best = Some((d, Pick::Entity(i)));
                }
            }
        }
        best.map(|b| b.1)
    }

    fn snap(&self, cam: &Camera, rect: Rect, mouse: Pos2, exclude: Option<usize>) -> Option<Snap> {
        let raw = self.local(cam, rect, mouse)?;
        // 1) существующие точки
        let mut best: Option<(f32, usize)> = None;
        for (i, p) in self.sketch.points.iter().enumerate() {
            if Some(i) == exclude {
                continue;
            }
            if let Some(s) = self.screen(cam, rect, p.pos) {
                let d = s.distance(mouse);
                if d < PICK_PX + 3.0 && best.is_none_or(|b| d < b.0) {
                    best = Some((d, i));
                }
            }
        }
        if let Some((_, i)) = best {
            return Some(Snap { pos: self.sketch.points[i].pos, point: Some(i) });
        }
        // 2) сетка
        if self.snap_grid && self.grid_step > 0.0 {
            let g = self.grid_step;
            let q = [(raw[0] / g).round() * g, (raw[1] / g).round() * g];
            // привязываем, только если узел сетки близко на экране
            if let (Some(a), Some(b)) = (self.screen(cam, rect, q), self.screen(cam, rect, raw)) {
                if a.distance(b) < 10.0 {
                    return Some(Snap { pos: q, point: None });
                }
            }
        }
        Some(Snap { pos: raw, point: None })
    }

    fn point_for(&mut self, s: Snap) -> usize {
        s.point.unwrap_or_else(|| self.sketch.add_point(s.pos))
    }

    /// Обработать мышь и клавиатуру. `resp` — отклик области 3D-вида.
    pub fn handle(&mut self, ui: &egui::Ui, resp: &egui::Response, cam: &mut Camera, rect: Rect, params: &Params) -> SketchAction {
        self.grid_step = render::decade_step(cam.world_per_pixel(rect), 14.0);
        let ctx = ui.ctx().clone();
        let mouse = resp.hover_pos();
        let exclude = match self.pending {
            Pending::Drag { points: [Some((p, _)), None], .. } => Some(p),
            _ => None,
        };
        self.cursor = mouse.and_then(|m| self.snap(cam, rect, m, exclude));
        self.hover = if matches!(self.pending, Pending::Drag { .. }) { None } else { mouse.and_then(|m| self.pick(cam, rect, m)) };

        // ---------- клавиатура (если не набираем текст в поле)
        if !crate::widgets::text_input_active(&ctx) && self.edit_dim.is_none() {
            // Tab в чертеже переключает поля ввода, а не фокус кнопок
            crate::widgets::release_button_focus(&ctx);
            let mut action = SketchAction::None;
            ctx.input(|i| {
                for ev in &i.events {
                    if let egui::Event::Text(t) = ev {
                        if self.accepts_typing() && !i.modifiers.command {
                            for ch in t.chars() {
                                if ch.is_ascii_digit() || ".,-+*/()^_".contains(ch) || ch.is_alphabetic() {
                                    self.typed[self.typed_field].push(if ch == ',' { '.' } else { ch });
                                }
                            }
                        }
                    }
                }
            });
            let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k));
            let typing = self.accepts_typing() && !self.typed[0].is_empty() || !self.typed[1].is_empty();
            if pressed(egui::Key::Backspace) && typing {
                self.typed[self.typed_field].pop();
            } else if (pressed(egui::Key::Tab) || ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == ";" || t == " ")))) && matches!(self.pending, Pending::Rect { .. }) {
                self.typed_field = 1 - self.typed_field;
            } else if pressed(egui::Key::Enter) && typing {
                self.apply_typed(params);
            } else if pressed(egui::Key::Escape) {
                if !matches!(self.pending, Pending::None) {
                    self.cancel_pending();
                } else if self.tool != Tool::Select {
                    self.set_tool(Tool::Select);
                } else if !self.selection.is_empty() {
                    self.selection.clear();
                } else {
                    action = SketchAction::Finish;
                }
            } else if (pressed(egui::Key::Delete) || pressed(egui::Key::Backspace)) && !self.selection.is_empty() {
                self.delete_selection(params);
            } else if !typing && !ctx.input(|i| i.modifiers.command) {
                // горячие клавиши инструментов
                for (k, t) in [
                    (egui::Key::L, Tool::Line),
                    (egui::Key::R, Tool::Rect),
                    (egui::Key::C, Tool::Circle),
                    (egui::Key::A, Tool::Arc),
                    (egui::Key::P, Tool::Polygon),
                    (egui::Key::O, Tool::Slot),
                    (egui::Key::T, Tool::Trim),
                    (egui::Key::D, Tool::Dimension),
                ] {
                    if pressed(k) && matches!(self.pending, Pending::None) {
                        self.set_tool(t);
                    }
                }
                if pressed(egui::Key::X) && matches!(self.pending, Pending::None) {
                    self.toggle_construction(params);
                }
            }
            if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
                if ctx.input(|i| i.modifiers.shift) {
                    self.redo(params);
                } else {
                    self.undo(params);
                }
            }
            if !matches!(action, SketchAction::None) {
                return action;
            }
        }

        // ---------- мышь
        let primary_drag = resp.dragged_by(egui::PointerButton::Primary);
        if resp.drag_started_by(egui::PointerButton::Primary) && self.tool == Tool::Select {
            if let Some(press) = ctx.input(|i| i.pointer.press_origin()) {
                if let Some(hit) = self.pick(cam, rect, press) {
                    let grab = self.local(cam, rect, press).unwrap_or([0.0, 0.0]);
                    let points = match hit {
                        Pick::Point(p) => [Some((p, self.sketch.points[p].pos)), None],
                        Pick::Entity(e) => match self.sketch.entities[e] {
                            Entity::Line { a, b, .. } => [Some((a, self.sketch.points[a].pos)), Some((b, self.sketch.points[b].pos))],
                            Entity::Circle { c, .. } | Entity::Arc { c, .. } => [Some((c, self.sketch.points[c].pos)), None],
                        },
                        Pick::Cons(_) => [None, None],
                    };
                    if points[0].is_some() {
                        self.checkpoint();
                        self.pending = Pending::Drag { points, grab };
                    }
                }
            }
        }
        if let Pending::Drag { points, grab } = self.pending {
            if primary_drag {
                if let Some(cur) = mouse.and_then(|m| self.local(cam, rect, m)) {
                    let mut delta = [cur[0] - grab[0], cur[1] - grab[1]];
                    // одна точка — с привязкой к сетке/точкам
                    if points[1].is_none() {
                        if let (Some((_, p0)), Some(s)) = (points[0], self.cursor) {
                            delta = [s.pos[0] - p0[0], s.pos[1] - p0[1]];
                        }
                    }
                    let drag: Vec<(usize, P2)> = points.iter().flatten().map(|&(p, o)| (p, [o[0] + delta[0], o[1] + delta[1]])).collect();
                    let _ = self.sketch.solve(params, &drag);
                    self.regions = self.sketch.regions();
                }
            } else {
                // отпустили: при наведении на другую точку — совмещаем
                if let (Some((p, _)), None, Some(Snap { point: Some(q), .. })) = (points[0], points[1], self.cursor) {
                    if q != p {
                        self.sketch.merge_points(p, q);
                    }
                }
                self.pending = Pending::None;
                self.commit_checked(params, tr("Перемещение"));
            }
            return SketchAction::None;
        }
        if primary_drag || resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle) {
            return SketchAction::None; // вращение/пан обрабатывает окно
        }
        if resp.secondary_clicked() {
            // правая кнопка завершает цепочку линий/отменяет действие
            self.cancel_pending();
            return SketchAction::None;
        }
        if resp.double_clicked() {
            if let Some(Pick::Cons(i)) = self.hover {
                self.open_dim_editor(i);
                return SketchAction::None;
            }
            if matches!(self.pending, Pending::Line { .. }) {
                self.cancel_pending();
                return SketchAction::None;
            }
        }
        if resp.clicked() {
            let shift = ctx.input(|i| i.modifiers.shift);
            self.click(params, shift);
        }
        SketchAction::None
    }

    fn finish_polygon(&mut self, c: P2, pt: Option<usize>, to: P2, params: &Params) {
        let r = DVec2::from(c).distance(DVec2::from(to));
        if r < 1e-9 {
            return;
        }
        self.checkpoint();
        let n = self.poly_sides.clamp(3, 64) as usize;
        let cp = pt.unwrap_or_else(|| self.sketch.add_point(c));
        // вспомогательная окружность держит вершины на одном радиусе, равные стороны делают многоугольник правильным
        let circ = self.sketch.add(Entity::Circle { c: cp, r, construction: true });
        let verts: Vec<usize> = polygon_pts(c, to, n).into_iter().map(|p| self.sketch.add_point(p)).collect();
        let lines: Vec<usize> = (0..n).map(|i| self.sketch.add(Entity::Line { a: verts[i], b: verts[(i + 1) % n], construction: false })).collect();
        for v in &verts {
            self.sketch.constraints.push(Constraint::PointOn(*v, circ));
        }
        for l in &lines[1..] {
            self.sketch.constraints.push(Constraint::Equal(lines[0], *l));
        }
        self.commit_checked(params, tr("Многоугольник"));
        self.pending = Pending::None;
    }

    fn finish_slot(&mut self, a: P2, pa: Option<usize>, b: P2, pb: Option<usize>, w: P2, params: &Params) {
        let Some((pts, r)) = slot_geometry(a, b, w) else { return };
        let _ = pts;
        self.checkpoint();
        let (u, nrm) = slot_axes(a, b);
        let (c1, c2) = (pa.unwrap_or_else(|| self.sketch.add_point(a)), pb.unwrap_or_else(|| self.sketch.add_point(b)));
        let off = |c: P2, k: f64, m: f64| [c[0] + nrm[0] * r * k + u[0] * r * m, c[1] + nrm[1] * r * k + u[1] * r * m];
        let a1 = self.sketch.add_point(off(a, -1.0, 0.0));
        let b1 = self.sketch.add_point(off(b, -1.0, 0.0));
        let b2 = self.sketch.add_point(off(b, 1.0, 0.0));
        let a2 = self.sketch.add_point(off(a, 1.0, 0.0));
        self.sketch.add(Entity::Line { a: a1, b: b1, construction: false });
        self.sketch.add(Entity::Line { a: b2, b: a2, construction: false });
        self.sketch.add(Entity::Arc { c: c2, a: b1, b: b2, construction: false });
        self.sketch.add(Entity::Arc { c: c1, a: a2, b: a1, construction: false });
        self.commit_checked(params, tr("Паз"));
        self.pending = Pending::None;
    }

    fn accepts_typing(&self) -> bool {
        matches!(self.pending, Pending::Line { .. } | Pending::Rect { .. } | Pending::Circle { .. })
    }

    fn open_dim_editor(&mut self, i: usize) {
        if let Some(e) = self.sketch.constraints.get(i).and_then(|c| c.expr()) {
            self.edit_dim = Some((i, e.0.clone(), true));
        }
    }

    fn click(&mut self, params: &Params, shift: bool) {
        let Some(s) = self.cursor else { return };
        self.message = None;
        match self.tool {
            Tool::Select => {
                match self.hover {
                    Some(p) => {
                        if shift {
                            if let Some(i) = self.selection.iter().position(|x| *x == p) {
                                self.selection.remove(i);
                            } else {
                                self.selection.push(p);
                            }
                        } else {
                            self.selection = vec![p];
                        }
                    }
                    None if !shift => self.selection.clear(),
                    None => {}
                }
            }
            Tool::Line => match self.pending {
                Pending::None => {
                    self.checkpoint();
                    let p = self.point_for(s);
                    self.pending = Pending::Line { start: p, first: p };
                }
                Pending::Line { start, first } => self.finish_line(start, first, s, None, params),
                _ => {}
            },
            Tool::Rect => match self.pending {
                Pending::None => self.pending = Pending::Rect { p0: s.pos, pt: s.point },
                Pending::Rect { p0, pt } => self.finish_rect(p0, pt, s, [None, None], params),
                _ => {}
            },
            Tool::Circle => match self.pending {
                Pending::None => self.pending = Pending::Circle { c: s.pos, pt: s.point },
                Pending::Circle { c, pt } => {
                    let r = DVec2::from(c).distance(DVec2::from(s.pos));
                    self.finish_circle(c, pt, r, None, params);
                }
                _ => {}
            },
            Tool::Arc => match self.pending {
                Pending::None => self.pending = Pending::Arc1 { a: s.pos, pa: s.point },
                Pending::Arc1 { a, pa } => {
                    if DVec2::from(a).distance(DVec2::from(s.pos)) > 1e-9 {
                        self.pending = Pending::Arc2 { a, pa, b: s.pos, pb: s.point };
                    }
                }
                Pending::Arc2 { a, pa, b, pb } => self.finish_arc(a, pa, b, pb, s.pos, params),
                _ => {}
            },
            Tool::Polygon => match self.pending {
                Pending::None => self.pending = Pending::Polygon { c: s.pos, pt: s.point },
                Pending::Polygon { c, pt } => self.finish_polygon(c, pt, s.pos, params),
                _ => {}
            },
            Tool::Slot => match self.pending {
                Pending::None => self.pending = Pending::Slot1 { a: s.pos, pa: s.point },
                Pending::Slot1 { a, pa } => {
                    if DVec2::from(a).distance(DVec2::from(s.pos)) > 1e-9 {
                        self.pending = Pending::Slot2 { a, pa, b: s.pos, pb: s.point };
                    }
                }
                Pending::Slot2 { a, pa, b, pb } => self.finish_slot(a, pa, b, pb, s.pos, params),
                _ => {}
            },
            Tool::CenterRect => match self.pending {
                Pending::None => self.pending = Pending::CRect { c: s.pos, pt: s.point },
                Pending::CRect { c, pt } => {
                    let p0 = [2.0 * c[0] - s.pos[0], 2.0 * c[1] - s.pos[1]];
                    let _ = pt;
                    self.finish_rect(p0, None, Snap { pos: s.pos, point: None }, [None, None], params);
                }
                _ => {}
            },
            Tool::Trim => {
                if let (Some(Pick::Entity(e)), Some(c)) = (self.hover, self.cursor) {
                    self.checkpoint();
                    if self.sketch.trim(e, c.pos) {
                        self.selection.clear();
                        self.commit_checked(params, tr("Обрезка"));
                    } else {
                        self.history.pop();
                        self.message = Some(tr("Нечего обрезать: нет пересечений").into());
                    }
                }
            }
            Tool::Offset => {
                if let (Some(Pick::Entity(e)), Some(c)) = (self.hover, self.cursor) {
                    self.checkpoint();
                    if self.sketch.offset(e, c.pos, self.offset_dist).is_some() {
                        self.commit_checked(params, tr("Смещение"));
                    } else {
                        self.history.pop();
                        self.message = Some(tr("Не удаётся сместить на такое расстояние").into());
                    }
                }
            }
            Tool::Corner => {
                if let Some(Pick::Point(p)) = self.hover {
                    self.checkpoint();
                    match self.sketch.fillet_corner(p, self.corner_r) {
                        Ok(()) => {
                            self.selection.clear();
                            self.commit_checked(params, tr("Скругление угла"));
                        }
                        Err(_) => {
                            self.history.pop();
                            self.message = Some(tr("Здесь нельзя скруглить: нужны две линии и радиус меньше их длины").into());
                        }
                    }
                }
            }
            Tool::Dimension => self.click_dimension(params),
            Tool::Angle => self.click_angle(params),
            Tool::Cons(k) => self.click_constraint(k, params),
        }
    }

    fn finish_line(&mut self, start: usize, first: usize, s: Snap, length: Option<Expr>, params: &Params) {
        let end_pos = s.pos;
        if DVec2::from(self.sketch.points[start].pos).distance(DVec2::from(end_pos)) < 1e-9 {
            return;
        }
        let end = match s.point {
            Some(p) if p != start => p,
            _ => self.sketch.add_point(end_pos),
        };
        let l = self.sketch.add(Entity::Line { a: start, b: end, construction: false });
        // авто-горизонталь/вертикаль при почти ровной линии
        let (pa, pb) = (self.sketch.p(start), self.sketch.p(end));
        let d = pb - pa;
        let ang = d.y.atan2(d.x).to_degrees().abs();
        if ang < 1.5 || (180.0 - ang).abs() < 1.5 {
            self.sketch.constraints.push(Constraint::Horizontal(l));
        } else if (ang - 90.0).abs() < 1.5 {
            self.sketch.constraints.push(Constraint::Vertical(l));
        }
        if let Some(e) = length {
            self.sketch.constraints.push(Constraint::Length(l, e));
        }
        self.commit_checked(params, tr("Линия"));
        self.typed = Default::default();
        if end == first {
            self.pending = Pending::None; // контур замкнулся
        } else {
            self.checkpoint();
            self.pending = Pending::Line { start: end, first };
        }
    }

    fn finish_rect(&mut self, p0: P2, pt: Option<usize>, s: Snap, dims: [Option<Expr>; 2], params: &Params) {
        let p1 = s.pos;
        if (p1[0] - p0[0]).abs() < 1e-9 || (p1[1] - p0[1]).abs() < 1e-9 {
            return;
        }
        self.checkpoint();
        let l = self.sketch.add_rect(p0, p1);
        let corner = |sk: &Sketch, li: usize| match sk.entities[li] {
            Entity::Line { a, .. } => a,
            _ => unreachable!(),
        };
        if let Some(e) = dims[0].clone() {
            self.sketch.constraints.push(Constraint::Length(l[0], e));
        }
        if let Some(e) = dims[1].clone() {
            self.sketch.constraints.push(Constraint::Length(l[1], e));
        }
        // привязанные углы совмещаем с существующими точками (сначала больший индекс)
        let mut merges = vec![];
        if let Some(q) = pt {
            merges.push((corner(&self.sketch, l[0]), q));
        }
        if let Some(q) = s.point {
            merges.push((corner(&self.sketch, l[2]), q));
        }
        merges.sort_by(|a, b| b.0.cmp(&a.0));
        for (from, into) in merges {
            self.sketch.merge_points(from, into);
        }
        self.commit_checked(params, tr("Прямоугольник"));
        self.pending = Pending::None;
        self.typed = Default::default();
        self.typed_field = 0;
    }

    fn finish_circle(&mut self, c: P2, pt: Option<usize>, r: f64, dia: Option<Expr>, params: &Params) {
        if r < 1e-9 {
            return;
        }
        self.checkpoint();
        let cp = pt.unwrap_or_else(|| self.sketch.add_point(c));
        let e = self.sketch.add(Entity::Circle { c: cp, r, construction: false });
        if let Some(d) = dia {
            self.sketch.constraints.push(Constraint::Diameter(e, d));
        }
        self.commit_checked(params, tr("Окружность"));
        self.pending = Pending::None;
        self.typed = Default::default();
    }

    fn finish_arc(&mut self, a: P2, pa: Option<usize>, b: P2, pb: Option<usize>, m: P2, params: &Params) {
        let Some(center) = circumcenter(a, b, m) else {
            self.message = Some(tr("Точки лежат на одной прямой — дуга невозможна").into());
            return;
        };
        self.checkpoint();
        let ia = pa.unwrap_or_else(|| self.sketch.add_point(a));
        let ib = pb.unwrap_or_else(|| self.sketch.add_point(b));
        let ic = self.sketch.add_point(center);
        // дуга хранится против часовой: если средняя точка не на дуге a→b, меняем концы
        let ang = |p: P2| (p[1] - center[1]).atan2(p[0] - center[0]);
        let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
        let through = ccw(ang(a), ang(m)) < ccw(ang(a), ang(b));
        let (s, e) = if through { (ia, ib) } else { (ib, ia) };
        self.sketch.add(Entity::Arc { c: ic, a: s, b: e, construction: false });
        self.commit_checked(params, tr("Дуга"));
        self.pending = Pending::None;
    }

    fn apply_typed(&mut self, params: &Params) {
        let parse = |t: &str| -> Option<(Expr, f64)> {
            let t = t.trim();
            if t.is_empty() {
                return None;
            }
            params.eval(t).ok().filter(|v| *v > 0.0).map(|v| (Expr(t.to_string()), v))
        };
        let Some(cur) = self.cursor else { return };
        match self.pending {
            Pending::Line { start, first } => {
                let Some((e, len)) = parse(&self.typed[0]) else {
                    self.message = Some(tr("Неверная длина").into());
                    return;
                };
                let p0 = self.sketch.p(start);
                let dir = (DVec2::from(cur.pos) - p0).normalize_or(DVec2::X);
                let end = p0 + dir * len;
                self.finish_line(start, first, Snap { pos: end.to_array(), point: None }, Some(e), params);
            }
            Pending::Rect { p0, pt } => {
                let w = parse(&self.typed[0]);
                let h = parse(&self.typed[1]);
                let sx = if cur.pos[0] >= p0[0] { 1.0 } else { -1.0 };
                let sy = if cur.pos[1] >= p0[1] { 1.0 } else { -1.0 };
                let x = w.as_ref().map(|w| p0[0] + sx * w.1).unwrap_or(cur.pos[0]);
                let y = h.as_ref().map(|h| p0[1] + sy * h.1).unwrap_or(cur.pos[1]);
                self.finish_rect(p0, pt, Snap { pos: [x, y], point: None }, [w.map(|w| w.0), h.map(|h| h.0)], params);
            }
            Pending::Circle { c, pt } => {
                let Some((e, d)) = parse(&self.typed[0]) else {
                    self.message = Some(tr("Неверный диаметр").into());
                    return;
                };
                self.finish_circle(c, pt, d / 2.0, Some(e), params);
            }
            _ => {}
        }
    }

    fn add_dimension(&mut self, c: Constraint, params: &Params) {
        self.checkpoint();
        self.sketch.constraints.push(c);
        if self.commit_checked(params, tr("Размер")) {
            let i = self.sketch.constraints.len() - 1;
            self.open_dim_editor(i);
            if let Some(ed) = &mut self.edit_dim {
                ed.2 = true;
            }
        }
    }

    fn click_dimension(&mut self, params: &Params) {
        let num = |v: f64| Expr::num((v * 1000.0).round() / 1000.0);
        match (self.pending, self.hover) {
            (Pending::None, Some(Pick::Entity(e))) => match self.sketch.entities[e] {
                Entity::Line { .. } => {
                    let v = self.sketch.measure(e);
                    self.add_dimension(Constraint::Length(e, num(v)), params);
                }
                _ => {
                    let v = self.sketch.measure(e);
                    self.add_dimension(Constraint::Diameter(e, num(v)), params);
                }
            },
            (Pending::None, Some(Pick::Point(p))) => self.pending = Pending::First(Pick::Point(p)),
            (Pending::First(Pick::Point(a)), Some(Pick::Point(b))) if a != b => {
                self.pending = Pending::None;
                let (pa, pb) = (self.sketch.p(a), self.sketch.p(b));
                let d = pb - pa;
                // почти горизонтально/вертикально — размер по оси, иначе по прямой
                let c = if d.y.abs() < d.x.abs() * 0.02 {
                    Constraint::HDistance(a, b, num(d.x.abs()))
                } else if d.x.abs() < d.y.abs() * 0.02 {
                    Constraint::VDistance(a, b, num(d.y.abs()))
                } else {
                    Constraint::Distance(a, b, num(d.length()))
                };
                self.add_dimension(c, params);
            }
            _ => {}
        }
    }

    fn click_angle(&mut self, params: &Params) {
        let is_line = |s: &Sketch, e: usize| matches!(s.entities[e], Entity::Line { .. });
        match (self.pending, self.hover) {
            (Pending::None, Some(Pick::Entity(e))) if is_line(&self.sketch, e) => self.pending = Pending::First(Pick::Entity(e)),
            (Pending::First(Pick::Entity(a)), Some(Pick::Entity(b))) if a != b && is_line(&self.sketch, b) => {
                self.pending = Pending::None;
                let dir = |e: usize| match self.sketch.entities[e] {
                    Entity::Line { a, b, .. } => self.sketch.p(b) - self.sketch.p(a),
                    _ => DVec2::X,
                };
                let (u, w) = (dir(a), dir(b));
                let ang = u.perp_dot(w).atan2(u.dot(w)).abs().to_degrees();
                self.add_dimension(Constraint::Angle(a, b, Expr::num((ang * 100.0).round() / 100.0)), params);
            }
            _ => self.message = Some(tr("Нужно щёлкнуть линию").into()),
        }
    }

    fn click_constraint(&mut self, k: ConsKind, params: &Params) {
        let ent = |s: &Sketch, e: usize| s.entities[e];
        let is_line = |e: Entity| matches!(e, Entity::Line { .. });
        let is_round = |e: Entity| matches!(e, Entity::Circle { .. } | Entity::Arc { .. });
        let Some(h) = self.hover else {
            self.message = Some(k.hint().into());
            return;
        };
        let add = |me: &mut Self, c: Constraint| {
            me.checkpoint();
            me.sketch.constraints.push(c);
            me.commit_checked(params, k.label());
            me.pending = Pending::None;
        };
        match (k, self.pending, h) {
            (ConsKind::Horizontal, _, Pick::Entity(e)) if is_line(ent(&self.sketch, e)) => add(self, Constraint::Horizontal(e)),
            (ConsKind::Vertical, _, Pick::Entity(e)) if is_line(ent(&self.sketch, e)) => add(self, Constraint::Vertical(e)),
            (ConsKind::Fix, _, Pick::Point(p)) => {
                self.checkpoint();
                if let Some(i) = self.sketch.constraints.iter().position(|c| matches!(c, Constraint::Fix(q, _) if *q == p)) {
                    self.sketch.constraints.remove(i);
                    self.resolve(params);
                } else {
                    let pos = self.sketch.points[p].pos;
                    self.sketch.constraints.push(Constraint::Fix(p, pos));
                    self.commit_checked(params, tr("Фиксация"));
                }
            }
            (ConsKind::Coincident | ConsKind::Midpoint, Pending::None, Pick::Point(p)) => self.pending = Pending::First(Pick::Point(p)),
            (ConsKind::Coincident, Pending::First(Pick::Point(a)), Pick::Point(b)) if a != b => {
                self.checkpoint();
                self.sketch.merge_points(a, b);
                self.commit_checked(params, tr("Совпадение"));
                self.pending = Pending::None;
            }
            (ConsKind::Coincident, Pending::First(Pick::Point(a)), Pick::Entity(e)) => add(self, Constraint::PointOn(a, e)),
            (ConsKind::Midpoint, Pending::First(Pick::Point(a)), Pick::Entity(e)) if is_line(ent(&self.sketch, e)) => {
                add(self, Constraint::Midpoint(a, e))
            }
            (ConsKind::Parallel | ConsKind::Perpendicular | ConsKind::Tangent | ConsKind::Equal | ConsKind::Concentric, Pending::None, Pick::Entity(e)) => {
                self.pending = Pending::First(Pick::Entity(e))
            }
            (_, Pending::First(Pick::Entity(a)), Pick::Entity(b)) if a != b => {
                let (ea, eb) = (ent(&self.sketch, a), ent(&self.sketch, b));
                let c = match k {
                    ConsKind::Parallel if is_line(ea) && is_line(eb) => Some(Constraint::Parallel(a, b)),
                    ConsKind::Perpendicular if is_line(ea) && is_line(eb) => Some(Constraint::Perpendicular(a, b)),
                    ConsKind::Tangent if (is_line(ea) && is_round(eb)) || (is_round(ea) && is_line(eb)) || (is_round(ea) && is_round(eb)) => {
                        Some(Constraint::Tangent(a, b))
                    }
                    ConsKind::Equal if (is_line(ea) && is_line(eb)) || (is_round(ea) && is_round(eb)) => Some(Constraint::Equal(a, b)),
                    ConsKind::Concentric if is_round(ea) && is_round(eb) => Some(Constraint::Concentric(a, b)),
                    _ => None,
                };
                match c {
                    Some(c) => add(self, c),
                    None => {
                        self.pending = Pending::None;
                        self.message = Some(format!("{}: {}", k.label(), k.hint()));
                    }
                }
            }
            _ => self.message = Some(k.hint().into()),
        }
    }

    pub fn delete_selection(&mut self, params: &Params) {
        if self.selection.is_empty() {
            return;
        }
        self.checkpoint();
        let mut ents: Vec<usize> = Vec::new();
        let mut cons: Vec<usize> = Vec::new();
        for p in &self.selection {
            match *p {
                Pick::Entity(e) => ents.push(e),
                Pick::Point(pt) => {
                    for (i, e) in self.sketch.entities.iter().enumerate() {
                        if e.points().contains(&pt) {
                            ents.push(i);
                        }
                    }
                }
                Pick::Cons(c) => cons.push(c),
            }
        }
        cons.sort_unstable();
        cons.dedup();
        for c in cons.into_iter().rev() {
            self.sketch.remove_constraint(c);
        }
        ents.sort_unstable();
        ents.dedup();
        self.sketch.remove_entities(&ents);
        self.selection.clear();
        self.resolve(params);
    }

    pub fn toggle_construction(&mut self, params: &Params) {
        let ents: Vec<usize> = self.selection.iter().filter_map(|p| if let Pick::Entity(e) = p { Some(*e) } else { None }).collect();
        if ents.is_empty() {
            self.message = Some(tr("Выберите линии/окружности, чтобы сделать их вспомогательными (X)").into());
            return;
        }
        self.checkpoint();
        for e in ents {
            let c = self.sketch.entities[e].construction();
            self.sketch.entities[e].set_construction(!c);
        }
        self.resolve(params);
    }

    // -------------------------------------------------------------- панель инструментов

    pub fn toolbar(&mut self, ui: &mut egui::Ui) -> SketchAction {
        let mut action = SketchAction::None;
        let tool_btn = |ui: &mut egui::Ui, me: &mut Self, t: Tool, label: &str, key: &str| {
            let sel = me.tool == t;
            let icon_id = match t {
                Tool::Select => "sk_select",
                Tool::Line => "sk_line",
                Tool::Rect => "sk_rect",
                Tool::Circle => "sk_circle",
                Tool::Arc => "sk_arc",
                Tool::Polygon => "sk_polygon",
                Tool::Slot => "sk_slot",
                Tool::CenterRect => "sk_rect_center",
                Tool::Trim => "sk_trim",
                Tool::Offset => "sk_offset",
                Tool::Corner => "sk_corner",
                Tool::Dimension => "sk_dim",
                Tool::Angle => "sk_angle",
                Tool::Cons(_) => "",
            };
            let main = if sel { Color32::WHITE } else { ui.visuals().text_color() };
            let text = label.split_once(' ').map(|x| x.1).unwrap_or(label);
            let btn = match crate::icons::icon(icon_id, 24.0, main) {
                Some(img) => egui::Button::image(img),
                None => egui::Button::new(label),
            };
            let r = ui.add(btn.selected(sel).min_size(egui::vec2(44.0, 42.0))).on_hover_text(text);
            let r = if key.is_empty() { r } else { r.on_hover_text(tf!("Клавиша {}", key)) };
            let r = match t {
                Tool::CenterRect => r.on_hover_text(tr("Прямоугольник от центра")),
                Tool::Corner => r.on_hover_text(tr("Скруглить угол между двумя линиями")),
                _ => r,
            };
            if r.clicked() {
                me.set_tool(t);
            }
        };
        ui.horizontal(|ui| {
            group(ui, tr("СТРОИТЬ"), |ui| {
                tool_btn(ui, self, Tool::Select, tr("↖ Выбор"), "Esc");
                tool_btn(ui, self, Tool::Line, tr("╱ Линия"), "L");
                tool_btn(ui, self, Tool::Rect, tr("▭ Прямоуг."), "R");
                tool_btn(ui, self, Tool::Circle, tr("○ Окружность"), "C");
                tool_btn(ui, self, Tool::Arc, tr("◠ Дуга"), "A");
                tool_btn(ui, self, Tool::Polygon, tr("◇ Многоуг."), "P");
                tool_btn(ui, self, Tool::Slot, tr("▬ Паз"), "O");
                tool_btn(ui, self, Tool::CenterRect, tr("▣ Из центра"), "");
            });
            ui.separator();
            group(ui, tr("ИМПОРТ"), |ui| {
                ui.menu_button(tr("⤓ Файлы ▾"), |ui| {
                if ui.button(tr("🖼 Фото → чертёж")).on_hover_text(tr("Превратить картинку (PNG, JPG…) в контуры чертежа")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter(tr("Изображения"), &["png", "jpg", "jpeg", "bmp", "gif", "webp"]).pick_file() {
                        match crate::trace_ui::TraceDialog::load(&path) {
                            Ok(d) => self.trace = Some(d),
                            Err(e) => self.message = Some(tf!("Не удалось открыть картинку: {}", e)),
                        }
                    }
                }
                if ui.button(tr("📐 Импорт DXF")).on_hover_text(tr("Добавить в чертёж линии, окружности и дуги из файла DXF")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("DXF", &["dxf"]).pick_file() {
                        match std::fs::read_to_string(&path) {
                            Ok(text) => {
                                self.checkpoint();
                                match fusion_core::dxf::import(&mut self.sketch, &text) {
                                    Ok(n) => {
                                        self.pending_resolve = true;
                                        self.message = Some(tf!("Добавлено кривых: {}", n));
                                    }
                                    Err(e) => {
                                        self.history.pop();
                                        self.message = Some(tf!("Не удалось прочитать DXF: {}", tr_dyn(&e)));
                                    }
                                }
                            }
                            Err(e) => self.message = Some(tf!("Не удалось прочитать DXF: {}", e)),
                        }
                    }
                }
                if ui.button(tr("📤 Экспорт DXF")).on_hover_text(tr("Сохранить чертёж в файл DXF")).clicked() {
                    if let Some(path) = rfd::FileDialog::new().add_filter("DXF", &["dxf"]).set_file_name("blueprint.dxf").save_file() {
                        self.message = Some(match std::fs::write(&path, fusion_core::dxf::export(&self.sketch)) {
                            Ok(()) => tr("Чертёж сохранён в DXF").into(),
                            Err(e) => tf!("Не удалось сохранить: {}", e),
                        });
                    }
                }
                });
            });
            ui.separator();
            group(ui, tr("ПРАВИТЬ"), |ui| {
                tool_btn(ui, self, Tool::Trim, tr("✂ Обрезать"), "T");
                tool_btn(ui, self, Tool::Offset, tr("⧉ Смещение"), "");
                tool_btn(ui, self, Tool::Corner, tr("◜ Скругл. угол"), "");
            });
            ui.separator();
            group(ui, tr("РАЗМЕРЫ"), |ui| {
                tool_btn(ui, self, Tool::Dimension, tr("↔ Размер"), "D");
                tool_btn(ui, self, Tool::Angle, tr("∠ Угол"), "");
            });
            ui.separator();
            group(ui, tr("ГОТОВО"), |ui| {
                let finish = egui::Button::new(RichText::new(tr("Завершить чертёж")).strong().color(Color32::WHITE))
                    .fill(Color32::from_rgb(40, 150, 70))
                    .min_size(egui::vec2(0.0, 26.0));
                if ui.add(finish).on_hover_text(tr("Сохранить чертёж и выйти")).clicked() {
                    action = SketchAction::Finish;
                }
                if ui.add(egui::Button::new(tr("Отмена")).min_size(egui::vec2(0.0, 26.0))).on_hover_text(tr("Выйти без сохранения изменений")).clicked() {
                    action = SketchAction::Cancel;
                }
            });
        });
        action
    }

    /// Палитра чертежа для панели справа: ограничения, настройки, состояние, выбранное.
    pub fn status_ui(&mut self, ui: &mut egui::Ui, params: &Params) {
        ui.label(RichText::new(tr("ОГРАНИЧЕНИЯ")).small().weak());
        egui::Grid::new("cons_grid").num_columns(2).spacing([4.0, 4.0]).show(ui, |ui| {
            for (i, k) in ConsKind::ALL.iter().enumerate() {
                let sel = self.tool == Tool::Cons(*k);
                let cid = match k {
                    ConsKind::Horizontal => "c_horizontal",
                    ConsKind::Vertical => "c_vertical",
                    ConsKind::Parallel => "c_parallel",
                    ConsKind::Perpendicular => "c_perpendicular",
                    ConsKind::Tangent => "c_tangent",
                    ConsKind::Equal => "c_equal",
                    ConsKind::Concentric => "c_concentric",
                    ConsKind::Coincident => "c_coincident",
                    ConsKind::Midpoint => "c_midpoint",
                    ConsKind::Fix => "c_fix",
                };
                let main = if sel { Color32::WHITE } else { ui.visuals().text_color() };
                let cbtn = match crate::icons::icon(cid, 18.0, main) {
                    Some(img) => egui::Button::image_and_text(img, k.label()),
                    None => egui::Button::new(k.label()),
                };
                if ui.add_sized([124.0, 30.0], cbtn.selected(sel)).on_hover_text(k.hint()).clicked() {
                    self.set_tool(Tool::Cons(*k));
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(6.0);
        if self.tool == Tool::Polygon {
            ui.horizontal(|ui| {
                ui.label(tr("Сторон"));
                ui.add(egui::DragValue::new(&mut self.poly_sides).range(3..=64));
            });
        }
        if self.tool == Tool::Offset {
            ui.horizontal(|ui| {
                ui.label(tr("Расстояние"));
                ui.add(egui::DragValue::new(&mut self.offset_dist).range(0.01..=100000.0).speed(0.2));
            });
        }
        if self.tool == Tool::Corner {
            ui.horizontal(|ui| {
                ui.label(tr("Радиус"));
                ui.add(egui::DragValue::new(&mut self.corner_r).range(0.01..=100000.0).speed(0.2));
            });
        }
        ui.label(RichText::new(tr("ПАРАМЕТРЫ")).small().weak());
        ui.checkbox(&mut self.snap_grid, tr("Привязка к сетке"));
        ui.checkbox(&mut self.show_constraints, tr("Показывать значки ограничений"));
        if ui.button(tr("Вспомогательная линия (X)")).on_hover_text(tr("Выбранные кривые станут вспомогательными (не образуют профилей) и обратно")).clicked() {
            self.toggle_construction(params);
        }
        ui.separator();
        match self.status {
            SolveStatus::Ok if self.sketch.entities.is_empty() => {
                ui.label(RichText::new(tr("Чертёж пуст — нарисуйте контур")).weak());
            }
            SolveStatus::Ok if self.dof == 0 => {
                ui.label(RichText::new(tr("● Чертёж полностью определён")).color(Color32::from_rgb(30, 140, 60)));
            }
            SolveStatus::Ok => {
                ui.label(tf!("Свободных степеней: {}", self.dof)).on_hover_text(tr("Добавьте размеры и ограничения, чтобы чертёж был полностью определён"));
            }
            SolveStatus::Conflict(_) => {
                ui.label(RichText::new(tr("⚠ Ограничения противоречат друг другу")).color(Color32::from_rgb(200, 40, 40)));
            }
        }
        ui.label(tf!("Профилей: {}", self.regions.len()));
        if !self.selection.is_empty() {
            ui.separator();
            let sel = self.selection.clone();
            for p in sel {
                match p {
                    Pick::Entity(e) => {
                        let s = &self.sketch;
                        let txt = match s.entities[e] {
                            Entity::Line { .. } => tf!("Линия, длина {}", format_num(round3(s.measure(e)))),
                            Entity::Circle { .. } => tf!("Окружность Ø{}", format_num(round3(s.measure(e)))),
                            Entity::Arc { .. } => tf!("Дуга R{}", format_num(round3(s.measure(e) / 2.0))),
                        };
                        ui.label(txt);
                    }
                    Pick::Point(p) => {
                        let q = self.sketch.points[p].pos;
                        ui.label(tf!("Точка ({}; {})", format_num(round3(q[0])), format_num(round3(q[1]))));
                    }
                    Pick::Cons(c) => {
                        if let Some(cons) = self.sketch.constraints.get(c) {
                            ui.label(cons.label());
                            if cons.is_dimension() {
                                let mut e = cons.expr().unwrap().clone();
                                if crate::widgets::expr_field(ui, ("dimsel", c), &mut e, params, "") {
                                    self.checkpoint();
                                    *self.sketch.constraints[c].expr_mut().unwrap() = e;
                                    self.commit_checked(params, tr("Размер"));
                                }
                            }
                        }
                    }
                }
            }
            if ui.button(tr("Удалить выбранное")).clicked() {
                self.delete_selection(params);
            }
        }
    }

    // -------------------------------------------------------------- отрисовка

    pub fn overlay(&self, ov: &mut Overlay, cam: &Camera, rect: Rect, c: &ViewColors) {
        let pl = &self.plane;
        let wpp = cam.world_per_pixel(rect);
        // сетка вокруг центра обзора
        let center = pl.to_local(cam.target);
        let ext = (cam.distance * 1.2).max(wpp * 800.0);
        // на грани тела сетка лежит в одной плоскости с гранью — рисуем её поверх, иначе её не видно
        let on_face = *pl != Plane::XY && *pl != Plane::XZ && *pl != Plane::YZ;
        let mut g = Vec::new();
        // на светлой грани тела обычная (светлая) сетка не видна — рисуем тёмную
        let (gm, gM) = if on_face { (Color32::from_rgba_unmultiplied(25, 35, 50, 150), Color32::from_rgba_unmultiplied(25, 35, 50, 230)) } else { (c.grid_minor, c.grid_major) };
        render::grid_adaptive(&mut g, pl, center, wpp, ext, gm, gM);
        if on_face { ov.lines_top.extend(g) } else { ov.lines.extend(g) }
        ov.lines.push(render::line(pl.to_world([-ext, 0.0]), pl.to_world([ext, 0.0]), Color32::from_rgb(210, 60, 60), 1.2));
        ov.lines.push(render::line(pl.to_world([0.0, -ext]), pl.to_world([0.0, ext]), Color32::from_rgb(60, 170, 60), 1.2));

        for r in &self.regions {
            fill_region(&mut ov.tris_top, pl, r, c.profile);
        }
        let full = matches!(self.status, SolveStatus::Ok) && self.dof == 0;
        for (i, e) in self.sketch.entities.iter().enumerate() {
            let pick = Pick::Entity(i);
            let sel = self.selection.contains(&pick);
            let hov = self.hover == Some(pick);
            let col = if sel {
                c.sketch_selected
            } else if hov {
                c.sketch_hover
            } else if e.construction() {
                c.sketch_construction
            } else if full {
                c.sketch_constrained
            } else {
                c.sketch
            };
            let w = if sel || hov { 2.6 } else { 1.8 };
            let pts: Vec<DVec3> = self.sketch.tessellate(i).iter().map(|p| pl.to_world(*p)).collect();
            if e.construction() {
                for s in pts.windows(2) {
                    dashed(&mut ov.lines_top, s[0], s[1], wpp * 6.0, col, w * 0.8);
                }
            } else {
                polyline(&mut ov.lines_top, &pts, col, w, false);
            }
        }
        // предпросмотр текущего инструмента
        let Some(cur) = self.cursor else { return };
        let pv = c.sketch_hover;
        let seg = |ov: &mut Overlay, a: P2, b: P2| ov.lines_top.push(render::line(pl.to_world(a), pl.to_world(b), pv, 1.6));
        match self.pending {
            Pending::Line { start, .. } => {
                let a = self.sketch.points[start].pos;
                let b = self.typed_line_end(a, cur.pos);
                seg(ov, a, b);
            }
            Pending::Rect { p0, .. } => {
                let p1 = self.typed_rect_corner(p0, cur.pos);
                let q = [p0, [p1[0], p0[1]], p1, [p0[0], p1[1]]];
                for i in 0..4 {
                    seg(ov, q[i], q[(i + 1) % 4]);
                }
            }
            Pending::Circle { c: cc, .. } => {
                let r = self.typed_radius(cc, cur.pos);
                let n = 72;
                let pts: Vec<DVec3> = (0..=n)
                    .map(|i| {
                        let t = std::f64::consts::TAU * i as f64 / n as f64;
                        pl.to_world([cc[0] + r * t.cos(), cc[1] + r * t.sin()])
                    })
                    .collect();
                polyline(&mut ov.lines_top, &pts, pv, 1.6, false);
            }
            Pending::Polygon { c: cc, .. } => {
                let pts = polygon_pts(cc, cur.pos, self.poly_sides.clamp(3, 64) as usize);
                let w: Vec<DVec3> = pts.iter().map(|p| pl.to_world(*p)).collect();
                polyline(&mut ov.lines_top, &w, pv, 1.6, true);
            }
            Pending::Slot1 { a, .. } => seg(ov, a, cur.pos),
            Pending::Slot2 { a, b, .. } => {
                if let Some((pts, _)) = slot_geometry(a, b, cur.pos) {
                    let w: Vec<DVec3> = pts.iter().map(|p| pl.to_world(*p)).collect();
                    polyline(&mut ov.lines_top, &w, pv, 1.6, true);
                }
            }
            Pending::CRect { c: cc, .. } => {
                let p1 = cur.pos;
                let p0 = [2.0 * cc[0] - p1[0], 2.0 * cc[1] - p1[1]];
                let q = [p0, [p1[0], p0[1]], p1, [p0[0], p1[1]]];
                for i in 0..4 {
                    seg(ov, q[i], q[(i + 1) % 4]);
                }
            }
            Pending::Arc1 { a, .. } => seg(ov, a, cur.pos),
            Pending::Arc2 { a, b, .. } => {
                if let Some(cc) = circumcenter(a, b, cur.pos) {
                    let ang = |p: P2| (p[1] - cc[1]).atan2(p[0] - cc[0]);
                    let ccw = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
                    let (s, e) = if ccw(ang(a), ang(cur.pos)) < ccw(ang(a), ang(b)) { (a, b) } else { (b, a) };
                    let r = DVec2::from(cc).distance(DVec2::from(a));
                    let a0 = ang(s);
                    let sweep = ccw(a0, ang(e));
                    let n = 48;
                    let pts: Vec<DVec3> = (0..=n)
                        .map(|i| {
                            let t = a0 + sweep * i as f64 / n as f64;
                            pl.to_world([cc[0] + r * t.cos(), cc[1] + r * t.sin()])
                        })
                        .collect();
                    polyline(&mut ov.lines_top, &pts, pv, 1.6, false);
                } else {
                    seg(ov, a, b);
                }
            }
            _ => {}
        }
    }

    fn typed_value(&self, i: usize) -> Option<f64> {
        let t = self.typed[i].trim();
        if t.is_empty() {
            return None;
        }
        // параметры недоступны здесь — достаточно чисел для предпросмотра
        Params::default().eval(t).ok().filter(|v| *v > 0.0)
    }

    fn typed_line_end(&self, a: P2, cur: P2) -> P2 {
        match self.typed_value(0) {
            Some(len) => {
                let dir = (DVec2::from(cur) - DVec2::from(a)).normalize_or(DVec2::X);
                (DVec2::from(a) + dir * len).to_array()
            }
            None => cur,
        }
    }

    fn typed_rect_corner(&self, p0: P2, cur: P2) -> P2 {
        let sx = if cur[0] >= p0[0] { 1.0 } else { -1.0 };
        let sy = if cur[1] >= p0[1] { 1.0 } else { -1.0 };
        [
            self.typed_value(0).map(|w| p0[0] + sx * w).unwrap_or(cur[0]),
            self.typed_value(1).map(|h| p0[1] + sy * h).unwrap_or(cur[1]),
        ]
    }

    fn typed_radius(&self, c: P2, cur: P2) -> f64 {
        self.typed_value(0).map(|d| d / 2.0).unwrap_or_else(|| DVec2::from(c).distance(DVec2::from(cur)))
    }

    /// Точки, размеры, значки ограничений, поле набора — рисуются поверх 3D.
    pub fn paint(&mut self, ui: &mut egui::Ui, cam: &Camera, rect: Rect, c: &ViewColors, params: &Params) {
        if std::mem::take(&mut self.pending_resolve) {
            self.commit_checked(params, "DXF");
        }
        if let Some(mut d) = self.trace.take() {
            match d.ui(ui.ctx()) {
                crate::trace_ui::TraceResult::None => self.trace = Some(d),
                crate::trace_ui::TraceResult::Cancel => {}
                crate::trace_ui::TraceResult::Insert(loops) => {
                    self.checkpoint();
                    for l in &loops {
                        self.sketch.add_loop(l);
                    }
                    self.commit_checked(params, tr("Фото → чертёж"));
                    self.message = Some(tf!("Добавлено контуров: {}", loops.len()));
                }
            }
        }
        let painter = ui.painter_at(rect);
        let font = FontId::proportional(12.5);
        self.label_rects.clear();
        let scr = |p: P2| cam.project(rect, self.plane.to_world(p));

        // точки
        let fixed: Vec<usize> = self.sketch.constraints.iter().filter_map(|c| if let Constraint::Fix(p, _) = c { Some(*p) } else { None }).collect();
        for (i, p) in self.sketch.points.iter().enumerate() {
            let Some(s) = scr(p.pos) else { continue };
            let pick = Pick::Point(i);
            let col = if self.selection.contains(&pick) {
                c.sketch_selected
            } else if self.hover == Some(pick) {
                c.sketch_hover
            } else {
                c.sketch
            };
            let r = if self.hover == Some(pick) { 3.5 } else { 2.5 };
            painter.rect_filled(Rect::from_center_size(s, egui::vec2(r * 2.0, r * 2.0)), 0.0, col);
            if fixed.contains(&i) {
                painter.text(s + egui::vec2(6.0, -6.0), Align2::LEFT_BOTTOM, "🔒", FontId::proportional(10.0), c.dim_text);
            }
        }
        // привязка к точке
        if let Some(Snap { point: Some(p), .. }) = self.cursor {
            if self.tool != Tool::Select || matches!(self.pending, Pending::Drag { .. }) {
                if let Some(s) = scr(self.sketch.points[p].pos) {
                    painter.circle_stroke(s, 7.0, Stroke::new(1.5_f32, c.sketch_selected));
                }
            }
        }

        // размеры
        for (i, cons) in self.sketch.constraints.iter().enumerate() {
            let pick = Pick::Cons(i);
            let hl = self.selection.contains(&pick) || self.hover == Some(pick);
            // значки ограничений (=, •, ⟂…) показываем только по запросу, чтобы не захламлять чертёж
            if !cons.is_dimension() && !hl && !self.show_constraints && !matches!(self.tool, Tool::Cons(_)) {
                continue;
            }
            let col = if hl { c.sketch_selected } else { c.dim_text };
            let anchor = self.cons_anchor(cons, cam, rect, params);
            let Some((pos, text)) = anchor else { continue };
            // размерные линии со стрелками для расстояний
            self.dim_lines(&painter, cons, cam, rect, col);
            let galley = painter.layout_no_wrap(text, font.clone(), col);
            let mut r = Rect::from_center_size(pos, galley.size() + egui::vec2(8.0, 4.0));
            // раздвигаем подписи, если они налезают друг на друга
            for _ in 0..6 {
                match self.label_rects.iter().find(|(_, o)| o.intersects(r)) {
                    Some((_, o)) => r = r.translate(egui::vec2(0.0, o.bottom() - r.top() + 2.0)),
                    None => break,
                }
            }
            let fill = if cons.is_dimension() { ui.visuals().extreme_bg_color.gamma_multiply(0.9) } else { ui.visuals().faint_bg_color.gamma_multiply(0.85) };
            painter.rect(r, 3.0, fill, Stroke::new(if hl { 1.5_f32 } else { 0.6 }, col.gamma_multiply(0.7)), egui::StrokeKind::Inside);
            painter.galley(r.min + egui::vec2(4.0, 2.0), galley, col);
            self.label_rects.push((i, r));
        }

        // набираемое значение возле курсора
        if self.accepts_typing() {
            if let Some(m) = ui.ctx().pointer_hover_pos() {
                let label = match self.pending {
                    Pending::Rect { .. } => {
                        let f = |i: usize, name: &str| {
                            let v = if self.typed[i].is_empty() { "…".to_string() } else { self.typed[i].clone() };
                            if self.typed_field == i { format!("[{name} {v}]") } else { format!("{name} {v}") }
                        };
                        tf!("{}  {}   (Tab — переключить)", f(0, tr("Ш")), f(1, tr("В")))
                    }
                    Pending::Circle { c: cc, .. } => {
                        if self.typed[0].is_empty() {
                            format!("Ø {}", format_num(round3(2.0 * DVec2::from(cc).distance(DVec2::from(self.cursor.map(|s| s.pos).unwrap_or(cc))))))
                        } else {
                            format!("Ø {} ↵", self.typed[0])
                        }
                    }
                    Pending::Line { start, .. } => {
                        if self.typed[0].is_empty() {
                            let a = DVec2::from(self.sketch.points[start].pos);
                            let b = DVec2::from(self.cursor.map(|s| s.pos).unwrap_or(a.to_array()));
                            tf!("{} мм", format_num(round3(a.distance(b))))
                        } else {
                            format!("{} ↵", self.typed[0])
                        }
                    }
                    _ => String::new(),
                };
                let g = painter.layout_no_wrap(label, font.clone(), Color32::WHITE);
                let r = Rect::from_min_size(m + egui::vec2(16.0, 14.0), g.size() + egui::vec2(10.0, 6.0));
                painter.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(20, 40, 70, 220));
                painter.galley(r.min + egui::vec2(5.0, 3.0), g, Color32::WHITE);
            }
        }

        // редактор размера
        if let Some((i, mut text, first)) = self.edit_dim.take() {
            let pos = self.label_rects.iter().find(|(k, _)| *k == i).map(|(_, r)| r.center()).unwrap_or(rect.center());
            let mut keep = true;
            let mut apply = false;
            egui::Area::new(egui::Id::new("dim_editor")).fixed_pos(pos - egui::vec2(60.0, 12.0)).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let ok = params.eval(&text).map(|v| v > 0.0).unwrap_or(false);
                        let mut te = egui::TextEdit::singleline(&mut text).desired_width(110.0);
                        if !ok {
                            te = te.text_color(Color32::from_rgb(190, 0, 0));
                        }
                        let r = ui.add(te);
                        if first {
                            r.request_focus();
                            // выделяем всё, чтобы сразу набирать новое значение
                            if let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), r.id) {
                                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()))));
                                st.store(ui.ctx(), r.id);
                            }
                        }
                        if r.lost_focus() {
                            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && ok {
                                apply = true;
                            }
                            keep = false;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            keep = false;
                        }
                        ui.label(RichText::new("Enter").weak());
                    });
                });
            });
            if apply {
                self.checkpoint();
                if let Some(e) = self.sketch.constraints.get_mut(i).and_then(|c| c.expr_mut()) {
                    *e = Expr(text.trim().to_string());
                }
                self.commit_checked(params, tr("Размер"));
            } else if keep {
                self.edit_dim = Some((i, text, false));
            }
        }
    }

    /// Размерная линия со стрелками и выносными линиями (как на чертеже).
    fn dim_lines(&self, painter: &egui::Painter, cons: &Constraint, cam: &Camera, rect: Rect, col: Color32) {
        let s = &self.sketch;
        let scr = |p: DVec2| cam.project(rect, self.plane.to_world(p.to_array()));
        let (p0, p1, ext) = match cons {
            Constraint::HDistance(a, b, _) => {
                let (pa, pb) = (s.p(*a), s.p(*b));
                (DVec2::new(pa.x, pb.y), pb, Some(pa))
            }
            Constraint::VDistance(a, b, _) => {
                let (pa, pb) = (s.p(*a), s.p(*b));
                (DVec2::new(pb.x, pa.y), pb, Some(pa))
            }
            Constraint::Distance(a, b, _) => (s.p(*a), s.p(*b), None),
            _ => return,
        };
        let (Some(a), Some(b)) = (scr(p0), scr(p1)) else { return };
        let thin = Stroke::new(0.8_f32, col.gamma_multiply(0.6));
        if let Some(e) = ext.and_then(|e| scr(e)) {
            painter.line_segment([e, a], Stroke::new(0.6_f32, col.gamma_multiply(0.35)));
        }
        painter.line_segment([a, b], thin);
        let d = (b - a).normalized();
        if (b - a).length() > 16.0 {
            for (tip, dir) in [(a, d), (b, -d)] {
                let n = egui::vec2(-dir.y, dir.x);
                painter.add(egui::Shape::convex_polygon(vec![tip, tip + dir * 7.0 + n * 2.5, tip + dir * 7.0 - n * 2.5], col.gamma_multiply(0.7), Stroke::NONE));
            }
        }
    }

    /// Где и что подписать для ограничения.
    fn cons_anchor(&self, cons: &Constraint, cam: &Camera, rect: Rect, params: &Params) -> Option<(Pos2, String)> {
        let s = &self.sketch;
        let scr = |p: DVec2| cam.project(rect, self.plane.to_world(p.to_array()));
        let mid = |e: usize| -> Option<DVec2> {
            match s.entities.get(e)? {
                Entity::Line { a, b, .. } => Some((s.p(*a) + s.p(*b)) * 0.5),
                Entity::Circle { c, r, .. } => Some(s.p(*c) + DVec2::new(r * 0.7071, r * 0.7071)),
                Entity::Arc { .. } => {
                    let t = s.tessellate(e);
                    Some(DVec2::from(t[t.len() / 2]))
                }
            }
        };
        let normal_off = |e: usize, px: f32| -> egui::Vec2 {
            if let Some(Entity::Line { a, b, .. }) = s.entities.get(e) {
                if let (Some(pa), Some(pb)) = (scr(s.p(*a)), scr(s.p(*b))) {
                    let d = (pb - pa).normalized();
                    return egui::vec2(-d.y, d.x) * px;
                }
            }
            egui::vec2(px * 0.7, -px * 0.7)
        };
        let value = |e: &Expr| {
            let v = params.eval(&e.0).map(format_num).unwrap_or_else(|_| "?".into());
            if e.is_plain_number() { v } else { format!("{} = {v}", e.0) }
        };
        let fmt = |e: &Expr| value(e);
        use Constraint::*;
        Some(match cons {
            Length(e, x) => (scr(mid(*e)?)? + normal_off(*e, 16.0), fmt(x)),
            Diameter(e, x) => (scr(mid(*e)?)? + egui::vec2(14.0, -14.0), format!("Ø{}", fmt(x))),
            // размер по оси подписываем на уровне второй точки — так подписи разных точек не сливаются
            HDistance(a, b, x) => (scr(DVec2::new((s.p(*a).x + s.p(*b).x) * 0.5, s.p(*b).y))? + egui::vec2(0.0, 12.0), fmt(x)),
            VDistance(a, b, x) => (scr(DVec2::new(s.p(*b).x, (s.p(*a).y + s.p(*b).y) * 0.5))? + egui::vec2(-14.0, 0.0), fmt(x)),
            Distance(a, b, x) => {
                let m = (s.p(*a) + s.p(*b)) * 0.5;
                (scr(m)? + egui::vec2(0.0, -14.0), fmt(x))
            }
            Angle(a, b, x) => {
                let m = (mid(*a)? + mid(*b)?) * 0.5;
                (scr(m)?, format!("{}°", fmt(x)))
            }
            Horizontal(e) => (scr(mid(*e)?)? + normal_off(*e, -14.0), "—".into()),
            Vertical(e) => (scr(mid(*e)?)? + normal_off(*e, -14.0), "|".into()),
            Parallel(a, _) => (scr(mid(*a)?)? + normal_off(*a, -14.0) + egui::vec2(16.0, 0.0), "‖".into()),
            Perpendicular(a, _) => (scr(mid(*a)?)? + normal_off(*a, -14.0) + egui::vec2(-16.0, 0.0), "⟂".into()),
            Tangent(a, _) => (scr(mid(*a)?)? + egui::vec2(10.0, 10.0), "T".into()),
            Equal(a, _) => (scr(mid(*a)?)? + normal_off(*a, -14.0) + egui::vec2(0.0, 12.0), "=".into()),
            Concentric(a, _) => (scr(mid(*a)?)? + egui::vec2(-12.0, 12.0), "◎".into()),
            Midpoint(p, _) => (scr(s.p(*p))? + egui::vec2(10.0, 10.0), "△".into()),
            PointOn(p, _) => (scr(s.p(*p))? + egui::vec2(10.0, 10.0), "•".into()),
            Fix(..) => return None,
        })
    }
}

fn group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        if !title.is_empty() {
            ui.label(RichText::new(title).small().weak());
        }
        ui.horizontal(|ui| add(ui));
    });
}

fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

/// Вершины правильного многоугольника: центр, вершина-ориентир, число сторон.
fn polygon_pts(c: P2, to: P2, n: usize) -> Vec<P2> {
    let r = DVec2::from(c).distance(DVec2::from(to));
    let a0 = (to[1] - c[1]).atan2(to[0] - c[0]);
    (0..n)
        .map(|i| {
            let t = a0 + std::f64::consts::TAU * i as f64 / n as f64;
            [c[0] + r * t.cos(), c[1] + r * t.sin()]
        })
        .collect()
}

/// Оси паза: вдоль центров и перпендикулярно (против часовой).
fn slot_axes(a: P2, b: P2) -> (P2, P2) {
    let d = (DVec2::from(b) - DVec2::from(a)).normalize_or(DVec2::X);
    (d.to_array(), [-d.y, d.x])
}

/// Контур паза (ломаная) и радиус краёв; радиус — расстояние точки `w` до оси.
fn slot_geometry(a: P2, b: P2, w: P2) -> Option<(Vec<P2>, f64)> {
    let (u, n) = slot_axes(a, b);
    let r = ((w[0] - a[0]) * n[0] + (w[1] - a[1]) * n[1]).abs();
    if r < 1e-9 {
        return None;
    }
    let at = |c: P2, k: f64, m: f64| [c[0] + n[0] * r * k + u[0] * r * m, c[1] + n[1] * r * k + u[1] * r * m];
    let mut pts = vec![at(a, -1.0, 0.0), at(b, -1.0, 0.0)];
    for i in 1..16 {
        let t = std::f64::consts::PI * i as f64 / 16.0 - std::f64::consts::FRAC_PI_2;
        pts.push([b[0] + (u[0] * t.cos() + n[0] * t.sin()) * r, b[1] + (u[1] * t.cos() + n[1] * t.sin()) * r]);
    }
    pts.push(at(b, 1.0, 0.0));
    pts.push(at(a, 1.0, 0.0));
    for i in 1..16 {
        let t = std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * i as f64 / 16.0;
        pts.push([a[0] + (u[0] * t.cos() + n[0] * t.sin()) * r, a[1] + (u[1] * t.cos() + n[1] * t.sin()) * r]);
    }
    Some((pts, r))
}

fn seg_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

fn circumcenter(a: P2, b: P2, c: P2) -> Option<P2> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    let scale = (a[0].abs() + a[1].abs() + b[0].abs() + b[1].abs() + c[0].abs() + c[1].abs()).max(1.0);
    if d.abs() < 1e-9 * scale * scale {
        return None;
    }
    let sq = |p: P2| p[0] * p[0] + p[1] * p[1];
    let ux = (sq(a) * (b[1] - c[1]) + sq(b) * (c[1] - a[1]) + sq(c) * (a[1] - b[1])) / d;
    let uy = (sq(a) * (c[0] - b[0]) + sq(b) * (a[0] - c[0]) + sq(c) * (b[0] - a[0])) / d;
    Some([ux, uy])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_polygon_stays_regular_after_solving() {
        let mut sk = Sketch::default();
        let c = sk.add_point([0.0, 0.0]);
        let circ = sk.add(Entity::Circle { c, r: 10.0, construction: true });
        let verts: Vec<usize> = polygon_pts([0.0, 0.0], [10.0, 0.0], 6).into_iter().map(|p| sk.add_point(p)).collect();
        let lines: Vec<usize> = (0..6).map(|i| sk.add(Entity::Line { a: verts[i], b: verts[(i + 1) % 6], construction: false })).collect();
        for v in &verts {
            sk.constraints.push(Constraint::PointOn(*v, circ));
        }
        for l in &lines[1..] {
            sk.constraints.push(Constraint::Equal(lines[0], *l));
        }
        assert!(matches!(sk.solve(&Params::default(), &[]).unwrap(), fusion_core::sketch::SolveStatus::Ok));
        let r = sk.regions();
        assert_eq!(r.len(), 1);
        let exact = 3.0 * 3f64.sqrt() / 2.0 * 100.0;
        assert!((r[0].area() - exact).abs() / exact < 1e-6);
    }

    #[test]
    fn slot_outline_encloses_expected_area() {
        let (pts, r) = slot_geometry([0.0, 0.0], [20.0, 0.0], [5.0, 3.0]).unwrap();
        assert!((r - 3.0).abs() < 1e-12);
        let area = fusion_core::sketch::signed_area(&pts).abs();
        let exact = 20.0 * 6.0 + std::f64::consts::PI * 9.0;
        assert!((area - exact).abs() / exact < 0.01, "{area} vs {exact}");
    }

    #[test]
    fn circumcenter_works() {
        let c = circumcenter([1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]).unwrap();
        assert!(c[0].abs() < 1e-12 && c[1].abs() < 1e-12);
        assert!(circumcenter([0.0, 0.0], [1.0, 1.0], [2.0, 2.0]).is_none());
    }
}
