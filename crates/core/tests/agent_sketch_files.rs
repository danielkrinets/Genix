//! QA suite: sketches, sketch operations and file formats in `fusion-core`, as a user meets them.
//! Tests that expose a confirmed product bug are kept but marked `#[ignore = "BUG: ..."]` so the suite stays green.
use fusion_core::doc::*;
use fusion_core::mesh::Mesh;
use fusion_core::sketch::{region_at, ProfileRef, Region, SolveStatus};
use fusion_core::text::SketchText;
use fusion_core::{dxf, Constraint, Document, Entity, Evaluator, Expr, FeatureKind, Op, Params, Plane, Sketch};
use glam::DVec2;
use std::f64::consts::PI;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::time::Instant;

// ------------------------------------------------------------------ helpers

fn n(v: f64) -> Expr {
    Expr::num(v)
}

fn params() -> Params {
    Params::default()
}

fn total_area(r: &[Region]) -> f64 {
    r.iter().map(|x| x.area()).sum()
}

fn sorted_areas(r: &[Region]) -> Vec<f64> {
    let mut a: Vec<f64> = r.iter().map(|x| x.area()).collect();
    a.sort_by(|x, y| x.total_cmp(y));
    a
}

/// Area of a regular n-gon inscribed in a circle of radius r (how sketch circles are tessellated: 72 segments).
fn ngon_area(r: f64, segs: usize) -> f64 {
    0.5 * segs as f64 * r * r * (2.0 * PI / segs as f64).sin()
}

/// No sample point lies in two regions at once (regions tile, they never overlap).
fn assert_no_overlap(r: &[Region], lo: [f64; 2], hi: [f64; 2], steps: usize) {
    for i in 0..steps {
        for j in 0..steps {
            let p = [lo[0] + (hi[0] - lo[0]) * (i as f64 + 0.37) / steps as f64, lo[1] + (hi[1] - lo[1]) * (j as f64 + 0.61) / steps as f64];
            let k = r.iter().filter(|x| x.contains(p)).count();
            assert!(k <= 1, "{p:?} lies in {k} regions; areas {:?}", sorted_areas(r));
        }
    }
}

fn line_pts(s: &Sketch, e: usize) -> (DVec2, DVec2) {
    match s.entities[e] {
        Entity::Line { a, b, .. } => (s.p(a), s.p(b)),
        _ => panic!("entity {e} is not a line"),
    }
}

fn polyline(s: &mut Sketch, pts: &[[f64; 2]], closed: bool) -> Vec<usize> {
    let ids: Vec<usize> = pts.iter().map(|p| s.add_point(*p)).collect();
    let m = if closed { ids.len() } else { ids.len() - 1 };
    (0..m).map(|i| s.add(Entity::Line { a: ids[i], b: ids[(i + 1) % ids.len()], construction: false })).collect()
}

fn run(d: &Document) -> Evaluator {
    let mut ev = Evaluator::default();
    ev.evaluate(d);
    ev
}

fn errors(ev: &Evaluator, d: &Document) -> Vec<String> {
    d.features.iter().filter_map(|f| ev.error(f.uid).map(|e| format!("{}: {e}", f.name))).collect()
}

/// Puts the sketch into a document, extrudes every region as its own new body by `h` and returns
/// (sum of region areas, total solid volume, number of bodies, step errors).
fn extrude_all(s: &Sketch, h: f64) -> (f64, f64, usize, Vec<String>) {
    let mut d = Document::default();
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s.clone() });
    let ev = run(&d);
    let solved = ev.sketch(sk).expect("sketch result");
    let regions = solved.regions.clone();
    for r in &regions {
        d.insert(FeatureKind::Extrude {
            sketch: sk,
            profiles: vec![ProfileRef::of(r)],
            extent: Extent::OneSide(n(h)),
            op: Op::NewBody,
            face: None,
            taper: n(0.0),
            thin: n(0.0),
            start: n(0.0),
        });
    }
    let ev = run(&d);
    let m = ev.model();
    (total_area(&regions), m.bodies.iter().map(|b| b.solid.volume()).sum(), m.bodies.len(), errors(&ev, &d))
}

fn assert_extrudes(s: &Sketch, h: f64, rel: f64) {
    let (area, vol, bodies, errs) = extrude_all(s, h);
    assert!(errs.is_empty(), "extrude errors {errs:?}");
    assert_eq!(bodies, s.regions().len(), "one body per region");
    assert!((vol - area * h).abs() <= rel * area * h + 1e-6, "volume {vol} vs area {area} x {h} = {}", area * h);
}

fn constraints_hold(s: &Sketch, tol: f64) {
    // re-solve must report Ok without moving anything noticeably
    let mut c = s.clone();
    assert_eq!(c.solve(&params(), &[]).unwrap(), SolveStatus::Ok);
    for (p, q) in s.points.iter().zip(&c.points) {
        assert!((p.pos[0] - q.pos[0]).abs() < tol && (p.pos[1] - q.pos[1]).abs() < tol, "a solved sketch moved on re-solve: {p:?} -> {q:?}");
    }
}

// ------------------------------------------------------------------ drawing and regions

#[test]
fn basic_shapes_give_one_region_each_with_the_right_area() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [40.0, 25.0]);
    let r = s.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - 1000.0).abs() < 1e-9);
    assert!(r[0].holes.is_empty());

    let mut c = Sketch::default();
    c.add_circle([3.0, -4.0], 10.0);
    let r = c.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - ngon_area(10.0, 72)).abs() < 1e-6, "{}", r[0].area());
    assert!((r[0].area() - PI * 100.0).abs() / (PI * 100.0) < 0.002);

    // half disc: arc from (10,0) counter-clockwise to (-10,0) closed by its diameter
    let mut h = Sketch::default();
    let (cc, a, b) = (h.add_point([0.0, 0.0]), h.add_point([10.0, 0.0]), h.add_point([-10.0, 0.0]));
    h.add(Entity::Arc { c: cc, a, b, construction: false });
    h.add(Entity::Line { a: b, b: a, construction: false });
    let r = h.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - PI * 50.0).abs() / (PI * 50.0) < 0.005, "{}", r[0].area());
    assert!(r[0].outer.iter().all(|p| p[1] >= -1e-9), "the bulge is on top (counter-clockwise from a to b)");

    // triangle by a closed polyline
    let mut t = Sketch::default();
    polyline(&mut t, &[[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]], true);
    let r = t.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - 50.0).abs() < 1e-9);
}

#[test]
fn open_chains_and_stray_lines_bound_nothing() {
    let mut s = Sketch::default();
    polyline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], false);
    assert!(s.regions().is_empty(), "a U shape is open");
    // an arc alone is open
    let mut a = Sketch::default();
    let (c, p, q) = (a.add_point([0.0, 0.0]), a.add_point([5.0, 0.0]), a.add_point([0.0, 5.0]));
    a.add(Entity::Arc { c, a: p, b: q, construction: false });
    assert!(a.regions().is_empty());
    // a single line
    let mut l = Sketch::default();
    l.add_line([0.0, 0.0], [5.0, 5.0]);
    assert!(l.regions().is_empty());
    // empty sketch
    assert!(Sketch::default().regions().is_empty());
    // an all-construction rectangle bounds nothing
    let mut k = Sketch::default();
    for e in k.add_rect([0.0, 0.0], [5.0, 5.0]) {
        k.entities[e].set_construction(true);
    }
    assert!(k.regions().is_empty());
}

#[test]
fn separate_lines_whose_ends_only_touch_still_close_a_loop() {
    let mut s = Sketch::default();
    s.add_line([0.0, 0.0], [10.0, 0.0]);
    s.add_line([10.0, 0.0], [10.0, 10.0]);
    s.add_line([10.0, 10.0], [0.0, 10.0]);
    s.add_line([0.0, 10.0], [0.0, 0.0]);
    let r = s.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - 100.0).abs() < 1e-9);
    assert_extrudes(&s, 5.0, 1e-6);
}

#[test]
fn nested_shapes_make_rings_with_holes() {
    // three nested squares: outer frame, middle frame, inner square
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [100.0, 100.0]);
    s.add_rect([20.0, 20.0], [80.0, 80.0]);
    s.add_rect([40.0, 40.0], [60.0, 60.0]);
    let r = s.regions();
    assert_eq!(r.len(), 3, "{:?}", sorted_areas(&r));
    assert_eq!(sorted_areas(&r).iter().map(|a| a.round() as i64).collect::<Vec<_>>(), vec![400, 3200, 6400]);
    let outer = r.iter().find(|x| (x.area() - 6400.0).abs() < 1e-6).unwrap();
    assert_eq!(outer.holes.len(), 1, "only the middle square is a hole of the outer frame");
    let inner = r.iter().find(|x| (x.area() - 400.0).abs() < 1e-6).unwrap();
    assert!(inner.holes.is_empty());
    assert_eq!(region_at(&r, [10.0, 10.0]).map(|i| r[i].area().round()), Some(6400.0));
    assert_eq!(region_at(&r, [30.0, 30.0]).map(|i| r[i].area().round()), Some(3200.0));
    assert_eq!(region_at(&r, [50.0, 50.0]).map(|i| r[i].area().round()), Some(400.0));
    assert_no_overlap(&r, [-5.0, -5.0], [105.0, 105.0], 40);
    assert_extrudes(&s, 10.0, 1e-6);

    // a plate with two holes (one round, one square)
    let mut p = Sketch::default();
    p.add_rect([0.0, 0.0], [60.0, 40.0]);
    p.add_circle([15.0, 20.0], 5.0);
    p.add_rect([35.0, 10.0], [50.0, 30.0]);
    let r = p.regions();
    assert_eq!(r.len(), 3);
    let plate = r.iter().find(|x| x.holes.len() == 2).expect("plate region with two holes");
    let want = 2400.0 - ngon_area(5.0, 72) - 300.0;
    assert!((plate.area() - want).abs() < 1e-6, "{} vs {want}", plate.area());
    assert_extrudes(&p, 4.0, 1e-6);

    // concentric circles: ring + disc
    let mut c = Sketch::default();
    c.add_circle([0.0, 0.0], 10.0);
    c.add_circle([0.0, 0.0], 4.0);
    let r = c.regions();
    assert_eq!(r.len(), 2);
    assert!((total_area(&r) - ngon_area(10.0, 72)).abs() < 1e-6);
    assert_extrudes(&c, 3.0, 1e-6);
}

#[test]
fn overlapping_rectangles_split_into_three_pieces_that_tile_the_union() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [20.0, 10.0]);
    s.add_rect([10.0, 5.0], [30.0, 15.0]);
    let r = s.regions();
    assert_eq!(r.len(), 3, "{:?}", sorted_areas(&r));
    assert_eq!(sorted_areas(&r).iter().map(|a| a.round() as i64).collect::<Vec<_>>(), vec![50, 150, 150]);
    assert_no_overlap(&r, [-1.0, -1.0], [31.0, 16.0], 40);
    assert_extrudes(&s, 2.0, 1e-6);
}

#[test]
fn rectangles_sharing_a_whole_edge_give_two_regions() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    s.add_rect([10.0, 0.0], [20.0, 10.0]);
    let r = s.regions();
    assert_eq!(sorted_areas(&r).iter().map(|a| a.round() as i64).collect::<Vec<_>>(), vec![100, 100], "{:?}", sorted_areas(&r));
    assert_no_overlap(&r, [-1.0, -1.0], [21.0, 11.0], 40);
}

#[test]
fn rectangles_sharing_part_of_an_edge_give_two_regions() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    s.add_rect([10.0, 5.0], [20.0, 15.0]);
    let r = s.regions();
    assert_eq!(sorted_areas(&r).iter().map(|a| a.round() as i64).collect::<Vec<_>>(), vec![100, 100], "{:?}", sorted_areas(&r));
    assert_no_overlap(&r, [-1.0, -1.0], [21.0, 16.0], 40);
    assert_extrudes(&s, 3.0, 1e-6);
}

#[test]
fn rectangles_touching_at_one_corner_give_two_regions() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    s.add_rect([10.0, 10.0], [20.0, 20.0]);
    let r = s.regions();
    assert_eq!(sorted_areas(&r).iter().map(|a| a.round() as i64).collect::<Vec<_>>(), vec![100, 100], "{:?}", sorted_areas(&r));
    assert_extrudes(&s, 3.0, 1e-6);
}

#[test]
fn circle_touching_a_rectangle_from_outside_keeps_both_regions() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    s.add_circle([15.0, 5.0], 5.0); // touches the right side at (10,5)
    let r = s.regions();
    assert_eq!(r.len(), 2, "{:?}", sorted_areas(&r));
    assert!((total_area(&r) - (100.0 + ngon_area(5.0, 72))).abs() < 1e-3, "{:?}", sorted_areas(&r));
}

#[test]
fn a_bow_tie_gives_two_triangles() {
    let mut s = Sketch::default();
    polyline(&mut s, &[[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]], true);
    let r = s.regions();
    assert_eq!(r.len(), 2, "{:?}", sorted_areas(&r));
    assert!(r.iter().all(|x| (x.area() - 25.0).abs() < 1e-9), "{:?}", sorted_areas(&r));
    assert_extrudes(&s, 1.0, 1e-6);
}

