//! Real modelling scenarios, feature by feature and in combination: every step must build, bodies must be closed,
//! and volumes must match what the geometry says. A failing scenario is a bug in the modeller.
use fusion_core::csg::BoolOp;
use fusion_core::doc::*;
use fusion_core::sketch::ProfileRef;
use fusion_core::{Document, Entity, Evaluator, Expr, FeatureKind, Op, Plane, Sketch};
use std::collections::HashMap;

fn n(v: f64) -> Expr {
    Expr::num(v)
}
fn p3(a: f64, b: f64, c: f64) -> [Expr; 3] {
    [n(a), n(b), n(c)]
}
fn run(d: &Document) -> Evaluator {
    let mut ev = Evaluator::default();
    ev.evaluate(d);
    ev
}
fn errors(ev: &Evaluator, d: &Document) -> Vec<String> {
    d.features.iter().filter_map(|f| ev.error(f.uid).map(|e| format!("{}: {e}", f.kind.type_label()))).collect()
}
/// Every undirected edge of the mesh is shared by exactly two triangles (after welding equal points).
fn closed(b: &Body) -> bool {
    let m = b.mesh();
    let key = |v: [f32; 3]| ((v[0] as f64 * 1e4).round() as i64, (v[1] as f64 * 1e4).round() as i64, (v[2] as f64 * 1e4).round() as i64);
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut idx = Vec::new();
    for v in &m.vertices {
        let k = key([v[0] as f32, v[1] as f32, v[2] as f32]);
        let next = ids.len();
        idx.push(*ids.entry(k).or_insert(next));
    }
    let mut edges: HashMap<(usize, usize), i32> = HashMap::new();
    for t in &m.triangles {
        let t = [idx[t[0] as usize], idx[t[1] as usize], idx[t[2] as usize]];
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            continue;
        }
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *edges.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    edges.values().all(|c| *c == 2)
}
fn volume(ev: &Evaluator) -> f64 {
    ev.model().bodies.iter().map(|b| b.solid.volume()).sum()
}
fn check(name: &str, d: &Document, bodies: usize, vol: Option<(f64, f64)>) -> Evaluator {
    let ev = run(d);
    let errs = errors(&ev, d);
    assert!(errs.is_empty(), "{name}: step errors {errs:?}");
    let m = ev.model();
    assert_eq!(m.bodies.len(), bodies, "{name}: body count");
    for b in &m.bodies {
        assert!(closed(b), "{name}: body {} is not a closed surface", b.id);
        assert!(b.solid.volume() > 0.0, "{name}: body {} has no volume", b.id);
    }
    if let Some((want, tol)) = vol {
        let v = volume(&ev);
        assert!((v - want).abs() <= tol, "{name}: volume {v} vs {want} (±{tol})");
    }
    ev
}
fn boxp(d: &mut Document, at: [f64; 3], w: f64, dd: f64, h: f64, op: Op) {
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(w), d: n(dd), h: n(h) }, at: p3(at[0], at[1], at[2]), op });
}
fn cylp(d: &mut Document, at: [f64; 3], r: f64, h: f64, op: Op) {
    d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(r), h: n(h) }, at: p3(at[0], at[1], at[2]), op });
}
fn ext(sketch: u64, at: [f64; 2], dist: f64, op: Op) -> FeatureKind {
    FeatureKind::Extrude { sketch, profiles: vec![ProfileRef::at(at)], extent: Extent::OneSide(n(dist)), op, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) }
}
fn top_edges_box(w: f64, dd: f64, z: f64) -> Vec<EdgeRef> {
    vec![
        EdgeRef::line([0.0, 0.0, z], [w, 0.0, z]),
        EdgeRef::line([w, 0.0, z], [w, dd, z]),
        EdgeRef::line([w, dd, z], [0.0, dd, z]),
        EdgeRef::line([0.0, dd, z], [0.0, 0.0, z]),
    ]
}
const PI: f64 = std::f64::consts::PI;

