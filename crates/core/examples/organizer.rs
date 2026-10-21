//! A desk pen organizer built from the features that were reworked recently: rounded corners and a rounded top rim
//! (the rim is picked as one tangent chain), three pen holes, and a pane of frosted glass in front.
//! Usage: `cargo run -p fusion-core --example organizer -- out.genix`.
use fusion_core::csg::BoolOp;
use fusion_core::doc::{Document, EdgeRef, FeatureKind, Shape};
use fusion_core::{Evaluator, Expr, Op};

fn main() {
    let n = Expr::num;
    let mut d = Document::default();
    let (w, depth, h, r_corner, r_rim) = (90.0, 60.0, 28.0, 8.0, 3.0);
    // 1. block
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(w), d: n(depth), h: n(h) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
    let body = Evaluator::default().tap(&d).model().bodies[0].id;
    // 2. round the four vertical corners
    let vertical: Vec<EdgeRef> = [(0.0, 0.0), (w, 0.0), (w, depth), (0.0, depth)].iter().map(|(x, y)| EdgeRef::line([*x, *y, 0.0], [*x, *y, h])).collect();
    d.insert(FeatureKind::Fillet { body, edges: vertical, radius: n(r_corner) });
    // 3. round the whole top rim: four straight edges and the four corner arcs form one tangent chain
    let arc = |cx: f64, cy: f64, sx: f64, sy: f64| {
        let mut e = EdgeRef::round([sx, sy, h], [cx, cy, h], [0.0, 0.0, 1.0]);
        e.sweep = Some(90.0);
        e
    };
    let rim = vec![
        EdgeRef::line([r_corner, 0.0, h], [w - r_corner, 0.0, h]),
        arc(w - r_corner, r_corner, w - r_corner, 0.0),
        EdgeRef::line([w, r_corner, h], [w, depth - r_corner, h]),
        arc(w - r_corner, depth - r_corner, w, depth - r_corner),
        EdgeRef::line([w - r_corner, depth, h], [r_corner, depth, h]),
        arc(r_corner, depth - r_corner, r_corner, depth),
        EdgeRef::line([0.0, depth - r_corner, h], [0.0, r_corner, h]),
        arc(r_corner, r_corner, 0.0, r_corner),
    ];
    d.insert(FeatureKind::Fillet { body, edges: rim, radius: n(r_rim) });
    // 4. three pen holes of different sizes
    let mut tools = Vec::new();
    for (x, radius) in [(22.0, 7.0), (45.0, 9.0), (68.0, 7.0)] {
        d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(radius), h: n(h) }, at: [n(x), n(depth / 2.0 + 2.0), n(6.0)], op: Op::NewBody });
        tools.push(Evaluator::default().tap(&d).model().bodies.last().unwrap().id);
    }
    d.insert(FeatureKind::Combine { target: body, tools, op: BoolOp::Subtract, keep_tools: false });
    // 5. a pane of frosted glass leaning in front of the block (material 7 = glass)
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(70.0), d: n(3.0), h: n(34.0) }, at: [n(10.0), n(-14.0), n(0.0)], op: Op::NewBody });
    let ev = Evaluator::default().tap(&d);
    let glass = ev.model().bodies.last().unwrap().id;
    d.body_materials.insert(glass, 7);
    let ev = Evaluator::default().tap(&d);
    let errors: Vec<_> = ev.steps().iter().enumerate().filter_map(|(i, s)| s.error.clone().map(|e| (i, e))).collect();
    println!("steps {}, bodies {}, errors {:?}", d.features.len(), ev.model().bodies.len(), errors);
    for (i, st) in ev.steps().iter().enumerate() {
        println!("step {i} {}: {:.0} ms", d.features[i].kind.type_label(), st.millis);
    }
    println!("organizer volume {:.0} mm3", ev.model().bodies[0].solid.volume());
    if let Some(p) = std::env::args().nth(1) {
        std::fs::write(&p, d.to_json()).unwrap();
        println!("written {p}");
    }
}

trait Tap {
    fn tap(self, d: &Document) -> Self;
}
impl Tap for Evaluator {
    fn tap(mut self, d: &Document) -> Self {
        self.evaluate(d);
        self
    }
}