#[test]
fn a_line_through_a_circle_centre_gives_two_halves() {
    let mut s = Sketch::default();
    s.add_circle([0.0, 0.0], 10.0);
    s.add_line([-15.0, 0.0], [15.0, 0.0]);
    let r = s.regions();
    assert_eq!(r.len(), 2);
    for x in &r {
        assert!((x.area() - ngon_area(10.0, 72) / 2.0).abs() < 1e-6, "{:?}", sorted_areas(&r));
    }
}

#[test]
fn duplicated_rectangle_does_not_double_the_region() {
    // a user draws the same rectangle twice (copy-paste in place): regions must still tile without overlap
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    let r = s.regions();
    assert!(!r.is_empty());
    assert_no_overlap(&r, [-1.0, -1.0], [11.0, 11.0], 30);
    assert!((total_area(&r) - 100.0).abs() < 1e-6, "{:?}", sorted_areas(&r));
}

#[test]
fn repeated_vertices_and_zero_length_lines_do_not_break_a_loop() {
    // closed polyline with a repeated vertex (zero-length segment), as DXF or a double click produces
    let mut s = Sketch::default();
    polyline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], true);
    let r = s.regions();
    assert_eq!(r.len(), 1, "{:?}", sorted_areas(&r));
    assert!((r[0].area() - 100.0).abs() < 1e-9);
    assert_extrudes(&s, 2.0, 1e-6);
}

#[test]
fn tiny_and_huge_shapes_are_found() {
    for side in [0.01, 0.1, 1.0, 1000.0, 100000.0] {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [side, side]);
        let r = s.regions();
        assert_eq!(r.len(), 1, "side {side}");
        assert!((r[0].area() - side * side).abs() <= 1e-9 * side * side, "side {side}: {}", r[0].area());
    }
    for rad in [0.005, 5000.0] {
        let mut s = Sketch::default();
        s.add_circle([0.0, 0.0], rad);
        let r = s.regions();
        assert_eq!(r.len(), 1, "radius {rad}");
        assert!((r[0].area() - ngon_area(rad, 72)).abs() <= 1e-9 * rad * rad);
    }
    // a 1 mm hole in a 10 m plate
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10000.0, 10000.0]);
    s.add_circle([5000.0, 5000.0], 0.5);
    let r = s.regions();
    assert_eq!(r.len(), 2);
    assert_eq!(r.iter().filter(|x| x.holes.len() == 1).count(), 1);
    // a small rectangle far from the origin
    let mut s = Sketch::default();
    s.add_rect([5000.0, 5000.0], [5000.5, 5000.25]);
    let r = s.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - 0.125).abs() < 1e-6);
}

#[test]
fn small_shapes_extrude_with_the_right_volume() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [0.5, 0.2]);
    assert_extrudes(&s, 0.3, 1e-4);
    let mut big = Sketch::default();
    big.add_rect([0.0, 0.0], [2000.0, 1000.0]);
    big.add_circle([1000.0, 500.0], 100.0);
    assert_extrudes(&big, 50.0, 1e-6);
}

#[test]
fn ellipse_and_closed_spline_regions_extrude() {
    let mut s = Sketch::default();
    let (c, a) = (s.add_point([0.0, 0.0]), s.add_point([20.0, 0.0]));
    s.add(Entity::Ellipse { c, a, ratio: 0.5, construction: false });
    let r = s.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - PI * 200.0).abs() / (PI * 200.0) < 0.01);
    assert_extrudes(&s, 5.0, 1e-6);

    let mut sp = Sketch::default();
    let first = sp.points.len();
    for p in [[0.0, 0.0], [20.0, -3.0], [25.0, 10.0], [10.0, 18.0], [-4.0, 9.0]] {
        sp.add_point(p);
    }
    sp.add(Entity::Spline { first, count: 5, closed: true, construction: false });
    assert_eq!(sp.regions().len(), 1);
    assert_extrudes(&sp, 5.0, 1e-6);
    // the spline crossed by a line: two regions that together keep the spline's area
    let whole = sp.regions()[0].area();
    sp.add_line([-10.0, 8.0], [40.0, 8.0]);
    let r = sp.regions();
    assert_eq!(r.len(), 2, "{:?}", sorted_areas(&r));
    assert!((total_area(&r) - whole).abs() / whole < 0.005, "{} vs {whole}", total_area(&r));
}

#[test]
fn every_region_of_a_busy_sketch_extrudes_to_area_times_height() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [50.0, 30.0]);
    s.add_circle([50.0, 15.0], 8.0); // overlaps the right edge
    s.add_circle([15.0, 15.0], 6.0); // hole
    s.add_line([25.0, -5.0], [25.0, 35.0]); // splits the plate
    let r = s.regions();
    assert!(r.len() >= 5, "{:?}", sorted_areas(&r));
    assert_no_overlap(&r, [-6.0, -6.0], [60.0, 36.0], 50);
    assert_extrudes(&s, 7.0, 1e-5);
}

// ------------------------------------------------------------------ constraints and dimensions

#[test]
fn every_constraint_kind_is_satisfied_after_solving() {
    let mut s = Sketch::default();
    let l1 = s.add_line([0.0, 0.0], [10.0, 1.0]);
    let l2 = s.add_line([0.0, 5.0], [8.0, 9.0]);
    let l3 = s.add_line([20.0, 0.0], [21.0, 12.0]);
    let c1 = s.add_circle([30.0, 30.0], 3.0);
    let c2 = s.add_circle([40.0, 31.0], 5.0);
    let l4 = s.add_line([-10.0, -10.0], [-3.0, -6.0]);
    let (a0, _) = (0usize, 1usize);
    s.constraints.push(Constraint::Fix(a0, [0.0, 0.0]));
    s.constraints.push(Constraint::Horizontal(l1));
    s.constraints.push(Constraint::Length(l1, n(12.0)));
    s.constraints.push(Constraint::Perpendicular(l1, l3));
    s.constraints.push(Constraint::Parallel(l1, l2));
    s.constraints.push(Constraint::Equal(l2, l1));
    s.constraints.push(Constraint::Diameter(c1, n(8.0)));
    s.constraints.push(Constraint::Equal(c1, c2));
    s.constraints.push(Constraint::Tangent(c1, c2));
    s.constraints.push(Constraint::Tangent(l2, c1));
    s.constraints.push(Constraint::Angle(l1, l4, n(30.0)));
    s.constraints.push(Constraint::Length(l4, n(5.0)));
    let status = s.solve(&params(), &[]).unwrap();
    assert_eq!(status, SolveStatus::Ok);
    let (a, b) = line_pts(&s, l1);
    assert!((a - DVec2::ZERO).length() < 1e-6 && (a.y - b.y).abs() < 1e-6 && ((a - b).length() - 12.0).abs() < 1e-6);
    let (c, d) = line_pts(&s, l2);
    assert!((c.y - d.y).abs() < 1e-6, "parallel to a horizontal line");
    assert!(((c - d).length() - 12.0).abs() < 1e-6, "equal length");
    let (e, f) = line_pts(&s, l3);
    assert!((e.x - f.x).abs() < 1e-6, "perpendicular to a horizontal line");
    let (r1, r2) = (s.radius(c1).unwrap(), s.radius(c2).unwrap());
    assert!((r1 - 4.0).abs() < 1e-6 && (r2 - 4.0).abs() < 1e-6);
    let (Entity::Circle { c: k1, .. }, Entity::Circle { c: k2, .. }) = (s.entities[c1], s.entities[c2]) else { panic!() };
    let dist = s.p(k1).distance(s.p(k2));
    assert!((dist - 8.0).abs() < 1e-6 || dist.abs() < 1e-6, "tangent circles: centres {dist} apart");
    assert!(((s.p(k1).y - c.y).abs() - 4.0).abs() < 1e-6, "line l2 tangent to c1");
    let (g, h) = line_pts(&s, l4);
    let u = (h - g).normalize();
    let ang = u.y.atan2(u.x).abs().to_degrees();
    assert!((ang - 30.0).abs() < 1e-5 || (ang - 150.0).abs() < 1e-5, "angle {ang}");
    assert!(((h - g).length() - 5.0).abs() < 1e-6);
    constraints_hold(&s, 1e-6);
}

#[test]
fn point_dimensions_midpoint_and_point_on_curve() {
    let mut s = Sketch::default();
    let l = s.add_line([0.0, 0.0], [10.0, 0.0]);
    let p = s.add_point([3.0, 4.0]);
    let q = s.add_point([7.0, 1.0]);
    let c = s.add_circle([20.0, 0.0], 5.0);
    let on = s.add_point([26.0, 2.0]);
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    s.constraints.push(Constraint::Fix(1, [10.0, 0.0]));
    s.constraints.push(Constraint::Midpoint(p, l));
    s.constraints.push(Constraint::HDistance(p, q, n(4.0)));
    s.constraints.push(Constraint::VDistance(p, q, n(3.0)));
    s.constraints.push(Constraint::PointOn(on, c));
    assert_eq!(s.solve(&params(), &[]).unwrap(), SolveStatus::Ok);
    assert!((s.p(p) - DVec2::new(5.0, 0.0)).length() < 1e-6, "{:?}", s.p(p));
    // horizontal/vertical distances keep the side the point was on before solving (q was below p)
    assert!(((s.p(q).x - s.p(p).x) - 4.0).abs() < 1e-6 && ((s.p(q).y - s.p(p).y) + 3.0).abs() < 1e-6, "{:?} {:?}", s.p(p), s.p(q));
    let Entity::Circle { c: cc, r, .. } = s.entities[c] else { panic!() };
    assert!((s.p(on).distance(s.p(cc)) - r).abs() < 1e-6);
    // a plain point-to-point distance
    let mut t = Sketch::default();
    let (a, b) = (t.add_point([0.0, 0.0]), t.add_point([3.0, 3.0]));
    t.constraints.push(Constraint::Fix(a, [0.0, 0.0]));
    t.constraints.push(Constraint::Distance(a, b, n(10.0)));
    assert_eq!(t.solve(&params(), &[]).unwrap(), SolveStatus::Ok);
    assert!((t.p(b).length() - 10.0).abs() < 1e-6);
}

#[test]
fn coincident_points_by_merging_keep_a_closed_profile_through_solving() {
    // a triangle drawn with one corner left slightly apart, then snapped together (coincident)
    let mut s = Sketch::default();
    let p0 = s.add_point([0.0, 0.0]);
    let p1 = s.add_point([10.0, 0.0]);
    let p1b = s.add_point([10.2, 0.1]);
    let p2 = s.add_point([10.0, 10.0]);
    let l1 = s.add(Entity::Line { a: p0, b: p1, construction: false });
    let l2 = s.add(Entity::Line { a: p1b, b: p2, construction: false });
    s.add(Entity::Line { a: p2, b: p0, construction: false });
    s.merge_points(p1b, p1);
    assert_eq!(s.points.len(), 3, "three corners after merging");
    assert_eq!(s.entities.len(), 3);
    let shared = |s: &Sketch| match (s.entities[l1], s.entities[l2]) {
        (Entity::Line { b, .. }, Entity::Line { a, .. }) => a == b,
        _ => false,
    };
    assert!(shared(&s), "the two lines now share the corner point");
    s.constraints.push(Constraint::Horizontal(l1));
    s.constraints.push(Constraint::Vertical(l2));
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    s.constraints.push(Constraint::Length(l1, n(20.0)));
    s.constraints.push(Constraint::Length(l2, n(15.0)));
    assert_eq!(s.solve(&params(), &[]).unwrap(), SolveStatus::Ok);
    assert_eq!(s.free_dof(&params()), 0);
    let r = s.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - 150.0).abs() < 1e-6, "{}", r[0].area());
    assert_extrudes(&s, 4.0, 1e-6);
}

#[test]
fn fully_dimensioned_rectangle_with_a_redundant_but_consistent_dimension_solves() {
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [10.0, 5.0]);
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    s.constraints.push(Constraint::Length(l[0], n(30.0)));
    s.constraints.push(Constraint::Length(l[1], n(20.0)));
    // redundant: the opposite side has the same length anyway
    s.constraints.push(Constraint::Length(l[2], n(30.0)));
    let t = Instant::now();
    let st = s.solve(&params(), &[]).unwrap();
    assert!(t.elapsed().as_secs_f64() < 2.0);
    assert_eq!(st, SolveStatus::Ok);
    assert!((s.regions()[0].area() - 600.0).abs() < 1e-6);
}

