//! A 40 mm cube, all 12 edges, growing radius: where does the fillet stop working? `cargo run --release -p fusion-core --example fillet_cube`.
use fusion_core::doc::{Document, EdgeRef, FeatureKind, Shape};
use fusion_core::{Evaluator, Expr, Op};

fn main() {
    let n = Expr::num;
    let s = 40.0;
    let c = [[0.0, 0.0], [s, 0.0], [s, s], [0.0, s]];
    let mut all: Vec<EdgeRef> = Vec::new();
    for i in 0..4 {
        let (a, b) = (c[i], c[(i + 1) % 4]);
        all.push(EdgeRef::line([a[0], a[1], 0.0], [b[0], b[1], 0.0]));
        all.push(EdgeRef::line([a[0], a[1], s], [b[0], b[1], s]));
        all.push(EdgeRef::line([a[0], a[1], 0.0], [a[0], a[1], s]));
    }
    for r in [1.0, 2.0, 5.0, 10.0, 19.0, 20.0] {
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(s), d: n(s), h: n(s) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let body = ev.model().bodies[0].id;
        d.insert(FeatureKind::Fillet { body, edges: all.clone(), radius: n(r) });
        let t = std::time::Instant::now();
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let err: Vec<_> = ev.steps().iter().filter_map(|st| st.error.clone()).collect();
        let vol = ev.model().bodies.first().map(|b| b.solid.volume()).unwrap_or(0.0);
        // a cube with every edge rounded: volume = s^3 - (12 edges x (1 - pi/4) r^2 (s - 2r)) - 8 corners x (r^3 (1 - pi/6)) ... approximately
        let rr = r.min(s / 2.0);
        let exact = s.powi(3) - 12.0 * (1.0 - std::f64::consts::FRAC_PI_4) * rr * rr * (s - 2.0 * rr) - 8.0 * rr.powi(3) * (1.0 - std::f64::consts::FRAC_PI_6 * 1.0) * (1.0 - 0.0) + 0.0;
        println!("r={r}: volume {vol:.0} (rough {exact:.0}), {:.0} ms, errors {err:?}", t.elapsed().as_secs_f64() * 1000.0);
    }
}
