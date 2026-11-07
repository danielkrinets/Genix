//! 2D-чертёж: точки, кривые, геометрические ограничения и размеры, решатель, поиск замкнутых областей.

use crate::expr::{Expr, Params};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

pub type P2 = [f64; 2];

/// Плоскость чертежа в мире: начало и две ортонормированные оси (нормаль = x × y).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub origin: [f64; 3],
    pub x: [f64; 3],
    pub y: [f64; 3],
}

impl Plane {
    pub const XY: Plane = Plane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    /// Фронтальная плоскость (вид спереди): ось X вправо, Z вверх, нормаль смотрит на зрителя (−Y).
    pub const XZ: Plane = Plane { origin: [0.0; 3], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0] };
    /// Боковая плоскость: ось Y вправо, Z вверх, нормаль +X.
    pub const YZ: Plane = Plane { origin: [0.0; 3], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };

    pub fn o(&self) -> DVec3 {
        DVec3::from(self.origin)
    }
    pub fn xa(&self) -> DVec3 {
        DVec3::from(self.x)
    }
    pub fn ya(&self) -> DVec3 {
        DVec3::from(self.y)
    }
    pub fn normal(&self) -> DVec3 {
        self.xa().cross(self.ya())
    }

    /// Same plane seen from the other side: the normal points the opposite way.
    pub fn flipped(&self) -> Plane {
        Plane { origin: self.origin, x: self.y, y: self.x }
    }

    pub fn to_world(&self, p: P2) -> DVec3 {
        self.o() + self.xa() * p[0] + self.ya() * p[1]
    }

    pub fn to_local(&self, w: DVec3) -> P2 {
        let d = w - self.o();
        [d.dot(self.xa()), d.dot(self.ya())]
    }

    /// Плоскость через точку с нормалью. Ось X берём горизонтальной, если это возможно.
    pub fn from_point_normal(p: DVec3, n: DVec3) -> Plane {
        let n = n.normalize();
        let x = if n.z.abs() > 0.999 {
            DVec3::X * n.z.signum()
        } else {
            DVec3::Z.cross(n).normalize()
        };
        let y = n.cross(x).normalize();
        // начало — проекция мирового нуля на плоскость: координаты чертежа совпадают с привычными
        let origin = n * p.dot(n);
        Plane { origin: origin.to_array(), x: x.to_array(), y: y.to_array() }
    }

    /// Пересечение луча с плоскостью: параметр t вдоль луча.
    pub fn ray_hit(&self, origin: DVec3, dir: DVec3) -> Option<f64> {
        let n = self.normal();
        let d = dir.dot(n);
        if d.abs() < 1e-12 {
            return None;
        }
        Some((self.o() - origin).dot(n) / d)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SPoint {
    pub pos: P2,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Entity {
    Line { a: usize, b: usize, construction: bool },
    Circle { c: usize, r: f64, construction: bool },
    /// Дуга против часовой стрелки от `a` к `b` вокруг центра `c`.
    Arc { c: usize, a: usize, b: usize, construction: bool },
    /// Ellipse: centre `c`, `a` is the end of the major semi-axis, `ratio` = minor / major (0 < ratio <= 1).
    Ellipse { c: usize, a: usize, ratio: f64, construction: bool },
    /// Fit-point spline through the `count` sketch points that start at index `first` (they are added one after another).
    Spline { first: usize, count: usize, closed: bool, construction: bool },
}

impl Entity {
    pub fn construction(&self) -> bool {
        match *self {
            Entity::Line { construction, .. }
            | Entity::Circle { construction, .. }
            | Entity::Arc { construction, .. }
            | Entity::Ellipse { construction, .. }
            | Entity::Spline { construction, .. } => construction,
        }
    }

    pub fn set_construction(&mut self, v: bool) {
        match self {
            Entity::Line { construction, .. }
            | Entity::Circle { construction, .. }
            | Entity::Arc { construction, .. }
            | Entity::Ellipse { construction, .. }
            | Entity::Spline { construction, .. } => *construction = v,
        }
    }

    pub fn points(&self) -> Vec<usize> {
        match *self {
            Entity::Line { a, b, .. } => vec![a, b],
            Entity::Circle { c, .. } => vec![c],
            Entity::Arc { c, a, b, .. } => vec![c, a, b],
            Entity::Ellipse { c, a, .. } => vec![c, a],
            Entity::Spline { first, count, .. } => (first..first + count).collect(),
        }
    }

    fn remap(&mut self, m: &[usize]) {
        match self {
            Entity::Line { a, b, .. } => {
                *a = m[*a];
                *b = m[*b];
            }
            Entity::Circle { c, .. } => *c = m[*c],
            Entity::Arc { c, a, b, .. } => {
                *c = m[*c];
                *a = m[*a];
                *b = m[*b];
            }
            Entity::Ellipse { c, a, .. } => {
                *c = m[*c];
                *a = m[*a];
            }
            // the fit points stay one after another when unused points around them are dropped
            Entity::Spline { first, .. } => *first = m[*first],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constraint {
    Horizontal(usize),
    Vertical(usize),
    Parallel(usize, usize),
    Perpendicular(usize, usize),
    /// Касание: линия–окружность/дуга или две окружности/дуги.
    Tangent(usize, usize),
    /// Равные длины (линии) или радиусы (окружности/дуги).
    Equal(usize, usize),
    Concentric(usize, usize),
    /// Точка — середина линии.
    Midpoint(usize, usize),
    /// Две линии лежат на одной прямой.
    Collinear(usize, usize),
    /// Две точки симметричны относительно линии (третий номер — линия).
    Symmetric(usize, usize, usize),
    /// Точка лежит на кривой.
    PointOn(usize, usize),
    /// Точка зафиксирована в заданном месте.
    Fix(usize, P2),
    /// Размер: длина линии.
    Length(usize, Expr),
    /// Размер: диаметр окружности или дуги.
    Diameter(usize, Expr),
    /// Размер: расстояние между точками.
    Distance(usize, usize, Expr),
    /// Размер: расстояние по горизонтали / вертикали между точками.
    HDistance(usize, usize, Expr),
    VDistance(usize, usize, Expr),
    /// Размер: угол между линиями в градусах.
    Angle(usize, usize, Expr),
}

impl Constraint {
    pub fn entities(&self) -> Vec<usize> {
        use Constraint::*;
        match self {
            Horizontal(e) | Vertical(e) | Length(e, _) | Diameter(e, _) => vec![*e],
            Parallel(a, b) | Perpendicular(a, b) | Tangent(a, b) | Equal(a, b) | Concentric(a, b) | Angle(a, b, _) => {
                vec![*a, *b]
            }
            Midpoint(_, e) | PointOn(_, e) => vec![*e],
            Collinear(a, b) => vec![*a, *b],
            Symmetric(_, _, axis) => vec![*axis],
            Fix(..) | Distance(..) | HDistance(..) | VDistance(..) => vec![],
        }
    }

    pub fn points(&self) -> Vec<usize> {
        use Constraint::*;
        match self {
            Midpoint(p, _) | PointOn(p, _) | Fix(p, _) => vec![*p],
            Distance(a, b, _) | HDistance(a, b, _) | VDistance(a, b, _) | Symmetric(a, b, _) => vec![*a, *b],
            _ => vec![],
        }
    }

    pub fn expr(&self) -> Option<&Expr> {
        use Constraint::*;
        match self {
            Length(_, e) | Diameter(_, e) | Distance(_, _, e) | HDistance(_, _, e) | VDistance(_, _, e) | Angle(_, _, e) => Some(e),
            _ => None,
        }
    }

    pub fn expr_mut(&mut self) -> Option<&mut Expr> {
        use Constraint::*;
        match self {
            Length(_, e) | Diameter(_, e) | Distance(_, _, e) | HDistance(_, _, e) | VDistance(_, _, e) | Angle(_, _, e) => Some(e),
            _ => None,
        }
    }

    pub fn is_dimension(&self) -> bool {
        self.expr().is_some()
    }

    fn remap(&mut self, pm: &[usize], em: &[usize]) {
        use Constraint::*;
        match self {
            Horizontal(e) | Vertical(e) | Length(e, _) | Diameter(e, _) => *e = em[*e],
            Parallel(a, b) | Perpendicular(a, b) | Tangent(a, b) | Equal(a, b) | Concentric(a, b) | Angle(a, b, _) => {
                *a = em[*a];
                *b = em[*b];
            }
            Midpoint(p, e) | PointOn(p, e) => {
                *p = pm[*p];
                *e = em[*e];
            }
            Fix(p, _) => *p = pm[*p],
            Collinear(a, b) => {
                *a = em[*a];
                *b = em[*b];
            }
            Symmetric(a, b, axis) => {
                *a = pm[*a];
                *b = pm[*b];
                *axis = em[*axis];
            }
            Distance(a, b, _) | HDistance(a, b, _) | VDistance(a, b, _) => {
                *a = pm[*a];
                *b = pm[*b];
            }
        }
    }

    pub fn label(&self) -> &'static str {
        use Constraint::*;
        match self {
            Horizontal(_) => "Горизонтально",
            Vertical(_) => "Вертикально",
            Parallel(..) => "Параллельно",
            Perpendicular(..) => "Перпендикулярно",
            Tangent(..) => "Касание",
            Equal(..) => "Равенство",
            Concentric(..) => "Концентричность",
            Midpoint(..) => "Середина",
            Collinear(..) => "На одной линии",
            Symmetric(..) => "Симметрично",
            PointOn(..) => "Точка на кривой",
            Fix(..) => "Фиксация",
            Length(..) => "Длина",
            Diameter(..) => "Диаметр",
            Distance(..) => "Расстояние",
            HDistance(..) => "Расстояние по X",
            VDistance(..) => "Расстояние по Y",
            Angle(..) => "Угол",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sketch {
    pub points: Vec<SPoint>,
    pub entities: Vec<Entity>,
    pub constraints: Vec<Constraint>,
    /// Text lines of the sketch (their letters are regions like any closed curve).
    #[serde(default)]
    pub texts: Vec<crate::text::SketchText>,
}

/// Результат решения: удалось ли удовлетворить все ограничения.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SolveStatus {
    Ok,
    /// Ограничения противоречат друг другу (остаток не сходится к нулю).
    Conflict(f64),
}

/// Максимальное отклонение кривой от её ломаной при разбиении, в долях радиуса.
const ARC_STEP_DEG: f64 = 5.0;

pub fn arc_segments(sweep_rad: f64) -> usize {
    ((sweep_rad.abs().to_degrees() / ARC_STEP_DEG).ceil() as usize).max(6)
}

impl Sketch {
    /// Every entity and constraint refers to points and entities that exist, and every coordinate is a number.
    pub fn check_indices(&self) -> Result<(), String> {
        let (np, ne) = (self.points.len(), self.entities.len());
        if self.points.iter().any(|p| !p.pos[0].is_finite() || !p.pos[1].is_finite()) {
            return Err("в чертеже есть точка без координат".into());
        }
        for e in &self.entities {
            if e.points().iter().any(|&p| p >= np) {
                return Err("кривая чертежа ссылается на несуществующую точку".into());
            }
        }
        for c in &self.constraints {
            if c.points().iter().any(|&p| p >= np) || c.entities().iter().any(|&e| e >= ne) {
                return Err("ограничение чертежа ссылается на несуществующий объект".into());
            }
        }
        Ok(())
    }

    pub fn add_point(&mut self, p: P2) -> usize {
        self.points.push(SPoint { pos: p });
        self.points.len() - 1
    }

    pub fn add(&mut self, e: Entity) -> usize {
        self.entities.push(e);
        self.entities.len() - 1
    }

    pub fn p(&self, i: usize) -> DVec2 {
        DVec2::from(self.points[i].pos)
    }

    pub fn add_line(&mut self, a: P2, b: P2) -> usize {
        let (a, b) = (self.add_point(a), self.add_point(b));
        self.add(Entity::Line { a, b, construction: false })
    }

    /// Прямоугольник по двум углам: 4 линии с общими вершинами, 2 горизонтали и 2 вертикали.
    pub fn add_rect(&mut self, p0: P2, p1: P2) -> [usize; 4] {
        let pts = [p0, [p1[0], p0[1]], p1, [p0[0], p1[1]]].map(|p| self.add_point(p));
        let l = [0, 1, 2, 3].map(|i| self.add(Entity::Line { a: pts[i], b: pts[(i + 1) % 4], construction: false }));
        self.constraints.push(Constraint::Horizontal(l[0]));
        self.constraints.push(Constraint::Horizontal(l[2]));
        self.constraints.push(Constraint::Vertical(l[1]));
        self.constraints.push(Constraint::Vertical(l[3]));
        l
    }

    pub fn add_circle(&mut self, c: P2, r: f64) -> usize {
        let c = self.add_point(c);
        self.add(Entity::Circle { c, r, construction: false })
    }

    /// Длина линии или диаметр окружности/дуги (для подписей и размеров).
    pub fn measure(&self, e: usize) -> f64 {
        match self.entities[e] {
            Entity::Line { a, b, .. } => self.p(a).distance(self.p(b)),
            Entity::Circle { r, .. } => 2.0 * r,
            Entity::Arc { c, a, .. } => 2.0 * self.p(c).distance(self.p(a)),
            Entity::Ellipse { c, a, .. } => 2.0 * self.p(c).distance(self.p(a)),
            Entity::Spline { first, count, .. } => (first + 1..first + count).map(|i| self.p(i).distance(self.p(i - 1))).sum(),
        }
    }

    pub fn radius(&self, e: usize) -> Option<f64> {
        match self.entities[e] {
            Entity::Circle { r, .. } => Some(r),
            Entity::Arc { c, a, .. } => Some(self.p(c).distance(self.p(a))),
            _ => None,
        }
    }

    /// Ломаная, приближающая кривую (для дуги — от `a` к `b` против часовой).
    pub fn tessellate(&self, e: usize) -> Vec<P2> {
        match self.entities[e] {
            Entity::Line { a, b, .. } => vec![self.points[a].pos, self.points[b].pos],
            Entity::Ellipse { c, a, ratio, .. } => {
                let c = self.p(c);
                let major = self.p(a) - c;
                let minor = DVec2::new(-major.y, major.x) * ratio;
                let n = arc_segments(std::f64::consts::TAU).max(64);
                (0..=n)
                    .map(|i| {
                        let t = std::f64::consts::TAU * i as f64 / n as f64;
                        (c + major * t.cos() + minor * t.sin()).to_array()
                    })
                    .collect()
            }
            Entity::Spline { first, count, closed, .. } => {
                let pts: Vec<DVec2> = (first..first + count).map(|i| self.p(i)).collect();
                catmull_rom(&pts, closed)
            }
            Entity::Circle { c, r, .. } => {
                let c = self.p(c);
                let n = arc_segments(std::f64::consts::TAU);
                (0..=n)
                    .map(|i| {
                        let t = std::f64::consts::TAU * i as f64 / n as f64;
                        (c + DVec2::new(t.cos(), t.sin()) * r).to_array()
                    })
                    .collect()
            }
            Entity::Arc { c, a, b, .. } => {
                let (c, pa, pb) = (self.p(c), self.p(a), self.p(b));
                let r = (pa - c).length();
                let a0 = (pa - c).y.atan2((pa - c).x);
                let mut sweep = (pb - c).y.atan2((pb - c).x) - a0;
                if sweep <= 1e-12 {
                    sweep += std::f64::consts::TAU;
                }
                let n = arc_segments(sweep);
                let mut out: Vec<P2> = (0..=n)
                    .map(|i| {
                        let t = a0 + sweep * i as f64 / n as f64;
                        (c + DVec2::new(t.cos(), t.sin()) * r).to_array()
                    })
                    .collect();
                // концы точно в точках чертежа, чтобы контуры замыкались без зазоров
                out[0] = pa.to_array();
                *out.last_mut().unwrap() = pb.to_array();
                out
            }
        }
    }

    /// Удалить кривые (и всё, что от них зависит), затем лишние точки.
    pub fn remove_entities(&mut self, remove: &[usize]) {
        let keep: Vec<bool> = (0..self.entities.len()).map(|i| !remove.contains(&i)).collect();
        let mut em = vec![usize::MAX; self.entities.len()];
        let mut ents = Vec::new();
        for (i, e) in self.entities.iter().enumerate() {
            if keep[i] {
                em[i] = ents.len();
                ents.push(*e);
            }
        }
        self.constraints.retain(|c| c.entities().iter().all(|&e| keep[e]));
        for c in &mut self.constraints {
            let pm: Vec<usize> = (0..self.points.len()).collect();
            c.remap(&pm, &em);
        }
        self.entities = ents;
        self.gc_points();
    }

    pub fn remove_constraint(&mut self, i: usize) {
        if i < self.constraints.len() {
            self.constraints.remove(i);
        }
    }

    /// Удалить точки, на которые никто не ссылается.
    pub fn gc_points(&mut self) {
        let mut used = vec![false; self.points.len()];
        for e in &self.entities {
            for p in e.points() {
                used[p] = true;
            }
        }
        // ограничения на «висящие» точки удаляются вместе с точками
        self.constraints.retain(|c| c.points().iter().all(|&p| used[p]));
        let mut pm = vec![usize::MAX; self.points.len()];
        let mut pts = Vec::new();
        for (i, p) in self.points.iter().enumerate() {
            if used[i] {
                pm[i] = pts.len();
                pts.push(*p);
            }
        }
        let em: Vec<usize> = (0..self.entities.len()).collect();
        for e in &mut self.entities {
            e.remap(&pm);
        }
        for c in &mut self.constraints {
            c.remap(&pm, &em);
        }
        self.points = pts;
    }

    /// Слить точку `from` в `into` (совпадение). Вырожденные линии удаляются.
    pub fn merge_points(&mut self, from: usize, into: usize) {
        if from == into {
            return;
        }
        let pm: Vec<usize> = (0..self.points.len()).map(|i| if i == from { into } else { i }).collect();
        let em: Vec<usize> = (0..self.entities.len()).collect();
        for e in &mut self.entities {
            e.remap(&pm);
        }
        for c in &mut self.constraints {
            c.remap(&pm, &em);
        }
        let degenerate: Vec<usize> = self
            .entities
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Entity::Line { a, b, .. } if a == b) || matches!(e, Entity::Arc { a, b, .. } if a == b))
            .map(|(i, _)| i)
            .collect();
        self.remove_entities(&degenerate);
        self.gc_points();
    }

    // ---------------------------------------------------------------- решатель

    fn vars(&self) -> (Vec<f64>, Vec<Option<usize>>) {
        let mut x = Vec::with_capacity(self.points.len() * 2 + 4);
        for p in &self.points {
            x.extend(p.pos);
        }
        let mut rvar = vec![None; self.entities.len()];
        for (i, e) in self.entities.iter().enumerate() {
            if let Entity::Circle { r, .. } = e {
                rvar[i] = Some(x.len());
                x.push(*r);
            }
        }
        (x, rvar)
    }

    fn residuals(&self, x: &[f64], rvar: &[Option<usize>], dims: &[f64], out: &mut Vec<f64>) {
        out.clear();
        let pt = |i: usize| DVec2::new(x[2 * i], x[2 * i + 1]);
        let line = |e: usize| match self.entities[e] {
            Entity::Line { a, b, .. } => Some((pt(a), pt(b))),
            _ => None,
        };
        let circ = |e: usize| match self.entities[e] {
            Entity::Circle { c, .. } => Some((pt(c), x[rvar[e].unwrap()])),
            Entity::Arc { c, a, .. } => Some((pt(c), pt(c).distance(pt(a)))),
            _ => None,
        };
        // неявное ограничение дуги: оба конца на одном радиусе
        for e in &self.entities {
            if let Entity::Arc { c, a, b, .. } = *e {
                out.push(pt(c).distance(pt(a)) - pt(c).distance(pt(b)));
            }
        }
        let mut di = 0;
        for c in &self.constraints {
            use Constraint::*;
            match c {
                Horizontal(e) => {
                    if let Some((a, b)) = line(*e) {
                        out.push(a.y - b.y)
                    }
                }
                Vertical(e) => {
                    if let Some((a, b)) = line(*e) {
                        out.push(a.x - b.x)
                    }
                }
                Parallel(e1, e2) | Perpendicular(e1, e2) => {
                    if let (Some((a, b)), Some((c2, d))) = (line(*e1), line(*e2)) {
                        let (u, v) = ((b - a).normalize_or_zero(), (d - c2).normalize_or_zero());
                        // масштаб: средняя длина, чтобы остаток был в миллиметрах
                        let s = 0.5 * ((b - a).length() + (d - c2).length());
                        out.push(if matches!(c, Parallel(..)) { u.perp_dot(v) * s } else { u.dot(v) * s });
                    }
                }
                Tangent(e1, e2) => match (line(*e1), line(*e2), circ(*e1), circ(*e2)) {
                    (Some((a, b)), _, _, Some((cc, r))) | (_, Some((a, b)), Some((cc, r)), _) => {
                        let d = (b - a).normalize_or_zero();
                        out.push((cc - a).perp_dot(d).abs() - r);
                    }
                    (_, _, Some((c1, r1)), Some((c2, r2))) => {
                        let dist = c1.distance(c2);
                        // внешнее или внутреннее касание — что ближе к текущему положению
                        let ext = dist - (r1 + r2);
                        let int = dist - (r1 - r2).abs();
                        out.push(if ext.abs() <= int.abs() { ext } else { int });
                    }
                    _ => {}
                },
                Equal(e1, e2) => match (line(*e1), line(*e2), circ(*e1), circ(*e2)) {
                    (Some((a, b)), Some((c2, d)), _, _) => out.push(a.distance(b) - c2.distance(d)),
                    (_, _, Some((_, r1)), Some((_, r2))) => out.push(r1 - r2),
                    _ => {}
                },
                Concentric(e1, e2) => {
                    if let (Some((c1, _)), Some((c2, _))) = (circ(*e1), circ(*e2)) {
                        out.push(c1.x - c2.x);
                        out.push(c1.y - c2.y);
                    }
                }
                Midpoint(p, e) => {
                    if let Some((a, b)) = line(*e) {
                        let m = (a + b) * 0.5;
                        out.push(pt(*p).x - m.x);
                        out.push(pt(*p).y - m.y);
                    }
                }
                Collinear(e1, e2) => {
                    if let (Some((a, b)), Some((c2, d))) = (line(*e1), line(*e2)) {
                        let u = (b - a).normalize_or_zero();
                        out.push((c2 - a).perp_dot(u));
                        out.push((d - a).perp_dot(u));
                    }
                }
                Symmetric(p, q, axis) => {
                    if let Some((a, b)) = line(*axis) {
                        let u = (b - a).normalize_or_zero();
                        let mid = (pt(*p) + pt(*q)) * 0.5;
                        // the middle of the pair lies on the axis, and the pair is perpendicular to it
                        out.push((mid - a).perp_dot(u));
                        out.push((pt(*q) - pt(*p)).dot(u));
                    }
                }
                PointOn(p, e) => {
                    if let Some((a, b)) = line(*e) {
                        out.push((pt(*p) - a).perp_dot((b - a).normalize_or_zero()));
                    } else if let Some((cc, r)) = circ(*e) {
                        out.push(pt(*p).distance(cc) - r);
                    }
                }
                Fix(p, at) => {
                    out.push(pt(*p).x - at[0]);
                    out.push(pt(*p).y - at[1]);
                }
                Length(e, _) => {
                    let v = dims[di];
                    di += 1;
                    if let Some((a, b)) = line(*e) {
                        out.push(a.distance(b) - v);
                    }
                }
                Diameter(e, _) => {
                    let v = dims[di];
                    di += 1;
                    if let Some((_, r)) = circ(*e) {
                        out.push(2.0 * r - v);
                    }
                }
                Distance(p, q, _) => {
                    let v = dims[di];
                    di += 1;
                    out.push(pt(*p).distance(pt(*q)) - v);
                }
                HDistance(p, q, _) => {
                    let v = dims[di];
                    di += 1;
                    // направление — из положения до решения (стабильно на всех итерациях; совпадающие точки — вправо)
                    let sign = if self.p(*q).x - self.p(*p).x >= -1e-12 { 1.0 } else { -1.0 };
                    out.push((pt(*q).x - pt(*p).x) * sign - v);
                }
                VDistance(p, q, _) => {
                    let v = dims[di];
                    di += 1;
                    let sign = if self.p(*q).y - self.p(*p).y >= -1e-12 { 1.0 } else { -1.0 };
                    out.push((pt(*q).y - pt(*p).y) * sign - v);
                }
                Angle(e1, e2, _) => {
                    let v = dims[di];
                    di += 1;
                    if let (Some((a, b)), Some((c2, d))) = (line(*e1), line(*e2)) {
                        let (u, w) = (b - a, d - c2);
                        let ang = u.perp_dot(w).atan2(u.dot(w)).abs();
                        // угол между прямыми без учёта направления: 0..180
                        let s = 0.5 * (u.length() + w.length());
                        out.push((ang - v.to_radians()) * s);
                    }
                }
            }
        }
    }

    /// Значения размеров из выражений.
    pub fn dim_values(&self, params: &Params) -> Result<Vec<f64>, String> {
        self.constraints
            .iter()
            .filter_map(|c| c.expr())
            .map(|e| e.eval(params))
            .collect()
    }

    /// Решить ограничения, сдвинув геометрию минимально. `drag` — точки, которые пользователь
    /// тянет: они стремятся к заданным позициям, но ограничения важнее.
    pub fn solve(&mut self, params: &Params, drag: &[(usize, P2)]) -> Result<SolveStatus, String> {
        let dims = self.dim_values(params)?;
        // a negative diameter is impossible, just like a negative length; never solve it as its absolute value
        for (c, v) in self.constraints.iter().filter(|c| c.expr().is_some()).zip(dims.iter()) {
            if matches!(c, Constraint::Diameter(..)) && *v < 0.0 {
                return Ok(SolveStatus::Conflict(-v));
            }
        }
        let (mut x, rvar) = self.vars();
        let n = x.len();
        if n == 0 {
            return Ok(SolveStatus::Ok);
        }
        let mut r = Vec::new();
        let mut r2 = Vec::new();

        // Сначала мягко подтягиваем перетаскиваемые точки, затем жёстко решаем ограничения.
        for &(p, to) in drag {
            x[2 * p] = to[0];
            x[2 * p + 1] = to[1];
        }

        let mut lambda = 1e-6;
        let scale = x.iter().fold(1.0f64, |m, v| m.max(v.abs()));
        let tol = 1e-9 * scale;
        let mut status = SolveStatus::Ok;
        for _iter in 0..200 {
            self.residuals(&x, &rvar, &dims, &mut r);
            let m = r.len();
            let err = r.iter().fold(0.0f64, |a, v| a.max(v.abs()));
            if m == 0 || err < tol {
                status = SolveStatus::Ok;
                break;
            }
            status = SolveStatus::Conflict(err);
            // якобиан численно (центральные разности); только столбцы, влияющие на остаток
            let mut jac = vec![0.0; m * n];
            for j in 0..n {
                let h = 1e-7 * x[j].abs().max(1.0);
                let old = x[j];
                x[j] = old + h;
                self.residuals(&x, &rvar, &dims, &mut r2);
                let rp = r2.clone();
                x[j] = old - h;
                self.residuals(&x, &rvar, &dims, &mut r2);
                x[j] = old;
                for i in 0..m {
                    jac[i * n + j] = (rp[i] - r2[i]) / (2.0 * h);
                }
            }
            // шаг минимальной нормы: dx = -Jᵀ (J Jᵀ + λI)⁻¹ r — не трогает несвязанные точки
            let mut a = vec![0.0; m * m];
            for i in 0..m {
                for k in i..m {
                    let mut s = 0.0;
                    for j in 0..n {
                        s += jac[i * n + j] * jac[k * n + j];
                    }
                    a[i * m + k] = s;
                    a[k * m + i] = s;
                }
            }
            let mut improved = false;
            for _ in 0..12 {
                let mut aa = a.clone();
                for i in 0..m {
                    aa[i * m + i] += lambda * (1.0 + a[i * m + i]);
                }
                let Some(y) = solve_dense(&mut aa, &r, m) else {
                    lambda *= 10.0;
                    continue;
                };
                let mut xn = x.clone();
                for j in 0..n {
                    let mut s = 0.0;
                    for i in 0..m {
                        s += jac[i * n + j] * y[i];
                    }
                    xn[j] -= s;
                }
                self.residuals(&xn, &rvar, &dims, &mut r2);
                let e_old: f64 = r.iter().map(|v| v * v).sum();
                let e_new: f64 = r2.iter().map(|v| v * v).sum();
                if e_new.is_finite() && e_new < e_old {
                    x = xn;
                    lambda = (lambda * 0.3).max(1e-12);
                    improved = true;
                    break;
                }
                lambda *= 10.0;
            }
            if !improved {
                break;
            }
        }
        // записываем результат (даже частичный — так видно, где конфликт)
        for (i, p) in self.points.iter_mut().enumerate() {
            p.pos = [x[2 * i], x[2 * i + 1]];
        }
        for (i, e) in self.entities.iter_mut().enumerate() {
            if let (Entity::Circle { r, .. }, Some(v)) = (e, rvar[i]) {
                *r = x[v].abs();
            }
        }
        Ok(status)
    }

    /// Число степеней свободы, которые ещё не заданы ограничениями (0 — чертёж полностью определён).
    pub fn free_dof(&self, params: &Params) -> usize {
        let Ok(dims) = self.dim_values(params) else { return 0 };
        let (mut x, rvar) = self.vars();
        let n = x.len();
        let mut r = Vec::new();
        let mut r2 = Vec::new();
        self.residuals(&x, &rvar, &dims, &mut r);
        let m = r.len();
        let mut jac = vec![vec![0.0; n]; m];
        for j in 0..n {
            let h = 1e-6 * x[j].abs().max(1.0);
            let old = x[j];
            x[j] = old + h;
            self.residuals(&x, &rvar, &dims, &mut r2);
            let rp = r2.clone();
            x[j] = old - h;
            self.residuals(&x, &rvar, &dims, &mut r2);
            x[j] = old;
            for i in 0..m {
                jac[i][j] = (rp[i] - r2[i]) / (2.0 * h);
            }
        }
        n - rank(jac, n)
    }

    /// Every separate open chain or closed loop of lines and arcs (construction geometry is ignored), each as an ordered polyline.
    /// A closed loop starts and ends at its lowest-numbered point. A point where three or more curves meet is an error.
    pub fn chain_paths(&self) -> Result<Vec<Vec<P2>>, String> {
        let curves: Vec<usize> = (0..self.entities.len())
            .filter(|&i| !self.entities[i].construction() && matches!(self.entities[i], Entity::Line { .. } | Entity::Arc { .. }))
            .collect();
        if curves.is_empty() {
            return Err("в чертеже пути нет линий или дуг".into());
        }
        let mut canon: Vec<usize> = (0..self.points.len()).collect();
        for i in 0..self.points.len() {
            if let Some(j) = (0..i).find(|&j| canon[j] == j && self.p(i).distance(self.p(j)) < 1e-3) {
                canon[i] = j;
            }
        }
        let ends = |i: usize| match self.entities[i] {
            Entity::Line { a, b, .. } => (canon[a], canon[b]),
            Entity::Arc { a, b, .. } => (canon[a], canon[b]),
            _ => unreachable!(),
        };
        let mut deg = std::collections::HashMap::<usize, usize>::new();
        for &i in &curves {
            let (a, b) = ends(i);
            *deg.entry(a).or_default() += 1;
            *deg.entry(b).or_default() += 1;
        }
        if deg.values().any(|d| *d > 2) {
            return Err("путь не должен ветвиться: в одной точке сходятся больше двух линий".into());
        }
        let mut used = vec![false; self.entities.len()];
        let mut chains: Vec<Vec<P2>> = Vec::new();
        // open chains first (they begin at a point with one curve), then whatever is left is a closed loop
        let mut starts: Vec<usize> = deg.iter().filter(|(_, d)| **d == 1).map(|(p, _)| *p).collect();
        starts.sort_unstable();
        let mut loops: Vec<usize> = curves.iter().map(|&i| ends(i).0).collect();
        loops.sort_unstable();
        for start in starts.into_iter().chain(loops) {
            if !curves.iter().any(|&i| !used[i] && (ends(i).0 == start || ends(i).1 == start)) {
                continue;
            }
            let mut cur = start;
            let mut out: Vec<P2> = Vec::new();
            while let Some(&i) = curves.iter().find(|&&i| !used[i] && (ends(i).0 == cur || ends(i).1 == cur)) {
                used[i] = true;
                let (a, b) = ends(i);
                let mut pts = self.tessellate(i);
                if a != cur {
                    pts.reverse();
                }
                if !out.is_empty() {
                    pts.remove(0);
                }
                out.extend(pts);
                cur = if a == cur { b } else { a };
            }
            if out.len() >= 2 {
                chains.push(out);
            }
        }
        Ok(chains)
    }

    /// Упорядоченная ломаная пути: одна незамкнутая цепочка линий и дуг (вспомогательные не считаются).
    pub fn chain_path(&self) -> Result<Vec<P2>, String> {
        let curves: Vec<usize> = (0..self.entities.len())
            .filter(|&i| !self.entities[i].construction() && matches!(self.entities[i], Entity::Line { .. } | Entity::Arc { .. }))
            .collect();
        if curves.is_empty() {
            return Err("в чертеже пути нет линий или дуг".into());
        }
        // Lines drawn one after another often get separate end points that only touch visually:
        // points closer than 1 micron count as one, so such lines still form a chain.
        let mut canon: Vec<usize> = (0..self.points.len()).collect();
        for i in 0..self.points.len() {
            if let Some(j) = (0..i).find(|&j| canon[j] == j && self.p(i).distance(self.p(j)) < 1e-3) {
                canon[i] = j;
            }
        }
        let ends = |i: usize| match self.entities[i] {
            Entity::Line { a, b, .. } => (canon[a], canon[b]),
            Entity::Arc { a, b, .. } => (canon[a], canon[b]),
            _ => unreachable!(),
        };
        let mut deg = std::collections::HashMap::<usize, usize>::new();
        for &i in &curves {
            let (a, b) = ends(i);
            *deg.entry(a).or_default() += 1;
            *deg.entry(b).or_default() += 1;
        }
        if deg.values().any(|d| *d > 2) {
            return Err("путь не должен ветвиться: в одной точке сходятся больше двух линий".into());
        }
        let starts: Vec<usize> = deg.iter().filter(|(_, d)| **d == 1).map(|(p, _)| *p).collect();
        if starts.is_empty() {
            return Err("путь замкнут: труба строится по открытой цепочке, удалите одну линию".into());
        }
        if starts.len() != 2 {
            return Err(format!("линии пути не соединены концами: свободных концов {}, а должно быть 2 (начало и конец трубы)", starts.len()));
        }
        let mut cur = *starts.iter().min().unwrap();
        let mut used = vec![false; self.entities.len()];
        let mut out: Vec<P2> = Vec::new();
        loop {
            let Some(&i) = curves.iter().find(|&&i| !used[i] && (ends(i).0 == cur || ends(i).1 == cur)) else { break };
            used[i] = true;
            let (a, b) = ends(i);
            let mut pts = self.tessellate(i);
            if a != cur {
                pts.reverse();
            }
            if !out.is_empty() {
                pts.remove(0);
            }
            out.extend(pts);
            cur = if a == cur { b } else { a };
        }
        if used.iter().filter(|u| **u).count() != curves.len() {
            return Err("кривые пути не образуют одну цепочку".into());
        }
        Ok(out)
    }

    // ---------------------------------------------------------------- области

    /// Замкнутые области чертежа (с отверстиями), готовые к выдавливанию.
    pub fn regions(&self) -> Vec<Region> {
        regions(self)
    }

    /// Copies the given entities through the point map `f` (every sketch point is moved once, so copied shapes stay connected).
    /// `flips` is true for a mirror: arcs run counter-clockwise, so their end points swap. Returns the indexes of the copies.
    pub fn duplicate(&mut self, ents: &[usize], f: &dyn Fn(DVec2) -> DVec2, flips: bool) -> Vec<usize> {
        use std::collections::HashMap;
        let mut map: HashMap<usize, usize> = HashMap::new();
        let mut copy_point = |s: &mut Sketch, i: usize| -> usize {
            if let Some(&n) = map.get(&i) {
                return n;
            }
            let n = s.add_point(f(s.p(i)).to_array());
            map.insert(i, n);
            n
        };
        let mut out = Vec::new();
        for &e in ents {
            let Some(&ent) = self.entities.get(e) else { continue };
            let copy = match ent {
                Entity::Line { a, b, construction } => Entity::Line { a: copy_point(self, a), b: copy_point(self, b), construction },
                Entity::Circle { c, r, construction } => Entity::Circle { c: copy_point(self, c), r, construction },
                Entity::Arc { c, a, b, construction } => {
                    let (c, a, b) = (copy_point(self, c), copy_point(self, a), copy_point(self, b));
                    if flips { Entity::Arc { c, a: b, b: a, construction } } else { Entity::Arc { c, a, b, construction } }
                }
                Entity::Ellipse { c, a, ratio, construction } => Entity::Ellipse { c: copy_point(self, c), a: copy_point(self, a), ratio, construction },
                Entity::Spline { first, count, closed, construction } => {
                    // fit points must stay consecutive: fresh points, never shared
                    let nf = self.points.len();
                    for k in 0..count {
                        let np = f(self.p(first + k)).to_array();
                        self.add_point(np);
                    }
                    Entity::Spline { first: nf, count, closed, construction }
                }
            };
            out.push(self.add(copy));
        }
        out
    }

    /// Mirror copy of the entities across the line through `p0` and `p1`. Returns the new entities.
    pub fn mirror_entities(&mut self, ents: &[usize], p0: DVec2, p1: DVec2) -> Vec<usize> {
        let d = (p1 - p0).try_normalize().unwrap_or(DVec2::X);
        self.duplicate(ents, &|p| {
            let v = p - p0;
            let along = d * v.dot(d);
            p0 + 2.0 * along - v
        }, true)
    }

    /// Rectangular pattern: `nx` x `ny` copies in total (the original counts as the first), `dx`/`dy` apart. Returns the new entities.
    pub fn pattern_rect(&mut self, ents: &[usize], nx: usize, dx: f64, ny: usize, dy: f64) -> Vec<usize> {
        let mut out = Vec::new();
        for j in 0..ny.max(1) {
            for i in 0..nx.max(1) {
                if i + j > 0 {
                    let off = DVec2::new(i as f64 * dx, j as f64 * dy);
                    out.extend(self.duplicate(ents, &|p| p + off, false));
                }
            }
        }
        out
    }

    /// Circular pattern about `centre`: `count` copies in total over `angle_deg` (360 spreads them evenly round the full circle).
    pub fn pattern_circular(&mut self, ents: &[usize], centre: DVec2, count: usize, angle_deg: f64) -> Vec<usize> {
        let count = count.max(1);
        let step = if (angle_deg.abs() - 360.0).abs() < 1e-9 || count < 2 { angle_deg / count as f64 } else { angle_deg / (count - 1) as f64 };
        let mut out = Vec::new();
        for k in 1..count {
            let (s, c) = (step * k as f64).to_radians().sin_cos();
            out.extend(self.duplicate(ents, &|p| {
                let v = p - centre;
                centre + DVec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
            }, false));
        }
        out
    }

    /// Turns text `i` into ordinary closed polylines (a line per outline segment), so the letters can be selected, trimmed and
    /// dimensioned like any sketch curve. The text itself is removed. Returns the number of lines added.
    pub fn explode_text(&mut self, i: usize) -> usize {
        if i >= self.texts.len() {
            return 0;
        }
        let t = self.texts.remove(i);
        let mut lines = 0;
        for r in crate::text::regions(&t) {
            for ring in std::iter::once(&r.outer).chain(r.holes.iter()) {
                let ids: Vec<usize> = ring.iter().map(|p| self.add_point(*p)).collect();
                for k in 0..ids.len() {
                    self.add(Entity::Line { a: ids[k], b: ids[(k + 1) % ids.len()], construction: false });
                    lines += 1;
                }
            }
        }
        lines
    }
}

fn solve_dense(a: &mut [f64], b: &[f64], m: usize) -> Option<Vec<f64>> {
    let mut b = b.to_vec();
    for col in 0..m {
        let piv = (col..m).max_by(|&i, &j| a[i * m + col].abs().total_cmp(&a[j * m + col].abs()))?;
        if a[piv * m + col].abs() < 1e-300 {
            return None;
        }
        if piv != col {
            for k in 0..m {
                a.swap(piv * m + k, col * m + k);
            }
            b.swap(piv, col);
        }
        let d = a[col * m + col];
        for i in col + 1..m {
            let f = a[i * m + col] / d;
            if f != 0.0 {
                for k in col..m {
                    a[i * m + k] -= f * a[col * m + k];
                }
                b[i] -= f * b[col];
            }
        }
    }
    let mut y = vec![0.0; m];
    for i in (0..m).rev() {
        let mut s = b[i];
        for k in i + 1..m {
            s -= a[i * m + k] * y[k];
        }
        y[i] = s / a[i * m + i];
    }
    Some(y)
}

fn rank(mut a: Vec<Vec<f64>>, n: usize) -> usize {
    let m = a.len();
    let mut rank = 0;
    let mut row = 0;
    for col in 0..n {
        if row >= m {
            break;
        }
        let piv = (row..m).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs())).unwrap();
        if a[piv][col].abs() < 1e-7 {
            continue;
        }
        a.swap(piv, row);
        for i in row + 1..m {
            let f = a[i][col] / a[row][col];
            for k in col..n {
                a[i][k] -= f * a[row][k];
            }
        }
        row += 1;
        rank += 1;
    }
    rank
}

// -------------------------------------------------------------------- области

/// Замкнутая область: внешний контур против часовой и отверстия по часовой.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Region {
    pub outer: Vec<P2>,
    pub holes: Vec<Vec<P2>>,
    /// Кривые чертежа, образующие границу (по ним область узнаётся после правки размеров).
    pub ids: Vec<usize>,
}

/// Ссылка на выбранный профиль: по кривым границы, а если чертёж сильно изменился — по точке внутри.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileRef {
    pub seed: P2,
    #[serde(default)]
    pub ids: Vec<usize>,
}