#[test]
fn conflicting_constraints_report_conflict_quickly_and_do_not_panic() {
    let cases: Vec<(&str, Box<dyn Fn(&mut Sketch)>)> = vec![
        ("two lengths", Box::new(|s: &mut Sketch| {
            let l = s.add_line([0.0, 0.0], [10.0, 0.0]);
            s.constraints.push(Constraint::Length(l, n(10.0)));
            s.constraints.push(Constraint::Length(l, n(20.0)));
        })),
        ("horizontal and vertical with length", Box::new(|s: &mut Sketch| {
            let l = s.add_line([0.0, 0.0], [10.0, 1.0]);
            s.constraints.push(Constraint::Horizontal(l));
            s.constraints.push(Constraint::Vertical(l));
            s.constraints.push(Constraint::Length(l, n(5.0)));
        })),
        ("parallel and perpendicular with lengths", Box::new(|s: &mut Sketch| {
            let a = s.add_line([0.0, 0.0], [10.0, 0.0]);
            let b = s.add_line([0.0, 5.0], [10.0, 7.0]);
            s.constraints.push(Constraint::Parallel(a, b));
            s.constraints.push(Constraint::Perpendicular(a, b));
            s.constraints.push(Constraint::Length(a, n(10.0)));
            s.constraints.push(Constraint::Length(b, n(10.0)));
        })),
        ("triangle inequality", Box::new(|s: &mut Sketch| {
            let ls = polyline(s, &[[0.0, 0.0], [10.0, 0.0], [5.0, 5.0]], true);
            s.constraints.push(Constraint::Length(ls[0], n(1.0)));
            s.constraints.push(Constraint::Length(ls[1], n(1.0)));
            s.constraints.push(Constraint::Length(ls[2], n(10.0)));
        })),
        ("fixed points with a distance", Box::new(|s: &mut Sketch| {
            let (a, b) = (s.add_point([0.0, 0.0]), s.add_point([10.0, 0.0]));
            s.constraints.push(Constraint::Fix(a, [0.0, 0.0]));
            s.constraints.push(Constraint::Fix(b, [10.0, 0.0]));
            s.constraints.push(Constraint::Distance(a, b, n(3.0)));
        })),
        ("negative length", Box::new(|s: &mut Sketch| {
            let l = s.add_line([0.0, 0.0], [10.0, 0.0]);
            s.constraints.push(Constraint::Length(l, n(-5.0)));
        })),
        ("tangent circles with fixed centres too far apart", Box::new(|s: &mut Sketch| {
            let c1 = s.add_circle([0.0, 0.0], 1.0);
            let c2 = s.add_circle([50.0, 0.0], 1.0);
            s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
            s.constraints.push(Constraint::Fix(1, [50.0, 0.0]));
            s.constraints.push(Constraint::Diameter(c1, n(2.0)));
            s.constraints.push(Constraint::Diameter(c2, n(2.0)));
            s.constraints.push(Constraint::Tangent(c1, c2));
        })),
    ];
    for (name, build) in cases {
        let mut s = Sketch::default();
        build(&mut s);
        let t = Instant::now();
        let r = catch_unwind(AssertUnwindSafe(|| s.solve(&params(), &[])));
        let secs = t.elapsed().as_secs_f64();
        let r = r.unwrap_or_else(|_| panic!("{name}: solver panicked"));
        assert!(matches!(r, Ok(SolveStatus::Conflict(_))), "{name}: {r:?}");
        assert!(secs < 2.0, "{name}: took {secs} s");
        // all geometry stays finite even after giving up
        assert!(s.points.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite()), "{name}: {:?}", s.points);
    }
}

#[test]
fn contradictory_direction_constraints_do_not_collapse_lines() {
    let mut s = Sketch::default();
    let a = s.add_line([0.0, 0.0], [10.0, 0.0]);
    let b = s.add_line([0.0, 5.0], [10.0, 7.0]);
    s.constraints.push(Constraint::Parallel(a, b));
    s.constraints.push(Constraint::Perpendicular(a, b));
    let st = s.solve(&params(), &[]).unwrap();
    let (la, lb) = (s.measure(a), s.measure(b));
    assert!(matches!(st, SolveStatus::Conflict(_)), "status {st:?}; line lengths after solving: {la} and {lb}");
    assert!(la > 1.0 && lb > 1.0, "a line was collapsed to a point: {la}, {lb}");

    let mut t = Sketch::default();
    let l = t.add_line([0.0, 0.0], [10.0, 1.0]);
    t.constraints.push(Constraint::Horizontal(l));
    t.constraints.push(Constraint::Vertical(l));
    let st = t.solve(&params(), &[]).unwrap();
    assert!(matches!(st, SolveStatus::Conflict(_)), "horizontal+vertical: {st:?}, length now {}", t.measure(l));
}

#[test]
fn negative_diameter_is_not_silently_accepted() {
    let mut s = Sketch::default();
    let c = s.add_circle([0.0, 0.0], 3.0);
    s.constraints.push(Constraint::Diameter(c, n(-5.0)));
    let st = s.solve(&params(), &[]).unwrap();
    assert!(matches!(st, SolveStatus::Conflict(_)), "status {st:?}, radius now {:?}", s.radius(c));
}

#[test]
fn bad_expressions_in_dimensions_are_errors_not_panics() {
    for e in ["unknown_param", "1/0", "sqrt(-1)", "", "2*", "((3)"] {
        let mut s = Sketch::default();
        let l = s.add_line([0.0, 0.0], [10.0, 0.0]);
        s.constraints.push(Constraint::Length(l, Expr::from(e)));
        let r = catch_unwind(AssertUnwindSafe(|| s.solve(&params(), &[]))).unwrap_or_else(|_| panic!("{e:?} panicked"));
        assert!(r.is_err(), "{e:?} gave {r:?}");
        // and inside a document the sketch step reports an error
        let mut d = Document::default();
        let uid = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s.clone() });
        let ev = run(&d);
        assert!(ev.error(uid).is_some(), "{e:?}: no step error");
    }
    // a parameter drives the dimension
    let mut s = Sketch::default();
    let l = s.add_line([0.0, 0.0], [10.0, 0.0]);
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    s.constraints.push(Constraint::Horizontal(l));
    s.constraints.push(Constraint::Length(l, Expr::from("w*2+1")));
    let mut p = Params::default();
    p.set("w", "7");
    assert_eq!(s.solve(&p, &[]).unwrap(), SolveStatus::Ok);
    assert!((s.measure(l) - 15.0).abs() < 1e-6);
}

#[test]
fn degenerate_dimensions_do_not_hang() {
    // zero length line and coincident-by-distance points
    let mut s = Sketch::default();
    let l = s.add_line([0.0, 0.0], [10.0, 0.0]);
    s.constraints.push(Constraint::Length(l, n(0.0)));
    let t = Instant::now();
    let r = catch_unwind(AssertUnwindSafe(|| s.solve(&params(), &[]))).expect("no panic");
    assert!(r.is_ok());
    assert!(t.elapsed().as_secs_f64() < 2.0);
    assert!(s.measure(l) < 1e-3, "pulled to zero length: {}", s.measure(l));
    // angle 0 and 180 between lines
    for deg in [0.0, 180.0, 90.0] {
        let mut s = Sketch::default();
        let a = s.add_line([0.0, 0.0], [10.0, 0.0]);
        let b = s.add_line([0.0, 5.0], [7.0, 9.0]);
        s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
        s.constraints.push(Constraint::Fix(1, [10.0, 0.0]));
        s.constraints.push(Constraint::Angle(a, b, n(deg)));
        let t = Instant::now();
        let st = s.solve(&params(), &[]).unwrap();
        assert!(t.elapsed().as_secs_f64() < 2.0);
        let (c, d) = line_pts(&s, b);
        let u = (d - c).normalize();
        let got = u.y.atan2(u.x).abs().to_degrees();
        assert_eq!(st, SolveStatus::Ok, "angle {deg}: got {got}");
        assert!((got - deg).abs() < 1e-3, "angle {deg}: got {got}");
    }
}

#[test]
fn solver_handles_a_sketch_with_200_entities_quickly() {
    // 50 dimensioned rectangles, each fixed at a corner: 200 lines, 200 points
    let mut s = Sketch::default();
    for i in 0..50 {
        let x = (i % 10) as f64 * 30.0;
        let y = (i / 10) as f64 * 30.0;
        let l = s.add_rect([x, y], [x + 11.0, y + 6.0]);
        let Entity::Line { a, .. } = s.entities[l[0]] else { unreachable!() };
        s.constraints.push(Constraint::Fix(a, [x, y]));
        s.constraints.push(Constraint::Length(l[0], n(20.0)));
        s.constraints.push(Constraint::Length(l[1], n(10.0)));
    }
    assert_eq!(s.entities.len(), 200);
    let t = Instant::now();
    let st = s.solve(&params(), &[]).unwrap();
    let secs = t.elapsed().as_secs_f64();
    println!("200-entity solve: {secs:.3} s");
    assert_eq!(st, SolveStatus::Ok);
    // debug build: generous bound (the release app is ~10-30x faster)
    assert!(secs < 10.0, "solving 200 entities took {secs} s");
    let r = s.regions();
    assert_eq!(r.len(), 50);
    assert!(r.iter().all(|x| (x.area() - 200.0).abs() < 1e-6));
}

#[test]
fn solver_handles_a_chained_200_segment_polygon() {
    // one closed 200-gon with shared corners, every side dimensioned equal to the first, first corner fixed
    let mut s = Sketch::default();
    let k = 200;
    let pts: Vec<[f64; 2]> = (0..k).map(|i| {
        let t = 2.0 * PI * i as f64 / k as f64;
        [100.0 * t.cos() + 0.3 * (i as f64 * 1.7).sin(), 100.0 * t.sin()]
    }).collect();
    let ls = polyline(&mut s, &pts, true);
    s.constraints.push(Constraint::Fix(0, pts[0]));
    for w in ls.windows(2) {
        s.constraints.push(Constraint::Equal(w[0], w[1]));
    }
    let t = Instant::now();
    let st = s.solve(&params(), &[]).unwrap();
    let secs = t.elapsed().as_secs_f64();
    println!("200-gon solve: {secs:.3} s, {st:?}");
    assert_eq!(st, SolveStatus::Ok);
    assert!(secs < 10.0, "took {secs} s");
    let len0 = s.measure(ls[0]);
    // each Equal residual is below the solver tolerance, so 199 chained equalities may drift a little in total
    assert!(ls.iter().all(|&l| (s.measure(l) - len0).abs() < 1e-4), "{:?}", ls.iter().map(|&l| s.measure(l) - len0).fold(0.0f64, |m, v| m.max(v.abs())));
    assert_eq!(s.regions().len(), 1);
}

#[test]
fn free_dof_counts_what_is_left() {
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [10.0, 5.0]);
    assert_eq!(s.free_dof(&params()), 4, "rectangle: position (2) + width + height");
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    assert_eq!(s.free_dof(&params()), 2);
    s.constraints.push(Constraint::Length(l[0], n(10.0)));
    assert_eq!(s.free_dof(&params()), 1);
    s.constraints.push(Constraint::Length(l[1], n(5.0)));
    assert_eq!(s.free_dof(&params()), 0);
    let mut c = Sketch::default();
    c.add_circle([0.0, 0.0], 3.0);
    assert_eq!(c.free_dof(&params()), 3);
}

// ------------------------------------------------------------------ trim / offset / fillet / mirror / pattern

#[test]
fn trimming_overhangs_leaves_two_regions_that_extrude() {
    // a rectangle with a vertical line sticking out above and below; trim both stubs
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [20.0, 10.0]);
    let l = s.add_line([8.0, -5.0], [8.0, 15.0]);
    assert_eq!(s.regions().len(), 2);
    assert!(s.trim(l, [8.0, -3.0]));
    let n_before = s.entities.len();
    // the line was shortened, still the same entity count
    assert_eq!(n_before, 5);
    let line = s.entities.len() - 1;
    let _ = line;
    let stub = (0..s.entities.len()).find(|&e| matches!(s.entities[e], Entity::Line { .. }) && {
        let (a, b) = line_pts(&s, e);
        (a.x - 8.0).abs() < 1e-9 && (b.x - 8.0).abs() < 1e-9
    }).unwrap();
    assert!(s.trim(stub, [8.0, 13.0]));
    let (a, b) = line_pts(&s, stub);
    assert!((a.y.min(b.y)).abs() < 1e-9 && (a.y.max(b.y) - 10.0).abs() < 1e-9, "{a:?} {b:?}");
    let r = s.regions();
    assert_eq!(sorted_areas(&r).iter().map(|x| x.round() as i64).collect::<Vec<_>>(), vec![80, 120]);
    assert_extrudes(&s, 6.0, 1e-6);
}

#[test]
fn trimming_the_middle_of_a_rectangle_side_opens_it() {
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [20.0, 10.0]);
    s.add_line([5.0, -5.0], [5.0, 15.0]);
    s.add_line([15.0, -5.0], [15.0, 15.0]);
    // cut the top side between the two vertical lines
    assert!(s.trim(l[2], [10.0, 10.0]));
    let r = s.regions();
    // left and right parts stay closed, the middle opens upwards... but the middle still closes with the bottom side
    // and both verticals up to their crossing with the top: top is gone in the middle, so the middle is open
    assert_eq!(r.len(), 2, "{:?}", sorted_areas(&r));
    assert!(r.iter().all(|x| (x.area() - 50.0).abs() < 1e-6), "{:?}", sorted_areas(&r));
    assert_extrudes(&s, 2.0, 1e-6);
}

