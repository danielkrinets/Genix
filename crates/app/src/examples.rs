//! Готовые примеры, чтобы сразу посмотреть возможности.

use fusion_core::doc::{Extent, RevolveAxis};
use fusion_core::sketch::{region_at, Constraint, Entity, ProfileRef};
use fusion_core::{Document, Expr, FeatureKind, Op, Plane, Sketch};

/// Ссылка на профиль чертежа, содержащий точку (по кривым границы — как при выборе мышью).
fn profile(s: &Sketch, seed: [f64; 2]) -> ProfileRef {
    let r = s.regions();
    ProfileRef::of(&r[region_at(&r, seed).expect("точка внутри профиля")])
}

/// Кронштейн: пластина с параметрической шириной, четыре отверстия, стойка и вырез.
pub fn bracket() -> Document {
    let mut d = Document::default();
    d.params.set("ширина", "80");
    d.params.set("глубина", "50");
    d.params.set("толщина", "8");
    d.params.set("отверстие", "8");

    // основание: угол закреплён в нуле, размеры из параметров
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [80.0, 50.0]);
    let corner = match s.entities[l[0]] {
        Entity::Line { a, .. } => a,
        _ => unreachable!(),
    };
    s.constraints.push(Constraint::Fix(corner, [0.0, 0.0]));
    s.constraints.push(Constraint::Length(l[0], Expr::from("ширина")));
    s.constraints.push(Constraint::Length(l[1], Expr::from("глубина")));
    // отверстия привязаны к краям: при изменении ширины/глубины они остаются в 10 мм от кромки
    for (x, y, ex, ey) in [(10.0, 10.0, "10", "10"), (70.0, 10.0, "ширина-10", "10"), (10.0, 40.0, "10", "глубина-10"), (70.0, 40.0, "ширина-10", "глубина-10")] {
        let c = s.add_circle([x, y], 4.0);
        let center = match s.entities[c] {
            Entity::Circle { c, .. } => c,
            _ => unreachable!(),
        };
        s.constraints.push(Constraint::Diameter(c, Expr::from("отверстие")));
        s.constraints.push(Constraint::HDistance(corner, center, Expr::from(ex)));
        s.constraints.push(Constraint::VDistance(corner, center, Expr::from(ey)));
    }
    let p1 = profile(&s, [40.0, 25.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![p1], extent: Extent::OneSide(Expr::from("толщина")), op: Op::NewBody });

    // стойка по центру задней кромки (чертёж во фронтальной плоскости, выдавливание симметрично)
    let mut s2 = Sketch::default();
    // вспомогательная линия от начала координат — опора для размеров
    let o = s2.add_point([0.0, 0.0]);
    let r = s2.add_rect([25.0, 8.0], [55.0, 48.0]);
    let rc = match s2.entities[r[0]] {
        Entity::Line { a, .. } => a,
        _ => unreachable!(),
    };
    s2.add(Entity::Line { a: o, b: rc, construction: true });
    s2.constraints.push(Constraint::Fix(o, [0.0, 0.0]));
    s2.constraints.push(Constraint::HDistance(o, rc, Expr::from("ширина/2-15")));
    s2.constraints.push(Constraint::VDistance(o, rc, Expr::from("толщина")));
    s2.constraints.push(Constraint::Length(r[0], Expr::num(30.0)));
    s2.constraints.push(Constraint::Length(r[1], Expr::num(40.0)));
    let c = s2.add_circle([40.0, 34.0], 6.0);
    let cc = match s2.entities[c] {
        Entity::Circle { c, .. } => c,
        _ => unreachable!(),
    };
    s2.constraints.push(Constraint::Diameter(c, Expr::num(12.0)));
    s2.constraints.push(Constraint::HDistance(o, cc, Expr::from("ширина/2")));
    s2.constraints.push(Constraint::VDistance(o, cc, Expr::from("толщина+26")));
    let mut plane = Plane::XZ;
    plane.origin = [0.0, 45.0, 0.0];
    let p2 = profile(&s2, [27.0, 10.0]);
    let sk2 = d.insert(FeatureKind::Sketch { plane, sketch: s2 });
    d.insert(FeatureKind::Extrude { sketch: sk2, profiles: vec![p2], extent: Extent::Symmetric(Expr::num(10.0)), op: Op::Join });
    d.visible_sketches.clear();
    d
}

/// Тело вращения: втулка из профиля в плоскости XZ.
pub fn bushing() -> Document {
    let mut d = Document::default();
    let mut s = Sketch::default();
    let pts = [[6.0, 0.0], [20.0, 0.0], [20.0, 6.0], [12.0, 6.0], [12.0, 30.0], [6.0, 30.0]].map(|p| s.add_point(p));
    for i in 0..pts.len() {
        s.add(Entity::Line { a: pts[i], b: pts[(i + 1) % pts.len()], construction: false });
    }
    let p = profile(&s, [9.0, 10.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: s });
    d.insert(FeatureKind::Revolve { sketch: sk, profiles: vec![p], axis: RevolveAxis::SketchY, angle: Expr::num(360.0), op: Op::NewBody });
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::Evaluator;

    #[test]
    fn bracket_follows_width_parameter() {
        let mut d = bracket();
        d.params.set("ширина", "120");
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        assert!(ev.steps().iter().all(|s| s.error.is_none()), "{:?}", ev.steps().iter().map(|s| s.error.clone()).collect::<Vec<_>>());
        let sk = ev.sketch(d.features[0].uid).unwrap();
        // правые отверстия ушли вместе с кромкой
        let max_x = sk.sketch.points.iter().map(|p| p.pos[0]).fold(f64::MIN, f64::max);
        assert!((max_x - 120.0).abs() < 1e-6);
        let centers: Vec<f64> = sk.sketch.entities.iter().filter_map(|e| if let Entity::Circle { c, .. } = e { Some(sk.sketch.points[*c].pos[0]) } else { None }).collect();
        assert!(centers.iter().any(|x| (x - 110.0).abs() < 1e-6), "{centers:?}");
        assert_eq!(sk.sketch.free_dof(&d.params), 0, "чертёж основания полностью определён");
    }

    #[test]
    fn examples_build_without_errors() {
        for d in [bracket(), bushing()] {
            let mut ev = Evaluator::default();
            ev.evaluate(&d);
            for (i, s) in ev.steps().iter().enumerate() {
                assert!(s.error.is_none(), "шаг {i}: {:?}", s.error);
            }
            assert_eq!(ev.model().bodies.len(), 1);
        }
    }
}
