//! QA sweep of the modelling engine: features stacked in many orders, boundary values, touching bodies,
//! parameters and history edits. Every failure must be a clean evaluator error (never a panic), every body a
//! closed surface, and volumes must match hand-computed values within a tolerance that allows for facets.
//! Tests marked `#[ignore = "BUG: ..."]` reproduce confirmed engine bugs.
use fusion_core::csg::BoolOp;
use fusion_core::doc::*;
use fusion_core::sketch::ProfileRef;
use fusion_core::{Document, Entity, Evaluator, Expr, FeatureKind, Op, Plane, Sketch};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};

const PI: f64 = std::f64::consts::PI;
/// Removed area of a 90-degree fillet of radius 1.
const FIL: f64 = 1.0 - PI / 4.0;
/// The message the evaluator uses when it caught a panic inside a step.
const INTERNAL: &str = "внутренняя ошибка";

fn n(v: f64) -> Expr {
    Expr::num(v)
}
fn p3(a: f64, b: f64, c: f64) -> [Expr; 3] {
    [n(a), n(b), n(c)]
}

/// Evaluates with a panic guard of our own: a panic escaping the evaluator fails the test with its message.
fn run(d: &Document) -> Evaluator {
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut ev = Evaluator::default();
        ev.evaluate(d);
        ev
    }));
    match r {
        Ok(ev) => ev,
        Err(e) => {
            let msg = e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            panic!("evaluator panicked: {msg}");
        }
    }
}
fn errors(ev: &Evaluator, d: &Document) -> Vec<String> {
    d.features.iter().filter_map(|f| ev.error(f.uid).map(|e| format!("{}: {e}", f.kind.type_label()))).collect()
}
/// No step may hide a panic behind the generic "internal error" message.
fn no_internal(name: &str, ev: &Evaluator, d: &Document) {
    for e in errors(ev, d) {
        assert!(!e.contains(INTERNAL), "{name}: a step panicked inside the evaluator: {e}");
    }
}
fn err_of(ev: &Evaluator, uid: u64) -> Option<String> {
    ev.error(uid).map(str::to_string)
}
/// Every undirected edge is shared by exactly two triangles (after welding equal points).
fn closed(b: &Body) -> bool {
    let m = b.mesh();
    let key = |v: [f64; 3]| ((v[0] * 1e4).round() as i64, (v[1] * 1e4).round() as i64, (v[2] * 1e4).round() as i64);
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let mut idx = Vec::new();
    for v in &m.vertices {
        let next = ids.len();
        idx.push(*ids.entry(key(*v)).or_insert(next));
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
    !edges.is_empty() && edges.values().all(|c| *c == 2)
}
fn volume(ev: &Evaluator) -> f64 {
    ev.model().bodies.iter().map(|b| b.solid.volume()).sum()
}
fn body_volumes(ev: &Evaluator) -> Vec<f64> {
    ev.model().bodies.iter().map(|b| b.solid.volume()).collect()
}
/// Bounds of all bodies together: (lo, hi).
fn bounds(ev: &Evaluator) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for b in &ev.model().bodies {
        if let Some((l, h)) = b.mesh().bounds() {
            for (i, (lv, hv)) in [(l.x, h.x), (l.y, h.y), (l.z, h.z)].into_iter().enumerate() {
                lo[i] = lo[i].min(lv);
                hi[i] = hi[i].max(hv);
            }
        }
    }
    (lo, hi)
}
fn near3(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= tol)
}
/// Builds without errors, with `bodies` closed bodies and (optionally) the given total volume.
fn check(name: &str, d: &Document, bodies: usize, vol: Option<(f64, f64)>) -> Evaluator {
    let ev = run(d);
    let errs = errors(&ev, d);
    assert!(errs.is_empty(), "{name}: step errors {errs:?}");
    let m = ev.model();
    assert_eq!(m.bodies.len(), bodies, "{name}: body count (volumes {:?})", body_volumes(&ev));
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
/// The step must fail with a clean message (not a panic) and leave the model of the step before it.
fn expect_error(name: &str, d: &Document, uid: u64) -> String {
    let ev = run(d);
    let e = err_of(&ev, uid).unwrap_or_else(|| panic!("{name}: expected an error, got none (volume {})", volume(&ev)));
    assert!(!e.contains(INTERNAL), "{name}: the step panicked instead of reporting a clean error: {e}");
    for b in &ev.model().bodies {
        assert!(closed(b), "{name}: the model after the failed step is not closed");
    }
    e
}
/// Saving and loading keeps the document and the model.
fn round_trip(name: &str, d: &Document) {
    let back = Document::from_json(&d.to_json()).unwrap_or_else(|e| panic!("{name}: saved document does not load: {e}"));
    assert_eq!(&back, d, "{name}: document changed after save and load");
    let (a, b) = (run(d), run(&back));
    assert_eq!(a.model().bodies.len(), b.model().bodies.len(), "{name}: body count changed after save and load");
    assert!((volume(&a) - volume(&b)).abs() < 1e-6, "{name}: volume changed after save and load");
    assert_eq!(errors(&a, d), errors(&b, &back), "{name}: errors changed after save and load");
}
fn boxp(d: &mut Document, at: [f64; 3], w: f64, dd: f64, h: f64, op: Op) -> u64 {
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(w), d: n(dd), h: n(h) }, at: p3(at[0], at[1], at[2]), op })
}
fn cylp(d: &mut Document, at: [f64; 3], r: f64, h: f64, op: Op) -> u64 {
    d.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: n(r), h: n(h) }, at: p3(at[0], at[1], at[2]), op })
}
fn ext_kind(sketch: u64, at: [f64; 2], extent: Extent, op: Op) -> FeatureKind {
    FeatureKind::Extrude { sketch, profiles: vec![ProfileRef::at(at)], extent, op, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) }
}
fn ext(sketch: u64, at: [f64; 2], dist: f64, op: Op) -> FeatureKind {
    ext_kind(sketch, at, Extent::OneSide(n(dist)), op)
}
fn rect_sketch(d: &mut Document, plane: Plane, p0: [f64; 2], p1: [f64; 2]) -> u64 {
    let mut s = Sketch::default();
    s.add_rect(p0, p1);
    d.insert(FeatureKind::Sketch { plane, sketch: s })
}
fn top_edges_box(x0: f64, y0: f64, w: f64, dd: f64, z: f64) -> Vec<EdgeRef> {
    let (x1, y1) = (x0 + w, y0 + dd);
    vec![
        EdgeRef::line([x0, y0, z], [x1, y0, z]),
        EdgeRef::line([x1, y0, z], [x1, y1, z]),
        EdgeRef::line([x1, y1, z], [x0, y1, z]),
        EdgeRef::line([x0, y1, z], [x0, y0, z]),
    ]
}
fn vertical_edges_box(w: f64, dd: f64, h: f64) -> Vec<EdgeRef> {
    vec![
        EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, h]),
        EdgeRef::line([w, 0.0, 0.0], [w, 0.0, h]),
        EdgeRef::line([w, dd, 0.0], [w, dd, h]),
        EdgeRef::line([0.0, dd, 0.0], [0.0, dd, h]),
    ]
}
/// A face plane facing +Z whose origin is the point the user clicked on that face (what Shell expects).
fn click(p: [f64; 3]) -> Plane {
    Plane { origin: p, x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] }
}
fn zplane(z: f64) -> Plane {
    Plane { origin: [0.0, 0.0, z], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] }
}
/// Volume of `b` inside a 0.2 mm cube centred on `p`.
fn probe_volume(b: &Body, p: [f64; 3]) -> f64 {
    let h = 0.1;
    let (lo, hi) = ([p[0] - h, p[1] - h, p[2] - h], [p[0] + h, p[1] + h, p[2] + h]);
    let m = fusion_core::Mesh {
        vertices: vec![[lo[0], lo[1], lo[2]], [hi[0], lo[1], lo[2]], [hi[0], hi[1], lo[2]], [lo[0], hi[1], lo[2]], [lo[0], lo[1], hi[2]], [hi[0], lo[1], hi[2]], [hi[0], hi[1], hi[2]], [lo[0], hi[1], hi[2]]],
        triangles: vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]],
    };
    let cube = fusion_core::csg::Solid::from_mesh(&m).expect("probe cube");
    b.solid.boolean(&cube, BoolOp::Intersect).volume()
}
fn inside(b: &Body, p: [f64; 3]) -> bool {
    probe_volume(b, p) > 0.004
}
fn first_body(d: &Document) -> u64 {
    run(d).model().bodies[0].id
}
fn face_extrude(plane: Plane, seed: [f64; 3], dist: f64) -> FeatureKind {
    FeatureKind::Extrude { sketch: 0, profiles: vec![], extent: Extent::OneSide(n(dist)), op: Op::Join, face: Some(FaceRef { plane, seed }), taper: n(0.0), thin: n(0.0), start: n(0.0) }
}

// ------------------------------------------------------------------------------------------------ feature orders

#[test]
fn fillet_then_shell_rounded_box_volume() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Fillet { body: id, edges: vertical_edges_box(40.0, 30.0, 20.0), radius: n(5.0) });
    let outer = 24000.0 - (4.0 - PI) * 25.0 * 20.0;
    check("rounded box", &d, 1, Some((outer, 20.0)));
    d.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(2.0), open: true, more: vec![] });
    // the cavity is the rounded outline offset by 2 (corner radius 3), 18 deep
    let cavity = (36.0 * 26.0 - (4.0 - PI) * 9.0) * 18.0;
    check("fillet then shell", &d, 1, Some((outer - cavity, 0.015 * (outer - cavity))));
    round_trip("fillet then shell", &d);
}

#[test]
fn shell_then_fillet_outer_and_inner_edges() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(2.0), open: true, more: vec![] });
    let shelled = 24000.0 - 36.0 * 26.0 * 18.0;
    check("shell", &d, 1, Some((shelled, 30.0)));
    let mut outer = d.clone();
    outer.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, 20.0])], radius: n(1.5) });
    check("shell then outer vertical fillet", &outer, 1, Some((shelled - FIL * 2.25 * 20.0, 30.0)));
    let mut inner = d.clone();
    // the inner vertical corner of the cavity is concave: rounding it adds material
    inner.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([2.0, 2.0, 2.0], [2.0, 2.0, 20.0])], radius: n(1.0) });
    let ev = check("shell then inner (concave) fillet", &inner, 1, None);
    let v = volume(&ev);
    assert!(v > shelled + 1.0 && v < shelled + 8.0, "concave fillet must add about {} mm3, got {}", FIL * 18.0, v - shelled);
    round_trip("shell then fillet", &outer);
}

#[test]
fn holes_then_fillet_top_edges() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(20.0), n(15.0)], dia: n(8.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    d.insert(FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), radius: n(3.0) });
    check("holes then fillet", &d, 1, Some((24000.0 - PI * 16.0 * 20.0 - FIL * 9.0 * 140.0, 45.0)));
    round_trip("holes then fillet", &d);
}

#[test]
fn fillet_the_rim_of_a_hole_after_a_top_fillet() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), radius: n(2.0) });
    d.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(20.0), n(15.0)], dia: n(10.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    let before = volume(&check("fillet then hole", &d, 1, None));
    d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::round([25.0, 15.0, 20.0], [20.0, 15.0, 20.0], [0.0, 0.0, 1.0])], radius: n(1.0) });
    // a rim fillet of a hole removes the corner ring: area FIL*r^2 revolved at radius ~ 5 + 0.22
    let ring = FIL * 1.0 * 2.0 * PI * (5.0 + 0.2234);
    check("hole rim fillet after top fillet", &d, 1, Some((before - ring, 3.0)));
    round_trip("hole rim fillet", &d);
}

#[test]
fn pattern_then_fillet_a_copy() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    let pat = d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Rect { nx: n(3.0), dx: n(30.0), ny: n(1.0), dy: n(0.0), nz: n(1.0), dz: n(0.0) } });
    let ev = check("pattern", &d, 3, Some((12000.0, 1.0)));
    let copy = ev.model().bodies[1].id;
    assert_eq!(copy, pat * 1000, "first copy id");
    d.insert(FeatureKind::Fillet { body: copy, edges: vec![EdgeRef::line([30.0, 0.0, 10.0], [50.0, 0.0, 10.0])], radius: n(2.0) });
    let ev = check("pattern then fillet copy", &d, 3, Some((12000.0 - FIL * 4.0 * 20.0, 1.5)));
    let vols = body_volumes(&ev);
    assert!((vols[0] - 4000.0).abs() < 1e-6 && (vols[2] - 4000.0).abs() < 1e-6, "only the copy is rounded: {vols:?}");
    round_trip("pattern then fillet", &d);
}

#[test]
fn mirror_then_combine_every_boolean() {
    let mut base = Document::default();
    boxp(&mut base, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&base);
    // the mirror plane x = 15 lies inside the body: the copy spans x 10..30
    let mir = base.insert(FeatureKind::Mirror { bodies: vec![id], normal: Axis3::X, join: false, offset: n(15.0) });
    check("mirror copy overlapping", &base, 2, Some((8000.0, 1.0)));
    let copy = mir * 1000;
    for (op, bodies, vol) in [(BoolOp::Union, 1, 6000.0), (BoolOp::Subtract, 1, 2000.0), (BoolOp::Intersect, 1, 2000.0), (BoolOp::Xor, 1, 4000.0), (BoolOp::Split, 2, 4000.0)] {
        let mut d = base.clone();
        d.insert(FeatureKind::Combine { target: id, tools: vec![copy], op, keep_tools: false });
        let name = format!("mirror then combine {op:?}");
        check(&name, &d, bodies, Some((vol, 1.0)));
        round_trip(&name, &d);
    }
}