#[test]
fn trimming_a_line_without_crossings_deletes_it() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    let l = s.add_line([20.0, 0.0], [30.0, 0.0]);
    assert!(s.trim(l, [25.0, 0.0]));
    assert_eq!(s.entities.len(), 4);
    assert_eq!(s.regions().len(), 1);
}

#[test]
fn trimming_a_circle_by_two_lines_keeps_the_far_arc() {
    let mut s = Sketch::default();
    let c = s.add_circle([0.0, 0.0], 10.0);
    s.add_line([-6.0, -20.0], [-6.0, 20.0]);
    s.add_line([6.0, -20.0], [6.0, 20.0]);
    // click on the right part of the circle, outside the strip: removes the right cap
    assert!(s.trim(c, [10.0, 0.0]));
    assert!(matches!(s.entities[c], Entity::Arc { .. }));
    let pts = s.tessellate(c);
    assert!(pts.iter().all(|p| p[0] <= 6.0 + 1e-6), "the right cap is gone");
    assert!(pts.iter().any(|p| p[0] < -9.0), "the left side stays");
    // the arc + the two lines still bound something: the strip piece and the left cap
    let r = s.regions();
    assert_eq!(r.len(), 2, "{:?}", sorted_areas(&r));
    // trim the remaining arc's left cap away as well: clicking at the far left leaves two separate arcs
    assert!(s.trim(c, [-10.0, 0.0]));
    let arcs = s.entities.iter().filter(|e| matches!(e, Entity::Arc { .. })).count();
    assert_eq!(arcs, 2, "top and bottom arcs remain");
    let r = s.regions();
    assert_eq!(r.len(), 1, "only the strip piece between the lines");
    let strip = r[0].area();
    // exact: area of a circle strip |x| <= 6 for r = 10
    let exact = 2.0 * (6.0 * (100.0f64 - 36.0).sqrt() + 100.0 * (0.6f64).asin());
    assert!((strip - exact).abs() / exact < 0.01, "{strip} vs {exact}");
    assert_extrudes(&s, 3.0, 1e-6);
}

#[test]
fn a_refused_circle_trim_leaves_the_sketch_untouched() {
    let mut s = Sketch::default();
    let c = s.add_circle([0.0, 0.0], 5.0);
    s.add_line([-10.0, 5.0], [10.0, 5.0]); // tangent: one touching point only
    s.constraints.push(Constraint::Diameter(c, n(10.0)));
    let before = s.clone();
    assert!(!s.trim(c, [0.0, -5.0]), "nothing to trim on a circle touched once");
    assert_eq!(s, before, "a trim that did nothing must not change the sketch (the diameter dimension vanished)");
}

#[test]
fn trimming_arcs_and_lines_against_arcs() {
    // a slot shape (two lines + two half circles) with a line across: trim the line ends outside
    let mut s = Sketch::default();
    let p = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0], [20.0, 5.0], [0.0, 5.0]].map(|q| s.add_point(q));
    s.add(Entity::Line { a: p[0], b: p[1], construction: false });
    let right = s.add(Entity::Arc { c: p[4], a: p[1], b: p[2], construction: false });
    s.add(Entity::Line { a: p[2], b: p[3], construction: false });
    s.add(Entity::Arc { c: p[5], a: p[3], b: p[0], construction: false });
    let cut = s.add_line([-10.0, 5.0], [30.0, 5.0]);
    assert_eq!(s.regions().len(), 2);
    assert!(s.trim(cut, [-8.0, 5.0]));
    let cut2 = (0..s.entities.len()).find(|&e| matches!(s.entities[e], Entity::Line { .. }) && {
        let (a, b) = line_pts(&s, e);
        (a.y - 5.0).abs() < 1e-9 && (b.y - 5.0).abs() < 1e-9
    }).unwrap();
    assert!(s.trim(cut2, [28.0, 5.0]));
    let (a, b) = line_pts(&s, cut2);
    assert!((a.x.min(b.x) + 5.0).abs() < 1e-6 && (a.x.max(b.x) - 25.0).abs() < 1e-6, "{a:?} {b:?}");
    let r = s.regions();
    assert_eq!(r.len(), 2);
    let want = 200.0 + PI * 25.0;
    assert!((total_area(&r) - want).abs() / want < 0.005, "{} vs {want}", total_area(&r));
    // trim the upper half of the right arc: the right end opens at the top
    assert!(s.trim(right, [24.0, 8.0]));
    let r = s.regions();
    assert_eq!(r.len(), 1, "{:?}", sorted_areas(&r));
    assert_extrudes(&s, 2.0, 1e-6);
}

#[test]
fn offsets_of_lines_circles_and_arcs() {
    let mut s = Sketch::default();
    let l = s.add_line([0.0, 0.0], [10.0, 0.0]);
    let up = s.offset(l, [3.0, 7.0], 2.0).unwrap();
    let down = s.offset(l, [3.0, -7.0], -2.0).unwrap();
    let (a, b) = line_pts(&s, up);
    assert!((a.y - 2.0).abs() < 1e-12 && (b.y - 2.0).abs() < 1e-12);
    let (a, b) = line_pts(&s, down);
    assert!((a.y + 2.0).abs() < 1e-12 && (b.y + 2.0).abs() < 1e-12, "negative distance still goes towards the side point");
    assert!(s.offset(l, [3.0, 7.0], 0.0).is_none());

    let c = s.add_circle([50.0, 0.0], 10.0);
    let out = s.offset(c, [70.0, 0.0], 3.0).unwrap();
    let inn = s.offset(c, [51.0, 0.0], 3.0).unwrap();
    assert!((s.radius(out).unwrap() - 13.0).abs() < 1e-12);
    assert!((s.radius(inn).unwrap() - 7.0).abs() < 1e-12);
    assert!(s.offset(c, [51.0, 0.0], 10.0).is_none(), "an inward offset as large as the radius vanishes");
    assert!(s.offset(c, [51.0, 0.0], 25.0).is_none());

    let (cc, pa, pb) = (s.add_point([0.0, 50.0]), s.add_point([10.0, 50.0]), s.add_point([0.0, 60.0]));
    let arc = s.add(Entity::Arc { c: cc, a: pa, b: pb, construction: false });
    let ao = s.offset(arc, [20.0, 70.0], 5.0).unwrap();
    let ai = s.offset(arc, [1.0, 51.0], 5.0).unwrap();
    assert!((s.radius(ao).unwrap() - 15.0).abs() < 1e-12);
    assert!((s.radius(ai).unwrap() - 5.0).abs() < 1e-12);
    // offsets keep the sweep: quarter arcs
    let span = |s: &Sketch, e: usize| {
        let t = s.tessellate(e);
        let (f, l) = (t[0], t[t.len() - 1]);
        let Entity::Arc { c, .. } = s.entities[e] else { panic!() };
        let c = s.p(c);
        let a0 = (f[1] - c.y).atan2(f[0] - c.x);
        let a1 = (l[1] - c.y).atan2(l[0] - c.x);
        (a1 - a0).to_degrees()
    };
    assert!((span(&s, ao) - 90.0).abs() < 1e-6 && (span(&s, ai) - 90.0).abs() < 1e-6);
    // the sketch with all the offset constraints still solves without moving anything
    constraints_hold(&s, 1e-6);
}

#[test]
fn offset_circle_inside_a_circle_makes_a_ring_that_extrudes() {
    let mut s = Sketch::default();
    let c = s.add_circle([0.0, 0.0], 20.0);
    s.offset(c, [0.0, 0.0], 4.0).unwrap();
    let r = s.regions();
    assert_eq!(r.len(), 2);
    let ring = r.iter().find(|x| x.holes.len() == 1).unwrap();
    assert!((ring.area() - (ngon_area(20.0, 72) - ngon_area(16.0, 72))).abs() < 1e-6);
    assert_extrudes(&s, 5.0, 1e-6);
}

#[test]
fn filleting_all_four_corners_of_a_rectangle() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [40.0, 20.0]);
    let corners: Vec<[f64; 2]> = vec![[0.0, 0.0], [40.0, 0.0], [40.0, 20.0], [0.0, 20.0]];
    for c in corners {
        let p = s.points.iter().position(|q| q.pos == c).unwrap();
        s.fillet_corner(p, 5.0).unwrap();
    }
    assert_eq!(s.entities.iter().filter(|e| matches!(e, Entity::Arc { .. })).count(), 4);
    let st = s.solve(&params(), &[]).unwrap();
    assert_eq!(st, SolveStatus::Ok);
    let r = s.regions();
    assert_eq!(r.len(), 1);
    let want = 800.0 - 4.0 * 25.0 * (1.0 - PI / 4.0);
    assert!((r[0].area() - want).abs() / want < 0.002, "{} vs {want}", r[0].area());
    assert_extrudes(&s, 10.0, 1e-6);
}

#[test]
fn fillet_of_an_acute_corner_and_refusals() {
    let mut s = Sketch::default();
    polyline(&mut s, &[[0.0, 0.0], [30.0, 0.0], [0.0, 17.320508075688775]], true); // 30-60-90 triangle
    let p = s.points.iter().position(|q| q.pos == [30.0, 0.0]).unwrap();
    s.fillet_corner(p, 2.0).unwrap();
    let r = s.regions();
    assert_eq!(r.len(), 1);
    let tri = 0.5 * 30.0 * 17.320508075688775;
    // removed area at a corner of angle θ with fillet r: r² (cot(θ/2) - (π - θ)/2)
    let th = 30.0f64.to_radians();
    let removed = 4.0 * (1.0 / (th / 2.0).tan() - (PI - th) / 2.0);
    assert!((r[0].area() - (tri - removed)).abs() < 0.05, "{} vs {}", r[0].area(), tri - removed);

    // too large a radius is refused and leaves the sketch unchanged
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    let before = s.clone();
    let p = s.points.iter().position(|q| q.pos == [10.0, 0.0]).unwrap();
    assert!(s.fillet_corner(p, 20.0).is_err());
    assert_eq!(s, before);
    assert!(s.fillet_corner(p, 0.0).is_err());
    assert!(s.fillet_corner(p, -1.0).is_err());
    assert_eq!(s, before);
    // the end of an open line is no corner
    let mut s = Sketch::default();
    s.add_line([0.0, 0.0], [10.0, 0.0]);
    assert!(s.fillet_corner(0, 1.0).is_err());
    // collinear lines are no corner
    let mut s = Sketch::default();
    polyline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [20.0, 0.0]], false);
    assert!(s.fillet_corner(1, 1.0).is_err());
}

#[test]
fn fillet_keeps_dimensioned_rectangle_solvable() {
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [30.0, 20.0]);
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    s.constraints.push(Constraint::Length(l[0], n(30.0)));
    s.constraints.push(Constraint::Length(l[1], n(20.0)));
    let p = s.points.iter().position(|q| q.pos == [30.0, 20.0]).unwrap();
    s.fillet_corner(p, 4.0).unwrap();
    let st = s.solve(&params(), &[]).unwrap();
    assert_eq!(st, SolveStatus::Ok);
    let r = s.regions();
    assert_eq!(r.len(), 1);
    let want = 600.0 - 16.0 * (1.0 - PI / 4.0);
    assert!((r[0].area() - want).abs() < 0.1, "{} vs {want}", r[0].area());
}

#[test]
fn fillet_works_on_an_imported_dxf_rectangle() {
    let mut src = Sketch::default();
    src.add_rect([0.0, 0.0], [20.0, 10.0]);
    let mut s = Sketch::default();
    dxf::import(&mut s, &dxf::export(&src)).unwrap();
    let p = s.points.iter().position(|q| q.pos == [20.0, 0.0]).unwrap();
    assert!(s.fillet_corner(p, 2.0).is_ok(), "fillet of an imported corner: {:?}", s.fillet_corner(p, 2.0));
}

