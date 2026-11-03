//! Demo model for the 0.00.27 modeling batch: tapered extrude, through-all cut, mirror about an offset plane,
//! split by a tool body and an angled construction plane. Usage: `cargo run -p fusion-core --example modeling_demo -- out.genix`.
use fusion_core::doc::{Axis3, Document, Extent, FeatureKind, Shape};
use fusion_core::sketch::{ProfileRef, Sketch};
use fusion_core::csg::BoolOp;
use fusion_core::{Evaluator, Expr, Op, Plane};

fn main() {
    let n = Expr::num;
    let mut d = Document::default();
    // 1. a 30 mm square extruded 25 mm with a 6 degree taper (a draft-like tapered post)
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [30.0, 30.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::OneSide(n(25.0)), op: Op::NewBody, face: None, taper: n(6.0), thin: n(0.0) });
    // 2. a hole cut through the whole post
    let mut h = Sketch::default();
    h.add_circle([15.0, 15.0], 5.0);
    let hs = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: h });
    d.insert(FeatureKind::Extrude { sketch: hs, profiles: vec![ProfileRef::at([15.0, 15.0])], extent: Extent::All(true), op: Op::Cut, face: None, taper: n(0.0), thin: Expr::num(0.0) });
    // 3. a mirrored copy about a plane 50 mm away
    let id = {
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        ev.model().bodies[0].id
    };
    d.insert(FeatureKind::Mirror { bodies: vec![id], normal: Axis3::X, join: false, offset: n(50.0) });
    // 4. a bar that crosses the first post, then split the post with it
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(8.0), h: n(10.0) }, at: [n(-5.0), n(11.0), n(8.0)], op: Op::NewBody });
    let ids: Vec<u64> = {
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        ev.model().bodies.iter().map(|b| b.id).collect()
    };
    d.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[2]], op: BoolOp::Split, keep_tools: false });
    // 5. a thin-walled box (thin extrude) and a 2 x 1 x 2 grid of small cubes (3D pattern)
    let mut t = Sketch::default();
    t.add_rect([0.0, 70.0], [30.0, 100.0]);
    let ts = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: t });
    d.insert(FeatureKind::Extrude { sketch: ts, profiles: vec![ProfileRef::at([1.0, 71.0])], extent: Extent::OneSide(n(20.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(2.0) });
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(6.0), d: n(6.0), h: n(6.0) }, at: [n(-60.0), n(0.0), n(0.0)], op: Op::NewBody });
    let cube = {
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        ev.model().bodies.last().unwrap().id
    };
    d.insert(FeatureKind::Pattern { bodies: vec![cube], kind: fusion_core::doc::PatternKind::Rect { nx: n(2.0), dx: n(12.0), ny: n(1.0), dy: n(0.0), nz: n(2.0), dz: n(12.0) } });
    d.insert(FeatureKind::AnglePlane { base: Plane::XY, angle: n(30.0) });
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    for (i, step) in ev.steps().iter().enumerate() {
        assert!(step.error.is_none(), "step {i}: {:?}", step.error);
    }
    println!("bodies: {}, interferences: {}", ev.model().bodies.len(), ev.model().interferences().len());
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(&path, d.to_json()).expect("write the .genix file");
        println!("written {path}");
    }
}