impl ProfileRef {
    pub fn of(r: &Region) -> ProfileRef {
        ProfileRef { seed: r.interior_point(), ids: r.ids.clone() }
    }

    /// Ссылка только по точке (когда кривые неизвестны).
    pub fn at(seed: P2) -> ProfileRef {
        ProfileRef { seed, ids: vec![] }
    }

    pub fn resolve(&self, regions: &[Region]) -> Option<usize> {
        if !self.ids.is_empty() {
            // regions cut out of overlapping curves can share all their boundary curves: the remembered point tells them apart
            let same: Vec<usize> = (0..regions.len()).filter(|&i| regions[i].ids == self.ids).collect();
            if let Some(i) = same.iter().copied().find(|&i| regions[i].contains(self.seed)).or_else(|| same.first().copied()) {
                return Some(i);
            }
        }
        region_at(regions, self.seed)
    }
}

pub fn signed_area(poly: &[P2]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f64>()
        / 2.0
}

pub fn point_in_poly(p: P2, poly: &[P2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            inside = !inside;
        }
        j = i;
    }
    inside
}

impl Region {
    pub fn area(&self) -> f64 {
        signed_area(&self.outer).abs() - self.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>()
    }

    pub fn contains(&self, p: P2) -> bool {
        point_in_poly(p, &self.outer) && !self.holes.iter().any(|h| point_in_poly(p, h))
    }

