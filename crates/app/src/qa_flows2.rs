//! Second round of user flows: sketching in the app, building from sketch profiles, editing earlier steps.
//! Helpers from the first round are reused from `super::qa_flows`.
//!
//! Sketches are drawn through the sketcher's own input handling: a real egui frame runs `SketchSession::handle` with pointer
//! moves, presses and releases at the screen position of a sketch point, and typed text / keys, as the viewport does.
#![allow(dead_code)]
use super::qa_flows::{app, box_app, click_face};
use super::*;
use crate::sketcher::Tool;
use fusion_core::doc::{Extent, Shape};
use fusion_core::Op;
use std::f64::consts::PI;

// ------------------------------------------------------------------ helpers

fn cmd_uid(app: &App) -> u64 {
    match &app.mode {
        Mode::Command(c) => c.uid,
        _ => panic!("no command is running"),
    }
}

fn edit(app: &mut App, uid: u64, f: impl FnOnce(&mut FeatureKind)) {
    f(&mut app.doc.feature_mut(uid).expect("feature exists").kind);
    app.after_doc_change();
}

fn vol(app: &App, i: usize) -> f64 {
    app.ev.model().bodies[i].solid.volume()
}

fn total_vol(app: &App) -> f64 {
    app.ev.model().bodies.iter().map(|b| b.solid.volume()).sum()
}

fn nbodies(app: &App) -> usize {
    app.ev.model().bodies.len()
}

fn bounds(app: &App, i: usize) -> (DVec3, DVec3) {
    app.ev.model().bodies[i].mesh().bounds().expect("body has bounds")
}

fn all_bounds(app: &App) -> (DVec3, DVec3) {
    let mut lo = DVec3::splat(f64::MAX);
    let mut hi = DVec3::splat(f64::MIN);
    for b in &app.ev.model().bodies {
        if let Some((a, c)) = b.mesh().bounds() {
            lo = lo.min(a);
            hi = hi.max(c);
        }
    }
    (lo, hi)
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn add_box(app: &mut App, at: [f64; 3], size: [f64; 3]) -> u64 {
    let n = Expr::num;
    let uid = app.doc.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(size[0]), d: n(size[1]), h: n(size[2]) }, at: [n(at[0]), n(at[1]), n(at[2])], op: Op::NewBody });
    app.after_doc_change();
    uid
}

fn errors(app: &App) -> Vec<String> {
    app.doc.features.iter().take(app.doc.rollback).filter(|f| !f.suppressed).filter_map(|f| app.ev.error(f.uid).map(|e| format!("{} #{}: {e}", f.kind.type_label(), f.uid))).collect()
}

fn set_box(app: &mut App, uid: u64, f: impl FnOnce(&mut Expr, &mut Expr, &mut Expr)) {
    app.edit_feature(uid, 0.0);
    edit(app, uid, |k| if let FeatureKind::Primitive { shape: Shape::Box { w, d, h }, .. } = k { f(w, d, h) });
    app.finish_command(true);
}

/// Drives the sketcher with real egui input, the way the 3D view does every frame.
struct Pad {
    ctx: egui::Context,
    t: f64,
    rect: Rect,
}

impl Pad {
    fn new() -> Self {
        Pad { ctx: egui::Context::default(), t: 1.0, rect: Rect::from_min_size(Pos2::ZERO, egui::vec2(1600.0, 1200.0)) }
    }

    /// Puts the camera straight onto the sketch plane (the animation finished), looking at `center` from 120 mm,
    /// so the grid step is 1 mm and every point of the drawing is on screen.
    fn look(&mut self, app: &mut App, center: [f64; 2]) {
        app.cam.update(100.0);
        let Mode::Sketch(s) = &app.mode else { panic!("not sketching") };
        app.cam.target = s.plane.to_world(center);
        app.cam.distance = 120.0;
        self.frame(app, vec![]);
    }

    fn frame(&mut self, app: &mut App, events: Vec<egui::Event>) {
        self.t += 0.05;
        let rect = self.rect;
        let mut input = egui::RawInput { screen_rect: Some(rect), time: Some(self.t), events, focused: true, ..Default::default() };
        input.viewports.entry(egui::ViewportId::ROOT).or_default();
        let mut action = None;
        let _ = self.ctx.run(input, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                let resp = ui.allocate_rect(rect, egui::Sense::click_and_drag());
                if let Mode::Sketch(s) = &mut app.mode {
                    action = Some(s.handle(ui, &resp, &mut app.cam, rect, &app.doc.params));
                }
                // the dimension labels and the dimension editor live in the painting pass, as in the viewport
                let colors = app.theme.view();
                if let Mode::Sketch(s) = &mut app.mode {
                    s.paint(ui, &app.cam, rect, &colors, &app.doc.params);
                }
            });
        });
        match action {
            Some(crate::sketcher::SketchAction::Finish) => app.finish_sketch(true),
            Some(crate::sketcher::SketchAction::Cancel) => app.finish_sketch(false),
            _ => {}
        }
    }

    fn screen(&self, app: &App, p: [f64; 2]) -> Pos2 {
        let Mode::Sketch(s) = &app.mode else { panic!("not sketching") };
        app.cam.project(self.rect, s.plane.to_world(p)).expect("on screen")
    }

    /// A left click at a sketch point: move there, press, release.
    fn click(&mut self, app: &mut App, p: [f64; 2]) {
        let at = self.screen(app, p);
        assert!(self.rect.contains(at), "{p:?} is off screen at {at:?}");
        self.t += 0.5; // far apart from the previous click: never a double click
        self.frame(app, vec![egui::Event::PointerMoved(at)]);
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        self.frame(app, vec![button(true)]);
        self.frame(app, vec![button(false)]);
        self.frame(app, vec![]);
    }

    /// Hover only (the typed value takes the direction from the cursor).
    fn hover(&mut self, app: &mut App, p: [f64; 2]) {
        let at = self.screen(app, p);
        self.frame(app, vec![egui::Event::PointerMoved(at)]);
    }

    fn key(&mut self, app: &mut App, key: egui::Key) {
        let ev = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        self.frame(app, vec![ev(true)]);
        self.frame(app, vec![ev(false)]);
    }

    fn typed(&mut self, app: &mut App, text: &str) {
        self.frame(app, vec![egui::Event::Text(text.to_string())]);
    }

    fn tool(&mut self, app: &mut App, t: Tool) {
        let Mode::Sketch(s) = &mut app.mode else { panic!("not sketching") };
        s.set_tool(t);
    }

    fn rect(&mut self, app: &mut App, a: [f64; 2], b: [f64; 2]) {
        self.tool(app, Tool::Rect);
        self.click(app, a);
        self.click(app, b);
    }

    fn circle(&mut self, app: &mut App, c: [f64; 2], r: f64) {
        self.tool(app, Tool::Circle);
        self.click(app, c);
        self.click(app, [c[0] + r, c[1]]);
    }

    fn polyline(&mut self, app: &mut App, pts: &[[f64; 2]]) {
        self.tool(app, Tool::Line);
        for p in pts {
            self.click(app, *p);
        }
        self.key(app, egui::Key::Escape);
    }

    /// Esc until the sketch is finished (ends the tool, selects nothing, then leaves the sketch).
    fn finish(&mut self, app: &mut App) -> u64 {
        for _ in 0..4 {
            if !matches!(app.mode, Mode::Sketch(_)) {
                break;
            }
            self.key(app, egui::Key::Escape);
        }
        assert!(matches!(app.mode, Mode::Idle), "Esc leaves the sketch");
        app.doc.features.iter().take(app.doc.rollback).rev().find(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.uid).expect("a sketch step")
    }

    fn sketch(&self, app: &App) -> fusion_core::sketch::Sketch {
        let Mode::Sketch(s) = &app.mode else { panic!("not sketching") };
        s.sketch.clone()
    }
}

/// "New sketch" button, then a click on origin plane `i` (0 XY, 1 XZ, 2 YZ); the camera is put onto it.
fn sketch_on_origin(app: &mut App, pad: &mut Pad, i: usize, center: [f64; 2]) {
    app.run_tool("sketch", 0.0);
    assert!(matches!(app.mode, Mode::PickPlane));
    app.handle_click(Some(Hit::OriginPlane(i, 10.0)), false, 0.0);
    assert!(matches!(app.mode, Mode::Sketch(_)), "the sketch starts on the origin plane");
    pad.look(app, center);
}

/// "New sketch", then a click on the face of body `slot` whose normal is `dir`.
fn sketch_on_face(app: &mut App, pad: &mut Pad, slot: usize, dir: DVec3, center: [f64; 2]) {
    app.run_tool("sketch", 0.0);
    click_face(app, slot, dir, false);
    assert!(matches!(app.mode, Mode::Sketch(_)), "the sketch starts on the face");
    pad.look(app, center);
}

/// A click with a ray (as the viewport hit-tests it): from `from` along `dir`.
fn click_ray(app: &mut App, from: DVec3, dir: DVec3, shift: bool) {
    let dir = dir.normalize();
    let model = app.pick_model();
    let hit = app.hit_test(&model, from, dir);
    app.last_ray = (from, dir);
    app.handle_click(hit, shift, 0.0);
}

/// Click a profile of sketch `sk` at its local point, the ray coming from the side the plane normal points to (or the other side).
fn click_profile(app: &mut App, sk: u64, local: [f64; 2], from_back: bool) {
    let plane = app.ev.sketch(sk).expect("evaluated sketch").plane;
    let n = if from_back { -plane.normal() } else { plane.normal() };
    let p = plane.to_world(local);
    let model = app.pick_model();
    let hit = app.hit_test(&model, p + n * 200.0, -n);
    assert!(matches!(hit, Some(Hit::Profile { sketch, .. }) if sketch == sk), "the click lands on the profile, got {hit:?}");
    click_ray(app, p + n * 200.0, -n, false);
}

fn regions(app: &App, sk: u64) -> usize {
    app.ev.sketch(sk).map(|s| s.regions.len()).unwrap_or(0)
}

fn set_extent(app: &mut App, uid: u64, e: Extent) {
    edit(app, uid, |k| if let FeatureKind::Extrude { extent, .. } = k { *extent = e });
}

/// A rectangle sketched on XY by two clicks: (0,0) to (w,d).
fn rect_sketch(app: &mut App, w: f64, d: f64) -> u64 {
    let mut pad = Pad::new();
    sketch_on_origin(app, &mut pad, 0, [w / 2.0, d / 2.0]);
    pad.rect(app, [0.0, 0.0], [w, d]);
    pad.finish(app)
}

/// Sketch rectangle w×d on XY and extrude it by `h` (OK): a box made the way a user makes it.
fn sketched_block(w: f64, d: f64, h: f64) -> (App, u64, u64) {
    let mut app = app();
    let sk = rect_sketch(&mut app, w, d);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    set_extent(&mut app, ex, Extent::OneSide(Expr::num(h)));
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    (app, sk, ex)
}

/// Change the Length dimension of sketch line `line` through the sketch editor (open, change the value as the dimension box does, Esc out).
fn set_sketch_dim(app: &mut App, sk: u64, pick: impl Fn(&fusion_core::sketch::Constraint) -> bool, value: f64) {
    app.edit_feature(sk, 0.0);
    let Mode::Sketch(s) = &mut app.mode else { panic!("sketch edit") };
    let c = s.sketch.constraints.iter_mut().find(|c| pick(c)).expect("the dimension");
    *c.expr_mut().unwrap() = Expr::num(value);
    let params = app.doc.params.clone();
    s.sketch.solve(&params, &[]).expect("solves");
    app.finish_sketch(true);
}

fn length_of(line: usize) -> impl Fn(&fusion_core::sketch::Constraint) -> bool {
    move |c| matches!(c, fusion_core::sketch::Constraint::Length(l, _) if *l == line)
}

// ------------------------------------------------------------------ sketches and profiles

