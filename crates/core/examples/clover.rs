//! A sketch of three overlapping circles, the case where every overlap and crescent must be its own profile.
//! Usage: `cargo run -p fusion-core --example clover -- out.genix`.
use fusion_core::doc::{Document, FeatureKind};
use fusion_core::sketch::Sketch;
use fusion_core::Plane;

fn main() {
    let mut s = Sketch::default();
    s.add_circle([50.0, 40.0], 14.0);
    s.add_circle([61.0, 40.0], 14.0);
    s.add_circle([55.5, 49.0], 14.0);
    println!("{} regions", s.regions().len());
    let mut d = Document::default();
    let uid = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.visible_sketches.insert(uid);
    if let Some(p) = std::env::args().nth(1) {
        std::fs::write(p, d.to_json()).unwrap();
    }
}