#[test]
fn boolean_primitives_and_holes() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    cylp(&mut d, [20.0, 15.0, -1.0], 5.0, 22.0, Op::Cut);
    check("box minus through cylinder", &d, 1, Some((40.0 * 30.0 * 20.0 - PI * 25.0 * 20.0, 12.0)));
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    d.insert(FeatureKind::Primitive { shape: Shape::Sphere { r: n(12.0) }, at: p3(20.0, 15.0, 20.0), op: Op::Cut });
    check("box minus sphere dome", &d, 1, Some((24000.0 - 0.5 * 4.0 / 3.0 * PI * 1728.0, 80.0)));
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    boxp(&mut d, [30.0, 10.0, 5.0], 30.0, 10.0, 10.0, Op::Join);
    check("overlapping union", &d, 1, Some((24000.0 + 20.0 * 10.0 * 10.0 - 0.0, 1.0)));
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    boxp(&mut d, [30.0, 10.0, 5.0], 30.0, 10.0, 10.0, Op::Intersect);
    check("intersect", &d, 1, Some((10.0 * 10.0 * 10.0, 1.0)));
    let mut d = Document::default();
    d.insert(FeatureKind::Primitive { shape: Shape::Torus { major: n(20.0), minor: n(5.0) }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    check("torus", &d, 1, Some((2.0 * PI * PI * 20.0 * 25.0, 150.0)));
}

#[test]
fn sketch_extrude_variants() {
    let sketch_rect = |w: f64, h: f64| {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [w, h]);
        s
    };
    // plain, symmetric, two sides, taper, thin, start offset
    let mut d = Document::default();
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: sketch_rect(40.0, 30.0) });
    d.insert(ext(sk, [5.0, 5.0], 10.0, Op::NewBody));
    check("extrude", &d, 1, Some((12000.0, 1.0)));
    for (name, extent, vol) in [
        ("symmetric", Extent::Symmetric(n(10.0)), 12000.0),
        ("two sides", Extent::TwoSides(n(6.0), n(4.0)), 12000.0),
    ] {
        let mut d = Document::default();
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: sketch_rect(40.0, 30.0) });
        d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([5.0, 5.0])], extent, op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
        check(name, &d, 1, Some((vol, 1.0)));
    }
    let mut d = Document::default();
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: sketch_rect(40.0, 30.0) });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([5.0, 5.0])], extent: Extent::OneSide(n(20.0)), op: Op::NewBody, face: None, taper: n(5.0), thin: n(0.0), start: n(0.0) });
    let ev = check("taper", &d, 1, None);
    assert!(volume(&ev) < 24000.0 && volume(&ev) > 15000.0, "tapered volume {}", volume(&ev));
    let mut d = Document::default();
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: sketch_rect(40.0, 30.0) });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([5.0, 5.0])], extent: Extent::OneSide(n(10.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(2.0), start: n(0.0) });
    check("thin wall", &d, 1, Some(((40.0 * 30.0 - 36.0 * 26.0) * 10.0, 2.0)));
    // holes in a profile (a circle inside a rectangle)
    let mut d = Document::default();
    let mut s = sketch_rect(40.0, 30.0);
    s.add_circle([20.0, 15.0], 6.0);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(ext(sk, [2.0, 2.0], 10.0, Op::NewBody));
    check("profile with a hole", &d, 1, Some(((1200.0 - PI * 36.0) * 10.0, 15.0)));
    // extrude on another plane and cut
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let mut s = Sketch::default();
    s.add_circle([15.0, 10.0], 4.0);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: s });
    d.insert(ext(sk, [15.0, 10.0], -40.0, Op::Cut));
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
}