#[test]
fn smoke_second_round() {
    assert_eq!(box_app(5.0, 5.0, 5.0).ev.model().bodies.len(), 1);
}

/// New sketch → XY → rectangle by two clicks → Esc out; extrude picks the only profile at once; OK gives a 40x30x10 block.
#[test]
fn a_clicked_rectangle_on_xy_extrudes_into_a_block() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [20.0, 15.0]);
    pad.rect(&mut app, [0.0, 0.0], [40.0, 30.0]);
    let s = pad.sketch(&app);
    assert_eq!(s.entities.len(), 4, "four lines");
    assert_eq!(s.regions().len(), 1);
    let sk = pad.finish(&mut app);
    assert_eq!(app.undo.len(), 1, "finishing the sketch is one undo step");
    assert!(app.doc.visible_sketches.contains(&sk));
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    let FeatureKind::Extrude { sketch, profiles, .. } = app.doc.feature(ex).unwrap().kind.clone() else { panic!() };
    assert_eq!((sketch, profiles.len()), (sk, 1), "the only profile is taken at once");
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 1);
    assert!(close(vol(&app, 0), 40.0 * 30.0 * 10.0, 1e-3), "{}", vol(&app, 0));
    let (lo, hi) = bounds(&app, 0);
    assert!(close(lo.x, 0.0, 1e-6) && close(hi.x, 40.0, 1e-6) && close(hi.y, 30.0, 1e-6) && close(hi.z, 10.0, 1e-6), "{lo:?} {hi:?}");
    assert!(!app.doc.visible_sketches.contains(&sk), "the used sketch is hidden");
    app.do_undo();
    assert_eq!(nbodies(&app), 0);
    app.do_undo();
    assert!(app.doc.features.is_empty());
}

/// The rectangle's size typed while drawing: first corner, "40", Tab, "25", Enter. Both values become dimensions.
#[test]
fn a_rectangle_with_typed_width_and_height() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 10.0]);
    pad.tool(&mut app, Tool::Rect);
    pad.click(&mut app, [0.0, 0.0]);
    pad.hover(&mut app, [10.0, 10.0]);
    pad.typed(&mut app, "40");
    pad.key(&mut app, egui::Key::Tab);
    pad.typed(&mut app, "25");
    pad.key(&mut app, egui::Key::Enter);
    let s = pad.sketch(&app);
    assert_eq!(s.entities.len(), 4, "the rectangle is made by Enter");
    let dims: Vec<f64> = s.constraints.iter().filter_map(|c| match c { fusion_core::sketch::Constraint::Length(_, e) => e.eval(&app.doc.params).ok(), _ => None }).collect();
    let mut sorted = dims.clone();
    sorted.sort_by(f64::total_cmp);
    assert_eq!(sorted, vec![25.0, 40.0], "typed sizes become the dimensions");
    let r = &s.regions()[0];
    assert!(close(r.area(), 1000.0, 1e-6), "{}", r.area());
    let sk = pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    app.finish_command(true);
    assert!(close(vol(&app, 0), 10000.0, 1e-3));
    let _ = sk;
}

/// A circle with a typed diameter, extruded symmetrically 20 mm: a cylinder from z −10 to 10.
#[test]
fn a_typed_circle_extruded_symmetrically() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [0.0, 0.0]);
    pad.tool(&mut app, Tool::Circle);
    pad.click(&mut app, [0.0, 0.0]);
    pad.hover(&mut app, [5.0, 0.0]);
    pad.typed(&mut app, "16");
    pad.key(&mut app, egui::Key::Enter);
    assert_eq!(pad.sketch(&app).entities.len(), 1);
    pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    set_extent(&mut app, ex, Extent::Symmetric(Expr::num(20.0)));
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(lo.z, -10.0, 1e-6) && close(hi.z, 10.0, 1e-6), "{lo:?} {hi:?}");
    let want = PI * 64.0 * 20.0;
    assert!(close(vol(&app, 0), want, want * 0.01), "{} vs {want}", vol(&app, 0));
}

/// A negative distance on an empty document: the block grows below the sketch plane, a new body.
#[test]
fn a_negative_extrude_on_an_empty_document_goes_down() {
    let mut app = app();
    rect_sketch(&mut app, 20.0, 10.0);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    set_extent(&mut app, ex, Extent::OneSide(Expr::num(-7.0)));
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 1);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(lo.z, -7.0, 1e-6) && close(hi.z, 0.0, 1e-6), "{lo:?} {hi:?}");
}

/// Sketch on the top face of a box, a circle in the middle, extrude −5: the automatic operation cuts a pocket.
#[test]
fn a_circle_sketched_on_a_top_face_cuts_a_pocket() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let mut pad = Pad::new();
    sketch_on_face(&mut app, &mut pad, 0, DVec3::Z, [20.0, 15.0]);
    {
        let Mode::Sketch(s) = &app.mode else { panic!() };
        assert!(close(s.plane.o().z, 20.0, 1e-9), "the sketch plane is the top face: {:?}", s.plane);
    }
    // the face's plane has its own origin: draw at the world centre of the face
    let c = { let Mode::Sketch(s) = &app.mode else { panic!() }; s.plane.to_local(DVec3::new(20.0, 15.0, 20.0)) };
    pad.look(&mut app, c);
    pad.circle(&mut app, [c[0].round(), c[1].round()], 5.0);
    let sk = pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    set_extent(&mut app, ex, Extent::OneSide(Expr::num(-5.0)));
    let FeatureKind::Extrude { op, profiles, .. } = app.doc.feature(ex).unwrap().kind.clone() else { panic!() };
    assert_eq!(op, Op::Cut, "a negative distance on a body cuts");
    assert_eq!(profiles.len(), 1);
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 1);
    let want = 24000.0 - PI * 25.0 * 5.0;
    assert!(close(vol(&app, 0), want, 3.0), "{} vs {want}", vol(&app, 0));
    let _ = sk;
}

/// Extrude Join / Cut / New body from a rectangle that overlaps a box.
#[test]
fn join_cut_and_new_body_from_an_overlapping_profile() {
    for (op, want_bodies, want_vol) in [(Op::Join, 1, 24000.0 + 10.0 * 30.0 * 10.0), (Op::Cut, 1, 24000.0 - 10.0 * 30.0 * 10.0), (Op::NewBody, 2, 24000.0 + 20.0 * 30.0 * 10.0)] {
        let mut app = box_app(40.0, 30.0, 20.0);
        // rectangle x 30..50, y 0..30 on an offset plane z = 10: half inside the box, half outside
        let mut pad = Pad::new();
        sketch_on_origin(&mut app, &mut pad, 0, [40.0, 15.0]);
        pad.rect(&mut app, [30.0, 0.0], [50.0, 30.0]);
        let sk = pad.finish(&mut app);
        app.start_command("extrude", 0.0);
        let ex = cmd_uid(&app);
        // choosing the operation in the panel ends the automatic choice
        if let Mode::Command(c) = &mut app.mode { c.auto_op = false; }
        edit(&mut app, ex, |k| if let FeatureKind::Extrude { op: o, start, .. } = k { *o = op; *start = Expr::num(10.0); });
        assert_eq!(app.ev.error(ex), None, "{op:?}");
        app.finish_command(true);
        assert_eq!(nbodies(&app), want_bodies, "{op:?}");
        assert!(close(total_vol(&app), want_vol, 1.0), "{op:?}: {} vs {want_vol}", total_vol(&app));
        let _ = sk;
    }
}

/// Thin extrude: a 40x30 rectangle with 2 mm walls, 10 high: a frame.
#[test]
fn a_thin_extrude_makes_a_frame() {
    let mut app = app();
    rect_sketch(&mut app, 40.0, 30.0);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    edit(&mut app, ex, |k| if let FeatureKind::Extrude { thin, .. } = k { *thin = Expr::num(2.0) });
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    let want = (40.0 * 30.0 - 36.0 * 26.0) * 10.0;
    assert!(close(vol(&app, 0), want, 1.0), "{} vs {want}", vol(&app, 0));
}

/// Taper 10°: a 40x30 rectangle 10 mm high narrows to (40−2·10·tan10°) × (30−…) at the top.
#[test]
fn a_tapered_extrude_narrows() {
    let mut app = app();
    rect_sketch(&mut app, 40.0, 30.0);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    edit(&mut app, ex, |k| if let FeatureKind::Extrude { taper, .. } = k { *taper = Expr::num(10.0) });
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    let k = 10.0 * 10f64.to_radians().tan();
    let (a1, b1) = (40.0 - 2.0 * k, 30.0 - 2.0 * k);
    // frustum of a rectangle: h/3·(A1 + A2 + sqrt(A1·A2)) is exact only for similar shapes; prismatoid formula:
    let mid = (40.0 - k) * (30.0 - k);
    let want = 10.0 / 6.0 * (1200.0 + 4.0 * mid + a1 * b1);
    assert!(close(vol(&app, 0), want, 2.0), "{} vs {want}", vol(&app, 0));
    let (_, hi) = bounds(&app, 0);
    assert!(close(hi.z, 10.0, 1e-6));
}

/// A rectangle sketched on XY in the middle of a box that reaches from z −10 to 10: the profile starts inside the body.
/// +15 joins (adds 10x10x5 above the top), −5 cuts a closed void inside.
/// Found: the default +10 lies wholly inside the box, so `settle_auto_op` switches the step to «New body»; when the user then
/// types 15 (now sticking out) or −5, the automatic choice never comes back (`sync_auto_op` only flips Join↔Cut), and a
/// separate body is left inside the part.
#[test]
fn an_extrude_whose_profile_starts_inside_a_body() {
    for (d, want) in [(15.0, 24000.0 + 500.0), (-5.0, 24000.0 - 500.0)] {
        let mut app = app();
        add_box(&mut app, [0.0, 0.0, -10.0], [40.0, 30.0, 20.0]);
        let mut pad = Pad::new();
        sketch_on_origin(&mut app, &mut pad, 0, [20.0, 15.0]);
        pad.rect(&mut app, [15.0, 10.0], [25.0, 20.0]);
        pad.finish(&mut app);
        app.start_command("extrude", 0.0);
        let ex = cmd_uid(&app);
        set_extent(&mut app, ex, Extent::OneSide(Expr::num(d)));
        assert_eq!(app.ev.error(ex), None, "{d}");
        app.finish_command(true);
        assert_eq!(nbodies(&app), 1, "{d}");
        assert!(close(vol(&app, 0), want, 1.0), "{d}: {} vs {want}", vol(&app, 0));
    }
}

/// The profile of a sketch inside a body: a click on it (the ray comes through the body's top) must pick the profile,
/// the sketch is drawn over the body and the extrude tool is waiting for profiles.
/// Found: `hit_test` takes the nearer body face (t 190) over the profile (t 200), so the click becomes a face extrude of the top.
#[test]
fn clicking_a_profile_that_lies_inside_a_body_picks_the_profile() {
    let mut app = app();
    add_box(&mut app, [0.0, 0.0, -10.0], [40.0, 30.0, 20.0]);
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [20.0, 15.0]);
    pad.rect(&mut app, [15.0, 10.0], [25.0, 20.0]);
    pad.rect(&mut app, [30.0, 10.0], [35.0, 20.0]);
    let sk = pad.finish(&mut app);
    assert_eq!(regions(&app, sk), 2);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    let model = app.pick_model();
    let hit = app.hit_test(&model, DVec3::new(20.0, 15.0, 200.0), -DVec3::Z);
    assert!(matches!(hit, Some(Hit::Profile { .. })), "the click on the visible profile hits {hit:?}: the extrude turns into a face extrude of the top face");
    let _ = ex;
}

