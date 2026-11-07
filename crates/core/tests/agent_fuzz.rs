//! Robustness of the modelling engine under random and stressful input.
//!
//! A deterministic pseudo-random fuzzer builds documents of random steps (primitives, sketches with extrude and revolve,
//! holes, fillets and chamfers on real edges, shells, press-pull on real faces, move/mirror/pattern/scale/split/combine,
//! parameters and history edits) and checks invariants after every step: no panic, every error a clean non-empty
//! message, every body a closed manifold with positive volume and finite coordinates, unique body ids, the cached
//! evaluator equal to a fresh one, and a JSON round trip that reproduces the model.
//!
//! Targeted tests attack recently added behaviour: faces and edges that follow earlier changes, cuts that split or miss
//! bodies, closed pipe paths and the T-junction triangulation. Tests marked `#[ignore = "BUG: ..."]` reproduce
//! confirmed engine bugs.
use fusion_core::csg::{BoolOp, Solid};
use fusion_core::doc::*;
use fusion_core::sketch::ProfileRef;
use fusion_core::{Document, Evaluator, Expr, FeatureKind, Mesh, Op, Plane, Sketch};
use glam::DVec3;
use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

/// The message the evaluator uses when it caught a panic inside a step.
const INTERNAL: &str = "внутренняя ошибка";

// ------------------------------------------------------------------------------------------------ helpers

fn n(v: f64) -> Expr {
    Expr::num(v)
}
fn p3(a: f64, b: f64, c: f64) -> [Expr; 3] {
    [n(a), n(b), n(c)]
}

fn panic_text(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default()
}

/// Evaluates with a panic guard of our own: a panic escaping the evaluator fails the test with its message.
fn run(d: &Document) -> Evaluator {
    let mut ev = Evaluator::default();
    if let Err(e) = catch_unwind(AssertUnwindSafe(|| ev.evaluate(d))) {
        panic!("evaluator panicked: {}", panic_text(&*e));
    }
    ev
}
fn errors(ev: &Evaluator, d: &Document) -> Vec<String> {
    d.features.iter().filter_map(|f| ev.error(f.uid).map(|e| format!("{}: {e}", f.kind.type_label()))).collect()
}
fn err_of(ev: &Evaluator, uid: u64) -> Option<String> {
    ev.error(uid).map(str::to_string)
}
fn volume(ev: &Evaluator) -> f64 {
    ev.model().bodies.iter().map(|b| b.solid.volume()).sum()
}
fn body_volumes(ev: &Evaluator) -> Vec<f64> {
    ev.model().bodies.iter().map(|b| b.solid.volume()).collect()
}
fn boxp(d: &mut Document, at: [f64; 3], w: f64, dd: f64, h: f64, op: Op) -> u64 {
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(w), d: n(dd), h: n(h) }, at: p3(at[0], at[1], at[2]), op })
}
fn zplane(z: f64) -> Plane {
    Plane { origin: [0.0, 0.0, z], x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] }
}
/// A face plane facing +Z whose origin is the point the user clicked on that face.
fn click(p: [f64; 3]) -> Plane {
    Plane { origin: p, x: [1.0, 0.0, 0.0], y: [0.0, 1.0, 0.0] }
}

