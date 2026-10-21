//! Box 90 x 60 x 28 with 1..4 vertical edges rounded: where does it break? `cargo run -p fusion-core --example fillet_n`.
use fusion_core::doc::{Document, EdgeRef, FeatureKind, Shape};
use fusion_core::{Evaluator, Expr, Op};

fn main() {
    let n = Expr::num;
    for r in [2.0, 8.0] {
        for count in 1..=4 {
            let mut d = Document::default();
            d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(90.0), d: n(60.0), h: n(28.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
            let mut ev = Evaluator::default();
            ev.evaluate(&d);
            let body = ev.model().bodies[0].id;
            let corners = [(0.0, 0.0), (90.0, 0.0), (90.0, 60.0), (0.0, 60.0)];
            let edges: Vec<EdgeRef> = corners[..count].iter().map(|(x, y)| EdgeRef::line([*x, *y, 0.0], [*x, *y, 28.0])).collect();
            let _ = edges;
            let top: Vec<EdgeRef> = (0..count).map(|i| { let (a, b) = (corners[i], corners[(i + 1) % 4]); EdgeRef::line([a.0, a.1, 28.0], [b.0, b.1, 28.0]) }).collect();
            let edges = top;
            d.insert(FeatureKind::Fillet { body, edges, radius: n(r) });
            if r == 8.0 && count >= 3 { std::fs::write(format!("/tmp/fn_{count}.genix"), d.to_json()).unwrap(); }
            let mut ev = Evaluator::default();
            ev.evaluate(&d);
            let err: Vec<_> = ev.steps().iter().filter_map(|s| s.error.clone()).collect();
            println!("r={r} edges={count}: volume {:.0} (box 151200), ms {:.0}, errors {err:?}", ev.model().bodies[0].solid.volume(), ev.steps()[1].millis);
        }
    }
}
