//! Правка чертежа: обрезка, смещение кривой, скругление угла.

use crate::sketch::{Constraint, Entity, Sketch, P2};
use glam::DVec2;
use std::f64::consts::TAU;

#[derive(Clone, Copy)]
enum G {
    L(DVec2, DVec2),
    C(DVec2, f64),
}

fn ang(c: DVec2, p: DVec2) -> f64 {
    (p - c).y.atan2((p - c).x)
}

/// Пересечения «носителей» двух кривых (бесконечная прямая / полная окружность).
fn carrier_hits(e: G, f: G) -> Vec<DVec2> {
    match (e, f) {
        (G::L(a, b), G::L(c, d)) => {
            let (r, s) = (b - a, d - c);
            let den = r.perp_dot(s);
            if den.abs() < 1e-12 {
                return vec![];
            }
            let t = (c - a).perp_dot(s) / den;
            vec![a + r * t]
        }
        (G::L(a, b), G::C(c, r)) | (G::C(c, r), G::L(a, b)) => {
            let d = b - a;
            let f0 = a - c;
            let (qa, qb, qc) = (d.dot(d), 2.0 * f0.dot(d), f0.dot(f0) - r * r);
            let disc = qb * qb - 4.0 * qa * qc;
            if qa < 1e-18 || disc < 0.0 {
                return vec![];
            }
            let sq = disc.sqrt();
            let mut v = vec![a + d * ((-qb - sq) / (2.0 * qa))];
            if sq > 1e-9 {
                v.push(a + d * ((-qb + sq) / (2.0 * qa)));
            }
            v
        }
        (G::C(c1, r1), G::C(c2, r2)) => {
            let d = c1.distance(c2);
            if d < 1e-12 || d > r1 + r2 + 1e-9 || d < (r1 - r2).abs() - 1e-9 {
                return vec![];
            }
            let a = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
            let h = (r1 * r1 - a * a).max(0.0).sqrt();
            let u = (c2 - c1) / d;
            let m = c1 + u * a;
            let mut v = vec![m + u.perp() * h];
            if h > 1e-9 {
                v.push(m - u.perp() * h);
            }
            v
        }
    }
}

impl Sketch {
    fn carrier(&self, e: usize) -> G {
        match self.entities[e] {
            Entity::Line { a, b, .. } => G::L(self.p(a), self.p(b)),
            Entity::Circle { c, r, .. } => G::C(self.p(c), r),
            Entity::Arc { c, a, .. } => G::C(self.p(c), self.p(a).distance(self.p(c))),
        }
    }

    /// (начальный угол, размах) дуги.
    fn arc_span(&self, e: usize) -> Option<(f64, f64)> {
        if let Entity::Arc { c, a, b, .. } = self.entities[e] {
            let (c, pa, pb) = (self.p(c), self.p(a), self.p(b));
            let a0 = ang(c, pa);
            let mut sw = ang(c, pb) - a0;
            if sw <= 1e-12 {
                sw += TAU;
            }
            Some((a0, sw))
        } else {
            None
        }
    }

    fn on_extent(&self, f: usize, p: DVec2) -> bool {
        match self.entities[f] {
            Entity::Line { a, b, .. } => {
                let (pa, pb) = (self.p(a), self.p(b));
                let d = pb - pa;
                let t = (p - pa).dot(d) / d.length_squared().max(1e-18);
                (-1e-9..=1.0 + 1e-9).contains(&t)
            }
            Entity::Circle { .. } => true,
            Entity::Arc { c, .. } => {
                let (a0, sw) = self.arc_span(f).unwrap();
                (ang(self.p(c), p) - a0).rem_euclid(TAU) <= sw + 1e-9
            }
        }
    }

    /// Все точки, где кривая `e` пересекает остальные (по их реальной протяжённости).
    fn hits_on(&self, e: usize) -> Vec<DVec2> {
        let ge = self.carrier(e);
        let mut out = Vec::new();
        for f in 0..self.entities.len() {
            if f == e || self.entities[f].construction() {
                continue;
            }
            for p in carrier_hits(ge, self.carrier(f)) {
                if self.on_extent(f, p) {
                    out.push(p);
                }
            }
        }
        out
    }