#[test]
fn revolve_sweep_loft_pipe_coil() {
    // revolve a rectangle about the sketch Y axis: a ring
    let mut d = Document::default();
    let mut s = Sketch::default();
    s.add_rect([10.0, 0.0], [20.0, 15.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: s });
    d.insert(FeatureKind::Revolve { sketch: sk, profiles: vec![ProfileRef::at([15.0, 5.0])], axis: RevolveAxis::SketchY, angle: n(360.0), op: Op::NewBody });
    check("revolve ring", &d, 1, Some((PI * (400.0 - 100.0) * 15.0, 40.0)));
    // half revolve
    let mut d2 = d.clone();
    if let FeatureKind::Revolve { angle, .. } = &mut d2.features[1].kind {
        *angle = n(180.0);
    }
    check("half revolve", &d2, 1, Some((PI * 300.0 * 15.0 / 2.0, 40.0)));
    // loft two circles on offset planes
    let mut d = Document::default();
    let mut a = Sketch::default();
    a.add_circle([0.0, 0.0], 10.0);
    let sa = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: a });
    let mut b = Sketch::default();
    b.add_circle([0.0, 0.0], 5.0);
    let up = Plane { origin: [0.0, 0.0, 30.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let sb = d.insert(FeatureKind::Sketch { plane: up, sketch: b });
    d.insert(FeatureKind::Loft { profiles: vec![(sa, ProfileRef::at([1.0, 1.0])), (sb, ProfileRef::at([1.0, 1.0]))], op: Op::NewBody });
    let cone = PI * 30.0 / 3.0 * (100.0 + 25.0 + 50.0);
    check("loft cone frustum", &d, 1, Some((cone, cone * 0.08)));
    // pipe along an L path
    let mut d = Document::default();
    let mut s = Sketch::default();
    s.add_line([0.0, 0.0], [40.0, 0.0]);
    s.add_line([40.0, 0.0], [40.0, 30.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(FeatureKind::Pipe { path: sk, outer: n(8.0), wall: n(1.0), op: Op::NewBody, shape: PipeShape::Round, corner: n(0.0), bend: n(0.0) });
    let ev = check("pipe", &d, 1, None);
    let approx = PI * (16.0 - 9.0) * 70.0;
    assert!((volume(&ev) - approx).abs() < approx * 0.1, "pipe volume {} vs ~{approx}", volume(&ev));
    // sweep a circle along a path
    let mut d = Document::default();
    let mut path = Sketch::default();
    path.add_line([0.0, 0.0], [0.0, 50.0]);
    let sp = d.insert(FeatureKind::Sketch { plane: Plane::YZ, sketch: path });
    let mut prof = Sketch::default();
    prof.add_circle([0.0, 0.0], 4.0);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: prof });
    d.insert(FeatureKind::Sweep { sketch: sk, profiles: vec![ProfileRef::at([1.0, 1.0])], path: sp, op: Op::NewBody });
    check("sweep", &d, 1, Some((PI * 16.0 * 50.0, 60.0)));
    // coil
    let mut d = Document::default();
    d.insert(FeatureKind::Coil { at: p3(0.0, 0.0, 0.0), axis: Axis3::Z, dia: n(20.0), pitch: n(6.0), turns: n(5.0), section: n(2.0), op: Op::NewBody });
    check("coil", &d, 1, None);
}

#[test]
fn edge_treatments_and_shell() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let ev = run(&d);
    let body = ev.model().bodies[0].id;
    let mut f = d.clone();
    f.insert(FeatureKind::Fillet { body, edges: top_edges_box(40.0, 30.0, 20.0), radius: n(3.0) });
    check("fillet top", &f, 1, Some((24000.0 - (1.0 - PI / 4.0) * 9.0 * 140.0, 40.0)));
    let mut c = d.clone();
    c.insert(FeatureKind::Chamfer { body, edges: top_edges_box(40.0, 30.0, 20.0), dist: n(3.0), second: ChamferSecond::Equal, flip: false });
    // the remaining part is a frustum slice: h/3 * (A1 + A2 + sqrt(A1 A2)) with A1 = 40x30 and A2 = 34x24
    let frustum = 3.0 / 3.0 * (1200.0 + 816.0 + (1200.0f64 * 816.0).sqrt());
    check("chamfer top", &c, 1, Some((24000.0 - (3600.0 - frustum), 5.0)));
    let mut c2 = d.clone();
    c2.insert(FeatureKind::Chamfer { body, edges: top_edges_box(40.0, 30.0, 20.0), dist: n(3.0), second: ChamferSecond::Distance(n(2.0)), flip: false });
    check("asymmetric chamfer", &c2, 1, Some((24000.0 - (3.0 * 1200.0 - 3.0 / 3.0 * (1200.0 + 936.0 + (1200.0f64 * 936.0).sqrt())), 8.0)));
    // vertical edge fillet
    let mut v = d.clone();
    v.insert(FeatureKind::Fillet { body, edges: vec![EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, 20.0])], radius: n(4.0) });
    check("vertical edge fillet", &v, 1, Some((24000.0 - (1.0 - PI / 4.0) * 16.0 * 20.0, 15.0)));
    // shell with the top open
    let mut s = d.clone();
    let top = Plane { origin: [0.0, 0.0, 20.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    s.insert(FeatureKind::Shell { plane: top, thickness: n(2.0), open: true, more: vec![] });
    check("shell", &s, 1, Some((24000.0 - 36.0 * 26.0 * 18.0, 30.0)));
    // fillet then shell (rounded box, hollowed)
    let mut fs = f.clone();
    fs.insert(FeatureKind::Shell { plane: top, thickness: n(2.0), open: true, more: vec![] });
    let ev = run(&fs);
    let errs = errors(&ev, &fs);
    assert!(errs.is_empty(), "fillet then shell: {errs:?}");
    // fillet a round edge of a cylinder
    let mut cyl = Document::default();
    cylp(&mut cyl, [0.0; 3], 15.0, 20.0, Op::NewBody);
    let body = run(&cyl).model().bodies[0].id;
    cyl.insert(FeatureKind::Fillet { body, edges: vec![EdgeRef::round([15.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0])], radius: n(3.0) });
    check("cylinder rim fillet", &cyl, 1, Some((PI * 225.0 * 20.0 - (1.0 - PI / 4.0) * 9.0 * 2.0 * PI * 13.0, 60.0)));
    // oversized fillet is a clear error, not a corrupt body
    let mut big = d.clone();
    big.insert(FeatureKind::Fillet { body, edges: top_edges_box(40.0, 30.0, 20.0), radius: n(500.0) });
    let ev = run(&big);
    assert!(ev.error(big.features[1].uid).is_some() || ev.model().bodies.iter().all(closed), "oversized fillet must fail clearly");
}

#[test]
fn holes_all_kinds() {
    let top = Plane { origin: [0.0, 0.0, 20.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let base = |d: &mut Document| boxp(d, [0.0; 3], 60.0, 40.0, 20.0, Op::NewBody);
    let mut d = Document::default();
    base(&mut d);
    d.insert(FeatureKind::Hole { plane: top, at: [n(20.0), n(20.0)], dia: n(8.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    check("simple through hole", &d, 1, Some((48000.0 - PI * 16.0 * 20.0, 20.0)));
    let mut d = Document::default();
    base(&mut d);
    d.insert(FeatureKind::Hole { plane: top, at: [n(20.0), n(20.0)], dia: n(8.0), depth: Some(n(10.0)), kind: HoleKind::Simple, more: vec![] });
    check("blind hole", &d, 1, Some((48000.0 - PI * 16.0 * 10.0, 20.0)));
    let mut d = Document::default();
    base(&mut d);
    d.insert(FeatureKind::Hole { plane: top, at: [n(20.0), n(20.0)], dia: n(6.0), depth: None, kind: HoleKind::Counterbore { dia: n(12.0), depth: n(5.0) }, more: vec![] });
    check("counterbore", &d, 1, Some((48000.0 - PI * 9.0 * 15.0 - PI * 36.0 * 5.0, 30.0)));
    let mut d = Document::default();
    base(&mut d);
    d.insert(FeatureKind::Hole { plane: top, at: [n(20.0), n(20.0)], dia: n(6.0), depth: None, kind: HoleKind::Countersink { dia: n(12.0), angle: n(90.0) }, more: vec![] });
    check("countersink", &d, 1, None);
    let mut d = Document::default();
    base(&mut d);
    d.insert(FeatureKind::Hole { plane: top, at: [n(20.0), n(20.0)], dia: n(5.0), depth: Some(n(12.0)), kind: HoleKind::Tapped { major: n(6.0), pitch: n(1.0), modeled: false }, more: vec![] });
    check("tapped cosmetic", &d, 1, None);
    let mut d = Document::default();
    base(&mut d);
    d.insert(FeatureKind::Hole { plane: top, at: [n(15.0), n(20.0)], dia: n(4.0), depth: None, kind: HoleKind::Simple, more: vec![[n(30.0), n(20.0)], [n(45.0), n(20.0)]] });
    check("three holes at once", &d, 1, Some((48000.0 - 3.0 * PI * 4.0 * 20.0, 30.0)));
    // hole on a side face
    let side = Plane { origin: [0.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };
    let mut d = Document::default();
    base(&mut d);
    d.insert(FeatureKind::Hole { plane: Plane { origin: [60.0, 0.0, 0.0], ..side }, at: [n(20.0), n(10.0)], dia: n(6.0), depth: Some(n(15.0)), kind: HoleKind::Simple, more: vec![] });
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "side hole: {:?}", errors(&ev, &d));
}

#[test]
fn transforms_patterns_mirror_split_scale_draft() {
    let mut base = Document::default();
    boxp(&mut base, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = run(&base).model().bodies[0].id;
    let vol = 4000.0;
    let mut d = base.clone();
    d.insert(FeatureKind::Move { bodies: vec![id], translate: p3(50.0, 0.0, 0.0), rotate: p3(0.0, 0.0, 45.0), copy: true });
    check("move copy rotate", &d, 2, Some((2.0 * vol, 1.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Move { bodies: vec![id], translate: p3(5.0, 5.0, 5.0), rotate: p3(10.0, 20.0, 30.0), copy: false });
    check("move rotate", &d, 1, Some((vol, 2.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Mirror { bodies: vec![id], normal: Axis3::X, join: false, offset: n(30.0) });
    check("mirror copy", &d, 2, Some((2.0 * vol, 1.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Mirror { bodies: vec![id], normal: Axis3::X, join: true, offset: n(20.0) });
    check("mirror joined at the face", &d, 1, Some((2.0 * vol, 5.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Rect { nx: n(3.0), dx: n(30.0), ny: n(2.0), dy: n(30.0), nz: n(1.0), dz: n(0.0) } });
    check("rect pattern 3x2", &d, 6, Some((6.0 * vol, 2.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Move { bodies: vec![id], translate: p3(30.0, 0.0, 0.0), rotate: p3(0.0, 0.0, 0.0), copy: false });
    d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Circular { count: n(6.0), axis: Axis3::Z, angle: n(360.0) } });
    check("circular pattern", &d, 6, Some((6.0 * vol, 4.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Split { bodies: vec![id], axis: Axis3::Z, offset: n(4.0) });
    check("split", &d, 2, Some((vol, 1.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Scale { bodies: vec![id], factor: [n(2.0), n(1.0), n(0.5)], uniform: false });
    check("non-uniform scale", &d, 1, Some((vol, 1.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Scale { bodies: vec![id], factor: [n(2.0), n(1.0), n(1.0)], uniform: true });
    check("uniform scale", &d, 1, Some((vol * 8.0, 2.0)));
    let mut d = base.clone();
    d.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(5.0) });
    let ev = check("draft", &d, 1, None);
    assert!(volume(&ev) != vol, "draft changed nothing");
    // combine
    let mut d = base.clone();
    cylp(&mut d, [10.0, 10.0, -5.0], 4.0, 20.0, Op::NewBody);
    let ids: Vec<u64> = run(&d).model().bodies.iter().map(|b| b.id).collect();
    let mut sub = d.clone();
    sub.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[1]], op: BoolOp::Subtract, keep_tools: false });
    check("combine subtract", &sub, 1, Some((vol - PI * 16.0 * 10.0, 15.0)));
    let mut keep = d.clone();
    keep.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[1]], op: BoolOp::Subtract, keep_tools: true });
    check("combine subtract keeping the tool", &keep, 2, None);
    let mut uni = d.clone();
    uni.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[1]], op: BoolOp::Union, keep_tools: false });
    check("combine union", &uni, 1, None);
    let mut rem = d.clone();
    rem.insert(FeatureKind::Remove { bodies: vec![ids[1]] });
    check("remove body", &rem, 1, Some((vol, 1.0)));
}

#[test]
fn face_operations_after_other_features() {
    let top = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 10.0, Op::NewBody);
    for (dist, name) in [(8.0, "pull"), (-6.0, "push")] {
        let mut x = d.clone();
        x.insert(FeatureKind::PressPull { plane: top, seed: [20.0, 15.0, 10.0], distance: n(dist) });
        check(name, &x, 1, Some((12000.0 + 1200.0 * dist, 2.0)));
    }
    // pull a face of a body that has a hole in it
    let mut h = d.clone();
    h.insert(FeatureKind::Hole { plane: top, at: [n(20.0), n(15.0)], dia: n(6.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    h.insert(FeatureKind::PressPull { plane: top, seed: [5.0, 5.0, 10.0], distance: n(5.0) });
    let ev = run(&h);
    assert!(errors(&ev, &h).is_empty(), "press-pull around a hole: {:?}", errors(&ev, &h));
    // extrude a face of the body (face reference) up
    let mut f = d.clone();
    f.insert(FeatureKind::Extrude { sketch: 0, profiles: vec![], extent: Extent::OneSide(n(7.0)), op: Op::Join, face: Some(FaceRef { plane: top, seed: [20.0, 15.0, 10.0] }), taper: n(0.0), thin: n(0.0), start: n(0.0) });
    check("extrude a face", &f, 1, Some((12000.0 + 1200.0 * 7.0, 2.0)));
    // offset plane + sketch on it + extrude down to cut a pocket
    let mut p = d.clone();
    let pl = p.insert(FeatureKind::OffsetPlane { base: Plane::XY, offset: n(10.0) });
    let _ = pl;
    let plane_at = Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let mut s = Sketch::default();
    s.add_rect([10.0, 10.0], [30.0, 20.0]);
    let sk = p.insert(FeatureKind::Sketch { plane: plane_at, sketch: s });
    p.insert(ext(sk, [15.0, 15.0], -4.0, Op::Cut));
    check("pocket", &p, 1, Some((12000.0 - 20.0 * 10.0 * 4.0, 2.0)));
    // place on plane
    let mut pl = d.clone();
    pl.insert(FeatureKind::PlaceOnPlane { body: run(&d).model().bodies[0].id, face: top, seed: [20.0, 15.0, 10.0], target: Plane::XY, to: None });
    let ev = run(&pl);
    assert!(errors(&ev, &pl).is_empty(), "place on plane: {:?}", errors(&ev, &pl));
}

#[test]
fn a_realistic_part_end_to_end() {
    // a bracket: base plate, upright wall, fillets, two holes, a pocket, then a mirrored copy
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 80.0, 40.0, 6.0, Op::NewBody);
    boxp(&mut d, [0.0, 0.0, 6.0], 80.0, 6.0, 34.0, Op::Join);
    let base_top = Plane { origin: [0.0, 0.0, 6.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    d.insert(FeatureKind::Hole { plane: base_top, at: [n(15.0), n(25.0)], dia: n(6.0), depth: None, kind: HoleKind::Simple, more: vec![[n(65.0), n(25.0)]] });
    let wall_face = Plane { origin: [0.0, 6.0, 0.0], x: [1.0, 0.0, 0.0], y: [0.0, 0.0, 1.0] };
    d.insert(FeatureKind::Hole { plane: wall_face, at: [n(40.0), n(25.0)], dia: n(10.0), depth: None, kind: HoleKind::Counterbore { dia: n(16.0), depth: n(2.0) }, more: vec![] });
    let ev = run(&d);
    let errs = errors(&ev, &d);
    assert!(errs.is_empty(), "bracket: {errs:?}");
    let m = ev.model();
    assert_eq!(m.bodies.len(), 1, "bracket must be one body");
    assert!(closed(&m.bodies[0]), "bracket is not closed");
    // fillet the outer top edge of the wall and chamfer the plate edge
    let id = m.bodies[0].id;
    d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 40.0], [80.0, 0.0, 40.0])], radius: n(3.0) });
    d.insert(FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::line([0.0, 40.0, 6.0], [80.0, 40.0, 6.0])], dist: n(1.5), second: ChamferSecond::Equal, flip: false });
    let ev = run(&d);
    let errs = errors(&ev, &d);
    assert!(errs.is_empty(), "bracket fillet/chamfer: {errs:?}");
    assert!(ev.model().bodies.iter().all(closed), "bracket after edge work is not closed");
    d.insert(FeatureKind::Mirror { bodies: vec![id], normal: Axis3::Y, join: false, offset: n(-10.0) });
    check("bracket mirrored", &d, 2, None);
}

fn heart() -> Sketch {
    let mut s = Sketch::default();
    let tip = s.add_point([0.0, -25.0]);
    let (lc, rc) = (s.add_point([-8.0, 8.0]), s.add_point([8.0, 8.0]));
    let k = (std::f64::consts::FRAC_1_SQRT_2) * 10.0;
    let lp = s.add_point([-8.0 - k, 8.0 - k]);
    let rp = s.add_point([8.0 + k, 8.0 - k]);
    let cusp = s.add_point([0.0, 14.0]);
    s.add(Entity::Line { a: tip, b: lp, construction: false });
    s.add(Entity::Arc { c: lc, a: cusp, b: lp, construction: false });
    s.add(Entity::Arc { c: rc, a: rp, b: cusp, construction: false });
    s.add(Entity::Line { a: rp, b: tip, construction: false });
    s
}

#[test]
fn curved_outline_parts_like_the_heart() {
    let mut d = Document::default();
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: heart() });
    d.insert(ext(sk, [0.0, 0.0], 12.0, Op::NewBody));
    let ev = check("heart extrude", &d, 1, None);
    let area = volume(&ev) / 12.0;
    assert!(area > 800.0 && area < 1500.0, "heart area {area}");
    let top = Plane { origin: [0.0, 0.0, 12.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    for dist in [6.0, -5.0, -9.0] {
        let mut x = d.clone();
        x.insert(FeatureKind::PressPull { plane: top, seed: [0.0, 0.0, 12.0], distance: n(dist) });
        let ev2 = check(&format!("heart press-pull {dist}"), &x, 1, Some((area * (12.0 + dist), area * 0.02)));
        let (_, hi) = ev2.model().bodies[0].mesh().bounds().unwrap();
        assert!((hi.z - (12.0 + dist)).abs() < 1e-2, "heart {dist}: top at {}", hi.z);
    }
    // the same by extruding the face
    for dist in [6.0, -5.0] {
        let mut x = d.clone();
        x.insert(FeatureKind::Extrude { sketch: 0, profiles: vec![], extent: Extent::OneSide(n(dist)), op: Op::Join, face: Some(FaceRef { plane: top, seed: [0.0, 0.0, 12.0] }), taper: n(0.0), thin: n(0.0), start: n(0.0) });
        check(&format!("heart face extrude {dist}"), &x, 1, Some((area * (12.0 + dist), area * 0.02)));
    }
    // a thin-walled heart box, then a smaller heart cut out of a plate
    let mut t = Document::default();
    let sk = t.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: heart() });
    t.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([0.0, 0.0])], extent: Extent::OneSide(n(8.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(1.5), start: n(0.0) });
    check("heart thin wall", &t, 1, None);
    let mut plate = Document::default();
    boxp(&mut plate, [-40.0, -40.0, -5.0], 80.0, 80.0, 5.0, Op::NewBody);
    let sk = plate.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: heart() });
    plate.insert(ext(sk, [0.0, 0.0], -3.0, Op::Cut));
    check("heart pocket in plate", &plate, 1, None);
}