/// Two rectangles: clicking one profile, then the other, then the first again leaves only the second.
#[test]
fn clicking_profiles_toggles_them() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [20.0, 10.0]);
    pad.rect(&mut app, [0.0, 0.0], [10.0, 10.0]);
    pad.rect(&mut app, [20.0, 0.0], [40.0, 10.0]);
    let sk = pad.finish(&mut app);
    assert_eq!(regions(&app, sk), 2);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    assert!(app.ev.error(ex).is_some() || nbodies(&app) == 0, "nothing picked yet");
    click_profile(&mut app, sk, [5.0, 5.0], false);
    assert!(close(total_vol(&app), 1000.0, 1e-3));
    click_profile(&mut app, sk, [30.0, 5.0], false);
    assert!(close(total_vol(&app), 3000.0, 1e-3));
    click_profile(&mut app, sk, [5.0, 5.0], false);
    assert!(close(total_vol(&app), 2000.0, 1e-3), "{}", total_vol(&app));
    app.finish_command(true);
    assert_eq!(nbodies(&app), 1);
}

/// Overlapping rectangle and circle make three regions; picking the lens in the middle extrudes only it.
#[test]
fn overlapping_shapes_make_separate_profiles() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 0.0]);
    pad.rect(&mut app, [0.0, -10.0], [20.0, 10.0]);
    pad.circle(&mut app, [20.0, 0.0], 8.0);
    let sk = pad.finish(&mut app);
    assert_eq!(regions(&app, sk), 3, "rectangle without the cap, the overlap, the circle outside");
    app.start_command("extrude", 0.0);
    click_profile(&mut app, sk, [16.0, 0.0], false);
    let ex = cmd_uid(&app);
    assert_eq!(app.ev.error(ex), None);
    let want = PI * 64.0 / 2.0 * 10.0;
    assert!(close(total_vol(&app), want, want * 0.02), "{} vs {want}", total_vol(&app));
}

/// Revolve: a rectangle on XZ (x 10..20, z 0..30) around the sketch's Y axis (world Z): a tube.
#[test]
fn revolving_a_rectangle_around_the_sketch_axis_makes_a_tube() {
    for (angle, frac) in [(360.0, 1.0), (180.0, 0.5), (90.0, 0.25)] {
        let mut app = app();
        let mut pad = Pad::new();
        sketch_on_origin(&mut app, &mut pad, 1, [15.0, 15.0]);
        pad.rect(&mut app, [10.0, 0.0], [20.0, 30.0]);
        let sk = pad.finish(&mut app);
        app.start_command("revolve", 0.0);
        let rv = cmd_uid(&app);
        let FeatureKind::Revolve { sketch, profiles, .. } = app.doc.feature(rv).unwrap().kind.clone() else { panic!() };
        assert_eq!((sketch, profiles.len()), (sk, 1));
        edit(&mut app, rv, |k| if let FeatureKind::Revolve { angle: a, .. } = k { *a = Expr::num(angle) });
        assert_eq!(app.ev.error(rv), None, "{angle}");
        app.finish_command(true);
        let want = PI * (400.0 - 100.0) * 30.0 * frac;
        assert!(close(vol(&app, 0), want, want * 0.01), "{angle}: {} vs {want}", vol(&app, 0));
        let (lo, hi) = bounds(&app, 0);
        assert!(close(hi.z, 30.0, 1e-6) && close(lo.z, 0.0, 1e-6), "{angle}: {lo:?} {hi:?}");
    }
}

/// Revolve about a sketch line drawn as the axis (a construction-free line beside the profile).
#[test]
fn revolving_around_a_drawn_line() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 1, [10.0, 10.0]);
    pad.rect(&mut app, [5.0, 0.0], [10.0, 20.0]);
    pad.polyline(&mut app, &[[0.0, -5.0], [0.0, 25.0]]);
    let sk = pad.finish(&mut app);
    let line = {
        let s = &app.ev.sketch(sk).unwrap().sketch;
        (0..s.entities.len()).find(|&i| matches!(s.entities[i], fusion_core::sketch::Entity::Line { a, b, .. } if (s.p(a).x).abs() < 1e-9 && (s.p(b).x).abs() < 1e-9)).expect("the axis line")
    };
    app.start_command("revolve", 0.0);
    let rv = cmd_uid(&app);
    let FeatureKind::Revolve { profiles, .. } = app.doc.feature(rv).unwrap().kind.clone() else { panic!() };
    assert_eq!(profiles.len(), 1, "the only closed profile is taken at once");
    edit(&mut app, rv, |k| if let FeatureKind::Revolve { axis, .. } = k { *axis = fusion_core::doc::RevolveAxis::Line(line) });
    assert_eq!(app.ev.error(rv), None);
    app.finish_command(true);
    let want = PI * (100.0 - 25.0) * 20.0;
    assert!(close(vol(&app, 0), want, want * 0.01), "{} vs {want}", vol(&app, 0));
}

/// Sweep: a circle r 4 on XZ swept along a line along +Y drawn in another sketch on XY.
#[test]
fn sweeping_a_circle_along_a_line() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [0.0, 20.0]);
    pad.polyline(&mut app, &[[0.0, 0.0], [0.0, 40.0]]);
    let path = pad.finish(&mut app);
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 1, [0.0, 0.0]);
    pad.circle(&mut app, [0.0, 0.0], 4.0);
    let prof = pad.finish(&mut app);
    app.start_command("sweep", 0.0);
    let sw = cmd_uid(&app);
    let FeatureKind::Sweep { profiles, .. } = app.doc.feature(sw).unwrap().kind.clone() else { panic!() };
    if profiles.is_empty() {
        // the profile sits on XZ: the ray comes along +Y (from the −Y side the plane normal points to)
        click_profile(&mut app, prof, [0.0, 0.0], false);
    }
    edit(&mut app, sw, |k| if let FeatureKind::Sweep { path: p, .. } = k { *p = path });
    assert_eq!(app.ev.error(sw), None);
    app.finish_command(true);
    let want = PI * 16.0 * 40.0;
    assert!(close(vol(&app, 0), want, want * 0.02), "{} vs {want}", vol(&app, 0));
}

/// Loft between a 20x20 square on XY and a circle r 6 on an offset plane 30 mm up: profiles picked by clicking.
#[test]
fn lofting_a_square_into_a_circle() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [0.0, 0.0]);
    pad.rect(&mut app, [-10.0, -10.0], [10.0, 10.0]);
    let a = pad.finish(&mut app);
    // an offset plane 30 above XY, made with the plane tool
    app.start_command("plane", 0.0);
    let pl = cmd_uid(&app);
    app.handle_click(Some(Hit::OriginPlane(0, 10.0)), false, 0.0);
    edit(&mut app, pl, |k| if let FeatureKind::OffsetPlane { offset, .. } = k { *offset = Expr::num(30.0) });
    app.finish_command(true);
    app.run_tool("sketch", 0.0);
    app.handle_click(Some(Hit::UserPlane(0, 10.0)), false, 0.0);
    let mut pad = Pad::new();
    pad.look(&mut app, [0.0, 0.0]);
    pad.circle(&mut app, [0.0, 0.0], 6.0);
    let b = pad.finish(&mut app);
    app.start_command("loft", 0.0);
    let lf = cmd_uid(&app);
    click_profile(&mut app, a, [0.0, 0.0], true);
    click_profile(&mut app, b, [0.0, 0.0], false);
    let FeatureKind::Loft { profiles, .. } = app.doc.feature(lf).unwrap().kind.clone() else { panic!() };
    assert_eq!(profiles.len(), 2);
    assert_eq!(app.ev.error(lf), None);
    app.finish_command(true);
    let v = vol(&app, 0);
    // between the prism of the circle and the prism of the square
    assert!(v > PI * 36.0 * 30.0 && v < 400.0 * 30.0, "{v}");
    let (lo, hi) = bounds(&app, 0);
    assert!(close(lo.z, 0.0, 1e-6) && close(hi.z, 30.0, 1e-6));
}

/// Pipe along a polyline (L shape) drawn on XY, picked by clicking near the line.
#[test]
fn a_pipe_along_a_clicked_polyline() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [20.0, 10.0]);
    pad.polyline(&mut app, &[[0.0, 0.0], [40.0, 0.0], [40.0, 30.0]]);
    let path = pad.finish(&mut app);
    app.start_command("pipe", 0.0);
    let pp = cmd_uid(&app);
    click_ray(&mut app, DVec3::new(20.0, 0.0, 100.0), -DVec3::Z, false);
    let FeatureKind::Pipe { path: p, .. } = app.doc.feature(pp).unwrap().kind.clone() else { panic!() };
    assert_eq!(p, path);
    assert_eq!(app.ev.error(pp), None);
    app.finish_command(true);
    let area = PI * (25.0 - 3.5 * 3.5);
    let v = vol(&app, 0);
    assert!(v > area * 60.0 && v < area * 75.0, "{v} vs about {}", area * 70.0);
}

/// A slot drawn by three clicks extrudes to the stadium area.
#[test]
fn a_slot_extrudes_to_a_stadium() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 0.0]);
    pad.tool(&mut app, Tool::Slot);
    pad.click(&mut app, [0.0, 0.0]);
    pad.click(&mut app, [20.0, 0.0]);
    pad.click(&mut app, [10.0, 5.0]);
    let sk = pad.finish(&mut app);
    assert_eq!(regions(&app, sk), 1);
    app.start_command("extrude", 0.0);
    app.finish_command(true);
    let want = (20.0 * 10.0 + PI * 25.0) * 10.0;
    assert!(close(vol(&app, 0), want, want * 0.01), "{} vs {want}", vol(&app, 0));
}

/// A closed polyline of lines (a triangle) and an arc closing a D-shape.
#[test]
fn a_triangle_and_a_d_shape_from_lines_and_an_arc() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 10.0]);
    pad.tool(&mut app, Tool::Line);
    for p in [[0.0, 0.0], [20.0, 0.0], [0.0, 20.0], [0.0, 0.0]] {
        pad.click(&mut app, p);
    }
    let s = pad.sketch(&app);
    assert_eq!(s.entities.len(), 3);
    assert_eq!(s.regions().len(), 1, "clicking the first point closes the loop");
    // D: a vertical line x=30 from y 0..20 and an arc through (40,10)
    pad.tool(&mut app, Tool::Line);
    pad.click(&mut app, [30.0, 0.0]);
    pad.click(&mut app, [30.0, 20.0]);
    pad.key(&mut app, egui::Key::Escape);
    pad.tool(&mut app, Tool::Arc);
    pad.click(&mut app, [30.0, 0.0]);
    pad.click(&mut app, [30.0, 20.0]);
    pad.click(&mut app, [40.0, 10.0]);
    let sk = pad.finish(&mut app);
    assert_eq!(regions(&app, sk), 2, "triangle and D");
    app.start_command("extrude", 0.0);
    click_profile(&mut app, sk, [5.0, 5.0], false);
    click_profile(&mut app, sk, [35.0, 10.0], false);
    app.finish_command(true);
    let want = (200.0 + PI * 100.0 / 2.0) * 10.0;
    assert!(close(total_vol(&app), want, want * 0.01), "{} vs {want}", total_vol(&app));
}

/// The dimension tool: a click on a rectangle edge adds a length; editing its value later resizes the extruded block.
#[test]
fn a_dimension_added_with_the_tool_drives_the_block() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 10.0]);
    pad.tool(&mut app, Tool::Line);
    for p in [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [0.0, 10.0], [0.0, 0.0]] {
        pad.click(&mut app, p);
    }
    pad.tool(&mut app, Tool::Dimension);
    pad.click(&mut app, [10.0, 0.0]);
    // the value box opens with the measured value selected: Enter keeps it
    pad.key(&mut app, egui::Key::Enter);
    let s = pad.sketch(&app);
    let dim = s.constraints.iter().position(|c| matches!(c, fusion_core::sketch::Constraint::Length(..))).expect("a length dimension on the clicked line");
    let fusion_core::sketch::Constraint::Length(line, e) = &s.constraints[dim] else { unreachable!() };
    assert_eq!(e.eval(&app.doc.params).unwrap(), 20.0);
    let line = *line;
    let sk = pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    app.finish_command(true);
    assert!(close(vol(&app, 0), 2000.0, 1e-3));
    set_sketch_dim(&mut app, sk, length_of(line), 35.0);
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    assert!(close(vol(&app, 0), 3500.0, 1e-3), "{}", vol(&app, 0));
}