#[test]
fn press_pull_after_fillet_chamfer_shell_and_holes() {
    let top = zplane(10.0);
    let mut base = Document::default();
    boxp(&mut base, [0.0; 3], 40.0, 30.0, 10.0, Op::NewBody);
    let id = first_body(&base);
    let mut variants: Vec<(&str, Document, [f64; 3], f64)> = Vec::new();
    let mut f = base.clone();
    f.insert(FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 10.0), radius: n(2.0) });
    variants.push(("fillet", f, [20.0, 15.0, 10.0], 1200.0));
    let mut c = base.clone();
    c.insert(FeatureKind::Chamfer { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 10.0), dist: n(2.0), second: ChamferSecond::Equal, flip: false });
    variants.push(("chamfer", c, [20.0, 15.0, 10.0], 1200.0));
    let mut s = base.clone();
    s.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 10.0]), thickness: n(2.0), open: true, more: vec![] });
    // the top of an open shell is the rim ring
    variants.push(("shell rim", s, [1.0, 1.0, 10.0], 1200.0 - 36.0 * 26.0));
    let mut h = base.clone();
    h.insert(FeatureKind::Hole { plane: top, at: [n(20.0), n(15.0)], dia: n(6.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    variants.push(("holed", h, [5.0, 5.0, 10.0], 1200.0 - PI * 9.0));
    for (name, d, seed, area) in variants {
        let v0 = volume(&check(name, &d, 1, None));
        for dist in [5.0, -4.0] {
            let mut x = d.clone();
            x.insert(FeatureKind::PressPull { plane: top, seed, distance: n(dist) });
            let label = format!("press-pull {dist} after {name}");
            let ev = check(&label, &x, 1, Some((v0 + area * dist, 0.01 * area * dist.abs() + 2.0)));
            let (_, hi) = bounds(&ev);
            assert!((hi[2] - (10.0 + dist)).abs() < 1e-3, "{label}: top at {}", hi[2]);
            let mut y = d.clone();
            y.insert(face_extrude(top, seed, dist));
            let label = format!("face extrude {dist} after {name}");
            check(&label, &y, 1, Some((v0 + area * dist, 0.01 * area * dist.abs() + 2.0)));
            round_trip(&label, &y);
        }
    }
}

#[test]
fn draft_then_fillet_a_leaning_edge() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(5.0) });
    let inset = 10.0 * 5f64.to_radians().tan();
    let a2 = (20.0 - 2.0 * inset).powi(2);
    let frustum = 10.0 / 3.0 * (400.0 + a2 + (400.0 * a2).sqrt());
    let v0 = volume(&check("draft", &d, 1, Some((frustum, 1.0))));
    // fillet the top edge of the drafted body (95 degree corner) and a bottom edge (85 degree corner)
    let mut t = d.clone();
    t.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([inset, inset, 10.0], [20.0 - inset, inset, 10.0])], radius: n(1.0) });
    let ev = check("draft then top fillet", &t, 1, None);
    let removed = v0 - volume(&ev);
    // r^2 (cot(theta/2) - (pi - theta)/2) for theta = 95 degrees over 18.25 mm
    let th = 95f64.to_radians();
    let want = (1.0 / (th / 2.0).tan() - (PI - th) / 2.0) * (20.0 - 2.0 * inset);
    assert!((removed - want).abs() < 0.6, "draft then top fillet removed {removed}, want about {want}");
    let mut b = d.clone();
    b.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 0.0], [20.0, 0.0, 0.0])], radius: n(1.0) });
    let ev = check("draft then bottom fillet", &b, 1, None);
    let th = 85f64.to_radians();
    let want = (1.0 / (th / 2.0).tan() - (PI - th) / 2.0) * 20.0;
    let removed = v0 - volume(&ev);
    assert!((removed - want).abs() < 0.8, "draft then bottom fillet removed {removed}, want about {want}");
    round_trip("draft then fillet", &b);
}

#[test]
fn split_then_fillet_each_half() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    let sp = d.insert(FeatureKind::Split { bodies: vec![id], axis: Axis3::Z, offset: n(4.0) });
    check("split", &d, 2, Some((4000.0, 1e-3)));
    let upper = sp * 1000;
    d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 4.0], [20.0, 0.0, 4.0])], radius: n(1.0) });
    d.insert(FeatureKind::Fillet { body: upper, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 0.0, 10.0])], radius: n(2.0) });
    check("split then fillet both halves", &d, 2, Some((4000.0 - FIL * 20.0 - FIL * 4.0 * 20.0, 1.0)));
    round_trip("split then fillet", &d);
}

#[test]
fn chamfer_then_shell_and_shell_closed() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    let mut closed_shell = d.clone();
    closed_shell.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(2.0), open: false, more: vec![] });
    check("closed shell (hollow)", &closed_shell, 1, Some((24000.0 - 36.0 * 26.0 * 16.0, 30.0)));
    d.insert(FeatureKind::Chamfer { body: id, edges: vertical_edges_box(40.0, 30.0, 20.0), dist: n(4.0), second: ChamferSecond::Equal, flip: false });
    let outer = 24000.0 - 4.0 * 8.0 * 20.0;
    check("chamfered verticals", &d, 1, Some((outer, 1.0)));
    d.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(2.0), open: true, more: vec![] });
    let ev = check("chamfer then shell", &d, 1, None);
    let v = volume(&ev);
    // the wall is 2 thick everywhere: about perimeter * 2 * 18 + floor
    assert!(v > 0.2 * outer && v < 0.35 * outer, "chamfer then shell volume {v}");
    round_trip("chamfer then shell", &d);
}

#[test]
fn pattern_of_filleted_body_then_mirror_join() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Fillet { body: id, edges: vertical_edges_box(20.0, 20.0, 10.0), radius: n(3.0) });
    let one = 4000.0 - (4.0 - PI) * 9.0 * 10.0;
    d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Circular { count: n(4.0), axis: Axis3::Z, angle: n(360.0) } });
    check("circular pattern of rounded box", &d, 4, Some((4.0 * one, 2.0)));
    // mirror the original about x = -40 (outside all bodies) and join: a separate lump in the same body
    d.insert(FeatureKind::Mirror { bodies: vec![id], normal: Axis3::X, join: true, offset: n(-40.0) });
    check("mirror join outside", &d, 4, Some((5.0 * one, 3.0)));
    round_trip("pattern then mirror", &d);
}

// ------------------------------------------------------------------------------------------------ boundary values

#[test]
fn extrude_distances_zero_negative_tiny_huge() {
    let mk = |dist: f64| {
        let mut d = Document::default();
        let sk = rect_sketch(&mut d, Plane::XY, [0.0, 0.0], [40.0, 30.0]);
        let uid = d.insert(ext(sk, [5.0, 5.0], dist, Op::NewBody));
        (d, uid)
    };
    let (d, uid) = mk(0.0);
    expect_error("extrude 0", &d, uid);
    let (d, _) = mk(-10.0);
    let ev = check("extrude -10", &d, 1, Some((12000.0, 1e-6)));
    let (lo, hi) = bounds(&ev);
    assert!(near3(lo, [0.0, 0.0, -10.0], 1e-9) && near3(hi, [40.0, 30.0, 0.0], 1e-9), "extrude -10 bounds {lo:?} {hi:?}");
    let (d, _) = mk(0.01);
    check("extrude 0.01", &d, 1, Some((12.0, 1e-6)));
    let (d, _) = mk(10000.0);
    check("extrude 10000", &d, 1, Some((1.2e7, 1e-3)));
    round_trip("extrude 10000", &d);
}

#[test]
fn extrude_options_taper_thin_start_symmetric() {
    let base = |extent: Extent, taper: f64, thin: f64, start: f64| {
        let mut d = Document::default();
        let sk = rect_sketch(&mut d, Plane::XY, [0.0, 0.0], [40.0, 30.0]);
        let uid = d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([5.0, 5.0])], extent, op: Op::NewBody, face: None, taper: n(taper), thin: n(thin), start: n(start) });
        (d, uid)
    };
    // taper: a frustum with the top inset by h*tan(a)
    let frustum = |h: f64, a: f64| {
        let i = h * a.to_radians().tan();
        let a2 = (40.0 - 2.0 * i) * (30.0 - 2.0 * i);
        h / 3.0 * (1200.0 + a2 + (1200.0 * a2).sqrt())
    };
    let (d, _) = base(Extent::OneSide(n(10.0)), 5.0, 0.0, 0.0);
    check("taper 5", &d, 1, Some((frustum(10.0, 5.0), 1.0)));
    let (d, _) = base(Extent::OneSide(n(10.0)), -5.0, 0.0, 0.0);
    check("taper -5 widens", &d, 1, Some((frustum(10.0, -5.0), 1.0)));
    let (d, _) = base(Extent::OneSide(n(-10.0)), 5.0, 0.0, 0.0);
    check("taper on a negative extrude", &d, 1, Some((frustum(10.0, 5.0), 1.0)));
    let (d, _) = base(Extent::Symmetric(n(20.0)), 5.0, 0.0, 0.0);
    check("symmetric taper", &d, 1, Some((2.0 * frustum(10.0, 5.0), 2.0)));
    let (d, uid) = base(Extent::OneSide(n(10.0)), 45.0, 0.0, 0.0);
    expect_error("taper 45", &d, uid);
    let (d, uid) = base(Extent::OneSide(n(10.0)), 40.0, 0.0, 0.0);
    // 10 * tan 40 = 8.4 per side: still fits into 30 mm, must build
    let ev = run(&d);
    no_internal("taper 40", &ev, &d);
    assert!(err_of(&ev, uid).is_none(), "taper 40 on 40x30x10 fits: {:?}", err_of(&ev, uid));
    let (d, uid) = base(Extent::OneSide(n(30.0)), 40.0, 0.0, 0.0);
    // 30 * tan 40 = 25 per side: the walls cross, must be a clean error (or a closed body)
    let ev = run(&d);
    no_internal("taper past apex", &ev, &d);
    if err_of(&ev, uid).is_none() {
        assert!(ev.model().bodies.iter().all(closed), "taper past apex built a broken body");
        assert!(volume(&ev) <= 1200.0 * 30.0, "taper past apex grew the body: {}", volume(&ev));
    }
    let (d, _) = base(Extent::OneSide(n(10.0)), 0.0, 0.0, 5.0);
    let ev = check("start offset 5", &d, 1, Some((12000.0, 1e-6)));
    let (lo, hi) = bounds(&ev);
    assert!((lo[2] - 5.0).abs() < 1e-9 && (hi[2] - 15.0).abs() < 1e-9, "start offset bounds z {} .. {}", lo[2], hi[2]);
    let (d, _) = base(Extent::TwoSides(n(6.0), n(4.0)), 0.0, 0.0, 0.0);
    let ev = check("two sides", &d, 1, Some((12000.0, 1e-6)));
    let (lo, hi) = bounds(&ev);
    assert!((lo[2] + 4.0).abs() < 1e-9 && (hi[2] - 6.0).abs() < 1e-9, "two sides z {} .. {}", lo[2], hi[2]);
    let (d, uid) = base(Extent::OneSide(n(10.0)), 0.0, 20.0, 0.0);
    expect_error("thin wall thicker than the profile", &d, uid);
    let (d, uid) = base(Extent::OneSide(n(10.0)), 0.0, -1.0, 0.0);
    expect_error("negative thin wall", &d, uid);
    let (d, _) = base(Extent::OneSide(n(10.0)), 0.0, 0.01, 0.0);
    check("thin wall 0.01", &d, 1, Some(((1200.0 - 39.98 * 29.98) * 10.0, 0.2)));
    round_trip("thin wall 0.01", &d);
}

#[test]
fn extrude_up_to_body_and_through_all() {
    let mut d = Document::default();
    boxp(&mut d, [0.0, 0.0, 20.0], 40.0, 30.0, 10.0, Op::NewBody);
    let target = first_body(&d);
    let sk = rect_sketch(&mut d, Plane::XY, [10.0, 10.0], [20.0, 20.0]);
    d.insert(ext_kind(sk, [15.0, 15.0], Extent::ToBody(target), Op::NewBody));
    check("extrude to body", &d, 2, Some((12000.0 + 100.0 * 20.0, 1e-3)));
    // through all, both ways, cuts a slot through a body straddling the sketch plane
    let mut t = Document::default();
    boxp(&mut t, [0.0, 0.0, -10.0], 40.0, 30.0, 20.0, Op::NewBody);
    let sk = rect_sketch(&mut t, Plane::XY, [10.0, 10.0], [20.0, 20.0]);
    t.insert(ext_kind(sk, [15.0, 15.0], Extent::All(true), Op::Cut));
    check("through all both ways", &t, 1, Some((24000.0 - 100.0 * 20.0, 1e-3)));
    // through all one way only cuts the upper half
    let mut o = Document::default();
    boxp(&mut o, [0.0, 0.0, -10.0], 40.0, 30.0, 20.0, Op::NewBody);
    let sk = rect_sketch(&mut o, Plane::XY, [10.0, 10.0], [20.0, 20.0]);
    o.insert(ext_kind(sk, [15.0, 15.0], Extent::All(false), Op::Cut));
    check("through all one way", &o, 1, Some((24000.0 - 100.0 * 10.0, 1e-3)));
    // through all on an empty model is a clean error
    let mut e = Document::default();
    let sk = rect_sketch(&mut e, Plane::XY, [10.0, 10.0], [20.0, 20.0]);
    let uid = e.insert(ext_kind(sk, [15.0, 15.0], Extent::All(true), Op::Cut));
    expect_error("through-all cut with no bodies", &e, uid);
    // up to a body that is behind the profile
    let mut b = Document::default();
    boxp(&mut b, [0.0, 0.0, -30.0], 40.0, 30.0, 10.0, Op::NewBody);
    let tb = first_body(&b);
    let sk = rect_sketch(&mut b, Plane::XY, [10.0, 10.0], [20.0, 20.0]);
    let uid = b.insert(ext_kind(sk, [15.0, 15.0], Extent::ToBody(tb), Op::NewBody));
    expect_error("extrude to a body behind the profile", &b, uid);
    round_trip("extrude to body", &d);
}

