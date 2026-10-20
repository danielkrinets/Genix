//! Builds a practical model through the same feature list the app uses: a desk stand for an iPad mini 6
//! (195.4 × 134.8 × 6.3 mm). Usage: `cargo run -p fusion-core --example ipad_stand -- out.genix`.
use fusion_core::doc::{Document, Extent, FeatureKind, Shape};
use fusion_core::sketch::{Entity, ProfileRef, Sketch};
use fusion_core::{Evaluator, Expr, Op, Plane};

/// Closed polygon of lines that share their corner points.
fn polygon(points: &[[f64; 2]]) -> Sketch {
    let mut s = Sketch::default();
    let ids: Vec<usize> = points.iter().map(|&p| s.add_point(p)).collect();
    for i in 0..ids.len() {
        s.add(Entity::Line { a: ids[i], b: ids[(i + 1) % ids.len()], construction: false });
    }
    s
}

fn main() {
    let mut d = Document::default();
    let num = Expr::num;
    // 1-2. base plate with a front lip: the tablet's bottom edge stands in the groove behind the lip
    let base = d.insert(FeatureKind::Sketch { plane: Plane::YZ, sketch: polygon(&[[0.0, 0.0], [80.0, 0.0], [80.0, 8.0], [10.0, 8.0], [10.0, 22.0], [0.0, 22.0]]) });
    d.insert(FeatureKind::Extrude { sketch: base, profiles: vec![ProfileRef::at([40.0, 4.0])], extent: Extent::Symmetric(num(120.0)), op: Op::NewBody, face: None });
    // 3-4. back rest leaning about 76° from the desk; the 12 mm groove between lip and rest holds the tablet's bottom edge
    let rest = d.insert(FeatureKind::Sketch { plane: Plane::YZ, sketch: polygon(&[[22.0, 6.0], [34.0, 6.0], [58.0, 105.0], [46.0, 105.0]]) });
    d.insert(FeatureKind::Extrude { sketch: rest, profiles: vec![ProfileRef::at([38.0, 50.0])], extent: Extent::Symmetric(num(120.0)), op: Op::Join, face: None });
    // 5. cable slot through the rest
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: num(44.0), d: num(60.0), h: num(40.0) }, at: [num(-22.0), num(18.0), num(38.0)], op: Op::Cut });
    // 6-9. four recesses for rubber feet underneath
    for (x, y) in [(-40.0, 14.0), (40.0, 14.0), (-40.0, 66.0), (40.0, 66.0)] {
        d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: num(7.0), h: num(2.0) }, at: [num(x), num(y), num(-0.01)], op: Op::Cut });
    }
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    for (i, step) in ev.steps().iter().enumerate() {
        assert!(step.error.is_none(), "step {i}: {:?}", step.error);
    }
    let model = ev.model();
    assert_eq!(model.bodies.len(), 1, "the stand must be one solid body");
    let volume = model.bodies[0].solid.volume();
    println!("steps: {}, volume: {:.0} mm3", d.features.len(), volume);
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(&path, d.to_json()).expect("write the .genix file");
        println!("written {path}");
    }
}