#[test]
fn edge_work_around_holes_and_chains() {
    let top = Plane { origin: [0.0, 0.0, 20.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 60.0, 40.0, 20.0, Op::NewBody);
    let id = run(&d).model().bodies[0].id;
    d.insert(FeatureKind::Hole { plane: top, at: [n(30.0), n(20.0)], dia: n(10.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    // round the rim of the hole
    let mut f = d.clone();
    f.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::round([35.0, 20.0, 20.0], [30.0, 20.0, 20.0], [0.0, 0.0, 1.0])], radius: n(1.5) });
    check("fillet a hole rim", &f, 1, None);
    let mut c = d.clone();
    c.insert(FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::round([35.0, 20.0, 20.0], [30.0, 20.0, 20.0], [0.0, 0.0, 1.0])], dist: n(1.0), second: ChamferSecond::Equal, flip: false });
    check("chamfer a hole rim", &c, 1, None);
    // four vertical edges, then the top
    let mut ch = d.clone();
    let verticals = vec![
        EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, 20.0]),
        EdgeRef::line([60.0, 0.0, 0.0], [60.0, 0.0, 20.0]),
        EdgeRef::line([60.0, 40.0, 0.0], [60.0, 40.0, 20.0]),
        EdgeRef::line([0.0, 40.0, 0.0], [0.0, 40.0, 20.0]),
    ];
    ch.insert(FeatureKind::Fillet { body: id, edges: verticals, radius: n(5.0) });
    check("four vertical fillets", &ch, 1, None);
    // shell with two open faces
    let mut sh = Document::default();
    boxp(&mut sh, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    sh.insert(FeatureKind::Shell { plane: top, thickness: n(2.0), open: true, more: vec![Plane { origin: [40.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] }] });
    let ev = run(&sh);
    assert!(errors(&ev, &sh).is_empty(), "shell with two openings: {:?}", errors(&ev, &sh));
    assert!(ev.model().bodies.iter().all(closed), "shell with two openings is not closed");
    // a holed body copied in a pattern and mirrored
    let mut pm = d.clone();
    pm.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Rect { nx: n(2.0), dx: n(70.0), ny: n(2.0), dy: n(50.0), nz: n(1.0), dz: n(0.0) } });
    check("pattern of holed bodies", &pm, 4, Some((4.0 * (48000.0 - PI * 25.0 * 20.0), 40.0)));
}