/// Every directed edge is used exactly once and its reverse exactly once (exact index topology, as Manifold promises).
fn closed_by_index(m: &Mesh) -> bool {
    let mut edges: HashMap<(u32, u32), i32> = HashMap::new();
    for t in &m.triangles {
        for k in 0..3 {
            *edges.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    !edges.is_empty() && edges.iter().all(|(&(a, b), &c)| c == 1 && edges.get(&(b, a)) == Some(&1))
}
/// Every undirected edge is shared by exactly two triangles after welding points closer than 1e-4 (what the eye sees).
fn closed_welded(m: &Mesh) -> bool {
    let key = |v: [f64; 3]| ((v[0] * 1e4).round() as i64, (v[1] * 1e4).round() as i64, (v[2] * 1e4).round() as i64);
    let mut ids: HashMap<(i64, i64, i64), usize> = HashMap::new();
    let idx: Vec<usize> = m.vertices.iter().map(|v| { let next = ids.len(); *ids.entry(key(*v)).or_insert(next) }).collect();
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
fn bounds_of(m: &Mesh) -> ([f64; 3], [f64; 3]) {
    let (l, h) = m.bounds().expect("non-empty body");
    (l.to_array(), h.to_array())
}
/// Volume of `b` inside a 0.2 mm cube centred on `p`.
fn probe_volume(b: &Body, p: [f64; 3]) -> f64 {
    let h = 0.1;
    let cube = Solid::from_mesh(&fusion_core::geom::box_mesh(DVec3::from(p) - DVec3::splat(h), DVec3::from(p) + DVec3::splat(h))).expect("probe cube");
    b.solid.boolean(&cube, BoolOp::Intersect).volume()
}
fn inside(b: &Body, p: [f64; 3]) -> bool {
    probe_volume(b, p) > 0.004
}

thread_local! {
    /// The long run also uses the slow features (modeled threads, shells and fillets on dense bodies).
    static HEAVY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static SLIVERS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static INEXACT_JSON: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Invariants of one evaluated document: clean errors and sound bodies.
fn check_model(ev: &Evaluator, d: &Document) -> Result<(), String> {
    for f in d.features.iter().take(d.rollback) {
        if let Some(e) = ev.error(f.uid) {
            if e.trim().is_empty() {
                return Err(format!("step {} ({}) failed with an empty message", f.uid, f.kind.type_label()));
            }
            if e.contains(INTERNAL) {
                return Err(format!("step {} ({}) panicked inside the evaluator: {e}", f.uid, f.kind.type_label()));
            }
        }
    }
    let m = ev.model();
    let mut ids = HashSet::new();
    for b in &m.bodies {
        if !ids.insert(b.id) {
            return Err(format!("two bodies share the id {}", b.id));
        }
        let mesh = b.mesh();
        if mesh.vertices.iter().flatten().any(|c| !c.is_finite()) {
            return Err(format!("body {} has non-finite coordinates", b.id));
        }
        let v = b.solid.volume();
        if !(v.is_finite() && v > 0.0) {
            return Err(format!("body {} has volume {v}", b.id));
        }
        if !closed_by_index(mesh) {
            return Err(format!("body {} is not a closed manifold (index topology)", b.id));
        }
        // a mesh that is closed by index but not once points 1e-4 apart are welded has slivers: counted, not failed (curved
        // bodies legitimately get vertices that close after booleans)
        if !closed_welded(mesh) {
            SLIVERS.with(|c| c.set(c.get() + 1));
        }
    }
    Ok(())
}

/// The model a cached evaluator holds equals the one a fresh evaluator builds.
fn same_result(a: &Evaluator, b: &Evaluator, d: &Document, what: &str) -> Result<(), String> {
    let (ea, eb) = (errors(a, d), errors(b, d));
    if ea != eb {
        return Err(format!("{what}: errors differ: {ea:?} vs {eb:?}"));
    }
    let (ma, mb) = (a.model(), b.model());
    let ia: Vec<u64> = ma.bodies.iter().map(|b| b.id).collect();
    let ib: Vec<u64> = mb.bodies.iter().map(|b| b.id).collect();
    if ia != ib {
        return Err(format!("{what}: body ids differ: {ia:?} vs {ib:?}"));
    }
    for (x, y) in ma.bodies.iter().zip(mb.bodies.iter()) {
        let (vx, vy) = (x.solid.volume(), y.solid.volume());
        if (vx - vy).abs() > 1e-7 * vx.abs().max(1.0) {
            return Err(format!("{what}: body {} volume {vx} vs {vy}", x.id));
        }
        let ((lx, hx), (ly, hy)) = (bounds_of(x.mesh()), bounds_of(y.mesh()));
        if (0..3).any(|i| (lx[i] - ly[i]).abs() > 1e-6 || (hx[i] - hy[i]).abs() > 1e-6) {
            return Err(format!("{what}: body {} bounds differ", x.id));
        }
    }
    Ok(())
}

/// Straight sharp edges of a mesh with collinear pieces joined, as the app offers them for picking.
fn sharp_lines(mesh: &Mesh) -> Vec<(DVec3, DVec3)> {
    let mut groups: HashMap<[i64; 6], (DVec3, DVec3, Vec<(f64, f64)>)> = HashMap::new();
    for (i, j) in mesh.sharp_edges(20.0) {
        let (a, b) = (DVec3::from(mesh.vertices[i as usize]), DVec3::from(mesh.vertices[j as usize]));
        let mut d = (b - a).normalize_or_zero();
        if d == DVec3::ZERO {
            continue;
        }
        if d.x < -1e-9 || (d.x.abs() <= 1e-9 && (d.y < -1e-9 || (d.y.abs() <= 1e-9 && d.z < 0.0))) {
            d = -d;
        }
        let foot = a - d * a.dot(d);
        let key = [(d.x * 1e6).round() as i64, (d.y * 1e6).round() as i64, (d.z * 1e6).round() as i64, (foot.x * 1e4).round() as i64, (foot.y * 1e4).round() as i64, (foot.z * 1e4).round() as i64];
        let (ta, tb) = (a.dot(d), b.dot(d));
        groups.entry(key).or_insert((d, foot, Vec::new())).2.push((ta.min(tb), ta.max(tb)));
    }
    let mut keys: Vec<[i64; 6]> = groups.keys().copied().collect();
    keys.sort();
    let mut out = Vec::new();
    for k in keys {
        let (d, foot, mut spans) = groups.remove(&k).unwrap();
        spans.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut cur = spans[0];
        for s in spans.into_iter().skip(1) {
            if s.0 <= cur.1 + 1e-6 {
                cur.1 = cur.1.max(s.1);
            } else {
                out.push((foot + d * cur.0, foot + d * cur.1));
                cur = s;
            }
        }
        out.push((foot + d * cur.0, foot + d * cur.1));
    }
    out
}

// ------------------------------------------------------------------------------------------------ random source

/// xorshift64: tiny, deterministic, no dependencies.
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        let mut r = Rng((seed.wrapping_add(1)).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        for _ in 0..4 {
            r.next();
        }
        r
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    /// A value on a coarse grid: coincident faces and aligned vertices are where booleans and triangulation break.
    fn grid(&mut self, lo: f64, hi: f64, step: f64) -> f64 {
        let v = (self.range(lo, hi) / step).round() * step;
        if v == 0.0 { 0.0 } else { v }
    }
    fn below(&mut self, k: usize) -> usize {
        (self.next() % k.max(1) as u64) as usize
    }
    fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    fn pick<'a, T>(&mut self, v: &'a [T]) -> &'a T {
        &v[self.below(v.len())]
    }
}

// ------------------------------------------------------------------------------------------------ fuzzer

/// Bounds of every body together, or a default box when the model is empty.
fn model_bounds(ev: &Evaluator) -> (DVec3, DVec3) {
    let m = ev.model();
    let mut lo = DVec3::splat(f64::MAX);
    let mut hi = DVec3::splat(f64::MIN);
    for b in &m.bodies {
        if let Some((l, h)) = b.mesh().bounds() {
            lo = lo.min(l);
            hi = hi.max(h);
        }
    }
    if lo.x > hi.x { (DVec3::ZERO, DVec3::new(40.0, 30.0, 20.0)) } else { (lo, hi) }
}

/// A random planar face of the body: the plane (normal outward, origin at the picked point) and the picked point.
fn pick_face(rng: &mut Rng, mesh: &Mesh) -> Option<(Plane, [f64; 3])> {
    let groups = mesh.planar_faces();
    let mut area: HashMap<u32, (f64, usize, f64)> = HashMap::new();
    for (i, t) in mesh.triangles.iter().enumerate() {
        let [a, b, c] = mesh.tri(t);
        let ar = (b - a).cross(c - a).length() * 0.5;
        let e = area.entry(groups[i]).or_insert((0.0, i, 0.0));
        e.0 += ar;
        if ar > e.2 {
            e.1 = i;
            e.2 = ar;
        }
    }
    let mut faces: Vec<(u32, (f64, usize, f64))> = area.into_iter().filter(|(_, (a, _, _))| *a > 0.5).collect();
    faces.sort_by_key(|f| f.0);
    if faces.is_empty() {
        return None;
    }
    let total: f64 = faces.iter().map(|f| f.1 .0).sum();
    let mut x = rng.range(0.0, total);
    let mut tri = faces[0].1 .1;
    for f in &faces {
        if x <= f.1 .0 {
            tri = f.1 .1;
            break;
        }
        x -= f.1 .0;
    }
    let [a, b, c] = mesh.tri(&mesh.triangles[tri]);
    let nrm = (b - a).cross(c - a).normalize_or_zero();
    if nrm == DVec3::ZERO {
        return None;
    }
    let seed = (a + b + c) / 3.0;
    let pl = Plane::from_point_normal(seed, nrm);
    Some((Plane { origin: seed.to_array(), ..pl }, seed.to_array()))
}

fn pick_edges(rng: &mut Rng, mesh: &Mesh) -> Vec<EdgeRef> {
    let lines: Vec<(DVec3, DVec3)> = sharp_lines(mesh).into_iter().filter(|(a, b)| a.distance(*b) > 1.0).collect();
    if lines.is_empty() {
        return vec![];
    }
    let k = 1 + rng.below(3);
    let mut out = Vec::new();
    // sometimes edges that meet at a corner, sometimes any
    let first = *rng.pick(&lines);
    out.push(EdgeRef::line(first.0.to_array(), first.1.to_array()));
    for _ in 1..k {
        let next = if rng.chance(0.5) {
            lines.iter().filter(|(a, b)| [first.0, first.1].iter().any(|p| p.distance(*a) < 1e-6 || p.distance(*b) < 1e-6) && (*a, *b) != first).copied().collect::<Vec<_>>()
        } else {
            lines.clone()
        };
        if next.is_empty() {
            break;
        }
        let l = *rng.pick(&next);
        let e = EdgeRef::line(l.0.to_array(), l.1.to_array());
        if !out.contains(&e) {
            out.push(e);
        }
    }
    // the app now stores the normals of the two faces with every straight edge; old files have none
    if rng.chance(0.75) {
        out.iter_mut().for_each(|e| e.faces = e.face_normals(mesh));
    }
    out
}

fn random_op(rng: &mut Rng, has_bodies: bool) -> Op {
    if !has_bodies {
        return Op::NewBody;
    }
    *rng.pick(&[Op::NewBody, Op::Join, Op::Join, Op::Cut, Op::Cut, Op::Cut, Op::Intersect])
}

/// A size: mostly a plain number on a grid, sometimes an expression through the document parameter `s`.
fn size(rng: &mut Rng, lo: f64, hi: f64) -> Expr {
    if rng.chance(0.15) {
        Expr(format!("s*{}", fusion_core::expr::format_num(rng.grid(0.2, 1.5, 0.1))))
    } else {
        n(rng.grid(lo, hi, 0.5).max(0.5))
    }
}

fn axis(rng: &mut Rng) -> Axis3 {
    *rng.pick(&[Axis3::X, Axis3::Y, Axis3::Z])
}

/// Inserts one random step (or a sketch and the step that uses it). Bodies, faces and edges come from the current model.
fn random_step(rng: &mut Rng, d: &mut Document, ev: &Evaluator) -> &'static str {
    let m = ev.model();
    let has = !m.bodies.is_empty();
    let (lo, hi) = model_bounds(ev);
    let span = (hi - lo).max(DVec3::splat(5.0));
    let near = |rng: &mut Rng, k: usize| (lo[k] + rng.range(-0.2, 1.0) * span[k]).round();
    let body = |rng: &mut Rng| m.bodies[rng.below(m.bodies.len())].clone();
    let choice = if has { rng.below(22) } else { rng.below(4) };
    match choice {
        0 | 1 => {
            let at = [near(rng, 0) - 5.0, near(rng, 1) - 5.0, near(rng, 2) - 5.0];
            let shape = match rng.below(6) {
                0..=2 => Shape::Box { w: size(rng, 1.0, 40.0), d: size(rng, 1.0, 30.0), h: size(rng, 1.0, 25.0) },
                3 => Shape::Cylinder { r: size(rng, 1.0, 12.0), h: size(rng, 1.0, 25.0) },
                4 => Shape::Sphere { r: size(rng, 1.0, 12.0) },
                _ => {
                    let major = rng.grid(4.0, 15.0, 0.5);
                    Shape::Torus { major: n(major), minor: n(rng.grid(0.5, major - 0.5, 0.5)) }
                }
            };
            d.insert(FeatureKind::Primitive { shape, at: [n(at[0]), n(at[1]), n(at[2])], op: random_op(rng, has) });
            "primitive"
        }
        2 | 3 => {
            // a sketch of a rectangle (sometimes with holes) or a circle on a principal plane at some offset, extruded
            let (plane, k) = match rng.below(3) {
                0 => (Plane { origin: [0.0, 0.0, rng.grid(lo.z, hi.z, 1.0)], ..Plane::XY }, [0usize, 1usize]),
                1 => (Plane { origin: [0.0, rng.grid(lo.y, hi.y, 1.0), 0.0], ..Plane::XZ }, [0, 2]),
                _ => (Plane { origin: [rng.grid(lo.x, hi.x, 1.0), 0.0, 0.0], ..Plane::YZ }, [1, 2]),
            };
            let mut s = Sketch::default();
            let x0 = rng.grid(lo[k[0]] - 5.0, hi[k[0]], 1.0);
            let y0 = rng.grid(lo[k[1]] - 5.0, hi[k[1]], 1.0);
            let w = rng.grid(2.0, span[k[0]] + 10.0, 1.0).max(2.0);
            let h = rng.grid(2.0, span[k[1]] + 10.0, 1.0).max(2.0);
            let seed;
            if rng.chance(0.75) {
                s.add_rect([x0, y0], [x0 + w, y0 + h]);
                seed = [x0 + w * 0.05 + 0.05, y0 + h * 0.05 + 0.05];
                for _ in 0..rng.below(4) {
                    // holes kept away from the seed corner; aligned to the grid on purpose
                    let (hx, hy) = (x0 + rng.grid(w * 0.25, w * 0.75, 0.5), y0 + rng.grid(h * 0.25, h * 0.75, 0.5));
                    let r = rng.grid(0.5, (w.min(h) * 0.2).max(0.6), 0.5);
                    if rng.chance(0.5) {
                        s.add_circle([hx, hy], r);
                    } else {
                        s.add_rect([hx - r, hy - r], [hx + r, hy + r]);
                    }
                }
            } else {
                let r = rng.grid(1.0, w.min(h) / 2.0 + 1.0, 0.5);
                s.add_circle([x0 + r, y0 + r], r);
                seed = [x0 + r, y0 + r];
            }
            let sk = d.insert(FeatureKind::Sketch { plane, sketch: s });
            let dist = rng.grid(-20.0, 25.0, 0.5);
            let extent = match rng.below(8) {
                0 => Extent::Symmetric(n(dist.abs().max(1.0))),
                1 => Extent::TwoSides(n(rng.grid(1.0, 10.0, 0.5)), n(rng.grid(1.0, 10.0, 0.5))),
                2 => Extent::All(rng.chance(0.5)),
                _ => Extent::OneSide(n(if dist.abs() < 0.5 { 5.0 } else { dist })),
            };
            let taper = if rng.chance(0.15) { rng.grid(-10.0, 10.0, 1.0) } else { 0.0 };
            let thin = if rng.chance(if HEAVY.with(|h| h.get()) { 0.15 } else { 0.06 }) { rng.grid(0.5, 3.0, 0.5) } else { 0.0 };
            let start = if rng.chance(0.15) { rng.grid(-5.0, 5.0, 0.5) } else { 0.0 };
            d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at(seed)], extent, op: random_op(rng, has), face: None, taper: n(taper), thin: n(thin), start: n(start) });
            "sketch+extrude"
        }
        4 => {
            let mut s = Sketch::default();
            let r0 = rng.grid(0.0, 15.0, 1.0);
            let z0 = rng.grid(lo.z - 5.0, hi.z, 1.0);
            let (w, h) = (rng.grid(1.0, 10.0, 1.0), rng.grid(1.0, 15.0, 1.0));
            s.add_rect([r0, z0], [r0 + w, z0 + h]);
            let sk = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: s });
            let free = rng.grid(10.0, 350.0, 5.0);
            let angle = *rng.pick(&[90.0, 180.0, 270.0, 360.0, 360.0, free]);
            d.insert(FeatureKind::Revolve { sketch: sk, profiles: vec![ProfileRef::at([r0 + w / 2.0, z0 + h / 2.0])], axis: RevolveAxis::SketchY, angle: n(angle), op: random_op(rng, has) });
            "sketch+revolve"
        }
        5 | 6 => {
            let b = body(rng);
            let Some((plane, _)) = pick_face(rng, b.mesh()) else { return "skip" };
            let dia = rng.grid(1.0, 8.0, 0.5);
            let kind = match rng.below(6) {
                0 => HoleKind::Counterbore { dia: n(dia + 2.0), depth: n(rng.grid(0.5, 3.0, 0.5)) },
                1 => HoleKind::Countersink { dia: n(dia + 2.0), angle: n(90.0) },
                2 => HoleKind::Tapped { major: n(dia + 1.0), pitch: n(1.0), modeled: HEAVY.with(|h| h.get()) && rng.chance(0.2) },
                _ => HoleKind::Simple,
            };
            let depth = if rng.chance(0.5) { None } else { Some(n(rng.grid(1.0, 15.0, 0.5))) };
            let more = if rng.chance(0.2) { vec![[n(rng.grid(-8.0, 8.0, 1.0)), n(rng.grid(-8.0, 8.0, 1.0))]] } else { vec![] };
            d.insert(FeatureKind::Hole { plane, at: [n(0.0), n(0.0)], dia: n(dia), depth, kind, more });
            "hole"
        }
        7 | 8 | 9 => {
            let b = body(rng);
            if b.mesh().triangles.len() > if HEAVY.with(|h| h.get()) { 4000 } else { 1500 } {
                return "skip";
            }
            let edges = pick_edges(rng, b.mesh());
            if edges.is_empty() {
                return "skip";
            }
            let v = rng.grid(0.5, 4.0, 0.5);
            if rng.chance(0.5) {
                d.insert(FeatureKind::Fillet { body: b.id, edges, radius: n(v) });
                "fillet"
            } else {
                let second = match rng.below(4) {
                    0 => ChamferSecond::Distance(n(rng.grid(0.5, 3.0, 0.5))),
                    1 => ChamferSecond::Angle(n(rng.grid(20.0, 70.0, 5.0))),
                    _ => ChamferSecond::Equal,
                };
                d.insert(FeatureKind::Chamfer { body: b.id, edges, dist: n(v), second, flip: rng.chance(0.3) });
                "chamfer"
            }
        }
        10 => {
            let b = body(rng);
            if b.mesh().triangles.len() > if HEAVY.with(|h| h.get()) { 1500 } else { 400 } {
                return "skip";
            }
            let Some((plane, _)) = pick_face(rng, b.mesh()) else { return "skip" };
            d.insert(FeatureKind::Shell { plane, thickness: n(rng.grid(0.5, 2.5, 0.5)), open: rng.chance(0.85), more: vec![] });
            "shell"
        }
        11 | 12 => {
            let b = body(rng);
            let Some((plane, seed)) = pick_face(rng, b.mesh()) else { return "skip" };
            let dist = rng.grid(-8.0, 8.0, 0.5);
            let dist = if dist == 0.0 { 2.0 } else { dist };
            if rng.chance(0.5) {
                d.insert(FeatureKind::PressPull { plane, seed, distance: n(dist) });
                "press-pull"
            } else {
                let extent = if rng.chance(0.8) { Extent::OneSide(n(dist)) } else { Extent::Symmetric(n(dist.abs())) };
                d.insert(FeatureKind::Extrude { sketch: 0, profiles: vec![], extent, op: Op::Join, face: Some(FaceRef { plane, seed }), taper: n(0.0), thin: n(0.0), start: n(0.0) });
                "face extrude"
            }
        }
        13 => {
            let b = body(rng);
            let rot = if rng.chance(0.3) { p3(0.0, 0.0, *rng.pick(&[90.0, 45.0, 180.0])) } else { p3(0.0, 0.0, 0.0) };
            d.insert(FeatureKind::Move { bodies: vec![b.id], translate: p3(rng.grid(-20.0, 20.0, 1.0), rng.grid(-20.0, 20.0, 1.0), rng.grid(-10.0, 10.0, 1.0)), rotate: rot, copy: rng.chance(0.4) });
            "move"
        }
        14 => {
            let b = body(rng);
            let ax = axis(rng);
            let (l, h) = bounds_of(b.mesh());
            let off = rng.grid(l[ax as usize] - 5.0, h[ax as usize] + 5.0, 0.5);
            d.insert(FeatureKind::Mirror { bodies: vec![b.id], normal: ax, join: rng.chance(0.5), offset: n(off) });
            "mirror"
        }
        15 => {
            let b = body(rng);
            let kind = if rng.chance(0.6) {
                PatternKind::Rect { nx: n((1 + rng.below(3)) as f64), dx: n(rng.grid(-30.0, 30.0, 1.0)), ny: n((1 + rng.below(2)) as f64), dy: n(rng.grid(-30.0, 30.0, 1.0)), nz: n(1.0), dz: n(0.0) }
            } else {
                PatternKind::Circular { count: n((2 + rng.below(3)) as f64), axis: axis(rng), angle: n(*rng.pick(&[360.0, 90.0, 180.0])) }
            };
            d.insert(FeatureKind::Pattern { bodies: vec![b.id], kind });
            "pattern"
        }
        16 => {
            let b = body(rng);
            let uniform = rng.chance(0.6);
            let f = [rng.grid(0.5, 2.0, 0.25), rng.grid(0.5, 2.0, 0.25), rng.grid(0.5, 2.0, 0.25)];
            d.insert(FeatureKind::Scale { bodies: vec![b.id], factor: p3(f[0], f[1], f[2]), uniform });
            "scale"
        }
        17 => {
            let b = body(rng);
            let ax = axis(rng);
            let (l, h) = bounds_of(b.mesh());
            d.insert(FeatureKind::Split { bodies: vec![b.id], axis: ax, offset: n(rng.grid(l[ax as usize] - 2.0, h[ax as usize] + 2.0, 0.5)) });
            "split"
        }
        18 => {
            if m.bodies.len() < 2 {
                return "skip";
            }
            let t = body(rng);
            let tools: Vec<u64> = m.bodies.iter().filter(|b| b.id != t.id && rng.chance(0.6)).map(|b| b.id).collect();
            let tools = if tools.is_empty() { vec![m.bodies.iter().find(|b| b.id != t.id).unwrap().id] } else { tools };
            d.insert(FeatureKind::Combine { target: t.id, tools, op: *rng.pick(&BoolOp::ALL), keep_tools: rng.chance(0.3) });
            "combine"
        }
        19 => {
            // a pipe along an open chain or a closed loop of lines
            let z = rng.grid(lo.z, hi.z, 1.0);
            let mut s = Sketch::default();
            let k = 2 + rng.below(4);
            let pts: Vec<[f64; 2]> = (0..k).map(|_| [rng.grid(lo.x - 5.0, hi.x + 5.0, 1.0), rng.grid(lo.y - 5.0, hi.y + 5.0, 1.0)]).collect();
            let closed = k >= 3 && rng.chance(0.5);
            let ids: Vec<usize> = pts.iter().map(|p| s.add_point(*p)).collect();
            for i in 0..k - 1 {
                s.add(fusion_core::Entity::Line { a: ids[i], b: ids[i + 1], construction: false });
            }
            if closed {
                s.add(fusion_core::Entity::Line { a: ids[k - 1], b: ids[0], construction: false });
            }
            let sk = d.insert(FeatureKind::Sketch { plane: zplane(z), sketch: s });
            let outer = rng.grid(1.0, 6.0, 0.5);
            let wall = if rng.chance(0.4) { 0.0 } else { rng.grid(0.25, outer / 2.0 - 0.25, 0.25).max(0.25) };
            d.insert(FeatureKind::Pipe { path: sk, outer: n(outer), wall: n(wall), op: random_op(rng, has), shape: if rng.chance(0.3) { PipeShape::Square } else { PipeShape::Round }, corner: n(0.0), bend: n(if rng.chance(0.5) { rng.grid(0.5, 6.0, 0.5) } else { 0.0 }) });
            "pipe"
        }
        20 => {
            // a parameter or an earlier step changes: faces and edges picked later must follow or fail cleanly
            if rng.chance(0.5) {
                let s = rng.grid(3.0, 30.0, 1.0);
                d.params.set("s", s.to_string().as_str());
                return "param edit";
            }
            let cands: Vec<usize> = d.features.iter().enumerate().filter(|(_, f)| matches!(f.kind, FeatureKind::Primitive { .. } | FeatureKind::Extrude { face: None, .. })).map(|(i, _)| i).collect();
            if cands.is_empty() {
                return "skip";
            }
            let i = *rng.pick(&cands);
            let factor = rng.grid(0.5, 2.0, 0.25);
            let params = params_of(d);
            match &mut d.features[i].kind {
                FeatureKind::Primitive { shape: Shape::Box { h, .. }, .. } | FeatureKind::Primitive { shape: Shape::Cylinder { h, .. }, .. } => {
                    if let Ok(v) = h.eval(&params) {
                        *h = n((v * factor).max(0.5));
                    }
                }
                FeatureKind::Primitive { at, .. } => at[0] = n(at[0].eval(&params).unwrap_or(0.0) + 3.0),
                FeatureKind::Extrude { extent: Extent::OneSide(e), .. } => {
                    if let Ok(v) = e.eval(&params) {
                        *e = n(v * factor);
                    }
                }
                _ => {}
            }
            "history edit"
        }
        _ => {
            match rng.below(4) {
                0 => {
                    let b = body(rng);
                    d.insert(FeatureKind::Remove { bodies: vec![b.id] });
                    "remove"
                }
                1 => {
                    let b = body(rng);
                    let (l, h) = bounds_of(b.mesh());
                    d.insert(FeatureKind::Draft { bodies: vec![b.id], axis: Axis3::Z, neutral: n(l[2]), angle: n(rng.grid(-10.0, 10.0, 1.0)) });
                    let _ = h;
                    "draft"
                }
                2 => {
                    let mut a = Sketch::default();
                    a.add_circle([lo.x + 5.0, lo.y + 5.0], rng.grid(2.0, 8.0, 0.5));
                    let sa = d.insert(FeatureKind::Sketch { plane: zplane(hi.z), sketch: a });
                    let mut b = Sketch::default();
                    let r = rng.grid(1.0, 6.0, 0.5);
                    b.add_rect([lo.x + 5.0 - r, lo.y + 5.0 - r], [lo.x + 5.0 + r, lo.y + 5.0 + r]);
                    let sb = d.insert(FeatureKind::Sketch { plane: zplane(hi.z + rng.grid(2.0, 15.0, 1.0)), sketch: b });
                    d.insert(FeatureKind::Loft { profiles: vec![(sa, ProfileRef::at([lo.x + 5.0, lo.y + 5.0])), (sb, ProfileRef::at([lo.x + 5.0, lo.y + 5.0]))], op: random_op(rng, true) });
                    "loft"
                }
                _ => {
                    let b = body(rng);
                    let Some((face, seed)) = pick_face(rng, b.mesh()) else { return "skip" };
                    d.insert(FeatureKind::PlaceOnPlane { body: b.id, face, seed, target: Plane { origin: [0.0, 0.0, rng.grid(-10.0, 10.0, 1.0)], ..Plane::XY }, to: None });
                    "place on plane"
                }
            }
        }
    }
}

