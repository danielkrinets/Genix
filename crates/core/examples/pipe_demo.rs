//! A bent square pipe with rounded bends, the file used to check pipe editing.
//! Usage: `cargo run -p fusion-core --example pipe_demo -- out.genix`.
use fusion_core::doc::{Document, FeatureKind, PipeShape};
use fusion_core::sketch::Sketch;
use fusion_core::{Evaluator, Expr, Op, Plane};

fn main() {
    let mut d = Document::default();
    let mut path = Sketch::default();
    path.add_line([0.0, 0.0], [60.0, 0.0]);
    path.add_line([60.0, 0.0], [60.0, 40.0]);
    path.add_line([60.0, 40.0], [100.0, 40.0]);
    let ps = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: path });
    d.visible_sketches.insert(ps);
    d.insert(FeatureKind::Pipe { path: ps, outer: Expr::num(10.0), wall: Expr::num(1.5), op: Op::NewBody, shape: PipeShape::Square, corner: Expr::num(2.0), bend: Expr::num(10.0) });
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    println!("volume {:.0} mm3, errors: {:?}", ev.model().bodies[0].solid.volume(), ev.steps().iter().filter_map(|s| s.error.clone()).collect::<Vec<_>>());
    if let Some(p) = std::env::args().nth(1) {
        std::fs::write(&p, d.to_json()).unwrap();
        println!("written {p}");
    }
}
