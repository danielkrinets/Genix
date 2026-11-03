//! Документ: параметры + история операций (timeline). Пересчёт с кэшем: изменение шага N
//! пересчитывает только шаги начиная с N.

use crate::csg::{BoolOp, Solid};
use crate::expr::{Expr, Params};
use crate::geom::{self, EdgeOp};
use crate::mesh::Mesh;
use crate::sketch::{Plane, ProfileRef, Region, SolveStatus, Sketch};
use glam::{DMat4, DQuat, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Что делать с новым телом.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Op {
    NewBody,
    Join,
    Cut,
    Intersect,
}

impl Op {
    pub const ALL: [Op; 4] = [Op::NewBody, Op::Join, Op::Cut, Op::Intersect];
    pub fn label(self) -> &'static str {
        match self {
            Op::NewBody => "Новое тело",
            Op::Join => "Объединить",
            Op::Cut => "Вырезать",
            Op::Intersect => "Пересечь",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Extent {
    /// В одну сторону (отрицательное значение — в обратную).
    OneSide(Expr),
    /// Симметрично в обе стороны; значение — полная толщина.
    Symmetric(Expr),
    /// В обе стороны на разные расстояния: (вперёд, назад).
    TwoSides(Expr, Expr),
    /// Through the whole model; `true` goes both ways from the profile, `false` only forward.
    All(bool),
    /// Up to another body: forward until the nearest side of that body's bounding box (a flat target face is reached exactly).
    ToBody(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis3 {
    X,
    Y,
    Z,
}

impl Axis3 {
    pub fn dir(self) -> DVec3 {
        match self {
            Axis3::X => DVec3::X,
            Axis3::Y => DVec3::Y,
            Axis3::Z => DVec3::Z,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Axis3::X => "X",
            Axis3::Y => "Y",
            Axis3::Z => "Z",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum RevolveAxis {
    SketchX,
    SketchY,
    /// Линия чертежа (по индексу кривой).
    Line(usize),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Box { w: Expr, d: Expr, h: Expr },
    Cylinder { r: Expr, h: Expr },
    Sphere { r: Expr },
    /// Большой радиус (до центра сечения) и малый радиус.
    Torus { major: Expr, minor: Expr },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HoleKind {
    Simple,
    Counterbore { dia: Expr, depth: Expr },
    Countersink { dia: Expr, angle: Expr },
    /// Threaded (tapped) hole for a screw of nominal diameter `major` and pitch `pitch`. The hole's own diameter is the tap
    /// drill. With `modeled` the helical groove is cut into the wall (up to 40 mm deep), otherwise the thread is only
    /// implied by the drill size, as a cosmetic thread.
    Tapped { major: Expr, pitch: Expr, modeled: bool },
}

/// Ребро тела по двум концам (в мировых координатах; находится в теле с допуском).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EdgeRef {
    pub a: [f64; 3],
    pub b: [f64; 3],
    /// Круглое ребро: `a` — точка на окружности, `b` — центр, `axis` — ось.
    #[serde(default)]
    pub circle: bool,
    #[serde(default)]
    pub axis: [f64; 3],
    /// Optional sweep in degrees for an open circular edge; old files mean a full circle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sweep: Option<f64>,
}

impl EdgeRef {
    pub fn line(a: [f64; 3], b: [f64; 3]) -> Self {
        EdgeRef { a, b, circle: false, axis: [0.0; 3], sweep: None }
    }

    pub fn round(point: [f64; 3], center: [f64; 3], axis: [f64; 3]) -> Self {
        EdgeRef { a: point, b: center, circle: true, axis, sweep: None }
    }

    /// Ломаная для показа и выбора мышью (окружность — 72 отрезка).
    pub fn polyline(&self) -> Vec<DVec3> {
        if !self.circle {
            return vec![DVec3::from(self.a), DVec3::from(self.b)];
        }
        let (c, ax) = (DVec3::from(self.b), DVec3::from(self.axis).normalize_or(DVec3::Z));
        let rv = DVec3::from(self.a) - c;
        let rv = rv - ax * rv.dot(ax);
        let (x, y) = (rv, ax.cross(rv));
        let sweep = self.sweep.unwrap_or(360.0).to_radians();
        let steps = (sweep.abs().to_degrees()/5.0).ceil().max(2.0) as usize;
        (0..=steps).map(|i| {
            let t = sweep * i as f64 / steps as f64;
            c + x * t.cos() + y * t.sin()
        }).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PatternKind {
    /// A grid of copies; `nz` copies stacked `dz` apart extend it into the third direction (1 keeps it flat).
    Rect {
        nx: Expr,
        dx: Expr,
        ny: Expr,
        dy: Expr,
        #[serde(default = "one_expr")]
        nz: Expr,
        #[serde(default = "zero_expr")]
        dz: Expr,
    },
    Circular { count: Expr, axis: Axis3, angle: Expr },
}

/// A flat face of a body, found again by its plane and a point on it.
fn zero_expr() -> Expr {
    Expr::num(0.0)
}

fn one_expr() -> Expr {
    Expr::num(1.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PipeShape {
    #[default]
    Round,
    Square,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FaceRef {
    pub plane: Plane,
    pub seed: [f64; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FeatureKind {
    Sketch { plane: Plane, sketch: Sketch },
    /// `profiles` — выбранные области чертежа (узнаются по кривым границы и после правки размеров).
    /// `face` is set when the body face itself is the profile (as in Fusion); then `sketch` and `profiles` are not used.
    Extrude {
        sketch: u64,
        profiles: Vec<ProfileRef>,
        extent: Extent,
        op: Op,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<FaceRef>,
        /// Taper angle in degrees: positive narrows the body away from the profile plane.
        #[serde(default = "zero_expr")]
        taper: Expr,
        /// Thin extrude: wall thickness grown inwards from the profile outline; 0 gives a solid body.
        #[serde(default = "zero_expr")]
        thin: Expr,
        /// Offset of the start of the extrusion from the profile plane (along the normal).
        #[serde(default = "zero_expr")]
        start: Expr,
    },
    Revolve { sketch: u64, profiles: Vec<ProfileRef>, axis: RevolveAxis, angle: Expr, op: Op },
    Primitive { shape: Shape, at: [Expr; 3], op: Op },
    Move { bodies: Vec<u64>, translate: [Expr; 3], rotate: [Expr; 3], copy: bool },
    /// Mirror about the plane perpendicular to `normal` that sits `offset` from the origin.
    Mirror {
        bodies: Vec<u64>,
        normal: Axis3,
        join: bool,
        #[serde(default = "zero_expr")]
        offset: Expr,
    },
    /// Cuts each body by the plane perpendicular to `axis` at `offset`; the part on the far side becomes a new body.
    Split { bodies: Vec<u64>, axis: Axis3, offset: Expr },
    /// A closed mesh imported from a file (STL); the data lives inside the project so it opens anywhere.
    Import { name: String, verts: Vec<f32>, tris: Vec<u32> },
    /// Draft: side faces of the bodies lean by `angle` degrees from the neutral plane perpendicular to `axis` at `neutral`.
    Draft { bodies: Vec<u64>, axis: Axis3, neutral: Expr, angle: Expr },
    /// Thread on an existing cylinder of `body`: `edge` is a circle of that cylinder (its rim). The thread runs from the rim along
    /// the cylinder for `length`; on a boss it is a helical ridge added to the surface, in a hole a helical groove cut into the wall
    /// (found automatically). `depth` is how far the thread reaches beyond the cylinder surface.
    ThreadOn { body: u64, edge: EdgeRef, length: Expr, pitch: Expr, depth: Expr },
    /// Modeled external thread: a helical ridge of depth `depth` on a rod of outer diameter `dia`, starting at `at`.
    Thread { at: [Expr; 3], axis: Axis3, dia: Expr, pitch: Expr, length: Expr, depth: Expr, op: Op },
    Pattern { bodies: Vec<u64>, kind: PatternKind },
    Combine { target: u64, tools: Vec<u64>, op: BoolOp, keep_tools: bool },
    Remove { bodies: Vec<u64> },
    /// Отверстие на плоской грани: центр в координатах плоскости (нормаль наружу), глубина `None` — насквозь.
    Hole { plane: Plane, at: [Expr; 2], dia: Expr, depth: Option<Expr>, kind: HoleKind },
    /// Пружина/спираль: круглое сечение вдоль винтовой линии вокруг оси.
    Coil { at: [Expr; 3], axis: Axis3, dia: Expr, pitch: Expr, turns: Expr, section: Expr, op: Op },
    /// Труба вдоль пути из чертежа: внешний диаметр и толщина стенки.
    /// `shape`: round or square section; `wall` 0 makes a solid rod; `corner` rounds the section's corners,
    /// `bend` rounds the bends of the path (0 keeps them sharp).
    Pipe {
        path: u64,
        outer: Expr,
        wall: Expr,
        op: Op,
        #[serde(default)]
        shape: PipeShape,
        #[serde(default = "zero_expr")]
        corner: Expr,
        #[serde(default = "zero_expr")]
        bend: Expr,
    },
    /// Сдвиг: профиль чертежа вдоль пути из другого чертежа.
    Sweep { sketch: u64, profiles: Vec<ProfileRef>, path: u64, op: Op },
    /// Лофт: плавный переход между профилями разных чертежей.
    Loft { profiles: Vec<(u64, ProfileRef)>, op: Op },
    Scale { bodies: Vec<u64>, factor: [Expr; 3], uniform: bool },
    Chamfer { body: u64, edges: Vec<EdgeRef>, dist: Expr },
    Fillet { body: u64, edges: Vec<EdgeRef>, radius: Expr },
    /// Вспомогательная плоскость на расстоянии от другой плоскости (вдоль её нормали).
    OffsetPlane { base: Plane, offset: Expr },
    /// Construction plane turned about the X axis of `base` (through its origin) by `angle` degrees.
    AnglePlane { base: Plane, angle: Expr },
    /// Поставить тело на плоскость: грань `face` (нормаль наружу, `seed` — точка на ней) ложится на плоскость `target`,
    /// тело оказывается по ту сторону плоскости, куда смотрит её нормаль. Положение вдоль плоскости сохраняется.
    /// Align as in Fusion: the body turns so that `face` lies on `target`. With `to` (a point picked on another body's face)
    /// the picked point of the face lands exactly there; without it the body only moves across the plane.
    PlaceOnPlane {
        body: u64,
        face: Plane,
        seed: [f64; 3],
        target: Plane,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<[f64; 3]>,
    },
    /// Вытягивание плоской грани тела: `plane` — плоскость грани (нормаль наружу), `seed` — точка на грани, `distance` > 0 добавляет материал, < 0 вырезает.
    PressPull { plane: Plane, seed: [f64; 3], distance: Expr },
    /// Оболочка: стенки толщиной `thickness`; грань на плоскости `plane` остаётся открытой (если `open`).
    /// `more` are further faces that stay open as well (Shift-click in the window); old files have none.
    Shell {
        plane: Plane,
        thickness: Expr,
        open: bool,
        #[serde(default)]
        more: Vec<Plane>,
    },
}

impl FeatureKind {
    pub fn type_label(&self) -> &'static str {
        match self {
            FeatureKind::Sketch { .. } => "Чертёж",
            FeatureKind::Extrude { .. } => "Выдавливание",
            FeatureKind::Revolve { .. } => "Вращение",
            FeatureKind::Primitive { shape: Shape::Box { .. }, .. } => "Параллелепипед",
            FeatureKind::Primitive { shape: Shape::Cylinder { .. }, .. } => "Цилиндр",
            FeatureKind::Primitive { shape: Shape::Sphere { .. }, .. } => "Сфера",
            FeatureKind::Primitive { shape: Shape::Torus { .. }, .. } => "Тор",
            FeatureKind::Hole { .. } => "Отверстие",
            FeatureKind::PressPull { .. } => "Вытягивание грани",
            FeatureKind::PlaceOnPlane { .. } => "Наложить",
            FeatureKind::Coil { .. } => "Спираль",
            FeatureKind::Pipe { .. } => "Труба",
            FeatureKind::Sweep { .. } => "По траектории",
            FeatureKind::Loft { .. } => "Переход",
            FeatureKind::Scale { .. } => "Масштаб",
            FeatureKind::Chamfer { .. } => "Фаска",
            FeatureKind::Fillet { .. } => "Скругление",
            FeatureKind::OffsetPlane { .. } => "Смещённая плоскость",
            FeatureKind::AnglePlane { .. } => "Плоскость под углом",
            FeatureKind::Shell { .. } => "Оболочка",
            FeatureKind::Move { .. } => "Перемещение",
            FeatureKind::Mirror { .. } => "Зеркало",
            FeatureKind::Split { .. } => "Разрезать",
            FeatureKind::Import { .. } => "Импорт",
            FeatureKind::Draft { .. } => "Уклон",
            FeatureKind::Thread { .. } => "Резьба",
            FeatureKind::ThreadOn { .. } => "Резьба на цилиндре",
            FeatureKind::Pattern { .. } => "Массив",
            FeatureKind::Combine { .. } => "Комбинирование",
            FeatureKind::Remove { .. } => "Удаление",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Feature {
    pub uid: u64,
    pub name: String,
    #[serde(default)]
    pub suppressed: bool,
    pub kind: FeatureKind,
}

#[derive(Debug, Clone)]
pub struct Body {
    pub id: u64,
    pub name: String,
    pub solid: Arc<Solid>,
}

impl Body {
    pub fn mesh(&self) -> &Mesh {
        &self.solid.mesh
    }
}

/// Состояние модели после очередного шага истории.
#[derive(Debug, Clone, Default)]
pub struct Model {
    pub bodies: Vec<Body>,
    pub body_counter: u32,
}

impl Model {
    /// Interference check: every pair of bodies that share volume, with the volume of the overlap (mm³).
    pub fn interferences(&self) -> Vec<(u64, u64, f64)> {
        let mut out = Vec::new();
        for (i, a) in self.bodies.iter().enumerate() {
            for b in &self.bodies[i + 1..] {
                if !a.solid.bounds_overlap(&b.solid) {
                    continue;
                }
                let v = a.solid.boolean(&b.solid, BoolOp::Intersect).volume();
                if v > 1e-6 {
                    out.push((a.id, b.id, v));
                }
            }
        }
        out
    }

    pub fn body(&self, id: u64) -> Option<&Body> {
        self.bodies.iter().find(|b| b.id == id)
    }

    fn new_body(&mut self, id: u64, solid: Solid) {
        self.body_counter += 1;
        self.bodies.push(Body { id, name: format!("Тело {}", self.body_counter), solid: Arc::new(solid) });
    }
}

/// Решённый чертёж с найденными областями.
#[derive(Debug, Clone)]
pub struct SketchResult {
    pub plane: Plane,
    pub sketch: Sketch,
    pub regions: Vec<Region>,
    pub status: SolveStatus,
}

/// Сохранённый вид камеры.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedView {
    pub name: String,
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub target: [f64; 3],
}

pub const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub format: String,
    pub version: u32,
    pub params: Params,
    pub features: Vec<Feature>,
    pub next_uid: u64,
    /// Сколько первых шагов истории активно (маркер отката).
    pub rollback: usize,
    #[serde(default)]
    pub body_names: BTreeMap<u64, String>,
    #[serde(default)]
    pub hidden_bodies: BTreeSet<u64>,
    /// Индекс визуального материала тела. Отсутствующее значение означает
    /// материал по умолчанию приложения и сохраняет совместимость старых файлов.
    #[serde(default)]
    pub body_materials: BTreeMap<u64, usize>,
    /// Полупрозрачные тела-инструменты, вычитаемые как «отверстия».
    #[serde(default)]
    pub cutter_bodies: BTreeSet<u64>,
    /// Чертежи, показанные в окне (по uid).
    #[serde(default)]
    pub visible_sketches: BTreeSet<u64>,
    #[serde(default)]
    pub views: Vec<NamedView>,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            format: "genix".into(),
            version: FORMAT_VERSION,
            params: Params::default(),
            features: Vec::new(),
            next_uid: 1,
            rollback: 0,
            body_names: BTreeMap::new(),
            hidden_bodies: BTreeSet::new(),
            body_materials: BTreeMap::new(),
            cutter_bodies: BTreeSet::new(),
            visible_sketches: BTreeSet::new(),
            views: Vec::new(),
        }
    }
}

impl Document {
    /// Вставить шаг в позицию маркера отката. Возвращает uid.
    pub fn insert(&mut self, kind: FeatureKind) -> u64 {
        let uid = self.next_uid;
        self.next_uid += 1;
        let n = self.features.iter().filter(|f| std::mem::discriminant(&f.kind) == std::mem::discriminant(&kind)).count() + 1;
        let name = format!("{} {n}", kind.type_label());
        self.features.insert(self.rollback, Feature { uid, name, suppressed: false, kind });
        self.rollback += 1;
        uid
    }

    pub fn index_of(&self, uid: u64) -> Option<usize> {
        self.features.iter().position(|f| f.uid == uid)
    }

    pub fn feature(&self, uid: u64) -> Option<&Feature> {
        self.features.iter().find(|f| f.uid == uid)
    }

    pub fn feature_mut(&mut self, uid: u64) -> Option<&mut Feature> {
        self.features.iter_mut().find(|f| f.uid == uid)
    }

    /// Удалить шаг. Возвращает uid шагов, которые ссылались на него (они станут ошибочными).
    pub fn delete(&mut self, uid: u64) -> Vec<u64> {
        let Some(i) = self.index_of(uid) else { return vec![] };
        self.features.remove(i);
        if i < self.rollback {
            self.rollback -= 1;
        }
        self.visible_sketches.remove(&uid);
        self.features
            .iter()
            .filter(|f| match &f.kind {
                FeatureKind::Extrude { sketch, .. } | FeatureKind::Revolve { sketch, .. } => *sketch == uid,
                FeatureKind::Sweep { sketch, path, .. } => *sketch == uid || *path == uid,
                FeatureKind::Pipe { path, .. } => *path == uid,
                FeatureKind::Loft { profiles, .. } => profiles.iter().any(|(s, _)| *s == uid),
                _ => false,
            })
            .map(|f| f.uid)
            .collect()
    }

    pub fn body_name<'a>(&'a self, b: &'a Body) -> &'a str {
        self.body_names.get(&b.id).map(String::as_str).unwrap_or(&b.name)
    }

    /// Вспомогательные плоскости (uid, имя, плоскость) среди активных шагов.
    pub fn offset_planes(&self) -> Vec<(u64, String, Plane)> {
        self.features
            .iter()
            .take(self.rollback)
            .filter(|f| !f.suppressed)
            .filter_map(|f| match &f.kind {
                FeatureKind::OffsetPlane { base, offset } => {
                    let d = offset.eval(&self.params).ok()?;
                    Some((f.uid, f.name.clone(), Plane { origin: (base.o() + base.normal() * d).to_array(), ..*base }))
                }
                FeatureKind::AnglePlane { base, angle } => {
                    let a = angle.eval(&self.params).ok()?.to_radians();
                    let (c, s) = (a.cos(), a.sin());
                    let y = (base.ya() * c + base.normal() * s).to_array();
                    Some((f.uid, f.name.clone(), Plane { y, ..*base }))
                }
                _ => None,
            })
            .collect()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("документ сериализуется")
    }

    pub fn from_json(s: &str) -> Result<Document, String> {
        let d: Document = serde_json::from_str(s).map_err(|e| format!("файл повреждён: {e}"))?;
        if d.format != "genix" && d.format != "ostov" {
            return Err("это не файл Genix".into());
        }
        if d.version > FORMAT_VERSION {
            return Err("файл создан более новой версией программы".into());
        }
        let mut d = d;
        d.rollback = d.rollback.min(d.features.len());
        Ok(d)
    }
}

/// Результат одного шага.
#[derive(Debug, Clone)]
pub struct StepResult {
    pub model: Arc<Model>,
    pub sketch: Option<Arc<SketchResult>>,
    pub error: Option<String>,
    /// Время пересчёта шага (для отладки производительности).
    pub millis: f64,
}

/// Upper bound for selected edges times body triangles in one fillet or chamfer (measured on the bracket: 34 edges take 12 s, 78 edges on 1.6k triangles take 9 s).
const MAX_EDGE_WORK: usize = 1_500_000;

/// Вычислитель истории с кэшем по шагам.
#[derive(Default, Clone)]
pub struct Evaluator {
    features: Vec<Feature>,
    params: Params,
    steps: Vec<StepResult>,
}

impl Evaluator {
    /// Пересчитать документ. Возвращает число реально пересчитанных шагов.
    pub fn evaluate(&mut self, doc: &Document) -> usize {
        let n = doc.rollback.min(doc.features.len());
        let mut keep = if self.params != doc.params { 0 } else { self.steps.len().min(n) };
        for i in 0..keep {
            if self.features.get(i) != Some(&doc.features[i]) {
                keep = i;
                break;
            }
        }
        self.steps.truncate(keep);
        self.features.truncate(keep);
        self.params = doc.params.clone();
        for i in keep..n {
            let f = &doc.features[i];
            let prev = self.steps.last().map(|s| s.model.clone()).unwrap_or_default();
            let t = std::time::Instant::now();
            let r = if f.suppressed {
                Ok((prev.clone(), None))
            } else {
                // geometry code can panic on extreme values (e.g. a huge fillet while a value is dragged):
                // that step fails with a message instead of taking the whole program down
                crate::guarded(std::panic::AssertUnwindSafe(|| eval_feature(f, &prev, &doc.params, &self.steps, &self.features)))
                    .unwrap_or_else(|_| Err("внутренняя ошибка при построении шага: проверьте значения".to_string()))
            };
            let millis = t.elapsed().as_secs_f64() * 1000.0;
            let step = match r {
                Ok((model, sketch)) => StepResult { model, sketch: sketch.map(Arc::new), error: None, millis },
                Err(e) => StepResult { model: prev, sketch: None, error: Some(e), millis },
            };
            self.steps.push(step);
            self.features.push(f.clone());
        }
        n - keep
    }

    pub fn steps(&self) -> &[StepResult] {
        &self.steps
    }

    /// Модель после всех активных шагов.
    pub fn model(&self) -> Arc<Model> {
        self.steps.last().map(|s| s.model.clone()).unwrap_or_default()
    }

    /// Модель до шага `i` (для показа при редактировании чертежа).
    pub fn model_before(&self, i: usize) -> Arc<Model> {
        if i == 0 {
            Arc::default()
        } else {
            self.steps.get(i - 1).map(|s| s.model.clone()).unwrap_or_default()
        }
    }

    pub fn sketch(&self, uid: u64) -> Option<Arc<SketchResult>> {
        let i = self.features.iter().position(|f| f.uid == uid)?;
        self.steps.get(i)?.sketch.clone()
    }

    pub fn error(&self, uid: u64) -> Option<&str> {
        let i = self.features.iter().position(|f| f.uid == uid)?;
        self.steps.get(i)?.error.as_deref()
    }
}

fn eval3(e: &[Expr; 3], p: &Params) -> Result<DVec3, String> {
    Ok(DVec3::new(e[0].eval(p)?, e[1].eval(p)?, e[2].eval(p)?))
}

fn count(e: &Expr, p: &Params) -> Result<usize, String> {
    let v = e.eval(p)?.round();
    if !(1.0..=1000.0).contains(&v) {
        return Err("количество должно быть от 1 до 1000".into());
    }
    Ok(v as usize)
}

fn positive(e: &Expr, p: &Params, what: &str) -> Result<f64, String> {
    let v = e.eval(p)?;
    if v <= 0.0 {
        return Err(format!("{what} должно быть больше нуля"));
    }
    Ok(v)
}

/// Найти выбранные области чертежа.
fn pick_regions(sk: &SketchResult, refs: &[ProfileRef]) -> Result<Vec<Region>, String> {
    if refs.is_empty() {
        return Err("не выбран ни один профиль".into());
    }
    let mut out: Vec<usize> = Vec::new();
    for r in refs {
        let i = r.resolve(&sk.regions).ok_or("профиль не найден — чертёж изменился, выберите профиль заново")?;
        if !out.contains(&i) {
            out.push(i);
        }
    }
    Ok(out.into_iter().map(|i| sk.regions[i].clone()).collect())
}

fn find_sketch(uid: u64, steps: &[StepResult], feats: &[Feature]) -> Result<Arc<SketchResult>, String> {
    let i = feats.iter().position(|f| f.uid == uid).ok_or("чертёж удалён или находится ниже в истории")?;
    steps[i].sketch.clone().ok_or_else(|| "чертёж отключён или содержит ошибку".to_string())
}

/// Объединить несколько кусков в одно тело-инструмент.
/// Destination point for "Place on plane" so that the placed face lands flush against an anchor of the target face:
/// `anchor` = (column, row) with 0 = low edge, 1 = middle, 2 = high edge along the target plane's axes; `lo`/`hi` are the
/// bounds of the target face in its plane coordinates. The turn the body makes is replayed here, so a source face that
/// ends up rotated is still aligned by its real footprint. The returned point is where the picked `seed` has to land.
pub fn place_anchor(face: &Plane, source: &[[f64; 2]], seed: DVec3, target: &Plane, lo: [f64; 2], hi: [f64; 2], anchor: (usize, usize)) -> DVec3 {
    let (from, to) = (face.normal().normalize(), target.normal().normalize());
    let q = DQuat::from_rotation_arc(from, -to);
    let (xa, ya) = (target.xa(), target.ya());
    let (mut umin, mut umax, mut vmin, mut vmax) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in source {
        let rel = q * (face.to_world(*p) - seed);
        let (u, v) = (rel.dot(xa), rel.dot(ya));
        umin = umin.min(u);
        umax = umax.max(u);
        vmin = vmin.min(v);
        vmax = vmax.max(v);
    }
    let place = |a: usize, lo: f64, hi: f64, min: f64, max: f64| match a {
        0 => lo - min,
        1 => (lo + hi) / 2.0 - (min + max) / 2.0,
        _ => hi - max,
    };
    target.to_world([place(anchor.0, lo[0], hi[0], umin, umax), place(anchor.1, lo[1], hi[1], vmin, vmax)])
}

/// Distance from `plane` along its normal to the nearest side of body `id` (the extrusion stops there).
fn distance_to_body(model: &Model, plane: &Plane, id: u64) -> Result<f64, String> {
    let b = model.body(id).ok_or("тело, до которого строится выдавливание, не найдено")?;
    let (lo, hi) = b.mesh().bounds().ok_or("тело пустое")?;
    let n = plane.normal();
    let mut near = f64::MAX;
    for c in 0..8 {
        let corner = DVec3::new(if c & 1 == 0 { lo.x } else { hi.x }, if c & 2 == 0 { lo.y } else { hi.y }, if c & 4 == 0 { lo.z } else { hi.z });
        near = near.min((corner - plane.o()).dot(n));
    }
    if near <= 1e-6 {
        return Err("тело не лежит впереди профиля".into());
    }
    Ok(near)
}

/// How far a through-all feature must reach from `plane`: past the farthest point of any body.
fn through_all(model: &Model, plane: &Plane) -> f64 {
    let mut far = 1.0f64;
    for b in &model.bodies {
        if let Some((lo, hi)) = b.mesh().bounds() {
            for c in 0..8 {
                let corner = DVec3::new(if c & 1 == 0 { lo.x } else { hi.x }, if c & 2 == 0 { lo.y } else { hi.y }, if c & 4 == 0 { lo.z } else { hi.z });
                far = far.max((corner - plane.o()).length());
            }
        }
    }
    far * 1.5 + 1.0
}

/// Extrusion from `z0` to `z1` along the plane normal. A taper leans the side walls away from the profile plane
/// (positive narrows); a body on both sides of the plane gets the taper on each half.
fn tapered_extrude_mesh(plane: &Plane, region: &Region, z0: f64, z1: f64, taper_deg: f64) -> Result<Mesh, String> {
    let plain = |a: f64, b: f64| geom::extrude_region(plane, region, a, b).ok_or_else(|| "не удалось построить выдавливание".to_string());
    if taper_deg.abs() < 1e-9 {
        return plain(z0, z1);
    }
    if taper_deg.abs() >= 45.0 {
        return Err("угол наклона должен быть меньше 45°".into());
    }
    let n = plane.normal();
    let neutral = n.dot(plane.o());
    let half = |a: f64, b: f64, pull: DVec3, neutral: f64| -> Result<Mesh, String> {
        let drafted = geom::draft_mesh(&plain(a, b)?, pull, neutral, taper_deg);
        Solid::from_mesh(&drafted).map_err(|_| "угол слишком велик для этого профиля".to_string())?;
        Ok(drafted)
    };
    let (lo, hi) = (z0.min(z1), z0.max(z1));
    let mut parts = Vec::new();
    if hi > 1e-9 {
        parts.push(half(lo.max(0.0), hi, n, neutral)?);
    }
    if lo < -1e-9 {
        parts.push(half(lo, hi.min(0.0), -n, -neutral)?);
    }
    union_all(parts).map(|s| s.mesh.clone())
}

fn tapered_extrude(plane: &Plane, region: &Region, z0: f64, z1: f64, taper_deg: f64) -> Result<Solid, String> {
    Solid::from_mesh(&tapered_extrude_mesh(plane, region, z0, z1, taper_deg)?)
}

/// Extrusion that can also be thin (Fusion's "thin extrude"): the prism minus its copy shrunk by `thin` in the profile plane,
/// so only a wall of that thickness along the outline (and around every hole) is left.
fn thin_extrude(plane: &Plane, region: &Region, z0: f64, z1: f64, taper_deg: f64, thin: f64) -> Result<Solid, String> {
    let prism = tapered_extrude(plane, region, z0, z1, taper_deg)?;
    if thin.abs() < 1e-9 {
        return Ok(prism);
    }
    if thin < 0.0 {
        return Err("толщина стенки должна быть положительной".into());
    }
    // a flat disc lying in the profile plane erodes the outline sideways only; the prism is made taller first so the caps stay intact
    let pad = (z1 - z0).abs() + 2.0 * thin + 2.0;
    let tall = tapered_extrude(plane, region, z0.min(z1) - pad, z0.max(z1) + pad, taper_deg)?;
    let (xa, ya, n) = (plane.xa(), plane.ya(), plane.normal());
    let mat = DMat4::from_cols(xa.extend(0.0), ya.extend(0.0), n.extend(0.0), plane.o().extend(1.0));
    let disc = Solid::from_mesh(&geom::cylinder_mesh(DVec3::new(0.0, 0.0, -0.005), thin, 0.01))?.transformed(&mat);
    let inner = tall.eroded(&disc);
    let cavity = inner.boolean(&prism, BoolOp::Intersect);
    let wall = prism.boolean(&cavity, BoolOp::Subtract);
    if wall.is_empty() || (wall.volume() - prism.volume()).abs() < 1e-9 {
        return Err("стенка толще профиля: утончение не получилось".into());
    }
    Ok(wall)
}

fn union_all(parts: Vec<Mesh>) -> Result<Solid, String> {
    let solids = parts.iter().map(Solid::from_mesh).collect::<Result<Vec<_>, _>>()?;
    match solids.len() {
        0 => Err("пустой профиль".into()),
        1 => Ok(solids.into_iter().next().unwrap()),
        _ => Ok(Solid::union_all(&solids)),
    }
}

/// Применить тело-инструмент к модели согласно операции.
pub fn apply_op(model: &mut Model, tool: Solid, op: Op, new_id: u64) -> Result<(), String> {
    if tool.is_empty() {
        return Err("пустое тело".into());
    }
    let overlapping: Vec<usize> = model.bodies.iter().enumerate().filter(|(_, b)| b.solid.bounds_overlap(&tool)).map(|(i, _)| i).collect();
    match op {
        Op::NewBody => model.new_body(new_id, tool),
        Op::Join => {
            if overlapping.is_empty() {
                model.new_body(new_id, tool);
                return Ok(());
            }
            let mut all: Vec<Solid> = overlapping.iter().map(|&i| (*model.bodies[i].solid).clone()).collect();
            all.push(tool);
            let acc = if all.len() == 2 { all[0].boolean(&all[1], BoolOp::Union) } else { Solid::union_all(&all) };
            let first = overlapping[0];
            model.bodies[first].solid = Arc::new(acc);
            for &i in overlapping[1..].iter().rev() {
                model.bodies.remove(i);
            }
        }
        Op::Cut | Op::Intersect => {
            if overlapping.is_empty() {
                return Err(if op == Op::Cut { "инструмент не касается ни одного тела" } else { "нет пересечения с телами" }.into());
            }
            let bop = if op == Op::Cut { BoolOp::Subtract } else { BoolOp::Intersect };
            for &i in overlapping.iter().rev() {
                let r = model.bodies[i].solid.boolean(&tool, bop);
                if r.is_empty() {
                    model.bodies.remove(i);
                } else {
                    model.bodies[i].solid = Arc::new(r);
                }
            }
        }
    }
    Ok(())
}

fn bodies_by_id<'a>(model: &'a Model, ids: &[u64]) -> Result<Vec<&'a Body>, String> {
    if ids.is_empty() {
        return Err("не выбрано ни одного тела".into());
    }
    ids.iter().map(|id| model.body(*id).ok_or_else(|| "выбранное тело больше не существует".to_string())).collect()
}

fn center_of(bodies: &[&Body]) -> DVec3 {
    let mut lo = DVec3::splat(f64::MAX);
    let mut hi = DVec3::splat(f64::MIN);
    for b in bodies {
        if let Some((l, h)) = b.mesh().bounds() {
            lo = lo.min(l);
            hi = hi.max(h);
        }
    }
    (lo + hi) / 2.0
}

type EvalOut = (Arc<Model>, Option<SketchResult>);

/// Segments round the equator of the ball that opens a rounded corner (Minkowski sums cost time with the square of the facets).
const OPEN_SEGMENTS: usize = 24;

fn eval_feature(f: &Feature, prev: &Arc<Model>, p: &Params, steps: &[StepResult], feats: &[Feature]) -> Result<EvalOut, String> {
    let id_base = f.uid * 1000;
    match &f.kind {
        FeatureKind::Sketch { plane, sketch } => {
            let mut s = sketch.clone();
            let status = s.solve(p, &[])?;
            let regions = s.regions();
            Ok((prev.clone(), Some(SketchResult { plane: *plane, sketch: s, regions, status })))
        }
        FeatureKind::Extrude { face: Some(f), extent, taper, thin, .. } => {
            // same rule as press-pull: a positive distance adds material, a negative one cuts; symmetric adds
            let (z0, z1, op) = match extent {
                Extent::OneSide(d) => {
                    let d = d.eval(p)?;
                    if d.abs() < 1e-9 {
                        return Err("расстояние не может быть нулевым".into());
                    }
                    if d > 0.0 { (-0.01, d, Op::Join) } else { (d, 0.01, Op::Cut) }
                }
                Extent::Symmetric(d) => {
                    let d = d.eval(p)?.abs();
                    (-d / 2.0, d / 2.0, Op::Join)
                }
                Extent::TwoSides(..) => return Err("для грани задайте одно расстояние или симметрично".into()),
                Extent::All(_) => (-through_all(prev, &f.plane), 0.01, Op::Cut),
                Extent::ToBody(id) => (0.0, distance_to_body(prev, &f.plane, *id)?, Op::Join),
            };
            let region = prev.bodies.iter().find_map(|b| geom::face_region(b.mesh(), &f.plane, DVec3::from(f.seed))).ok_or("грань не найдена: тело изменилось")?;
            let tool = thin_extrude(&f.plane, &region, z0, z1, taper.eval(p)?, thin.eval(p)?)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Extrude { sketch, profiles, extent, op, taper, thin, start, .. } => {
            let sk = find_sketch(*sketch, steps, feats)?;
            let regions = pick_regions(&sk, profiles)?;
            let taper = taper.eval(p)?;
            let thin = thin.eval(p)?;
            let (z0, z1) = match extent {
                Extent::All(both) => {
                    let far = through_all(prev, &sk.plane);
                    (if *both { -far } else { 0.0 }, far)
                }
                Extent::ToBody(id) => (0.0, distance_to_body(prev, &sk.plane, *id)?),
                Extent::OneSide(d) => (0.0, d.eval(p)?),
                Extent::Symmetric(d) => {
                    let d = d.eval(p)?.abs();
                    (-d / 2.0, d / 2.0)
                }
                Extent::TwoSides(a, b) => (-b.eval(p)?, a.eval(p)?),
            };
            let off = start.eval(p)?;
            let (z0, z1) = if matches!(extent, Extent::ToBody(_)) { (z0 + off, z1) } else { (z0 + off, z1 + off) };
            if (z1 - z0).abs() < 1e-9 {
                return Err("расстояние не может быть нулевым".into());
            }
            let parts = regions.iter().map(|r| thin_extrude(&sk.plane, r, z0, z1, taper, thin).map(|s| s.mesh)).collect::<Result<Vec<_>, _>>()?;
            let tool = union_all(parts)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, *op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, op } => {
            let sk = find_sketch(*sketch, steps, feats)?;
            let regions = pick_regions(&sk, profiles)?;
            let (ap, ad) = match axis {
                RevolveAxis::SketchX => ([0.0, 0.0], [1.0, 0.0]),
                RevolveAxis::SketchY => ([0.0, 0.0], [0.0, 1.0]),
                RevolveAxis::Line(e) => match sk.sketch.entities.get(*e) {
                    Some(crate::sketch::Entity::Line { a, b, .. }) => {
                        let (pa, pb) = (sk.sketch.points[*a].pos, sk.sketch.points[*b].pos);
                        (pa, [pb[0] - pa[0], pb[1] - pa[1]])
                    }
                    _ => return Err("ось вращения не найдена в чертеже".into()),
                },
            };
            let ang = angle.eval(p)?;
            let parts = regions.iter().map(|r| geom::revolve_region(&sk.plane, r, ap, ad, ang)).collect::<Result<Vec<_>, _>>()?;
            let tool = union_all(parts)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, *op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Primitive { shape, at, op } => {
            let at = eval3(at, p)?;
            let tool = match shape {
                Shape::Box { w, d, h } => {
                    let size = DVec3::new(positive(w, p, "ширина")?, positive(d, p, "глубина")?, positive(h, p, "высота")?);
                    geom::box_mesh(at, at + size)
                }
                Shape::Cylinder { r, h } => geom::cylinder_mesh(at, positive(r, p, "радиус")?, positive(h, p, "высота")?),
                Shape::Sphere { r } => geom::sphere_mesh(at, positive(r, p, "радиус")?),
                Shape::Torus { major, minor } => {
                    let (r1, r2) = (positive(major, p, "большой радиус")?, positive(minor, p, "малый радиус")?);
                    if r2 >= r1 {
                        return Err("малый радиус должен быть меньше большого".into());
                    }
                    geom::torus_mesh(at, r1, r2)
                }
            };
            let tool = Solid::from_mesh(&tool)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, *op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Move { bodies, translate, rotate, copy } => {
            let sel = bodies_by_id(prev, bodies)?;
            let c = center_of(&sel);
            let t = eval3(translate, p)?;
            let r = eval3(rotate, p)?;
            let q = DQuat::from_euler(glam::EulerRot::XYZ, r.x.to_radians(), r.y.to_radians(), r.z.to_radians());
            let mat = DMat4::from_translation(c + t) * DMat4::from_quat(q) * DMat4::from_translation(-c);
            let moved: Vec<(u64, Solid)> = sel.iter().map(|b| (b.id, b.solid.transformed(&mat))).collect();
            let mut m = (**prev).clone();
            for (k, (id, mesh)) in moved.into_iter().enumerate() {
                if *copy {
                    m.new_body(id_base + k as u64, mesh);
                } else if let Some(b) = m.bodies.iter_mut().find(|b| b.id == id) {
                    b.solid = Arc::new(mesh);
                }
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Mirror { bodies, normal, join, offset } => {
            let sel = bodies_by_id(prev, bodies)?;
            let n = normal.dir();
            let s = DVec3::ONE - 2.0 * n;
            let shift = n * offset.eval(p)?;
            let mat = DMat4::from_translation(shift) * DMat4::from_scale(s) * DMat4::from_translation(-shift);
            let mirrored: Vec<(u64, Solid)> = sel.iter().map(|b| (b.id, b.solid.transformed(&mat))).collect();
            let mut m = (**prev).clone();
            for (k, (id, mesh)) in mirrored.into_iter().enumerate() {
                if *join {
                    let i = m.bodies.iter().position(|b| b.id == id).unwrap();
                    let r = m.bodies[i].solid.boolean(&mesh, BoolOp::Union);
                    m.bodies[i].solid = Arc::new(r);
                } else {
                    m.new_body(id_base + k as u64, mesh);
                }
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Split { bodies, axis, offset } => {
            let sel = bodies_by_id(prev, bodies)?;
            let at = offset.eval(p)?;
            let n = axis.dir();
            let mut m = (**prev).clone();
            let mut k = 0u64;
            for b in sel {
                let (lo, hi) = b.mesh().bounds().ok_or("тело пустое")?;
                let (l, h) = (lo[*axis as usize], hi[*axis as usize]);
                if at <= l + 1e-6 || at >= h - 1e-6 {
                    return Err("плоскость не пересекает тело".into());
                }
                let pad = (hi - lo).max_element() * 2.0 + 1.0;
                let far_lo = lo - DVec3::splat(pad);
                let far_hi = hi + DVec3::splat(pad);
                let cut = |upper: bool| -> Result<Solid, String> {
                    let (mut a, mut z) = (far_lo, far_hi);
                    if upper {
                        a = a * (DVec3::ONE - n) + n * at;
                    } else {
                        z = z * (DVec3::ONE - n) + n * at;
                    }
                    Solid::from_mesh(&geom::box_mesh(a, z))
                };
                let upper = b.solid.boolean(&cut(true)?, BoolOp::Intersect);
                let lower = b.solid.boolean(&cut(false)?, BoolOp::Intersect);
                let i = m.bodies.iter().position(|x| x.id == b.id).unwrap();
                m.bodies[i].solid = Arc::new(lower);
                m.new_body(id_base + k, upper);
                k += 1;
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Import { verts, tris, .. } => {
            let mesh = Mesh {
                vertices: verts.chunks_exact(3).map(|c| [c[0] as f64, c[1] as f64, c[2] as f64]).collect(),
                triangles: tris.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect(),
            };
            if mesh.vertices.is_empty() || tris.iter().any(|&i| i as usize >= mesh.vertices.len()) {
                return Err("данные импорта повреждены".into());
            }
            let mut m = (**prev).clone();
            m.new_body(id_base, Solid::from_mesh(&mesh).map_err(|_| "сетка не замкнута: STL должен быть цельным телом без дыр".to_string())?);
            Ok((Arc::new(m), None))
        }
        FeatureKind::Draft { bodies, axis, neutral, angle } => {
            let sel = bodies_by_id(prev, bodies)?;
            let (neutral, angle) = (neutral.eval(p)?, angle.eval(p)?);
            if angle.abs() >= 45.0 {
                return Err("угол уклона должен быть меньше 45°".into());
            }
            let mut m = (**prev).clone();
            for b in sel {
                let drafted = geom::draft_mesh(b.mesh(), axis.dir(), neutral, angle);
                let solid = Solid::from_mesh(&drafted).map_err(|_| "угол слишком велик для этого тела".to_string())?;
                if solid.volume() <= 0.0 {
                    return Err("угол слишком велик для этого тела".into());
                }
                let i = m.bodies.iter().position(|x| x.id == b.id).unwrap();
                m.bodies[i].solid = Arc::new(solid);
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::ThreadOn { body, edge, length, pitch, depth } => {
            let b = prev.body(*body).ok_or("выберите круглое ребро цилиндра")?;
            if !edge.circle {
                return Err("выберите круглое ребро цилиндра".into());
            }
            let (length, pitch, depth) = (positive(length, p, "длина")?, positive(pitch, p, "шаг")?, positive(depth, p, "глубина")?);
            let center = DVec3::from(edge.b);
            let axis = DVec3::from(edge.axis).normalize_or(DVec3::Z);
            let radial = {
                let v = DVec3::from(edge.a) - center;
                (v - axis * v.dot(axis)).normalize_or(DVec3::X)
            };
            let r = (DVec3::from(edge.a) - center).length();
            if r < 1e-6 || depth >= r {
                return Err("глубина резьбы должна быть меньше радиуса цилиндра".into());
            }
            // which way the cylinder runs and whether the rim belongs to a boss or to a hole: probe just inside and just outside the wall
            let inside = |dir: f64, rad: f64| -> bool {
                let q = center + axis * dir * (length * 0.5) + radial * rad;
                let probe = Solid::from_mesh(&geom::box_mesh(q - DVec3::splat(1e-3), q + DVec3::splat(1e-3)));
                probe.map_or(false, |pr| pr.boolean(&b.solid, BoolOp::Intersect).volume() > 1e-12)
            };
            let eps = (depth * 0.1).max(1e-3).min(r * 0.5);
            let mut found: Option<(f64, bool)> = None;
            for dir in [1.0, -1.0] {
                let (inner, outer) = (inside(dir, r - eps), inside(dir, r + eps));
                if inner && !outer {
                    found = Some((dir, true));
                    break;
                }
                if !inner && outer {
                    found = Some((dir, false));
                    break;
                }
            }
            let (dir, external) = found.ok_or("у этого ребра нет цилиндрической стенки")?;
            let sec = (depth * 2.0).min(pitch * 0.8);
            let helix = geom::helix_path(DVec3::new(0.0, 0.0, -pitch), DVec3::Z, 2.0 * (r + depth - sec / 2.0), pitch, length / pitch + 2.0);
            let t = (helix[1] - helix[0]).normalize();
            let thread = Solid::from_mesh(&geom::sweep_loops(&helix, &geom::circle_loop(helix[0], t, sec / 2.0, true), &[])?)?;
            let envelope = Solid::from_mesh(&geom::cylinder_mesh(DVec3::ZERO, r + depth + 1.0, length))?;
            let z = axis * dir;
            let (xa, ya) = geom::perp_frame(z);
            let mat = DMat4::from_cols(xa.extend(0.0), ya.extend(0.0), z.extend(0.0), center.extend(1.0));
            let cut = thread.boolean(&envelope, BoolOp::Intersect).transformed(&mat);
            let result = if external { b.solid.boolean(&cut, BoolOp::Union) } else { b.solid.boolean(&cut, BoolOp::Subtract) };
            let mut m = (**prev).clone();
            if let Some(slot) = m.bodies.iter_mut().find(|x| x.id == *body) {
                slot.solid = Arc::new(result);
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Thread { at, axis, dia, pitch, length, depth, op } => {
            let at = eval3(at, p)?;
            let (dia, pitch, length, depth) = (positive(dia, p, "диаметр")?, positive(pitch, p, "шаг")?, positive(length, p, "длина")?, positive(depth, p, "глубина")?);
            if depth * 2.0 >= dia {
                return Err("глубина резьбы должна быть меньше радиуса".into());
            }
            let sec = (depth * 2.0).min(pitch * 0.8);
            let core = Solid::from_mesh(&geom::cylinder_mesh(DVec3::ZERO, dia / 2.0 - depth, length))?;
            let helix = geom::helix_path(DVec3::new(0.0, 0.0, -pitch), DVec3::Z, dia - sec, pitch, length / pitch + 2.0);
            let t = (helix[1] - helix[0]).normalize();
            let ridge = Solid::from_mesh(&geom::sweep_loops(&helix, &geom::circle_loop(helix[0], t, sec / 2.0, true), &[])?)?;
            let envelope = Solid::from_mesh(&geom::cylinder_mesh(DVec3::ZERO, dia / 2.0, length))?;
            let rod = core.boolean(&ridge, BoolOp::Union).boolean(&envelope, BoolOp::Intersect);
            let rot = match axis {
                Axis3::Z => DMat4::IDENTITY,
                Axis3::X => DMat4::from_rotation_y(90f64.to_radians()),
                Axis3::Y => DMat4::from_rotation_x(-90f64.to_radians()),
            };
            let tool = rod.transformed(&(DMat4::from_translation(at) * rot));
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, *op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Pattern { bodies, kind } => {
            let sel = bodies_by_id(prev, bodies)?;
            let mut mats = Vec::new();
            match kind {
                PatternKind::Rect { nx, dx, ny, dy, nz, dz } => {
                    let (nx, ny, nz) = (count(nx, p)?, count(ny, p)?, count(nz, p)?);
                    let (dx, dy, dz) = (dx.eval(p)?, dy.eval(p)?, dz.eval(p)?);
                    for k in 0..nz {
                        for j in 0..ny {
                            for i in 0..nx {
                                if i + j + k > 0 {
                                    mats.push(DMat4::from_translation(DVec3::new(i as f64 * dx, j as f64 * dy, k as f64 * dz)));
                                }
                            }
                        }
                    }
                }
                PatternKind::Circular { count: c, axis, angle } => {
                    let c = count(c, p)?;
                    let ang = angle.eval(p)?;
                    // полный круг: копии не должны совпасть с исходным телом
                    let step = if (ang.abs() - 360.0).abs() < 1e-9 { ang / c as f64 } else { ang / (c.max(2) - 1) as f64 };
                    for k in 1..c {
                        mats.push(DMat4::from_axis_angle(axis.dir(), (step * k as f64).to_radians()));
                    }
                }
            }
            if mats.is_empty() {
                return Err("массив без копий: увеличьте количество".into());
            }
            let mut m = (**prev).clone();
            let mut k = 0;
            for mat in &mats {
                for b in &sel {
                    m.new_body(id_base + k, b.solid.transformed(mat));
                    k += 1;
                }
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Combine { target, tools, op, keep_tools } => {
            if tools.contains(target) {
                return Err("тело не может быть одновременно целью и инструментом".into());
            }
            let t = prev.body(*target).ok_or("целевое тело не найдено")?;
            let tl = bodies_by_id(prev, tools)?;
            // объединение и вычитание — одной пакетной операцией (на порядки быстрее, чем по одному)
            let acc = match op {
                BoolOp::Split => {
                    // the tool cuts the target in two: the part outside it stays, the part inside becomes a new body
                    let solids: Vec<Solid> = tl.iter().map(|b| (*b.solid).clone()).collect();
                    let tool = if solids.len() == 1 { solids.into_iter().next().unwrap() } else { Solid::union_all(&solids) };
                    let inside = t.solid.boolean(&tool, BoolOp::Intersect);
                    let outside = t.solid.boolean(&tool, BoolOp::Subtract);
                    if inside.is_empty() || outside.is_empty() {
                        return Err("инструмент не делит тело на две части".into());
                    }
                    let mut m = (**prev).clone();
                    if !keep_tools {
                        m.bodies.retain(|b| !tools.contains(&b.id));
                    }
                    if let Some(b) = m.bodies.iter_mut().find(|b| b.id == *target) {
                        b.solid = Arc::new(outside);
                    }
                    m.new_body(id_base, inside);
                    return Ok((Arc::new(m), None));
                }
                BoolOp::Union => {
                    let mut all: Vec<Solid> = vec![(*t.solid).clone()];
                    all.extend(tl.iter().map(|b| (*b.solid).clone()));
                    Solid::union_all(&all)
                }
                BoolOp::Subtract => {
                    let tools: Vec<Solid> = tl.iter().map(|b| (*b.solid).clone()).collect();
                    let tool = if tools.len() == 1 { tools.into_iter().next().unwrap() } else { Solid::union_all(&tools) };
                    t.solid.boolean(&tool, BoolOp::Subtract)
                }
                BoolOp::Intersect => {
                    let mut acc = (*t.solid).clone();
                    for b in &tl {
                        acc = acc.boolean(&b.solid, BoolOp::Intersect);
                    }
                    acc
                }
            };
            let mut m = (**prev).clone();
            if !keep_tools {
                m.bodies.retain(|b| !tools.contains(&b.id));
            }
            if acc.is_empty() {
                m.bodies.retain(|b| b.id != *target);
            } else if let Some(b) = m.bodies.iter_mut().find(|b| b.id == *target) {
                b.solid = Arc::new(acc);
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Hole { plane, at, dia, depth, kind } => {
            let r = positive(dia, p, "диаметр")? / 2.0;
            let center = plane.to_world([at[0].eval(p)?, at[1].eval(p)?]);
            let n = plane.normal();
            let big = prev.bodies.iter().filter_map(|b| b.mesh().bounds()).fold(0.0f64, |m, (l, h)| m.max((h - l).length())) + 10.0;
            let d = match depth {
                Some(e) => positive(e, p, "глубина")?,
                None => big,
            };
            let mut parts = vec![Solid::from_mesh(&geom::frustum_mesh(center, n, r, -d, r, 1.0))?];
            match kind {
                HoleKind::Simple => {}
                HoleKind::Counterbore { dia: d2, depth: h2 } => {
                    let (r2, h2) = (positive(d2, p, "диаметр цековки")? / 2.0, positive(h2, p, "глубина цековки")?);
                    if r2 <= r {
                        return Err("диаметр цековки должен быть больше диаметра отверстия".into());
                    }
                    parts.push(Solid::from_mesh(&geom::frustum_mesh(center, n, r2, -h2.min(d), r2, 1.0))?);
                }
                HoleKind::Countersink { dia: d2, angle } => {
                    let (r2, ang) = (positive(d2, p, "диаметр зенковки")? / 2.0, positive(angle, p, "угол")?);
                    if r2 <= r || ang >= 179.0 {
                        return Err("неверные размеры зенковки".into());
                    }
                    let tn = (ang.to_radians() / 2.0).tan();
                    let depth_cone = ((r2 - r) / tn).min(d);
                    parts.push(Solid::from_mesh(&geom::frustum_mesh(center, n, r, -depth_cone, r2 + tn * 1.0, 1.0))?);
                }
                HoleKind::Tapped { major, pitch, modeled } => {
                    let (rm, pitch) = (positive(major, p, "диаметр резьбы")? / 2.0, positive(pitch, p, "шаг резьбы")?);
                    if rm <= r {
                        return Err("диаметр резьбы должен быть больше диаметра отверстия под резьбу".into());
                    }
                    if *modeled {
                        let len = d.min(40.0);
                        let sr = ((rm - r) * 0.75).min(pitch * 0.45);
                        let helix = geom::helix_path(DVec3::new(0.0, 0.0, -pitch), DVec3::Z, 2.0 * (rm - sr), pitch, len / pitch + 2.0);
                        let t = (helix[1] - helix[0]).normalize();
                        let groove = Solid::from_mesh(&geom::sweep_loops(&helix, &geom::circle_loop(helix[0], t, sr, true), &[])?)?;
                        let envelope = Solid::from_mesh(&geom::cylinder_mesh(DVec3::ZERO, rm + 1.0, len))?;
                        let (xa, ya) = geom::perp_frame(n);
                        // local z runs into the material (-n); y is flipped so that the frame stays right-handed and the thread right-hand
                        let mat = DMat4::from_cols(xa.extend(0.0), (-ya).extend(0.0), (-n).extend(0.0), center.extend(1.0));
                        parts.push(groove.boolean(&envelope, BoolOp::Intersect).transformed(&mat));
                    }
                }
            }
            let tool = if parts.len() == 1 { parts.remove(0) } else { Solid::union_all(&parts) };
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, Op::Cut, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::PlaceOnPlane { body, face, seed, target, to: point } => {
            let sel = bodies_by_id(prev, &[*body])?;
            let (from, to) = (face.normal().normalize(), target.normal().normalize());
            // the face normal must end up opposite to the plane normal: the face lies on the plane, the body stands on top of it
            let q = DQuat::from_rotation_arc(from, -to);
            let pivot = DVec3::from(*seed);
            let lift = (target.o() - pivot).dot(to);
            // the turned pivot goes onto the picked point, or straight onto the plane when no point was picked
            let dest = point.map(DVec3::from).unwrap_or(pivot + to * lift);
            let mat = DMat4::from_translation(dest) * DMat4::from_quat(q) * DMat4::from_translation(-pivot);
            let moved = sel[0].solid.transformed(&mat);
            let mut m = (**prev).clone();
            if let Some(b) = m.bodies.iter_mut().find(|b| b.id == *body) { b.solid = Arc::new(moved); }
            Ok((Arc::new(m), None))
        }
        FeatureKind::PressPull { plane, seed, distance } => {
            let d = distance.eval(p)?;
            if d.abs() < 1e-9 {
                return Err("расстояние не может быть нулевым".into());
            }
            let region = prev.bodies.iter().find_map(|b| geom::face_region(b.mesh(), plane, DVec3::from(*seed))).ok_or("грань не найдена: тело изменилось")?;
            // заход на 0,01 мм внутрь тела (при добавлении) или за грань (при вырезании) исключает щель на стыке
            let (z0, z1, op) = if d > 0.0 { (-0.01, d, Op::Join) } else { (d, 0.01, Op::Cut) };
            let tool = Solid::from_mesh(&geom::extrude_region(plane, &region, z0, z1).ok_or("не удалось построить грань")?)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Coil { at, axis, dia, pitch, turns, section, op } => {
            let at = eval3(at, p)?;
            let (dia, sec) = (positive(dia, p, "диаметр")?, positive(section, p, "диаметр сечения")?);
            let (pitch, turns) = (positive(pitch, p, "шаг")?, positive(turns, p, "число витков")?);
            if sec >= pitch {
                return Err("сечение должно быть меньше шага, иначе витки пересекутся".into());
            }
            let path = geom::helix_path(at, axis.dir(), dia, pitch, turns);
            let t = (path[1] - path[0]).normalize();
            let prof = geom::circle_loop(path[0], t, sec / 2.0, true);
            let tool = Solid::from_mesh(&geom::sweep_loops(&path, &prof, &[])?)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, *op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Pipe { path, outer, wall, op, shape, corner, bend } => {
            let sk = find_sketch(*path, steps, feats)?;
            let raw: Vec<DVec3> = sk.sketch.chain_path()?.into_iter().map(|q| sk.plane.to_world(q)).collect();
            let od = positive(outer, p, "диаметр")?;
            let w = wall.eval(p)?;
            if w < 0.0 {
                return Err("толщина стенки не может быть отрицательной".into());
            }
            let (corner_r, bend_r) = (corner.eval(p)?, bend.eval(p)?);
            if corner_r < 0.0 || bend_r < 0.0 {
                return Err("радиус скругления не может быть отрицательным".into());
            }
            if raw.len() < 2 {
                return Err("путь слишком короткий".into());
            }
            if w * 2.0 >= od {
                return Err("стенка слишком толстая для такого диаметра".into());
            }
            let pts = geom::round_path(&raw, bend_r);
            let t = (pts[1] - pts[0]).normalize();
            let (o, i) = match shape {
                PipeShape::Round => (geom::circle_loop(pts[0], t, od / 2.0, true), geom::circle_loop(pts[0], t, od / 2.0 - w, false)),
                PipeShape::Square => (
                    geom::rounded_rect_loop(pts[0], t, od / 2.0, od / 2.0, corner_r, true),
                    geom::rounded_rect_loop(pts[0], t, od / 2.0 - w, od / 2.0 - w, (corner_r - w).max(0.0), false),
                ),
            };
            let holes = if w > 1e-9 { vec![i] } else { vec![] };
            let tool = Solid::from_mesh(&geom::sweep_loops(&pts, &o, &holes)?)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, *op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Sweep { sketch, profiles, path, op } => {
            let sk = find_sketch(*sketch, steps, feats)?;
            let ps = find_sketch(*path, steps, feats)?;
            let regions = pick_regions(&sk, profiles)?;
            let pts: Vec<DVec3> = ps.sketch.chain_path()?.into_iter().map(|q| ps.plane.to_world(q)).collect();
            let mut parts = Vec::new();
            for r in &regions {
                let w = |l: &[[f64; 2]]| -> Vec<DVec3> { l.iter().map(|q| sk.plane.to_world(*q)).collect() };
                let holes: Vec<Vec<DVec3>> = r.holes.iter().map(|h| w(h)).collect();
                parts.push(Solid::from_mesh(&geom::sweep_loops(&pts, &w(&r.outer), &holes)?)?);
            }
            let tool = if parts.len() == 1 { parts.remove(0) } else { Solid::union_all(&parts) };
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, *op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Loft { profiles, op } => {
            if profiles.len() < 2 {
                return Err("выберите минимум два профиля в разных чертежах".into());
            }
            let mut loops = Vec::new();
            for (uid, pr) in profiles {
                let sk = find_sketch(*uid, steps, feats)?;
                let i = pr.resolve(&sk.regions).ok_or("профиль не найден — чертёж изменился, выберите профиль заново")?;
                loops.push(sk.regions[i].outer.iter().map(|q| sk.plane.to_world(*q)).collect::<Vec<DVec3>>());
            }
            let tool = Solid::from_mesh(&geom::loft_loops(&loops)?)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, *op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Scale { bodies, factor, uniform } => {
            let sel = bodies_by_id(prev, bodies)?;
            let f = if *uniform { DVec3::splat(factor[0].eval(p)?) } else { eval3(factor, p)? };
            if f.min_element() <= 0.0 {
                return Err("коэффициент должен быть больше нуля".into());
            }
            let c = center_of(&sel);
            let mat = DMat4::from_translation(c) * DMat4::from_scale(f) * DMat4::from_translation(-c);
            let moved: Vec<(u64, Solid)> = sel.iter().map(|b| (b.id, b.solid.transformed(&mat))).collect();
            let mut m = (**prev).clone();
            for (id, s) in moved {
                if let Some(b) = m.bodies.iter_mut().find(|b| b.id == id) {
                    b.solid = Arc::new(s);
                }
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Chamfer { body, edges, dist: size } | FeatureKind::Fillet { body, edges, radius: size } => {
            let is_fillet = matches!(f.kind, FeatureKind::Fillet { .. });
            let b = prev.body(*body).ok_or("тело не найдено")?;
            if edges.is_empty() {
                return Err("не выбрано ни одного ребра".into());
            }
            let v = positive(size, p, if is_fillet { "радиус" } else { "расстояние" })?;
            // a size beyond the body itself can only break the geometry (and a drag past it would keep the window busy for minutes)
            if let Some((lo, hi)) = b.solid.mesh.bounds() {
                if v > (hi - lo).max_element() {
                    return Err("размер больше самого тела".into());
                }
            }
            // every selected edge is a boolean against the whole body: edges x triangles past this would freeze the window for minutes
            if edges.len().saturating_mul(b.mesh().triangles.len()) > MAX_EDGE_WORK {
                return Err("слишком много рёбер для такого тела: выберите меньше".into());
            }
            let op = if is_fillet { EdgeOp::Fillet(v) } else { EdgeOp::Chamfer(v) };
            let (mut cuts, mut adds) = (Vec::new(), Vec::new());
            // ends where a fillet meets one made earlier, or a corner of three selected edges (see `blend_vertices`)
            let spots = if is_fillet {
                let lines: Vec<(DVec3, DVec3)> = edges.iter().filter(|e| !e.circle).map(|e| (DVec3::from(e.a), DVec3::from(e.b))).collect();
                geom::blend_vertices(b.mesh(), &lines)
            } else {
                Vec::new()
            };
            let run_on = |p: [f64; 3]| spots.iter().any(|(c, all)| !*all && c.distance(DVec3::from(p)) < 1e-4) as u8 as f64 * (v * 3.0 + 0.5);
            for e in edges {
                let (t, concave) = if e.circle {
                    geom::circle_tool_span(b.mesh(), DVec3::from(e.b), DVec3::from(e.axis), DVec3::from(e.a), op, e.sweep.unwrap_or(360.0))?
                } else {
                    geom::edge_tool_ends(b.mesh(), DVec3::from(e.a), DVec3::from(e.b), op, run_on(e.a), run_on(e.b))?
                };
                let s = Solid::from_mesh(&t)?;
                if concave { adds.push(s) } else { cuts.push(s) }
            }
            // Стык «под ус» для любых соседних рёбер. Дуги тоже нужно немного
            // продолжать: иначе на общей вершине двух дуг остаётся тонкий шип.
            let reach = v * 3.0 + 0.5;
            let ends = |e: &EdgeRef| {
                let p = e.polyline();
                (*p.first().unwrap(), *p.last().unwrap())
            };
            let extended = |e: &EdgeRef| {
                if e.circle {
                    let radius = DVec3::from(e.a).distance(DVec3::from(e.b)).max(1e-6);
                    let ext_deg = (reach / radius).to_degrees();
                    geom::circle_tool_span_ext(b.mesh(), DVec3::from(e.b), DVec3::from(e.axis), DVec3::from(e.a), op, e.sweep.unwrap_or(360.0), ext_deg)
                } else {
                    geom::edge_tool_ext(b.mesh(), DVec3::from(e.a), DVec3::from(e.b), op, reach)
                }
            };
            for i in 0..edges.len() {
                for j in i + 1..edges.len() {
                    let ((a1, b1), (a2, b2)) = (ends(&edges[i]), ends(&edges[j]));
                    // Аппроксимированные дуги могут оканчиваться в соседних
                    // вершинах сетки, хотя геометрически образуют один угол.
                    let join_tol = if edges[i].circle || edges[j].circle { (v * 0.75).max(1e-4) } else { 1e-6 };
                    let shares = [a1, b1].iter().any(|p| [a2, b2].iter().any(|q| p.distance(*q) < join_tol));
                    if !shares {
                        continue;
                    }
                    let (Ok((t1, c1)), Ok((t2, c2))) = (extended(&edges[i]), extended(&edges[j])) else { continue };
                    if c1 || c2 {
                        continue;
                    }
                    let joint = Solid::from_mesh(&t1)?.boolean(&Solid::from_mesh(&t2)?, BoolOp::Intersect);
                    if !joint.is_empty() {
                        cuts.push(joint);
                    }
                }
            }
            // A corner where all three edges are selected needs a sphere, which the per-edge wedges cannot make:
            // the body is "opened" with a ball of the fillet radius (shrink, then grow back); what that removes
            // around such corners is exactly the rolling-ball blend.
            let mut blended = false;
            let corners: Vec<DVec3> = spots.iter().filter(|(_, all)| *all).map(|(c, _)| *c).collect();
            if !corners.is_empty() {
                let shrink = Solid::from_mesh(&geom::sphere_mesh_n(DVec3::ZERO, v * 0.99, OPEN_SEGMENTS))?;
                // growing back with a ball 2 % bigger keeps flat faces strictly outside the opened body, so rounding error cannot leave slivers
                let grow = Solid::from_mesh(&geom::sphere_mesh_n(DVec3::ZERO, v * 1.01, OPEN_SEGMENTS))?;
                // only the surroundings of the corners are opened: the zones are 2.5 radii wide and the opening reaches 2 radii,
                // so a box-shaped crop (few facets: the opening is slow on round ones) of 4.5 radii leaves the zones untouched but keeps the work (and the wait) small
                let crops: Vec<Solid> = corners.iter().filter_map(|c| { let h = DVec3::splat(v * 4.5 + 1.0); Solid::from_mesh(&geom::box_mesh(*c - h, *c + h)).ok() }).collect();
                let local = (*b.solid).boolean(&Solid::union_all(&crops), BoolOp::Intersect);
                let opened = local.eroded(&shrink).dilated(&grow);
                if !opened.is_empty() {
                    let removed = local.boolean(&opened, BoolOp::Subtract);
                    let zones: Vec<Solid> = corners.iter().filter_map(|c| Solid::from_mesh(&geom::sphere_mesh(*c, v * 2.5 + 0.5)).ok()).collect();
                    let blend = removed.boolean(&Solid::union_all(&zones), BoolOp::Intersect);
                    if !blend.is_empty() {
                        cuts.push(blend);
                        blended = true;
                    }
                }
            }
            // выпуклые рёбра снимают материал, вогнутые — добавляют (скругление внутренних углов)
            let mut r = (*b.solid).clone();
            if !cuts.is_empty() {
                r = r.boolean(&Solid::union_all(&cuts), BoolOp::Subtract);
            }
            if !adds.is_empty() {
                r = r.boolean(&Solid::union_all(&adds), BoolOp::Union);
            }
            if blended {
                // the blend leaves slivers where its facets meet an earlier fillet's: merge what is within 0.1 mm of flat
                r = r.simplified((v * 0.02).clamp(1e-3, 0.1));
            }
            if r.is_empty() {
                return Err("результат пуст — размер слишком большой".into());
            }
            // a rounding never makes the body smaller across: if it did, the radius does not fit (the geometry has collapsed)
            if let (Some((lo0, hi0)), Some((lo1, hi1))) = (b.solid.mesh.bounds(), r.mesh.bounds()) {
                let (before, after) = (hi0 - lo0, hi1 - lo1);
                if (0..3).any(|i| after[i] < before[i] * 0.9 - 1e-6) {
                    return Err("радиус слишком велик для этого тела".into());
                }
            }
            let mut m = (**prev).clone();
            if let Some(bb) = m.bodies.iter_mut().find(|x| x.id == *body) {
                bb.solid = Arc::new(r);
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Shell { plane, thickness, open, more } => {
            let t = positive(thickness, p, "толщина")?;
            let ball = Solid::from_mesh(&geom::sphere_mesh(DVec3::ZERO, t))?;
            let mut m = (**prev).clone();
            let mut done = 0;
            let faces: Vec<&Plane> = std::iter::once(plane).chain(more.iter()).collect();
            for b in m.bodies.iter_mut() {
                let mesh = b.solid.mesh.clone();
                let Some((lo, hi)) = mesh.bounds() else { continue };
                let on_body = |pl: &Plane| mesh.vertices.iter().any(|q| (DVec3::from(*q) - pl.o()).dot(pl.normal()).abs() < 1e-6);
                if *open && !faces.iter().any(|pl| on_body(pl)) {
                    continue;
                }
                let mut s = (*b.solid).clone();
                if *open {
                    // every open face is continued outwards by a slab, so the hollowing reaches the opening
                    for pl in faces.iter().filter(|pl| on_body(pl)) {
                        let (o, n) = (pl.o(), pl.normal());
                        let out = if ((lo + hi) * 0.5 - o).dot(n) > 0.0 { -n } else { n };
                        let big = (hi - lo).length() * 2.0 + 10.0;
                        let (xa, ya) = geom::perp_frame(n);
                        let mat = DMat4::from_cols(xa.extend(0.0), ya.extend(0.0), out.extend(0.0), o.extend(1.0));
                        let slab = Solid::from_mesh(&geom::box_mesh(DVec3::new(-big, -big, 0.0), DVec3::new(big, big, 2.0 * t + 1.0)))?.transformed(&mat);
                        s = s.boolean(&slab, BoolOp::Union);
                    }
                }
                let cavity = s.eroded(&ball);
                if cavity.is_empty() {
                    return Err("толщина слишком велика для этого тела".into());
                }
                b.solid = Arc::new(b.solid.boolean(&cavity, BoolOp::Subtract));
                done += 1;
            }
            if done == 0 {
                return Err("выберите плоскую грань тела".into());
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::OffsetPlane { offset, .. } => {
            offset.eval(p)?;
            Ok((prev.clone(), None))
        }
        FeatureKind::AnglePlane { angle, .. } => {
            let a = angle.eval(p)?;
            if a.abs() >= 180.0 {
                return Err("угол должен быть меньше 180°".into());
            }
            Ok((prev.clone(), None))
        }
        FeatureKind::Remove { bodies } => {
            bodies_by_id(prev, bodies)?;
            let mut m = (**prev).clone();
            m.bodies.retain(|b| !bodies.contains(&b.id));
            Ok((Arc::new(m), None))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plate_doc() -> (Document, u64) {
        let mut d = Document::default();
        d.params.set("w", "40");
        let mut s = Sketch::default();
        let l = s.add_rect([0.0, 0.0], [40.0, 25.0]);
        s.constraints.push(crate::sketch::Constraint::Length(l[0], Expr::from("w")));
        s.add_circle([20.0, 12.5], 5.0);
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
        d.insert(FeatureKind::Extrude {
            sketch: sk,
            profiles: vec![ProfileRef::at([1.0, 1.0])],
            extent: Extent::OneSide(Expr::num(10.0)),
            op: Op::NewBody, face: None, taper: Expr::num(0.0), thin: Expr::num(0.0), start: Expr::num(0.0) });
        (d, sk)
    }

    #[test]
    fn plate_with_hole_and_param_edit() {
        let (mut d, _) = plate_doc();
        let mut ev = Evaluator::default();
        assert_eq!(ev.evaluate(&d), 2);
        let m = ev.model();
        assert_eq!(m.bodies.len(), 1);
        let hole = std::f64::consts::PI * 25.0;
        let v0 = m.bodies[0].solid.volume();
        assert!((v0 - (1000.0 - hole) * 10.0).abs() < 2.0, "{v0}");
        // повтор без изменений — ничего не пересчитывается
        assert_eq!(ev.evaluate(&d), 0);
        // параметр меняет ширину
        d.params.set("w", "60");
        ev.evaluate(&d);
        let (lo, hi) = ev.model().bodies[0].mesh().bounds().unwrap();
        assert!((hi.x - lo.x - 60.0).abs() < 1e-6);
    }

    #[test]
    fn editing_later_step_reuses_earlier_ones() {
        let (mut d, _) = plate_doc();
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        if let FeatureKind::Extrude { extent, .. } = &mut d.features[1].kind {
            *extent = Extent::OneSide(Expr::num(20.0));
        }
        assert_eq!(ev.evaluate(&d), 1);
    }

    #[test]
    fn cut_join_and_rollback() {
        let (mut d, sk) = plate_doc();
        // вырезаем диск сверху (профиль-диск) на 3 мм вниз — карман
        d.insert(FeatureKind::Extrude {
            sketch: sk,
            profiles: vec![ProfileRef::at([20.0, 12.5])],
            extent: Extent::OneSide(Expr::num(10.0)),
            op: Op::Join, face: None, taper: Expr::num(0.0), thin: Expr::num(0.0), start: Expr::num(0.0) });
        d.insert(FeatureKind::Primitive {
            shape: Shape::Box { w: Expr::num(5.0), d: Expr::num(5.0), h: Expr::num(20.0) },
            at: [Expr::num(-1.0), Expr::num(-1.0), Expr::num(-5.0)],
            op: Op::Cut,
        });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.steps().iter().all(|s| s.error.is_none()), "{:?}", ev.steps().iter().map(|s| &s.error).collect::<Vec<_>>());
        let m = ev.model();
        assert_eq!(m.bodies.len(), 1);
        // отверстие заполнено, угол вырезан 4×4×10
        let v = m.bodies[0].solid.volume();
        assert!((v - (10000.0 - 160.0)).abs() < 1e-6, "{v}");
        // откат на 2 шага: только пластина с отверстием
        d.rollback = 2;
        ev.evaluate(&d);
        assert!(ev.model().bodies[0].solid.volume() < 9300.0);
    }

    #[test]
    fn a_fillet_with_far_too_many_edges_fails_fast_instead_of_freezing() {
        let mut d = Document::default();
        let n = Expr::num;
        d.insert(FeatureKind::Primitive { shape: Shape::Sphere { r: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let body = ev.model().bodies[0].id;
        let tris = ev.model().bodies[0].mesh().triangles.len();
        let edges: Vec<EdgeRef> = (0..(MAX_EDGE_WORK / tris + 2)).map(|i| EdgeRef::line([i as f64, 0.0, 0.0], [i as f64, 1.0, 0.0])).collect();
        let uid = d.insert(FeatureKind::Fillet { body, edges, radius: n(1.0) });
        let t = std::time::Instant::now();
        ev.evaluate(&d);
        assert!(t.elapsed().as_secs_f64() < 2.0, "the guard answers at once");
        assert!(ev.error(uid).is_some_and(|e| e.contains("слишком много рёбер")), "{:?}", ev.error(uid));
    }

    #[test]
    fn split_cuts_a_box_into_two_bodies_with_the_same_total_volume() {
        let mut d = Document::default();
        let n = Expr::num;
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(10.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let id = ev.model().bodies[0].id;
        let uid = d.insert(FeatureKind::Split { bodies: vec![id], axis: Axis3::Z, offset: n(4.0) });
        ev.evaluate(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let vols: Vec<f64> = ev.model().bodies.iter().map(|b| b.mesh().volume().abs()).collect();
        assert_eq!(vols.len(), 2);
        assert!((vols[0] - 400.0).abs() < 1.0 && (vols[1] - 600.0).abs() < 1.0, "{vols:?}");
        let off = d.insert(FeatureKind::Split { bodies: vec![id], axis: Axis3::Z, offset: n(50.0) });
        ev.evaluate(&d);
        assert!(ev.error(off).is_some());
    }

    #[test]
    fn errors_do_not_break_following_steps() {
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive {
            shape: Shape::Box { w: Expr::num(10.0), d: Expr::num(10.0), h: Expr::from("missing") },
            at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)],
            op: Op::NewBody,
        });
        d.insert(FeatureKind::Primitive {
            shape: Shape::Sphere { r: Expr::num(5.0) },
            at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)],
            op: Op::NewBody,
        });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.steps()[0].error.as_deref().unwrap().contains("missing"));
        assert_eq!(ev.model().bodies.len(), 1);
    }

    #[test]
    fn move_copy_pattern_mirror_combine() {
        let mut d = Document::default();
        let b = d.insert(FeatureKind::Primitive {
            shape: Shape::Box { w: Expr::num(10.0), d: Expr::num(10.0), h: Expr::num(10.0) },
            at: [Expr::num(5.0), Expr::num(0.0), Expr::num(0.0)],
            op: Op::NewBody,
        });
        let body = b * 1000;
        d.insert(FeatureKind::Mirror { bodies: vec![body], normal: Axis3::X, join: false, offset: Expr::num(0.0) });
        d.insert(FeatureKind::Pattern {
            bodies: vec![body],
            kind: PatternKind::Rect { nx: Expr::num(3.0), dx: Expr::num(20.0), ny: Expr::num(1.0), dy: Expr::num(0.0), nz: Expr::num(1.0), dz: Expr::num(0.0) },
        });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.steps().iter().all(|s| s.error.is_none()));
        let m = ev.model();
        assert_eq!(m.bodies.len(), 4);
        let (lo, _) = m.bodies[1].mesh().bounds().unwrap();
        assert!((lo.x + 15.0).abs() < 1e-9, "зеркало относительно YZ");
        let ids: Vec<u64> = m.bodies.iter().map(|b| b.id).collect();
        d.insert(FeatureKind::Combine { target: ids[0], tools: ids[1..].to_vec(), op: BoolOp::Union, keep_tools: false });
        d.insert(FeatureKind::Move {
            bodies: vec![ids[0]],
            translate: [Expr::num(0.0), Expr::num(0.0), Expr::num(7.0)],
            rotate: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)],
            copy: false,
        });
        ev.evaluate(&d);
        assert!(ev.steps().iter().all(|s| s.error.is_none()), "{:?}", ev.steps().last().unwrap().error);
        let m = ev.model();
        assert_eq!(m.bodies.len(), 1);
        assert!((m.bodies[0].solid.volume() - 4000.0).abs() < 1e-6);
        assert!((m.bodies[0].mesh().bounds().unwrap().0.z - 7.0).abs() < 1e-9);
    }

    #[test]
    fn revolve_feature() {
        let mut d = Document::default();
        let mut s = Sketch::default();
        s.add_rect([5.0, 0.0], [10.0, 4.0]);
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: s });
        d.insert(FeatureKind::Revolve { sketch: sk, profiles: vec![ProfileRef::at([7.0, 2.0])], axis: RevolveAxis::SketchY, angle: Expr::num(360.0), op: Op::NewBody });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.steps()[1].error.is_none(), "{:?}", ev.steps()[1].error);
        let v = ev.model().bodies[0].solid.volume();
        let exact = std::f64::consts::PI * 75.0 * 4.0;
        assert!((v - exact).abs() / exact < 0.01);
    }

    #[test]
    fn profile_survives_moving_geometry() {
        // прямоугольник, положение которого задано параметром: точка-«семя» оказывается снаружи,
        // но профиль узнаётся по кривым границы
        let mut d = Document::default();
        d.params.set("x", "0");
        let mut s = Sketch::default();
        let l = s.add_rect([0.0, 0.0], [10.0, 10.0]);
        let c = match s.entities[l[0]] {
            crate::sketch::Entity::Line { a, .. } => a,
            _ => unreachable!(),
        };
        s.constraints.push(crate::sketch::Constraint::Fix(c, [0.0, 0.0]));
        let o = s.add_point([0.0, -5.0]);
        s.add(crate::sketch::Entity::Line { a: o, b: c, construction: true });
        s.constraints.push(crate::sketch::Constraint::HDistance(o, c, Expr::from("x")));
        s.constraints.retain(|k| !matches!(k, crate::sketch::Constraint::Fix(..)));
        s.constraints.push(crate::sketch::Constraint::Fix(o, [0.0, -5.0]));
        s.constraints.push(crate::sketch::Constraint::Length(l[0], Expr::num(10.0)));
        s.constraints.push(crate::sketch::Constraint::Length(l[1], Expr::num(10.0)));
        let r = s.regions();
        let pref = ProfileRef::of(&r[0]);
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
        d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![pref], extent: Extent::OneSide(Expr::num(5.0)), op: Op::NewBody, face: None, taper: Expr::num(0.0), thin: Expr::num(0.0), start: Expr::num(0.0) });
        d.params.set("x", "100");
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.steps()[1].error.is_none(), "{:?}", ev.steps()[1].error);
        let (lo, _) = ev.model().bodies[0].mesh().bounds().unwrap();
        let st = ev.sketch(sk).unwrap();
        assert!((lo.x - 100.0).abs() < 1e-6, "{lo} {:?} {:?}", st.status, st.sketch.points);
    }

    #[test]
    fn json_roundtrip_and_delete() {
        let (mut d, sk) = plate_doc();
        d.body_materials.insert(2000, 3);
        d.cutter_bodies.insert(2000);
        let s = d.to_json();
        let back = Document::from_json(&s).unwrap();
        assert_eq!(back, d);
        assert!(Document::from_json("{}").is_err());
        let broken = d.delete(sk);
        assert_eq!(broken.len(), 1);
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.steps()[0].error.is_some());
    }
}

#[cfg(test)]
mod new_ops {
    use super::*;

    fn plate() -> (Document, u64) {
        let mut d = Document::default();
        let b = d.insert(FeatureKind::Primitive {
            shape: Shape::Box { w: Expr::num(40.0), d: Expr::num(30.0), h: Expr::num(10.0) },
            at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)],
            op: Op::NewBody,
        });
        (d, b * 1000)
    }

    fn run(d: &Document) -> Evaluator {
        let mut ev = Evaluator::default();
        ev.evaluate(d);
        for (i, s) in ev.steps().iter().enumerate() {
            assert!(s.error.is_none(), "шаг {i}: {:?}", s.error);
        }
        ev
    }

    #[test]
    fn holes_of_all_kinds() {
        let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        for (kind, depth, expect_min) in [
            (HoleKind::Simple, None, 12000.0 - std::f64::consts::PI * 25.0 * 10.0),
            (HoleKind::Simple, Some(Expr::num(4.0)), 12000.0 - std::f64::consts::PI * 25.0 * 4.0),
            (HoleKind::Counterbore { dia: Expr::num(16.0), depth: Expr::num(3.0) }, None, 12000.0 - std::f64::consts::PI * 64.0 * 3.0 - std::f64::consts::PI * 25.0 * 7.0),
            (HoleKind::Countersink { dia: Expr::num(14.0), angle: Expr::num(90.0) }, None, 0.0),
        ] {
            let (mut d, _) = plate();
            d.insert(FeatureKind::Hole { plane: top, at: [Expr::num(20.0), Expr::num(15.0)], dia: Expr::num(10.0), depth, kind });
            let ev = run(&d);
            let v = ev.model().bodies[0].solid.volume();
            assert!(v < 12000.0 - 100.0 && v > 12000.0 - 1500.0 && v >= expect_min - 80.0, "{v} {expect_min}");
        }
    }

    #[test]
    fn a_face_can_be_put_on_a_plane_from_any_side() {
        // 40 x 30 x 10 plate. Put its x = 40 end face on the floor (XY), then the same face on the XZ wall.
        let end = Plane { origin: [40.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };
        assert!((end.normal() - DVec3::X).length() < 1e-12);
        for (target, axis) in [(Plane::XY, 2), (Plane { origin: [0.0, 5.0, 0.0], ..Plane::XZ }.flipped(), 1)] {
            let (mut d, body) = plate();
            d.insert(FeatureKind::PlaceOnPlane { body, face: end, seed: [40.0, 15.0, 5.0], target, to: None });
            let ev = run(&d);
            assert_eq!(ev.model().bodies.len(), 1);
            let b = &ev.model().bodies[0];
            assert!((b.solid.volume() - 12000.0).abs() < 1.0, "a rigid move must keep the volume");
            let (lo, hi) = b.mesh().bounds().unwrap();
            let (min, max) = (lo.to_array()[axis], hi.to_array()[axis]);
            if axis == 2 {
                assert!(min.abs() < 1e-6, "the face must touch the floor, lowest point {min}");
                assert!((max - 40.0).abs() < 1e-6, "the plate is 40 long, so it now stands 40 high, got {max}");
            } else {
                // XZ flipped has its normal along +Y at y = 5: the body stands on the +Y side
                assert!((min - 5.0).abs() < 1e-6 && (max - 45.0).abs() < 1e-6, "{min}..{max}");
            }
        }
    }

    fn cube_with_edges(edges: &[([f64; 3], [f64; 3])], then: Option<&[([f64; 3], [f64; 3])]>) -> f64 {
        let n = Expr::num;
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(40.0), h: n(40.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let body = run(&d).model().bodies[0].id;
        d.insert(FeatureKind::Fillet { body, edges: edges.iter().map(|(a, b)| EdgeRef::line(*a, *b)).collect(), radius: n(5.0) });
        if let Some(more) = then {
            let body = run(&d).model().bodies[0].id;
            d.insert(FeatureKind::Fillet { body, edges: more.iter().map(|(a, b)| EdgeRef::line(*a, *b)).collect(), radius: n(5.0) });
        }
        run(&d).model().bodies[0].solid.volume()
    }

    #[test]
    fn a_cube_with_all_twelve_edges_rounded_gets_spherical_corners() {
        let mut edges = Vec::new();
        for z in [0.0, 40.0] {
            edges.extend([([0.0, 0.0, z], [40.0, 0.0, z]), ([40.0, 0.0, z], [40.0, 40.0, z]), ([40.0, 40.0, z], [0.0, 40.0, z]), ([0.0, 40.0, z], [0.0, 0.0, z])]);
        }
        for (x, y) in [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)] {
            edges.push(([x, y, 0.0], [x, y, 40.0]));
        }
        // a cube of side 30 grown by a ball of radius 5: a^3 + 6 a^2 r + 3 pi a r^2 + 4/3 pi r^3
        let (a, r) = (30.0_f64, 5.0_f64);
        let ideal = a.powi(3) + 6.0 * a * a * r + 3.0 * std::f64::consts::PI * a * r * r + 4.0 / 3.0 * std::f64::consts::PI * r.powi(3);
        let v = cube_with_edges(&edges, None);
        assert!((v - ideal).abs() / ideal < 0.002, "volume {v}, a ball-rounded cube has {ideal}");
    }

    #[test]
    fn rounding_an_edge_that_meets_an_earlier_fillet_continues_through_it() {
        let top = [([0.0, 0.0, 40.0], [40.0, 0.0, 40.0]), ([40.0, 0.0, 40.0], [40.0, 40.0, 40.0]), ([40.0, 40.0, 40.0], [0.0, 40.0, 40.0]), ([0.0, 40.0, 40.0], [0.0, 0.0, 40.0])];
        let verticals = [([0.0, 0.0, 0.0], [0.0, 0.0, 35.0]), ([40.0, 0.0, 0.0], [40.0, 0.0, 35.0]), ([40.0, 40.0, 0.0], [40.0, 40.0, 35.0]), ([0.0, 40.0, 0.0], [0.0, 40.0, 35.0])];
        let first = cube_with_edges(&top, None);
        let both = cube_with_edges(&top, Some(&verticals));
        // four vertical rounds of 5 mm over 35 mm remove 4 * 0.2146 * 25 * 35 = 751 mm3 on their own; the old result stopped at the fillet and
        // left the overhanging corners, removing only about 755 mm3 in total
        let removed = first - both;
        assert!(removed > 760.0 && removed < 840.0, "removed {removed}");
    }

    #[test]
    fn extreme_values_do_not_take_the_evaluator_down() {
        // absurd sizes may fail (or even panic inside the geometry code); either way `evaluate` returns
        let (mut d, _) = plate();
        d.insert(FeatureKind::Primitive { shape: Shape::Sphere { r: Expr::num(1e300) }, at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)], op: Op::Join });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert_eq!(ev.steps().len(), 2);
    }

    #[test]
    fn a_square_pipe_with_rounded_bends_has_the_expected_volume() {
        // L-shaped path 60 + 40 long; square 10 x 10 section, wall 1: area 100 - 64 = 36
        let mut d = Document::default();
        let mut path = Sketch::default();
        path.add_line([0.0, 0.0], [60.0, 0.0]);
        path.add_line([60.0, 0.0], [60.0, 40.0]);
        let ps = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: path });
        let mk = |bend: f64, shape: PipeShape| FeatureKind::Pipe { path: ps, outer: Expr::num(10.0), wall: Expr::num(1.0), op: Op::NewBody, shape, corner: Expr::num(0.0), bend: Expr::num(bend) };
        let uid = d.insert(mk(0.0, PipeShape::Square));
        let sharp = run(&d).model().bodies[0].solid.volume();
        assert!((sharp - 36.0 * 100.0).abs() / sharp < 0.08, "sharp square pipe: {sharp}");
        d.feature_mut(uid).unwrap().kind = mk(8.0, PipeShape::Square);
        let round = run(&d).model().bodies[0].solid.volume();
        // rounding the bend shortens the path a little (about r * (2 - pi/2) at the centre line)
        assert!(round < sharp && round > sharp * 0.9, "rounded bend {round} vs sharp {sharp}");
    }

    #[test]
    fn extrude_can_take_a_body_face_instead_of_a_sketch() {
        let (mut d, _) = plate();
        let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        d.insert(FeatureKind::Extrude { sketch: 0, profiles: vec![], extent: Extent::OneSide(Expr::num(5.0)), op: Op::Join, face: Some(FaceRef { plane: top, seed: [20.0, 15.0, 10.0] }), taper: Expr::num(0.0), thin: Expr::num(0.0), start: Expr::num(0.0) });
        let v = run(&d).model().bodies[0].solid.volume();
        assert!((v - (12000.0 + 40.0 * 30.0 * 5.0)).abs() < 1.0, "a 5 mm pull on the top face adds 6000 mm3, got {v}");
    }

    #[test]
    fn align_puts_a_face_onto_a_point_of_another_bodys_face() {
        // the plate's x = 40 end face goes onto the top of a stand at (100, 100, 20): the plate stands up there
        let (mut d, body) = plate();
        let end = Plane { origin: [40.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };
        let top = Plane { origin: [0.0, 0.0, 20.0], ..Plane::XY };
        d.insert(FeatureKind::PlaceOnPlane { body, face: end, seed: [40.0, 15.0, 5.0], target: top, to: Some([100.0, 100.0, 20.0]) });
        let ev = run(&d);
        let (lo, hi) = ev.model().bodies[0].mesh().bounds().unwrap();
        assert!((lo.z - 20.0).abs() < 1e-6 && (hi.z - 60.0).abs() < 1e-6, "{lo} {hi}");
        let centre = (lo + hi) / 2.0;
        assert!((centre.x - 100.0).abs() < 5.0 + 1e-6 && (centre.y - 100.0).abs() < 15.0 + 1e-6, "the face centre lands on the picked point: {centre}");
        // an old file without `to` still reads
        let json = serde_json::to_string(&d).unwrap().replace(",\"to\":[100.0,100.0,20.0]", "");
        assert!(Document::from_json(&json).is_ok());
    }

    #[test]
    fn pipe_follows_lines_drawn_one_by_one_whose_ends_only_touch() {
        // the line tool snaps to the grid, so a line can start exactly where the previous one ended without sharing its end point
        let mut d = Document::default();
        let mut path = Sketch::default();
        path.add_line([0.0, 0.0], [0.0, 30.0]);
        path.add_line([0.0, 30.0], [30.0, 30.0]);
        let ps = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: path });
        d.insert(FeatureKind::Pipe { path: ps, outer: Expr::num(8.0), wall: Expr::num(1.0), op: Op::NewBody, shape: PipeShape::Round, corner: Expr::num(0.0), bend: Expr::num(0.0) });
        let v = run(&d).model().bodies[0].solid.volume();
        assert!((v - std::f64::consts::PI * (16.0 - 9.0) * 60.0).abs() / v < 0.03, "{v}");
    }

    #[test]
    fn press_pull_pushes_and_pulls_a_planar_face_in_both_directions() {
        let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        for (distance, expect) in [(5.0, 40.0 * 30.0 * 15.0), (-4.0, 40.0 * 30.0 * 6.0)] {
            let (mut d, _) = plate();
            d.insert(FeatureKind::PressPull { plane: top, seed: [20.0, 15.0, 10.0], distance: Expr::num(distance) });
            let ev = run(&d);
            assert_eq!(ev.model().bodies.len(), 1);
            let v = ev.model().bodies[0].solid.volume();
            assert!((v - expect).abs() < 5.0, "{distance}: {v} != {expect}");
        }
    }

    #[test]
    fn press_pull_keeps_the_hole_of_a_face_with_an_opening() {
        let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let (mut d, _) = plate();
        d.insert(FeatureKind::Hole { plane: top, at: [Expr::num(20.0), Expr::num(15.0)], dia: Expr::num(10.0), depth: None, kind: HoleKind::Simple });
        let before = run(&d).model().bodies[0].solid.volume();
        d.insert(FeatureKind::PressPull { plane: top, seed: [1.0, 1.0, 10.0], distance: Expr::num(5.0) });
        let after = run(&d).model().bodies[0].solid.volume();
        let ring = (40.0 * 30.0 - std::f64::consts::PI * 25.0) * 5.0;
        assert!(((after - before) - ring).abs() < ring * 0.02, "{} vs {ring}", after - before);
    }

    #[test]
    fn editing_a_value_after_the_fact_rebuilds() {
        // правка числа в уже созданном шаге (отверстие, масштаб) должна менять модель при том же Evaluator
        let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let (mut d, body) = plate();
        let hole = d.insert(FeatureKind::Hole { plane: top, at: [Expr::num(20.0), Expr::num(15.0)], dia: Expr::num(6.0), depth: None, kind: HoleKind::Simple });
        d.insert(FeatureKind::Scale { bodies: vec![body], factor: [Expr::num(1.0), Expr::num(1.0), Expr::num(1.0)], uniform: true });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let v1 = ev.model().bodies[0].solid.volume();
        if let Some(f) = d.feature_mut(hole) {
            if let FeatureKind::Hole { dia, .. } = &mut f.kind {
                *dia = Expr::num(14.0);
            }
        }
        ev.evaluate(&d);
        let v2 = ev.model().bodies[0].solid.volume();
        assert!(v2 < v1 - 500.0, "объём не изменился: {v1} -> {v2}");
        assert!(ev.steps().iter().all(|s| s.error.is_none()));
    }

    #[test]
    fn shell_open_plate() {
        let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let (mut d, _) = plate();
        d.insert(FeatureKind::Shell { plane: top, thickness: Expr::num(2.0), open: true, more: vec![] });
        let ev = run(&d);
        let v = ev.model().bodies[0].solid.volume();
        assert!((v - 4512.0).abs() < 60.0, "{v}");
    }

    #[test]
    fn torus_coil_pipe_sweep_loft_scale() {
        let (mut d, body) = plate();
        d.insert(FeatureKind::Scale { bodies: vec![body], factor: [Expr::num(2.0), Expr::num(1.0), Expr::num(1.0)], uniform: true });
        let ev = run(&d);
        assert!((ev.model().bodies[0].solid.volume() - 12000.0 * 8.0).abs() < 1e-6);

        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Torus { major: Expr::num(10.0), minor: Expr::num(2.0) }, at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)], op: Op::NewBody });
        d.insert(FeatureKind::Coil { at: [Expr::num(50.0), Expr::num(0.0), Expr::num(0.0)], axis: Axis3::Z, dia: Expr::num(20.0), pitch: Expr::num(6.0), turns: Expr::num(4.0), section: Expr::num(2.0), op: Op::NewBody });
        let ev = run(&d);
        assert_eq!(ev.model().bodies.len(), 2);

        // труба и сдвиг по пути из чертежа
        let mut d = Document::default();
        let mut path = Sketch::default();
        let pts = [[0.0, 0.0], [0.0, 30.0], [30.0, 30.0]].map(|q| path.add_point(q));
        path.add(crate::sketch::Entity::Line { a: pts[0], b: pts[1], construction: false });
        path.add(crate::sketch::Entity::Line { a: pts[1], b: pts[2], construction: false });
        let ps = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: path });
        d.insert(FeatureKind::Pipe { path: ps, outer: Expr::num(8.0), wall: Expr::num(1.0), op: Op::NewBody, shape: PipeShape::Round, corner: Expr::num(0.0), bend: Expr::num(0.0) });
        let ev = run(&d);
        let v = ev.model().bodies[0].solid.volume();
        assert!((v - std::f64::consts::PI * (16.0 - 9.0) * 60.0).abs() / v < 0.03, "{v}");

        let mut prof = Sketch::default();
        prof.add_rect([-3.0, -3.0], [3.0, 3.0]);
        let r = prof.regions();
        let pr = ProfileRef::of(&r[0]);
        let s2 = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: prof });
        d.insert(FeatureKind::Sweep { sketch: s2, profiles: vec![pr], path: ps, op: Op::NewBody });
        let ev = run(&d);
        assert_eq!(ev.model().bodies.len(), 2);
        assert!((ev.model().bodies[1].solid.volume() - 36.0 * 60.0).abs() < 36.0 * 6.0);

        // лофт квадрат → круг на разной высоте
        let mut d = Document::default();
        let mut a = Sketch::default();
        a.add_rect([-5.0, -5.0], [5.0, 5.0]);
        let ra = ProfileRef::of(&a.regions()[0]);
        let sa = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: a });
        let mut b = Sketch::default();
        b.add_circle([0.0, 0.0], 4.0);
        let rb = ProfileRef::of(&b.regions()[0]);
        let mut up = Plane::XY;
        up.origin = [0.0, 0.0, 15.0];
        let sb = d.insert(FeatureKind::Sketch { plane: up, sketch: b });
        d.insert(FeatureKind::Loft { profiles: vec![(sa, ra), (sb, rb)], op: Op::NewBody });
        let ev = run(&d);
        assert!(ev.model().bodies[0].solid.volume() > 700.0);
    }

    #[test]
    fn chamfer_and_fillet_features() {
        let (mut d, body) = plate();
        let edges = vec![
            EdgeRef::line([0.0, 0.0, 10.0], [40.0, 0.0, 10.0]),
            EdgeRef::line([0.0, 30.0, 10.0], [40.0, 30.0, 10.0]),
        ];
        d.insert(FeatureKind::Chamfer { body, edges: edges.clone(), dist: Expr::num(2.0) });
        let ev = run(&d);
        assert!((ev.model().bodies[0].solid.volume() - (12000.0 - 2.0 * 0.5 * 4.0 * 40.0)).abs() < 1e-6);
        let (mut d, body) = plate();
        d.insert(FeatureKind::Fillet { body, edges, radius: Expr::num(3.0) });
        let ev = run(&d);
        assert!(ev.model().bodies[0].solid.volume() < 12000.0 - 100.0);
        // правка радиуса после создания меняет результат
        let (mut d, body) = plate();
        let fid = d.insert(FeatureKind::Fillet { body, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [40.0, 0.0, 10.0])], radius: Expr::num(1.0) });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let v1 = ev.model().bodies[0].solid.volume();
        if let FeatureKind::Fillet { radius, .. } = &mut d.feature_mut(fid).unwrap().kind {
            *radius = Expr::num(4.0);
        }
        ev.evaluate(&d);
        let v2 = ev.model().bodies[0].solid.volume();
        assert!(v2 < v1 - 20.0, "радиус не повлиял: {v1} -> {v2}");
        // нет ребра -> ошибка, а не паника
        let (mut d, body) = plate();
        d.insert(FeatureKind::Chamfer { body, edges: vec![EdgeRef::line([1.0, 1.0, 1.0], [2.0, 1.0, 1.0])], dist: Expr::num(1.0) });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.steps()[1].error.is_some());
    }
}

#[cfg(test)]
mod perf {
    use super::*;
    use crate::sketch::Constraint;

    /// Тяжёлая модель: пластина со 100 отверстиями, массив 10×10 и объединение 100 тел.
    /// Запуск: cargo test --release -p fusion-core heavy_model -- --ignored --nocapture
    #[test]
    #[ignore]
    fn heavy_model() {
        let mut d = Document::default();
        let mut s = Sketch::default();
        let l = s.add_rect([0.0, 0.0], [200.0, 200.0]);
        let _ = (l, Constraint::Horizontal(0));
        for i in 0..10 {
            for j in 0..10 {
                s.add_circle([10.0 + i as f64 * 20.0, 10.0 + j as f64 * 20.0], 4.0);
            }
        }
        let r = s.regions();
        let outer = r.iter().find(|r| r.holes.len() == 100).unwrap();
        let pref = ProfileRef::of(outer);
        let t = std::time::Instant::now();
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
        d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![pref], extent: Extent::OneSide(Expr::num(5.0)), op: Op::NewBody, face: None, taper: Expr::num(0.0), thin: Expr::num(0.0), start: Expr::num(0.0) });
        let b = d.insert(FeatureKind::Primitive {
            shape: Shape::Cylinder { r: Expr::num(3.0), h: Expr::num(10.0) },
            at: [Expr::num(20.0), Expr::num(20.0), Expr::num(5.0)],
            op: Op::NewBody,
        });
        d.insert(FeatureKind::Pattern {
            bodies: vec![b * 1000],
            kind: PatternKind::Rect { nx: Expr::num(9.0), dx: Expr::num(20.0), ny: Expr::num(9.0), dy: Expr::num(20.0), nz: Expr::num(1.0), dz: Expr::num(0.0) },
        });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        for (i, s) in ev.steps().iter().enumerate() {
            println!("шаг {i}: {:.1} мс {:?}", s.millis, s.error);
        }
        let ids: Vec<u64> = ev.model().bodies.iter().map(|b| b.id).collect();
        d.insert(FeatureKind::Combine { target: ids[0], tools: ids[1..].to_vec(), op: BoolOp::Union, keep_tools: false });
        let t2 = std::time::Instant::now();
        ev.evaluate(&d);
        println!("объединение {} тел: {:.1} мс", ids.len(), t2.elapsed().as_secs_f64() * 1000.0);
        let m = ev.model();
        let tris: usize = m.bodies.iter().map(|b| b.mesh().triangles.len()).sum();
        println!("итого: тел {}, треугольников {tris}, всё за {:.1} мс", m.bodies.len(), t.elapsed().as_secs_f64() * 1000.0);
        // правка одного шага в конце — пересчитывается только он
        let t3 = std::time::Instant::now();
        if let FeatureKind::Combine { keep_tools, .. } = &mut d.features.last_mut().unwrap().kind {
            *keep_tools = true;
        }
        let n = ev.evaluate(&d);
        println!("правка последнего шага: пересчитано {n}, {:.1} мс", t3.elapsed().as_secs_f64() * 1000.0);
        let t4 = std::time::Instant::now();
        let r = m.bodies[0].mesh().corner_normals(30.0);
        let e = m.bodies[0].mesh().sharp_edges(30.0);
        println!("данные для отрисовки: {:.1} мс ({} рёбер, {} углов)", t4.elapsed().as_secs_f64() * 1000.0, e.len(), r.len() * 3);
    }

    #[test]
    fn a_fillet_radius_that_does_not_fit_is_an_error_not_a_collapsed_body() {
        let n = Expr::num;
        let s = 40.0;
        let corners = [[0.0, 0.0], [s, 0.0], [s, s], [0.0, s]];
        let mut edges = Vec::new();
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            edges.push(EdgeRef::line([a[0], a[1], 0.0], [b[0], b[1], 0.0]));
            edges.push(EdgeRef::line([a[0], a[1], s], [b[0], b[1], s]));
            edges.push(EdgeRef::line([a[0], a[1], 0.0], [a[0], a[1], s]));
        }
        for (radius, fits) in [(5.0, true), (20.0, true), (25.0, false)] {
            let mut d = Document::default();
            d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(s), d: n(s), h: n(s) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
            let mut ev = Evaluator::default();
            ev.evaluate(&d);
            let body = ev.model().bodies[0].id;
            d.insert(FeatureKind::Fillet { body, edges: edges.clone(), radius: n(radius) });
            let mut ev = Evaluator::default();
            ev.evaluate(&d);
            let err = ev.steps().iter().find_map(|st| st.error.clone());
            assert_eq!(err.is_none(), fits, "radius {radius}: {err:?}");
            if fits {
                let v = ev.model().bodies[0].solid.volume();
                assert!(v > 0.5 * s * s * s && v < s * s * s, "radius {radius}: volume {v}");
            }
        }
    }
}

#[cfg(test)]
mod draft_thread_tests {
    use super::*;

    fn run(d: &Document) -> Evaluator {
        let mut ev = Evaluator::default();
        ev.evaluate(d);
        ev
    }

    #[test]
    fn draft_tilts_box_walls_and_keeps_the_volume_formula() {
        let n = Expr::num;
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(20.0), d: n(20.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let id = run(&d).model().bodies[0].id;
        let uid = d.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(5.0) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let b = &ev.model().bodies[0];
        let (lo, hi) = b.mesh().bounds().unwrap();
        let shrink = 10.0 * 5f64.to_radians().tan();
        let top: Vec<_> = b.mesh().vertices.iter().filter(|v| (v[2] - 10.0).abs() < 1e-6).collect();
        assert!(top.iter().all(|v| (v[0] - shrink).abs() < 1e-6 || (v[0] - (20.0 - shrink)).abs() < 1e-6), "{top:?}");
        assert!(lo[0].abs() < 1e-9 && (hi[0] - 20.0).abs() < 1e-9, "bottom stays on the neutral plane");
        let (w0, w1) = (20.0, 20.0 - 2.0 * shrink);
        let expect = 10.0 / 3.0 * (w0 * w0 + w0 * w1 + w1 * w1);
        assert!((b.mesh().volume() - expect).abs() < 1e-3, "{} vs {}", b.mesh().volume(), expect);
        let bad = d.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(60.0) });
        assert!(run(&d).error(bad).is_some());
    }

    #[test]
    fn draft_works_on_a_cylinder_and_in_the_opposite_direction() {
        let n = Expr::num;
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(10.0), h: n(20.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let id = run(&d).model().bodies[0].id;
        let before = run(&d).model().bodies[0].mesh().volume();
        let up = d.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(3.0) });
        let v_up = run(&d).model().bodies[0].mesh().volume();
        assert!(run(&d).error(up).is_none() && v_up < before);
        let mut d2 = Document::default();
        d2.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(10.0), h: n(20.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        d2.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(-3.0) });
        assert!(run(&d2).model().bodies[0].mesh().volume() > before);
    }

    #[test]
    fn draft_after_a_fillet_still_builds() {
        let n = Expr::num;
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(30.0), d: n(30.0), h: n(15.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let id = run(&d).model().bodies[0].id;
        let edges = vec![EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, 15.0])];
        d.insert(FeatureKind::Fillet { body: id, edges, radius: n(3.0) });
        let uid = d.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(2.0) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
    }

    #[test]
    fn thread_builds_a_ribbed_rod_on_every_axis_and_joins_to_a_body() {
        let n = Expr::num;
        for axis in [Axis3::X, Axis3::Y, Axis3::Z] {
            let mut d = Document::default();
            let uid = d.insert(FeatureKind::Thread { at: [n(5.0), n(5.0), n(5.0)], axis, dia: n(10.0), pitch: n(2.0), length: n(12.0), depth: n(0.8), op: Op::NewBody });
            let ev = run(&d);
            assert!(ev.error(uid).is_none(), "{axis:?}: {:?}", ev.error(uid));
            let b = &ev.model().bodies[0];
            let core = std::f64::consts::PI * 4.2f64.powi(2) * 12.0;
            let full = std::f64::consts::PI * 25.0 * 12.0;
            let v = b.mesh().volume();
            assert!(v > core * 1.02 && v < full, "{axis:?}: volume {v} between {core} and {full}");
            let (lo, hi) = b.mesh().bounds().unwrap();
            let ext = hi - lo;
            let long = match axis { Axis3::X => ext.x, Axis3::Y => ext.y, Axis3::Z => ext.z };
            assert!((long - 12.0).abs() < 1e-6, "{axis:?}: length {long}");
        }
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(8.0), h: n(5.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        d.insert(FeatureKind::Thread { at: [n(0.0), n(0.0), n(5.0)], axis: Axis3::Z, dia: n(10.0), pitch: n(2.0), length: n(12.0), depth: n(0.8), op: Op::Join });
        assert_eq!(run(&d).model().bodies.len(), 1);
        let bad = d.insert(FeatureKind::Thread { at: [n(0.0), n(0.0), n(0.0)], axis: Axis3::Z, dia: n(10.0), pitch: n(2.0), length: n(12.0), depth: n(6.0), op: Op::NewBody });
        assert!(run(&d).error(bad).is_some());
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;

    fn cube_stl_binary() -> Vec<u8> {
        let b = geom::box_mesh(DVec3::ZERO, DVec3::new(10.0, 20.0, 30.0));
        let mut out = Vec::new();
        Mesh::write_stl_binary(&[&b], &mut out);
        out
    }

    #[test]
    fn binary_and_ascii_stl_round_trip_to_the_same_volume() {
        let bin = cube_stl_binary();
        let m = Mesh::read_stl(&bin).unwrap();
        assert!((m.volume() - 6000.0).abs() < 1e-3, "{}", m.volume());
        let mut ascii = String::from("solid t\n");
        for t in &m.triangles {
            ascii.push_str("facet normal 0 0 0\nouter loop\n");
            for &i in t {
                let v = m.vertices[i as usize];
                ascii.push_str(&format!("vertex {} {} {}\n", v[0], v[1], v[2]));
            }
            ascii.push_str("endloop\nendfacet\n");
        }
        ascii.push_str("endsolid t\n");
        let a = Mesh::read_stl(ascii.as_bytes()).unwrap();
        assert!((a.volume() - 6000.0).abs() < 1e-3);
        assert!(Mesh::read_stl(b"hello world").is_err());
        assert!(Mesh::read_stl(&bin[..90]).is_err());
    }

    #[test]
    fn inside_out_stl_is_turned_around() {
        let mut m = geom::box_mesh(DVec3::ZERO, DVec3::splat(5.0));
        m.flip();
        let mut out = Vec::new();
        Mesh::write_stl_binary(&[&m], &mut out);
        assert!(Mesh::read_stl(&out).unwrap().volume() > 0.0);
    }

    #[test]
    fn imported_feature_becomes_a_body_survives_json_and_can_be_edited() {
        let m = Mesh::read_stl(&cube_stl_binary()).unwrap();
        let verts: Vec<f32> = m.vertices.iter().flatten().map(|&c| c as f32).collect();
        let tris: Vec<u32> = m.triangles.iter().flatten().copied().collect();
        let mut d = Document::default();
        d.insert(FeatureKind::Import { name: "cube.stl".into(), verts, tris });
        let json = serde_json::to_string(&d).unwrap();
        let d: Document = serde_json::from_str(&json).unwrap();
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert_eq!(ev.model().bodies.len(), 1);
        assert!((ev.model().bodies[0].mesh().volume() - 6000.0).abs() < 1e-2);
        let mut d = d;
        let id = ev.model().bodies[0].id;
        let uid = d.insert(FeatureKind::Split { bodies: vec![id], axis: Axis3::Z, offset: Expr::num(10.0) });
        ev.evaluate(&d);
        assert!(ev.error(uid).is_none());
        assert_eq!(ev.model().bodies.len(), 2);
    }

    #[test]
    fn an_open_mesh_gives_a_clear_error_not_a_crash() {
        let mut m = geom::box_mesh(DVec3::ZERO, DVec3::splat(5.0));
        m.triangles.truncate(m.triangles.len() - 2);
        let verts: Vec<f32> = m.vertices.iter().flatten().map(|&c| c as f32).collect();
        let tris: Vec<u32> = m.triangles.iter().flatten().copied().collect();
        let mut d = Document::default();
        let uid = d.insert(FeatureKind::Import { name: "open.stl".into(), verts, tris });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.error(uid).is_some_and(|e| e.contains("не замкнута")), "{:?}", ev.error(uid));
    }
}

#[cfg(test)]
mod modeling_batch_tests {
    use super::*;
    use crate::sketch::Entity;

    fn n(v: f64) -> Expr {
        Expr::num(v)
    }

    fn run(d: &Document) -> Evaluator {
        let mut ev = Evaluator::default();
        ev.evaluate(d);
        ev
    }

    fn square_sketch(d: &mut Document, side: f64) -> u64 {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [side, side]);
        d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s })
    }

    fn extrude(sk: u64, extent: Extent, op: Op, taper: f64) -> FeatureKind {
        FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([1.0, 1.0])], extent, op, face: None, taper: n(taper), thin: Expr::num(0.0), start: Expr::num(0.0) }
    }

    #[test]
    fn taper_extrude_narrows_the_walls_like_a_frustum() {
        let mut d = Document::default();
        let sk = square_sketch(&mut d, 20.0);
        let uid = d.insert(extrude(sk, Extent::OneSide(n(10.0)), Op::NewBody, 5.0));
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let shrink = 10.0 * 5f64.to_radians().tan();
        let w1 = 20.0 - 2.0 * shrink;
        let expect = 10.0 / 3.0 * (400.0 + 20.0 * w1 + w1 * w1);
        let v = ev.model().bodies[0].mesh().volume();
        assert!((v - expect).abs() < 1e-3, "{v} vs {expect}");
        // a negative taper widens it, zero keeps the plain prism
        let mut d2 = Document::default();
        let sk2 = square_sketch(&mut d2, 20.0);
        d2.insert(extrude(sk2, Extent::OneSide(n(10.0)), Op::NewBody, -5.0));
        assert!(run(&d2).model().bodies[0].mesh().volume() > 4000.0);
        let mut d3 = Document::default();
        let sk3 = square_sketch(&mut d3, 20.0);
        d3.insert(extrude(sk3, Extent::OneSide(n(10.0)), Op::NewBody, 0.0));
        assert!((run(&d3).model().bodies[0].mesh().volume() - 4000.0).abs() < 1e-6);
    }

    #[test]
    fn taper_works_downwards_symmetrically_and_rejects_big_angles() {
        let mut d = Document::default();
        let sk = square_sketch(&mut d, 20.0);
        let down = d.insert(extrude(sk, Extent::OneSide(n(-10.0)), Op::NewBody, 5.0));
        let ev = run(&d);
        assert!(ev.error(down).is_none(), "{:?}", ev.error(down));
        let shrink = 10.0 * 5f64.to_radians().tan();
        let w1 = 20.0 - 2.0 * shrink;
        let expect = 10.0 / 3.0 * (400.0 + 20.0 * w1 + w1 * w1);
        assert!((ev.model().bodies[0].mesh().volume() - expect).abs() < 1e-3);
        let mut d2 = Document::default();
        let sk2 = square_sketch(&mut d2, 20.0);
        let sym = d2.insert(extrude(sk2, Extent::Symmetric(n(10.0)), Op::NewBody, 5.0));
        let ev2 = run(&d2);
        assert!(ev2.error(sym).is_none(), "{:?}", ev2.error(sym));
        let half = 5.0 / 3.0 * (400.0 + 20.0 * (20.0 - 2.0 * 5.0 * 5f64.to_radians().tan()) + (20.0 - 2.0 * 5.0 * 5f64.to_radians().tan()).powi(2));
        assert!((ev2.model().bodies[0].mesh().volume() - 2.0 * half).abs() < 1e-2);
        let mut d3 = Document::default();
        let sk3 = square_sketch(&mut d3, 20.0);
        let bad = d3.insert(extrude(sk3, Extent::OneSide(n(10.0)), Op::NewBody, 50.0));
        assert!(run(&d3).error(bad).is_some());
    }

    #[test]
    fn through_all_cuts_the_whole_body_in_either_direction() {
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(40.0), h: n(30.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let mut s = Sketch::default();
        s.add_circle([20.0, 20.0], 5.0);
        let sk = d.insert(FeatureKind::Sketch { plane: Plane { origin: [0.0, 0.0, 30.0], ..Plane::XY }, sketch: s });
        // cutting from the top face downwards needs the reverse; `All(true)` reaches both ways and never misses
        let uid = d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([20.0, 20.0])], extent: Extent::All(true), op: Op::Cut, face: None, taper: n(0.0), thin: Expr::num(0.0), start: Expr::num(0.0) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let v = ev.model().bodies[0].mesh().volume();
        let hole = std::f64::consts::PI * 25.0 * 30.0;
        assert!((v - (48000.0 - hole)).abs() < hole * 0.02, "{v}");
        // one-way, forward from a sketch on the bottom face goes through the full height too
        let mut d2 = Document::default();
        d2.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(40.0), h: n(30.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let mut s2 = Sketch::default();
        s2.add_circle([20.0, 20.0], 5.0);
        let sk2 = d2.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s2 });
        let uid2 = d2.insert(FeatureKind::Extrude { sketch: sk2, profiles: vec![ProfileRef::at([20.0, 20.0])], extent: Extent::All(false), op: Op::Cut, face: None, taper: n(0.0), thin: Expr::num(0.0), start: Expr::num(0.0) });
        let ev2 = run(&d2);
        assert!(ev2.error(uid2).is_none());
        assert!((ev2.model().bodies[0].mesh().volume() - (48000.0 - hole)).abs() < hole * 0.02);
    }

    fn plate_with_hole(kind: HoleKind, dia: f64, depth: Option<f64>) -> (Evaluator, u64) {
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(30.0), d: n(30.0), h: n(12.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let top = Plane { origin: [0.0, 0.0, 12.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let uid = d.insert(FeatureKind::Hole { plane: top, at: [n(15.0), n(15.0)], dia: n(dia), depth: depth.map(n), kind });
        (run(&d), uid)
    }

    #[test]
    fn tapped_hole_is_a_tap_drill_and_modeled_thread_cuts_a_groove() {
        use crate::fasteners::find;
        let m6 = find("M6").unwrap();
        let plate = 30.0 * 30.0 * 12.0;
        let (ev, uid) = plate_with_hole(HoleKind::Tapped { major: n(m6.d), pitch: n(m6.pitch), modeled: false }, m6.tap_drill, None);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let drill = std::f64::consts::PI * (m6.tap_drill / 2.0f64).powi(2) * 12.0;
        let v0 = ev.model().bodies[0].mesh().volume();
        assert!((plate - v0 - drill).abs() < drill * 0.03, "cosmetic thread = plain drill: removed {}", plate - v0);
        let (ev2, uid2) = plate_with_hole(HoleKind::Tapped { major: n(m6.d), pitch: n(m6.pitch), modeled: true }, m6.tap_drill, None);
        assert!(ev2.error(uid2).is_none(), "{:?}", ev2.error(uid2));
        let v1 = ev2.model().bodies[0].mesh().volume();
        let full = std::f64::consts::PI * (m6.d / 2.0f64).powi(2) * 12.0;
        assert!(plate - v1 > drill * 1.05 && plate - v1 < full, "groove removes more than the drill but less than the full bore: {}", plate - v1);
        assert_eq!(ev2.model().bodies.len(), 1, "still one solid");
        // a blind threaded hole keeps its floor
        let (ev3, uid3) = plate_with_hole(HoleKind::Tapped { major: n(m6.d), pitch: n(m6.pitch), modeled: true }, m6.tap_drill, Some(8.0));
        assert!(ev3.error(uid3).is_none(), "{:?}", ev3.error(uid3));
        let (lo, _) = ev3.model().bodies[0].mesh().bounds().unwrap();
        assert!(lo.z.abs() < 1e-9);
        let v3 = ev3.model().bodies[0].mesh().volume();
        assert!(plate - v3 > 0.0 && plate - v3 < full * 8.0 / 12.0 * 1.01, "{}", plate - v3);
        // the thread must be wider than the drill
        let (_, bad) = plate_with_hole(HoleKind::Tapped { major: n(4.0), pitch: n(0.7), modeled: false }, 5.0, None);
        assert!(plate_with_hole(HoleKind::Tapped { major: n(4.0), pitch: n(0.7), modeled: false }, 5.0, None).0.error(bad).is_some());
    }

    #[test]
    fn bolt_presets_build_clearance_counterbore_and_countersink_holes() {
        use crate::fasteners::{find, Fit};
        let m5 = find("M5").unwrap();
        let (ev, uid) = plate_with_hole(HoleKind::Counterbore { dia: n(m5.cbore.0), depth: n(m5.cbore.1) }, m5.clearance_dia(Fit::Normal), None);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let (ev2, uid2) = plate_with_hole(HoleKind::Countersink { dia: n(m5.csk), angle: n(90.0) }, m5.clearance_dia(Fit::Close), None);
        assert!(ev2.error(uid2).is_none(), "{:?}", ev2.error(uid2));
        let plain = 30.0 * 30.0 * 12.0 - std::f64::consts::PI * (m5.clearance_dia(Fit::Close) / 2.0f64).powi(2) * 12.0;
        assert!(ev2.model().bodies[0].mesh().volume() < plain, "the countersink removes extra material");
    }

    #[test]
    fn text_extrudes_into_letters_and_an_ellipse_into_an_oval_post() {
        let mut d = Document::default();
        let mut s = Sketch::default();
        s.texts.push(crate::text::SketchText { text: "O".into(), pos: [0.0, 0.0], height: 20.0, angle: 0.0 });
        let region = s.regions().remove(0);
        let at = region.interior_point();
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
        let uid = d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at(at)], extent: Extent::OneSide(n(3.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let v = ev.model().bodies[0].mesh().volume();
        assert!((v - region.area() * 3.0).abs() < 1e-6, "letter ring with its hole: {v} vs {}", region.area() * 3.0);
        // an ellipse post
        let mut d2 = Document::default();
        let mut e = Sketch::default();
        let c = e.add_point([0.0, 0.0]);
        let a = e.add_point([10.0, 0.0]);
        e.add(Entity::Ellipse { c, a, ratio: 0.5, construction: false });
        let sk2 = d2.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: e });
        let post = d2.insert(FeatureKind::Extrude { sketch: sk2, profiles: vec![ProfileRef::at([0.0, 0.0])], extent: Extent::OneSide(n(8.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
        let ev2 = run(&d2);
        assert!(ev2.error(post).is_none(), "{:?}", ev2.error(post));
        let expect = std::f64::consts::PI * 10.0 * 5.0 * 8.0;
        assert!((ev2.model().bodies[0].mesh().volume() - expect).abs() < expect * 0.01);
    }

    #[test]
    fn shell_with_several_open_faces_removes_every_chosen_wall() {
        let build = |more: Vec<Plane>| {
            let mut d = Document::default();
            d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(30.0), d: n(30.0), h: n(20.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
            let top = Plane { origin: [0.0, 0.0, 20.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
            let uid = d.insert(FeatureKind::Shell { plane: top, thickness: n(2.0), open: true, more });
            let ev = run(&d);
            (ev.error(uid).map(|e| e.to_string()), ev.model().bodies.iter().map(|b| b.mesh().volume()).collect::<Vec<_>>())
        };
        // only the top open: cavity 26 x 26 x 18
        let (err, v) = build(vec![]);
        assert!(err.is_none(), "{err:?}");
        assert!((v[0] - (18000.0 - 26.0 * 26.0 * 18.0)).abs() < 18000.0 * 0.01, "{v:?}");
        // top and the +x wall open: cavity 28 x 26 x 18
        let side = Plane { origin: [30.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };
        let (err2, v2) = build(vec![side]);
        assert!(err2.is_none(), "{err2:?}");
        assert_eq!(v2.len(), 1);
        assert!((v2[0] - (18000.0 - 28.0 * 26.0 * 18.0)).abs() < 18000.0 * 0.01, "two open faces: {v2:?}");
        assert!(v2[0] < v[0], "an extra opening takes away more material");
        // an old document without `more` still loads and means one open face
        let json = serde_json::to_string(&FeatureKind::Shell { plane: Plane::XY, thickness: n(1.0), open: true, more: vec![] }).unwrap();
        let old = json.replace(r#","more":[]"#, "");
        assert!(serde_json::from_str::<FeatureKind>(&old).is_ok(), "{old}");
    }

    #[test]
    fn thread_on_an_existing_boss_and_in_an_existing_hole() {
        let pi = std::f64::consts::PI;
        // a boss: cylinder r = 5, 20 tall, thread from its top rim downwards
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(5.0), h: n(20.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let id = run(&d).model().bodies[0].id;
        let rim = EdgeRef::round([5.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0]);
        let uid = d.insert(FeatureKind::ThreadOn { body: id, edge: rim, length: n(12.0), pitch: n(2.0), depth: n(0.8) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        assert_eq!(ev.model().bodies.len(), 1, "the ridge fuses into the boss");
        let v = ev.model().bodies[0].mesh().volume();
        let (plain, fat) = (pi * 25.0 * 20.0, pi * 5.8f64.powi(2) * 12.0 + pi * 25.0 * 8.0);
        assert!(v > plain * 1.01 && v < fat, "external thread adds some material: {v} between {plain} and {fat}");
        let (_, hi) = ev.model().bodies[0].mesh().bounds().unwrap();
        assert!(hi.x > 5.4 && hi.x < 5.81, "the ridge reaches about r + depth: {}", hi.x);
        // a hole: plate 30 x 30 x 14 with a through hole r = 4, thread from the top rim downwards
        let mut h = Document::default();
        h.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(30.0), d: n(30.0), h: n(14.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let top = Plane { origin: [0.0, 0.0, 14.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        h.insert(FeatureKind::Hole { plane: top, at: [n(15.0), n(15.0)], dia: n(8.0), depth: None, kind: HoleKind::Simple });
        let pid = run(&h).model().bodies[0].id;
        let before = run(&h).model().bodies[0].mesh().volume();
        let rim2 = EdgeRef::round([19.0, 15.0, 14.0], [15.0, 15.0, 14.0], [0.0, 0.0, 1.0]);
        let uid2 = h.insert(FeatureKind::ThreadOn { body: pid, edge: rim2, length: n(10.0), pitch: n(1.5), depth: n(0.6) });
        let ev2 = run(&h);
        assert!(ev2.error(uid2).is_none(), "{:?}", ev2.error(uid2));
        let after = ev2.model().bodies[0].mesh().volume();
        assert!(after < before - 1.0 && after > before - pi * 4.6f64.powi(2) * 10.0, "internal thread cuts a groove: {after} vs {before}");
        // a rim in the air and a plain box edge are refused with a message, never a crash
        let air = EdgeRef::round([60.0, 0.0, 0.0], [55.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
        let bad = h.insert(FeatureKind::ThreadOn { body: pid, edge: air, length: n(10.0), pitch: n(1.5), depth: n(0.6) });
        assert!(run(&h).error(bad).is_some());
        let line = h.insert(FeatureKind::ThreadOn { body: pid, edge: EdgeRef::line([0.0; 3], [1.0, 0.0, 0.0]), length: n(10.0), pitch: n(1.5), depth: n(0.6) });
        assert!(run(&h).error(line).is_some());
    }

    #[test]
    fn extrude_up_to_a_body_and_with_a_start_offset() {
        let mut d = Document::default();
        let sk = square_sketch(&mut d, 20.0);
        // a slab 50 mm above the profile plane
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(60.0), d: n(60.0), h: n(5.0) }, at: [n(-10.0), n(-10.0), n(50.0)], op: Op::NewBody });
        let slab = run(&d).model().bodies[0].id;
        let uid = d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::ToBody(slab), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let model = ev.model();
        let post = model.bodies.iter().find(|b| b.id != slab).unwrap();
        let (lo, hi) = post.mesh().bounds().unwrap();
        assert!(lo.z.abs() < 1e-6 && (hi.z - 50.0).abs() < 1e-6, "reaches the slab: {lo:?} {hi:?}");
        // a missing or a behind-the-profile body is an error, not a crash
        let bad = d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::ToBody(99999), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
        assert!(run(&d).error(bad).is_some());
        // start offset shifts the whole prism along the normal
        let mut d2 = Document::default();
        let sk2 = square_sketch(&mut d2, 20.0);
        let off = d2.insert(FeatureKind::Extrude { sketch: sk2, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::OneSide(n(10.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(7.0) });
        let ev2 = run(&d2);
        assert!(ev2.error(off).is_none());
        let (lo2, hi2) = ev2.model().bodies[0].mesh().bounds().unwrap();
        assert!((lo2.z - 7.0).abs() < 1e-6 && (hi2.z - 17.0).abs() < 1e-6, "{lo2:?} {hi2:?}");
    }

    #[test]
    fn grid_pattern_stacks_copies_along_z() {
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(5.0), d: n(5.0), h: n(5.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let id = run(&d).model().bodies[0].id;
        let uid = d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Rect { nx: n(2.0), dx: n(10.0), ny: n(2.0), dy: n(10.0), nz: n(3.0), dz: n(8.0) } });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        assert_eq!(ev.model().bodies.len(), 12);
        let top = ev.model().bodies.iter().filter_map(|b| b.mesh().bounds()).map(|(_, hi)| hi.z).fold(0.0, f64::max);
        assert!((top - 21.0).abs() < 1e-9, "{top}");
        // a flat grid (nz = 1) is the same as before
        let mut flat = Document::default();
        flat.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(5.0), d: n(5.0), h: n(5.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let id2 = run(&flat).model().bodies[0].id;
        flat.insert(FeatureKind::Pattern { bodies: vec![id2], kind: PatternKind::Rect { nx: n(2.0), dx: n(10.0), ny: n(2.0), dy: n(10.0), nz: n(1.0), dz: n(0.0) } });
        assert_eq!(run(&flat).model().bodies.len(), 4);
    }

    #[test]
    fn thin_extrude_leaves_only_a_wall_along_the_outline() {
        let mut d = Document::default();
        let sk = square_sketch(&mut d, 20.0);
        let uid = d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::OneSide(n(10.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(2.0), start: Expr::num(0.0) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let v = ev.model().bodies[0].mesh().volume();
        // 20x20 outline with a 16x16 opening, 10 mm tall; the disc is a polygon so allow a little slack
        assert!((v - 1440.0).abs() < 1440.0 * 0.04, "{v}");
        let (lo, hi) = ev.model().bodies[0].mesh().bounds().unwrap();
        assert!(lo.z.abs() < 1e-6 && (hi.z - 10.0).abs() < 1e-6, "the height stays 10: {lo:?} {hi:?}");
        // a wall as thick as half the profile (or more) cannot be thinned
        let mut d2 = Document::default();
        let sk2 = square_sketch(&mut d2, 20.0);
        let bad = d2.insert(FeatureKind::Extrude { sketch: sk2, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::OneSide(n(10.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(15.0), start: Expr::num(0.0) });
        assert!(run(&d2).error(bad).is_some());
        let neg = d2.insert(FeatureKind::Extrude { sketch: sk2, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::OneSide(n(10.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(-1.0), start: Expr::num(0.0) });
        assert!(run(&d2).error(neg).is_some());
    }

    #[test]
    fn thin_extrude_works_around_a_hole_and_with_a_taper() {
        let mut d = Document::default();
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [40.0, 40.0]);
        s.add_circle([20.0, 20.0], 8.0);
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
        let uid = d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([2.0, 2.0])], extent: Extent::OneSide(n(12.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(3.0), start: Expr::num(0.0) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let plain = (1600.0 - std::f64::consts::PI * 64.0) * 12.0;
        let v = ev.model().bodies[0].mesh().volume();
        assert!(v > 0.0 && v < plain * 0.8, "walls around the outline and the hole only: {v} vs {plain}");
        let mut d2 = Document::default();
        let sk2 = square_sketch(&mut d2, 30.0);
        let tapered = d2.insert(FeatureKind::Extrude { sketch: sk2, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::OneSide(n(10.0)), op: Op::NewBody, face: None, taper: n(4.0), thin: n(2.0), start: Expr::num(0.0) });
        assert!(run(&d2).error(tapered).is_none(), "{:?}", run(&d2).error(tapered));
    }

    #[test]
    fn mirror_about_an_offset_plane_lands_on_the_far_side() {
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(10.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let id = run(&d).model().bodies[0].id;
        d.insert(FeatureKind::Mirror { bodies: vec![id], normal: Axis3::X, join: false, offset: n(20.0) });
        let ev = run(&d);
        assert_eq!(ev.model().bodies.len(), 2);
        let (lo, hi) = ev.model().bodies[1].mesh().bounds().unwrap();
        assert!((lo.x - 30.0).abs() < 1e-9 && (hi.x - 40.0).abs() < 1e-9, "{lo:?} {hi:?}");
        assert!((ev.model().bodies[1].mesh().volume() - 1000.0).abs() < 1e-6);
    }

    #[test]
    fn combine_split_cuts_a_body_with_another_body() {
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(20.0), d: n(20.0), h: n(20.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(30.0), h: n(30.0) }, at: [n(5.0), n(-5.0), n(-5.0)], op: Op::NewBody });
        let ids: Vec<u64> = run(&d).model().bodies.iter().map(|b| b.id).collect();
        let uid = d.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[1]], op: BoolOp::Split, keep_tools: false });
        let ev = run(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        assert_eq!(ev.model().bodies.len(), 2, "tool is consumed, the target splits in two");
        let total: f64 = ev.model().bodies.iter().map(|b| b.mesh().volume()).sum();
        assert!((total - 8000.0).abs() < 1e-6, "{total}");
        let mut vols: Vec<f64> = ev.model().bodies.iter().map(|b| b.mesh().volume()).collect();
        vols.sort_by(f64::total_cmp);
        assert!((vols[0] - 4000.0).abs() < 1e-6 && (vols[1] - 4000.0).abs() < 1e-6, "{vols:?}");
        // a tool that misses the body does not split it
        let mut d2 = Document::default();
        d2.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(10.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        d2.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(5.0), d: n(5.0), h: n(5.0) }, at: [n(50.0), n(0.0), n(0.0)], op: Op::NewBody });
        let ids2: Vec<u64> = run(&d2).model().bodies.iter().map(|b| b.id).collect();
        let bad = d2.insert(FeatureKind::Combine { target: ids2[0], tools: vec![ids2[1]], op: BoolOp::Split, keep_tools: true });
        assert!(run(&d2).error(bad).is_some());
    }

    #[test]
    fn angle_plane_turns_about_the_base_x_axis_and_hosts_a_sketch() {
        let mut d = Document::default();
        let uid = d.insert(FeatureKind::AnglePlane { base: Plane::XY, angle: n(90.0) });
        let ev = run(&d);
        assert!(ev.error(uid).is_none());
        let planes = d.offset_planes();
        assert_eq!(planes.len(), 1);
        let pl = planes[0].2;
        // XY turned 90 degrees about X becomes the XZ-like wall: its normal lies along -Y
        assert!((pl.normal().dot(DVec3::NEG_Y) - 1.0).abs() < 1e-9 || (pl.normal().dot(DVec3::Y) - 1.0).abs() < 1e-9, "{:?}", pl.normal());
        let bad = d.insert(FeatureKind::AnglePlane { base: Plane::XY, angle: n(200.0) });
        assert!(run(&d).error(bad).is_some());
    }

    #[test]
    fn old_files_without_the_new_fields_still_open() {
        let mut d = Document::default();
        let sk = square_sketch(&mut d, 10.0);
        d.insert(extrude(sk, Extent::OneSide(n(5.0)), Op::NewBody, 0.0));
        d.insert(FeatureKind::Mirror { bodies: vec![], normal: Axis3::X, join: false, offset: n(0.0) });
        let text = d.to_json();
        let stripped = text.replace(r#""taper""#, r#""taper_unused""#).replace(r#""offset""#, r#""offset_unused""#);
        let back = Document::from_json(&stripped);
        assert!(back.is_ok(), "{:?}", back.err());
    }
}

#[cfg(test)]
mod interference_tests {
    use super::*;

    #[test]
    fn overlapping_bodies_are_reported_with_their_shared_volume() {
        let n = Expr::num;
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(10.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(10.0), h: n(10.0) }, at: [n(5.0), n(0.0), n(0.0)], op: Op::NewBody });
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(10.0), h: n(10.0) }, at: [n(50.0), n(0.0), n(0.0)], op: Op::NewBody });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let hits = ev.model().interferences();
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert!((hits[0].2 - 500.0).abs() < 1e-6);
        // bodies that only touch along a face do not interfere
        let mut d2 = Document::default();
        d2.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(10.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        d2.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(10.0), d: n(10.0), h: n(10.0) }, at: [n(10.0), n(0.0), n(0.0)], op: Op::NewBody });
        ev.evaluate(&d2);
        assert!(ev.model().interferences().is_empty());
    }
}

#[cfg(test)]
mod place_anchor_tests {
    use super::*;

    #[test]
    fn place_anchor_puts_the_face_flush_in_every_corner_even_when_turned() {
        let n = Expr::num;
        let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        for (rx, ry) in [(0, 0), (2, 0), (0, 2), (2, 2), (1, 1), (1, 2), (0, 1)] {
            // a 16 x 6 x 12 bar lying at x = 100 stands on one of its long sides: the footprint is turned relative to its own axes
            let mut d = Document::default();
            d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(40.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
            d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(16.0), d: n(6.0), h: n(12.0) }, at: [n(100.0), n(0.0), n(0.0)], op: Op::NewBody });
            let id = {
                let mut ev = Evaluator::default();
                ev.evaluate(&d);
                ev.model().bodies[1].id
            };
            // the side wall at y = 0 (normal -Y): 16 wide in x, 12 high in z
            let face = Plane { origin: [100.0, 0.0, 0.0], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0] };
            let outer = [[0.0, 0.0], [16.0, 0.0], [16.0, 12.0], [0.0, 12.0]];
            let seed = DVec3::new(108.0, 0.0, 6.0);
            let to = place_anchor(&face, &outer, seed, &top, [0.0, 0.0], [40.0, 40.0], (rx, ry));
            d.insert(FeatureKind::PlaceOnPlane { body: id, face, seed: seed.to_array(), target: top, to: Some(to.to_array()) });
            let mut ev = Evaluator::default();
            ev.evaluate(&d);
            assert!(ev.steps().iter().all(|s| s.error.is_none()), "{:?}", ev.steps().iter().filter_map(|s| s.error.clone()).collect::<Vec<_>>());
            let (lo, hi) = ev.model().body(id).unwrap().mesh().bounds().unwrap();
            let (x_ok, y_ok) = (
                match rx { 0 => lo.x.abs() < 1e-6, 2 => (hi.x - 40.0).abs() < 1e-6, _ => ((lo.x + hi.x) / 2.0 - 20.0).abs() < 1e-6 },
                match ry { 0 => lo.y.abs() < 1e-6, 2 => (hi.y - 40.0).abs() < 1e-6, _ => ((lo.y + hi.y) / 2.0 - 20.0).abs() < 1e-6 },
            );
            assert!(x_ok && y_ok, "anchor ({rx},{ry}): body {lo:?} .. {hi:?}");
            assert!(lo.z >= 10.0 - 1e-6, "it stands on the plate, not inside it: {lo:?}");
        }
    }

    #[test]
    fn placed_face_keeps_its_footprint_around_the_destination_point() {
        let n = Expr::num;
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(40.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(16.0), d: n(16.0), h: n(12.0) }, at: [n(100.0), n(0.0), n(0.0)], op: Op::NewBody });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let id = ev.model().bodies[1].id;
        let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let face = Plane { origin: [100.0, 0.0, 12.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
        let uid = d.insert(FeatureKind::PlaceOnPlane { body: id, face, seed: [108.0, 8.0, 12.0], target: top, to: Some([8.0, 32.0, 10.0]) });
        ev.evaluate(&d);
        assert!(ev.error(uid).is_none(), "{:?}", ev.error(uid));
        let (lo, hi) = ev.model().body(id).unwrap().mesh().bounds().unwrap();
        assert!((lo.x - 0.0).abs() < 1e-6 && (hi.x - 16.0).abs() < 1e-6, "x {lo:?} {hi:?}");
        assert!((lo.y - 24.0).abs() < 1e-6 && (hi.y - 40.0).abs() < 1e-6, "y {lo:?} {hi:?}");
    }
}