fn params_of(d: &Document) -> fusion_core::Params {
    d.params.clone()
}

/// One fuzz case: builds a document step by step and checks every invariant. Returns the document and a failure.
fn fuzz_case(seed: u64, max_steps: usize, timings: &mut Vec<(f64, u64, &'static str)>) -> (Document, Result<(), String>) {
    let mut rng = Rng::new(seed);
    let mut d = Document::default();
    d.params.set("s", rng.grid(5.0, 25.0, 1.0).to_string().as_str());
    let mut ev = Evaluator::default();
    let steps = 3 + rng.below(max_steps.saturating_sub(2).max(1));
    for k in 0..steps {
        let what = match catch_unwind(AssertUnwindSafe(|| random_step(&mut rng, &mut d, &ev))) {
            Ok(w) => w,
            Err(e) => return (d, Err(format!("seed {seed} step {k}: the fuzzer itself panicked: {}", panic_text(&*e)))),
        };
        let t = Instant::now();
        if let Err(e) = catch_unwind(AssertUnwindSafe(|| ev.evaluate(&d))) {
            return (d, Err(format!("seed {seed} step {k} ({what}): evaluator panicked: {}", panic_text(&*e))));
        }
        timings.push((t.elapsed().as_secs_f64(), seed, what));
        if let Err(e) = check_model(&ev, &d) {
            return (d, Err(format!("seed {seed} step {k} ({what}): {e}")));
        }
        // now and then look at an earlier state of the history and come back
        if rng.chance(0.1) && d.features.len() > 1 {
            let keep = d.rollback;
            d.rollback = rng.below(d.features.len());
            if let Err(e) = catch_unwind(AssertUnwindSafe(|| ev.evaluate(&d))) {
                return (d, Err(format!("seed {seed} step {k}: evaluator panicked on rollback: {}", panic_text(&*e))));
            }
            if let Err(e) = check_model(&ev, &d) {
                let rb = d.rollback;
                return (d, Err(format!("seed {seed} step {k} (rollback to {rb}): {e}")));
            }
            d.rollback = keep;
            ev.evaluate(&d);
        }
    }
    // one fresh evaluation serves two checks: it is of the saved and reloaded document, and it must equal the cached one
    let json = d.to_json();
    let back = match Document::from_json(&json) {
        Ok(b) => b,
        Err(e) => return (d, Err(format!("seed {seed}: saved document does not load: {e}"))),
    };
    // serde_json without `float_roundtrip` may read a coordinate back one ULP off: see
    // `json_round_trip_keeps_picked_coordinates_exactly`; here only the model has to survive
    if back != d {
        INEXACT_JSON.with(|c| c.set(c.get() + 1));
    }
    let again = run(&back);
    if same_result(&ev, &again, &d, "").is_err() {
        // tell a caching problem from a save-and-load problem
        let fresh = run(&d);
        if let Err(e) = same_result(&ev, &fresh, &d, &format!("seed {seed}: cached vs fresh evaluation")) {
            return (d, Err(e));
        }
        if let Err(e) = same_result(&fresh, &again, &d, &format!("seed {seed}: JSON round trip")) {
            return (d, Err(e));
        }
    }
    (d, Ok(()))
}

/// A failure that one of the dedicated `#[ignore = "BUG: ..."]` tests already reproduces: counted, not failed, so that the
/// fuzzer keeps looking for new problems.
fn known_bug(d: &Document, msg: &str) -> Option<&'static str> {
    let last = d.features.iter().take(d.rollback).last().map(|f| &f.kind);
    if msg.contains("has volume") && d.features.iter().take(d.rollback).any(|f| matches!(&f.kind, FeatureKind::Combine { op: BoolOp::Intersect | BoolOp::Split, .. })) {
        let tiny = msg.rsplit("has volume ").next().and_then(|v| v.trim().parse::<f64>().ok()).is_some_and(|v| v.abs() < 1e-6);
        if tiny {
            return Some("combine of touching bodies leaves an empty body (combine_of_touching_bodies_leaves_no_empty_body)");
        }
    }
    if msg.contains("has volume -") && d.features.iter().take(d.rollback).any(|f| matches!(&f.kind, FeatureKind::Extrude { taper, .. } if taper.0 != "0")) {
        // an inside-out region left by a closing taper shows up at a later cut
        if !matches!(last, Some(FeatureKind::Extrude { .. })) {
            return Some("tapered extrude turns inside out (tapered_extrude_that_closes_up_is_an_error_not_an_inside_out_body)");
        }
    }
    if msg.contains("has volume -") {
        match last {
            Some(FeatureKind::Extrude { taper, .. }) if taper.0 != "0" => return Some("tapered extrude turns inside out (tapered_extrude_that_closes_up_is_an_error_not_an_inside_out_body)"),
            Some(FeatureKind::Pipe { bend, .. }) if bend.0 != "0" => return Some("pipe bend tighter than the pipe (pipe_round_a_hairpin_with_a_bend_radius_is_not_inside_out)"),
            _ => {}
        }
    }
    if msg.contains("cached vs fresh") || msg.contains("JSON round trip") {
        let sig = |ev: &Evaluator| format!("{:?} {:?}", errors(ev, d), ev.model().bodies.iter().map(|b| (b.id, format!("{:.6}", b.solid.volume()))).collect::<Vec<_>>());
        let first = sig(&run(d));
        if (0..6).any(|_| sig(&run(d)) != first) {
            return Some("the same document evaluates differently (press_pull_of_a_face_whose_holes_touch_is_deterministic)");
        }
        // a stable evaluation that changes after save and load: coordinates read back one ULP off moved the geometry
        if msg.contains("JSON round trip") && Document::from_json(&d.to_json()).ok().as_ref() != Some(d) {
            return Some("save and load changes coordinates and so the model (json_round_trip_keeps_picked_coordinates_exactly)");
        }
    }
    None
}

