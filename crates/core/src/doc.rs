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
    Rect { nx: Expr, dx: Expr, ny: Expr, dy: Expr },
    Circular { count: Expr, axis: Axis3, angle: Expr },
}

/// A flat face of a body, found again by its plane and a point on it.
fn zero_expr() -> Expr {
    Expr::num(0.0)
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
    },
    Revolve { sketch: u64, profiles: Vec<ProfileRef>, axis: RevolveAxis, angle: Expr, op: Op },
    Primitive { shape: Shape, at: [Expr; 3], op: Op },
    Move { bodies: Vec<u64>, translate: [Expr; 3], rotate: [Expr; 3], copy: bool },
    Mirror { bodies: Vec<u64>, normal: Axis3, join: bool },
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
    Shell { plane: Plane, thickness: Expr, open: bool },
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
            FeatureKind::PlaceOnPlane { .. } => "Выровнять",
            FeatureKind::Coil { .. } => "Спираль",
            FeatureKind::Pipe { .. } => "Труба",
            FeatureKind::Sweep { .. } => "По траектории",
            FeatureKind::Loft { .. } => "Переход",
            FeatureKind::Scale { .. } => "Масштаб",
            FeatureKind::Chamfer { .. } => "Фаска",
            FeatureKind::Fillet { .. } => "Скругление",
            FeatureKind::OffsetPlane { .. } => "Смещённая плоскость",
            FeatureKind::Shell { .. } => "Оболочка",
            FeatureKind::Move { .. } => "Перемещение",
            FeatureKind::Mirror { .. } => "Зеркало",
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

/// Вычислитель истории с кэшем по шагам.
#[derive(Default)]
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
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| eval_feature(f, &prev, &doc.params, &self.steps, &self.features)))
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