#[test]
fn mirroring_a_shape_onto_its_own_edge_gives_two_touching_regions() {
    let mut s = Sketch::default();
    let r = s.add_rect([0.0, 0.0], [5.0, 3.0]);
    s.mirror_entities(&r, DVec2::new(0.0, 0.0), DVec2::new(0.0, 1.0));
    let regs = s.regions();
    assert_eq!(sorted_areas(&regs).iter().map(|x| x.round() as i64).collect::<Vec<_>>(), vec![15, 15], "{:?}", sorted_areas(&regs));
    assert_no_overlap(&regs, [-6.0, -1.0], [6.0, 4.0], 30);
    assert_extrudes(&s, 2.0, 1e-6);

    // a half disc mirrored across its diameter: a whole disc split in two halves
    let mut h = Sketch::default();
    let (c, a, b) = (h.add_point([0.0, 0.0]), h.add_point([10.0, 0.0]), h.add_point([-10.0, 0.0]));
    let arc = h.add(Entity::Arc { c, a, b, construction: false });
    let dia = h.add(Entity::Line { a: b, b: a, construction: false });
    h.mirror_entities(&[arc, dia], DVec2::new(-1.0, 0.0), DVec2::new(1.0, 0.0));
    let regs = h.regions();
    assert_eq!(regs.len(), 2, "{:?}", sorted_areas(&regs));
    let half = regs[0].area();
    assert!((regs[1].area() - half).abs() < 1e-6);
    assert!(regs.iter().any(|x| x.outer.iter().all(|p| p[1] <= 1e-9)), "the mirrored half lies below");
    // mirror across a slanted axis keeps areas
    let mut t = Sketch::default();
    polyline(&mut t, &[[2.0, 0.0], [6.0, 0.0], [4.0, 3.0]], true);
    let ents: Vec<usize> = (0..t.entities.len()).collect();
    t.mirror_entities(&ents, DVec2::new(0.0, 0.0), DVec2::new(1.0, 1.0));
    let regs = t.regions();
    assert_eq!(regs.len(), 2);
    assert!(regs.iter().all(|x| (x.area() - 6.0).abs() < 1e-9), "{:?}", sorted_areas(&regs));
    // degenerate mirror axis (both points the same) does not panic
    let mut u = Sketch::default();
    let e = u.add_rect([1.0, 1.0], [2.0, 2.0]);
    let out = catch_unwind(AssertUnwindSafe(|| u.mirror_entities(&e, DVec2::ZERO, DVec2::ZERO)));
    assert!(out.is_ok());
    assert!(u.points.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite()));
}

#[test]
fn patterns_make_the_right_number_of_regions() {
    let mut s = Sketch::default();
    let c = s.add_circle([10.0, 0.0], 2.0);
    let made = s.pattern_circular(&[c], DVec2::ZERO, 8, 360.0);
    assert_eq!(made.len(), 7);
    let r = s.regions();
    assert_eq!(r.len(), 8);
    assert!(r.iter().all(|x| (x.area() - ngon_area(2.0, 72)).abs() < 1e-6));
    assert_extrudes(&s, 3.0, 1e-6);
    // a plate with a bolt circle of holes
    s.add_circle([0.0, 0.0], 20.0);
    let r = s.regions();
    assert_eq!(r.len(), 9);
    assert_eq!(r.iter().filter(|x| x.holes.len() == 8).count(), 1);
    assert_extrudes(&s, 3.0, 1e-6);

    // rectangular pattern of squares with gap 0 (squares share sides)
    let mut g = Sketch::default();
    let sq = g.add_rect([0.0, 0.0], [5.0, 5.0]);
    g.pattern_rect(&sq, 3, 5.0, 2, 5.0);
    let r = g.regions();
    assert_eq!(r.len(), 6, "{:?}", sorted_areas(&r));
    assert!(r.iter().all(|x| (x.area() - 25.0).abs() < 1e-6), "{:?}", sorted_areas(&r));
    assert_no_overlap(&r, [-1.0, -1.0], [16.0, 11.0], 40);

    // overlapping circular pattern: regions tile the union
    let mut o = Sketch::default();
    let c = o.add_circle([6.0, 0.0], 4.0);
    o.pattern_circular(&[c], DVec2::ZERO, 4, 360.0);
    let r = o.regions();
    assert_no_overlap(&r, [-11.0, -11.0], [11.0, 11.0], 50);
    // count = 0/1, angle 0 produce no copies and no panic
    let mut z = Sketch::default();
    let c = z.add_circle([6.0, 0.0], 1.0);
    assert!(z.pattern_circular(&[c], DVec2::ZERO, 0, 360.0).is_empty());
    assert!(z.pattern_circular(&[c], DVec2::ZERO, 1, 360.0).is_empty());
    assert!(z.pattern_rect(&[c], 0, 5.0, 0, 5.0).is_empty());
    // a missing entity index is ignored
    assert!(z.pattern_rect(&[99], 2, 5.0, 1, 0.0).is_empty());
}

#[test]
fn remove_entities_and_merge_points_keep_constraints_valid() {
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [10.0, 5.0]);
    let c = s.add_circle([5.0, 2.5], 1.0);
    s.constraints.push(Constraint::Length(l[0], n(10.0)));
    s.constraints.push(Constraint::Diameter(c, n(2.0)));
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    s.remove_entities(&[l[1]]);
    // every constraint still refers to existing things
    for k in &s.constraints {
        assert!(k.entities().iter().all(|&e| e < s.entities.len()), "{k:?}");
        assert!(k.points().iter().all(|&p| p < s.points.len()), "{k:?}");
    }
    assert!(s.solve(&params(), &[]).is_ok());
    assert!(s.regions().len() == 1, "the circle remains, the opened rectangle bounds nothing");
}

// ------------------------------------------------------------------ text

#[test]
fn text_regions_extrude_to_area_times_height() {
    let mut s = Sketch::default();
    s.texts.push(SketchText { text: "Genix 08".into(), pos: [0.0, 0.0], height: 10.0, angle: 0.0 });
    let r = s.regions();
    // G e n i(2) x 0 8 = 8 bodies; holes: e(1) 0(1) 8(2)
    assert_eq!(r.len(), 8, "{} regions", r.len());
    assert_eq!(r.iter().map(|x| x.holes.len()).sum::<usize>(), 4);
    assert!(r.iter().all(|x| x.area() > 0.0));
    assert_no_overlap(&r, [-1.0, -3.0], [50.0, 12.0], 120);
    assert_extrudes(&s, 1.5, 1e-4);
}

#[test]
fn text_inside_a_rectangle_and_odd_strings() {
    let mut s = Sketch::default();
    s.add_rect([-5.0, -5.0], [40.0, 15.0]);
    s.texts.push(SketchText { text: "ПРИВЕТ".into(), pos: [0.0, 0.0], height: 8.0, angle: 0.0 });
    let r = s.regions();
    assert_eq!(r.len(), 7, "plate + 6 letters");
    // picking a point inside a letter finds the letter, not the plate
    let letter = r.iter().find(|x| x.ids.is_empty()).unwrap();
    let p = letter.interior_point();
    let i = region_at(&r, p).unwrap();
    assert!(r[i].ids.is_empty(), "the smallest region under the point is the letter");
    // odd strings never panic
    for t in ["", " ", "\n\t", "🙂", "A\nB", "\u{0}", "שָׁלוֹם", "ﷺ", &"W".repeat(200)] {
        let out = catch_unwind(|| fusion_core::text::regions(&SketchText { text: t.to_string(), pos: [0.0, 0.0], height: 5.0, angle: 0.0 }));
        assert!(out.is_ok(), "{t:?} panicked");
    }
    for h in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e-9, 1e6] {
        let out = catch_unwind(|| fusion_core::text::regions(&SketchText { text: "Ab".into(), pos: [0.0, 0.0], height: h, angle: 0.0 }));
        let out = out.unwrap_or_else(|_| panic!("height {h} panicked"));
        assert!(out.iter().all(|r| r.outer.iter().all(|p| p[0].is_finite() && p[1].is_finite())), "height {h}: non-finite outline");
    }
}

#[test]
fn rotated_text_keeps_every_letter() {
    for text in ["ET", "ПРИВЕТ", "BET"] {
        let flat = fusion_core::text::regions(&SketchText { text: text.into(), pos: [0.0, 0.0], height: 8.0, angle: 0.0 });
        for angle in [15.0, 30.0, 45.0, 90.0] {
            let turned = fusion_core::text::regions(&SketchText { text: text.into(), pos: [0.0, 0.0], height: 8.0, angle });
            let holes = |r: &[Region]| r.iter().map(|x| x.holes.len()).sum::<usize>();
            assert_eq!((turned.len(), holes(&turned)), (flat.len(), holes(&flat)), "{text:?} at {angle} deg: regions/holes differ from the unrotated text");
            assert!((total_area(&turned) - total_area(&flat)).abs() / total_area(&flat) < 1e-6, "{text:?} at {angle} deg: area {} vs {}", total_area(&turned), total_area(&flat));
            // every hole really lies inside its region
            for r in &turned {
                for h in &r.holes {
                    assert!(h.iter().all(|p| fusion_core::sketch::point_in_poly(*p, &r.outer)), "{text:?} at {angle} deg: a hole lies outside its letter");
                }
            }
        }
    }
}

#[test]
fn exploded_text_extrudes_like_the_text() {
    let mut s = Sketch::default();
    s.texts.push(SketchText { text: "B".into(), pos: [0.0, 0.0], height: 20.0, angle: 0.0 });
    let text_area = total_area(&s.regions());
    s.explode_text(0);
    let r = s.regions();
    // outer loop + two counters as separate regions; the outer region has the counters as holes
    assert_eq!(r.len(), 3, "{:?}", sorted_areas(&r));
    let body = r.iter().find(|x| x.holes.len() == 2).expect("letter body with two holes");
    assert!((body.area() - text_area).abs() / text_area < 1e-6, "{} vs {text_area}", body.area());
}

// ------------------------------------------------------------------ DXF

#[test]
fn dxf_round_trip_keeps_entities_and_regions() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [60.0, 40.0]);
    s.add_circle([15.0, 20.0], 6.0);
    let (c, a, b) = (s.add_point([45.0, 20.0]), s.add_point([50.0, 20.0]), s.add_point([40.0, 20.0]));
    s.add(Entity::Arc { c, a, b, construction: false });
    s.add_line([40.0, 20.0], [50.0, 20.0]);
    let k = s.add_line([0.0, 0.0], [60.0, 40.0]);
    s.entities[k].set_construction(true);
    let before = s.regions();
    let text = dxf::export(&s);
    let mut back = Sketch::default();
    assert_eq!(dxf::import(&mut back, &text).unwrap(), 8);
    assert_eq!(back.entities.len(), 8);
    assert!(back.entities[7].construction(), "construction layer survives");
    assert!(back.entities[..7].iter().all(|e| !e.construction()));
    let after = back.regions();
    assert_eq!(after.len(), before.len());
    for (x, y) in sorted_areas(&before).iter().zip(sorted_areas(&after)) {
        assert!((x - y).abs() < 1e-6, "{x} vs {y}");
    }
    // exporting the imported sketch again gives the same DXF
    assert_eq!(dxf::export(&back), text);
}

#[test]
fn dxf_round_trip_of_an_ellipse_keeps_one_closed_region() {
    let mut s = Sketch::default();
    let (c, a) = (s.add_point([0.0, 0.0]), s.add_point([10.0, 0.0]));
    s.add(Entity::Ellipse { c, a, ratio: 0.5, construction: false });
    let want = s.regions()[0].area();
    let mut back = Sketch::default();
    dxf::import(&mut back, &dxf::export(&s)).unwrap();
    let r = back.regions();
    assert_eq!(r.len(), 1, "{:?}", sorted_areas(&r));
    assert!((r[0].area() - want).abs() / want < 1e-6);
}

#[test]
fn dxf_round_trip_of_an_ellipse_adds_no_zero_length_line() {
    let mut s = Sketch::default();
    let (c, a) = (s.add_point([0.0, 0.0]), s.add_point([10.0, 0.0]));
    s.add(Entity::Ellipse { c, a, ratio: 0.5, construction: false });
    let mut back = Sketch::default();
    let added = dxf::import(&mut back, &dxf::export(&s)).unwrap();
    let zero: Vec<usize> = (0..back.entities.len()).filter(|&i| back.measure(i) < 1e-9).collect();
    assert!(zero.is_empty(), "{} of {added} imported lines have zero length: {zero:?}", zero.len());
}

#[test]
fn fillet_next_to_a_zero_length_line_is_refused_not_nan() {
    // closed polylines with a repeated vertex are common in DXF files from other programs
    let mut s = Sketch::default();
    polyline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]], true);
    let r = s.fillet_corner(1, 2.0);
    let finite = s.points.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite());
    assert!(finite, "fillet returned {r:?} and left non-finite points: {:?}", s.points);
    // and the project with that sketch still saves and reopens
    let mut d = Document::default();
    d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    assert!(Document::from_json(&d.to_json()).is_ok());
}

#[test]
fn dxf_round_trip_of_an_open_spline() {
    let mut s = Sketch::default();
    let first = s.points.len();
    for p in [[0.0, 0.0], [10.0, 5.0], [20.0, 0.0]] {
        s.add_point(p);
    }
    s.add(Entity::Spline { first, count: 3, closed: false, construction: false });
    s.add_line([0.0, 0.0], [20.0, 0.0]);
    let want = s.regions()[0].area();
    let mut back = Sketch::default();
    dxf::import(&mut back, &dxf::export(&s)).unwrap();
    let r = back.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - want).abs() / want < 1e-6);
}