#[test]
fn parameters_history_and_rollback() {
    let mut d = Document::default();
    d.params.set("w", "40");
    d.params.set("h", "w / 2");
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: Expr::from("w"), d: n(30.0), h: Expr::from("h") }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    cylp(&mut d, [20.0, 15.0, -1.0], 5.0, 30.0, Op::Cut);
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    let v1 = volume(&ev);
    d.params.set("w", "60");
    let rebuilt = ev.evaluate(&d);
    assert!(rebuilt >= 1, "a changed parameter must rebuild");
    let v2 = volume(&ev);
    assert!(v2 > v1 * 1.4, "{v1} -> {v2}");
    assert!(errors(&ev, &d).is_empty());
    // roll back to before the cut: the model is the plain box
    d.rollback = 1;
    ev.evaluate(&d);
    assert!((volume(&ev) - 60.0 * 30.0 * 30.0).abs() < 1.0, "rolled back volume {}", volume(&ev));
    d.rollback = d.features.len();
    ev.evaluate(&d);
    assert!((volume(&ev) - v2).abs() < 1.0);
    // a broken expression is an error on that step, the steps before it still stand
    d.params.set("h", "w / 0");
    ev.evaluate(&d);
    assert!(!errors(&ev, &d).is_empty(), "division by zero must be reported");
    // suppress a feature
    d.params.set("h", "w / 2");
    d.features[1].suppressed = true;
    ev.evaluate(&d);
    assert!((volume(&ev) - 60.0 * 30.0 * 30.0).abs() < 1.0, "suppressed cut must not apply");
}

#[test]
fn json_round_trip_keeps_the_model() {
    let mut d = Document::default();
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: heart() });
    d.insert(ext(sk, [0.0, 0.0], 12.0, Op::NewBody));
    boxp(&mut d, [30.0, 0.0, 0.0], 10.0, 10.0, 10.0, Op::NewBody);
    let v = volume(&run(&d));
    let back = Document::from_json(&d.to_json()).expect("the saved document must load");
    assert!((volume(&run(&back)) - v).abs() < 1e-6, "volume changed after save and load");
}