/// Runs seeds `range` and reports every failure with the step list of its document.
fn fuzz_range(range: std::ops::Range<u64>, max_steps: usize, budget_s: f64) {
    let t = Instant::now();
    let mut timings = Vec::new();
    let mut failures = Vec::new();
    let mut known: HashMap<&'static str, usize> = HashMap::new();
    let mut done = 0;
    for seed in range.clone() {
        let (d, r) = fuzz_case(seed, max_steps, &mut timings);
        if let Err(e) = &r {
            if let Some(k) = known_bug(&d, e) {
                *known.entry(k).or_insert(0usize) += 1;
                println!("known bug, seed {seed}: {k}");
                done += 1;
                continue;
            }
        }
        if let Err(e) = r {
            let steps: Vec<String> = d.features.iter().map(|f| format!("{:?}", f.kind).chars().take(220).collect()).collect();
            failures.push(format!("{e}\n    params {:?}\n    steps:\n      {}", d.params.items, steps.join("\n      ")));
        }
        done += 1;
        if t.elapsed().as_secs_f64() > budget_s {
            break;
        }
    }
    timings.sort_by(|a, b| b.0.total_cmp(&a.0));
    let total = t.elapsed().as_secs_f64();
    println!(
        "fuzz: {done} seeds of {:?} in {total:.1} s; {} steps; {} sliver states; {} documents not bit-exact after JSON; slowest steps:",
        range,
        timings.len(),
        SLIVERS.with(|c| c.get()),
        INEXACT_JSON.with(|c| c.get())
    );
    for (s, seed, what) in timings.iter().take(6) {
        println!("  {s:.2} s  seed {seed}  {what}");
    }
    for (k, c) in &known {
        println!("  known bug hit {c} times: {k}");
    }
    assert!(failures.is_empty(), "{} of {done} fuzz cases failed:\n{}", failures.len(), failures.join("\n\n"));
}

#[test]
fn fuzz_random_documents_hold_the_invariants() {
    fuzz_range(0..300, 7, 55.0);
}

#[test]
#[ignore = "slow fuzz"]
fn fuzz_random_documents_long_run() {
    // FUZZ_FROM / FUZZ_TO pick other seeds; FUZZ_LIGHT=1 leaves the slow features out
    let env = |k: &str, v: u64| std::env::var(k).ok().and_then(|s| s.parse().ok()).unwrap_or(v);
    HEAVY.with(|h| h.set(std::env::var("FUZZ_LIGHT").is_err()));
    fuzz_range(env("FUZZ_FROM", 1000)..env("FUZZ_TO", 4000), 12, 3600.0);
}

/// Debugging aid: `FUZZ_SEED=<n> FUZZ_OUT=<file>` replays one seed and saves its document as JSON.
#[test]
#[ignore = "debugging aid"]
fn fuzz_replay_one_seed() {
    let Ok(seed) = std::env::var("FUZZ_SEED") else { return };
    let seed: u64 = seed.parse().unwrap();
    let mut timings = Vec::new();
    let (d, r) = fuzz_case(seed, std::env::var("FUZZ_STEPS").ok().and_then(|s| s.parse().ok()).unwrap_or(10), &mut timings);
    if let Ok(out) = std::env::var("FUZZ_OUT") {
        std::fs::write(out, d.to_json()).unwrap();
    }
    println!("{r:?}");
}

/// Debugging aid: `FUZZ_DOC=<file>` evaluates a saved document and prints every step's error and bodies.
#[test]
#[ignore = "debugging aid"]
fn fuzz_inspect_document() {
    let Ok(path) = std::env::var("FUZZ_DOC") else { return };
    let mut d = Document::from_json(&std::fs::read_to_string(path).unwrap()).unwrap();
    if let Some(rb) = std::env::var("FUZZ_ROLLBACK").ok().and_then(|s| s.parse().ok()) {
        d.rollback = rb;
    }
    if let Some(k) = std::env::var("FUZZ_REPEAT").ok().and_then(|s| s.parse::<usize>().ok()) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        for _ in 0..k {
            let ev = run(&d);
            let sig = format!("{:?} {:?}", errors(&ev, &d), ev.model().bodies.iter().map(|b| (b.id, format!("{:.6}", b.solid.volume()))).collect::<Vec<_>>());
            *seen.entry(sig).or_default() += 1;
        }
        for (sig, c) in &seen {
            println!("{c}x {sig}");
        }
        return;
    }
    let ev = run(&d);
    for (i, (f, st)) in d.features.iter().zip(ev.steps()).enumerate() {
        let vols: Vec<String> = st.model.bodies.iter().map(|b| format!("{}:{:.3}{}", b.id, b.solid.volume(), if closed_by_index(b.mesh()) { "" } else { "!open" })).collect();
        println!("{i} uid {} {} {:.0}ms err={:?} bodies=[{}]", f.uid, f.kind.type_label(), st.millis, st.error, vols.join(" "));
    }
}

#[test]
fn rng_is_deterministic() {
    let (mut a, mut b) = (Rng::new(7), Rng::new(7));
    for _ in 0..100 {
        assert_eq!(a.next(), b.next());
    }
    let mut c = Rng::new(8);
    assert_ne!(Rng::new(7).next(), c.next());
}

// ------------------------------------------------------------------------------------------------ shrunk fuzz findings

#[test]
fn cut_through_a_body_with_an_internal_void_keeps_the_void() {
    // a closed shell leaves a sealed cavity: its inner surface is a separate connected piece of the mesh, but not a body
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 20.0, Op::NewBody);
    d.insert(FeatureKind::Shell { plane: click([10.0, 10.0, 20.0]), thickness: n(2.0), open: false, more: vec![] });
    let hollow = 8000.0 - 16.0f64.powi(3);
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    assert!((volume(&ev) - hollow).abs() < 1.0, "closed shell volume {}", volume(&ev));
    // a small cut at a corner (it does not reach the cavity)
    boxp(&mut d, [-1.0, -1.0, -1.0], 2.0, 2.0, 2.0, Op::Cut);
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    let vols = body_volumes(&ev);
    assert!(vols.iter().all(|v| *v > 0.0), "a body with negative volume appeared: {vols:?}");
    assert_eq!(vols.len(), 1, "the cavity became a body of its own: {vols:?}");
    assert!((vols[0] - (hollow - 1.0)).abs() < 1.0, "the cut filled the cavity: volume {} instead of {}", vols[0], hollow - 1.0);
}