#[test]
fn primitive_sizes_zero_negative_tiny_huge() {
    for (w, ok) in [(0.0, false), (-5.0, false), (0.01, true), (10000.0, true)] {
        let mut d = Document::default();
        let uid = boxp(&mut d, [0.0; 3], w, w, w, Op::NewBody);
        if ok {
            check(&format!("box {w}"), &d, 1, Some((w * w * w, w * w * w * 1e-9)));
        } else {
            expect_error(&format!("box {w}"), &d, uid);
        }
    }
    let mut d = Document::default();
    let uid = d.insert(FeatureKind::Primitive { shape: Shape::Torus { major: n(5.0), minor: n(5.0) }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    expect_error("torus minor = major", &d, uid);
    let mut d = Document::default();
    let uid = d.insert(FeatureKind::Primitive { shape: Shape::Sphere { r: Expr::from("sqrt(-1)") }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    expect_error("sphere with NaN radius", &d, uid);
    let mut d = Document::default();
    let uid = d.insert(FeatureKind::Primitive { shape: Shape::Box { w: Expr::from("1e400"), d: n(1.0), h: n(1.0) }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    expect_error("box with infinite width", &d, uid);
}

#[test]
fn fillet_radius_zero_negative_tiny_too_big_huge() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    for r in [0.0, -1.0, 10000.0] {
        let mut x = d.clone();
        let uid = x.insert(FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), radius: n(r) });
        expect_error(&format!("fillet r={r}"), &x, uid);
    }
    let mut x = d.clone();
    x.insert(FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), radius: n(0.01) });
    check("fillet r=0.01", &x, 1, Some((24000.0 - FIL * 1e-4 * 140.0, 0.05)));
    // r = 16 does not fit into the 30 mm depth (two opposite fillets would overlap): clean error or a closed body
    for r in [15.0, 25.0] {
        let mut x = d.clone();
        let uid = x.insert(FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), radius: n(r) });
        let ev = run(&x);
        no_internal(&format!("fillet r={r}"), &ev, &x);
        if err_of(&ev, uid).is_none() {
            let m = ev.model();
            assert_eq!(m.bodies.len(), 1, "fillet r={r}: body count");
            assert!(closed(&m.bodies[0]), "fillet r={r}: not closed");
            let (lo, hi) = bounds(&ev);
            assert!(near3(lo, [0.0, 0.0, 0.0], 1e-3) && near3(hi, [40.0, 30.0, 20.0], 1e-3), "fillet r={r}: bounds changed {lo:?} {hi:?}");
            assert!(volume(&ev) > 24000.0 - FIL * r * r * 140.0 - 200.0 && volume(&ev) < 24000.0, "fillet r={r}: volume {}", volume(&ev));
        }
    }
}

#[test]
fn fillet_or_chamfer_wider_than_the_face_is_rejected() {
    // 40 x 30 x 20 box, all four top edges: two opposite fillets of r > 15 overlap and eat the top face
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    for (name, kind) in [
        ("fillet r=16", FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), radius: n(16.0) }),
        ("chamfer d=16", FeatureKind::Chamfer { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), dist: n(16.0), second: ChamferSecond::Equal, flip: false }),
        ("fillet r=20", FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), radius: n(20.0) }),
    ] {
        let mut x = d.clone();
        let uid = x.insert(kind);
        let ev = run(&x);
        let (_, hi) = bounds(&ev);
        assert!(err_of(&ev, uid).is_some(), "{name}: accepted, top of the body now at z = {} instead of 20", hi[2]);
    }
}

#[test]
fn fillet_single_edge_huge_and_tiny_bodies() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 10000.0, 10000.0, 10000.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 10000.0], [10000.0, 0.0, 10000.0])], radius: n(100.0) });
    check("fillet on a 10 m cube", &d, 1, Some((1e12 - FIL * 1e4 * 1e4, 1e12 * 1e-6)));
    let mut t = Document::default();
    boxp(&mut t, [0.0; 3], 0.1, 0.1, 0.1, Op::NewBody);
    let id = first_body(&t);
    t.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 0.1], [0.1, 0.0, 0.1])], radius: n(0.01) });
    check("fillet on a 0.1 mm cube", &t, 1, Some((1e-3 - FIL * 1e-4 * 0.1, 1e-6)));
}

#[test]
fn fillet_concave_edge_of_an_l_shape() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 10.0, Op::NewBody);
    boxp(&mut d, [0.0, 0.0, 10.0], 40.0, 10.0, 20.0, Op::Join);
    let ev = check("L shape", &d, 1, Some((20000.0, 1e-3)));
    let id = ev.model().bodies[0].id;
    d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 10.0, 10.0], [40.0, 10.0, 10.0])], radius: n(3.0) });
    check("concave fillet adds material", &d, 1, Some((20000.0 + FIL * 9.0 * 40.0, 1.5)));
    round_trip("concave fillet", &d);
}

#[test]
fn fillet_three_edges_meeting_at_a_corner() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    let edges = vec![
        EdgeRef::line([0.0, 0.0, 20.0], [20.0, 0.0, 20.0]),
        EdgeRef::line([0.0, 0.0, 20.0], [0.0, 20.0, 20.0]),
        EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, 20.0]),
    ];
    d.insert(FeatureKind::Fillet { body: id, edges, radius: n(3.0) });
    let ev = check("three edges at a corner", &d, 1, None);
    let removed = 8000.0 - volume(&ev);
    // three edges of 17 mm full section plus the corner: r^3 - sphere octant (0.4764 r^3) + 3 * FIL r^3
    let want = 3.0 * FIL * 9.0 * 17.0 + 27.0 * (1.0 - PI / 6.0) ;
    assert!((removed - want).abs() < 6.0, "corner blend removed {removed}, want about {want}");
    round_trip("corner blend", &d);
}

#[test]
fn edge_refs_that_do_not_exist_or_are_degenerate() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    let cases: Vec<(&str, FeatureKind)> = vec![
        ("fillet an edge in the air", FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([50.0, 50.0, 50.0], [60.0, 50.0, 50.0])], radius: n(1.0) }),
        ("fillet a zero-length edge", FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [0.0, 0.0, 10.0])], radius: n(1.0) }),
        ("fillet a diagonal of a face", FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 20.0, 10.0])], radius: n(1.0) }),
        ("fillet a circle that is not there", FeatureKind::Fillet { body: id, edges: vec![EdgeRef::round([5.0, 0.0, 10.0], [0.0, 0.0, 10.0], [0.0, 0.0, 1.0])], radius: n(1.0) }),
        ("fillet a circle with a zero axis", FeatureKind::Fillet { body: id, edges: vec![EdgeRef::round([5.0, 0.0, 10.0], [0.0, 0.0, 10.0], [0.0, 0.0, 0.0])], radius: n(1.0) }),
        ("fillet a body that does not exist", FeatureKind::Fillet { body: 999_999, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 0.0, 10.0])], radius: n(1.0) }),
        ("fillet with no edges", FeatureKind::Fillet { body: id, edges: vec![], radius: n(1.0) }),
        ("chamfer angle 0", FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 0.0, 10.0])], dist: n(1.0), second: ChamferSecond::Angle(n(0.0)), flip: false }),
        ("chamfer angle 90", FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 0.0, 10.0])], dist: n(1.0), second: ChamferSecond::Angle(n(90.0)), flip: false }),
        ("chamfer angle -30", FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 0.0, 10.0])], dist: n(1.0), second: ChamferSecond::Angle(n(-30.0)), flip: false }),
        ("chamfer second distance 0", FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 0.0, 10.0])], dist: n(1.0), second: ChamferSecond::Distance(n(0.0)), flip: false }),
    ];
    for (name, kind) in cases {
        let mut x = d.clone();
        let uid = x.insert(kind);
        let ev = run(&x);
        no_internal(name, &ev, &x);
        match err_of(&ev, uid) {
            Some(_) => assert!((volume(&ev) - 2000.0 * 2.0).abs() < 1e-6 || (volume(&ev) - 4000.0).abs() < 1e-6, "{name}: failed step changed the model"),
            None => {
                // a step that claims success must leave a closed body no bigger than before
                let m = ev.model();
                assert!(m.bodies.iter().all(closed), "{name}: built a broken body");
                assert!(volume(&ev) <= 4000.0 + 1e-6 && volume(&ev) > 3000.0, "{name}: silently produced volume {}", volume(&ev));
            }
        }
    }
}

#[test]
fn chamfer_angle_matches_two_distances() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    let e = vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 0.0, 10.0])];
    let mut a = d.clone();
    a.insert(FeatureKind::Chamfer { body: id, edges: e.clone(), dist: n(2.0), second: ChamferSecond::Angle(n(45.0)), flip: false });
    check("chamfer 2 at 45 deg", &a, 1, Some((4000.0 - 0.5 * 4.0 * 20.0, 0.05)));
    let mut b = d.clone();
    b.insert(FeatureKind::Chamfer { body: id, edges: e.clone(), dist: n(2.0), second: ChamferSecond::Distance(n(4.0)), flip: false });
    check("chamfer 2 x 4", &b, 1, Some((4000.0 - 0.5 * 8.0 * 20.0, 0.05)));
    let mut c = d.clone();
    c.insert(FeatureKind::Chamfer { body: id, edges: e, dist: n(2.0), second: ChamferSecond::Distance(n(4.0)), flip: true });
    check("chamfer 2 x 4 flipped", &c, 1, Some((4000.0 - 0.5 * 8.0 * 20.0, 0.05)));
    round_trip("chamfer flip", &c);
}

#[test]
fn chamfer_on_circular_edges() {
    let mut d = Document::default();
    cylp(&mut d, [0.0; 3], 15.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    let v0 = volume(&run(&d));
    let mut top = d.clone();
    top.insert(FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::round([15.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0])], dist: n(2.0), second: ChamferSecond::Equal, flip: false });
    // Pappus: triangle area 2 at the radius of its centroid 15 - 2/3
    let ring = 2.0 * 2.0 * PI * (15.0 - 2.0 / 3.0);
    check("chamfer top rim", &top, 1, Some((v0 - ring, 0.02 * ring + 2.0)));
    let mut both = top.clone();
    both.insert(FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::round([15.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, -1.0])], dist: n(2.0), second: ChamferSecond::Equal, flip: false });
    check("chamfer both rims", &both, 1, Some((v0 - 2.0 * ring, 0.04 * ring + 4.0)));
    // a chamfer as big as the radius cannot fit
    let mut big = d.clone();
    let uid = big.insert(FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::round([15.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0])], dist: n(16.0), second: ChamferSecond::Equal, flip: false });
    let ev = run(&big);
    no_internal("chamfer bigger than radius", &ev, &big);
    if err_of(&ev, uid).is_none() {
        assert!(ev.model().bodies.iter().all(closed), "chamfer bigger than radius: broken body");
    }
    // an open arc edge (half the rim)
    let mut half = d.clone();
    let mut e = EdgeRef::round([15.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0]);
    e.sweep = Some(180.0);
    half.insert(FeatureKind::Chamfer { body: id, edges: vec![e], dist: n(2.0), second: ChamferSecond::Equal, flip: false });
    check("chamfer half the rim", &half, 1, Some((v0 - ring / 2.0, 0.03 * ring + 3.0)));
    round_trip("chamfer both rims", &both);
}

#[test]
fn holes_at_an_edge_at_a_corner_through_all_and_too_big() {
    let top = zplane(20.0);
    let mk = |at: [f64; 2], dia: f64, depth: Option<f64>| {
        let mut d = Document::default();
        boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
        let uid = d.insert(FeatureKind::Hole { plane: top, at: [n(at[0]), n(at[1])], dia: n(dia), depth: depth.map(n), kind: HoleKind::Simple, more: vec![] });
        (d, uid)
    };
    let cyl = |r: f64, h: f64| PI * r * r * h;
    let (d, _) = mk([0.0, 15.0], 10.0, None);
    check("hole centred on an edge", &d, 1, Some((24000.0 - cyl(5.0, 20.0) / 2.0, 3.0)));
    let (d, _) = mk([0.0, 0.0], 10.0, None);
    check("hole centred on a corner", &d, 1, Some((24000.0 - cyl(5.0, 20.0) / 4.0, 2.0)));
    let (d, _) = mk([20.0, 15.0], 10.0, Some(10000.0));
    check("hole 10000 deep", &d, 1, Some((24000.0 - cyl(5.0, 20.0), 3.0)));
    let (d, _) = mk([20.0, 15.0], 0.01, None);
    check("hole 0.01 wide", &d, 1, Some((24000.0, 0.01)));
    let (d, uid) = mk([20.0, 15.0], 0.0, None);
    expect_error("hole 0 wide", &d, uid);
    let (d, uid) = mk([20.0, 15.0], 10.0, Some(-5.0));
    expect_error("hole -5 deep", &d, uid);
    let (d, uid) = mk([200.0, 15.0], 10.0, None);
    expect_error("hole beside the body", &d, uid);
    round_trip("hole on edge", &mk([0.0, 15.0], 10.0, None).0);
}

