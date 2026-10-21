//! A 90 x 60 x 28 box with k of its 4 vertical edges rounded (radius rv), then the whole top rim rounded as one chain (radius rr).
//! `cargo run -p fusion-core --example fillet_rim -- [rv] [rr]`.
use fusion_core::doc::{Document, EdgeRef, FeatureKind, Shape};
use fusion_core::{Evaluator, Expr, Op};

fn main() {
    let n = Expr::num;
    let rv: f64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(8.0);
    let rr: f64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(3.0);
    let (w, dp, h) = (90.0, 60.0, 28.0);
    let corners = [(0.0, 0.0), (w, 0.0), (w, dp), (0.0, dp)];
    for k in 1..=4usize {
        let mut d = Document::default();
        d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(w), d: n(dp), h: n(h) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let body = ev.model().bodies[0].id;
        let vertical: Vec<EdgeRef> = corners[..k].iter().map(|(x, y)| EdgeRef::line([*x, *y, 0.0], [*x, *y, h])).collect();
        d.insert(FeatureKind::Fillet { body, edges: vertical, radius: n(rv) });
        // rim: for each side, a straight edge (shortened by the rounded corners at its ends) and an arc at each rounded corner
        let rounded = |i: usize| i < k;
        let mut rim = Vec::new();
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            let (dx, dy) = ((b.0 - a.0) / ((b.0 - a.0).abs() + (b.1 - a.1).abs()), (b.1 - a.1) / ((b.0 - a.0).abs() + (b.1 - a.1).abs()));
            let start = if rounded(i) { rv } else { 0.0 };
            let end = if rounded((i + 1) % 4) { rv } else { 0.0 };
            let len = (b.0 - a.0).abs() + (b.1 - a.1).abs();
            rim.push(EdgeRef::line([a.0 + dx * start, a.1 + dy * start, h], [b.0 - dx * end, b.1 - dy * end, h]));
            if rounded((i + 1) % 4) {
                // arc around corner (i+1): from the end of this side to the start of the next
                let c = corners[(i + 1) % 4];
                let nxt = corners[(i + 2) % 4];
                let (ex, ey) = ((nxt.0 - c.0).signum() * (c.0 != nxt.0) as i32 as f64, (nxt.1 - c.1).signum() * (c.1 != nxt.1) as i32 as f64);
                let centre = [c.0 - dx * rv + ex * rv, c.1 - dy * rv + ey * rv, h];
                let mut arc = EdgeRef::round([b.0 - dx * rv, b.1 - dy * rv, h], centre, [0.0, 0.0, 1.0]);
                let cross = dx * ey - dy * ex;
                if cross < 0.0 { arc.axis = [0.0, 0.0, -1.0]; }
                arc.sweep = Some(90.0);
                rim.push(arc);
            }
            let _ = len;
        }
        d.insert(FeatureKind::Fillet { body, edges: rim, radius: n(rr) });
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let err: Vec<_> = ev.steps().iter().filter_map(|s| s.error.clone()).collect();
        println!("{k} rounded corners: volume {:.0}, rim fillet {:.0} ms, errors {err:?}", ev.model().bodies[0].solid.volume(), ev.steps()[2].millis);
        std::fs::write(format!("/tmp/rim_{k}.genix"), d.to_json()).unwrap();
    }
}