#[test]
fn dxf_round_trip_of_arcs_in_every_quadrant_and_wrapping_zero() {
    for (start, end) in [(0.0, 90.0), (90.0, 180.0), (200.0, 300.0), (300.0, 30.0), (350.0, 10.0), (10.0, 350.0)] {
        let mut s = Sketch::default();
        let pt = |a: f64| [5.0 + 7.0 * (a as f64).to_radians().cos(), -3.0 + 7.0 * (a as f64).to_radians().sin()];
        let (c, a, b) = (s.add_point([5.0, -3.0]), s.add_point(pt(start)), s.add_point(pt(end)));
        s.add(Entity::Arc { c, a, b, construction: false });
        let t0 = s.tessellate(0);
        let mut back = Sketch::default();
        dxf::import(&mut back, &dxf::export(&s)).unwrap();
        let t1 = back.tessellate(0);
        assert_eq!(t0.len(), t1.len(), "{start}..{end}");
        for (p, q) in t0.iter().zip(&t1) {
            assert!((p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9, "{start}..{end}: {p:?} vs {q:?}");
        }
    }
}

#[test]
fn dxf_full_circle_arc_is_a_closed_region() {
    // ARC 0..360 is a whole circle in DXF terms
    let text = "0\nSECTION\n2\nENTITIES\n0\nARC\n8\n0\n10\n0\n20\n0\n40\n5\n50\n0\n51\n360\n0\nENDSEC\n0\nEOF\n";
    let mut s = Sketch::default();
    assert_eq!(dxf::import(&mut s, text).unwrap(), 1);
    let r = s.regions();
    assert_eq!(r.len(), 1, "full arc bounds a disc");
    assert!((r[0].area() - PI * 25.0).abs() / (PI * 25.0) < 0.01);
}

#[test]
fn dxf_reads_files_from_other_programs() {
    // CRLF line endings, indented group codes, a HEADER and a BLOCKS section with entities that must be ignored,
    // comments (999), a polyline with bulges, an unsupported entity type
    let text = "999\r\nmade by someone\r\n  0\r\nSECTION\r\n  2\r\nHEADER\r\n  9\r\n$ACADVER\r\n  1\r\nAC1015\r\n  0\r\nENDSEC\r\n  0\r\nSECTION\r\n  2\r\nBLOCKS\r\n  0\r\nLINE\r\n  8\r\n0\r\n 10\r\n0\r\n 20\r\n0\r\n 11\r\n1\r\n 21\r\n1\r\n  0\r\nENDSEC\r\n  0\r\nSECTION\r\n  2\r\nENTITIES\r\n  0\r\nTEXT\r\n  8\r\n0\r\n  1\r\nhello\r\n  0\r\nLWPOLYLINE\r\n  8\r\n0\r\n 90\r\n4\r\n 70\r\n1\r\n 10\r\n0.0\r\n 20\r\n0.0\r\n 10\r\n20.0\r\n 20\r\n0.0\r\n 42\r\n1.0\r\n 10\r\n20.0\r\n 20\r\n10.0\r\n 10\r\n0.0\r\n 20\r\n10.0\r\n  0\r\nENDSEC\r\n  0\r\nEOF\r\n";
    let mut s = Sketch::default();
    assert_eq!(dxf::import(&mut s, text).unwrap(), 4, "only the 4 polyline segments; the block line and the text are skipped");
    let r = s.regions();
    assert_eq!(r.len(), 1);
    let want = 200.0 + PI * 25.0 / 2.0;
    assert!((r[0].area() - want).abs() / want < 0.005, "{} vs {want}", r[0].area());
    assert_extrudes(&s, 3.0, 1e-6);
}

#[test]
fn malformed_dxf_gives_errors_not_panics() {
    let cases = [
        "",
        "\n",
        "0",
        "0\nSECTION",
        "0\nSECTION\n2\nENTITIES\n",
        "0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n",
        "abc\ndef\n",
        "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\nx\n20\n0\n11\n1\n21\n1\n0\nENDSEC\n",
        "0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n0\nENDSEC\n",
        "0\nSECTION\n2\nENTITIES\n0\nCIRCLE\n10\n0\n20\n0\n0\nENDSEC\n",
        "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n10\n0\n20\n0\n0\nENDSEC\n",
        "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n42\n1\n10\n0\n0\nENDSEC\n",
        "99999999999999999999\nLINE\n",
        "\u{feff}0\nSECTION\n",
        "0\nSECTION\n2\nENTITIES\n0\nARC\n10\n0\n20\n0\n40\n5\n50\n0\n0\nENDSEC\n",
    ];
    for t in cases {
        let mut s = Sketch::default();
        let r = catch_unwind(AssertUnwindSafe(|| dxf::import(&mut s, t))).unwrap_or_else(|_| panic!("{t:?} panicked"));
        assert!(r.is_err(), "{t:?} gave {r:?}");
        assert!(s.entities.is_empty(), "{t:?}: nothing is added on failure");
    }
    // binary garbage turned to text
    let junk: String = (0..2000u32).map(|i| char::from_u32((i * 7919) % 0x7f).unwrap_or(' ')).collect();
    let mut s = Sketch::default();
    assert!(catch_unwind(AssertUnwindSafe(|| dxf::import(&mut s, &junk))).is_ok());
}

#[test]
fn dxf_with_a_utf8_bom_is_read() {
    // Windows programs often write a byte order mark before the first group code
    let mut src = Sketch::default();
    src.add_rect([0.0, 0.0], [5.0, 5.0]);
    let text = format!("\u{feff}{}", dxf::export(&src));
    let mut s = Sketch::default();
    assert_eq!(dxf::import(&mut s, &text).ok(), Some(4));
}

#[test]
fn dxf_with_non_finite_numbers_is_rejected_and_never_corrupts_the_project() {
    for bad in ["nan", "NaN", "inf", "-inf", "1e999"] {
        let text = format!("0\nSECTION\n2\nENTITIES\n0\nLINE\n8\n0\n10\n{bad}\n20\n0\n11\n10\n21\n10\n0\nCIRCLE\n8\n0\n10\n0\n20\n0\n40\n{bad}\n0\nENDSEC\n0\nEOF\n");
        let mut s = Sketch::default();
        let r = dxf::import(&mut s, &text);
        let finite = s.points.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite())
            && s.entities.iter().all(|e| !matches!(e, Entity::Circle { r, .. } if !r.is_finite()));
        // the user saves and reopens the project
        let mut d = Document::default();
        d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
        let back = Document::from_json(&d.to_json());
        assert!(back.is_ok(), "{bad}: import {r:?}; saved project cannot be reopened: {:?}", back.err());
        assert!(finite, "{bad}: import {r:?} added non-finite geometry");
    }
}

#[test]
fn dxf_import_appends_to_an_existing_sketch_and_regions_merge() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [20.0, 20.0]);
    let mut other = Sketch::default();
    other.add_circle([10.0, 10.0], 3.0);
    dxf::import(&mut s, &dxf::export(&other)).unwrap();
    let r = s.regions();
    assert_eq!(r.len(), 2);
    assert_eq!(r.iter().filter(|x| x.holes.len() == 1).count(), 1);
}

// ------------------------------------------------------------------ document JSON

fn rich_document() -> Document {
    let mut d = Document::default();
    d.params.set("w", "40");
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [40.0, 30.0]);
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    s.constraints.push(Constraint::Length(l[0], Expr::from("w")));
    s.constraints.push(Constraint::Length(l[1], n(30.0)));
    s.add_circle([20.0, 15.0], 5.0);
    let (c, a) = (s.add_point([60.0, 0.0]), s.add_point([70.0, 0.0]));
    s.add(Entity::Ellipse { c, a, ratio: 0.3, construction: true });
    let first = s.points.len();
    for p in [[80.0, 0.0], [85.0, 5.0], [90.0, 0.0]] {
        s.add_point(p);
    }
    s.add(Entity::Spline { first, count: 3, closed: false, construction: false });
    s.texts.push(SketchText { text: "Ok".into(), pos: [100.0, 0.0], height: 5.0, angle: 10.0 });
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XZ, sketch: s });
    d.insert(FeatureKind::Extrude {
        sketch: sk,
        profiles: vec![ProfileRef::at([2.0, 2.0])],
        extent: Extent::OneSide(n(10.0)),
        op: Op::NewBody,
        face: None,
        taper: n(0.0),
        thin: n(0.0),
        start: n(0.0),
    });
    d.insert(FeatureKind::Fillet {
        body: 2000,
        edges: vec![EdgeRef::round([5.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]), EdgeRef { sweep: Some(90.0), ..EdgeRef::round([5.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]) }, EdgeRef::line([0.0; 3], [1.0, 2.0, 3.0])],
        radius: n(1.0),
    });
    d.body_names.insert(2000, "Плита \"A\" <1>".into());
    d.hidden_bodies.insert(7);
    d.body_materials.insert(2000, 3);
    d
}

#[test]
fn document_json_round_trip_is_lossless_and_evaluates_the_same() {
    let d = rich_document();
    let json = d.to_json();
    let back = Document::from_json(&json).unwrap();
    assert_eq!(back, d);
    assert_eq!(back.to_json(), json, "saving again gives byte-identical output");
    let (e1, e2) = (run(&d), run(&back));
    let vol = |ev: &Evaluator| ev.model().bodies.iter().map(|b| b.solid.volume()).sum::<f64>();
    assert!(vol(&e1) > 0.0);
    assert_eq!(vol(&e1), vol(&e2));
    // the extrude made a ring: (40x30 minus the circle) x 10
    assert!((vol(&e1) - (1200.0 - ngon_area(5.0, 72)) * 10.0).abs() < 1e-3, "{}", vol(&e1));
    // the open-arc sweep is written only where it exists
    assert_eq!(json.matches("\"sweep\"").count(), 1);
}

#[test]
fn document_from_json_rejects_bad_input_without_panicking() {
    for bad in ["", "{", "null", "[]", "{}", "\"genix\"", "{\"format\":\"genix\"}", "\u{0}"] {
        let r = catch_unwind(|| Document::from_json(bad)).unwrap_or_else(|_| panic!("{bad:?} panicked"));
        assert!(r.is_err(), "{bad:?}");
    }
    let d = rich_document();
    let mut v: serde_json::Value = serde_json::from_str(&d.to_json()).unwrap();
    v["format"] = "something".into();
    assert!(Document::from_json(&v.to_string()).is_err());
    v["format"] = "ostov".into();
    assert!(Document::from_json(&v.to_string()).is_ok(), "old Ostov files open");
    v["version"] = 99.into();
    assert!(Document::from_json(&v.to_string()).unwrap_err().contains("новой"));
    v["version"] = 1.into();
    v["rollback"] = 999.into();
    assert_eq!(Document::from_json(&v.to_string()).unwrap().rollback, 3);
    // truncated file
    let json = d.to_json();
    assert!(Document::from_json(&json[..json.len() / 2]).is_err());
}

#[test]
fn old_document_without_optional_fields_still_opens() {
    // a file from an early version: no texts, no materials/cutters/views, extrude without taper/thin/start/face,
    // a fillet edge without circle/axis/sweep, a hole without `more`
    let json = r#"{
      "format": "ostov", "version": 1, "params": {"items": {}}, "next_uid": 4, "rollback": 3,
      "features": [
        {"uid": 1, "name": "Чертёж 1", "kind": {"Sketch": {"plane": {"origin": [0,0,0], "x": [1,0,0], "y": [0,1,0]},
          "sketch": {"points": [{"pos": [0,0]}, {"pos": [10,0]}, {"pos": [10,10]}, {"pos": [0,10]}],
                     "entities": [{"Line": {"a": 0, "b": 1, "construction": false}}, {"Line": {"a": 1, "b": 2, "construction": false}},
                                  {"Line": {"a": 2, "b": 3, "construction": false}}, {"Line": {"a": 3, "b": 0, "construction": false}}],
                     "constraints": []}}}},
        {"uid": 2, "name": "Выдавливание 1", "kind": {"Extrude": {"sketch": 1, "profiles": [{"seed": [5,5]}], "extent": {"OneSide": "10"}, "op": "NewBody"}}},
        {"uid": 3, "name": "Скругление 1", "kind": {"Fillet": {"body": 2000, "edges": [{"a": [0,0,10], "b": [10,0,10]}], "radius": "1"}}}
      ]
    }"#;
    let d = Document::from_json(json).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.features.len(), 3);
    let ev = run(&d);
    assert!(ev.error(1).is_none() && ev.error(2).is_none(), "{:?}", errors(&ev, &d));
    let v: f64 = ev.model().bodies.iter().map(|b| b.solid.volume()).sum();
    assert!(v > 900.0 && v <= 1000.0 + 1e-6, "{v}");
    if let FeatureKind::Fillet { edges, .. } = &d.features[2].kind {
        assert!(!edges[0].circle && edges[0].sweep.is_none());
    } else {
        panic!()
    }
}