#[test]
fn hole_bigger_than_the_body_removes_it() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let uid = d.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(20.0), n(15.0)], dia: n(200.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    let ev = run(&d);
    no_internal("hole bigger than body", &ev, &d);
    // either the user is told, or the body is gone: never a broken leftover
    assert!(err_of(&ev, uid).is_some() || ev.model().bodies.is_empty(), "hole bigger than the body left {:?}", body_volumes(&ev));
}

#[test]
fn countersink_counterbore_and_tapped_volumes() {
    let top = zplane(20.0);
    let mk = |kind: HoleKind, depth: Option<f64>| {
        let mut d = Document::default();
        boxp(&mut d, [0.0; 3], 60.0, 40.0, 20.0, Op::NewBody);
        let uid = d.insert(FeatureKind::Hole { plane: top, at: [n(20.0), n(20.0)], dia: n(6.0), depth: depth.map(n), kind, more: vec![] });
        (d, uid)
    };
    // countersink 12 at 90 degrees: a cone 3 deep from r 3 to r 6
    let (d, _) = mk(HoleKind::Countersink { dia: n(12.0), angle: n(90.0) }, None);
    let cone_extra = PI * 3.0 / 3.0 * (9.0 + 36.0 + 18.0) - PI * 9.0 * 3.0;
    check("countersink", &d, 1, Some((48000.0 - PI * 9.0 * 20.0 - cone_extra, 4.0)));
    // counterbore deeper than a blind hole is clipped to the hole depth
    let (d, _) = mk(HoleKind::Counterbore { dia: n(12.0), depth: n(15.0) }, Some(10.0));
    check("counterbore deeper than the hole", &d, 1, Some((48000.0 - PI * 36.0 * 10.0, 4.0)));
    let (d, uid) = mk(HoleKind::Counterbore { dia: n(4.0), depth: n(5.0) }, None);
    expect_error("counterbore narrower than the hole", &d, uid);
    let (d, uid) = mk(HoleKind::Countersink { dia: n(12.0), angle: n(180.0) }, None);
    expect_error("countersink angle 180", &d, uid);
    let (d, _) = mk(HoleKind::Tapped { major: n(8.0), pitch: n(1.25), modeled: true }, Some(12.0));
    let ev = check("modelled tapped hole", &d, 1, None);
    let v = volume(&ev);
    let drill = 48000.0 - PI * 9.0 * 12.0;
    assert!(v < drill - 10.0 && v > 48000.0 - PI * 16.0 * 12.0, "tapped hole volume {v} must lie between the drill ({drill}) and the major diameter");
    round_trip("tapped", &d);
}

#[test]
fn pattern_counts_one_hundred_and_out_of_range() {
    let mut base = Document::default();
    boxp(&mut base, [0.0; 3], 2.0, 2.0, 2.0, Op::NewBody);
    let id = first_body(&base);
    let rect = |nx: f64| PatternKind::Rect { nx: n(nx), dx: n(3.0), ny: n(1.0), dy: n(0.0), nz: n(1.0), dz: n(0.0) };
    for c in [1.0, 0.0, -3.0, 1001.0] {
        let mut d = base.clone();
        let uid = d.insert(FeatureKind::Pattern { bodies: vec![id], kind: rect(c) });
        expect_error(&format!("rect pattern {c}"), &d, uid);
        let mut d = base.clone();
        let uid = d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Circular { count: n(c), axis: Axis3::Z, angle: n(360.0) } });
        expect_error(&format!("circular pattern {c}"), &d, uid);
    }
    let mut d = base.clone();
    d.insert(FeatureKind::Pattern { bodies: vec![id], kind: rect(100.0) });
    let ev = check("rect pattern 100", &d, 100, Some((800.0, 1e-6)));
    let (_, hi) = bounds(&ev);
    assert!((hi[0] - (99.0 * 3.0 + 2.0)).abs() < 1e-9, "rect pattern 100 reaches x {}", hi[0]);
    let mut d = base.clone();
    d.insert(FeatureKind::Move { bodies: vec![id], translate: p3(100.0, 0.0, 0.0), rotate: p3(0.0, 0.0, 0.0), copy: false });
    d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Circular { count: n(100.0), axis: Axis3::Z, angle: n(360.0) } });
    check("circular pattern 100", &d, 100, Some((800.0, 1e-3)));
    // a partial circular pattern: 3 copies over 90 degrees end at 90 degrees
    let mut d = base.clone();
    d.insert(FeatureKind::Move { bodies: vec![id], translate: p3(50.0, -1.0, 0.0), rotate: p3(0.0, 0.0, 0.0), copy: false });
    d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Circular { count: n(3.0), axis: Axis3::Z, angle: n(90.0) } });
    let ev = check("circular pattern over 90 deg", &d, 3, Some((24.0, 1e-3)));
    let (lo, hi) = bounds(&ev);
    assert!((hi[1] - 52.0).abs() < 1e-6 && (lo[0] + 1.0).abs() < 1e-6, "the last copy must sit at 90 degrees: lo {lo:?} hi {hi:?}");
    round_trip("circular 90", &d);
}

#[test]
fn mirror_plane_through_and_outside_the_body() {
    let mut base = Document::default();
    boxp(&mut base, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&base);
    let cases = [(10.0, true, 1, 4000.0), (5.0, true, 1, 6000.0), (0.0, true, 1, 8000.0), (30.0, true, 1, 8000.0), (30.0, false, 2, 8000.0), (10.0, false, 2, 8000.0)];
    for (offset, join, bodies, vol) in cases {
        let mut d = base.clone();
        d.insert(FeatureKind::Mirror { bodies: vec![id], normal: Axis3::X, join, offset: n(offset) });
        let name = format!("mirror x={offset} join={join}");
        let ev = check(&name, &d, bodies, Some((vol, 1e-3)));
        let (lo, hi) = bounds(&ev);
        let want_lo = (2.0 * offset - 20.0).min(0.0);
        let want_hi = (2.0 * offset).max(20.0);
        assert!((lo[0] - want_lo).abs() < 1e-9 && (hi[0] - want_hi).abs() < 1e-9, "{name}: x range {} .. {}", lo[0], hi[0]);
        round_trip(&name, &d);
    }
}

#[test]
fn revolve_full_partial_touching_crossing_axis() {
    let mk = |p0: [f64; 2], p1: [f64; 2], axis: RevolveAxis, angle: f64| {
        let mut d = Document::default();
        let sk = rect_sketch(&mut d, Plane::XZ, p0, p1);
        let seed = [(p0[0] + p1[0]) / 2.0, (p0[1] + p1[1]) / 2.0];
        let uid = d.insert(FeatureKind::Revolve { sketch: sk, profiles: vec![ProfileRef::at(seed)], axis, angle: n(angle), op: Op::NewBody });
        (d, uid)
    };
    // 72 facets round a full turn: the polygon area factor
    let fac = (2.0 * PI / 72.0).sin() * 72.0 / (2.0 * PI);
    let solid = PI * 100.0 * 20.0 * fac;
    let (d, _) = mk([0.0, 0.0], [10.0, 20.0], RevolveAxis::SketchY, 360.0);
    check("revolve touching axis 360", &d, 1, Some((solid, 5.0)));
    round_trip("revolve 360", &d);
    let (d, _) = mk([0.0, 0.0], [10.0, 20.0], RevolveAxis::SketchY, 90.0);
    check("revolve touching axis 90", &d, 1, Some((solid / 4.0, 3.0)));
    let (d, _) = mk([0.0, 0.0], [10.0, 20.0], RevolveAxis::SketchY, -90.0);
    check("revolve touching axis -90", &d, 1, Some((solid / 4.0, 3.0)));
    let (d, _) = mk([0.0, 0.0], [10.0, 20.0], RevolveAxis::SketchY, 180.0);
    check("revolve touching axis 180", &d, 1, Some((solid / 2.0, 3.0)));
    let (d, _) = mk([0.0, 0.0], [10.0, 20.0], RevolveAxis::SketchY, 720.0);
    check("revolve 720 is a full turn", &d, 1, Some((solid, 5.0)));
    let (d, uid) = mk([0.0, 0.0], [10.0, 20.0], RevolveAxis::SketchY, 0.0);
    expect_error("revolve 0", &d, uid);
    let (d, uid) = mk([-5.0, 0.0], [10.0, 20.0], RevolveAxis::SketchY, 360.0);
    expect_error("revolve with the axis through the profile", &d, uid);
    // the left side of the rectangle (line index 3) as the axis is the same as the sketch Y axis
    let (d, _) = mk([0.0, 0.0], [10.0, 20.0], RevolveAxis::Line(3), 360.0);
    check("revolve about a sketch line on the profile", &d, 1, Some((solid, 5.0)));
    let (d, uid) = mk([0.0, 0.0], [10.0, 20.0], RevolveAxis::Line(42), 360.0);
    expect_error("revolve about a missing line", &d, uid);
    // ring about the sketch X axis (profile above it), and a 1-degree sliver
    let (d, _) = mk([0.0, 5.0], [20.0, 10.0], RevolveAxis::SketchX, 360.0);
    check("revolve ring about X", &d, 1, Some((PI * (100.0 - 25.0) * 20.0 * fac, 5.0)));
    let (d, _) = mk([10.0, 0.0], [20.0, 15.0], RevolveAxis::SketchY, 1.0);
    check("revolve 1 degree", &d, 1, Some((PI * 300.0 * 15.0 / 360.0, 0.1)));
}

#[test]
fn loft_between_different_profiles() {
    let up = |z: f64| Plane { origin: [0.0, 0.0, z], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] };
    let mut d = Document::default();
    let mut a = Sketch::default();
    a.add_rect([-10.0, -10.0], [10.0, 10.0]);
    let sa = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: a });
    let mut b = Sketch::default();
    b.add_circle([0.0, 0.0], 10.0);
    let sb = d.insert(FeatureKind::Sketch { plane: up(30.0), sketch: b });
    d.insert(FeatureKind::Loft { profiles: vec![(sa, ProfileRef::at([1.0, 1.0])), (sb, ProfileRef::at([1.0, 1.0]))], op: Op::NewBody });
    let ev = check("loft square to circle", &d, 1, None);
    let v = volume(&ev);
    assert!(v > PI * 100.0 * 30.0 && v < 400.0 * 30.0, "loft square to circle volume {v}");
    let (lo, hi) = bounds(&ev);
    assert!(near3(lo, [-10.0, -10.0, 0.0], 1e-6) && near3(hi, [10.0, 10.0, 30.0], 1e-6), "loft bounds {lo:?} {hi:?}");
    round_trip("loft square to circle", &d);
    // three sections: square, small circle, square (an hourglass)
    let mut c = Sketch::default();
    c.add_circle([0.0, 0.0], 4.0);
    let sc = d.insert(FeatureKind::Sketch { plane: up(15.0), sketch: c });
    let mut e = Sketch::default();
    e.add_rect([-10.0, -10.0], [10.0, 10.0]);
    let se = d.insert(FeatureKind::Sketch { plane: up(30.0), sketch: e });
    let mut h = d.clone();
    h.features.retain(|f| !matches!(f.kind, FeatureKind::Loft { .. }));
    h.rollback = h.features.len();
    h.insert(FeatureKind::Loft { profiles: vec![(sa, ProfileRef::at([1.0, 1.0])), (sc, ProfileRef::at([0.5, 0.5])), (se, ProfileRef::at([1.0, 1.0]))], op: Op::NewBody });
    let ev = check("loft hourglass", &h, 1, None);
    let v = volume(&ev);
    assert!(v > 2.0 * PI * 16.0 * 15.0 && v < 400.0 * 30.0 * 0.7, "hourglass loft volume {v}");
    // a single profile is an error
    let mut one = Document::default();
    let mut a = Sketch::default();
    a.add_circle([0.0, 0.0], 5.0);
    let sa = one.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: a });
    let uid = one.insert(FeatureKind::Loft { profiles: vec![(sa, ProfileRef::at([0.0, 0.0]))], op: Op::NewBody });
    expect_error("loft with one profile", &one, uid);
}

#[test]
fn coplanar_loft_is_a_clean_error() {
    let mut s = Document::default();
    let mut a = Sketch::default();
    a.add_circle([0.0, 0.0], 5.0);
    a.add_circle([30.0, 0.0], 5.0);
    let sa = s.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: a });
    let uid = s.insert(FeatureKind::Loft { profiles: vec![(sa, ProfileRef::at([0.0, 0.0])), (sa, ProfileRef::at([30.0, 0.0]))], op: Op::NewBody });
    let ev = run(&s);
    no_internal("coplanar loft", &ev, &s);
    if err_of(&ev, uid).is_none() {
        assert!(ev.model().bodies.iter().all(|b| closed(b) && b.solid.volume() > 1.0), "coplanar loft built a flat body: {:?}", body_volumes(&ev));
    }
}

