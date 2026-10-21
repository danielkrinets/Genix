//! A cube with two adjacent top edges filleted (the case that works in Fusion): `cargo run -p fusion-core --example fillet_demo -- out.genix [edges]`.
//! `edges` is how many top edges to round: 1, 2 (adjacent) or 4.
use fusion_core::doc::{Document, EdgeRef, FeatureKind, Shape};
use fusion_core::{Evaluator, Expr, Op};

fn main() {
    let n = Expr::num;
    let mut d = Document::default();
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(40.0), h: n(40.0) }, at: [n(0.0), n(0.0), n(0.0)], op: Op::NewBody });
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    let body = ev.model().bodies[0].id;
    let all = [
        EdgeRef::line([0.0, 0.0, 40.0], [40.0, 0.0, 40.0]),
        EdgeRef::line([40.0, 0.0, 40.0], [40.0, 40.0, 40.0]),
        EdgeRef::line([40.0, 40.0, 40.0], [0.0, 40.0, 40.0]),
        EdgeRef::line([0.0, 40.0, 40.0], [0.0, 0.0, 40.0]),
    ];
    if std::env::args().nth(2).as_deref() == Some("vround") {
        // only the four vertical edges rounded: the starting point for filleting the top rim as one tangent chain
        let vert: Vec<EdgeRef> = [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)].iter().map(|(x, y)| EdgeRef::line([*x, *y, 0.0], [*x, *y, 40.0])).collect();
        d.insert(FeatureKind::Fillet { body, edges: vert, radius: n(5.0) });
        if let Some(p) = std::env::args().nth(1) {
            std::fs::write(&p, d.to_json()).unwrap();
        }
        return;
    }
    let count: usize = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut edges = all[..count.min(4)].to_vec();
    if count == 12 {
        // every edge of the cube
        let c = |x: f64, y: f64, z: f64| [x, y, z];
        edges = vec![];
        for z in [0.0, 40.0] {
            edges.push(EdgeRef::line(c(0.0, 0.0, z), c(40.0, 0.0, z)));
            edges.push(EdgeRef::line(c(40.0, 0.0, z), c(40.0, 40.0, z)));
            edges.push(EdgeRef::line(c(40.0, 40.0, z), c(0.0, 40.0, z)));
            edges.push(EdgeRef::line(c(0.0, 40.0, z), c(0.0, 0.0, z)));
        }
        for (x, y) in [(0.0, 0.0), (40.0, 0.0), (40.0, 40.0), (0.0, 40.0)] {
            edges.push(EdgeRef::line(c(x, y, 0.0), c(x, y, 40.0)));
        }
    }
    d.insert(FeatureKind::Fillet { body, edges, radius: n(5.0) });
    // optional second step: round the four vertical edges that now end at the rounded top
    if std::env::args().nth(3).as_deref() == Some("verticals") {
        let h = 35.0;
        let vert = [
            EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, h]),
            EdgeRef::line([40.0, 0.0, 0.0], [40.0, 0.0, h]),
            EdgeRef::line([40.0, 40.0, 0.0], [40.0, 40.0, h]),
            EdgeRef::line([0.0, 40.0, 0.0], [0.0, 40.0, h]),
        ];
        let mut ev = Evaluator::default();
        ev.evaluate(&d);
        let body = ev.model().bodies[0].id;
        d.insert(FeatureKind::Fillet { body, edges: vert.to_vec(), radius: n(5.0) });
    }
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    for (i, st) in ev.steps().iter().enumerate() {
        println!("step {i}: {:.0} ms", st.millis);
    }
    println!("errors: {:?}", ev.steps().iter().filter_map(|s| s.error.clone()).collect::<Vec<_>>());
    {
        let m = ev.model();
        let mesh = m.bodies[0].mesh();
        let (lo, hi) = mesh.bounds().unwrap();
        println!("bounds {lo:?} {hi:?}");
        let mut slivers = 0;
        for t in &mesh.triangles {
            let p = mesh.tri(t);
            let area = (p[1] - p[0]).cross(p[2] - p[0]).length() / 2.0;
            let longest = (p[1] - p[0]).length().max((p[2] - p[1]).length()).max((p[0] - p[2]).length());
            if longest > 0.5 && area < 0.02 * longest {
                slivers += 1;
            }
        }
        println!("thin triangles: {slivers} of {}", mesh.triangles.len());
    }
    println!("volume {:.1} (cube 64000)", ev.model().bodies[0].solid.volume());
    if let Some(p) = std::env::args().nth(1) {
        std::fs::write(&p, d.to_json()).unwrap();
    }
}