    fn point_at(&mut self, p: DVec2) -> usize {
        if let Some(i) = (0..self.points.len()).find(|&i| self.p(i).distance(p) < 1e-6) {
            return i;
        }
        self.add_point(p.to_array())
    }

    fn drop_constraints_of(&mut self, e: usize) {
        self.constraints.retain(|c| !c.entities().contains(&e));
    }

    /// Обрезать кривую `e` по ближайшим пересечениям вокруг точки щелчка. false — обрезать нечего.
    pub fn trim(&mut self, e: usize, click: P2) -> bool {
        let click = DVec2::from(click);
        let hits = self.hits_on(e);
        let cons = self.entities[e].construction();
        match self.entities[e] {
            Entity::Line { a, b, .. } => {
                let (pa, pb) = (self.p(a), self.p(b));
                let d = pb - pa;
                let l2 = d.length_squared();
                let t_of = |p: DVec2| (p - pa).dot(d) / l2;
                let tc = t_of(click).clamp(0.0, 1.0);
                let ts: Vec<f64> = hits.iter().map(|&p| t_of(p)).filter(|t| *t > 1e-6 && *t < 1.0 - 1e-6).collect();
                let lo = ts.iter().copied().filter(|t| *t < tc).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |x| x.max(t))));
                let hi = ts.iter().copied().filter(|t| *t > tc).fold(None, |m: Option<f64>, t| Some(m.map_or(t, |x| x.min(t))));
                self.drop_constraints_of(e);
                match (lo, hi) {
                    (None, None) => {
                        self.remove_entities(&[e]);
                        return true;
                    }
                    (Some(l), Some(h)) => {
                        let (p1, p2) = (self.point_at(pa + d * l), self.point_at(pa + d * h));
                        self.entities[e] = Entity::Line { a, b: p1, construction: cons };
                        self.add(Entity::Line { a: p2, b, construction: cons });
                    }
                    (Some(l), None) => {
                        let p1 = self.point_at(pa + d * l);
                        self.entities[e] = Entity::Line { a, b: p1, construction: cons };
                    }
                    (None, Some(h)) => {
                        let p2 = self.point_at(pa + d * h);
                        self.entities[e] = Entity::Line { a: p2, b, construction: cons };
                    }
                }
            }
            Entity::Circle { c, .. } | Entity::Arc { c, .. } => {
                let cc = self.p(c);
                let (a0, sw, arc) = match self.arc_span(e) {
                    Some((a0, sw)) => (a0, sw, true),
                    None => (0.0, TAU, false),
                };
                let rel = |p: DVec2| (ang(cc, p) - a0).rem_euclid(TAU);
                let tc = rel(click);
                let mut ts: Vec<f64> = hits.iter().map(|&p| rel(p)).filter(|t| if arc { *t > 1e-6 && *t < sw - 1e-6 } else { *t < TAU - 1e-9 }).collect();
                ts.sort_by(|x, y| x.partial_cmp(y).unwrap());
                ts.dedup_by(|x, y| (*x - *y).abs() < 1e-7);
                let at = |s: &mut Sketch, t: f64, r: f64| s.point_at(cc + DVec2::new((a0 + t).cos(), (a0 + t).sin()) * r);
                let r = match self.carrier(e) {
                    G::C(_, r) => r,
                    _ => unreachable!(),
                };
                self.drop_constraints_of(e);
                if !arc {
                    if ts.len() < 2 {
                        return false;
                    }
                    // убираем участок, содержащий щелчок (циклически)
                    let n = ts.len();
                    let i = ts.iter().rposition(|t| *t <= tc).unwrap_or(n - 1);
                    let (lo, hi) = (ts[i], ts[(i + 1) % n]);
                    let (pa, pb) = (at(self, hi, r), at(self, lo, r));
                    self.entities[e] = Entity::Arc { c, a: pa, b: pb, construction: cons };
                } else {
                    let Entity::Arc { a, b, .. } = self.entities[e] else { unreachable!() };
                    let lo = ts.iter().copied().filter(|t| *t < tc).last();
                    let hi = ts.iter().copied().find(|t| *t > tc);
                    match (lo, hi) {
                        (None, None) => {
                            self.remove_entities(&[e]);
                            return true;
                        }
                        (Some(l), Some(h)) => {
                            let (p1, p2) = (at(self, l, r), at(self, h, r));
                            self.entities[e] = Entity::Arc { c, a, b: p1, construction: cons };
                            self.add(Entity::Arc { c, a: p2, b, construction: cons });
                        }
                        (Some(l), None) => {
                            let p1 = at(self, l, r);
                            self.entities[e] = Entity::Arc { c, a, b: p1, construction: cons };
                        }
                        (None, Some(h)) => {
                            let p2 = at(self, h, r);
                            self.entities[e] = Entity::Arc { c, a: p2, b, construction: cons };
                        }
                    }
                }
            }
        }
        self.gc_points();
        true
    }

    /// Смещённая копия кривой на `dist` в ту сторону, где лежит `side`. Возвращает номер новой кривой.
    pub fn offset(&mut self, e: usize, side: P2, dist: f64) -> Option<usize> {
        let side = DVec2::from(side);
        let dist = dist.abs();
        if dist < 1e-9 {
            return None;
        }
        match self.entities[e] {
            Entity::Line { a, b, construction } => {
                let (pa, pb) = (self.p(a), self.p(b));
                let d = (pb - pa).try_normalize()?;
                let mut n = d.perp();
                if (side - pa).dot(n) < 0.0 {
                    n = -n;
                }
                let (na, nb) = (self.add_point((pa + n * dist).to_array()), self.add_point((pb + n * dist).to_array()));
                let id = self.add(Entity::Line { a: na, b: nb, construction });
                self.constraints.push(Constraint::Parallel(e, id));
                Some(id)
            }
            Entity::Circle { c, r, construction } => {
                let inside = self.p(c).distance(side) < r;
                let nr = if inside { r - dist } else { r + dist };
                if nr < 1e-6 {
                    return None;
                }
                let nc = self.add_point(self.points[c].pos);
                let id = self.add(Entity::Circle { c: nc, r: nr, construction });
                self.constraints.push(Constraint::Concentric(e, id));
                Some(id)
            }
            Entity::Arc { c, a, b, construction } => {
                let cc = self.p(c);
                let r = self.p(a).distance(cc);
                let nr = if cc.distance(side) < r { r - dist } else { r + dist };
                if nr < 1e-6 {
                    return None;
                }
                let k = nr / r;
                let nc = self.add_point(self.points[c].pos);
                let na = self.add_point((cc + (self.p(a) - cc) * k).to_array());
                let nb = self.add_point((cc + (self.p(b) - cc) * k).to_array());
                let id = self.add(Entity::Arc { c: nc, a: na, b: nb, construction });
                self.constraints.push(Constraint::Concentric(e, id));
                Some(id)
            }
        }
    }

    /// Скруглить угол в точке `p`, где сходятся ровно две линии.
    pub fn fillet_corner(&mut self, p: usize, r: f64) -> Result<(), &'static str> {
        let lines: Vec<usize> = (0..self.entities.len())
            .filter(|&i| matches!(self.entities[i], Entity::Line { a, b, .. } if a == p || b == p))
            .collect();
        if lines.len() != 2 || r <= 1e-9 {
            return Err("corner");
        }
        let corner = self.p(p);
        let other = |s: &Sketch, l: usize| match s.entities[l] {
            Entity::Line { a, b, .. } => if a == p { s.p(b) } else { s.p(a) },
            _ => unreachable!(),
        };
        let (q1, q2) = (other(self, lines[0]), other(self, lines[1]));
        let (u1, u2) = ((q1 - corner).normalize(), (q2 - corner).normalize());
        let cosv = u1.dot(u2).clamp(-1.0, 1.0);
        let theta = cosv.acos();
        if theta < 1e-3 || theta > std::f64::consts::PI - 1e-3 {
            return Err("parallel");
        }
        let t = r / (theta / 2.0).tan();
        if t >= corner.distance(q1) - 1e-9 || t >= corner.distance(q2) - 1e-9 {
            return Err("big");
        }
        let (t1, t2) = (corner + u1 * t, corner + u2 * t);
        let center = corner + (u1 + u2).normalize() * (r / (theta / 2.0).sin());
        let (n1, n2) = (self.add_point(t1.to_array()), self.add_point(t2.to_array()));
        let nc = self.add_point(center.to_array());
        for (l, np) in [(lines[0], n1), (lines[1], n2)] {
            self.constraints.retain(|c| !(c.entities().contains(&l) && !matches!(c, Constraint::Horizontal(_) | Constraint::Vertical(_))));
            if let Entity::Line { a, b, .. } = &mut self.entities[l] {
                if *a == p {
                    *a = np
                } else {
                    *b = np
                }
            }
        }
        let (a, b) = if (t1 - center).perp_dot(t2 - center) > 0.0 { (n1, n2) } else { (n2, n1) };
        let arc = self.add(Entity::Arc { c: nc, a, b, construction: false });
        // касание дуги с обеими линиями
        self.constraints.push(Constraint::Tangent(lines[0], arc));
        self.constraints.push(Constraint::Tangent(lines[1], arc));
        self.gc_points();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Params;

    #[test]
    fn trim_line_between_hits() {
        let mut s = Sketch::default();
        s.add_line([0.0, 5.0], [10.0, 5.0]);
        s.add_line([3.0, 0.0], [3.0, 10.0]);
        s.add_line([7.0, 0.0], [7.0, 10.0]);
        assert!(s.trim(0, [5.0, 5.0]));
        let horiz = s.entities.iter().filter(|e| matches!(e, Entity::Line { a, b, .. } if (s.p(*a).y - 5.0).abs() < 1e-9 && (s.p(*b).y - 5.0).abs() < 1e-9)).count();
        assert_eq!(horiz, 2); // остались два хвоста
        assert_eq!(s.entities.len(), 4);
    }

    #[test]
    fn trim_circle_makes_arc() {
        let mut s = Sketch::default();
        s.add_circle([0.0, 0.0], 5.0);
        s.add_line([-10.0, 0.0], [10.0, 0.0]);
        assert!(s.trim(0, [0.0, 5.0]));
        assert!(matches!(s.entities[0], Entity::Arc { .. }));
        let t = s.tessellate(0);
        assert!(t.iter().all(|p| p[1] <= 1e-6)); // осталась нижняя половина
    }

    #[test]
    fn fillet_rect_corner() {
        let mut s = Sketch::default();
        let l = s.add_rect([0.0, 0.0], [10.0, 10.0]);
        let p = match s.entities[l[0]] {
            Entity::Line { b, .. } => b,
            _ => unreachable!(),
        };
        s.fillet_corner(p, 2.0).unwrap();
        let params = Params::default();
        assert!(s.solve(&params, &[]).is_ok());
        assert_eq!(s.regions().len(), 1);
        let area = s.regions()[0].area();
        assert!((area - (100.0 - (4.0 - std::f64::consts::PI))).abs() < 0.1, "{area}");
    }

    #[test]
    fn offset_line_and_circle() {
        let mut s = Sketch::default();
        s.add_line([0.0, 0.0], [10.0, 0.0]);
        let id = s.offset(0, [5.0, 3.0], 2.0).unwrap();
        if let Entity::Line { a, .. } = s.entities[id] {
            assert!((s.p(a).y - 2.0).abs() < 1e-9);
        }
        s.add_circle([0.0, 0.0], 5.0);
        let c = s.offset(id + 1, [0.0, 0.0], 1.0).unwrap();
        assert!((s.radius(c).unwrap() - 4.0).abs() < 1e-9);
    }
}