#[test]
fn sweep_along_an_arc_ends_flush_with_the_tangent_planes() {
    // a 20 mm radius disc swept along a quarter arc of radius 50: the ends must lie in the planes y = 0 and x = 0
    let mut d = Document::default();
    let mut path = Sketch::default();
    let c = path.add_point([0.0, 0.0]);
    let a = path.add_point([50.0, 0.0]);
    let b = path.add_point([0.0, 50.0]);
    path.add(Entity::Arc { c, a, b, construction: false });
    let sp = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: path });
    let mut prof = Sketch::default();
    prof.add_circle([50.0, 0.0], 20.0);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: prof });
    d.insert(FeatureKind::Sweep { sketch: sk, profiles: vec![ProfileRef::at([50.0, 0.0])], path: sp, op: Op::NewBody });
    let ev = check("elbow", &d, 1, None);
    let (lo, _) = bounds(&ev);
    assert!(lo[0] > -1e-3 && lo[1] > -1e-3, "elbow ends stick out past their planes: min x {} min y {}", lo[0], lo[1]);
}

#[test]
fn sweep_along_an_arc_and_an_l() {
    // circle of radius 2 swept along a quarter arc of radius 30: Pappus 2 pi^2 r^2 R / 4
    let mut d = Document::default();
    let mut path = Sketch::default();
    let c = path.add_point([0.0, 0.0]);
    let a = path.add_point([30.0, 0.0]);
    let b = path.add_point([0.0, 30.0]);
    path.add(Entity::Arc { c, a, b, construction: false });
    let sp = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: path });
    let mut prof = Sketch::default();
    prof.add_circle([30.0, 0.0], 2.0);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: prof });
    d.insert(FeatureKind::Sweep { sketch: sk, profiles: vec![ProfileRef::at([30.0, 0.0])], path: sp, op: Op::NewBody });
    let want = PI * 4.0 * (PI / 2.0 * 30.0);
    let ev = check("sweep along an arc", &d, 1, Some((want, want * 0.02)));
    let (lo, hi) = bounds(&ev);
    // the end ring stands across the arc at (0, 30): it spans y 28..32 at x = 0
    assert!(near3(hi, [32.0, 32.0, 2.0], 0.02) && near3(lo, [-2.0, -2.0, -2.0], 2.1), "swept arc spans {lo:?} .. {hi:?}");
    assert!(lo[0] > -0.1 && lo[1] > -0.1, "swept arc must stay in the first quadrant: {lo:?}");
    round_trip("sweep arc", &d);
    // a square profile along an L: the mitred corner keeps area * centreline length
    let mut l = Document::default();
    let mut path = Sketch::default();
    path.add_line([0.0, 0.0], [0.0, 40.0]);
    path.add_line([0.0, 40.0], [30.0, 40.0]);
    let sp = l.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: path });
    let mut prof = Sketch::default();
    prof.add_rect([-2.0, -2.0], [2.0, 2.0]);
    let sk = l.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: prof });
    l.insert(FeatureKind::Sweep { sketch: sk, profiles: vec![ProfileRef::at([0.0, 0.0])], path: sp, op: Op::NewBody });
    check("sweep square along L", &l, 1, Some((16.0 * 70.0, 1.0)));
    // a path that folds back on itself is a clean error
    let mut f = Document::default();
    let mut path = Sketch::default();
    path.add_line([0.0, 0.0], [0.0, 40.0]);
    path.add_line([0.0, 40.0], [1.0, 0.0]);
    let sp = f.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: path });
    let mut prof = Sketch::default();
    prof.add_circle([0.0, 0.0], 2.0);
    let sk = f.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: prof });
    let uid = f.insert(FeatureKind::Sweep { sketch: sk, profiles: vec![ProfileRef::at([0.0, 0.0])], path: sp, op: Op::NewBody });
    expect_error("sweep along a hairpin", &f, uid);
}

#[test]
fn pipe_along_open_and_closed_paths() {
    let pipe = |path: u64, od: f64, wall: f64, bend: f64| FeatureKind::Pipe { path, outer: n(od), wall: n(wall), op: Op::NewBody, shape: PipeShape::Round, corner: n(0.0), bend: n(bend) };
    let fac = (2.0 * PI / 72.0).sin() * 72.0 / (2.0 * PI);
    // open L path, solid rod: area * centreline length (mitred corner)
    let mut d = Document::default();
    let mut s = Sketch::default();
    s.add_line([0.0, 0.0], [40.0, 0.0]);
    s.add_line([40.0, 0.0], [40.0, 30.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(pipe(sk, 6.0, 0.0, 0.0));
    check("solid rod along L", &d, 1, Some((PI * 9.0 * fac * 70.0, 3.0)));
    // the same with rounded bends: shorter centreline, still closed
    let mut r = Document::default();
    let mut s = Sketch::default();
    s.add_line([0.0, 0.0], [40.0, 0.0]);
    s.add_line([40.0, 0.0], [40.0, 30.0]);
    let sk = r.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    r.insert(pipe(sk, 6.0, 1.0, 8.0));
    let centre = 70.0 - 2.0 * 8.0 + PI / 2.0 * 8.0;
    check("pipe with bend radius 8", &r, 1, Some((PI * (9.0 - 4.0) * fac * centre, 0.03 * PI * 5.0 * centre)));
    round_trip("pipe bend", &r);
    // wall too thick, negative bend
    let mut w = d.clone();
    let uid = w.insert(pipe(sk, 6.0, 3.0, 0.0));
    expect_error("pipe wall = radius", &w, uid);
    let mut w = d.clone();
    let uid = w.insert(pipe(sk, 6.0, 1.0, -1.0));
    expect_error("pipe negative bend", &w, uid);
}

#[test]
fn pipe_along_a_closed_rectangle_is_a_closed_ring() {
    let fac = (2.0 * PI / 72.0).sin() * 72.0 / (2.0 * PI);
    let mut d = Document::default();
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [40.0, 30.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(FeatureKind::Pipe { path: sk, outer: n(6.0), wall: n(0.0), op: Op::NewBody, shape: PipeShape::Round, corner: n(0.0), bend: n(0.0) });
    // a mitred square ring of a rod: area * centreline perimeter
    let ev = check("rod along a closed rectangle", &d, 1, Some((PI * 9.0 * fac * 140.0, 0.01 * PI * 9.0 * 140.0)));
    let (lo, hi) = bounds(&ev);
    assert!(near3(lo, [-3.0, -3.0, -3.0], 0.05) && near3(hi, [43.0, 33.0, 3.0], 0.05), "closed ring bounds {lo:?} {hi:?}");
    // every corner is mitred, including the seam where the loop starts and ends (0, 0)
    let b = &ev.model().bodies[0];
    for c in [[42.0, -2.0, 0.0], [42.0, 32.0, 0.0], [-2.0, 32.0, 0.0], [-2.0, -2.0, 0.0]] {
        assert!(inside(b, c), "outer corner {c:?} of the closed ring is missing (the start/end of the loop is not mitred)");
    }
    // and nothing is counted twice: a probe in the corner of the seam holds a single layer of material
    let v = probe_volume(b, [1.0, 1.0, 0.0]);
    assert!((v - 0.008).abs() < 1e-4, "the seam overlaps itself: probe volume {v} instead of 0.008");
}

#[test]
fn pipe_along_a_closed_circle_of_arcs() {
    let fac = (2.0 * PI / 72.0).sin() * 72.0 / (2.0 * PI);
    let mut d = Document::default();
    let mut s = Sketch::default();
    let c = s.add_point([0.0, 0.0]);
    let a = s.add_point([30.0, 0.0]);
    let b = s.add_point([-30.0, 0.0]);
    s.add(Entity::Arc { c, a, b, construction: false });
    s.add(Entity::Arc { c, a: b, b: a, construction: false });
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(FeatureKind::Pipe { path: sk, outer: n(6.0), wall: n(1.0), op: Op::NewBody, shape: PipeShape::Round, corner: n(0.0), bend: n(0.0) });
    // a torus-shaped tube: area * 2 pi R (the centreline is a 72-gon)
    let want = PI * (9.0 - 4.0) * fac * 2.0 * PI * 30.0 * fac;
    check("tube along a closed circle", &d, 1, Some((want, want * 0.02)));
}

#[test]
fn coil_and_thread_limits() {
    let coil = |pitch: f64, section: f64, turns: f64| FeatureKind::Coil { at: p3(0.0, 0.0, 0.0), axis: Axis3::Z, dia: n(20.0), pitch: n(pitch), turns: n(turns), section: n(section), op: Op::NewBody };
    let mut d = Document::default();
    d.insert(coil(6.0, 2.0, 5.0));
    // Pappus along the helix: area * length of the centreline
    let len = 5.0 * ((PI * 20.0).powi(2) + 36.0).sqrt();
    check("coil", &d, 1, Some((PI * len, PI * len * 0.03)));
    for (p, s, t) in [(2.0, 2.0, 5.0), (6.0, 0.0, 5.0), (6.0, 2.0, 0.0), (0.0, 2.0, 5.0)] {
        let mut d = Document::default();
        let uid = d.insert(coil(p, s, t));
        expect_error(&format!("coil pitch {p} section {s} turns {t}"), &d, uid);
    }
    let mut d = Document::default();
    d.insert(coil(6.0, 2.0, 0.25));
    check("quarter-turn coil", &d, 1, None);
    let mut t = Document::default();
    let uid = t.insert(FeatureKind::Thread { at: p3(0.0, 0.0, 0.0), axis: Axis3::Z, dia: n(10.0), pitch: n(1.5), length: n(20.0), depth: n(5.0), op: Op::NewBody });
    expect_error("thread depth = radius", &t, uid);
    let mut t = Document::default();
    t.insert(FeatureKind::Thread { at: p3(0.0, 0.0, 0.0), axis: Axis3::X, dia: n(10.0), pitch: n(1.5), length: n(20.0), depth: n(0.8), op: Op::NewBody });
    let ev = check("thread along X", &t, 1, None);
    let v = volume(&ev);
    assert!(v > PI * 4.2f64.powi(2) * 20.0 && v < PI * 25.0 * 20.0, "thread volume {v}");
    let (lo, hi) = bounds(&ev);
    assert!((lo[0]).abs() < 1e-6 && (hi[0] - 20.0).abs() < 1e-6, "thread along X spans {} .. {}", lo[0], hi[0]);
}

#[test]
fn thread_on_a_boss_and_in_a_hole() {
    let mut d = Document::default();
    cylp(&mut d, [0.0; 3], 5.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    let v0 = volume(&run(&d));
    let mut boss = d.clone();
    boss.insert(FeatureKind::ThreadOn { body: id, edge: EdgeRef::round([5.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0]), length: n(10.0), pitch: n(1.5), depth: n(0.7) });
    let ev = check("thread on a boss", &boss, 1, None);
    assert!(volume(&ev) > v0 + 5.0, "a thread on a boss adds a ridge: {} vs {v0}", volume(&ev));
    let mut hole = Document::default();
    boxp(&mut hole, [-15.0, -15.0, 0.0], 30.0, 30.0, 20.0, Op::NewBody);
    let hid = first_body(&hole);
    hole.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(0.0), n(0.0)], dia: n(8.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    let h0 = volume(&run(&hole));
    hole.insert(FeatureKind::ThreadOn { body: hid, edge: EdgeRef::round([4.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0]), length: n(10.0), pitch: n(1.25), depth: n(0.6) });
    let ev = check("thread in a hole", &hole, 1, None);
    assert!(volume(&ev) < h0 - 5.0, "a thread in a hole cuts a groove: {} vs {h0}", volume(&ev));
    round_trip("thread in hole", &hole);
}

// ------------------------------------------------------------------------------------------------ touching bodies

#[test]
fn join_cut_intersect_boxes_sharing_a_face() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 10.0, 10.0, 10.0, Op::NewBody);
    let mut j = d.clone();
    boxp(&mut j, [10.0, 0.0, 0.0], 10.0, 10.0, 10.0, Op::Join);
    let ev = check("join sharing a face", &j, 1, Some((2000.0, 1e-6)));
    let tris = ev.model().bodies[0].mesh().triangles.len();
    assert!(tris <= 24, "the shared face must vanish from a joined box: {tris} triangles");
    let mut c = d.clone();
    boxp(&mut c, [10.0, 0.0, 0.0], 10.0, 10.0, 10.0, Op::Cut);
    let ev = run(&c);
    no_internal("cut sharing a face", &ev, &c);
    assert_eq!(ev.model().bodies.len(), 1, "a tool that only touches must not delete the body");
    assert!((volume(&ev) - 1000.0).abs() < 1e-6, "cut sharing a face changed volume to {}", volume(&ev));
    // coplanar faces inside: a cut flush with the top face
    let mut f = d.clone();
    boxp(&mut f, [2.0, 2.0, 5.0], 6.0, 6.0, 5.0, Op::Cut);
    check("pocket flush with the top", &f, 1, Some((1000.0 - 180.0, 1e-6)));
    let mut g = d.clone();
    boxp(&mut g, [2.0, 2.0, 0.0], 6.0, 6.0, 10.0, Op::Cut);
    check("through cut flush with top and bottom", &g, 1, Some((1000.0 - 360.0, 1e-6)));
    round_trip("flush cut", &g);
}

#[test]
fn intersect_boxes_that_only_touch() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 10.0, 10.0, 10.0, Op::NewBody);
    let uid = boxp(&mut d, [10.0, 0.0, 0.0], 10.0, 10.0, 10.0, Op::Intersect);
    let ev = run(&d);
    no_internal("intersect touching", &ev, &d);
    // nothing is shared: the user must be told, not have the body vanish silently
    assert!(ev.model().bodies.iter().all(|b| b.solid.volume() > 1e-9), "a zero-volume body was left in the model: {:?}", body_volumes(&ev));
    assert!(err_of(&ev, uid).is_some() || !ev.model().bodies.is_empty(), "intersecting boxes that only touch silently deleted the body");
}