// ------------------------------------------------------------------ editing history

/// Fillet r 3 on the top edges of a block (face click).
fn fillet_top(app: &mut App, r: f64) -> u64 {
    app.start_command("fillet", 0.0);
    let f = cmd_uid(app);
    click_face(app, 0, DVec3::Z, false);
    edit(app, f, |k| if let FeatureKind::Fillet { radius, .. } = k { *radius = Expr::num(r) });
    assert_eq!(app.ev.error(f), None);
    app.finish_command(true);
    f
}

/// A through hole Ø8 at world point `at` of the face clicked with normal `dir`.
fn hole_at(app: &mut App, dir: DVec3, at: DVec3) -> u64 {
    app.start_command("hole", 0.0);
    click_face(app, 0, dir, false);
    let h = cmd_uid(app);
    edit(app, h, |k| if let FeatureKind::Hole { plane, at: a, .. } = k {
        let l = plane.to_local(at);
        a[0] = Expr::num(l[0]);
        a[1] = Expr::num(l[1]);
    });
    assert_eq!(app.ev.error(h), None);
    app.finish_command(true);
    h
}

fn press_pull(app: &mut App, slot: usize, dir: DVec3, d: f64) -> u64 {
    app.start_command("press_pull", 0.0);
    click_face(app, slot, dir, false);
    let pp = cmd_uid(app);
    edit(app, pp, |k| if let FeatureKind::PressPull { distance, .. } = k { *distance = Expr::num(d) });
    assert_eq!(app.ev.error(pp), None);
    app.finish_command(true);
    pp
}

/// Block from a sketch, then fillet, hole, press-pull of the +X side; the sketch is widened 40 → 60: every step still builds,
/// the block is 60 (+5 pulled) wide and the hole and fillet are still there.
#[test]
fn widening_the_first_sketch_keeps_fillet_hole_and_press_pull_building() {
    let (mut app, sk, ex) = sketched_block(40.0, 30.0, 20.0);
    let f = fillet_top(&mut app, 3.0);
    let h = hole_at(&mut app, DVec3::Z, DVec3::new(20.0, 15.0, 20.0));
    let pp = press_pull(&mut app, 0, DVec3::X, 5.0);
    let v_before = vol(&app, 0);
    assert!(close(bounds(&app, 0).1.x, 45.0, 1e-6));
    set_sketch_dim(&mut app, sk, length_of(0), 60.0);
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    let (lo, hi) = bounds(&app, 0);
    assert!(close(hi.x - lo.x, 65.0, 1e-3), "60 wide plus the 5 mm pull: {lo:?} {hi:?}");
    let hole = PI * 16.0 * 20.0;
    let full = 65.0 * 30.0 * 20.0;
    let v = vol(&app, 0);
    assert!(v < full - hole + 1.0 && v > full - hole - 400.0, "volume {v} (before {v_before}): hole and fillet still cut");
    let _ = (ex, f, h, pp);
}

/// Same chain, then the sketch made deeper and the extrude taller: steps follow, undo walks it back exactly.
/// Fails on the fillet for the same reason as `a_top_fillet_follows_a_block_that_is_deeper_and_taller`.
#[test]
fn editing_sketch_and_extrude_later_with_undo_redo() {
    let (mut app, sk, ex) = sketched_block(40.0, 30.0, 20.0);
    fillet_top(&mut app, 2.0);
    hole_at(&mut app, DVec3::Z, DVec3::new(20.0, 15.0, 20.0));
    let s0 = app.doc.clone();
    let v0 = vol(&app, 0);
    set_sketch_dim(&mut app, sk, length_of(1), 40.0);
    let s1 = app.doc.clone();
    app.edit_feature(ex, 0.0);
    set_extent(&mut app, ex, Extent::OneSide(Expr::num(30.0)));
    app.finish_command(true);
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    let (lo, hi) = bounds(&app, 0);
    assert!(close(hi.y - lo.y, 40.0, 1e-3) && close(hi.z, 30.0, 1e-6), "{lo:?} {hi:?}");
    let want = 40.0 * 40.0 * 30.0 - PI * 16.0 * 30.0;
    assert!(vol(&app, 0) < want && vol(&app, 0) > want - 200.0, "{} vs {want}: the hole still goes through", vol(&app, 0));
    app.do_undo();
    assert_eq!(app.doc, s1);
    app.do_undo();
    assert_eq!(app.doc, s0);
    assert!(close(vol(&app, 0), v0, 1e-6));
    app.do_redo();
    app.do_redo();
    assert!(close(bounds(&app, 0).1.z, 30.0, 1e-6));
}

/// Moving the rectangle in the sketch (dragging a whole edge with the Select tool) moves the block and the hole goes with it?
/// The hole is stored on the top face plane and a point: after the block moved 10 mm in +X, the hole stays at its world spot.
#[test]
fn dragging_the_sketch_rectangle_moves_the_block() {
    let (mut app, sk, ex) = sketched_block(40.0, 30.0, 20.0);
    app.edit_feature(sk, 0.0);
    let mut pad = Pad::new();
    pad.look(&mut app, [20.0, 15.0]);
    pad.tool(&mut app, Tool::Select);
    // drag the left edge (x = 0) to x = −10: the rectangle has fixed sizes, so it moves as a whole
    let from = pad.screen(&app, [0.0, 15.0]);
    let to = pad.screen(&app, [-10.0, 15.0]);
    pad.frame(&mut app, vec![egui::Event::PointerMoved(from)]);
    pad.frame(&mut app, vec![egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
    for k in 1..=10 {
        let p = from + (to - from) * (k as f32 / 10.0);
        pad.frame(&mut app, vec![egui::Event::PointerMoved(p)]);
    }
    pad.frame(&mut app, vec![egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
    pad.frame(&mut app, vec![]);
    pad.finish(&mut app);
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    let (lo, hi) = bounds(&app, 0);
    // a dragged entity follows the pointer without grid snapping: about −10, the size stays 40
    assert!(lo.x < -8.5 && close(hi.x - lo.x, 40.0, 1e-3), "the block moved with the rectangle: {lo:?} {hi:?}");
    assert!(close(vol(&app, 0), 40.0 * 30.0 * 20.0, 1e-2));
    let _ = ex;
}

/// Delete the extrude in the middle: the fillet after it loses its body; that is an error on the fillet, not a panic,
/// and undo brings everything back.
#[test]
fn deleting_the_middle_extrude_marks_later_steps_and_undo_restores() {
    let (mut app, _sk, ex) = sketched_block(40.0, 30.0, 20.0);
    let f = fillet_top(&mut app, 2.0);
    let before = app.doc.clone();
    let v = vol(&app, 0);
    app.delete_feature(ex, 0.0);
    assert_eq!(nbodies(&app), 0);
    assert!(app.ev.error(f).is_some(), "the fillet of a body that is gone reports it");
    app.do_undo();
    assert_eq!(app.doc, before);
    assert!(close(vol(&app, 0), v, 1e-6));
}

/// Delete the sketch an extrude uses: the toast says one step is broken, the extrude shows an error, undo restores.
#[test]
fn deleting_a_used_sketch_reports_the_broken_extrude() {
    let (mut app, sk, ex) = sketched_block(40.0, 30.0, 20.0);
    let before = app.doc.clone();
    app.delete_feature(sk, 0.0);
    let t = app.toast.clone().expect("a toast");
    assert!(t.2 && t.0.contains('1'), "{}", t.0);
    assert!(app.ev.error(ex).is_some());
    assert_eq!(nbodies(&app), 0);
    app.do_undo();
    assert_eq!(app.doc, before);
    assert_eq!(nbodies(&app), 1);
}

/// Deleting the path sketch of a sweep is reported as well.
#[test]
fn deleting_the_path_of_a_sweep_marks_it() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [0.0, 20.0]);
    pad.polyline(&mut app, &[[0.0, 0.0], [0.0, 40.0]]);
    let path = pad.finish(&mut app);
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 1, [0.0, 0.0]);
    pad.circle(&mut app, [0.0, 0.0], 4.0);
    let prof = pad.finish(&mut app);
    app.start_command("sweep", 0.0);
    let sw = cmd_uid(&app);
    let FeatureKind::Sweep { profiles, .. } = app.doc.feature(sw).unwrap().kind.clone() else { panic!() };
    if profiles.is_empty() { click_profile(&mut app, prof, [0.0, 0.0], false); }
    edit(&mut app, sw, |k| if let FeatureKind::Sweep { path: p, .. } = k { *p = path });
    app.finish_command(true);
    assert_eq!(app.ev.error(sw), None);
    app.delete_feature(path, 0.0);
    assert!(app.toast.as_ref().is_some_and(|t| t.2), "a warning toast");
    assert!(app.ev.error(sw).is_some(), "the sweep without its path reports it");
}

/// Suppress the fillet in the middle of block → fillet → hole: the hole still builds, volume = block − hole.
#[test]
fn suppressing_a_middle_step_keeps_the_later_ones() {
    let (mut app, _sk, _ex) = sketched_block(40.0, 30.0, 20.0);
    let f = fillet_top(&mut app, 3.0);
    let h = hole_at(&mut app, DVec3::Z, DVec3::new(20.0, 15.0, 20.0));
    app.checkpoint();
    app.doc.feature_mut(f).unwrap().suppressed = true;
    app.after_doc_change();
    assert_eq!(app.ev.error(h), None);
    let want = 24000.0 - PI * 16.0 * 20.0;
    assert!(close(vol(&app, 0), want, want * 0.003), "{} vs {want}", vol(&app, 0));
    app.do_undo();
    assert!(vol(&app, 0) < want - 10.0, "the fillet is back");
}

/// Roll the timeline back to after the extrude, sketch a circle on the top face and cut a pocket, roll forward:
/// the later fillet (whose top edges did not change) and hole still build.
#[test]
fn a_sketch_and_cut_added_with_the_timeline_rolled_back() {
    let (mut app, _sk, ex) = sketched_block(40.0, 30.0, 20.0);
    let f = fillet_top(&mut app, 2.0);
    let h = hole_at(&mut app, DVec3::X, DVec3::new(40.0, 15.0, 10.0));
    app.doc.rollback = app.doc.index_of(ex).unwrap() + 1;
    app.after_doc_change();
    let mut pad = Pad::new();
    sketch_on_face(&mut app, &mut pad, 0, DVec3::Z, [0.0, 0.0]);
    let c = { let Mode::Sketch(s) = &app.mode else { panic!() }; s.plane.to_local(DVec3::new(10.0, 15.0, 20.0)) };
    pad.look(&mut app, c);
    pad.circle(&mut app, [c[0].round(), c[1].round()], 4.0);
    let sk2 = pad.finish(&mut app);
    assert_eq!(app.doc.index_of(sk2), Some(app.doc.index_of(ex).unwrap() + 1), "the sketch goes in after the rolled-back step");
    app.start_command("extrude", 0.0);
    let cut = cmd_uid(&app);
    set_extent(&mut app, cut, Extent::OneSide(Expr::num(-6.0)));
    app.finish_command(true);
    assert_eq!(app.doc.index_of(cut), Some(app.doc.index_of(sk2).unwrap() + 1));
    app.doc.rollback = app.doc.features.len();
    app.after_doc_change();
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    let v = vol(&app, 0);
    let want = 24000.0 - PI * 16.0 * 6.0 - PI * 16.0 * 40.0;
    assert!(v < want && v > want - 150.0, "{v} vs {want} (minus the fillet)");
    let _ = (f, h);
}

/// Undo while sketching uses the sketch's own history; leaving the sketch then undoing removes the whole sketch at once.
#[test]
fn undo_inside_a_sketch_and_after_it() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 10.0]);
    pad.rect(&mut app, [0.0, 0.0], [20.0, 20.0]);
    pad.circle(&mut app, [30.0, 10.0], 3.0);
    {
        let Mode::Sketch(s) = &mut app.mode else { panic!() };
        let params = app.doc.params.clone();
        assert!(s.can_undo());
        s.undo(&params);
        assert_eq!(s.sketch.entities.len(), 4, "undo removes the circle");
        s.redo(&params);
        assert_eq!(s.sketch.entities.len(), 5);
    }
    pad.finish(&mut app);
    assert_eq!(app.doc.features.len(), 1);
    app.do_undo();
    assert!(app.doc.features.is_empty());
    app.do_redo();
    assert_eq!(app.doc.features.len(), 1);
}