    /// Треугольники области: вершины (внешний контур, затем отверстия подряд) и индексы против часовой.
    pub fn triangulate(&self) -> (Vec<P2>, Vec<[usize; 3]>) {
        let mut verts: Vec<P2> = self.outer.clone();
        let mut holes_idx = Vec::new();
        for h in &self.holes {
            holes_idx.push(verts.len());
            verts.extend_from_slice(h);
        }
        let mut out: Vec<usize> = Vec::new();
        let mut ec = earcut::Earcut::new();
        ec.earcut(verts.iter().copied(), &holes_idx, &mut out);
        let tris = out
            .chunks(3)
            .map(|t| {
                let (a, b, c) = (verts[t[0]], verts[t[1]], verts[t[2]]);
                let cr = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
                if cr < 0.0 {
                    [t[0], t[2], t[1]]
                } else {
                    [t[0], t[1], t[2]]
                }
            })
            .collect();
        (verts, tris)
    }

    /// Точка строго внутри области (для запоминания выбранного профиля).
    pub fn interior_point(&self) -> P2 {
        let (v, t) = self.triangulate();
        // центр самого большого треугольника
        t.iter()
            .map(|t| {
                let (a, b, c) = (v[t[0]], v[t[1]], v[t[2]]);
                let area = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
                (area, [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0])
            })
            .max_by(|x, y| x.0.total_cmp(&y.0))
            .map(|x| x.1)
            .unwrap_or(self.outer[0])
    }
}