#[test]
fn join_boxes_sharing_only_an_edge_or_a_corner() {
    for (at, name) in [([10.0, 10.0, 0.0], "edge"), ([10.0, 10.0, 10.0], "corner")] {
        let mut d = Document::default();
        boxp(&mut d, [0.0; 3], 10.0, 10.0, 10.0, Op::NewBody);
        boxp(&mut d, at, 10.0, 10.0, 10.0, Op::Join);
        let ev = run(&d);
        no_internal(name, &ev, &d);
        assert!(errors(&ev, &d).is_empty(), "join touching at an {name}: {:?}", errors(&ev, &d));
        assert!((volume(&ev) - 2000.0).abs() < 1e-6, "join touching at an {name}: volume {}", volume(&ev));
        // a body can be filleted afterwards without falling apart
        let id = ev.model().bodies[0].id;
        let mut f = d.clone();
        f.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [10.0, 0.0, 10.0])], radius: n(1.0) });
        let ev = run(&f);
        no_internal(name, &ev, &f);
        round_trip(name, &f);
    }
}

#[test]
fn cut_that_removes_the_whole_body() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 10.0, 10.0, 10.0, Op::NewBody);
    boxp(&mut d, [-1.0, -1.0, -1.0], 12.0, 12.0, 12.0, Op::Cut);
    let ev = run(&d);
    no_internal("cut everything", &ev, &d);
    assert!(ev.model().bodies.is_empty() || !errors(&ev, &d).is_empty(), "a cut through everything left {:?}", body_volumes(&ev));
    // the same through press-pull pushing a face further than the body is thick
    let mut p = Document::default();
    boxp(&mut p, [0.0; 3], 40.0, 30.0, 10.0, Op::NewBody);
    let uid = p.insert(FeatureKind::PressPull { plane: zplane(10.0), seed: [20.0, 15.0, 10.0], distance: n(-15.0) });
    let ev = run(&p);
    no_internal("push past the bottom", &ev, &p);
    assert!(err_of(&ev, uid).is_some() || ev.model().bodies.is_empty(), "push past the bottom left {:?}", body_volumes(&ev));
}

#[test]
fn cut_that_splits_a_body_makes_two_bodies() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 10.0, 10.0, Op::NewBody);
    boxp(&mut d, [15.0, -1.0, -1.0], 10.0, 12.0, 12.0, Op::Cut);
    check("cut splits a bar", &d, 2, Some((3000.0, 1e-6)));
}

#[test]
fn cut_that_splits_a_body_keeps_the_volume() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 10.0, 10.0, Op::NewBody);
    boxp(&mut d, [15.0, -1.0, -1.0], 10.0, 12.0, 12.0, Op::Cut);
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    assert!((volume(&ev) - 3000.0).abs() < 1e-6, "split bar volume {}", volume(&ev));
    assert!(ev.model().bodies.iter().all(closed));
    round_trip("split bar", &d);
}

#[test]
fn join_a_new_part_inside_the_bounding_box_but_not_touching() {
    // an L-shaped body and a cube in the empty corner of its bounding box: join must not glue them
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 10.0, 10.0, Op::NewBody);
    boxp(&mut d, [0.0, 0.0, 0.0], 10.0, 40.0, 10.0, Op::Join);
    boxp(&mut d, [25.0, 25.0, 0.0], 5.0, 5.0, 5.0, Op::Join);
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    assert!((volume(&ev) - (700.0 * 10.0 + 125.0)).abs() < 1e-6, "volume {}", volume(&ev));
}

#[test]
fn join_result_does_not_depend_on_bounding_boxes() {
    let count = |at: [f64; 3]| {
        let mut d = Document::default();
        boxp(&mut d, [0.0; 3], 40.0, 10.0, 10.0, Op::NewBody);
        boxp(&mut d, [0.0, 0.0, 0.0], 10.0, 40.0, 10.0, Op::Join);
        boxp(&mut d, at, 5.0, 5.0, 5.0, Op::Join);
        run(&d).model().bodies.len()
    };
    let (inside_bbox, outside_bbox) = (count([25.0, 25.0, 0.0]), count([55.0, 55.0, 0.0]));
    assert_eq!(inside_bbox, outside_bbox, "the same non-touching join gives {inside_bbox} bodies inside the bounding box and {outside_bbox} outside");
}

#[test]
fn cut_that_misses_is_reported_wherever_it_is() {
    // a cut that misses: an error outside the bbox, silently accepted inside it
    let cut = |at: [f64; 3]| {
        let mut d = Document::default();
        boxp(&mut d, [0.0; 3], 40.0, 10.0, 10.0, Op::NewBody);
        boxp(&mut d, [0.0, 0.0, 0.0], 10.0, 40.0, 10.0, Op::Join);
        let uid = boxp(&mut d, at, 5.0, 5.0, 5.0, Op::Cut);
        run(&d).error(uid).is_some()
    };
    assert_eq!(cut([25.0, 25.0, 0.0]), cut([55.0, 55.0, 0.0]), "a cut that touches nothing is reported only when it is outside the bounding box");
}

#[test]
fn split_plane_on_a_face_or_outside_and_a_u_shape() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    for off in [0.0, 10.0, -5.0, 50.0] {
        let mut x = d.clone();
        let uid = x.insert(FeatureKind::Split { bodies: vec![id], axis: Axis3::Z, offset: n(off) });
        expect_error(&format!("split at z={off}"), &x, uid);
    }
    // a U shape cut across its arms: the upper part is two separate arms
    let mut u = Document::default();
    boxp(&mut u, [0.0; 3], 30.0, 10.0, 5.0, Op::NewBody);
    boxp(&mut u, [0.0, 0.0, 5.0], 5.0, 10.0, 15.0, Op::Join);
    boxp(&mut u, [25.0, 0.0, 5.0], 5.0, 10.0, 15.0, Op::Join);
    let uid0 = first_body(&u);
    u.insert(FeatureKind::Split { bodies: vec![uid0], axis: Axis3::Z, offset: n(10.0) });
    let ev = run(&u);
    assert!(errors(&ev, &u).is_empty(), "{:?}", errors(&ev, &u));
    assert!((volume(&ev) - (1500.0 + 2.0 * 750.0)).abs() < 1e-6, "U split volume {}", volume(&ev));
    round_trip("U split", &u);
}

#[test]
fn repeated_body_in_a_selection_does_not_duplicate_material() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    let mut dup = d.clone();
    dup.insert(FeatureKind::Split { bodies: vec![id, id], axis: Axis3::Z, offset: n(4.0) });
    let ev = run(&dup);
    if errors(&ev, &dup).is_empty() {
        assert!((volume(&ev) - 4000.0).abs() < 1e-6, "split listing the body twice made volume {} from 4000 ({:?})", volume(&ev), body_volumes(&ev));
    }
    let mut pat = d.clone();
    pat.insert(FeatureKind::Pattern { bodies: vec![id, id], kind: PatternKind::Rect { nx: n(2.0), dx: n(30.0), ny: n(1.0), dy: n(0.0), nz: n(1.0), dz: n(0.0) } });
    let ev = run(&pat);
    assert!(ev.model().interferences().is_empty(), "pattern listing the body twice stacks copies on top of each other: {:?}", body_volumes(&ev));
}

#[test]
fn combine_edge_cases() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 10.0, 10.0, 10.0, Op::NewBody);
    boxp(&mut d, [50.0, 0.0, 0.0], 10.0, 10.0, 10.0, Op::NewBody);
    let ids: Vec<u64> = run(&d).model().bodies.iter().map(|b| b.id).collect();
    let mut x = d.clone();
    let uid = x.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[0]], op: BoolOp::Union, keep_tools: false });
    expect_error("combine a body with itself", &x, uid);
    let mut x = d.clone();
    let uid = x.insert(FeatureKind::Combine { target: ids[0], tools: vec![], op: BoolOp::Union, keep_tools: false });
    expect_error("combine without tools", &x, uid);
    let mut x = d.clone();
    let uid = x.insert(FeatureKind::Combine { target: 4242, tools: vec![ids[1]], op: BoolOp::Union, keep_tools: false });
    expect_error("combine a missing target", &x, uid);
    let mut x = d.clone();
    let uid = x.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[1]], op: BoolOp::Split, keep_tools: false });
    expect_error("split by a tool that does not touch", &x, uid);
    // union of separate bodies: one body, total volume
    let mut x = d.clone();
    x.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[1]], op: BoolOp::Union, keep_tools: false });
    check("union of separate bodies", &x, 1, Some((2000.0, 1e-6)));
    // intersect of separate bodies: nothing in common
    let mut x = d.clone();
    let uid = x.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[1]], op: BoolOp::Intersect, keep_tools: true });
    let ev = run(&x);
    no_internal("intersect of separate bodies", &ev, &x);
    assert!(err_of(&ev, uid).is_some() || ev.model().bodies.len() == 1, "intersect of separate bodies with keep_tools: {:?}", body_volumes(&ev));
    round_trip("combine union", &x);
}

#[test]
fn shell_does_not_touch_other_bodies_on_the_same_plane() {
    // two separate boxes with their tops on the same plane z = 20: shelling the first top must not hollow the second
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    boxp(&mut d, [100.0, 0.0, 0.0], 40.0, 30.0, 20.0, Op::NewBody);
    d.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(2.0), open: true, more: vec![] });
    let ev = check("shell beside a coplanar body", &d, 2, None);
    let vols = body_volumes(&ev);
    assert!((vols[1] - 24000.0).abs() < 1e-6, "the second body was shelled too: {vols:?}");
}

#[test]
fn shell_thickness_limits() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    for t in [0.0, -1.0, 10000.0] {
        let mut x = d.clone();
        let uid = x.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(t), open: true, more: vec![] });
        expect_error(&format!("shell thickness {t}"), &x, uid);
    }
    let mut x = d.clone();
    let uid = x.insert(FeatureKind::Shell { plane: zplane(55.0), thickness: n(2.0), open: true, more: vec![] });
    expect_error("shell with a face not on the body", &x, uid);
    let mut x = d.clone();
    x.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(0.1), open: true, more: vec![] });
    check("shell 0.1", &x, 1, Some((24000.0 - 39.8 * 29.8 * 19.9, 15.0)));
}

#[test]
fn open_shell_too_thick_is_an_error() {
    for t in [15.0, 20.0, 100.0] {
        let mut x = Document::default();
        boxp(&mut x, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
        let uid = x.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(t), open: true, more: vec![] });
        let ev = run(&x);
        // the closed shell of the same thickness reports "толщина слишком велика"
        assert!(err_of(&ev, uid).is_some(), "open shell t={t}: no error, body left untouched at volume {}", volume(&ev));
    }
}