#[test]
fn old_edge_ref_without_sweep_is_a_full_circle() {
    let old = r#"{"a":[5.0,0.0,2.0],"b":[0.0,0.0,2.0],"circle":true,"axis":[0.0,0.0,1.0]}"#;
    let e: EdgeRef = serde_json::from_str(old).unwrap();
    assert!(e.circle);
    assert_eq!(e.sweep, None);
    let poly = e.polyline();
    let (f, l) = (poly[0], *poly.last().unwrap());
    assert!((f - l).length() < 1e-9, "a full circle closes on itself: {f:?} {l:?}");
    assert!(poly.iter().all(|p| ((p.x * p.x + p.y * p.y).sqrt() - 5.0).abs() < 1e-9 && (p.z - 2.0).abs() < 1e-12));
    assert!(poly.iter().any(|p| p.x < -4.9), "the far side of the circle is part of it");
    // serialising it again does not invent a sweep
    let again = serde_json::to_string(&e).unwrap();
    assert!(!again.contains("sweep"), "{again}");
    // an open arc keeps its sweep through JSON and only covers that angle
    let arc = EdgeRef { sweep: Some(90.0), ..e };
    let back: EdgeRef = serde_json::from_str(&serde_json::to_string(&arc).unwrap()).unwrap();
    assert_eq!(back, arc);
    let poly = back.polyline();
    assert!(poly.iter().all(|p| p.x >= -1e-9 && p.y >= -1e-9), "quarter arc in the first quadrant");
    let end = *poly.last().unwrap();
    assert!((end.x).abs() < 1e-9 && (end.y - 5.0).abs() < 1e-9, "{end:?}");
    // a negative sweep runs the other way
    let neg = EdgeRef { sweep: Some(-90.0), ..e };
    assert!(neg.polyline().iter().all(|p| p.y <= 1e-9));
    // a line edge from very old files (only a and b)
    let line: EdgeRef = serde_json::from_str(r#"{"a":[0,0,0],"b":[1,2,3]}"#).unwrap();
    assert!(!line.circle && line.sweep.is_none());
    assert_eq!(line.polyline().len(), 2);
    // a null sweep (written by hand) also means a full circle
    let null: EdgeRef = serde_json::from_str(r#"{"a":[5,0,0],"b":[0,0,0],"circle":true,"axis":[0,0,1],"sweep":null}"#).unwrap();
    assert_eq!(null.sweep, None);
}

#[test]
fn sketch_json_from_before_texts_still_loads() {
    let json = r#"{"points":[{"pos":[0,0]},{"pos":[3,0]}],"entities":[{"Circle":{"c":0,"r":2.5,"construction":false}}],"constraints":[{"Diameter":[0,"5"]}]}"#;
    let s: Sketch = serde_json::from_str(json).unwrap();
    assert!(s.texts.is_empty());
    assert_eq!(s.regions().len(), 1);
}

// ------------------------------------------------------------------ mesh files

fn extruded_body_mesh() -> Mesh {
    let mut d = Document::default();
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [30.0, 20.0]);
    s.add_circle([15.0, 10.0], 4.0);
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([1.0, 1.0])], extent: Extent::OneSide(n(5.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty());
    ev.model().bodies[0].mesh().clone()
}

#[test]
fn stl_binary_round_trip_keeps_volume_and_triangles() {
    let m = extruded_body_mesh();
    let mut buf = Vec::new();
    Mesh::write_stl_binary(&[&m], &mut buf);
    assert_eq!(buf.len(), 84 + 50 * m.triangles.len());
    let back = Mesh::read_stl(&buf).unwrap();
    assert_eq!(back.triangles.len(), m.triangles.len());
    assert!((back.volume() - m.volume()).abs() / m.volume() < 1e-5, "{} vs {}", back.volume(), m.volume());
    // the read mesh is closed enough to become a body again
    let mut d = Document::default();
    d.insert(FeatureKind::Import { name: "part".into(), verts: back.vertices.iter().flatten().map(|v| *v as f32).collect(), tris: back.triangles.iter().flatten().copied().collect() });
    let ev = run(&d);
    assert!(errors(&ev, &d).is_empty(), "{:?}", errors(&ev, &d));
    assert!((ev.model().bodies[0].solid.volume() - m.volume()).abs() / m.volume() < 1e-4);
    // two bodies in one file
    let mut two = Vec::new();
    Mesh::write_stl_binary(&[&m, &m], &mut two);
    assert_eq!(Mesh::read_stl(&two).unwrap().triangles.len(), 2 * m.triangles.len());
}

#[test]
fn binary_stl_whose_header_starts_with_solid_is_read_as_binary() {
    let m = extruded_body_mesh();
    let mut buf = Vec::new();
    Mesh::write_stl_binary(&[&m], &mut buf);
    buf[..11].copy_from_slice(b"solid part ");
    let back = Mesh::read_stl(&buf).unwrap();
    assert_eq!(back.triangles.len(), m.triangles.len());
}

#[test]
fn ascii_stl_is_read_and_inside_out_meshes_are_flipped() {
    let tet = |flip: bool| {
        let v = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [0.0, 10.0, 0.0], [0.0, 0.0, 10.0]];
        let mut tris = vec![[0, 2, 1], [0, 1, 3], [1, 2, 3], [0, 3, 2]];
        if flip {
            for t in &mut tris {
                t.swap(1, 2);
            }
        }
        let mut s = String::from("solid tet\r\n");
        for t in tris {
            s.push_str("  facet normal 0 0 0\r\n    outer loop\r\n");
            for i in t {
                let p: [f64; 3] = v[i];
                s.push_str(&format!("      vertex {:e} {} {}\r\n", p[0], p[1], p[2]));
            }
            s.push_str("    endloop\r\n  endfacet\r\n");
        }
        s.push_str("endsolid tet\r\n");
        s
    };
    for flip in [false, true] {
        let m = Mesh::read_stl(tet(flip).as_bytes()).unwrap();
        assert_eq!(m.vertices.len(), 4, "welded");
        assert_eq!(m.triangles.len(), 4);
        assert!((m.volume() - 1000.0 / 6.0).abs() < 1e-9, "flip {flip}: {}", m.volume());
    }
}

#[test]
fn broken_stl_files_give_errors_not_panics() {
    let mut cases: Vec<Vec<u8>> = vec![
        vec![],
        b"solid".to_vec(),
        b"solid x\nendsolid x\n".to_vec(),
        b"solid x\nfacet normal 0 0 1\nouter loop\nvertex 1 2\nendloop\nendfacet\nendsolid\n".to_vec(),
        b"solid x\nvertex 0 0 0\nvertex 1 0 0\nendsolid\n".to_vec(),
        b"solid x\nvertex nan 0 0\nvertex 1 0 0\nvertex 0 1 0\nendsolid\n".to_vec(),
        b"hello world".to_vec(),
        vec![0u8; 84],
        vec![0xffu8; 300],
    ];
    // binary with a triangle count that does not match the size
    let mut b = vec![0u8; 84 + 50];
    b[80] = 5;
    cases.push(b);
    // binary with a NaN coordinate
    let mut b = vec![0u8; 84 + 50];
    b[80] = 1;
    for (k, v) in [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0].iter().enumerate() {
        b[84 + 24 + 4 * k..84 + 28 + 4 * k].copy_from_slice(&v.to_le_bytes());
    }
    b[84 + 12..84 + 16].copy_from_slice(&f32::NAN.to_le_bytes());
    cases.push(b);
    // binary truncated in the middle of a record
    let m = extruded_body_mesh();
    let mut full = Vec::new();
    Mesh::write_stl_binary(&[&m], &mut full);
    cases.push(full[..full.len() - 7].to_vec());
    for (i, c) in cases.iter().enumerate() {
        let r = catch_unwind(|| Mesh::read_stl(c)).unwrap_or_else(|_| panic!("case {i} panicked"));
        assert!(r.is_err(), "case {i} was accepted: {:?}", r.map(|m| m.triangles.len()));
    }
}

#[test]
fn stl_with_only_degenerate_triangles_is_rejected() {
    let mut b = vec![0u8; 84 + 50 * 2];
    b[80] = 2; // two triangles with all corners at the origin
    let r = Mesh::read_stl(&b);
    assert!(r.is_err(), "accepted with {:?} triangles", r.map(|m| m.triangles.len()));
}

#[test]
fn broken_import_feature_data_is_an_error() {
    let cases: Vec<(Vec<f32>, Vec<u32>)> = vec![
        (vec![], vec![]),
        (vec![0.0, 0.0, 0.0], vec![0, 0, 0]),
        (vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], vec![0, 1, 7]),
        (vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0], vec![0, 1, 2]), // open: one triangle
        (vec![f32::NAN; 12], vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 0, 3, 2]),
    ];
    for (i, (verts, tris)) in cases.into_iter().enumerate() {
        let mut d = Document::default();
        let uid = d.insert(FeatureKind::Import { name: "x".into(), verts, tris });
        let ev = run(&d);
        assert!(ev.error(uid).is_some(), "case {i} built a body: {}", ev.model().bodies.len());
    }
}

/// Minimal zip reader: returns (name, data) of every stored entry after checking the CRC and the central directory.
fn read_stored_zip(z: &[u8]) -> Vec<(String, Vec<u8>)> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    let u16_at = |o: usize| u16::from_le_bytes([z[o], z[o + 1]]) as usize;
    let u32_at = |o: usize| u32::from_le_bytes([z[o], z[o + 1], z[o + 2], z[o + 3]]) as usize;
    let eocd = z.len() - 22;
    assert_eq!(u32_at(eocd), 0x0605_4b50, "end of central directory");
    let count = u16_at(eocd + 10);
    let (cd_size, cd_off) = (u32_at(eocd + 12), u32_at(eocd + 16));
    assert_eq!(cd_off + cd_size, eocd, "central directory sits right before the end record");
    let mut out = Vec::new();
    let mut o = cd_off;
    for _ in 0..count {
        assert_eq!(u32_at(o), 0x0201_4b50);
        let crc = u32_at(o + 16) as u32;
        let size = u32_at(o + 20);
        let name_len = u16_at(o + 28);
        let extra = u16_at(o + 30);
        let comment = u16_at(o + 32);
        let local = u32_at(o + 42);
        let name = String::from_utf8(z[o + 46..o + 46 + name_len].to_vec()).unwrap();
        assert_eq!(u32_at(local), 0x0403_4b50, "{name}: local header");
        let lname = u16_at(local + 26);
        let lextra = u16_at(local + 28);
        let data = z[local + 30 + lname + lextra..local + 30 + lname + lextra + size].to_vec();
        assert_eq!(crc32(&data), crc, "{name}: crc");
        out.push((name, data));
        o += 46 + name_len + extra + comment;
    }
    out
}

#[test]
fn three_mf_is_a_valid_zip_with_every_vertex_and_triangle() {
    let m = extruded_body_mesh();
    let z = Mesh::write_3mf(&[("Plate <A> & \"B\"", &m), ("Тело 2", &m)]);
    let files = read_stored_zip(&z);
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(names, vec!["[Content_Types].xml", "_rels/.rels", "3D/3dmodel.model"]);
    let model = String::from_utf8(files[2].1.clone()).unwrap();
    assert_eq!(model.matches("<vertex ").count(), 2 * m.vertices.len());
    assert_eq!(model.matches("<triangle ").count(), 2 * m.triangles.len());
    assert!(model.contains("name=\"Plate &lt;A&gt; &amp; &quot;B&quot;\""));
    assert!(model.contains("name=\"Тело 2\""));
    assert_eq!(model.matches("<item objectid=").count(), 2);
    // every vertex attribute is a finite number
    for line in model.lines().filter(|l| l.starts_with("<vertex")) {
        for part in line.split('"').skip(1).step_by(2) {
            assert!(part.parse::<f64>().map(|v| v.is_finite()).unwrap_or(false), "{line}");
        }
    }
    // empty export is still a valid package
    let files = read_stored_zip(&Mesh::write_3mf(&[]));
    assert_eq!(files.len(), 3);
}

#[test]
fn obj_export_indices_and_volume() {
    let m = extruded_body_mesh();
    let obj = Mesh::write_obj(&[("Plate one", &m), ("Second", &m)]);
    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut faces: Vec<[usize; 3]> = Vec::new();
    let mut objects = Vec::new();
    for line in obj.lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.first() {
            Some(&"v") => verts.push([w[1].parse().unwrap(), w[2].parse().unwrap(), w[3].parse().unwrap()]),
            Some(&"f") => faces.push([w[1].parse().unwrap(), w[2].parse().unwrap(), w[3].parse().unwrap()]),
            Some(&"o") => objects.push(w[1..].join(" ")),
            _ => {}
        }
    }
    assert_eq!(objects, vec!["Plate_one", "Second"]);
    assert_eq!(verts.len(), 2 * m.vertices.len());
    assert_eq!(faces.len(), 2 * m.triangles.len());
    assert!(faces.iter().flatten().all(|&i| i >= 1 && i <= verts.len()), "OBJ indices are 1-based and in range");
    let back = Mesh { vertices: verts, triangles: faces.iter().map(|f| [f[0] as u32 - 1, f[1] as u32 - 1, f[2] as u32 - 1]).collect() };
    assert!((back.volume() - 2.0 * m.volume()).abs() < 1e-6 * m.volume());
}