#[test]
fn pipe_round_a_hairpin_with_a_bend_radius_is_not_inside_out() {
    // without a bend radius the hairpin is refused ("too sharp a turn"); with one the turn is cut into small steps that pass
    // the check while the arc left on the short side is far tighter than the pipe
    for (bend, wall, op) in [(5.0, 0.0, Op::NewBody), (5.5, 1.0, Op::NewBody), (5.0, 0.0, Op::Cut)] {
        let mut d = Document::default();
        if op == Op::Cut {
            boxp(&mut d, [-10.0, -10.0, -5.0], 40.0, 20.0, 10.0, Op::NewBody);
        }
        let mut s = Sketch::default();
        let p = [[0.0, 0.0], [20.0, 0.0], [0.0, 1.0]].map(|q| s.add_point(q));
        s.add(fusion_core::Entity::Line { a: p[0], b: p[1], construction: false });
        s.add(fusion_core::Entity::Line { a: p[1], b: p[2], construction: false });
        let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
        let uid = d.insert(FeatureKind::Pipe { path: sk, outer: n(4.0), wall: n(wall), op, shape: PipeShape::Round, corner: n(0.0), bend: n(bend) });
        let ev = run(&d);
        if err_of(&ev, uid).is_some() {
            continue;
        }
        let vols = body_volumes(&ev);
        assert!(vols.iter().all(|v| *v > 0.0), "bend {bend} wall {wall} {op:?}: a body with negative volume: {vols:?}");
        for b in &ev.model().bodies {
            let comps: Vec<f64> = b.mesh().components().iter().map(|c| c.volume()).collect();
            assert!(comps.iter().all(|v| *v > 0.0) || op == Op::NewBody && comps.len() == 1, "bend {bend} wall {wall} {op:?}: an inside-out shell is hidden in a body: {comps:?}");
        }
        if op == Op::NewBody {
            // the straight legs alone hold about (section area) x (20 + 20) of material
            let area = std::f64::consts::PI * (4.0 - if wall > 0.0 { (2.0 - wall) * (2.0 - wall) } else { 0.0 });
            let v = volume(&ev);
            assert!(v > 0.5 * area * 40.0, "bend {bend} wall {wall}: the hairpin pipe holds only {v} mm3");
        }
    }
}

#[test]
fn same_document_evaluates_identically_every_time() {
    // press-pull reads the face outline out of hash maps; the result (and which piece of a later split keeps which id) must
    // still not change from one evaluation to the next
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 60.0, 10.0, 10.0, Op::NewBody);
    for x in [10.0, 30.0, 50.0] {
        d.insert(FeatureKind::Hole { plane: zplane(10.0), at: [n(x), n(5.0)], dia: n(4.0), depth: None, kind: HoleKind::Simple, more: vec![] });
    }
    d.insert(FeatureKind::PressPull { plane: zplane(10.0), seed: [5.0, 2.0, 10.0], distance: n(3.0) });
    let mut s = Sketch::default();
    s.add_rect([19.0, -5.0], [21.0, 20.0]);
    s.add_rect([39.0, -5.0], [41.0, 20.0]);
    let sk = d.insert(FeatureKind::Sketch { plane: zplane(-1.0), sketch: s });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([20.0, 0.0]), ProfileRef::at([40.0, 0.0])], extent: Extent::OneSide(n(20.0)), op: Op::Cut, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
    let snapshot = |ev: &Evaluator| -> Vec<(u64, usize, usize, String)> {
        ev.model().bodies.iter().map(|b| (b.id, b.mesh().vertices.len(), b.mesh().triangles.len(), format!("{:.9}", b.solid.volume()))).collect()
    };
    let first = run(&d);
    assert!(errors(&first, &d).is_empty(), "{:?}", errors(&first, &d));
    let want = snapshot(&first);
    assert_eq!(want.len(), 3, "the cut makes three pieces: {want:?}");
    for k in 0..20 {
        let got = snapshot(&run(&d));
        assert_eq!(got, want, "evaluation {k} differs from the first one");
    }
}

#[test]
fn json_round_trip_keeps_picked_coordinates_exactly() {
    // picked faces store raw coordinates (a click on a slanted face): a save and load must give the same document back,
    // otherwise every reopened project differs from the saved one and the cached evaluation is thrown away
    let mut rng = Rng::new(99);
    let mut bad = Vec::new();
    for _ in 0..2000 {
        let v = [rng.range(-100.0, 100.0), rng.range(-100.0, 100.0), rng.range(-100.0, 100.0)];
        let mut d = Document::default();
        d.insert(FeatureKind::PressPull { plane: Plane { origin: v, ..Plane::XY }, seed: v, distance: n(1.0) });
        let back = Document::from_json(&d.to_json()).unwrap();
        if back != d {
            if let FeatureKind::PressPull { seed, .. } = &back.features[0].kind {
                bad.push((v, *seed));
            }
        }
    }
    assert!(bad.is_empty(), "{} of 2000 documents changed after save and load, e.g. {:?}", bad.len(), bad.first());
}

// ------------------------------------------------------------------------------------------------ (a) faces that follow

#[test]
fn press_pull_follows_a_taller_box() {
    let mut d = Document::default();
    d.params.set("h", "20");
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(30.0), h: Expr::from("h") }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    let pp = d.insert(FeatureKind::PressPull { plane: zplane(20.0), seed: [20.0, 15.0, 20.0], distance: n(5.0) });
    d.insert(FeatureKind::Hole { plane: zplane(25.0), at: [n(20.0), n(15.0)], dia: n(6.0), depth: Some(n(4.0)), kind: HoleKind::Simple, more: vec![] });
    for h in [20.0, 35.0, 8.0] {
        d.params.set("h", h.to_string().as_str());
        let ev = run(&d);
        assert!(err_of(&ev, pp).is_none(), "h={h}: {:?}", errors(&ev, &d));
        let (_, hi) = bounds_of(ev.model().bodies[0].mesh());
        assert!((hi[2] - (h + 5.0)).abs() < 1e-6, "h={h}: top at {} instead of {}", hi[2], h + 5.0);
        let want = 40.0 * 30.0 * (h + 5.0) - std::f64::consts::PI * 9.0 * 4.0;
        assert!((volume(&ev) - want).abs() < 2.0, "h={h}: volume {} vs {want}", volume(&ev));
    }
}

#[test]
fn press_pull_does_not_jump_into_a_pocket_inserted_before_it() {
    // the top face still exists at z = 20; only the picked point now lies over a pocket. Moving the pocket floor instead
    // silently edits a face the user never picked
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let pp = d.insert(FeatureKind::PressPull { plane: zplane(20.0), seed: [20.0, 15.0, 20.0], distance: n(5.0) });
    d.rollback = 1;
    boxp(&mut d, [15.0, 10.0, 15.0], 10.0, 10.0, 10.0, Op::Cut);
    d.rollback = d.features.len();
    let ev = run(&d);
    if err_of(&ev, pp).is_some() {
        return;
    }
    let b = &ev.model().bodies[0];
    let (_, hi) = bounds_of(b.mesh());
    assert!(
        (hi[2] - 25.0).abs() < 1e-6,
        "press-pull of the top face moved something else: top at {}, volume {} (pocket floor filled: {})",
        hi[2],
        b.solid.volume(),
        inside(b, [20.0, 15.0, 17.0])
    );
}

#[test]
fn hole_on_a_grown_body_stays_on_that_body() {
    // two separate boxes one above the other; the upper one grows. The hole picked on its top must stay there and not jump
    // to the top of the lower box, which happens to be nearer along the normal
    let mut d = Document::default();
    d.params.set("h", "20");
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 5.0, Op::NewBody);
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(30.0), h: Expr::from("h") }, at: p3(0.0, 0.0, 10.0), op: Op::NewBody });
    let hole = d.insert(FeatureKind::Hole { plane: zplane(30.0), at: [n(20.0), n(15.0)], dia: n(6.0), depth: Some(n(4.0)), kind: HoleKind::Simple, more: vec![] });
    let cyl = std::f64::consts::PI * 9.0 * 4.0;
    for h in [20.0, 30.0, 60.0] {
        d.params.set("h", h.to_string().as_str());
        let ev = run(&d);
        assert!(err_of(&ev, hole).is_none(), "h={h}: {:?}", errors(&ev, &d));
        let vols = body_volumes(&ev);
        assert!((vols[0] - 6000.0).abs() < 1.0, "h={h}: the hole went into the lower box: volumes {vols:?}");
        assert!((vols[1] - (1200.0 * h - cyl)).abs() < 2.0, "h={h}: the upper box has no hole: volumes {vols:?}");
    }
}

// ------------------------------------------------------------------------------------------------ (b) edges that follow

/// An edge as the app stores it now: with the normals of its two faces.
fn picked_edge(d: &Document, body: usize, a: [f64; 3], b: [f64; 3]) -> EdgeRef {
    // as the app notes it when an edge is picked: the normals of its two faces and whether it is a valley
    let e = EdgeRef::line(a, b);
    let info = e.face_info(run(d).model().bodies[body].mesh());
    assert!(info.is_some(), "edge {a:?}-{b:?} has two faces");
    let (faces, concave) = info.unwrap();
    EdgeRef { faces: Some(faces), concave, ..e }
}

#[test]
fn fillet_on_a_top_edge_stays_on_top_when_the_box_grows() {
    let mut d = Document::default();
    d.params.set("h", "20");
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(30.0), h: Expr::from("h") }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    let id = run(&d).model().bodies[0].id;
    let e = picked_edge(&d, 0, [0.0, 0.0, 20.0], [40.0, 0.0, 20.0]);
    let f = d.insert(FeatureKind::Fillet { body: id, edges: vec![e], radius: n(3.0) });
    for h in [20.0, 30.0, 39.0, 41.0, 60.0] {
        d.params.set("h", h.to_string().as_str());
        let ev = run(&d);
        assert!(err_of(&ev, f).is_none(), "h={h}: {:?}", errors(&ev, &d));
        let b = &ev.model().bodies[0];
        assert!(!inside(b, [20.0, 0.3, h - 0.3]), "h={h}: the top front edge is not rounded");
        assert!(inside(b, [20.0, 0.3, 0.3]), "h={h}: the bottom front edge was rounded instead");
    }
}

#[test]
fn fillet_on_a_side_edge_stays_on_its_face_when_the_box_gets_deeper() {
    // the same rule sideways: the front top edge, after the box grows towards -y, must stay the front top edge
    let mut d = Document::default();
    d.params.set("y0", "0");
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: Expr::from("30 - y0"), h: n(20.0) }, at: [n(0.0), Expr::from("y0"), n(0.0)], op: Op::NewBody });
    let id = run(&d).model().bodies[0].id;
    let e = picked_edge(&d, 0, [0.0, 0.0, 20.0], [40.0, 0.0, 20.0]);
    let f = d.insert(FeatureKind::Fillet { body: id, edges: vec![e], radius: n(3.0) });
    for y0 in [0.0, -10.0, -40.0] {
        d.params.set("y0", y0.to_string().as_str());
        let ev = run(&d);
        assert!(err_of(&ev, f).is_none(), "y0={y0}: {:?}", errors(&ev, &d));
        let b = &ev.model().bodies[0];
        assert!(!inside(b, [20.0, y0 + 0.3, 19.7]), "y0={y0}: the front top edge is not rounded");
        assert!(inside(b, [20.0, 29.7, 19.7]), "y0={y0}: the back top edge was rounded instead");
    }
}