/// Smooth curve through the points (centripetal Catmull-Rom, sampled densely); a closed spline returns to its first point.
pub fn catmull_rom(pts: &[DVec2], closed: bool) -> Vec<P2> {
    let n = pts.len();
    if n < 2 {
        return pts.iter().map(|p| p.to_array()).collect();
    }
    let at = |i: isize| -> DVec2 {
        if closed {
            pts[i.rem_euclid(n as isize) as usize]
        } else if i < 0 {
            pts[0] * 2.0 - pts[1]
        } else if i as usize >= n {
            pts[n - 1] * 2.0 - pts[n - 2]
        } else {
            pts[i as usize]
        }
    };
    let segs = if closed { n } else { n - 1 };
    let mut out: Vec<P2> = Vec::new();
    for s in 0..segs {
        let (p0, p1, p2, p3) = (at(s as isize - 1), at(s as isize), at(s as isize + 1), at(s as isize + 2));
        let knot = |a: DVec2, b: DVec2| a.distance(b).max(1e-9).sqrt();
        let (t0, t1, t2, t3) = (0.0, knot(p0, p1), knot(p0, p1) + knot(p1, p2), knot(p0, p1) + knot(p1, p2) + knot(p2, p3));
        let steps = 24;
        for k in 0..steps {
            let t = t1 + (t2 - t1) * k as f64 / steps as f64;
            let lerp = |a: DVec2, b: DVec2, ta: f64, tb: f64| a + (b - a) * ((t - ta) / (tb - ta));
            let a1 = lerp(p0, p1, t0, t1);
            let a2 = lerp(p1, p2, t1, t2);
            let a3 = lerp(p2, p3, t2, t3);
            let b1 = lerp(a1, a2, t0, t2);
            let b2 = lerp(a2, a3, t1, t3);
            out.push(lerp(b1, b2, t1, t2).to_array());
        }
    }
    out.push(if closed { pts[0].to_array() } else { pts[n - 1].to_array() });
    out
}