/// Editing a sketch and leaving it unchanged must not add an empty undo step (commands already skip it, see
/// `finish_command`; `finish_sketch` always calls `checkpoint`).
#[test]
fn opening_a_sketch_and_leaving_unchanged_adds_no_undo_step() {
    let (mut app, sk, _ex) = sketched_block(40.0, 30.0, 20.0);
    let n = app.undo.len();
    let before = app.doc.clone();
    app.edit_feature(sk, 0.0);
    let mut pad = Pad::new();
    pad.look(&mut app, [20.0, 15.0]);
    pad.finish(&mut app);
    assert_eq!(app.doc, before, "the document itself is unchanged");
    assert_eq!(app.undo.len(), n, "nothing changed in the sketch, yet an undo step was added");
}

/// A sketch edited so that its profile breaks (one rectangle line deleted): the extrude reports an error, the app does not panic;
/// undo brings the block back.
#[test]
fn breaking_the_profile_of_a_used_sketch_reports_and_undo_heals() {
    let (mut app, sk, ex) = sketched_block(40.0, 30.0, 20.0);
    app.edit_feature(sk, 0.0);
    let mut pad = Pad::new();
    pad.look(&mut app, [20.0, 15.0]);
    pad.tool(&mut app, Tool::Select);
    pad.click(&mut app, [20.0, 30.0]);
    pad.key(&mut app, egui::Key::Delete);
    assert_eq!(pad.sketch(&app).entities.len(), 3, "the top line is deleted");
    pad.finish(&mut app);
    assert!(app.ev.error(ex).is_some(), "an open outline cannot be extruded");
    app.do_undo();
    assert_eq!(app.ev.error(ex), None);
    assert_eq!(nbodies(&app), 1);
}

/// Splitting the rectangle in two with a line: the extrude used the whole rectangle; it should still extrude the
/// original area (both halves) or report, not silently extrude half.
/// Found: `ProfileRef::resolve` falls back to the region under the stored seed point: one half, no message.
#[test]
#[ignore = "BUG: splitting a used profile with a line silently extrudes only the half under the seed point"]
fn splitting_a_used_profile_with_a_line_keeps_the_whole_area_or_reports() {
    let (mut app, sk, ex) = sketched_block(40.0, 30.0, 20.0);
    app.edit_feature(sk, 0.0);
    let mut pad = Pad::new();
    pad.look(&mut app, [20.0, 15.0]);
    pad.polyline(&mut app, &[[20.0, 0.0], [20.0, 30.0]]);
    pad.finish(&mut app);
    assert_eq!(regions(&app, sk), 2);
    let v = total_vol(&app);
    assert!(app.ev.error(ex).is_some() || close(v, 24000.0, 1.0), "the extrude silently changed to {v} mm³ after the profile was split");
}

// ------------------------------------------------------------------ steps that follow moved faces and edges

/// Total area of the triangles of body `slot` with outward normal `n` lying on the plane n·p = `level`.
fn face_area(app: &App, slot: usize, n: DVec3, level: f64) -> f64 {
    let model = app.ev.model();
    let mesh = model.bodies[slot].mesh();
    mesh.triangles.iter().map(|t| {
        let [a, b, c] = mesh.tri(t);
        let cr = (b - a).cross(c - a);
        if cr.normalize_or_zero().dot(n) > 0.9999 && (a.dot(n) - level).abs() < 1e-4 { cr.length() / 2.0 } else { 0.0 }
    }).sum()
}

/// Click the face of body `slot` with normal `dir` exactly at world point `p` (the triangle under it).
fn click_face_at(app: &mut App, slot: usize, dir: DVec3, p: DVec3) {
    let model = app.pick_model();
    let mesh = model.bodies[slot].mesh();
    let tri = mesh.triangles.iter().position(|t| {
        let [a, b, c] = mesh.tri(t);
        let n = (b - a).cross(c - a).normalize_or_zero();
        n.dot(dir) > 0.999 && (p - a).dot(n).abs() < 1e-6 && [(a, b), (b, c), (c, a)].iter().all(|(x, y)| (*y - *x).cross(p - *x).dot(n) >= -1e-9)
    }).expect("a triangle of that face under the point");
    app.last_ray = (p + dir * 50.0, -dir);
    app.handle_click(Some(Hit::Body { slot, tri, t: 50.0 }), false, 0.0);
}

fn click_line_edge(app: &mut App, slot: usize, a: [f64; 3], b: [f64; 3]) {
    let model = app.pick_model();
    let body = model.bodies[slot].clone();
    let edges = app.straight_edges(&body);
    let same = |e: &EdgeRef| !e.circle && ((DVec3::from(e.a).distance(a.into()) < 1e-6 && DVec3::from(e.b).distance(b.into()) < 1e-6) || (DVec3::from(e.a).distance(b.into()) < 1e-6 && DVec3::from(e.b).distance(a.into()) < 1e-6));
    let e = *edges.iter().find(|e| same(e)).unwrap_or_else(|| panic!("edge {a:?}-{b:?} among {:?}", edges.iter().map(|e| (e.a, e.b)).collect::<Vec<_>>()));
    app.hover_edge = Some((e, body.id));
    let mid = (DVec3::from(e.a) + DVec3::from(e.b)) / 2.0;
    app.last_ray = (mid + DVec3::ONE * 50.0, -DVec3::ONE.normalize());
    app.handle_click(Some(Hit::Body { slot, tri: 0, t: 50.0 * 3f64.sqrt() }), false, 0.0);
    app.hover_edge = None;
}

/// Fillet r on the one straight edge a–b of body 0.
fn fillet_edge(app: &mut App, a: [f64; 3], b: [f64; 3], r: f64) -> u64 {
    app.start_command("fillet", 0.0);
    let f = cmd_uid(app);
    click_line_edge(app, 0, a, b);
    edit(app, f, |k| if let FeatureKind::Fillet { radius, .. } = k { *radius = Expr::num(r) });
    assert_eq!(app.ev.error(f), None);
    app.finish_command(true);
    f
}

/// Fillet of the top front edge (y 0, z 20); the box then made 45 tall. The edge is now on the front face, 25 below the new
/// top edge and 20 above the bottom edge: the replacement must still be the top edge, not the nearer bottom one.
/// Found: `followed_edges` takes the nearest parallel sharp line by distance, so the bottom front edge gets rounded instead.
#[test]
fn a_top_edge_fillet_stays_on_top_when_the_box_grows_much_taller() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    let f = fillet_edge(&mut app, [0.0, 0.0, 20.0], [40.0, 0.0, 20.0], 3.0);
    assert!(close(face_area(&app, 0, DVec3::Z, 20.0), 40.0 * 27.0, 1e-3));
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(45.0));
    assert_eq!(app.ev.error(f), None);
    let (top, bottom) = (face_area(&app, 0, DVec3::Z, 45.0), face_area(&app, 0, -DVec3::Z, 0.0));
    assert!(close(top, 40.0 * 27.0, 1e-3) && close(bottom, 1200.0, 1e-3), "top face {top} (1080 if rounded), bottom face {bottom} (1200 if sharp)");
}

/// The same with a moderate change (30 tall): the top edge is the nearer one, so it is followed correctly.
#[test]
fn a_top_edge_fillet_follows_a_moderately_taller_box() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    let f = fillet_edge(&mut app, [0.0, 0.0, 20.0], [40.0, 0.0, 20.0], 3.0);
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(30.0));
    assert_eq!(app.ev.error(f), None);
    assert!(close(face_area(&app, 0, DVec3::Z, 30.0), 1080.0, 1e-3) && close(face_area(&app, 0, -DVec3::Z, 0.0), 1200.0, 1e-3));
}

/// The box made lower (20 → 12) under a fillet of the top edges: the top edges moved down along with the top face.
/// Found: the stored edges at z 20 are now in the air, `followed_edges` refuses to follow them and the fillet is an error.
#[test]
fn a_top_fillet_follows_a_lower_box() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    let f = fillet_top(&mut app, 2.0);
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(12.0));
    assert_eq!(app.ev.error(f), None, "the top fillet of a lower box");
    assert!(vol(&app, 0) < 40.0 * 30.0 * 12.0 - 50.0);
}

/// Deeper and taller at once (30 → 40 deep, 20 → 30 tall): the back top edge (y 30, z 20) is now inside the body.
/// Found: that edge is neither on a sharp edge nor on the surface, so the fillet of the top face fails as a whole.
#[test]
fn a_top_fillet_follows_a_block_that_is_deeper_and_taller() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    let f = fillet_top(&mut app, 2.0);
    set_box(&mut app, bx, |_, d, h| { *d = Expr::num(40.0); *h = Expr::num(30.0); });
    assert_eq!(app.ev.error(f), None, "{:?}", app.ev.error(f));
    let full = 40.0 * 40.0 * 30.0;
    assert!(vol(&app, 0) < full - 50.0 && vol(&app, 0) > full - 300.0, "{}", vol(&app, 0));
}

/// A press-pull of the box's +X face added after a fillet of the top edge along Y at x 40, then the press-pull is moved
/// in history before the fillet with a large distance (45): the stored edge lies on the top face, 45 from the new right edge
/// and 40 from the left edge. The right edge has to be rounded.
#[test]
fn an_earlier_press_pull_moves_the_filleted_edge_far_away() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let f = fillet_edge(&mut app, [40.0, 0.0, 20.0], [40.0, 30.0, 20.0], 3.0);
    app.doc.rollback = 1;
    app.after_doc_change();
    let pp = press_pull(&mut app, 0, DVec3::X, 45.0);
    app.doc.rollback = app.doc.features.len();
    app.after_doc_change();
    assert_eq!(app.ev.error(pp), None);
    assert_eq!(app.ev.error(f), None);
    // the right face (x 85) is cut short by the fillet at the top, the left face (x 0) stays whole
    let (right, left) = (face_area(&app, 0, DVec3::X, 85.0), face_area(&app, 0, -DVec3::X, 0.0));
    assert!(close(right, 30.0 * 17.0, 1e-3) && close(left, 600.0, 1e-3), "right face {right} (510 if rounded), left face {left} (600 if sharp)");
}

/// A press-pull of box A's top, with a second body B floating above A. A is then made much lower (20 → 5): the face to follow
/// is A's top at z 5, not B's top at z 30 (which happens to be nearer the old plane z 20).
/// Found: `geom::follow_face` searches the faces of every body and takes the one nearest along the normal: B's top is pulled.
#[test]
fn a_press_pull_follows_its_own_body_not_a_neighbour_above() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    add_box(&mut app, [0.0, 0.0, 25.0], [40.0, 30.0, 5.0]);
    app.start_command("press_pull", 0.0);
    click_face_at(&mut app, 0, DVec3::Z, DVec3::new(10.0, 10.0, 20.0));
    let pp = cmd_uid(&app);
    edit(&mut app, pp, |k| if let FeatureKind::PressPull { distance, .. } = k { *distance = Expr::num(3.0) });
    app.finish_command(true);
    assert!(close(bounds(&app, 0).1.z, 23.0, 1e-6));
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(5.0));
    assert_eq!(app.ev.error(pp), None);
    let (a, b) = (bounds(&app, 0), bounds(&app, 1));
    assert!(close(a.1.z, 8.0, 1e-6) && close(b.1.z, 30.0, 1e-6), "A top {} (want 8), B top {} (want 30)", a.1.z, b.1.z);
}