#[test]
#[ignore = "LIMIT: an edge from an old file has no face normals, so a much taller box can move it to the bottom edge"]
fn old_file_fillet_on_a_top_edge_stays_on_top_when_the_box_grows() {
    // files saved before edges remembered their faces: the edge is replaced by the nearest parallel one on the surface
    let mut d = Document::default();
    d.params.set("h", "20");
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(30.0), h: Expr::from("h") }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    let id = run(&d).model().bodies[0].id;
    let f = d.insert(FeatureKind::Fillet { body: id, edges: vec![EdgeRef::line([0.0, 0.0, 20.0], [40.0, 0.0, 20.0])], radius: n(3.0) });
    for h in [20.0, 30.0, 39.0, 41.0, 60.0] {
        d.params.set("h", h.to_string().as_str());
        let ev = run(&d);
        assert!(err_of(&ev, f).is_none(), "h={h}: {:?}", errors(&ev, &d));
        let b = &ev.model().bodies[0];
        assert!(!inside(b, [20.0, 0.3, h - 0.3]), "h={h}: the top front edge is not rounded");
        assert!(inside(b, [20.0, 0.3, 0.3]), "h={h}: the bottom front edge was rounded instead");
    }
}

#[test]
fn fillet_with_faces_follows_a_moved_edge_on_an_l_shape() {
    // the step of an L moves up: the concave and convex step edges must be found again where their faces meet
    let mut d = Document::default();
    d.params.set("s", "10");
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(20.0), h: Expr::from("s") }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    boxp(&mut d, [0.0, 0.0, 0.0], 15.0, 20.0, 30.0, Op::Join);
    let id = run(&d).model().bodies[0].id;
    let convex = picked_edge(&d, 0, [40.0, 0.0, 10.0], [40.0, 20.0, 10.0]);
    let concave = picked_edge(&d, 0, [15.0, 0.0, 10.0], [15.0, 20.0, 10.0]);
    let f = d.insert(FeatureKind::Fillet { body: id, edges: vec![convex, concave], radius: n(2.0) });
    for s in [10.0, 16.0, 4.0] {
        d.params.set("s", s.to_string().as_str());
        let ev = run(&d);
        assert!(err_of(&ev, f).is_none(), "s={s}: {:?}", errors(&ev, &d));
        let b = &ev.model().bodies[0];
        assert!(!inside(b, [39.8, 10.0, s - 0.2]), "s={s}: the convex step edge is not rounded");
        assert!(inside(b, [15.2, 10.0, s + 0.2]), "s={s}: the concave step edge got no fillet material");
        assert!(inside(b, [39.8, 10.0, 0.2]), "s={s}: the bottom edge was rounded instead");
    }
}

#[test]
fn fillet_with_faces_follows_the_concave_edge_of_a_rising_step() {
    // only the concave edge at the foot of a column; the plate around it gets thicker
    let mut d = Document::default();
    d.params.set("s", "10");
    d.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(40.0), d: n(20.0), h: Expr::from("s") }, at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
    boxp(&mut d, [0.0, 0.0, 0.0], 15.0, 20.0, 30.0, Op::Join);
    let id = run(&d).model().bodies[0].id;
    let concave = picked_edge(&d, 0, [15.0, 0.0, 10.0], [15.0, 20.0, 10.0]);
    let f = d.insert(FeatureKind::Fillet { body: id, edges: vec![concave], radius: n(2.0) });
    for s in [10.0, 16.0, 4.0] {
        d.params.set("s", s.to_string().as_str());
        let ev = run(&d);
        assert!(err_of(&ev, f).is_none(), "s={s}: {:?}", errors(&ev, &d));
        let b = &ev.model().bodies[0];
        assert!(inside(b, [15.2, 10.0, s + 0.2]), "s={s}: the concave step edge got no fillet material");
        assert!(inside(b, [39.8, 10.0, s - 0.2]), "s={s}: the far convex edge was rounded instead");
    }
}

#[test]
fn fillet_edges_that_were_never_edges_stay_errors() {
    // an edge in the air or through the middle of the body was never an edge of it: no neighbour may be rounded instead
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let id = run(&d).model().bodies[0].id;
    for e in [EdgeRef::line([0.0, 50.0, 20.0], [40.0, 50.0, 20.0]), EdgeRef::line([0.0, 15.0, 10.0], [40.0, 15.0, 10.0])] {
        let mut x = d.clone();
        let f = x.insert(FeatureKind::Fillet { body: id, edges: vec![e], radius: n(2.0) });
        let ev = run(&x);
        assert!(err_of(&ev, f).is_some(), "edge {e:?} was followed to another edge: volume {}", volume(&ev));
    }
}

// ------------------------------------------------------------------------------------------------ (c) cuts and bodies

#[test]
fn cut_results_one_by_one() {
    let base = || {
        let mut d = Document::default();
        boxp(&mut d, [0.0; 3], 40.0, 10.0, 10.0, Op::NewBody);
        d
    };
    // touching face to face: nothing removed, an error
    let mut d = base();
    let uid = boxp(&mut d, [40.0, 0.0, 0.0], 5.0, 5.0, 5.0, Op::Cut);
    assert!(err_of(&run(&d), uid).is_some(), "a cut touching only a face must be an error");
    // touching along an edge only
    let mut d = base();
    let uid = boxp(&mut d, [40.0, 10.0, 0.0], 5.0, 5.0, 5.0, Op::Cut);
    assert!(err_of(&run(&d), uid).is_some(), "a cut touching only an edge must be an error");
    // swallowing the only body
    let mut d = base();
    let uid = boxp(&mut d, [-1.0, -1.0, -1.0], 50.0, 12.0, 12.0, Op::Cut);
    let ev = run(&d);
    assert!(err_of(&ev, uid).is_some() && ev.model().bodies.len() == 1, "a cut that swallows the body must be an error that keeps it");
    // three slots: four pieces, ids unique, volume kept
    let mut d = base();
    let mut s = Sketch::default();
    for x in [9.0, 19.0, 29.0] {
        s.add_rect([x, -5.0], [x + 2.0, 20.0]);
    }
    let sk = d.insert(FeatureKind::Sketch { plane: zplane(-1.0), sketch: s });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: [10.0, 20.0, 30.0].iter().map(|x| ProfileRef::at([*x, 0.0])).collect(), extent: Extent::OneSide(n(20.0)), op: Op::Cut, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
    let ev = run(&d);
    check_model(&ev, &d).unwrap();
    assert_eq!(ev.model().bodies.len(), 4, "{:?}", body_volumes(&ev));
    assert!((volume(&ev) - 3400.0).abs() < 1e-6, "{}", volume(&ev));
    // the same cut through two separate bars: each splits
    let mut d2 = base();
    boxp(&mut d2, [0.0, 20.0, 0.0], 40.0, 10.0, 10.0, Op::NewBody);
    let mut s = Sketch::default();
    s.add_rect([19.0, -5.0], [21.0, 40.0]);
    let sk = d2.insert(FeatureKind::Sketch { plane: zplane(-1.0), sketch: s });
    d2.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([20.0, 0.0])], extent: Extent::OneSide(n(20.0)), op: Op::Cut, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
    let ev = run(&d2);
    check_model(&ev, &d2).unwrap();
    assert_eq!(ev.model().bodies.len(), 4, "{:?}", body_volumes(&ev));
}

#[test]
fn combine_subtract_that_cuts_a_body_in_two_makes_two_bodies() {
    // a cut feature splits a bar into separate bodies; the same subtraction through Combine leaves one body of two pieces
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 10.0, 10.0, Op::NewBody);
    boxp(&mut d, [15.0, -1.0, -1.0], 10.0, 12.0, 12.0, Op::NewBody);
    let ids: Vec<u64> = run(&d).model().bodies.iter().map(|b| b.id).collect();
    d.insert(FeatureKind::Combine { target: ids[0], tools: vec![ids[1]], op: BoolOp::Subtract, keep_tools: false });
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    assert!((volume(&ev) - 3000.0).abs() < 1e-6);
    let pieces: Vec<usize> = ev.model().bodies.iter().map(|b| b.mesh().components().len()).collect();
    assert_eq!(ev.model().bodies.len(), 2, "one body holds {pieces:?} separate pieces");
}

#[test]
fn split_across_a_u_shape_makes_separate_arms() {
    let mut u = Document::default();
    boxp(&mut u, [0.0; 3], 30.0, 10.0, 5.0, Op::NewBody);
    boxp(&mut u, [0.0, 0.0, 5.0], 5.0, 10.0, 15.0, Op::Join);
    boxp(&mut u, [25.0, 0.0, 5.0], 5.0, 10.0, 15.0, Op::Join);
    let id = run(&u).model().bodies[0].id;
    u.insert(FeatureKind::Split { bodies: vec![id], axis: Axis3::Z, offset: n(10.0) });
    let ev = run(&u);
    assert!(errors(&ev, &u).is_empty(), "{:?}", errors(&ev, &u));
    let pieces: Vec<usize> = ev.model().bodies.iter().map(|b| b.mesh().components().len()).collect();
    assert!(pieces.iter().all(|p| *p == 1), "a body made by the split holds separate pieces: {pieces:?}");
}

#[test]
fn intersect_does_not_depend_on_bounding_boxes() {
    // an intersect tool that meets one small box: an untouched L-shaped body survives or vanishes depending only on whether
    // the tool happens to lie inside its bounding box
    let count = |l_at: f64| {
        let mut d = Document::default();
        boxp(&mut d, [l_at, 0.0, 0.0], 40.0, 10.0, 10.0, Op::NewBody);
        boxp(&mut d, [l_at, 0.0, 0.0], 10.0, 40.0, 10.0, Op::Join);
        boxp(&mut d, [45.0, 25.0, 0.0], 5.0, 5.0, 5.0, Op::NewBody);
        let uid = boxp(&mut d, [44.0, 24.0, -1.0], 3.0, 3.0, 3.0, Op::Intersect);
        let ev = run(&d);
        assert!(err_of(&ev, uid).is_none(), "{:?}", errors(&ev, &d));
        ev.model().bodies.len()
    };
    let (inside_bbox, far) = (count(30.0), count(-100.0));
    assert_eq!(inside_bbox, far, "the untouched L body survives far away ({far} bodies) but not when the tool is in its bounding box ({inside_bbox})");
}

#[test]
fn pattern_with_more_than_a_thousand_copies_keeps_body_ids_unique() {
    // body ids are uid * 1000 + k: copy number 1000 of a pattern collides with the body of the next step
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 1.0, 1.0, 1.0, Op::NewBody);
    let id = run(&d).model().bodies[0].id;
    d.insert(FeatureKind::Pattern { bodies: vec![id], kind: PatternKind::Rect { nx: n(40.0), dx: n(2.0), ny: n(30.0), dy: n(2.0), nz: n(1.0), dz: n(0.0) } });
    boxp(&mut d, [0.0, 0.0, 50.0], 5.0, 5.0, 5.0, Op::NewBody);
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    let mut seen = HashMap::new();
    for b in &ev.model().bodies {
        *seen.entry(b.id).or_insert(0) += 1;
    }
    let dup: Vec<u64> = seen.iter().filter(|(_, c)| **c > 1).map(|(id, _)| *id).collect();
    assert!(dup.is_empty(), "body ids used twice: {dup:?}");
}