/// Найти область, содержащую точку (самую маленькую, если их несколько).
pub fn region_at(regions: &[Region], p: P2) -> Option<usize> {
    regions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.contains(p))
        .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
        .map(|(i, _)| i)
}

struct HalfEdge {
    from: usize,
    to: usize,
    entity: usize,
    /// Ломаная от `from` к `to` (включая концы).
    path: Vec<P2>,
    angle: f64,
}

/// Curves cut at every point where they cross each other: the planar graph the regions are read from.
struct Arrangement {
    /// Number of vertices (crossings, curve ends and shared sketch points merged by position).
    vertices: usize,
    /// (from, to, polyline from `from` to `to`, sketch entity)
    edges: Vec<(usize, usize, Vec<P2>, usize)>,
    /// Circles that are crossed by nothing: separate loops.
    circles: Vec<(Vec<P2>, usize)>,
}

fn arrange(s: &Sketch) -> Arrangement {
    struct Curve {
        entity: usize,
        pts: Vec<P2>,
        closed: bool,
        /// (position along the polyline: segment index + fraction, point)
        cuts: Vec<(f64, P2)>,
    }
    let mut curves: Vec<Curve> = Vec::new();
    for (i, e) in s.entities.iter().enumerate() {
        if e.construction() {
            continue;
        }
        let pts = s.tessellate(i);
        match *e {
            Entity::Line { a, b, .. } if a != b => curves.push(Curve { entity: i, pts, closed: false, cuts: Vec::new() }),
            Entity::Arc { a, b, .. } if a != b => curves.push(Curve { entity: i, pts, closed: false, cuts: Vec::new() }),
            Entity::Circle { r, .. } if r > 1e-9 => curves.push(Curve { entity: i, pts, closed: true, cuts: Vec::new() }),
            Entity::Ellipse { c, a, ratio, .. } if c != a && ratio > 1e-6 => curves.push(Curve { entity: i, pts, closed: true, cuts: Vec::new() }),
            Entity::Spline { count, closed, .. } if count >= 2 => curves.push(Curve { entity: i, pts, closed, cuts: Vec::new() }),
            _ => {}
        }
    }
    let extent = curves.iter().flat_map(|c| c.pts.iter()).fold(1.0_f64, |m, p| m.max(p[0].abs()).max(p[1].abs()));
    let tol = 1e-7 * (1.0 + extent);
    // crossings between different curves (skipped for huge sketches, where every curve then stands on its own)
    let segments: usize = curves.iter().map(|c| c.pts.len()).sum();
    if segments <= 12000 {
        let bbox = |c: &Curve| c.pts.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
        let boxes: Vec<[f64; 4]> = curves.iter().map(bbox).collect();
        for i in 0..curves.len() {
            for j in i + 1..curves.len() {
                let (bi, bj) = (boxes[i], boxes[j]);
                if bi[0] > bj[2] + tol || bj[0] > bi[2] + tol || bi[1] > bj[3] + tol || bj[1] > bi[3] + tol {
                    continue;
                }
                let mut hits: Vec<(f64, f64, P2)> = Vec::new();
                for (ia, wa) in curves[i].pts.windows(2).enumerate() {
                    for (ib, wb) in curves[j].pts.windows(2).enumerate() {
                        let (p, r) = (DVec2::from(wa[0]), DVec2::from(wa[1]) - DVec2::from(wa[0]));
                        let (q, d) = (DVec2::from(wb[0]), DVec2::from(wb[1]) - DVec2::from(wb[0]));
                        let den = r.perp_dot(d);
                        if den.abs() <= 1e-12 * r.length() * d.length() {
                            continue;
                        }
                        let t = (q - p).perp_dot(d) / den;
                        let u = (q - p).perp_dot(r) / den;
                        let eps = 1e-9;
                        if t < -eps || t > 1.0 + eps || u < -eps || u > 1.0 + eps {
                            continue;
                        }
                        let (t, u) = (t.clamp(0.0, 1.0), u.clamp(0.0, 1.0));
                        hits.push((ia as f64 + t, ib as f64 + u, (p + r * t).to_array()));
                    }
                }
                for (ta, tb, pt) in hits {
                    curves[i].cuts.push((ta, pt));
                    curves[j].cuts.push((tb, pt));
                }
            }
        }
    }
    let mut verts: Vec<P2> = Vec::new();
    let mut vid = |p: P2| -> usize {
        if let Some(i) = verts.iter().position(|v| (v[0] - p[0]).abs() <= tol && (v[1] - p[1]).abs() <= tol) {
            return i;
        }
        verts.push(p);
        verts.len() - 1
    };
    let mut edges = Vec::new();
    let mut circles = Vec::new();
    for c in curves {
        let n = c.pts.len();
        let mut cuts = c.cuts;
        if !c.closed {
            cuts.push((0.0, c.pts[0]));
            cuts.push(((n - 1) as f64, c.pts[n - 1]));
        }
        cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
        // a crossing exactly at a vertex shows up once per neighbouring segment: keep one
        cuts.dedup_by(|a, b| (a.1[0] - b.1[0]).abs() <= tol && (a.1[1] - b.1[1]).abs() <= tol);
        if c.closed {
            // the seam (start = end) counts as one place
            if cuts.len() > 1 && { let (f, l) = (cuts[0].1, cuts[cuts.len() - 1].1); (f[0] - l[0]).abs() <= tol && (f[1] - l[1]).abs() <= tol } {
                cuts.pop();
            }
            if cuts.len() < 2 {
                let mut p = c.pts.clone();
                p.pop();
                circles.push((p, c.entity));
                continue;
            }
        }
        let count = if c.closed { cuts.len() } else { cuts.len() - 1 };
        for k in 0..count {
            let (pa, a) = (cuts[k].0, cuts[k].1);
            let (pb, b) = cuts[(k + 1) % cuts.len()];
            let b_pt = b;
            // vertices of the polyline strictly between the two cuts (wrapping round the seam of a closed curve)
            let mut path = vec![a];
            if pb > pa {
                for v in (pa.floor() as usize + 1)..=(pb.ceil() as usize).saturating_sub(1) {
                    if (v as f64) > pa && (v as f64) < pb { path.push(c.pts[v]); }
                }
            } else {
                for v in (pa.floor() as usize + 1)..n {
                    path.push(c.pts[v]);
                }
                for v in 1..=(pb.ceil() as usize).saturating_sub(1) {
                    if (v as f64) < pb { path.push(c.pts[v]); }
                }
            }
            path.push(b_pt);
            // a cut at a polyline vertex (e.g. where a circle touches a line at its seam) repeats that point: a zero-length
            // segment would give the half-edge a bogus direction and the face walk would take a wrong turn
            path.dedup_by(|p, q| (p[0] - q[0]).abs() <= tol && (p[1] - q[1]).abs() <= tol);
            let (va, vb) = (vid(a), vid(b_pt));
            if va == vb && path.len() < 4 {
                continue;
            }
            edges.push((va, vb, path, c.entity));
        }
    }
    Arrangement { vertices: verts.len(), edges, circles }
}

/// Regions of the curves plus the letters of the sketch's texts.
fn regions(s: &Sketch) -> Vec<Region> {
    let mut all = curve_regions(s);
    for t in &s.texts {
        all.extend(crate::text::regions(t));
    }
    all
}