#[test]
fn step_export_references_are_consistent() {
    let m = extruded_body_mesh();
    let step = Mesh::write_step(&[("Plate 'one'", &m)], "part's file.step");
    assert!(step.starts_with("ISO-10303-21;") && step.trim_end().ends_with("END-ISO-10303-21;"));
    let mut defined = std::collections::BTreeSet::new();
    let mut max_ref = 0u64;
    for line in step.lines().filter(|l| l.starts_with('#')) {
        let (id, body) = line.split_once('=').unwrap();
        let id: u64 = id[1..].parse().unwrap();
        assert!(defined.insert(id), "#{id} defined twice");
        // every reference points to an entity defined earlier (the writer emits in order)
        let mut rest = body;
        while let Some(i) = rest.find('#') {
            let digits: String = rest[i + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
            let r: u64 = digits.parse().unwrap();
            assert!(defined.contains(&r) && r < id || r < id, "#{id} refers to #{r} before it exists");
            max_ref = max_ref.max(r);
            rest = &rest[i + 1 + digits.len()..];
        }
        assert!(line.ends_with(';'), "{line}");
    }
    assert!(defined.contains(&max_ref));
    assert_eq!(step.matches("CARTESIAN_POINT").count(), m.vertices.len() + 1);
    assert_eq!(step.matches("POLY_LOOP").count(), m.triangles.len());
    // quotes in names are encoded, so every string literal is balanced on each line
    for line in step.lines() {
        assert_eq!(line.matches('\'').count() % 2, 0, "unbalanced quote: {line}");
    }
}

#[test]
fn mesh_dxf_export_writes_one_face_per_triangle() {
    let m = extruded_body_mesh();
    let text = Mesh::write_dxf(&[("Тело", &m)]);
    assert_eq!(text.matches("3DFACE").count(), m.triangles.len());
    assert!(text.contains("\nBody_1\n"), "a name without ASCII letters becomes Body_N");
    // the sketch DXF importer does not choke on a mesh DXF: it just finds no 2D curves
    let mut s = Sketch::default();
    let r = catch_unwind(AssertUnwindSafe(|| dxf::import(&mut s, &text))).expect("no panic");
    assert!(r.is_err());
}

#[test]
fn exports_of_empty_meshes_do_not_panic() {
    let empty = Mesh::default();
    let mut buf = Vec::new();
    Mesh::write_stl_binary(&[&empty], &mut buf);
    assert_eq!(buf.len(), 84);
    assert!(Mesh::read_stl(&buf).is_err());
    let _ = Mesh::write_obj(&[("e", &empty)]);
    let _ = Mesh::write_step(&[("e", &empty)], "e.step");
    let _ = Mesh::write_dxf(&[("e", &empty)]);
    let _ = Mesh::write_3mf(&[("e", &empty)]);
}

// ------------------------------------------------------------------ chains (paths for pipes / sweeps)

#[test]
fn chain_paths_lists_open_and_closed_chains() {
    let mut s = Sketch::default();
    polyline(&mut s, &[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], false);
    s.add_rect([20.0, 0.0], [30.0, 10.0]);
    let (c, a, b) = (s.add_point([50.0, 0.0]), s.add_point([55.0, 0.0]), s.add_point([45.0, 0.0]));
    s.add(Entity::Arc { c, a, b, construction: false });
    let chains = s.chain_paths().unwrap();
    assert_eq!(chains.len(), 3, "{chains:?}");
    let closed = chains.iter().filter(|c| {
        let (f, l) = (c[0], c[c.len() - 1]);
        (f[0] - l[0]).abs() < 1e-9 && (f[1] - l[1]).abs() < 1e-9
    }).count();
    assert_eq!(closed, 1);
    assert!(chains.iter().any(|c| c.len() == 3 && c[0] == [0.0, 0.0] && c[2] == [10.0, 10.0]));
    // circles and splines alone have no path
    let mut only = Sketch::default();
    only.add_circle([0.0, 0.0], 3.0);
    assert!(only.chain_paths().is_err());
}


// ------------------------------------------------------------------ deeper: realistic combinations

#[test]
fn overlapping_circles_extrude_each_piece_by_its_seed() {
    // lens and crescents share all their boundary curves: picking must still tell them apart
    let mut s = Sketch::default();
    s.add_circle([0.0, 0.0], 10.0);
    s.add_circle([12.0, 0.0], 10.0);
    assert_eq!(s.regions().len(), 3);
    assert_extrudes(&s, 4.0, 1e-5);
    s.add_circle([6.0, 9.0], 10.0);
    assert_eq!(s.regions().len(), 7);
    assert_extrudes(&s, 4.0, 1e-5);
}

#[test]
fn two_overlapping_holes_in_a_plate() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [60.0, 40.0]);
    s.add_circle([25.0, 20.0], 8.0);
    s.add_circle([35.0, 20.0], 8.0);
    let r = s.regions();
    assert_eq!(r.len(), 4, "plate + lens + two crescents: {:?}", sorted_areas(&r));
    assert!((total_area(&r) - 2400.0).abs() < 1e-6, "regions tile the plate: {}", total_area(&r));
    let plate = r.iter().find(|x| !x.holes.is_empty()).expect("plate with a hole");
    assert_eq!(plate.holes.len(), 1, "one hole: the union outline of the two circles");
    assert_no_overlap(&r, [-1.0, -1.0], [61.0, 41.0], 60);
    assert_extrudes(&s, 5.0, 1e-5);
}

#[test]
fn a_line_through_two_rectangle_corners_gives_two_triangles() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    s.add_line([-5.0, -5.0], [15.0, 15.0]);
    let r = s.regions();
    assert_eq!(r.len(), 2, "{:?}", sorted_areas(&r));
    assert!(r.iter().all(|x| (x.area() - 50.0).abs() < 1e-9), "{:?}", sorted_areas(&r));
    // a dangling line from the middle of a side into the inside changes nothing
    let mut t = Sketch::default();
    t.add_rect([0.0, 0.0], [10.0, 10.0]);
    t.add_line([5.0, 0.0], [5.0, 5.0]);
    let r = t.regions();
    assert_eq!(r.len(), 1);
    assert!((r[0].area() - 100.0).abs() < 1e-9);
    assert_extrudes(&t, 1.0, 1e-6);
}

#[test]
fn a_figure_eight_spline_gives_two_lobes() {
    // fit points (0,0) (20,6) (0,6) (20,0): the closed spline crosses itself near (10,3)
    let mut s = Sketch::default();
    let first = s.points.len();
    for p in [[0.0, 0.0], [20.0, 6.0], [0.0, 6.0], [20.0, 0.0]] {
        s.add_point(p);
    }
    s.add(Entity::Spline { first, count: 4, closed: true, construction: false });
    let r = s.regions();
    assert_eq!(r.len(), 2, "two lobes expected: {:?}", sorted_areas(&r));
    assert!(r.iter().all(|x| x.area() > 10.0), "{:?}", sorted_areas(&r));
}

#[test]
fn trimming_two_overlapping_circles_into_one_outline() {
    let mut s = Sketch::default();
    let a = s.add_circle([0.0, 0.0], 10.0);
    let b = s.add_circle([12.0, 0.0], 10.0);
    assert!(s.trim(a, [10.0, 0.0]), "remove the part of A inside B");
    assert!(s.trim(b, [2.0, 0.0]), "remove the part of B inside A");
    let r = s.regions();
    assert_eq!(r.len(), 1, "{:?}", sorted_areas(&r));
    let lens = 2.0 * 100.0 * (0.6f64).acos() - 6.0 * (400.0f64 - 144.0).sqrt();
    let want = 2.0 * PI * 100.0 - lens;
    assert!((r[0].area() - want).abs() / want < 0.003, "{} vs {want}", r[0].area());
    assert_extrudes(&s, 3.0, 1e-6);
}

#[test]
fn profile_follows_a_parameter_change() {
    let mut d = Document::default();
    d.params.set("hole", "5");
    let mut s = Sketch::default();
    let l = s.add_rect([0.0, 0.0], [40.0, 30.0]);
    s.constraints.push(Constraint::Fix(0, [0.0, 0.0]));
    s.constraints.push(Constraint::Length(l[0], n(40.0)));
    s.constraints.push(Constraint::Length(l[1], n(30.0)));
    let c = s.add_circle([20.0, 15.0], 5.0);
    s.constraints.push(Constraint::Diameter(c, Expr::from("hole*2")));
    s.constraints.push(Constraint::Fix(4, [20.0, 15.0]));
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    let ev = run(&d);
    let plate = ev.sketch(sk).unwrap().regions.iter().find(|x| !x.holes.is_empty()).unwrap().clone();
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::of(&plate)], extent: Extent::OneSide(n(10.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
    let vol = |d: &Document| {
        let ev = run(d);
        assert!(errors(&ev, d).is_empty(), "{:?}", errors(&ev, d));
        ev.model().bodies.iter().map(|b| b.solid.volume()).sum::<f64>()
    };
    assert!((vol(&d) - (1200.0 - ngon_area(5.0, 72)) * 10.0).abs() < 1e-3);
    for h in ["8", "12", "2"] {
        d.params.set("hole", h);
        let r: f64 = h.parse().unwrap();
        assert!((vol(&d) - (1200.0 - ngon_area(r, 72)) * 10.0).abs() < 1e-3, "hole {h}: {}", vol(&d));
    }
}

#[test]
fn perforated_plate_with_900_holes_finds_regions_in_reasonable_time() {
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [310.0, 310.0]);
    for i in 0..30 {
        for j in 0..30 {
            s.add_circle([10.0 + 10.0 * i as f64, 10.0 + 10.0 * j as f64], 3.0);
        }
    }
    let t = Instant::now();
    let r = s.regions();
    let secs = t.elapsed().as_secs_f64();
    println!("900-hole plate regions: {secs:.2} s (debug build)");
    assert_eq!(r.len(), 901);
    assert_eq!(r.iter().filter(|x| x.holes.len() == 900).count(), 1);
    assert!(secs < 15.0, "regions took {secs} s");
}

#[test]
fn grid_of_crossing_lines_finds_every_cell() {
    let mut s = Sketch::default();
    let k = 21;
    for i in 0..k {
        let v = i as f64 * 5.0;
        s.add_line([v, -1.0], [v, 101.0]);
        s.add_line([-1.0, v], [101.0, v]);
    }
    let t = Instant::now();
    let r = s.regions();
    let secs = t.elapsed().as_secs_f64();
    println!("21x21 grid regions: {secs:.2} s");
    assert_eq!(r.len(), (k - 1) * (k - 1));
    assert!(r.iter().all(|x| (x.area() - 25.0).abs() < 1e-9));
    assert!(secs < 15.0, "regions took {secs} s");
}

#[test]
fn dxf_export_keeps_sketch_text() {
    let mut s = Sketch::default();
    s.texts.push(SketchText { text: "OK".into(), pos: [0.0, 0.0], height: 10.0, angle: 0.0 });
    let want = s.regions().len();
    assert_eq!(want, 2);
    let mut back = Sketch::default();
    let r = dxf::import(&mut back, &dxf::export(&s));
    assert!(r.is_ok(), "exported DXF of a text-only sketch has no curves: {r:?}");
    assert_eq!(back.regions().len() + back.texts.len() * 0, 3, "the O ring, its counter and the K as closed loops");
}

#[test]
fn corrupted_document_with_bad_indices_does_not_crash_evaluation() {
    let mut d = Document::default();
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    s.entities.push(Entity::Line { a: 0, b: 99, construction: false });
    s.constraints.push(Constraint::Length(42, n(5.0)));
    let sk = d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    d.insert(FeatureKind::Extrude { sketch: sk, profiles: vec![ProfileRef::at([5.0, 5.0])], extent: Extent::OneSide(n(10.0)), op: Op::NewBody, face: None, taper: n(0.0), thin: n(0.0), start: n(0.0) });
    let json = d.to_json();
    let loaded = Document::from_json(&json);
    if let Ok(doc) = loaded {
        let ev = catch_unwind(AssertUnwindSafe(|| run(&doc))).expect("evaluation of a corrupted file must not panic");
        assert!(ev.error(sk).is_some(), "the broken sketch step reports an error");
    }
}

#[test]
fn corrupted_document_with_bad_indices_is_rejected_on_open() {
    let mut d = Document::default();
    let mut s = Sketch::default();
    s.add_rect([0.0, 0.0], [10.0, 10.0]);
    s.entities.push(Entity::Line { a: 0, b: 99, construction: false });
    d.insert(FeatureKind::Sketch { plane: Plane::XY, sketch: s });
    let loaded = Document::from_json(&d.to_json());
    if let Ok(doc) = &loaded {
        if let FeatureKind::Sketch { sketch, .. } = &doc.features[0].kind {
            let sketch = sketch.clone();
            let r = catch_unwind(AssertUnwindSafe(|| sketch.regions()));
            assert!(r.is_ok(), "file opened, but regions() of its sketch panics");
        }
    }
    assert!(loaded.is_err(), "a sketch line pointing at point 99 of 4 was accepted");
}