#[test]
fn closed_shell_hollows_only_the_clicked_body() {
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 20.0, 20.0, 20.0, Op::NewBody);
    boxp(&mut d, [50.0, 0.0, 0.0], 20.0, 20.0, 20.0, Op::NewBody);
    d.insert(FeatureKind::Shell { plane: click([10.0, 10.0, 20.0]), thickness: n(2.0), open: false, more: vec![] });
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    let vols = body_volumes(&ev);
    assert!((vols[1] - 8000.0).abs() < 1e-6, "the other body was hollowed too: {vols:?}");
}

// ------------------------------------------------------------------------------------------------ (d) closed paths

fn loop_sketch(pts: &[[f64; 2]], closed: bool) -> Sketch {
    let mut s = Sketch::default();
    let ids: Vec<usize> = pts.iter().map(|p| s.add_point(*p)).collect();
    for i in 0..ids.len() - 1 {
        s.add(fusion_core::Entity::Line { a: ids[i], b: ids[i + 1], construction: false });
    }
    if closed {
        s.add(fusion_core::Entity::Line { a: ids[ids.len() - 1], b: ids[0], construction: false });
    }
    s
}

#[test]
fn closed_pipe_rings_are_closed_with_the_expected_volume() {
    let pi = std::f64::consts::PI;
    let square = [[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]];
    let triangle = [[0.0, 0.0], [40.0, 0.0], [10.0, 30.0]];
    // seam in the middle of a straight side
    let mid_seam = [[20.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0], [0.0, 0.0]];
    for (name, pts) in [("square", &square[..]), ("triangle", &triangle[..]), ("seam on a side", &mid_seam[..])] {
        let m = pts.len();
        let perim: f64 = (0..m).map(|i| { let (a, b): ([f64; 2], [f64; 2]) = (pts[i], pts[(i + 1) % m]); ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt() }).sum();
        // a bend of radius r at a corner with interior angle t shortens the centre line by 2 r / tan(t/2) - r (pi - t)
        let shortening = |r: f64| -> f64 {
            (0..m).map(|i| {
                let (a, b, c): ([f64; 2], [f64; 2], [f64; 2]) = (pts[(i + m - 1) % m], pts[i], pts[(i + 1) % m]);
                let (u, v) = ([a[0] - b[0], a[1] - b[1]], [c[0] - b[0], c[1] - b[1]]);
                let t = ((u[0] * v[0] + u[1] * v[1]) / ((u[0] * u[0] + u[1] * u[1]).sqrt() * (v[0] * v[0] + v[1] * v[1]).sqrt())).clamp(-1.0, 1.0).acos();
                if t > std::f64::consts::PI - 1e-3 { 0.0 } else { 2.0 * r / (t / 2.0).tan() - r * (std::f64::consts::PI - t) }
            }).sum()
        };
        for (shape, wall, bend) in [(PipeShape::Round, 0.0, 0.0), (PipeShape::Round, 1.0, 4.0), (PipeShape::Square, 0.0, 0.0), (PipeShape::Square, 1.0, 3.0)] {
            let mut d = Document::default();
            let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: loop_sketch(pts, true) });
            d.insert(FeatureKind::Pipe { path: sk, outer: n(4.0), wall: n(wall), op: Op::NewBody, shape, corner: n(0.0), bend: n(bend) });
            let ev = run(&d);
            let tag = format!("{name} {shape:?} wall {wall} bend {bend}");
            assert!(errors(&ev, &d).is_empty(), "{tag}: {:?}", errors(&ev, &d));
            check_model(&ev, &d).unwrap_or_else(|e| panic!("{tag}: {e}"));
            assert_eq!(ev.model().bodies.len(), 1, "{tag}");
            let b = &ev.model().bodies[0];
            assert_eq!(b.mesh().components().len(), if wall > 0.0 { 2 } else { 1 }, "{tag}: a ring has no caps and one (or, hollow, two) surfaces");
            let area = match shape {
                PipeShape::Round => pi * (4.0 - if wall > 0.0 { (2.0 - wall).powi(2) } else { 0.0 }),
                PipeShape::Square => 16.0 - if wall > 0.0 { (4.0 - 2.0 * wall).powi(2) } else { 0.0 },
            };
            // mitred corners keep the centre-line length; rounded bends shorten it
            let want = (perim - shortening(bend)) * area;
            let v = b.solid.volume();
            assert!(v > 0.0 && (v - want).abs() < 0.06 * want, "{tag}: volume {v} vs about {want}");
        }
    }
}

#[test]
fn sweep_along_a_closed_path_is_a_ring() {
    let mut d = Document::default();
    let path = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: loop_sketch(&[[0.0, 0.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]], true) });
    let mut prof = Sketch::default();
    prof.add_circle([0.0, 0.0], 2.0);
    // the profile sits at the start of the path, square to it (plane YZ through the origin)
    let sk = d.insert(FeatureKind::Sketch { plane: Plane { origin: [20.0, 0.0, 0.0], ..Plane::YZ }, sketch: prof });
    let uid = d.insert(FeatureKind::Sweep { sketch: sk, profiles: vec![ProfileRef::at([0.0, 0.0])], path, op: Op::NewBody });
    let ev = run(&d);
    assert!(err_of(&ev, uid).is_none(), "a sweep along a closed path is refused: {:?}", err_of(&ev, uid));
    check_model(&ev, &d).unwrap();
}

#[test]
fn closed_pipe_with_a_bend_bigger_than_the_sides_stays_closed() {
    // a bend radius far above what the short sides allow is cut down; the ring must still close on itself
    let mut d = Document::default();
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: loop_sketch(&[[0.0, 0.0], [40.0, 0.0], [40.0, 6.0], [0.0, 6.0]], true) });
    let uid = d.insert(FeatureKind::Pipe { path: sk, outer: n(2.0), wall: n(0.0), op: Op::NewBody, shape: PipeShape::Round, corner: n(0.0), bend: n(50.0) });
    let ev = run(&d);
    if err_of(&ev, uid).is_none() {
        check_model(&ev, &d).unwrap();
        assert!(volume(&ev) > 0.0);
    }
}

// ------------------------------------------------------------------------------------------------ (e) triangulation speed

fn plate_with_holes(nx: usize, ny: usize, square: bool) -> Sketch {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [nx as f64 * 10.0, ny as f64 * 10.0]);
    for i in 0..nx {
        for j in 0..ny {
            let (cx, cy) = (i as f64 * 10.0 + 5.0, j as f64 * 10.0 + 5.0);
            if square {
                // corners on the same lines as the neighbours' corners: the alignment earcut handles worst
                s.add_rect([cx - 2.5, cy - 2.5], [cx + 2.5, cy + 2.5]);
            } else {
                s.add_circle([cx, cy], 3.0);
            }
        }
    }
    s
}

fn time_plate(nx: usize, ny: usize, square: bool) -> (f64, f64) {
    let s = plate_with_holes(nx, ny, square);
    let t = Instant::now();
    let regions = s.regions();
    let regions_s = t.elapsed().as_secs_f64();
    let plate = regions.iter().max_by(|a, b| a.area().total_cmp(&b.area())).expect("plate region").clone();
    assert_eq!(plate.holes.len(), nx * ny, "every hole is a hole of the plate");
    let t = Instant::now();
    let (v, tris) = plate.triangulate();
    let tri_s = t.elapsed().as_secs_f64();
    let got: f64 = tris.iter().map(|t| { let (a, b, c) = (v[t[0]], v[t[1]], v[t[2]]); ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5 }).sum();
    assert!((got - plate.area()).abs() < 1e-6 * plate.area(), "{nx}x{ny} square={square}: triangles cover {got} of {}", plate.area());
    println!("{nx}x{ny} holes (square={square}): regions {regions_s:.2} s, triangulate {tri_s:.3} s, {} vertices", v.len());
    (regions_s, tri_s)
}

#[test]
fn plates_with_many_round_holes_triangulate_in_reasonable_time() {
    for (nx, ny) in [(10, 10), (15, 15), (25, 20)] {
        let (r, t) = time_plate(nx, ny, false);
        // debug build: generous limits, the numbers are printed
        assert!(t < 10.0, "{nx}x{ny}: triangulation took {t:.1} s");
        assert!(r < 30.0, "{nx}x{ny}: finding regions took {r:.1} s");
    }
}

#[test]
fn plate_with_400_aligned_square_holes_triangulates_in_seconds() {
    // 1604 vertices, a fifth of a 15x15 plate of round holes that takes milliseconds
    let (_, t) = time_plate(10, 10, true);
    assert!(t < 0.5, "10x10 aligned square holes: triangulation took {t:.2} s");
    let (_, t) = time_plate(20, 20, true);
    assert!(t < 10.0, "20x20 aligned square holes: triangulation took {t:.1} s");
}

#[test]
fn plate_with_aligned_square_holes_extrudes_closed() {
    let mut d = Document::default();
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: plate_with_holes(8, 6, true) });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::OneSide(n(3.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
    let t = Instant::now();
    let ev = run(&d);
    println!("8x6 square holes extrude: {:.2} s", t.elapsed().as_secs_f64());
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    check_model(&ev, &d).unwrap();
    assert!(closed_welded(ev.model().bodies[0].mesh()), "cracks in the plate caps");
    assert!((volume(&ev) - (80.0 * 60.0 - 48.0 * 25.0) * 3.0).abs() < 1e-6, "{}", volume(&ev));
    // press-pull of the holed top face goes through face_region and triangulation again
    d.insert(FeatureKind::PressPull { plane: zplane(3.0), seed: [1.0, 1.0, 3.0], distance: n(2.0) });
    let t = Instant::now();
    let ev = run(&d);
    println!("press-pull of the holed face: {:.2} s", t.elapsed().as_secs_f64());
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    check_model(&ev, &d).unwrap();
    assert!((volume(&ev) - (80.0 * 60.0 - 48.0 * 25.0) * 5.0).abs() < 1e-6, "{}", volume(&ev));
}

// ------------------------------------------------------------------------------------------------ expressions

#[test]
fn deeply_nested_expression_does_not_overflow_the_stack() {
    // the parser recurses once per bracket or unary minus; a file (or a paste) with a very deep expression must give an
    // error, not abort the whole program. Run in a child process because a stack overflow cannot be caught.
    if let Ok(kind) = std::env::var("GENIX_FUZZ_DEEP_EXPR") {
        let depth = 200_000;
        let src = if kind == "brackets" { format!("{}1{}", "(".repeat(depth), ")".repeat(depth)) } else { format!("{}1", "-".repeat(depth)) };
        let _ = fusion_core::Params::default().eval(&src);
        return;
    }
    for kind in ["brackets", "minus"] {
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "deeply_nested_expression_does_not_overflow_the_stack", "--nocapture", "--include-ignored"])
            .env("GENIX_FUZZ_DEEP_EXPR", kind)
            .output()
            .unwrap();
        assert!(out.status.success(), "{kind}: evaluating a deeply nested expression crashed the process ({:?})", out.status);
    }
}