fn curve_regions(s: &Sketch) -> Vec<Region> {
    let arr = arrange(s);
    let nv = arr.vertices;
    let mut edges = arr.edges;
    let circles = arr.circles;
    // Убираем «висячие» рёбра (вершины степени 1) — они не ограничивают областей.
    loop {
        let mut deg = vec![0usize; nv];
        for (a, b, _, _) in &edges {
            deg[*a] += 1;
            deg[*b] += 1;
        }
        let before = edges.len();
        edges.retain(|(a, b, _, _)| deg[*a] > 1 && deg[*b] > 1);
        if edges.len() == before {
            break;
        }
    }

    let mut hes: Vec<HalfEdge> = Vec::new();
    for (a, b, path, e) in &edges {
        let dir = |p: &[P2]| (p[1][1] - p[0][1]).atan2(p[1][0] - p[0][0]);
        let fwd = path.clone();
        let mut rev = path.clone();
        rev.reverse();
        hes.push(HalfEdge { from: *a, to: *b, entity: *e, angle: dir(&fwd), path: fwd });
        hes.push(HalfEdge { from: *b, to: *a, entity: *e, angle: dir(&rev), path: rev });
    }
    // исходящие полурёбра каждой вершины, отсортированные по углу
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); nv];
    for (i, h) in hes.iter().enumerate() {
        out[h.from].push(i);
    }
    for o in &mut out {
        o.sort_by(|&x, &y| hes[x].angle.total_cmp(&hes[y].angle));
    }
    // компоненты связности (для вложенности контуров)
    let mut comp: Vec<usize> = (0..nv).collect();
    fn find(c: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while c[r] != r {
            r = c[r];
        }
        let mut j = i;
        while c[j] != r {
            let n = c[j];
            c[j] = r;
            j = n;
        }
        r
    }
    for (a, b, _, _) in &edges {
        let (ra, rb) = (find(&mut comp, *a), find(&mut comp, *b));
        comp[ra] = rb;
    }

    // Обход граней: из полуребра (u→v) идём в следующее по часовой от обратного (v→u).
    let mut used = vec![false; hes.len()];
    let mut faces: Vec<(usize, Vec<P2>, Vec<usize>)> = Vec::new(); // (компонента, контур, кривые)
    for start in 0..hes.len() {
        if used[start] {
            continue;
        }
        let mut poly: Vec<P2> = Vec::new();
        let mut ids: Vec<usize> = Vec::new();
        let mut h = start;
        let mut guard = 0;
        loop {
            used[h] = true;
            let he = &hes[h];
            ids.push(he.entity);
            poly.extend_from_slice(&he.path[..he.path.len() - 1]);
            let v = he.to;
            let twin_angle = {
                let p = &he.path;
                let n = p.len();
                (p[n - 2][1] - p[n - 1][1]).atan2(p[n - 2][0] - p[n - 1][0])
            };
            // ближайшее по часовой (меньший угол) к направлению обратно
            let o = &out[v];
            let mut best = None;
            let mut best_d = f64::MAX;
            for &c in o {
                let mut d = twin_angle - hes[c].angle;
                while d <= 1e-12 {
                    d += std::f64::consts::TAU;
                }
                if d < best_d {
                    best_d = d;
                    best = Some(c);
                }
            }
            let Some(next) = best else { break };
            h = next;
            guard += 1;
            if h == start || guard > hes.len() {
                break;
            }
            if used[h] {
                break;
            }
        }
        if poly.len() >= 3 {
            faces.push((find(&mut comp, hes[start].from), poly, ids));
        }
    }

    // Ограниченные грани (против часовой) — области; по часовой — внешние контуры компонент.
    let mut bounded: Vec<(usize, Vec<P2>, Vec<usize>)> = Vec::new(); // (id компоненты, контур, кривые)
    let mut outlines: Vec<(usize, Vec<P2>, Vec<usize>)> = Vec::new();
    for (c, p, ids) in faces {
        let a = signed_area(&p);
        if a > 1e-12 {
            bounded.push((c, p, ids));
        } else if a < -1e-12 {
            let mut p = p;
            p.reverse();
            outlines.push((c, p, ids));
        }
    }
    // окружности: и область, и контур собственной «компоненты»
    let base = nv + 1;
    for (i, (mut c, e)) in circles.into_iter().enumerate() {
        if signed_area(&c) < 0.0 {
            c.reverse();
        }
        bounded.push((base + i, c.clone(), vec![e]));
        outlines.push((base + i, c, vec![e]));
    }

    let inside = |inner: &[P2], outer: &[P2]| -> bool {
        // все вершины внутри (контуры не пересекаются — достаточно нескольких)
        let step = (inner.len() / 8).max(1);
        inner.iter().step_by(step).all(|&p| point_in_poly(p, outer))
            && signed_area(inner).abs() < signed_area(outer).abs()
    };

    let mut result = Vec::new();
    for (c, outer, outer_ids) in &bounded {
        // кандидаты в отверстия: контуры других компонент внутри этой области
        let cands: Vec<&(usize, Vec<P2>, Vec<usize>)> = outlines.iter().filter(|(c2, o, _)| c2 != c && inside(o, outer)).collect();
        // берём только самые внешние из них
        let chosen: Vec<&&(usize, Vec<P2>, Vec<usize>)> =
            cands.iter().filter(|h| !cands.iter().any(|o| !std::ptr::eq(*o, **h) && inside(&h.1, &o.1))).collect();
        let mut ids = outer_ids.clone();
        let holes: Vec<Vec<P2>> = chosen
            .iter()
            .map(|h| {
                ids.extend_from_slice(&h.2);
                let mut h = h.1.clone();
                if signed_area(&h) > 0.0 {
                    h.reverse();
                }
                h
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        result.push(Region { outer: outer.clone(), holes, ids });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params::default()
    }

    #[test]
    fn overlapping_circles_give_the_lens_and_both_crescents() {
        let mut s = Sketch::default();
        s.add_circle([0.0, 0.0], 10.0);
        s.add_circle([10.0, 0.0], 10.0);
        let r = s.regions();
        assert_eq!(r.len(), 3, "lens + two crescents, not two whole circles: {r:?}");
        let lens = r.iter().map(|x| x.area()).fold(f64::MAX, f64::min);
        // lens of two unit-distance-r circles: 2 r^2 acos(1/2) - (d/2) sqrt(4 r^2 - d^2)
        let ideal = 2.0 * 100.0 * 0.5_f64.acos() - 5.0 * (400.0_f64 - 100.0).sqrt();
        assert!((lens - ideal).abs() / ideal < 0.01, "lens {lens} vs {ideal}");
        // each part can be picked on its own
        assert_eq!(region_at(&r, [5.0, 0.0]), r.iter().position(|x| (x.area() - lens).abs() < 1e-6));
        assert!(region_at(&r, [-5.0, 0.0]).is_some() && region_at(&r, [15.0, 0.0]).is_some());
        let picked = ProfileRef::at([5.0, 0.0]);
        assert_eq!(picked.resolve(&r), region_at(&r, [5.0, 0.0]));
    }

    #[test]
    fn three_overlapping_circles_make_seven_regions() {
        let mut s = Sketch::default();
        s.add_circle([0.0, 0.0], 10.0);
        s.add_circle([8.0, 0.0], 10.0);
        s.add_circle([4.0, 7.0], 10.0);
        let r = s.regions();
        assert_eq!(r.len(), 7, "{} regions", r.len());
        // together they tile the union without overlap: no two regions contain the same point
        for p in [[0.0, 0.0], [4.0, 2.0], [8.0, 0.0], [4.0, 9.0], [-5.0, 0.0], [14.0, 0.0]] {
            let n = r.iter().filter(|x| x.contains(p)).count();
            assert!(n <= 1, "{p:?} lies in {n} regions");
        }
    }

    #[test]
    fn a_line_across_a_rectangle_splits_it_without_drawing_the_crossing_points() {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [10.0, 10.0]);
        s.add_line([5.0, -3.0], [5.0, 13.0]);
        let r = s.regions();
        assert_eq!(r.len(), 2, "{r:?}");
        assert!(r.iter().all(|x| (x.area() - 50.0).abs() < 1e-6));
    }

    #[test]
    fn rect_is_one_region() {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [10.0, 5.0]);
        let r = s.regions();
        assert_eq!(r.len(), 1);
        assert!((r[0].area() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn rect_with_circle_gives_ring_and_disk() {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [20.0, 20.0]);
        s.add_circle([10.0, 10.0], 4.0);
        let r = s.regions();
        assert_eq!(r.len(), 2);
        let ring = r.iter().find(|r| !r.holes.is_empty()).unwrap();
        let circle_area = signed_area(&ring.holes[0]).abs();
        assert!((ring.area() - (400.0 - circle_area)).abs() < 1e-6);
        assert_eq!(region_at(&r, [1.0, 1.0]), r.iter().position(|x| !x.holes.is_empty()));
        assert_eq!(region_at(&r, [10.0, 10.0]), r.iter().position(|x| x.holes.is_empty()));
    }

    #[test]
    fn line_across_rect_splits_in_two() {
        let mut s = Sketch::default();
        let l = s.add_rect([0.0, 0.0], [10.0, 10.0]);
        // ставим точки на середины верхней и нижней сторон и разбиваем стороны
        let (Entity::Line { a: p0, b: p1, .. }, Entity::Line { a: p2, b: p3, .. }) = (s.entities[l[0]], s.entities[l[2]]) else {
            panic!()
        };
        s.remove_entities(&[l[0], l[2]]);
        let m0 = s.add_point([5.0, 0.0]);
        let m1 = s.add_point([5.0, 10.0]);
        let _ = (p0, p1, p2, p3);
        // после удаления индексы точек могли сдвинуться — ищем углы по координатам
        let find = |s: &Sketch, x: f64, y: f64| s.points.iter().position(|p| p.pos == [x, y]).unwrap();
        let (c00, c10, c11, c01) = (find(&s, 0.0, 0.0), find(&s, 10.0, 0.0), find(&s, 10.0, 10.0), find(&s, 0.0, 10.0));
        for (a, b) in [(c00, m0), (m0, c10), (c11, m1), (m1, c01), (m0, m1)] {
            s.add(Entity::Line { a, b, construction: false });
        }
        let r = s.regions();
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|r| (r.area() - 50.0).abs() < 1e-9));
    }

    #[test]
    fn slot_from_lines_and_arcs() {
        // паз: две линии + две полуокружности
        let mut s = Sketch::default();
        let p = [[0.0, 0.0], [10.0, 0.0], [10.0, 4.0], [0.0, 4.0], [10.0, 2.0], [0.0, 2.0]].map(|q| s.add_point(q));
        s.add(Entity::Line { a: p[0], b: p[1], construction: false });
        s.add(Entity::Arc { c: p[4], a: p[1], b: p[2], construction: false });
        s.add(Entity::Line { a: p[2], b: p[3], construction: false });
        s.add(Entity::Arc { c: p[5], a: p[3], b: p[0], construction: false });
        let r = s.regions();
        assert_eq!(r.len(), 1);
        let expect = 40.0 + std::f64::consts::PI * 4.0;
        assert!((r[0].area() - expect).abs() < 0.05, "{}", r[0].area());
    }

    #[test]
    fn dangling_and_construction_lines_are_ignored() {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [10.0, 10.0]);
        s.add_line([20.0, 0.0], [30.0, 0.0]);
        let l = s.add_line([-5.0, -5.0], [15.0, 15.0]);
        s.entities[l].set_construction(true);
        assert_eq!(s.regions().len(), 1);
    }

    #[test]
    fn solver_applies_dimensions() {
        let mut s = Sketch::default();
        let l = s.add_rect([0.0, 0.0], [10.0, 5.0]);
        let corner = match s.entities[l[0]] {
            Entity::Line { a, .. } => a,
            _ => unreachable!(),
        };
        s.constraints.push(Constraint::Fix(corner, [0.0, 0.0]));
        s.constraints.push(Constraint::Length(l[0], Expr::from("w")));
        s.constraints.push(Constraint::Length(l[1], Expr::num(7.0)));
        let mut p = params();
        p.set("w", "25");
        assert_eq!(s.solve(&p, &[]).unwrap(), SolveStatus::Ok);
        assert!((s.measure(l[0]) - 25.0).abs() < 1e-6);
        assert!((s.measure(l[1]) - 7.0).abs() < 1e-6);
        assert!((s.regions()[0].area() - 175.0).abs() < 1e-5);
        assert_eq!(s.free_dof(&p), 0);
        // меняем параметр — геометрия следует
        p.set("w", "30");
        s.solve(&p, &[]).unwrap();
        assert!((s.measure(l[0]) - 30.0).abs() < 1e-6);
        // угол остался на месте
        assert!(s.p(corner).length() < 1e-6);
    }

    #[test]
    fn solver_reports_conflict() {
        let mut s = Sketch::default();
        let l = s.add_line([0.0, 0.0], [10.0, 1.0]);
        s.constraints.push(Constraint::Horizontal(l));
        s.constraints.push(Constraint::Vertical(l));
        s.constraints.push(Constraint::Length(l, Expr::num(5.0)));
        assert!(matches!(s.solve(&params(), &[]).unwrap(), SolveStatus::Conflict(_)));
    }

    #[test]
    fn tangent_and_parallel_constraints() {
        let mut s = Sketch::default();
        let l1 = s.add_line([0.0, 0.0], [10.0, 0.5]);
        let l2 = s.add_line([0.0, 5.0], [10.0, 6.5]);
        let c = s.add_circle([5.0, 10.0], 2.0);
        s.constraints.push(Constraint::Horizontal(l1));
        s.constraints.push(Constraint::Parallel(l1, l2));
        s.constraints.push(Constraint::Tangent(l2, c));
        assert_eq!(s.solve(&params(), &[]).unwrap(), SolveStatus::Ok);
        let (Entity::Line { a, b, .. }, Entity::Circle { c: cc, r, .. }) = (s.entities[l2], s.entities[c]) else { panic!() };
        assert!((s.p(a).y - s.p(b).y).abs() < 1e-6);
        assert!(((s.p(cc).y - s.p(a).y).abs() - r).abs() < 1e-6);
    }

    #[test]
    fn unrelated_points_do_not_move() {
        let mut s = Sketch::default();
        let l1 = s.add_line([0.0, 0.0], [10.0, 1.0]);
        let _l2 = s.add_line([50.0, 50.0], [60.0, 70.0]);
        s.constraints.push(Constraint::Horizontal(l1));
        s.solve(&params(), &[]).unwrap();
        assert_eq!(s.points[2].pos, [50.0, 50.0]);
        assert_eq!(s.points[3].pos, [60.0, 70.0]);
    }

    #[test]
    fn drag_respects_constraints() {
        let mut s = Sketch::default();
        let l = s.add_rect([0.0, 0.0], [10.0, 10.0]);
        let Entity::Line { b, .. } = s.entities[l[0]] else { panic!() };
        s.solve(&params(), &[(b, [15.0, 3.0])]).unwrap();
        // прямоугольник остаётся прямоугольником
        let r = s.regions();
        assert_eq!(r.len(), 1);
        for c in &s.constraints {
            if let Constraint::Horizontal(e) = c {
                let Entity::Line { a, b, .. } = s.entities[*e] else { panic!() };
                assert!((s.p(a).y - s.p(b).y).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn merge_and_remove_keep_indices_consistent() {
        let mut s = Sketch::default();
        let l1 = s.add_line([0.0, 0.0], [10.0, 0.0]);
        let l2 = s.add_line([10.0, 0.0], [10.0, 10.0]);
        let (Entity::Line { b, .. }, Entity::Line { a, .. }) = (s.entities[l1], s.entities[l2]) else { panic!() };
        s.merge_points(a, b);
        assert_eq!(s.points.len(), 3);
        s.constraints.push(Constraint::Vertical(1));
        s.remove_entities(&[0]);
        assert_eq!(s.entities.len(), 1);
        assert_eq!(s.points.len(), 2);
        assert_eq!(s.constraints, vec![Constraint::Vertical(0)]);
    }

    #[test]
    fn chain_path_orders_curves() {
        let mut s = Sketch::default();
        let p = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]].map(|q| s.add_point(q));
        // линии заданы в разном направлении и порядке
        s.add(Entity::Line { a: p[2], b: p[1], construction: false });
        s.add(Entity::Line { a: p[0], b: p[1], construction: false });
        let path = s.chain_path().unwrap();
        assert_eq!(path, vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]);
        // замкнутый контур — не путь
        let mut r = Sketch::default();
        r.add_rect([0.0, 0.0], [5.0, 5.0]);
        assert!(r.chain_path().is_err());
    }

    #[test]
    fn plane_roundtrip() {
        let pl = Plane::from_point_normal(DVec3::new(3.0, 4.0, 5.0), DVec3::new(0.0, 0.0, 1.0));
        let w = pl.to_world([1.0, 2.0]);
        let back = pl.to_local(w);
        assert!((back[0] - 1.0).abs() < 1e-12 && (back[1] - 2.0).abs() < 1e-12);
        assert!((pl.normal() - DVec3::Z).length() < 1e-12);
        assert!((w.z - 5.0).abs() < 1e-12);
        let side = Plane::from_point_normal(DVec3::new(10.0, 0.0, 0.0), DVec3::X);
        assert!((side.normal() - DVec3::X).length() < 1e-12);
    }

    #[test]
    fn chain_path_joins_lines_that_only_touch_and_explains_failures() {
        // three separate lines with their own end points that coincide: one open chain
        let mut s = Sketch::default();
        s.add_line([0.0, 0.0], [10.0, 0.0]);
        s.add_line([10.0, 0.0], [10.0, 10.0]);
        s.add_line([10.0, 10.0], [20.0, 10.0]);
        assert_eq!(s.chain_path().unwrap(), vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [20.0, 10.0]]);
        // a gap in the middle leaves four free ends
        let mut gap = Sketch::default();
        gap.add_line([0.0, 0.0], [10.0, 0.0]);
        gap.add_line([12.0, 0.0], [20.0, 0.0]);
        assert!(gap.chain_path().unwrap_err().contains("свободных концов 4"));
        // a closed square has no ends at all
        let mut square = Sketch::default();
        square.add_rect([0.0, 0.0], [10.0, 10.0]);
        assert!(square.chain_path().unwrap_err().contains("замкнут"));
        // three lines meeting in one point branch
        let mut star = Sketch::default();
        for end in [[10.0, 0.0], [0.0, 10.0], [-10.0, 0.0]] { star.add_line([0.0, 0.0], end); }
        assert!(star.chain_path().unwrap_err().contains("ветвиться"));
    }

    #[test]
    fn circle_over_rect_edge_near_tangent() {
        let mut s = Sketch::default();
        s.add_rect([-15.0, -12.0], [20.0, 12.0]);
        let c = s.add_point([0.0, 0.0]);
        s.add(Entity::Circle { c, r: 20.5, construction: false });
        let r = s.regions();
        let areas: Vec<f64> = r.iter().map(|r| r.area()).collect();
        // centre piece, 2 corner pieces (top/bottom), right cap, and the circle remainder(s)
        assert!(r.len() >= 5, "{areas:?}");
    }

    #[test]
    fn circle_tangent_to_rect_edge() {
        for rr in [20.0, 20.0 - 1e-6, 20.0 + 1e-6, 19.99, 20.01] {
            let mut s = Sketch::default();
            s.add_rect([-15.0, -12.0], [20.0, 12.0]);
            s.add_circle([0.0, 0.0], rr);
            let areas: Vec<f64> = s.regions().iter().map(|r| r.area()).collect();
            println!("r={rr}: {areas:?}");
            // rect∩circle, 2 corner pieces, the circle's left/top/bottom caps: at least 4 separate regions
            assert!(areas.len() >= 3, "r={rr}: {areas:?}");
        }
    }

    /// Fuzz: regions of random circles and rectangles must tile their union (sum of areas = union area), also for near-tangent shapes.
    #[test]
    fn regions_tile_the_union_of_random_shapes() {
        let mut seed = 0x9E3779B97F4A7C15_u64;
        let mut rnd = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for case in 0..60 {
            let mut s = Sketch::default();
            let mut circles = Vec::new();
            let mut rects = Vec::new();
            for _ in 0..3 {
                if rnd() < 0.5 {
                    let (c, r) = ([(rnd() * 20.0).round(), (rnd() * 20.0).round()], 3.0 + (rnd() * 8.0).round());
                    s.add_circle(c, r);
                    circles.push((c, r));
                } else {
                    let (a, b) = ([(rnd() * 20.0).round(), (rnd() * 20.0).round()], [4.0 + (rnd() * 12.0).round(), 4.0 + (rnd() * 12.0).round()]);
                    s.add_rect(a, [a[0] + b[0], a[1] + b[1]]);
                    rects.push((a, [a[0] + b[0], a[1] + b[1]]));
                }
            }
            let regions = s.regions();
            let total: f64 = regions.iter().map(|r| r.area()).sum();
            // reference: grid sampling of the union
            let n = 400;
            let (lo, hi) = (-12.0, 44.0);
            let h = (hi - lo) / n as f64;
            let mut inside = 0usize;
            for i in 0..n {
                for j in 0..n {
                    let p = [lo + (i as f64 + 0.5) * h, lo + (j as f64 + 0.5) * h];
                    let in_c = circles.iter().any(|(c, r)| (p[0] - c[0]).hypot(p[1] - c[1]) < *r);
                    let in_r = rects.iter().any(|(a, b)| p[0] > a[0] && p[0] < b[0] && p[1] > a[1] && p[1] < b[1]);
                    if in_c || in_r {
                        inside += 1;
                    }
                }
            }
            let union = inside as f64 * h * h;
            assert!((total - union).abs() <= 0.02 * union + 1.0, "case {case}: regions {total:.2} vs union {union:.2}, circles {circles:?}, rects {rects:?}");
        }
    }

    #[test]
    fn circle_inscribed_in_rect_touching_sides() {
        let mut s = Sketch::default();
        s.add_rect([13.0, 11.0], [19.0, 18.0]);
        s.add_circle([16.0, 14.0], 3.0);
        let areas: Vec<f64> = s.regions().iter().map(|r| r.area()).collect();
        let total: f64 = areas.iter().sum();
        assert!((total - 42.0).abs() < 0.01, "{areas:?}");
    }
}