/// The same for a through hole drilled from A's top: after A is made lower the hole must not follow onto B and drill it.
#[test]
fn a_hole_follows_its_own_body_not_a_neighbour_above() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    add_box(&mut app, [0.0, 0.0, 25.0], [40.0, 30.0, 5.0]);
    let h = hole_at(&mut app, DVec3::Z, DVec3::new(20.0, 15.0, 20.0));
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(5.0));
    assert_eq!(app.ev.error(h), None);
    assert!(close(vol(&app, 1), 40.0 * 30.0 * 5.0, 1e-3), "B is untouched: {}", vol(&app, 1));
    let want = 40.0 * 30.0 * 5.0 - PI * 16.0 * 5.0;
    assert!(close(vol(&app, 0), want, want * 0.005), "A is drilled through: {} vs {want}", vol(&app, 0));
}

/// A stepped body: a 40x30x10 slab with a 20x30x10 block joined on its left half (high top z 20 over x 0..20, low top z 10
/// over x 20..40). The high top is pulled up 5 at x 15. Then the joined block is made narrower (20 → 10): the high face did
/// not move along its normal, it only got smaller, so it is still the one to pull.
/// Found: the picked point (x 15) now lies over the low face; `follow_face` takes the low face and pulls it instead.
#[test]
fn a_press_pull_of_a_step_face_that_got_narrower() {
    let mut app = box_app(40.0, 30.0, 10.0);
    let n = Expr::num;
    let step = app.doc.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(20.0), d: n(30.0), h: n(10.0) }, at: [n(0.0), n(0.0), n(10.0)], op: Op::Join });
    app.after_doc_change();
    assert_eq!(nbodies(&app), 1);
    app.start_command("press_pull", 0.0);
    click_face_at(&mut app, 0, DVec3::Z, DVec3::new(15.0, 15.0, 20.0));
    let pp = cmd_uid(&app);
    edit(&mut app, pp, |k| if let FeatureKind::PressPull { distance, .. } = k { *distance = Expr::num(5.0) });
    app.finish_command(true);
    assert!(close(vol(&app, 0), 12000.0 + 6000.0 + 3000.0, 1e-3));
    set_box(&mut app, step, |w, _, _| *w = Expr::num(10.0));
    assert_eq!(app.ev.error(pp), None);
    // wanted: slab 12000 + block 3000 + pull 10x30x5 = 1500 → 16500, top at 25; the low face pulled would give 12000+3000+4500
    let v = vol(&app, 0);
    assert!(close(bounds(&app, 0).1.z, 25.0, 1e-6) && close(v, 16500.0, 1e-3), "top {} volume {v}", bounds(&app, 0).1.z);
}

/// A blind hole (depth 8) on the top face; the box made taller: the hole follows the top and keeps its depth.
#[test]
fn a_blind_hole_follows_the_top_of_a_taller_box() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    let h = hole_at(&mut app, DVec3::Z, DVec3::new(20.0, 15.0, 20.0));
    edit(&mut app, h, |k| if let FeatureKind::Hole { depth, .. } = k { *depth = Some(Expr::num(8.0)) });
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(30.0));
    assert_eq!(app.ev.error(h), None);
    let want = 36000.0 - PI * 16.0 * 8.0;
    assert!(close(vol(&app, 0), want, 3.0), "{} vs {want}", vol(&app, 0));
    // the hole opens on the new top: the top face has a hole in it
    assert!(close(face_area(&app, 0, DVec3::Z, 30.0), 1200.0 - PI * 16.0, 2.0), "{}", face_area(&app, 0, DVec3::Z, 30.0));
}

/// Face extrude (the Extrude tool on a box face) of the +X side; the box then made wider: the face extrude follows.
#[test]
fn a_face_extrude_follows_a_wider_box() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    app.start_command("extrude", 0.0);
    click_face(&mut app, 0, DVec3::X, false);
    let ex = cmd_uid(&app);
    set_extent(&mut app, ex, Extent::OneSide(Expr::num(5.0)));
    app.finish_command(true);
    set_box(&mut app, bx, |w, _, _| *w = Expr::num(60.0));
    assert_eq!(app.ev.error(ex), None);
    assert!(close(bounds(&app, 0).1.x, 65.0, 1e-6), "{:?}", bounds(&app, 0));
    assert!(close(vol(&app, 0), 65.0 * 600.0, 1e-3));
}

/// A boss and a pocket sketched on the top face; the box then made taller (20 → 30). The sketch lies on the face the user
/// clicked; when that face moves the boss should stay on top and the pocket should stay open.
/// Found: the sketch keeps its old plane z 20: the boss is buried in the taller box (top stays 30) and the pocket is gone from the
/// top (volume back to the full 36000).
#[test]
fn a_sketch_on_a_top_face_follows_the_face_when_the_box_gets_taller() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    let mut pad = Pad::new();
    sketch_on_face(&mut app, &mut pad, 0, DVec3::Z, [0.0, 0.0]);
    let (c1, c2) = { let Mode::Sketch(s) = &app.mode else { panic!() }; (s.plane.to_local(DVec3::new(10.0, 15.0, 20.0)), s.plane.to_local(DVec3::new(30.0, 15.0, 20.0))) };
    pad.look(&mut app, [(c1[0] + c2[0]) / 2.0, (c1[1] + c2[1]) / 2.0]);
    pad.circle(&mut app, [c1[0].round(), c1[1].round()], 4.0);
    pad.circle(&mut app, [c2[0].round(), c2[1].round()], 4.0);
    let sk = pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    let boss = cmd_uid(&app);
    click_profile(&mut app, sk, [c1[0].round(), c1[1].round()], false);
    set_extent(&mut app, boss, Extent::OneSide(Expr::num(6.0)));
    app.finish_command(true);
    app.start_command("extrude", 0.0);
    let pocket = cmd_uid(&app);
    click_profile(&mut app, sk, [c2[0].round(), c2[1].round()], false);
    set_extent(&mut app, pocket, Extent::OneSide(Expr::num(-6.0)));
    app.finish_command(true);
    let a = PI * 16.0 * 6.0;
    assert!(close(vol(&app, 0), 24000.0, 2.0), "boss and pocket of equal size: {}", vol(&app, 0));
    assert!(close(bounds(&app, 0).1.z, 26.0, 1e-6));
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(30.0));
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    let top = bounds(&app, 0).1.z;
    let open = face_area(&app, 0, DVec3::Z, 30.0);
    assert!(close(top, 36.0, 1e-6) && open < 1200.0 - PI * 16.0 * 2.0 + 5.0, "boss top at {top} (want 36), top face area {open} (want two circles less), volume {} (a = {a})", vol(&app, 0));
}

/// A shell open at the top; the box then made taller (20 → 30). The open face moved along its normal: the shell either
/// follows it or reports; it must not silently leave a solid box.
#[test]
fn a_shell_follows_its_open_face_when_the_box_gets_taller() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    app.start_command("shell", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let sh = cmd_uid(&app);
    app.finish_command(true);
    assert!(vol(&app, 0) < 24000.0 * 0.5);
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(30.0));
    let v = vol(&app, 0);
    assert!(app.ev.error(sh).is_some() || v < 36000.0 * 0.5, "the shell silently left a solid box: {v} mm³, no error");
}

/// Chamfer of the top front edge, box made 45 tall: as with the fillet the chamfer has to stay on the top edge.
#[test]
fn a_top_edge_chamfer_stays_on_top_when_the_box_grows_much_taller() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let bx = app.doc.features[0].uid;
    app.start_command("chamfer", 0.0);
    let c = cmd_uid(&app);
    click_line_edge(&mut app, 0, [0.0, 0.0, 20.0], [40.0, 0.0, 20.0]);
    edit(&mut app, c, |k| if let FeatureKind::Chamfer { dist, .. } = k { *dist = Expr::num(3.0) });
    app.finish_command(true);
    set_box(&mut app, bx, |_, _, h| *h = Expr::num(45.0));
    assert_eq!(app.ev.error(c), None);
    let (top, bottom) = (face_area(&app, 0, DVec3::Z, 45.0), face_area(&app, 0, -DVec3::Z, 0.0));
    assert!(close(top, 1080.0, 1e-3) && close(bottom, 1200.0, 1e-3), "top face {top} (1080 if cut), bottom face {bottom} (1200 if sharp)");
}

/// Press-pull the top of a sketched block near a corner (35, 25); the sketch then made narrower (40 → 20), so the picked point
/// is outside the block. The top face still exists on its plane: it should still be pulled, or the step should report.
#[test]
fn a_press_pull_whose_picked_point_falls_off_a_narrower_block() {
    let (mut app, sk, _ex) = sketched_block(40.0, 30.0, 20.0);
    app.start_command("press_pull", 0.0);
    click_face_at(&mut app, 0, DVec3::Z, DVec3::new(35.0, 25.0, 20.0));
    let pp = cmd_uid(&app);
    edit(&mut app, pp, |k| if let FeatureKind::PressPull { distance, .. } = k { *distance = Expr::num(5.0) });
    app.finish_command(true);
    set_sketch_dim(&mut app, sk, length_of(0), 20.0);
    let (lo, hi) = bounds(&app, 0);
    let silently_ignored = app.ev.error(pp).is_none() && close(hi.z, 20.0, 1e-6);
    assert!(!silently_ignored, "the press-pull does nothing and says nothing: {lo:?} {hi:?}");
    if app.ev.error(pp).is_none() {
        assert!(close(hi.z, 25.0, 1e-6) && close(vol(&app, 0), 20.0 * 30.0 * 25.0, 1e-3), "{lo:?} {hi:?} {}", vol(&app, 0));
    }
}

/// A hole near the right end of the top face; the sketch then made narrower so the hole centre is off the block.
/// The hole should report (or at least not cut something unexpected); nothing must panic.
#[test]
fn a_hole_whose_centre_falls_off_a_narrower_block() {
    let (mut app, sk, _ex) = sketched_block(40.0, 30.0, 20.0);
    let h = hole_at(&mut app, DVec3::Z, DVec3::new(35.0, 15.0, 20.0));
    set_sketch_dim(&mut app, sk, length_of(0), 20.0);
    let v = vol(&app, 0);
    assert!(app.ev.error(h).is_some() || close(v, 20.0 * 30.0 * 20.0, 1e-3), "{v}");
    assert!(app.ev.error(h).is_some(), "a hole that drills nothing gives no message (volume {v})");
}