#[test]
fn shell_open_bottom_face_with_outward_normal() {
    // the bottom face, given with its outward normal (-Z), is opened
    let bottom = Plane { origin: [0.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [1.0, 0.0, 0.0] };
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    d.insert(FeatureKind::Shell { plane: Plane { origin: [20.0, 15.0, 0.0], ..bottom }, thickness: n(2.0), open: true, more: vec![] });
    check("shell open at the bottom", &d, 1, Some((24000.0 - 36.0 * 26.0 * 18.0, 30.0)));
    round_trip("shell bottom", &d);
}

// ------------------------------------------------------------------------------------------------ parameters and history

#[test]
fn parameter_chain_rename_and_delete() {
    let mut d = Document::default();
    d.params.set("a", "10");
    d.params.set("b", "a * 2");
    d.params.set("c", "b + a");
    let bx = d.insert(FeatureKind::Primitive { shape: Shape::Box { w: Expr::from("c"), d: Expr::from("b"), h: Expr::from("a") }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    let hole = d.insert(FeatureKind::Hole { plane: Plane { origin: [0.0, 0.0, 10.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] }, at: [Expr::from("c / 2"), Expr::from("b / 2")], dia: Expr::from("a / 2"), depth: None, kind: HoleKind::Simple, more: vec![] });
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    let want = 6000.0 - PI * 6.25 * 10.0;
    assert!((volume(&ev) - want).abs() < 3.0, "chain volume {} vs {want}", volume(&ev));
    round_trip("parameter chain", &d);
    d.params.set("a", "5");
    ev.evaluate(&d);
    let want = 750.0 - PI * 1.5625 * 5.0;
    // the hole plane is fixed at z = 10 while the box is 5 high now: the hole still goes down through the box
    assert!((volume(&ev) - want).abs() < 2.0, "after a = 5: volume {} vs {want} ({:?})", volume(&ev), errors(&ev, &d));
    // rename: the old name disappears
    d.params.items.remove("a");
    d.params.set("aa", "10");
    ev.evaluate(&d);
    let e = err_of(&ev, bx).expect("a box using a renamed parameter must report it");
    assert!(e.contains('a'), "error names the parameter: {e}");
    assert!(ev.model().bodies.is_empty(), "nothing is built from a broken box");
    assert!(err_of(&ev, hole).is_some(), "the hole uses the missing parameter too");
    // the fresh evaluator agrees with the cached one
    let fresh = run(&d);
    assert_eq!(errors(&fresh, &d), errors(&ev, &d), "cached and fresh evaluation differ after a rename");
    // fix the references: everything rebuilds
    d.params.set("b", "aa * 2");
    d.params.set("c", "b + aa");
    if let FeatureKind::Primitive { shape: Shape::Box { h, .. }, .. } = &mut d.features[0].kind {
        *h = Expr::from("aa");
    }
    if let FeatureKind::Hole { dia, .. } = &mut d.features[1].kind {
        *dia = Expr::from("aa / 2");
    }
    ev.evaluate(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    assert!((volume(&ev) - (6000.0 - PI * 6.25 * 10.0)).abs() < 3.0);
    // a cycle is a clean error
    d.params.set("b", "c");
    ev.evaluate(&d);
    let e = err_of(&ev, bx).expect("cyclic parameters must fail");
    assert!(!e.contains(INTERNAL), "cycle: {e}");
}

#[test]
fn rollback_insert_in_the_middle_then_roll_forward() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(20.0), n(15.0)], dia: n(8.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    d.rollback = 1;
    ev.evaluate(&d);
    assert!((volume(&ev) - 24000.0).abs() < 1e-6, "rolled back volume {}", volume(&ev));
    let fil = d.insert(FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 20.0), radius: n(3.0) });
    assert_eq!(d.index_of(fil), Some(1), "inserted at the rollback marker");
    assert_eq!(d.rollback, 2);
    d.rollback = d.features.len();
    ev.evaluate(&d);
    let want = 24000.0 - PI * 16.0 * 20.0 - FIL * 9.0 * 140.0;
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    assert!((volume(&ev) - want).abs() < 45.0, "fillet inserted before the hole: {} vs {want}", volume(&ev));
    assert!((volume(&run(&d)) - volume(&ev)).abs() < 1e-6, "cached and fresh evaluation differ");
    round_trip("rollback insert", &d);
    // rollback past the end is clamped on load
    let mut j = d.clone();
    j.rollback = 99;
    let back = Document::from_json(&j.to_json()).unwrap();
    assert_eq!(back.rollback, back.features.len());
    let ev = run(&j);
    assert!((volume(&ev) - want).abs() < 45.0, "rollback past the end");
}

#[test]
fn deleting_a_feature_others_depend_on() {
    let mut d = Document::default();
    let sk = rect_sketch(&mut d, Plane::XY, [0.0, 0.0], [40.0, 30.0]);
    let ex = d.insert(ext(sk, [5.0, 5.0], 10.0, Op::NewBody));
    let body = first_body(&d);
    let fil = d.insert(FeatureKind::Fillet { body, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 10.0), radius: n(2.0) });
    let sh = d.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 10.0]), thickness: n(1.0), open: true, more: vec![] });
    check("full chain", &d, 1, None);
    // delete the sketch: the extrude is reported as dependent, everything after fails cleanly
    let mut x = d.clone();
    let deps = x.delete(sk);
    assert_eq!(deps, vec![ex], "the extrude depends on the sketch");
    let ev = run(&x);
    no_internal("delete sketch", &ev, &x);
    assert!(err_of(&ev, ex).is_some() && err_of(&ev, fil).is_some(), "dependent steps must fail: {:?}", errors(&ev, &x));
    assert!(ev.model().bodies.is_empty());
    round_trip("deleted sketch", &x);
    // delete the extrude: the fillet loses its body
    let mut x = d.clone();
    x.delete(ex);
    let ev = run(&x);
    no_internal("delete extrude", &ev, &x);
    assert!(err_of(&ev, fil).is_some(), "fillet without its body must fail");
    assert!(err_of(&ev, sh).is_some(), "shell without a body must fail");
    // delete the fillet in the middle: the shell applies to the plain box
    let mut x = d.clone();
    x.delete(fil);
    check("fillet deleted", &x, 1, Some((12000.0 - 38.0 * 28.0 * 9.0, 10.0)));
    // delete a feature uid that does not exist: nothing happens
    let mut x = d.clone();
    assert!(x.delete(12345).is_empty());
    assert_eq!(x, d);
}

#[test]
fn reordering_features_against_their_dependencies() {
    let mut d = Document::default();
    let sk = rect_sketch(&mut d, Plane::XY, [0.0, 0.0], [40.0, 30.0]);
    let ex = d.insert(ext(sk, [5.0, 5.0], 10.0, Op::NewBody));
    let body = first_body(&d);
    let fil = d.insert(FeatureKind::Fillet { body, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [40.0, 0.0, 10.0])], radius: n(2.0) });
    let good = volume(&check("ordered", &d, 1, None));
    // sketch after its extrude
    let mut x = d.clone();
    x.features.swap(0, 1);
    let ev = run(&x);
    no_internal("sketch after extrude", &ev, &x);
    assert!(err_of(&ev, ex).is_some(), "extrude before its sketch must fail");
    // fillet before the body exists
    let mut x = d.clone();
    let f = x.features.remove(2);
    x.features.insert(0, f);
    let ev = run(&x);
    no_internal("fillet first", &ev, &x);
    assert!(err_of(&ev, fil).is_some(), "fillet before its body must fail");
    // move them back with the same evaluator: the cache must not keep stale results
    let mut ev = Evaluator::default();
    ev.evaluate(&x);
    ev.evaluate(&d);
    assert!(errors(&ev, &d).is_empty(), "after restoring the order: {:?}", errors(&ev, &d));
    assert!((volume(&ev) - good).abs() < 1e-6, "restored order volume {} vs {good}", volume(&ev));
}

#[test]
fn cached_evaluation_matches_fresh_evaluation_through_edits() {
    let mut d = Document::default();
    d.params.set("w", "40");
    d.params.set("r", "2");
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: Expr::from("w"), d: n(30.0), h: n(20.0) }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    let id = first_body(&d);
    d.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(10.0), n(15.0)], dia: n(6.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, 20.0])], radius: Expr::from("r") });
    d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Rect { nx: n(2.0), dx: Expr::from("w + 10"), ny: n(1.0), dy: n(0.0), nz: n(1.0), dz: n(0.0) } });
    let mut ev = Evaluator::default();
    let mut step = |d: &Document, what: &str| {
        ev.evaluate(d);
        let fresh = run(d);
        assert_eq!(errors(&ev, d), errors(&fresh, d), "{what}: errors differ between cached and fresh evaluation");
        assert_eq!(ev.model().bodies.len(), fresh.model().bodies.len(), "{what}: body count differs");
        assert!((volume(&ev) - volume(&fresh)).abs() < 1e-6, "{what}: volume {} cached vs {} fresh", volume(&ev), volume(&fresh));
    };
    step(&d, "initial");
    d.params.set("r", "4");
    step(&d, "radius parameter");
    d.features[1].suppressed = true;
    step(&d, "suppress hole");
    d.rollback = 2;
    step(&d, "rollback to 2");
    d.rollback = 4;
    step(&d, "roll forward");
    d.features[1].suppressed = false;
    if let FeatureKind::Hole { dia, .. } = &mut d.features[1].kind {
        *dia = n(8.0);
    }
    step(&d, "edit hole");
    let pat = d.features[3].uid;
    d.delete(pat);
    step(&d, "delete pattern");
    d.params.set("w", "60");
    step(&d, "width parameter");
    round_trip("edited", &d);
}

#[test]
fn json_round_trip_of_a_feature_rich_document() {
    let mut d = Document::default();
    d.params.set("t", "2");
    d.views.push(NamedView { name: "iso".into(), yaw: 30.0, pitch: 20.0, distance: 200.0, target: [1.0, 2.0, 3.0] });
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    d.body_names.insert(id, "Корпус".into());
    d.hidden_bodies.insert(999);
    d.body_materials.insert(id, 3);
    let mut arc = EdgeRef::round([25.0, 15.0, 20.0], [20.0, 15.0, 20.0], [0.0, 0.0, 1.0]);
    arc.sweep = Some(90.0);
    d.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(20.0), n(15.0)], dia: n(10.0), depth: Some(n(12.0)), kind: HoleKind::Counterbore { dia: n(14.0), depth: n(2.0) }, more: vec![[n(8.0), n(8.0)]] });
    d.insert(FeatureKind::Chamfer { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 0.0], [40.0, 0.0, 0.0])], dist: n(1.0), second: ChamferSecond::Angle(n(30.0)), flip: true });
    d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 0.0], [0.0, 0.0, 20.0])], radius: n(2.0) });
    let arc_fillet = d.insert(FeatureKind::Fillet { body: id, edges: vec![arc], radius: n(0.5) });
    d.feature_mut(arc_fillet).unwrap().suppressed = true;
    d.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(1.0) });
    d.insert(FeatureKind::OffsetPlane { base: Plane::XY, offset: n(5.0) });
    d.insert(FeatureKind::AnglePlane { base: Plane::XZ, angle: n(30.0) });
    d.insert(FeatureKind::Scale { bodies: vec![id], factor: [n(1.0), n(1.0), n(1.0)], uniform: true });
    d.insert(FeatureKind::Move { bodies: vec![id], translate: p3(1.0, 2.0, 3.0), rotate: p3(0.0, 0.0, 90.0), copy: true });
    d.insert(FeatureKind::Shell { plane: Plane { origin: [0.0, 15.0, 10.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] }, thickness: Expr::from("t"), open: true, more: vec![click([20.0, 15.0, 20.0])] });
    let mut s = Sketch::default();
    s.add_line([0.0, 0.0], [10.0, 0.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::YZ, sketch: s });
    d.visible_sketches.insert(sk);
    d.insert(FeatureKind::Pipe { path: sk, outer: n(4.0), wall: n(0.5), op: Op::NewBody, shape: PipeShape::Square, corner: n(0.5), bend: n(0.0) });
    d.insert(FeatureKind::PlaceOnPlane { body: id, face: zplane(20.0), seed: [20.0, 15.0, 20.0], target: Plane::XY, to: Some([0.0, 0.0, 0.0]) });
    d.features[3].suppressed = true;
    let ev = run(&d);
    no_internal("feature-rich", &ev, &d);
    round_trip("feature-rich document", &d);
    // unknown format and future version are refused
    let mut j: serde_json_value::Value = serde_json_value::parse(&d.to_json());
    j.set_top("format", "\"other\"");
    assert!(Document::from_json(&j.0).is_err(), "a foreign format must not load");
    let mut j: serde_json_value::Value = serde_json_value::parse(&d.to_json());
    j.set_top("version", "99");
    assert!(Document::from_json(&j.0).is_err(), "a future version must not load");
    assert!(Document::from_json("{ not json").is_err());
}

/// Tiny helper to patch top-level JSON fields without adding a dependency to the test crate.
mod serde_json_value {
    pub struct Value(pub String);
    pub fn parse(s: &str) -> Value {
        Value(s.to_string())
    }
    impl Value {
        pub fn set_top(&mut self, key: &str, raw: &str) {
            let pat = format!("\n  \"{key}\": ");
            let i = self.0.find(&pat).expect("top-level key") + pat.len();
            let end = self.0[i..].find(",\n").expect("value end") + i;
            self.0.replace_range(i..end, raw);
        }
    }
}

#[test]
fn old_edge_refs_without_sweep_mean_a_full_circle() {
    let mut d = Document::default();
    cylp(&mut d, [0.0; 3], 15.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::round([15.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0])], radius: n(2.0) });
    let json = d.to_json();
    assert!(!json.contains("\"sweep\""), "a full circle is saved without a sweep field");
    let back = Document::from_json(&json).unwrap();
    assert!((volume(&run(&back)) - volume(&run(&d))).abs() < 1e-6);
}

// ------------------------------------------------------------------------------------------------ transforms

#[test]
fn scale_move_and_draft_limits() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    for f in [0.0, -1.0] {
        let mut x = d.clone();
        let uid = x.insert(FeatureKind::Scale { bodies: vec![id], factor: [n(f), n(1.0), n(1.0)], uniform: true });
        expect_error(&format!("scale {f}"), &x, uid);
    }
    let mut x = d.clone();
    x.insert(FeatureKind::Scale { bodies: vec![id], factor: [n(0.001), n(1.0), n(1.0)], uniform: true });
    check("scale 0.001", &x, 1, Some((4000.0 * 1e-9, 1e-12)));
    let mut x = d.clone();
    x.insert(FeatureKind::Scale { bodies: vec![id], factor: [n(100.0), n(-5.0), n(-5.0)], uniform: true });
    check("uniform scale ignores the other factors", &x, 1, Some((4000.0 * 1e6, 1.0)));
    for a in [45.0, -45.0, 90.0] {
        let mut x = d.clone();
        let uid = x.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(a) });
        expect_error(&format!("draft {a}"), &x, uid);
    }
    // a draft big enough to make the side faces cross before the top: 10 * tan 44 = 9.66 < 10, still fits
    let mut x = d.clone();
    x.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(44.0) });
    check("draft 44", &x, 1, None);
    // ... but on a taller body the faces cross: clean error or a closed body
    let mut t = Document::default();
    boxp(&mut t, [0.0; 3], 20.0, 20.0, 30.0, Op::NewBody);
    let tid = first_body(&t);
    let uid = t.insert(FeatureKind::Draft { bodies: vec![tid], axis: Axis3::Z, neutral: n(0.0), angle: n(30.0) });
    let ev = run(&t);
    no_internal("draft past apex", &ev, &t);
    if err_of(&ev, uid).is_none() {
        assert!(ev.model().bodies.iter().all(closed), "draft past apex: broken body");
        assert!(volume(&ev) < 12000.0 && volume(&ev) > 0.0, "draft past apex: volume {}", volume(&ev));
    }
    // move a body that does not exist
    let mut x = d.clone();
    let uid = x.insert(FeatureKind::Move { bodies: vec![777], translate: p3(1.0, 0.0, 0.0), rotate: p3(0.0, 0.0, 0.0), copy: false });
    expect_error("move a missing body", &x, uid);
    let mut x = d.clone();
    let uid = x.insert(FeatureKind::Move { bodies: vec![], translate: p3(1.0, 0.0, 0.0), rotate: p3(0.0, 0.0, 0.0), copy: false });
    expect_error("move nothing", &x, uid);
}