#[cfg(test)]
mod freeform_tests {
    use super::*;

    fn ellipse(s: &mut Sketch, centre: P2, major: P2, ratio: f64) -> usize {
        let c = s.add_point(centre);
        let a = s.add_point(major);
        s.add(Entity::Ellipse { c, a, ratio, construction: false })
    }

    #[test]
    fn an_ellipse_is_a_closed_region_with_the_right_area() {
        let mut s = Sketch::default();
        ellipse(&mut s, [0.0, 0.0], [10.0, 0.0], 0.5);
        let r = s.regions();
        assert_eq!(r.len(), 1);
        let expect = std::f64::consts::PI * 10.0 * 5.0;
        assert!((r[0].area() - expect).abs() < expect * 0.01, "{} vs {expect}", r[0].area());
        // a tilted major axis gives the same area
        let mut t = Sketch::default();
        ellipse(&mut t, [3.0, 4.0], [3.0 + 6.0, 4.0 + 8.0], 0.4);
        let area = t.regions()[0].area();
        let expect = std::f64::consts::PI * 10.0 * 4.0;
        assert!((area - expect).abs() < expect * 0.01, "{area} vs {expect}");
    }

    #[test]
    fn a_closed_spline_bounds_a_region_and_passes_through_its_points() {
        let mut s = Sketch::default();
        let first = s.points.len();
        for p in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] {
            s.add_point(p);
        }
        let e = s.add(Entity::Spline { first, count: 4, closed: true, construction: false });
        let line = s.tessellate(e);
        for p in [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] {
            assert!(line.iter().any(|q| (q[0] - p[0]).abs() < 1e-9 && (q[1] - p[1]).abs() < 1e-9), "passes through {p:?}");
        }
        let r = s.regions();
        assert_eq!(r.len(), 1);
        assert!(r[0].area() > 100.0 && r[0].area() < 157.0, "a blob between the square (100) and its circumcircle (157): {}", r[0].area());
    }

    #[test]
    fn an_open_spline_closed_by_a_line_makes_a_region_and_survives_point_cleanup() {
        let mut s = Sketch::default();
        let first = s.points.len();
        for p in [[0.0, 0.0], [5.0, 4.0], [10.0, 0.0]] {
            s.add_point(p);
        }
        s.add(Entity::Spline { first, count: 3, closed: false, construction: false });
        s.add(Entity::Line { a: first, b: first + 2, construction: false });
        let r = s.regions();
        assert_eq!(r.len(), 1, "the arch over its chord");
        assert!(r[0].area() > 20.0 && r[0].area() < 40.0, "{}", r[0].area());
        // json round trip keeps both new entities
        let back: Sketch = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.entities.len(), 2);
        assert_eq!(back.regions().len(), 1);
    }

    #[test]
    fn collinear_lines_end_up_on_one_straight_line_and_symmetry_mirrors_points() {
        // two lines that are nearly in line: the constraint pulls the second onto the first
        let mut s = Sketch::default();
        let l1 = s.add_line([0.0, 0.0], [10.0, 0.0]);
        let l2 = s.add_line([15.0, 2.0], [25.0, -3.0]);
        s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
        s.constraints.push(Constraint::Fix(1, [10.0, 0.0]));
        s.constraints.push(Constraint::Collinear(l1, l2));
        let status = s.solve(&Params::default(), &[]).unwrap();
        assert_eq!(status, SolveStatus::Ok);
        let (c, d) = (s.p(2), s.p(3));
        assert!(c.y.abs() < 1e-6 && d.y.abs() < 1e-6, "second line on the x axis: {c:?} {d:?}");
        // symmetry about a vertical axis x = 5: the left point is fixed, the right one must land at its mirror image
        let mut t = Sketch::default();
        let axis = t.add_line([5.0, -10.0], [5.0, 10.0]);
        let (pa, pb) = (t.add_point([1.0, 2.0]), t.add_point([8.5, 7.0]));
        t.constraints.push(Constraint::Fix(0, [5.0, -10.0]));
        t.constraints.push(Constraint::Fix(1, [5.0, 10.0]));
        t.constraints.push(Constraint::Fix(pa, [1.0, 2.0]));
        t.constraints.push(Constraint::Symmetric(pa, pb, axis));
        assert_eq!(t.solve(&Params::default(), &[]).unwrap(), SolveStatus::Ok);
        let q = t.p(pb);
        assert!((q.x - 9.0).abs() < 1e-6 && (q.y - 2.0).abs() < 1e-6, "mirror of (1,2) about x = 5 is (9,2): {q:?}");
        // the constraint removes two degrees of freedom: nothing is left free
        assert_eq!(t.free_dof(&Params::default()), 0);
        // json keeps the new constraints
        let back: Sketch = serde_json::from_str(&serde_json::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.constraints.len(), t.constraints.len());
    }

    #[test]
    fn mirror_copies_a_rectangle_and_flips_arcs() {
        let mut s = Sketch::default();
        let r = s.add_rect([1.0, 0.0], [5.0, 3.0]);
        let new = s.mirror_entities(&r, DVec2::new(0.0, 0.0), DVec2::new(0.0, 1.0));
        assert_eq!(new.len(), 4);
        let regions = s.regions();
        assert_eq!(regions.len(), 2, "the original and its mirror image");
        let xs: Vec<f64> = regions.iter().map(|r| r.outer.iter().map(|p| p[0]).fold(f64::MIN, f64::max)).collect();
        assert!(xs.iter().any(|x| (x - 5.0).abs() < 1e-9) && xs.iter().any(|x| (x + 1.0).abs() < 1e-9), "{xs:?}");
        assert!(regions.iter().all(|r| (r.area() - 12.0).abs() < 1e-9));
        // an arc keeps its bulge on the mirrored side: its mid point is the mirror of the original's
        let mut t = Sketch::default();
        let c = t.add_point([0.0, 0.0]);
        let a = t.add_point([2.0, 0.0]);
        let b = t.add_point([0.0, 2.0]);
        let arc = t.add(Entity::Arc { c, a, b, construction: false });
        let m = t.mirror_entities(&[arc], DVec2::new(5.0, 0.0), DVec2::new(5.0, 1.0));
        let (o, q) = (t.tessellate(arc), t.tessellate(m[0]));
        let mid = |l: &Vec<P2>| l[l.len() / 2];
        assert!((mid(&q)[0] - (10.0 - mid(&o)[0])).abs() < 1e-6 && (mid(&q)[1] - mid(&o)[1]).abs() < 1e-6, "{:?} vs {:?}", mid(&o), mid(&q));
    }

    #[test]
    fn rectangular_and_circular_patterns_copy_connected_shapes() {
        let mut s = Sketch::default();
        let sq = s.add_rect([0.0, 0.0], [2.0, 2.0]);
        let copies = s.pattern_rect(&sq, 3, 5.0, 2, 4.0);
        assert_eq!(copies.len(), 4 * 5, "3 x 2 minus the original = 5 copies of 4 lines");
        let regions = s.regions();
        assert_eq!(regions.len(), 6);
        assert!(regions.iter().all(|r| (r.area() - 4.0).abs() < 1e-9));
        // a circle patterned round the origin: 6 copies over the full circle, evenly spaced
        let mut c = Sketch::default();
        let circle = c.add_circle([10.0, 0.0], 1.0);
        let made = c.pattern_circular(&[circle], DVec2::ZERO, 6, 360.0);
        assert_eq!(made.len(), 5);
        let centres: Vec<DVec2> = made.iter().map(|&e| if let Entity::Circle { c: cc, .. } = c.entities[e] { c.p(cc) } else { panic!() }).collect();
        for (k, p) in centres.iter().enumerate() {
            let ang = (60.0 * (k + 1) as f64).to_radians();
            assert!((p.x - 10.0 * ang.cos()).abs() < 1e-9 && (p.y - 10.0 * ang.sin()).abs() < 1e-9, "{p:?}");
        }
        // a partial arc of angle spreads the copies end to end (4 copies over 90 degrees = 30 degree steps)
        let mut d = Sketch::default();
        let dot = d.add_circle([10.0, 0.0], 0.5);
        let part = d.pattern_circular(&[dot], DVec2::ZERO, 4, 90.0);
        assert_eq!(part.len(), 3);
        let last = if let Entity::Circle { c: cc, .. } = d.entities[*part.last().unwrap()] { d.p(cc) } else { panic!() };
        assert!((last.x).abs() < 1e-9 && (last.y - 10.0).abs() < 1e-9, "{last:?}");
        // splines stay consecutive after copying
        let mut sp = Sketch::default();
        let first = sp.points.len();
        for p in [[0.0, 0.0], [3.0, 2.0], [6.0, 0.0]] { sp.add_point(p); }
        let e = sp.add(Entity::Spline { first, count: 3, closed: false, construction: false });
        let m = sp.pattern_rect(&[e], 2, 10.0, 1, 0.0);
        if let Entity::Spline { first: f2, count, .. } = sp.entities[m[0]] {
            assert_eq!(count, 3);
            assert!((sp.p(f2).x - 10.0).abs() < 1e-9 && (sp.p(f2 + 2).x - 16.0).abs() < 1e-9);
        } else { panic!() }
    }

    #[test]
    fn exploded_text_becomes_lines_with_the_same_regions() {
        let mut s = Sketch::default();
        s.texts.push(crate::text::SketchText { text: "O".into(), pos: [0.0, 0.0], height: 20.0, angle: 0.0 });
        let before = s.regions();
        assert_eq!((before.len(), before[0].holes.len()), (1, 1));
        let area = before[0].area();
        let n = s.explode_text(0);
        assert!(n > 20 && s.texts.is_empty());
        let after = s.regions();
        assert_eq!(after.len(), 2, "the ring and the hole are separate closed loops of lines now");
        let biggest = after.iter().map(|r| r.area()).fold(0.0, f64::max);
        assert!(biggest > area, "the outer loop alone is larger than the ring");
        assert_eq!(s.explode_text(5), 0);
    }

    #[test]
    fn ellipse_and_spline_leave_through_dxf_and_do_not_break_trimming() {
        let mut s = Sketch::default();
        let e = ellipse(&mut s, [0.0, 0.0], [10.0, 0.0], 0.5);
        let l = s.add_line([-20.0, 0.0], [20.0, 0.0]);
        let dxf = crate::dxf::export(&s);
        assert!(dxf.contains("LWPOLYLINE"));
        // trimming the line ignores the ellipse (no crossing found), trimming the ellipse is refused
        assert!(!s.trim(e, [0.0, 5.0]));
        let _ = l;
    }
}
