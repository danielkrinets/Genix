//! A sketch of circle crossing a rectangle (near-tangent to its side).
//! Usage: `cargo run -p fusion-core --example rect_circle -- out.genix`.
use fusion_core::doc::{Document, FeatureKind};
use fusion_core::sketch::Sketch;
use fusion_core::Plane;

fn main() {
    let mut s = Sketch::default();
    s.add_rect([-15.0, -12.0], [20.0, 12.0]);
    s.add_circle([0.0, 0.0], 20.0);
    println!("{} regions", s.regions().len());
    let mut d = Document::default();
    let uid = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.visible_sketches.insert(uid);
    if let Some(p) = std::env::args().nth(1) {
        std::fs::write(p, d.to_json()).unwrap();
    }
}