/// Extents: TwoSides (5 forward, 3 back), All through the model as a cut, ToBody up to a second body.
#[test]
fn two_sided_through_all_and_up_to_body_extents() {
    // two sides
    let mut app = app();
    rect_sketch(&mut app, 10.0, 10.0);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    set_extent(&mut app, ex, Extent::TwoSides(Expr::num(5.0), Expr::num(3.0)));
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(lo.z, -3.0, 1e-6) && close(hi.z, 5.0, 1e-6), "{lo:?} {hi:?}");

    // through all as a cut: a circle on the top face of a box cuts all the way
    let mut app = box_app(40.0, 30.0, 20.0);
    let mut pad = Pad::new();
    sketch_on_face(&mut app, &mut pad, 0, DVec3::Z, [0.0, 0.0]);
    let c = { let Mode::Sketch(s) = &app.mode else { panic!() }; s.plane.to_local(DVec3::new(20.0, 15.0, 20.0)) };
    pad.look(&mut app, c);
    pad.circle(&mut app, [c[0].round(), c[1].round()], 5.0);
    pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    if let Mode::Command(cm) = &mut app.mode { cm.auto_op = false; }
    // "through all" one way goes along the plane normal, away from the body; the two-way choice reaches into it
    edit(&mut app, ex, |k| if let FeatureKind::Extrude { extent, op, .. } = k { *extent = Extent::All(true); *op = Op::Cut; });
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    let want = 24000.0 - PI * 25.0 * 20.0;
    assert!(close(vol(&app, 0), want, want * 0.005), "through all: {} vs {want}", vol(&app, 0));

    // up to a body: a square on XY up to a box floating at z 15..20
    let mut app = super::qa_flows::app();
    let target = add_box(&mut app, [0.0, 0.0, 15.0], [20.0, 20.0, 5.0]);
    let _ = target;
    let tid = app.ev.model().bodies[0].id;
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 10.0]);
    pad.rect(&mut app, [5.0, 5.0], [15.0, 15.0]);
    pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    if let Mode::Command(cm) = &mut app.mode { cm.auto_op = false; }
    edit(&mut app, ex, |k| if let FeatureKind::Extrude { extent, op, .. } = k { *extent = Extent::ToBody(tid); *op = Op::NewBody; });
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 2);
    let (lo, hi) = bounds(&app, 1);
    assert!(close(lo.z, 0.0, 1e-6) && close(hi.z, 15.0, 1e-6), "up to the body's near side: {lo:?} {hi:?}");
}

/// Revolve with Cut through a box: a groove ring.
#[test]
fn a_revolve_cut_makes_a_groove() {
    let mut app = app();
    app.doc.insert(FeatureKind::Primitive { shape: Shape::Cylinder { r: Expr::num(10.0), h: Expr::num(40.0) }, at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)], op: Op::NewBody });
    app.after_doc_change();
    let v0 = vol(&app, 0);
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 1, [10.0, 20.0]);
    pad.rect(&mut app, [8.0, 18.0], [12.0, 22.0]);
    pad.finish(&mut app);
    app.start_command("revolve", 0.0);
    let rv = cmd_uid(&app);
    edit(&mut app, rv, |k| if let FeatureKind::Revolve { op, .. } = k { *op = Op::Cut });
    assert_eq!(app.ev.error(rv), None);
    app.finish_command(true);
    let groove = PI * (100.0 - 64.0) * 4.0;
    assert!(close(vol(&app, 0), v0 - groove, groove * 0.05), "{} vs {}", vol(&app, 0), v0 - groove);
}

/// Revolving a profile that crosses its own axis is an error, not a panic or a broken solid.
#[test]
fn revolving_a_profile_across_its_axis_is_refused() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 1, [0.0, 10.0]);
    pad.rect(&mut app, [-5.0, 0.0], [5.0, 20.0]);
    pad.finish(&mut app);
    app.start_command("revolve", 0.0);
    let rv = cmd_uid(&app);
    let err = app.ev.error(rv).map(str::to_string);
    if err.is_none() {
        let v = vol(&app, 0);
        let want = PI * 25.0 * 20.0;
        assert!(close(v, want, want * 0.02), "a profile across the axis built {v} (a cylinder would be {want})");
    } else {
        assert!(!err.unwrap().contains("внутренняя ошибка"));
    }
}

/// Press-pull of the top face of a sketched block, then the extrude made taller (20 → 30): the press-pull follows (top 35).
/// Then the extrude made negative (−20, the block goes down): the top is now z 0, it is followed there.
#[test]
fn a_press_pull_follows_the_top_of_an_extrude_edited_up_and_down() {
    let (mut app, _sk, ex) = sketched_block(40.0, 30.0, 20.0);
    let pp = press_pull(&mut app, 0, DVec3::Z, 5.0);
    app.edit_feature(ex, 0.0);
    set_extent(&mut app, ex, Extent::OneSide(Expr::num(30.0)));
    app.finish_command(true);
    assert_eq!(app.ev.error(pp), None);
    assert!(close(bounds(&app, 0).1.z, 35.0, 1e-6), "{:?}", bounds(&app, 0));
    app.edit_feature(ex, 0.0);
    set_extent(&mut app, ex, Extent::OneSide(Expr::num(-20.0)));
    app.finish_command(true);
    assert_eq!(app.ev.error(pp), None);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(lo.z, -20.0, 1e-6) && close(hi.z, 5.0, 1e-6), "{lo:?} {hi:?}");
}

/// Undo and redo through a whole sketch-driven session, comparing the document at each point.
#[test]
fn undo_redo_through_a_sketch_driven_session() {
    let mut app = app();
    let mut states = vec![app.doc.clone()];
    let sk = rect_sketch(&mut app, 40.0, 30.0);
    states.push(app.doc.clone());
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    set_extent(&mut app, ex, Extent::OneSide(Expr::num(20.0)));
    app.finish_command(true);
    states.push(app.doc.clone());
    fillet_top(&mut app, 2.0);
    states.push(app.doc.clone());
    set_sketch_dim(&mut app, sk, length_of(0), 50.0);
    states.push(app.doc.clone());
    hole_at(&mut app, DVec3::Z, DVec3::new(25.0, 15.0, 20.0));
    states.push(app.doc.clone());
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    let v_end = vol(&app, 0);
    for want in states.iter().rev().skip(1) {
        app.do_undo();
        assert_eq!(&app.doc, want);
        assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    }
    for want in states.iter().skip(1) {
        app.do_redo();
        assert_eq!(&app.doc, want);
    }
    assert!(close(vol(&app, 0), v_end, 1e-6));
}

/// Deleting a step with the timeline rolled back to before it: the rollback marker stays at the same place.
#[test]
fn deleting_a_step_after_the_rollback_marker() {
    let (mut app, _sk, ex) = sketched_block(40.0, 30.0, 20.0);
    let f = fillet_top(&mut app, 2.0);
    let h = hole_at(&mut app, DVec3::Z, DVec3::new(20.0, 15.0, 20.0));
    app.doc.rollback = app.doc.index_of(f).unwrap();
    app.after_doc_change();
    assert!(close(vol(&app, 0), 24000.0, 1e-3));
    app.delete_feature(h, 0.0);
    assert_eq!(app.doc.rollback, app.doc.index_of(f).unwrap(), "the marker still sits before the fillet");
    assert!(close(vol(&app, 0), 24000.0, 1e-3));
    app.doc.rollback = app.doc.features.len();
    app.after_doc_change();
    assert!(errors(&app).is_empty());
    let _ = ex;
}

/// Editing a sketch that is behind the rollback marker (the step is greyed out) and finishing it must not panic and
/// the change must land when rolled forward.
#[test]
fn editing_a_sketch_while_rolled_back_to_before_it() {
    let (mut app, sk, ex) = sketched_block(40.0, 30.0, 20.0);
    app.doc.rollback = 0;
    app.after_doc_change();
    assert_eq!(nbodies(&app), 0);
    set_sketch_dim(&mut app, sk, length_of(0), 50.0);
    app.doc.rollback = app.doc.features.len();
    app.after_doc_change();
    assert_eq!(app.ev.error(ex), None);
    assert!(close(vol(&app, 0), 50.0 * 30.0 * 20.0, 1e-3), "{}", vol(&app, 0));
}

/// A sketched block 40 wide with its front right vertical edge (x 40, y 0) rounded; the sketch's first corner is fixed at the
/// origin (Fix constraint) and the width changed 40 → 90. The old edge lies on the front face, 50 from the new right edge and
/// 40 from the left one.
/// Found: the left vertical edge is rounded instead (nearest parallel line by distance).
/// (Without the Fix the solver grows the rectangle on both sides, 25 each, and the right edge is found.)
#[test]
fn a_vertical_edge_fillet_follows_a_much_wider_sketch() {
    let (mut app, sk, _ex) = sketched_block(40.0, 30.0, 20.0);
    let f = fillet_edge(&mut app, [40.0, 0.0, 0.0], [40.0, 0.0, 20.0], 4.0);
    app.edit_feature(sk, 0.0);
    {
        let Mode::Sketch(s) = &mut app.mode else { panic!() };
        let corner = s.sketch.points.iter().position(|p| close(p.pos[0], 0.0, 1e-9) && close(p.pos[1], 0.0, 1e-9)).unwrap();
        s.sketch.constraints.push(fusion_core::sketch::Constraint::Fix(corner, [0.0, 0.0]));
        *s.sketch.constraints.iter_mut().find(|c| length_of(0)(c)).unwrap().expr_mut().unwrap() = Expr::num(90.0);
        let params = app.doc.params.clone();
        s.sketch.solve(&params, &[]).unwrap();
    }
    app.finish_sketch(true);
    assert_eq!(app.ev.error(f), None);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(lo.x, 0.0, 1e-6) && close(hi.x, 90.0, 1e-6), "the rectangle grew to the right: {lo:?} {hi:?}");
    // the left face (x 0) stays whole, the right face (x 90) is 4 mm narrower
    let (left, right) = (face_area(&app, 0, -DVec3::X, 0.0), face_area(&app, 0, DVec3::X, 90.0));
    assert!(close(left, 600.0, 1e-3) && close(right, 26.0 * 20.0, 1e-3), "left face {left} (600 if sharp), right face {right} (520 if rounded)");
}

/// The top of a sketched block is pulled up 5 (picked in the middle, x 20). Then, with the timeline rolled back, a groove is cut
/// across the top (x 15..25, 5 deep) from a sketch on the top face. The top face is now split in two and the picked point lies
/// over the groove floor. The press-pull should keep lifting the top (both pieces) or report; it must not fill the groove.
/// Found: `follow_face` takes the groove floor (the nearest face with the same normal under the point) and pulls it up 5,
/// so the groove the user just added disappears again, with no message.
#[test]
fn a_press_pull_of_a_top_face_split_by_an_earlier_groove() {
    let (mut app, _sk, ex) = sketched_block(40.0, 30.0, 20.0);
    app.start_command("press_pull", 0.0);
    click_face_at(&mut app, 0, DVec3::Z, DVec3::new(20.0, 15.0, 20.0));
    let pp = cmd_uid(&app);
    edit(&mut app, pp, |k| if let FeatureKind::PressPull { distance, .. } = k { *distance = Expr::num(5.0) });
    app.finish_command(true);
    app.doc.rollback = app.doc.index_of(ex).unwrap() + 1;
    app.after_doc_change();
    let mut pad = Pad::new();
    sketch_on_face(&mut app, &mut pad, 0, DVec3::Z, [0.0, 0.0]);
    let (a, b) = { let Mode::Sketch(s) = &app.mode else { panic!() }; (s.plane.to_local(DVec3::new(15.0, 0.0, 20.0)), s.plane.to_local(DVec3::new(25.0, 30.0, 20.0))) };
    pad.look(&mut app, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]);
    pad.rect(&mut app, [a[0].round(), a[1].round()], [b[0].round(), b[1].round()]);
    pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    let cut = cmd_uid(&app);
    set_extent(&mut app, cut, Extent::OneSide(Expr::num(-5.0)));
    assert_eq!(app.ev.error(cut), None);
    app.finish_command(true);
    assert!(close(vol(&app, 0), 24000.0 - 10.0 * 30.0 * 5.0, 1e-3));
    app.doc.rollback = app.doc.features.len();
    app.after_doc_change();
    let v = vol(&app, 0);
    let filled = close(v, 24000.0, 1e-3);
    assert!(!filled || app.ev.error(pp).is_some(), "the press-pull filled the groove: volume {v} = the uncut block, no message");
}

/// Suppressing the sketch an extrude uses: the extrude reports (or keeps building from the suppressed sketch); no panic.
#[test]
fn suppressing_a_used_sketch() {
    let (mut app, sk, ex) = sketched_block(40.0, 30.0, 20.0);
    app.checkpoint();
    app.doc.feature_mut(sk).unwrap().suppressed = true;
    app.after_doc_change();
    let built = nbodies(&app) == 1 && close(vol(&app, 0), 24000.0, 1e-3);
    assert!(built || app.ev.error(ex).is_some(), "the extrude neither builds nor reports");
    app.do_undo();
    assert_eq!(app.ev.error(ex), None);
}

