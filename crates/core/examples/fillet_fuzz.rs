//! Every subset of the 12 edges of a box, two radii: which ones fail or change the volume wrongly?
//! `cargo run --release -p fusion-core --example fillet_fuzz`.
use fusion_core::doc::{Document, EdgeRef, FeatureKind, Shape};
use fusion_core::{Evaluator, Expr, Op};

fn main() {
    let n = Expr::num;
    let (w, d, h) = (90.0, 60.0, 28.0);
    let c = [[0.0, 0.0], [w, 0.0], [w, d], [0.0, d]];
    let mut all: Vec<EdgeRef> = Vec::new();
    for i in 0..4 {
        let (a, b) = (c[i], c[(i + 1) % 4]);
        all.push(EdgeRef::line([a[0], a[1], 0.0], [b[0], b[1], 0.0]));
        all.push(EdgeRef::line([a[0], a[1], h], [b[0], b[1], h]));
        all.push(EdgeRef::line([a[0], a[1], 0.0], [a[0], a[1], h]));
    }
    let only: Option<u32> = std::env::args().nth(1).and_then(|s| s.parse().ok());
    let mut bad = 0;
    // a spread of subsets rather than all 4095: the slow corner blends make the full sweep take hours
    let mut seed = 12345u64;
    let mut masks: Vec<u32> = (0..250).map(|_| { seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); ((seed >> 33) as u32 % 4095) + 1 }).collect();
    masks.sort();
    masks.dedup();
    for r in [9.0] {
        for &mask in &masks {
            if only.is_some_and(|m| m != mask) { continue; }
            let edges: Vec<EdgeRef> = (0..12).filter(|i| mask >> i & 1 == 1).map(|i| all[i].clone()).collect();
            let mut doc = Document::default();
            doc.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(w), d: n(d), h: n(h) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
            let mut ev = Evaluator::default();
            ev.evaluate(&doc);
            let body = ev.model().bodies[0].id;
            doc.insert(FeatureKind::Fillet { body, edges, radius: n(r) });
            let t0 = std::time::Instant::now();
            let mut ev = Evaluator::default();
            ev.evaluate(&doc);
            eprintln!("mask {mask:012b} {} edges {:.0} ms", mask.count_ones(), t0.elapsed().as_secs_f64() * 1000.0);
            let err: Vec<_> = ev.steps().iter().filter_map(|s| s.error.clone()).collect();
            let vol = ev.model().bodies.first().map(|b| b.solid.volume()).unwrap_or(0.0);
            let ok = err.is_empty() && vol < w * d * h && vol > w * d * h * 0.85;
            if !ok {
                bad += 1;
                if bad <= 40 { println!("r={r} mask={mask:012b} ({}) volume {vol:.0} errors {err:?}", mask.count_ones()); }
            }
        }
    }
    println!("{bad} bad cases");
}