fn eval_feature(f: &Feature, prev: &Arc<Model>, p: &Params, steps: &[StepResult], feats: &[Feature]) -> Result<EvalOut, String> {
    let id_base = f.uid * 1000;
    match &f.kind {
        FeatureKind::Sketch { plane, sketch } => {
            let mut s = sketch.clone();
            let status = s.solve(p, &[])?;
            let regions = s.regions();
            Ok((prev.clone(), Some(SketchResult { plane: *plane, sketch: s, regions, status })))
        }
        FeatureKind::Extrude { face: Some(f), extent, .. } => {
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
            };
            let region = prev.bodies.iter().find_map(|b| geom::face_region(b.mesh(), &f.plane, DVec3::from(f.seed))).ok_or("грань не найдена: тело изменилось")?;
            let tool = Solid::from_mesh(&geom::extrude_region(&f.plane, &region, z0, z1).ok_or("не удалось построить грань")?)?;
            let mut m = (**prev).clone();
            apply_op(&mut m, tool, op, id_base)?;
            Ok((Arc::new(m), None))
        }
        FeatureKind::Extrude { sketch, profiles, extent, op, .. } => {
            let sk = find_sketch(*sketch, steps, feats)?;
            let regions = pick_regions(&sk, profiles)?;
            let (z0, z1) = match extent {
                Extent::OneSide(d) => (0.0, d.eval(p)?),
                Extent::Symmetric(d) => {
                    let d = d.eval(p)?.abs();
                    (-d / 2.0, d / 2.0)
                }
                Extent::TwoSides(a, b) => (-b.eval(p)?, a.eval(p)?),
            };
            if (z1 - z0).abs() < 1e-9 {
                return Err("расстояние не может быть нулевым".into());
            }
            let parts: Vec<Mesh> = regions.iter().filter_map(|r| geom::extrude_region(&sk.plane, r, z0, z1)).collect();
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
        FeatureKind::Mirror { bodies, normal, join } => {
            let sel = bodies_by_id(prev, bodies)?;
            let n = normal.dir();
            let s = DVec3::ONE - 2.0 * n;
            let mat = DMat4::from_scale(s);
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
        FeatureKind::Pattern { bodies, kind } => {
            let sel = bodies_by_id(prev, bodies)?;
            let mut mats = Vec::new();
            match kind {
                PatternKind::Rect { nx, dx, ny, dy } => {
                    let (nx, ny) = (count(nx, p)?, count(ny, p)?);
                    let (dx, dy) = (dx.eval(p)?, dy.eval(p)?);
                    for j in 0..ny {
                        for i in 0..nx {
                            if i + j > 0 {
                                mats.push(DMat4::from_translation(DVec3::new(i as f64 * dx, j as f64 * dy, 0.0)));
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
            let op = if is_fillet { EdgeOp::Fillet(v) } else { EdgeOp::Chamfer(v) };
            let (mut cuts, mut adds) = (Vec::new(), Vec::new());
            for e in edges {
                let (t, concave) = if e.circle {
                    geom::circle_tool_span(b.mesh(), DVec3::from(e.b), DVec3::from(e.axis), DVec3::from(e.a), op, e.sweep.unwrap_or(360.0))?
                } else {
                    geom::edge_tool(b.mesh(), DVec3::from(e.a), DVec3::from(e.b), op)?
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
            // выпуклые рёбра снимают материал, вогнутые — добавляют (скругление внутренних углов)
            let mut r = (*b.solid).clone();
            if !cuts.is_empty() {
                r = r.boolean(&Solid::union_all(&cuts), BoolOp::Subtract);
            }
            if !adds.is_empty() {
                r = r.boolean(&Solid::union_all(&adds), BoolOp::Union);
            }
            if r.is_empty() {
                return Err("результат пуст — размер слишком большой".into());
            }
            let mut m = (**prev).clone();
            if let Some(bb) = m.bodies.iter_mut().find(|x| x.id == *body) {
                bb.solid = Arc::new(r);
            }
            Ok((Arc::new(m), None))
        }
        FeatureKind::Shell { plane, thickness, open } => {
            let t = positive(thickness, p, "толщина")?;
            let (o, n) = (plane.o(), plane.normal());
            let ball = Solid::from_mesh(&geom::sphere_mesh(DVec3::ZERO, t))?;
            let mut m = (**prev).clone();
            let mut done = 0;
            for b in m.bodies.iter_mut() {
                let mesh = b.solid.mesh.clone();
                let Some((lo, hi)) = mesh.bounds() else { continue };
                let on_plane = mesh.vertices.iter().any(|q| (DVec3::from(*q) - o).dot(n).abs() < 1e-6);
                if *open && !on_plane {
                    continue;
                }
                let mut s = (*b.solid).clone();
                if *open {
                    let out = if ((lo + hi) * 0.5 - o).dot(n) > 0.0 { -n } else { n };
                    let big = (hi - lo).length() * 2.0 + 10.0;
                    let (xa, ya) = geom::perp_frame(n);
                    let mat = DMat4::from_cols(xa.extend(0.0), ya.extend(0.0), out.extend(0.0), o.extend(1.0));
                    let slab = Solid::from_mesh(&geom::box_mesh(DVec3::new(-big, -big, 0.0), DVec3::new(big, big, 2.0 * t + 1.0)))?.transformed(&mat);
                    s = s.boolean(&slab, BoolOp::Union);
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
            op: Op::NewBody, face: None });
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
            op: Op::Join, face: None });
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
        d.insert(FeatureKind::Mirror { bodies: vec![body], normal: Axis3::X, join: false });
        d.insert(FeatureKind::Pattern {
            bodies: vec![body],
            kind: PatternKind::Rect { nx: Expr::num(3.0), dx: Expr::num(20.0), ny: Expr::num(1.0), dy: Expr::num(0.0) },
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
        d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![pref], extent: Extent::OneSide(Expr::num(5.0)), op: Op::NewBody, face: None });
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
        d.insert(FeatureKind::Extrude { sketch: 0, profiles: vec![], extent: Extent::OneSide(Expr::num(5.0)), op: Op::Join, face: Some(FaceRef { plane: top, seed: [20.0, 15.0, 10.0] }) });
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
        d.insert(FeatureKind::Shell { plane: top, thickness: Expr::num(2.0), open: true });
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
        d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![pref], extent: Extent::OneSide(Expr::num(5.0)), op: Op::NewBody, face: None });
        let b = d.insert(FeatureKind::Primitive {
            shape: Shape::Cylinder { r: Expr::num(3.0), h: Expr::num(10.0) },
            at: [Expr::num(20.0), Expr::num(20.0), Expr::num(5.0)],
            op: Op::NewBody,
        });
        d.insert(FeatureKind::Pattern {
            bodies: vec![b * 1000],
            kind: PatternKind::Rect { nx: Expr::num(9.0), dx: Expr::num(20.0), ny: Expr::num(9.0), dy: Expr::num(20.0) },
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
}
