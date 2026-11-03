//! A wine glass (revolved profile) with a bent-over drinking straw, built through the same feature list the app uses.
//! Usage: `cargo run -p fusion-core --example wine_glass -- out.genix`.
use fusion_core::csg::BoolOp;
use fusion_core::doc::{Document, FeatureKind, RevolveAxis, Shape};
use fusion_core::sketch::{Entity, ProfileRef, Sketch};
use fusion_core::{Evaluator, Expr, Op, Plane};

fn polygon(points: &[[f64; 2]]) -> Sketch {
    let mut s = Sketch::default();
    let ids: Vec<usize> = points.iter().map(|&p| s.add_point(p)).collect();
    for i in 0..ids.len() {
        s.add(Entity::Line { a: ids[i], b: ids[(i + 1) % ids.len()], construction: false });
    }
    s
}

/// Outer radius of the tulip-shaped bowl at height fraction `t` (0 = stem, 1 = rim).
fn bowl(t: f64) -> f64 {
    37.0 * (t * 1.9).sin() / 1.9f64.sin()
}

fn main() {
    let n = Expr::num;
    let mut d = Document::default();
    // profile (x = radius, y = height): foot, stem, bowl outside up to the rim, bowl inside back down, closed along the axis
    let (z0, z1, wall) = (70.0, 150.0, 2.0);
    let mut p: Vec<[f64; 2]> = vec![[0.0, 0.0], [32.0, 0.0], [32.0, 1.5], [14.0, 3.0], [3.0, 6.0], [2.5, 12.0], [2.5, z0]];
    for i in 1..=16 {
        let t = i as f64 / 16.0;
        p.push([bowl(t).max(2.5), z0 + (z1 - z0) * t]);
    }
    p.push([bowl(1.0) - wall, z1]);
    for i in (0..=16).rev() {
        let t = i as f64 / 16.0;
        p.push([(bowl(t) - wall).max(0.0), z0 + 4.0 + (z1 - z0 - 4.0) * t]);
    }
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::YZ, sketch: polygon(&p) });
    d.insert(FeatureKind::Revolve { sketch: sk, profiles: vec![ProfileRef::at([30.0, 0.7])], axis: RevolveAxis::SketchY, angle: n(360.0), op: Op::NewBody });
    // straw: a hollow tube (outer cylinder minus inner cylinder), resting on the bowl bottom, leaning against the rim side and rising above it
    d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(3.0), h: n(150.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
    d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(2.2), h: n(152.0) }, at: [n(0.0), n(0.0), n(-1.0)], op: Op::NewBody });
    let ids: Vec<u64> = {
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        ev.model().bodies.iter().map(|b| b.id).collect()
    };
    d.insert(FeatureKind::Combine { target: ids[1], tools: vec![ids[2]], op: BoolOp::Subtract, keep_tools: false });
    d.insert(FeatureKind::Move { bodies: vec![ids[1]], translate: [n(16.1), n(0.0), n(82.8)], rotate: [n(0.0), n(14.0), n(0.0)], copy: false });
    d.body_materials.insert(ids[0], 7); // glass
    d.body_materials.insert(ids[1], 4); // the straw: plastic
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    for (i, step) in ev.steps().iter().enumerate() {
        assert!(step.error.is_none(), "step {i}: {:?}", step.error);
    }
    for b in &ev.model().bodies {
        println!("{}: {:.0} mm3, bounds {:?}", b.name, b.solid.volume(), b.solid.mesh.bounds());
    }
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(&path, d.to_json()).expect("write the .genix file");
        println!("written {path}");
    }
}