#[test]
fn draft_of_a_body_with_a_hole_and_a_cylinder() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(20.0), n(15.0)], dia: n(10.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    d.insert(FeatureKind::Draft { bodies: vec![id], axis: Axis3::Z, neutral: n(0.0), angle: n(3.0) });
    let ev = check("draft of a holed box", &d, 1, None);
    let i = 20.0 * 3f64.to_radians().tan();
    let outer = 20.0 / 3.0 * (1200.0 + (40.0 - 2.0 * i) * (30.0 - 2.0 * i) + (1200.0 * (40.0 - 2.0 * i) * (30.0 - 2.0 * i)).sqrt());
    // the hole widens upwards (the body narrows): a cone frustum from r 5 to r 5 + i
    let hole = PI * 20.0 / 3.0 * (25.0 + (5.0 + i).powi(2) + 5.0 * (5.0 + i));
    assert!((volume(&ev) - (outer - hole)).abs() < 10.0, "drafted holed box volume {} vs {}", volume(&ev), outer - hole);
    let mut c = Document::default();
    cylp(&mut c, [0.0; 3], 10.0, 20.0, Op::NewBody);
    let cid = first_body(&c);
    c.insert(FeatureKind::Draft { bodies: vec![cid], axis: Axis3::Z, neutral: n(0.0), angle: n(5.0) });
    let ev = check("draft of a cylinder", &c, 1, None);
    let r1 = 10.0 - 20.0 * 5f64.to_radians().tan();
    let cone = PI * 20.0 / 3.0 * (100.0 + r1 * r1 + 10.0 * r1);
    assert!((volume(&ev) - cone).abs() < cone * 0.01, "drafted cylinder {} vs cone {cone}", volume(&ev));
    round_trip("draft cylinder", &c);
}


#[test]
fn square_pipe_with_corner_equal_to_wall() {
    for (corner, l_path) in [(0.5, false), (0.5, true), (2.0, false)] {
        let mut d = Document::default();
        let mut s = Sketch::default();
        s.add_line([0.0, 0.0], [10.0, 0.0]);
        if l_path {
            s.add_line([10.0, 0.0], [10.0, 20.0]);
        }
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
        let uid = d.insert(FeatureKind::Pipe { path: sk, outer: n(4.0), wall: n(0.5), op: Op::NewBody, shape: PipeShape::Square, corner: n(corner), bend: n(0.0) });
        let ev = run(&d);
        // 0.4 and 0.6 build fine; exactly the wall (inner corner 0) or the half width must too
        assert!(err_of(&ev, uid).is_none(), "square pipe 4 mm, wall 0.5, corner {corner} (L path {l_path}): {:?}", err_of(&ev, uid));
        assert!(ev.model().bodies.iter().all(closed));
    }
}

#[test]
fn shell_of_a_holed_box_is_fast_enough() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    d.insert(FeatureKind::Hole { plane: zplane(20.0), at: [n(20.0), n(15.0)], dia: n(10.0), depth: Some(n(12.0)), kind: HoleKind::Simple, more: vec![[n(14.0), n(8.0)], [n(22.0), n(8.0)], [n(30.0), n(8.0)]] });
    d.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 0.0]), thickness: n(2.0), open: true, more: vec![] });
    let t = std::time::Instant::now();
    let ev = run(&d);
    let secs = t.elapsed().as_secs_f64();
    assert!(errors(&ev, &d).is_empty());
    let tris = ev.model().bodies[0].mesh().triangles.len();
    assert!(secs < 5.0, "shell of a box with 4 blind holes took {secs:.1} s ({tris} triangles)");
}

#[test]
fn deeper_combinations_after_edge_work() {
    // press-pull a side face of a box whose top edges are rounded
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 10.0, Op::NewBody);
    let id = first_body(&d);
    d.insert(FeatureKind::Fillet { body: id, edges: top_edges_box(0.0, 0.0, 40.0, 30.0, 10.0), radius: n(2.0) });
    let v0 = volume(&check("rounded top", &d, 1, None));
    let side = Plane { origin: [40.0, 0.0, 0.0], x: [0.0, 1.0, 0.0], y: [0.0, 0.0, 1.0] };
    let mut x = d.clone();
    x.insert(FeatureKind::PressPull { plane: side, seed: [40.0, 15.0, 4.0], distance: n(5.0) });
    let ev = check("pull the side of a rounded box", &x, 1, None);
    let (_, hi) = bounds(&ev);
    assert!((hi[0] - 45.0).abs() < 1e-3, "side pulled to x = {}", hi[0]);
    // the side face is 30 x 10 minus the fillet strip on top: the pull adds between 26 x 8 and 30 x 10 per mm
    let added = volume(&ev) - v0;
    assert!(added > 5.0 * 26.0 * 8.0 && added <= 5.0 * 300.0 + 1.0, "side pull added {added}");
    round_trip("side pull", &x);
    // a hole through the rounded edge zone, then a fillet of the pulled box
    let mut h = d.clone();
    h.insert(FeatureKind::Hole { plane: zplane(10.0), at: [n(1.0), n(15.0)], dia: n(3.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    check("hole through the fillet zone", &h, 1, None);
    // shell then a hole through the floor, then a cut through a wall by an extrude
    let mut s = Document::default();
    boxp(&mut s, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    s.insert(FeatureKind::Shell { plane: click([20.0, 15.0, 20.0]), thickness: n(2.0), open: true, more: vec![] });
    s.insert(FeatureKind::Hole { plane: zplane(2.0), at: [n(20.0), n(15.0)], dia: n(6.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    let shelled = 24000.0 - 36.0 * 26.0 * 18.0;
    check("hole in the floor of a shell", &s, 1, Some((shelled - PI * 9.0 * 2.0, 3.0)));
    let mut sk = Sketch::default();
    sk.add_rect([10.0, 5.0], [30.0, 15.0]);
    let skid = s.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: sk });
    s.insert(ext_kind(skid, [20.0, 10.0], Extent::All(true), Op::Cut));
    // the window goes through both long walls: 2 x 20 x 10 x 2
    check("window through a shell", &s, 1, Some((shelled - PI * 9.0 * 2.0 - 2.0 * 200.0 * 2.0, 4.0)));
    round_trip("window through a shell", &s);
    // revolve, then fillet the rim the revolve made
    let mut r = Document::default();
    let rs = rect_sketch(&mut r, Plane::XZ, [0.0, 0.0], [10.0, 20.0]);
    r.insert(FeatureKind::Revolve { sketch: rs, profiles: vec![ProfileRef::at([5.0, 10.0])], axis: RevolveAxis::SketchY, angle: n(360.0), op: Op::NewBody });
    let rid = first_body(&r);
    let rv = volume(&run(&r));
    r.insert(FeatureKind::Fillet { body: rid, edges: vec![EdgeRef::round([10.0, 0.0, 20.0], [0.0, 0.0, 20.0], [0.0, 0.0, 1.0])], radius: n(2.0) });
    check("fillet the rim of a revolved cylinder", &r, 1, Some((rv - FIL * 4.0 * 2.0 * PI * (10.0 - 0.4468), 3.0)));
    // mirror-join a filleted body about its own face: the rounded edges stay rounded
    let mut m = Document::default();
    boxp(&mut m, [0.0; 3], 20.0, 20.0, 10.0, Op::NewBody);
    let mid = first_body(&m);
    m.insert(FeatureKind::Fillet { body: mid, edges: vec![EdgeRef::line([0.0, 0.0, 10.0], [20.0, 0.0, 10.0])], radius: n(2.0) });
    m.insert(FeatureKind::Mirror { bodies: vec![mid], normal: Axis3::X, join: true, offset: n(20.0) });
    check("mirror join of a filleted body", &m, 1, Some((2.0 * (4000.0 - FIL * 4.0 * 20.0), 1.0)));
    round_trip("mirror join filleted", &m);
}

#[test]
fn sketch_driven_by_parameters_and_suppressed_sketch() {
    let mut d = Document::default();
    d.params.set("w", "40");
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [40.0, 30.0]);
    s.constraints.push(fusion_core::Constraint::Length(l[0], Expr::from("w")));
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    let ex = d.insert(ext(sk, [5.0, 5.0], 10.0, Op::NewBody));
    check("sketch with a parameter", &d, 1, Some((12000.0, 1e-3)));
    d.params.set("w", "60");
    check("sketch parameter changed", &d, 1, Some((18000.0, 1e-3)));
    // remove the parameter the sketch uses: the sketch and the extrude fail cleanly
    let mut x = d.clone();
    x.params.items.remove("w");
    let ev = run(&x);
    no_internal("sketch without its parameter", &ev, &x);
    assert!(err_of(&ev, sk).is_some() && err_of(&ev, ex).is_some(), "{:?}", errors(&ev, &x));
    // a zero or negative length constraint
    let mut z = d.clone();
    z.params.set("w", "0");
    let ev = run(&z);
    no_internal("zero sketch length", &ev, &z);
    assert!(err_of(&ev, ex).is_some() || ev.model().bodies.iter().all(|b| closed(b) && b.solid.volume() > 0.0), "zero-length sketch: {:?}", body_volumes(&ev));
    let mut z = d.clone();
    z.params.set("w", "-20");
    let ev = run(&z);
    no_internal("negative sketch length", &ev, &z);
    assert!(ev.model().bodies.iter().all(|b| closed(b) && b.solid.volume() > 0.0), "negative sketch length: {:?}", body_volumes(&ev));
    // suppressing the sketch fails the extrude; rollback 0 leaves an empty model
    let mut x = d.clone();
    x.feature_mut(sk).unwrap().suppressed = true;
    let ev = run(&x);
    assert!(err_of(&ev, ex).is_some(), "extrude of a suppressed sketch must fail");
    let mut x = d.clone();
    x.rollback = 0;
    let ev = run(&x);
    assert!(ev.model().bodies.is_empty() && errors(&ev, &x).is_empty());
    // a pattern count driven by an expression, rounded to the nearest whole number
    let mut p = Document::default();
    p.params.set("k", "2.5");
    boxp(&mut p, [0.0; 3], 2.0, 2.0, 2.0, Op::NewBody);
    let pid = first_body(&p);
    p.insert(FeatureKind::Pattern { bodies: vec![pid], kind: PatternKind::Rect { nx: Expr::from("k + 1"), dx: n(3.0), ny: n(1.0), dy: n(0.0), nz: n(1.0), dz: n(0.0) } });
    check("pattern count 3.5 rounds to 4", &p, 4, Some((32.0, 1e-6)));
    round_trip("parametric pattern", &p);
}

#[test]
fn fillet_and_chamfer_concave_circle_at_a_boss_base() {
    let mut d = Document::default();
    boxp(&mut d, [-20.0, -20.0, 0.0], 40.0, 40.0, 5.0, Op::NewBody);
    cylp(&mut d, [0.0, 0.0, 5.0], 6.0, 10.0, Op::Join);
    let ev = run(&d);
    let id = ev.model().bodies[0].id;
    let v0 = volume(&ev);
    let base = EdgeRef::round([6.0, 0.0, 5.0], [0.0, 0.0, 5.0], [0.0, 0.0, 1.0]);
    let mut f = d.clone();
    f.insert(FeatureKind::Fillet { body: id, edges: vec![base], radius: n(2.0) });
    // the added ring: area FIL*r^2 whose centroid sits 0.4468 r out from the boss wall
    let ring = FIL * 4.0 * 2.0 * PI * (6.0 + 0.4468);
    check("concave fillet at a boss base", &f, 1, Some((v0 + ring, 0.04 * ring + 1.0)));
    let mut c = d.clone();
    c.insert(FeatureKind::Chamfer { body: id, edges: vec![base], dist: n(2.0), second: ChamferSecond::Equal, flip: false });
    let tri = 2.0 * 2.0 * PI * (6.0 + 2.0 / 3.0);
    check("concave chamfer at a boss base", &c, 1, Some((v0 + tri, 0.04 * tri + 1.0)));
    round_trip("boss base fillet", &f);
}

#[test]
fn shell_of_a_body_away_from_the_origin() {
    // the clicked point is the plane origin; the body is far from the world origin
    let mut d = Document::default();
    boxp(&mut d, [100.0, 50.0, 0.0], 40.0, 30.0, 20.0, Op::NewBody);
    d.insert(FeatureKind::Shell { plane: click([120.0, 65.0, 20.0]), thickness: n(2.0), open: true, more: vec![] });
    check("shell of a box away from the origin", &d, 1, Some((24000.0 - 36.0 * 26.0 * 18.0, 30.0)));
}


/// Files saved before the shell kept the clicked point have a plane whose origin is not on the face (the foot of the
/// perpendicular from the world origin). They must still hollow the body that has a face on that plane.
#[test]
fn an_old_file_shell_with_the_origin_off_the_face_still_hollows_the_body() {
    use fusion_core::doc::Shape;
    let mut d = Document::default();
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(30.0), h: n(20.0) }, at: [n(100.0), n(100.0), n(0.0)], op: Op::NewBody });
    let uid = d.insert(FeatureKind::Shell { plane: Plane { origin: [0.0, 0.0, 20.0], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] }, thickness: n(2.0), open: true, more: vec![] });
    let mut ev = Evaluator::default();
    ev.evaluate(&d);
    assert_eq!(ev.error(uid), None);
    let v = ev.model().bodies[0].solid.volume();
    assert!(v < 24000.0 * 0.5 && v > 1000.0, "{v}");
}