/// Moving the sketch's rectangle in the sketch editor (dims unchanged): a press-pull of the block's +X face follows the face.
#[test]
fn a_press_pull_follows_a_side_face_when_the_sketch_rectangle_is_widened_to_the_left() {
    let (mut app, sk, _ex) = sketched_block(40.0, 30.0, 20.0);
    let pp = press_pull(&mut app, 0, -DVec3::X, 5.0);
    assert!(close(bounds(&app, 0).0.x, -5.0, 1e-6));
    // fix the right side, widen: the left face moves to x −20
    app.edit_feature(sk, 0.0);
    {
        let Mode::Sketch(s) = &mut app.mode else { panic!() };
        let right = s.sketch.points.iter().position(|p| close(p.pos[0], 40.0, 1e-9) && close(p.pos[1], 0.0, 1e-9)).unwrap();
        let pos = s.sketch.points[right].pos;
        s.sketch.constraints.push(fusion_core::sketch::Constraint::Fix(right, pos));
        *s.sketch.constraints.iter_mut().find(|c| length_of(0)(c)).unwrap().expr_mut().unwrap() = Expr::num(60.0);
        let params = app.doc.params.clone();
        s.sketch.solve(&params, &[]).unwrap();
    }
    app.finish_sketch(true);
    assert_eq!(app.ev.error(pp), None);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(lo.x, -25.0, 1e-6) && close(hi.x, 40.0, 1e-6), "{lo:?} {hi:?}");
}

// ------------------------------------------------------------------ sketch tools by clicks

fn region_areas(app: &App) -> Vec<f64> {
    let Mode::Sketch(s) = &app.mode else { panic!() };
    let mut v: Vec<f64> = s.sketch.regions().iter().map(|r| (r.area() * 1000.0).round() / 1000.0).collect();
    v.sort_by(f64::total_cmp);
    v
}

/// A hexagon (default 6 sides) by centre and vertex: regular, area 3√3/2·r².
#[test]
fn a_polygon_by_two_clicks_is_regular() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [0.0, 0.0]);
    pad.tool(&mut app, Tool::Polygon);
    pad.click(&mut app, [0.0, 0.0]);
    pad.click(&mut app, [10.0, 0.0]);
    let a = region_areas(&app);
    assert_eq!(a.len(), 1);
    assert!(close(a[0], 1.5 * 3f64.sqrt() * 100.0, 0.05), "{a:?}");
    pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    app.finish_command(true);
    assert!(close(vol(&app, 0), 1.5 * 3f64.sqrt() * 100.0 * 10.0, 0.5));
}

/// A rectangle whose first corner is clicked on an existing rectangle's corner shares that point (snaps), so the two touch.
#[test]
fn a_second_rectangle_snapped_to_a_corner_shares_the_point() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 10.0]);
    pad.rect(&mut app, [0.0, 0.0], [10.0, 10.0]);
    let n = pad.sketch(&app).points.len();
    pad.rect(&mut app, [10.0, 10.0], [20.0, 20.0]);
    let s = pad.sketch(&app);
    assert_eq!(s.points.len(), n + 3, "the shared corner is not duplicated");
    assert_eq!(region_areas(&app), vec![100.0, 100.0]);
}

/// Trim: a rectangle with a line across; trimming the line's ends outside the rectangle keeps two regions.
#[test]
fn trimming_the_overhanging_ends_of_a_crossing_line() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 5.0]);
    pad.rect(&mut app, [0.0, 0.0], [20.0, 10.0]);
    pad.polyline(&mut app, &[[8.0, -5.0], [8.0, 15.0]]);
    assert_eq!(region_areas(&app), vec![80.0, 120.0]);
    pad.tool(&mut app, Tool::Trim);
    pad.click(&mut app, [8.0, -3.0]);
    pad.click(&mut app, [8.0, 13.0]);
    let s = pad.sketch(&app);
    assert_eq!(region_areas(&app), vec![80.0, 120.0], "the regions survive trimming the ends");
    // no part of any line reaches outside the rectangle any more
    for i in 0..s.entities.len() {
        for p in s.tessellate(i) {
            assert!(p[1] > -1e-6 && p[1] < 10.0 + 1e-6, "a trimmed end is left at {p:?}");
        }
    }
}

/// Offset a circle outwards by 5 (default distance): two concentric circles, a ring and a disc.
#[test]
fn offsetting_a_circle_makes_a_ring() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [0.0, 0.0]);
    pad.circle(&mut app, [0.0, 0.0], 10.0);
    pad.tool(&mut app, Tool::Offset);
    // just outside the circle (5 px), so the offset goes outwards
    pad.click(&mut app, [0.0, 10.3]);
    let a = region_areas(&app);
    assert_eq!(a.len(), 2, "{a:?}");
    assert!(close(a[0], PI * 100.0, 1.0) && close(a[1], PI * (225.0 - 100.0), 1.0), "disc and ring: {a:?}");
}

/// Corner rounding of a rectangle corner (radius 5 by default): the area loses 25·(1 − π/4). Then the width is changed 20 → 30.
/// Found: `Sketch::fillet_corner` (sketch_ops.rs, the `constraints.retain` on the two shortened lines) drops their Length
/// dimensions, so the rectangle's height dimension is gone, and the arc gets no radius; after the width change the profile
/// distorts (height 20 → 21.5, corner radius 5 → 8, volume 6306 instead of 5946).
#[test]
fn rounding_a_rectangle_corner() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [10.0, 10.0]);
    pad.rect(&mut app, [0.0, 0.0], [20.0, 20.0]);
    pad.tool(&mut app, Tool::Corner);
    pad.click(&mut app, [20.0, 20.0]);
    let a = region_areas(&app);
    assert_eq!(a.len(), 1);
    assert!(close(a[0], 400.0 - 25.0 * (1.0 - PI / 4.0), 0.1), "{a:?}");
    {
        // the rectangle's height dimension (on the right line, which the rounding shortened) must survive
        let Mode::Sketch(s) = &app.mode else { panic!() };
        let lengths = s.sketch.constraints.iter().filter(|c| matches!(c, fusion_core::sketch::Constraint::Length(..))).count();
        assert_eq!(lengths, 2, "width and height dimensions after rounding a corner: {:?}", s.sketch.constraints);
    }
    let sk = pad.finish(&mut app);
    // the rounded profile extrudes, and the sketch still solves after a width change
    app.start_command("extrude", 0.0);
    app.finish_command(true);
    set_sketch_dim(&mut app, sk, length_of(0), 30.0);
    assert!(errors(&app).is_empty(), "{:?}", errors(&app));
    let want = (600.0 - 25.0 * (1.0 - PI / 4.0)) * 10.0;
    assert!(close(vol(&app, 0), want, want * 0.005), "{} vs {want}", vol(&app, 0));
}

/// Mirror: select a circle, then click a vertical line as the axis; the copy lands on the other side.
#[test]
fn mirroring_a_circle_across_a_line() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [0.0, 0.0]);
    pad.polyline(&mut app, &[[0.0, -15.0], [0.0, 15.0]]);
    pad.circle(&mut app, [10.0, 0.0], 3.0);
    pad.tool(&mut app, Tool::Select);
    pad.click(&mut app, [13.0, 0.0]);
    pad.tool(&mut app, Tool::Mirror);
    pad.click(&mut app, [0.0, 7.0]);
    let a = region_areas(&app);
    assert_eq!(a.len(), 2, "{a:?}");
    let s = pad.sketch(&app);
    let centres: Vec<f64> = (0..s.entities.len()).filter_map(|i| match s.entities[i] { fusion_core::sketch::Entity::Circle { c, .. } => Some(s.p(c).x), _ => None }).collect();
    assert!(centres.iter().any(|x| close(*x, -10.0, 1e-6)), "{centres:?}");
}

/// Esc in the middle of a line chain drops the dangling start point; the drawing has no stray points.
#[test]
fn escaping_a_line_chain_leaves_no_stray_point() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [0.0, 0.0]);
    pad.tool(&mut app, Tool::Line);
    pad.click(&mut app, [0.0, 0.0]);
    pad.click(&mut app, [10.0, 0.0]);
    pad.key(&mut app, egui::Key::Escape);
    let s = pad.sketch(&app);
    assert_eq!(s.entities.len(), 1);
    assert_eq!(s.points.len(), 2, "only the two ends of the line: {:?}", s.points.iter().map(|p| p.pos).collect::<Vec<_>>());
}

/// A typed line length (10 then Enter) in the direction of the cursor; a chain of typed lengths closes into a square.
#[test]
fn a_square_from_typed_line_lengths() {
    let mut app = app();
    let mut pad = Pad::new();
    sketch_on_origin(&mut app, &mut pad, 0, [5.0, 5.0]);
    pad.tool(&mut app, Tool::Line);
    pad.click(&mut app, [0.0, 0.0]);
    for dir in [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0]] {
        let Mode::Sketch(s) = &app.mode else { panic!() };
        let last = s.sketch.points.last().unwrap().pos;
        pad.hover(&mut app, [last[0] + dir[0] * 3.0, last[1] + dir[1] * 3.0]);
        pad.typed(&mut app, "10");
        pad.key(&mut app, egui::Key::Enter);
    }
    pad.click(&mut app, [0.0, 0.0]);
    assert_eq!(region_areas(&app), vec![100.0]);
}

/// Sketching on a face of the second body, then extruding: only that body grows (Join).
#[test]
fn a_sketch_on_the_second_body_joins_to_it() {
    let mut app = box_app(20.0, 20.0, 20.0);
    add_box(&mut app, [50.0, 0.0, 0.0], [20.0, 20.0, 10.0]);
    let mut pad = Pad::new();
    sketch_on_face(&mut app, &mut pad, 1, DVec3::Z, [0.0, 0.0]);
    let c = { let Mode::Sketch(s) = &app.mode else { panic!() }; s.plane.to_local(DVec3::new(60.0, 10.0, 10.0)) };
    pad.look(&mut app, c);
    pad.rect(&mut app, [c[0].round() - 5.0, c[1].round() - 5.0], [c[0].round() + 5.0, c[1].round() + 5.0]);
    pad.finish(&mut app);
    app.start_command("extrude", 0.0);
    let ex = cmd_uid(&app);
    assert_eq!(app.ev.error(ex), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 2);
    assert!(close(vol(&app, 0), 8000.0, 1e-3) && close(vol(&app, 1), 4000.0 + 1000.0, 1e-3), "{} {}", vol(&app, 0), vol(&app, 1));
}

/// Starting a new sketch while a command is half-done drops the command; starting a command while sketching keeps the sketch.
#[test]
fn switching_between_sketch_and_commands() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let plain = app.doc.clone();
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    app.run_tool("sketch", 0.0);
    assert!(matches!(app.mode, Mode::PickPlane));
    assert_eq!(app.doc, plain, "the unfinished press-pull is dropped");
    app.handle_click(Some(Hit::OriginPlane(0, 10.0)), false, 0.0);
    let mut pad = Pad::new();
    pad.look(&mut app, [60.0, 15.0]);
    pad.rect(&mut app, [50.0, 0.0], [70.0, 30.0]);
    // a tool button pressed while sketching: the sketch is kept (finished) and the tool starts
    app.run_tool("extrude", 0.0);
    assert!(matches!(app.mode, Mode::Command(_)));
    assert!(app.doc.features.iter().any(|f| matches!(f.kind, FeatureKind::Sketch { .. })), "the sketch was kept");
    let ex = cmd_uid(&app);
    assert_eq!(app.ev.error(ex), None, "its only profile is taken");
    app.finish_command(true);
    assert_eq!(nbodies(&app), 2);
}