#[test]
fn odd_expressions_give_clean_errors_or_finite_values() {
    let p = fusion_core::Params::default();
    for src in ["", " ", "1/0", "0/0", "sqrt(-1)", "1e309", "-1e309", "1e308*10", "(", ")", "1+", "pi(", "min(1)", "max(1,2,3)", "1e", ".", "..", "1..2", "2^1000000", "(-8)^(1/3)", "tan(90)", "abc", "1 2", "--1", "+-+1", "\u{0}", "Ж"] {
        match catch_unwind(|| p.eval(src)) {
            Ok(Ok(v)) => assert!(v.is_finite(), "{src:?} gave {v}"),
            Ok(Err(e)) => assert!(!e.is_empty(), "{src:?} gave an empty error"),
            Err(_) => panic!("{src:?} panicked"),
        }
    }
    // a parameter that refers to itself, directly or in a ring
    let mut q = fusion_core::Params::default();
    q.set("a", "b+1");
    q.set("b", "a*2");
    q.set("c", "c");
    for src in ["a", "c", "a+c"] {
        assert!(q.eval(src).is_err(), "{src:?} in a reference ring must be an error");
    }
}

// ------------------------------------------------------------------------------------------------ extreme values

#[test]
fn extreme_sizes_give_clean_errors_or_sound_bodies() {
    let shapes = |v: f64| {
        vec![
            Shape::Box { w: n(v), d: n(v), h: n(v) },
            Shape::Cylinder { r: n(v), h: n(v) },
            Shape::Sphere { r: n(v) },
            Shape::Torus { major: n(v), minor: n(v / 3.0) },
        ]
    };
    for v in [1e-9, 1e-6, 1e-3, 1e6, 1e9, 1e12] {
        for shape in shapes(v) {
            let mut d = Document::default();
            d.insert(FeatureKind::Primitive { shape: shape.clone(), at: p3(0.0, 0.0, 0.0), op: Op::NewBody });
            let ev = run(&d);
            check_model(&ev, &d).unwrap_or_else(|e| panic!("{shape:?}: {e}"));
        }
    }
    // a normal box moved, scaled and patterned with extreme values
    for (t, s) in [(1e9, 1e-6), (1e12, 1e6), (-1e15, 1.0)] {
        let mut d = Document::default();
        boxp(&mut d, [0.0; 3], 10.0, 10.0, 10.0, Op::NewBody);
        let id = run(&d).model().bodies[0].id;
        d.insert(FeatureKind::Move { bodies: vec![id], translate: p3(t, 0.0, 0.0), rotate: p3(0.0, 0.0, 0.0), copy: false });
        d.insert(FeatureKind::Scale { bodies: vec![id], factor: p3(s, s, s), uniform: true });
        let ev = run(&d);
        check_model(&ev, &d).unwrap_or_else(|e| panic!("move {t} scale {s}: {e}"));
    }
}


#[test]
fn press_pull_of_a_face_whose_holes_touch_is_deterministic() {
    // two pockets meet at one corner: the outline of the top face passes that vertex twice. Which way the outline walk turns
    // there must not depend on hash map order (the same document gave different results from one evaluation to the next)
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 10.0, Op::NewBody);
    boxp(&mut d, [10.0, 10.0, 5.0], 10.0, 10.0, 10.0, Op::Cut);
    boxp(&mut d, [20.0, 20.0, 5.0], 10.0, 5.0, 10.0, Op::Cut);
    let pp = d.insert(FeatureKind::PressPull { plane: zplane(10.0), seed: [5.0, 5.0, 10.0], distance: n(3.0) });
    let mut seen: HashMap<String, usize> = HashMap::new();
    for _ in 0..30 {
        let ev = run(&d);
        let sig = format!("{:?} {:?}", err_of(&ev, pp), body_volumes(&ev).iter().map(|v| format!("{v:.6}")).collect::<Vec<_>>());
        *seen.entry(sig).or_default() += 1;
    }
    assert_eq!(seen.len(), 1, "30 evaluations of one document gave different results: {seen:?}");
    let ev = run(&d);
    assert!(err_of(&ev, pp).is_none(), "{:?}", err_of(&ev, pp));
    assert!((volume(&ev) - (11250.0 + 1050.0 * 3.0)).abs() < 1e-6, "volume {}", volume(&ev));
}

#[test]
fn tapered_extrude_that_closes_up_is_an_error_not_an_inside_out_body() {
    // a 20 mm square tapered by 10 degrees closes up after about 57 mm; going on to 100 mm turns the walls inside out
    for (extent, holes) in [(Extent::OneSide(n(100.0)), false), (Extent::All(true), false), (Extent::OneSide(n(40.0)), true)] {
        let mut d = Document::default();
        boxp(&mut d, [-30.0, -30.0, -30.0], 60.0, 60.0, 60.0, Op::NewBody);
        let mut s = Sketch::default();
        s.add_rect([-10.0, -10.0], [10.0, 10.0]);
        if holes {
            // holes grow while the outline shrinks: they cross the outline long before it closes
            s.add_rect([-8.0, -8.0], [-4.0, -4.0]);
            s.add_circle([5.0, 5.0], 2.0);
        }
        let sk = d.insert(FeatureKind::Sketch { plane: zplane(31.0), sketch: s });
        let uid = d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([0.0, -9.0])], extent: extent.clone(), op: Op::NewBody, face: None, taper: n(10.0), thin: n(0.0), start: n(0.0) });
        let ev = run(&d);
        if err_of(&ev, uid).is_some() {
            continue;
        }
        let vols = body_volumes(&ev);
        assert!(vols.iter().all(|v| *v > 0.0), "{extent:?} holes={holes}: a body with negative volume: {vols:?}");
        check_model(&ev, &d).unwrap_or_else(|e| panic!("{extent:?} holes={holes}: {e}"));
    }
}

#[test]
#[ignore = "LIMIT: faces are not tied to a body, so a hole whose body was removed may drill the body below"]
fn hole_whose_body_is_gone_does_not_jump_to_another_body() {
    // the hole was picked on the upper box; when an earlier step removes that box (or moves it aside), the lower, separate
    // box must not get the hole instead
    let mut bad = Vec::new();
    for variant in ["remove", "move aside"] {
        let mut d = Document::default();
        boxp(&mut d, [0.0; 3], 40.0, 30.0, 5.0, Op::NewBody);
        boxp(&mut d, [0.0, 0.0, 10.0], 40.0, 30.0, 20.0, Op::NewBody);
        let upper = run(&d).model().bodies[1].id;
        let hole = d.insert(FeatureKind::Hole { plane: zplane(30.0), at: [n(20.0), n(15.0)], dia: n(6.0), depth: Some(n(4.0)), kind: HoleKind::Simple, more: vec![] });
        d.rollback = 2;
        if variant == "remove" {
            d.insert(FeatureKind::Remove { bodies: vec![upper] });
        } else {
            d.insert(FeatureKind::Move { bodies: vec![upper], translate: p3(100.0, 0.0, 0.0), rotate: p3(0.0, 0.0, 0.0), copy: false });
        }
        d.rollback = d.features.len();
        let ev = run(&d);
        let vols = body_volumes(&ev);
        if (vols[0] - 6000.0).abs() > 1e-6 {
            bad.push(format!("{variant}: the hole went into the lower box (error {:?}, volumes {vols:?})", err_of(&ev, hole)));
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
fn press_pull_on_a_face_split_by_a_groove_moves_the_picked_half() {
    // a groove inserted earlier splits the top face in two: the half under the picked point is the one that moves
    let mut d = Document::default();
    boxp(&mut d, [0.0; 3], 40.0, 30.0, 20.0, Op::NewBody);
    let pp = d.insert(FeatureKind::PressPull { plane: zplane(20.0), seed: [30.0, 15.0, 20.0], distance: n(5.0) });
    d.rollback = 1;
    boxp(&mut d, [18.0, -1.0, 15.0], 4.0, 32.0, 10.0, Op::Cut);
    d.rollback = d.features.len();
    let ev = run(&d);
    assert!(err_of(&ev, pp).is_none(), "{:?}", errors(&ev, &d));
    let b = &ev.model().bodies[0];
    assert!(inside(b, [30.0, 15.0, 24.0]), "the picked half did not rise");
    assert!(!inside(b, [10.0, 15.0, 24.0]), "the other half rose too");
}

#[test]
fn combine_of_touching_bodies_leaves_no_empty_body() {
    // the two halves of a split share only the cut plane: their intersection has no volume, and a split by a touching tool
    // divides nothing. Neither may leave a body without volume (a move of it later even gives a negative one)
    let mut bad = Vec::new();
    // the plain case and the numbers of the box the fuzzer found it with (seed 1111)
    for (at, size, cut, hole) in [([0.0, 0.0, 0.0], [20.0, 20.0, 10.0], 4.0, [10.0, 10.0]), ([13.0, 23.0, 15.0], [18.0, 29.5, 4.375], 17.5, [19.0, 32.833333333333336])] {
        for op in [BoolOp::Intersect, BoolOp::Split] {
            let mut d = Document::default();
            boxp(&mut d, at, size[0], size[1], size[2], Op::NewBody);
            let id = run(&d).model().bodies[0].id;
            d.insert(FeatureKind::Split { bodies: vec![id], axis: Axis3::Z, offset: n(cut) });
            // a blind hole picked on the cut plane: it reaches down into the lower half and 1 mm up into the upper one
            d.insert(FeatureKind::Hole { plane: zplane(cut), at: [n(hole[0]), n(hole[1])], dia: n(3.5), depth: Some(n(14.5)), kind: HoleKind::Simple, more: vec![] });
            let ids: Vec<u64> = run(&d).model().bodies.iter().map(|b| b.id).collect();
            let uid = d.insert(FeatureKind::Combine { target: ids[1], tools: vec![ids[0]], op, keep_tools: false });
            let ev = run(&d);
            let vols = body_volumes(&ev);
            if err_of(&ev, uid).is_none() && vols.iter().any(|v| v.abs() < 1e-6) {
                bad.push(format!("box {size:?} {op:?}: volumes {vols:?}"));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

#[test]
#[ignore = "slow in debug (about 3 min); 11 s in release: cargo test --release"]
fn fuzz_seed_751_has_no_step_slower_than_twenty_seconds() {
    // the document (17 steps; step 15 is a press-pull of -2.5 on a slanted face where a square pipe meets a shelled,
    // mirrored cylinder) is easiest to rebuild through the fuzzer itself: FUZZ_SEED=751 FUZZ_STEPS=12 FUZZ_OUT=... saves it
    let mut timings = Vec::new();
    let (_, r) = fuzz_case(751, 12, &mut timings);
    let worst = timings.iter().cloned().fold((0.0, 0, ""), |a, b| if b.0 > a.0 { b } else { a });
    println!("{r:?}; slowest step {:.1} s ({})", worst.0, worst.2);
    assert!(worst.0 < 20.0, "a {} step took {:.0} s", worst.2, worst.0);
}
