//! Click-driven user flows through the app (commands, picks, history), as a user would do them.
//! Each test drives `App` the way the viewport does: `start_command`, `handle_click` with a `Hit`, field edits, `finish_command`.
use super::*;

pub(super) fn app() -> App {
    App::from_settings(Settings::default(), Theme::Light)
}

/// A new document with one box body.
pub(super) fn box_app(w: f64, d: f64, h: f64) -> App {
    use fusion_core::doc::Shape;
    let mut app = app();
    let n = Expr::num;
    app.doc.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(w), d: n(d), h: n(h) }, at: [n(0.0), n(0.0), n(0.0)], op: fusion_core::Op::NewBody });
    app.after_doc_change();
    app
}

/// Click the face of body `slot` (in the model the click is tested against) whose outward normal is `dir`.
pub(super) fn click_face(app: &mut App, slot: usize, dir: DVec3, shift: bool) {
    let model = app.pick_model();
    let mesh = model.bodies[slot].mesh();
    let tri = mesh.triangles.iter().position(|t| {
        let [a, b, c] = mesh.tri(t);
        (b - a).cross(c - a).normalize_or_zero().dot(dir) > 0.999
    }).expect("a face with that normal");
    let [a, b, c] = mesh.tri(&mesh.triangles[tri]);
    let at = (a + b + c) / 3.0;
    app.last_ray = (at + dir * 50.0, -dir);
    app.handle_click(Some(Hit::Body { slot, tri, t: 50.0 }), shift, 0.0);
}

#[test]
fn smoke_a_box_has_one_body() {
    let app = box_app(10.0, 10.0, 10.0);
    assert_eq!(app.ev.model().bodies.len(), 1);
}

// ------------------------------------------------------------------ helpers

fn cmd_uid(app: &App) -> u64 {
    match &app.mode {
        Mode::Command(c) => c.uid,
        _ => panic!("no command is running"),
    }
}

fn cmd_pick(app: &App) -> PickTarget {
    match &app.mode {
        Mode::Command(c) => c.pick,
        _ => panic!("no command is running"),
    }
}

/// Change the feature as the properties panel would, then rebuild.
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

/// Add a box body at `at` with the given size (a project step, as if the box tool was used and OK pressed).
fn add_box(app: &mut App, at: [f64; 3], size: [f64; 3]) {
    use fusion_core::doc::Shape;
    let n = Expr::num;
    app.doc.insert(FeatureKind::Primitive { shape: Shape::Box { w: n(size[0]), d: n(size[1]), h: n(size[2]) }, at: [n(at[0]), n(at[1]), n(at[2])], op: fusion_core::Op::NewBody });
    app.after_doc_change();
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn set_distance(app: &mut App, uid: u64, d: f64) {
    edit(app, uid, |k| match k {
        FeatureKind::PressPull { distance, .. } => *distance = Expr::num(d),
        FeatureKind::Extrude { extent: fusion_core::doc::Extent::OneSide(e), .. } => *e = Expr::num(d),
        other => panic!("no distance on {other:?}"),
    });
}

/// Click a straight edge of body `slot` (the cursor is near the edge, so `hover_edge` is set, as in the viewport).
fn click_edge(app: &mut App, slot: usize, pick: impl Fn(&EdgeRef) -> bool, shift: bool) {
    let model = app.pick_model();
    let body = model.bodies[slot].clone();
    let edges = app.straight_edges(&body);
    let e = *edges.iter().find(|e| pick(e)).expect("an edge that matches");
    app.hover_edge = Some((e, body.id));
    let mid = (DVec3::from(e.a) + DVec3::from(e.b)) / 2.0;
    // a hit on a triangle touching the edge
    let mesh = body.mesh();
    let tri = mesh.triangles.iter().position(|t| mesh.tri(t).iter().any(|p| p.distance(mid) < 1e3)).unwrap_or(0);
    app.last_ray = (mid + DVec3::new(1.0, 1.0, 1.0) * 50.0, -DVec3::new(1.0, 1.0, 1.0).normalize());
    app.handle_click(Some(Hit::Body { slot, tri, t: 50.0 * 3f64.sqrt() }), shift, 0.0);
    app.hover_edge = None;
}

// ------------------------------------------------------------------ OK and Cancel for every tool

/// Each modify/create tool on a box: a realistic click, then Cancel puts the document back exactly as it was.
#[test]
fn cancel_after_a_click_leaves_the_document_exactly_as_before() {
    for tool in ["press_pull", "hole", "fillet", "chamfer", "shell", "move", "scale", "mirror", "pattern", "split", "draft", "remove", "extrude", "place", "plane", "angle_plane", "combine", "thread_on"] {
        let mut app = box_app(40.0, 30.0, 20.0);
        let before = app.doc.clone();
        let v0 = vol(&app, 0);
        app.start_command(tool, 0.0);
        assert!(matches!(app.mode, Mode::Command(_)), "{tool}: the command starts");
        click_face(&mut app, 0, DVec3::Z, false);
        app.cancel_mode();
        assert!(matches!(app.mode, Mode::Idle), "{tool}");
        assert_eq!(app.doc, before, "{tool}: cancel restores the document");
        assert_eq!(nbodies(&app), 1, "{tool}");
        assert!(close(vol(&app, 0), v0, 1e-6), "{tool}: model restored");
        assert!(app.undo.is_empty(), "{tool}: a cancelled command leaves no history step");
    }
}

/// The same with OK: each tool leaves one undo step, and undo returns to the plain box.
#[test]
fn ok_after_a_click_adds_one_undo_step_and_undo_restores_the_box() {
    for tool in ["press_pull", "hole", "fillet", "chamfer", "shell", "move", "scale", "mirror", "pattern", "draft", "extrude"] {
        let mut app = box_app(40.0, 30.0, 20.0);
        let before = app.doc.clone();
        let v0 = vol(&app, 0);
        app.start_command(tool, 0.0);
        click_face(&mut app, 0, DVec3::Z, false);
        let uid = cmd_uid(&app);
        assert_eq!(app.ev.error(uid), None, "{tool}: builds after the click");
        app.finish_command(true);
        assert!(matches!(app.mode, Mode::Idle));
        assert_eq!(app.undo.len(), 1, "{tool}");
        let after = app.doc.clone();
        let changed = (total_vol(&app) - v0).abs() > 1e-6 || nbodies(&app) != 1 || tool == "move";
        assert!(changed, "{tool}: OK changed the model (volume {} vs {v0})", total_vol(&app));
        app.do_undo();
        assert_eq!(app.doc, before, "{tool}: undo");
        assert!(close(total_vol(&app), v0, 1e-6), "{tool}");
        app.do_redo();
        assert_eq!(app.doc, after, "{tool}: redo");
    }
}

/// A command started on an empty document either refuses with a message or starts cleanly; nothing panics, cancel leaves nothing.
#[test]
fn every_tool_on_an_empty_document_refuses_or_starts_cleanly() {
    let items: Vec<&str> = [crate::toolbar::CREATE, crate::toolbar::MODIFY, crate::toolbar::REFERENCE, crate::toolbar::INSPECT].into_iter().flatten().map(|i| i.id).collect();
    let mut red = Vec::new();
    for id in items {
        let mut app = app();
        app.run_tool(id, 0.0);
        if let Mode::Command(c) = &app.mode {
            let uid = c.uid;
            let kind = app.doc.feature(uid).unwrap().kind.clone();
            if !commands::awaiting_pick(&kind) {
                if let Some(e) = app.ev.error(uid) { red.push(format!("{id}: {e}")); }
            }
            // a click into empty space does nothing harmful
            app.handle_click(None, false, 0.0);
            app.cancel_mode();
        } else {
            app.handle_click(None, false, 0.0);
            app.cancel_mode();
        }
        assert!(app.doc.features.is_empty(), "{id}: nothing left behind");
    }
    assert!(red.is_empty(), "tools that open with a red error on an empty document instead of a hint: {red:?}");
}

/// Primitive tools on an empty document: OK gives exactly the default solid.
#[test]
fn primitives_on_an_empty_document_build_the_default_solids() {
    use std::f64::consts::PI;
    for (tool, want) in [("box", 8000.0), ("cylinder", PI * 100.0 * 20.0), ("sphere", 4.0 / 3.0 * PI * 1000.0), ("torus", 2.0 * PI * PI * 15.0 * 16.0)] {
        let mut app = app();
        app.start_command(tool, 0.0);
        let uid = cmd_uid(&app);
        assert_eq!(app.ev.error(uid), None, "{tool}");
        app.finish_command(true);
        assert_eq!(nbodies(&app), 1, "{tool}");
        let v = vol(&app, 0);
        assert!((v - want).abs() < want * 0.03, "{tool}: volume {v} vs {want}");
    }
}

// ------------------------------------------------------------------ history and later edits

/// Press-pull 5 → OK; later edit it to 12 → OK; the model follows; undo goes back to 5, then to the box.
#[test]
fn editing_a_press_pull_later_changes_the_model_and_undo_steps_back() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    app.finish_command(true);
    assert!(close(bounds(&app, 0).1.z, 25.0, 1e-6));
    app.edit_feature(uid, 0.0);
    assert_eq!(cmd_uid(&app), uid);
    set_distance(&mut app, uid, 12.0);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert!(close(bounds(&app, 0).1.z, 32.0, 1e-6), "{:?}", bounds(&app, 0));
    assert!(close(vol(&app, 0), 40.0 * 30.0 * 32.0, 1e-3));
    app.do_undo();
    assert!(close(bounds(&app, 0).1.z, 25.0, 1e-6), "undo returns to 5 mm: {:?}", bounds(&app, 0));
    app.do_undo();
    assert!(close(bounds(&app, 0).1.z, 20.0, 1e-6));
    app.do_redo();
    app.do_redo();
    assert!(close(bounds(&app, 0).1.z, 32.0, 1e-6));
}

/// Editing a feature and cancelling restores its old value.
#[test]
fn cancelling_an_edit_keeps_the_old_value() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    app.finish_command(true);
    let saved = app.doc.clone();
    app.edit_feature(uid, 0.0);
    set_distance(&mut app, uid, -7.0);
    assert!(close(bounds(&app, 0).1.z, 13.0, 1e-6));
    app.cancel_mode();
    assert_eq!(app.doc, saved);
    assert!(close(bounds(&app, 0).1.z, 25.0, 1e-6));
    assert_eq!(app.undo.len(), 1, "a cancelled edit adds no history step");
}

/// Editing the box under a fillet (a wider box): the fillet has to follow, not break.
#[test]
fn widening_the_box_under_a_fillet_keeps_the_fillet_building() {
    use fusion_core::doc::Shape;
    let mut app = box_app(40.0, 30.0, 20.0);
    let box_uid = app.doc.features[0].uid;
    app.start_command("fillet", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let fillet = cmd_uid(&app);
    assert_eq!(app.ev.error(fillet), None);
    app.finish_command(true);
    app.edit_feature(box_uid, 0.0);
    edit(&mut app, box_uid, |k| if let FeatureKind::Primitive { shape: Shape::Box { w, .. }, .. } = k { *w = Expr::num(60.0) });
    app.finish_command(true);
    assert_eq!(app.ev.error(fillet), None, "the fillet of the top face still builds on the wider box");
    let (lo, hi) = bounds(&app, 0);
    assert!(close(hi.x - lo.x, 60.0, 1e-6));
    assert!(vol(&app, 0) < 60.0 * 30.0 * 20.0 - 1.0, "the top edges are still rounded");
}

/// A taller box under a press-pull of its top face: the press-pull refers to the top face, which moved.
#[test]
fn making_the_box_taller_under_a_press_pull_keeps_pulling_the_top_face() {
    use fusion_core::doc::Shape;
    let mut app = box_app(40.0, 30.0, 20.0);
    let box_uid = app.doc.features[0].uid;
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let pp = cmd_uid(&app);
    app.finish_command(true);
    app.edit_feature(box_uid, 0.0);
    edit(&mut app, box_uid, |k| if let FeatureKind::Primitive { shape: Shape::Box { h, .. }, .. } = k { *h = Expr::num(30.0) });
    app.finish_command(true);
    assert_eq!(app.ev.error(pp), None, "the press-pull of the top face follows the taller box");
    assert!(close(bounds(&app, 0).1.z, 35.0, 1e-6), "{:?}", bounds(&app, 0));
}

/// The undo button stays enabled while a command runs. Undo there must not be silently reverted when the command closes.
#[test]
fn undo_pressed_during_a_command_is_not_reverted_by_closing_the_command() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let plain = app.doc.clone();
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    app.finish_command(true);
    // a second command is open when the user presses the toolbar's undo arrow
    app.start_command("fillet", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    app.do_undo();
    // the panel finds its feature gone and closes the command (dialog_ui returns Cancel), or the user presses Cancel
    if matches!(app.mode, Mode::Command(_)) {
        let uid = cmd_uid(&app);
        assert!(app.doc.feature(uid).is_some(), "after undo the open command refers to a feature that no longer exists");
        app.finish_command(false);
    }
    assert_eq!(app.doc, plain, "the undo the user asked for is kept: back to the plain box");
}

/// Redo after a new command is impossible (new branch), and undo across several commands walks back one at a time.
#[test]
fn undo_walks_back_through_several_commands_one_at_a_time() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let mut states = vec![app.doc.clone()];
    for tool in ["press_pull", "fillet", "hole"] {
        app.start_command(tool, 0.0);
        click_face(&mut app, 0, if tool == "hole" { DVec3::X } else { DVec3::Z }, false);
        let uid = cmd_uid(&app);
        assert_eq!(app.ev.error(uid), None, "{tool}");
        app.finish_command(true);
        states.push(app.doc.clone());
    }
    for want in states.iter().rev().skip(1) {
        app.do_undo();
        assert_eq!(&app.doc, want);
    }
    app.do_redo();
    assert_eq!(app.doc, states[1]);
    app.start_command("chamfer", 0.0);
    click_face(&mut app, 0, -DVec3::Z, false);
    app.finish_command(true);
    assert!(app.redo.is_empty(), "a new step drops the redo branch");
}

/// Deleting a feature from the tree is undoable.
#[test]
fn deleting_a_feature_is_undoable() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    app.finish_command(true);
    let with_hole = app.doc.clone();
    let v = vol(&app, 0);
    app.delete_feature(uid, 0.0);
    assert!(close(vol(&app, 0), 24000.0, 1e-3));
    app.do_undo();
    assert_eq!(app.doc, with_hole);
    assert!(close(vol(&app, 0), v, 1e-6));
}

// ------------------------------------------------------------------ several bodies

/// Shift-click two bodies in Idle, then Move: both move by the typed offset; a third shift-click on one of them deselects it.
#[test]
fn shift_selected_bodies_move_together_and_a_second_click_deselects() {
    let mut app = box_app(10.0, 10.0, 10.0);
    add_box(&mut app, [50.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    add_box(&mut app, [100.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let ids: Vec<u64> = app.ev.model().bodies.iter().map(|b| b.id).collect();
    click_face(&mut app, 0, DVec3::Z, true);
    click_face(&mut app, 1, DVec3::Z, true);
    click_face(&mut app, 2, DVec3::Z, true);
    assert_eq!(app.selection, ids);
    click_face(&mut app, 2, DVec3::Z, true);
    assert_eq!(app.selection, ids[..2].to_vec(), "a body picked twice toggles off");
    app.start_command("move", 0.0);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| if let FeatureKind::Move { translate, .. } = k { translate[2] = Expr::num(15.0) });
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert!(close(bounds(&app, 0).0.z, 15.0, 1e-6) && close(bounds(&app, 1).0.z, 15.0, 1e-6));
    assert!(close(bounds(&app, 2).0.z, 0.0, 1e-6), "the deselected body stays");
}

/// In a running body command, clicking a body twice removes it again.
#[test]
fn in_a_body_command_a_second_click_takes_the_body_out() {
    for tool in ["move", "scale", "mirror", "pattern", "split", "draft"] {
        let mut app = box_app(10.0, 10.0, 10.0);
        add_box(&mut app, [50.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        app.start_command(tool, 0.0);
        let uid = cmd_uid(&app);
        click_face(&mut app, 0, DVec3::Z, false);
        click_face(&mut app, 1, DVec3::Z, false);
        click_face(&mut app, 0, DVec3::Z, false);
        let bodies = match &app.doc.feature(uid).unwrap().kind {
            FeatureKind::Move { bodies, .. } | FeatureKind::Scale { bodies, .. } | FeatureKind::Mirror { bodies, .. } | FeatureKind::Pattern { bodies, .. } | FeatureKind::Split { bodies, .. } | FeatureKind::Draft { bodies, .. } | FeatureKind::Remove { bodies } => bodies.clone(),
            _ => panic!("{tool}"),
        };
        assert_eq!(bodies, vec![app.ev.model_before(app.doc.index_of(uid).unwrap()).bodies[1].id], "{tool}");
    }
}

/// Scale two selected bodies by 2: each gets eight times bigger and they scale about their common centre.
#[test]
fn scaling_two_selected_bodies_by_two() {
    let mut app = box_app(10.0, 10.0, 10.0);
    add_box(&mut app, [30.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    click_face(&mut app, 0, DVec3::Z, true);
    click_face(&mut app, 1, DVec3::Z, true);
    app.start_command("scale", 0.0);
    let uid = cmd_uid(&app);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 2);
    assert!(close(total_vol(&app), 2.0 * 8000.0, 1e-3), "{}", total_vol(&app));
    // the common centre is x = 20: the first body goes to -20..0, the second to 40..60
    assert!(close(bounds(&app, 0).0.x, -20.0, 1e-6) && close(bounds(&app, 1).1.x, 60.0, 1e-6), "{:?} {:?}", bounds(&app, 0), bounds(&app, 1));
}

/// Mirror a selected body: a copy appears on the other side of YZ.
#[test]
fn mirroring_a_body_makes_a_copy_across_the_plane() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("mirror", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 2);
    let (lo, hi) = bounds(&app, 1);
    assert!(close(lo.x, -40.0, 1e-6) && close(hi.x, 0.0, 1e-6), "{lo:?} {hi:?}");
    assert!(close(vol(&app, 1), 24000.0, 1e-3));
}

/// Mirror with "join" on a body touching the mirror plane: one body twice as big.
#[test]
fn mirroring_with_join_doubles_the_body() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("mirror", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| if let FeatureKind::Mirror { join, .. } = k { *join = true });
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 1);
    assert!(close(vol(&app, 0), 48000.0, 1e-2), "{}", vol(&app, 0));
}

/// Pattern a selected body: three copies 30 mm apart.
#[test]
fn a_rectangular_pattern_of_a_selected_body() {
    let mut app = box_app(20.0, 20.0, 20.0);
    click_face(&mut app, 0, DVec3::Z, false);
    app.start_command("pattern", 0.0);
    let uid = cmd_uid(&app);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 3);
    assert!(close(bounds(&app, 2).1.x, 80.0, 1e-6), "{:?}", bounds(&app, 2));
}

/// A circular pattern of 4 around Z.
#[test]
fn a_circular_pattern_of_four_around_z() {
    use fusion_core::doc::{Axis3, PatternKind};
    let mut app = app();
    add_box(&mut app, [20.0, -5.0, 0.0], [10.0, 10.0, 10.0]);
    app.start_command("pattern", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| if let FeatureKind::Pattern { kind, .. } = k { *kind = PatternKind::Circular { count: Expr::num(4.0), axis: Axis3::Z, angle: Expr::num(360.0) } });
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 4);
    assert!(close(total_vol(&app), 4000.0, 1e-2));
}

/// Combine: pick the target, then the tool. Union of two overlapping boxes, then subtract, then intersect.
#[test]
fn combine_by_clicking_target_then_tool() {
    use fusion_core::csg::BoolOp;
    for (op, want_bodies, want_vol) in [(BoolOp::Union, 1, 2.0 * 8000.0 - 4000.0), (BoolOp::Subtract, 1, 4000.0), (BoolOp::Intersect, 1, 4000.0)] {
        let mut app = box_app(20.0, 20.0, 20.0);
        add_box(&mut app, [10.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
        app.start_command("combine", 0.0);
        assert_eq!(cmd_pick(&app), PickTarget::CombineTarget);
        click_face(&mut app, 0, -DVec3::X, false);
        assert_eq!(cmd_pick(&app), PickTarget::CombineTools, "{op:?}: after the target the tools are picked");
        click_face(&mut app, 1, DVec3::X, false);
        let uid = cmd_uid(&app);
        edit(&mut app, uid, |k| if let FeatureKind::Combine { op: o, .. } = k { *o = op });
        assert_eq!(app.ev.error(uid), None, "{op:?}");
        app.finish_command(true);
        assert_eq!(nbodies(&app), want_bodies, "{op:?}");
        assert!(close(total_vol(&app), want_vol, 1.0), "{op:?}: {} vs {want_vol}", total_vol(&app));
    }
}

/// Combine with the target clicked again as a tool: it must not become its own tool.
#[test]
fn combine_ignores_the_target_clicked_as_a_tool() {
    let mut app = box_app(20.0, 20.0, 20.0);
    add_box(&mut app, [10.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
    app.start_command("combine", 0.0);
    click_face(&mut app, 0, -DVec3::X, false);
    click_face(&mut app, 0, -DVec3::X, false);
    let uid = cmd_uid(&app);
    let FeatureKind::Combine { target, tools, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_ne!(target, 0);
    assert!(tools.is_empty());
}

/// Preselected two bodies → combine starts with target and tool filled.
#[test]
fn combine_with_two_preselected_bodies_is_ready_at_once() {
    let mut app = box_app(20.0, 20.0, 20.0);
    add_box(&mut app, [10.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
    click_face(&mut app, 0, -DVec3::X, true);
    click_face(&mut app, 1, DVec3::X, true);
    app.start_command("combine", 0.0);
    let uid = cmd_uid(&app);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 1);
}

/// A new box added next to (but not touching) a body whose bounding box it overlaps stays a separate body.
#[test]
fn a_box_near_but_not_touching_a_sphere_stays_a_separate_body() {
    let mut app = app();
    app.doc.insert(FeatureKind::Primitive { shape: fusion_core::doc::Shape::Sphere { r: Expr::num(10.0) }, at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)], op: fusion_core::Op::NewBody });
    app.after_doc_change();
    // the box tool defaults to Join when bodies exist; place it in the corner of the sphere's bounding box
    app.start_command("box", 0.0);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| if let FeatureKind::Primitive { shape: fusion_core::doc::Shape::Box { w, d, h }, at, .. } = k {
        *w = Expr::num(2.0); *d = Expr::num(2.0); *h = Expr::num(2.0);
        *at = [Expr::num(7.5), Expr::num(7.5), Expr::num(7.5)];
    });
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 2, "a box that does not touch the sphere must not be merged into it");
}

/// Shell the first of two equally tall boxes: only the clicked body is hollowed.
#[test]
fn shelling_one_body_leaves_a_neighbour_of_the_same_height_solid() {
    let mut app = box_app(40.0, 30.0, 20.0);
    add_box(&mut app, [100.0, 0.0, 0.0], [40.0, 30.0, 20.0]);
    app.start_command("shell", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert!(vol(&app, 0) < 24000.0 * 0.6, "the clicked body is hollow");
    assert!(close(vol(&app, 1), 24000.0, 1e-3), "the other body was not clicked and must stay solid: {}", vol(&app, 1));
}

/// Fillet with two bodies: the first edge clicked chooses the body, and the size defaults to 10 % of what fits that body.
#[test]
fn a_fillet_on_the_second_of_two_bodies_takes_its_size_from_that_body() {
    let mut app = box_app(40.0, 40.0, 40.0);
    add_box(&mut app, [100.0, 0.0, 0.0], [6.0, 6.0, 6.0]);
    app.start_command("fillet", 0.0);
    let uid = cmd_uid(&app);
    click_edge(&mut app, 1, |e| !e.circle && e.a[2] > 5.9 && e.b[2] > 5.9, false);
    let FeatureKind::Fillet { body, edges, radius } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(body, app.ev.model().bodies[1].id);
    assert!(!edges.is_empty());
    assert_eq!(radius.eval(&app.doc.params).unwrap(), 0.3, "10 % of half the small box");
    assert_eq!(app.ev.error(uid), None);
    // an edge of the other body is not taken
    click_edge(&mut app, 0, |e| !e.circle && e.a[2] > 39.0 && e.b[2] > 39.0, false);
    let FeatureKind::Fillet { body: b2, edges: e2, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!((b2, e2.len()), (body, edges.len()));
}

/// Remove: a removed body vanishes from the view at once, so the remaining one is clicked by its own slot.
#[test]
fn removing_two_bodies_one_click_each() {
    let mut app = box_app(10.0, 10.0, 10.0);
    add_box(&mut app, [50.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    add_box(&mut app, [100.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    app.start_command("remove", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    assert_eq!(app.ev.error(uid), None);
    assert_eq!(app.pick_model().bodies.len(), 2);
    click_face(&mut app, 0, DVec3::Z, false);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 1);
    assert!(close(bounds(&app, 0).0.x, 100.0, 1e-6));
}

/// While mirroring, the copy made by this very mirror is visible and clickable. Clicking it must not break the command.
#[test]
fn clicking_the_preview_copy_during_mirror_does_not_break_it() {
    let mut broken = Vec::new();
    for tool in ["mirror", "pattern"] {
        let mut app = box_app(20.0, 20.0, 20.0);
        app.start_command(tool, 0.0);
        let uid = cmd_uid(&app);
        click_face(&mut app, 0, DVec3::Z, false);
        assert_eq!(app.ev.error(uid), None, "{tool}");
        let shown = app.pick_model();
        assert!(shown.bodies.len() >= 2, "{tool}: the copy is shown and hit-tested");
        // the user clicks the copy (it looks like any other body)
        click_face(&mut app, 1, DVec3::Z, false);
        if let Some(e) = app.ev.error(uid) { broken.push(format!("{tool}: {e}")); }
    }
    assert!(broken.is_empty(), "clicking the preview copy leaves an error: {broken:?}");
}

/// Move with "copy": the copy is shown; clicking it must not break the move.
#[test]
fn clicking_the_preview_copy_during_move_with_copy_does_not_break_it() {
    let mut app = box_app(20.0, 20.0, 20.0);
    app.start_command("move", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    edit(&mut app, uid, |k| if let FeatureKind::Move { translate, copy, .. } = k { translate[0] = Expr::num(50.0); *copy = true; });
    assert_eq!(app.ev.error(uid), None);
    assert_eq!(app.pick_model().bodies.len(), 2);
    click_face(&mut app, 1, DVec3::Z, false);
    assert_eq!(app.ev.error(uid), None, "clicking the moved copy must not leave an error");
}

/// Split by clicking a body: two bodies; clicking the newly split-off part must not break it.
#[test]
fn split_a_clicked_body_in_two() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("split", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    assert_eq!(app.ev.error(uid), None);
    assert_eq!(nbodies(&app), 2);
    assert!(close(total_vol(&app), 24000.0, 1e-2));
    app.finish_command(true);
    assert_eq!(nbodies(&app), 2);
}

// ------------------------------------------------------------------ faces: extrude, press-pull, holes, shell, fillet/chamfer

/// The Extrude tool on a box face (no sketch): + adds material, − cuts, on every face.
#[test]
fn face_extrude_on_every_face_of_a_box_adds_and_cuts() {
    for dir in [DVec3::X, -DVec3::X, DVec3::Y, -DVec3::Y, DVec3::Z, -DVec3::Z] {
        for dist in [8.0, -8.0] {
            let mut app = box_app(40.0, 30.0, 20.0);
            app.start_command("extrude", 0.0);
            click_face(&mut app, 0, dir, false);
            let uid = cmd_uid(&app);
            set_distance(&mut app, uid, dist);
            assert_eq!(app.ev.error(uid), None, "{dir:?} {dist}");
            app.finish_command(true);
            assert_eq!(nbodies(&app), 1, "{dir:?} {dist}");
            let area = 24000.0 / if dir.x != 0.0 { 40.0 } else if dir.y != 0.0 { 30.0 } else { 20.0 };
            let want = 24000.0 + area * dist;
            assert!(close(vol(&app, 0), want, 1.0), "{dir:?} {dist}: {} vs {want}", vol(&app, 0));
        }
    }
}

/// Face extrude by the whole thickness inwards: the box is cut away completely. That must not silently delete the body.
#[test]
fn pushing_a_face_through_the_whole_body_reports_instead_of_deleting_it() {
    let mut silent = Vec::new();
    for tool in ["press_pull", "extrude"] {
        let mut app = box_app(40.0, 30.0, 20.0);
        app.start_command(tool, 0.0);
        click_face(&mut app, 0, DVec3::Z, false);
        let uid = cmd_uid(&app);
        set_distance(&mut app, uid, -25.0);
        if nbodies(&app) == 0 && app.ev.error(uid).is_none() { silent.push(tool); }
    }
    assert!(silent.is_empty(), "−25 on a 20 mm box deleted the body without any message: {silent:?}");
}

/// Press-pull the top of a box whose top edges were rounded: up and down.
#[test]
fn press_pull_after_a_fillet_on_the_top_face() {
    for dist in [6.0, -6.0] {
        let mut app = box_app(40.0, 30.0, 20.0);
        app.start_command("fillet", 0.0);
        click_face(&mut app, 0, DVec3::Z, false);
        let f = cmd_uid(&app);
        edit(&mut app, f, |k| if let FeatureKind::Fillet { radius, .. } = k { *radius = Expr::num(3.0) });
        assert_eq!(app.ev.error(f), None);
        app.finish_command(true);
        let v0 = vol(&app, 0);
        app.start_command("press_pull", 0.0);
        click_face(&mut app, 0, DVec3::Z, false);
        let uid = cmd_uid(&app);
        set_distance(&mut app, uid, dist);
        assert_eq!(app.ev.error(uid), None, "{dist}");
        assert_eq!(nbodies(&app), 1);
        assert!(close(bounds(&app, 0).1.z, 20.0 + dist, 1e-3), "{dist}: top at {}", bounds(&app, 0).1.z);
        let v = vol(&app, 0);
        assert!(if dist > 0.0 { v > v0 } else { v < v0 }, "{dist}: {v} vs {v0}");
    }
}

/// Press-pull a side face of a box whose top edges were rounded.
#[test]
fn press_pull_a_side_face_after_a_fillet() {
    for dist in [5.0, -5.0] {
        let mut app = box_app(40.0, 30.0, 20.0);
        app.start_command("fillet", 0.0);
        click_face(&mut app, 0, DVec3::Z, false);
        app.finish_command(true);
        app.start_command("press_pull", 0.0);
        click_face(&mut app, 0, DVec3::X, false);
        let uid = cmd_uid(&app);
        set_distance(&mut app, uid, dist);
        assert_eq!(app.ev.error(uid), None, "{dist}");
        assert!(close(bounds(&app, 0).1.x, 40.0 + dist, 1e-3), "{dist}: {:?}", bounds(&app, 0));
    }
}

/// Press-pull the rim (top face) of a shelled box, and the bottom of it.
#[test]
fn press_pull_after_a_shell() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("shell", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    app.finish_command(true);
    let v0 = vol(&app, 0);
    // the bottom: thicker floor outwards
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, -DVec3::Z, false);
    let uid = cmd_uid(&app);
    set_distance(&mut app, uid, 4.0);
    assert_eq!(app.ev.error(uid), None);
    assert!(close(vol(&app, 0), v0 + 1200.0 * 4.0, 1.0), "{} vs {}", vol(&app, 0), v0 + 4800.0);
    app.finish_command(true);
    // the rim of the walls: pulling it up raises all four walls
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    set_distance(&mut app, uid, 5.0);
    assert_eq!(app.ev.error(uid), None);
    assert!(close(bounds(&app, 0).1.z, 25.0, 1e-3), "{:?}", bounds(&app, 0));
}

/// A hole on a side face near an edge (partly outside the face) still drills; Shift-click adds a second one on the same face.
#[test]
fn holes_on_a_side_face_near_an_edge_and_a_shift_click_second_hole() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::X, false);
    let uid = cmd_uid(&app);
    let v1 = vol(&app, 0);
    assert!(v1 < 24000.0);
    // a second hole by Shift-click on the same face
    click_face(&mut app, 0, DVec3::X, true);
    let FeatureKind::Hole { more, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(more.len(), 1, "Shift-click on the same face adds a hole");
    // move the first hole close to the edge (2 mm in from the top edge), as typed in the panel
    edit(&mut app, uid, |k| if let FeatureKind::Hole { plane, at, .. } = k {
        let local = plane.to_local(DVec3::new(40.0, 15.0, 18.0));
        at[0] = Expr::num(local[0]);
        at[1] = Expr::num(local[1]);
    });
    assert_eq!(app.ev.error(uid), None);
    assert_eq!(nbodies(&app), 1);
    let v = vol(&app, 0);
    assert!(v < 24000.0 && v > 24000.0 - 2.0 * std::f64::consts::PI * 16.0 * 40.0 - 1.0, "{v}");
    app.finish_command(true);
}

/// A hole on the side face goes through along X: 40 mm long.
#[test]
fn a_through_hole_on_a_side_face_removes_a_full_cylinder() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::X, false);
    let uid = cmd_uid(&app);
    // centre it on the face exactly
    edit(&mut app, uid, |k| if let FeatureKind::Hole { plane, at, .. } = k {
        let local = plane.to_local(DVec3::new(40.0, 15.0, 10.0));
        at[0] = Expr::num(local[0]);
        at[1] = Expr::num(local[1]);
    });
    assert_eq!(app.ev.error(uid), None);
    let want = 24000.0 - std::f64::consts::PI * 16.0 * 40.0;
    assert!(close(vol(&app, 0), want, want * 0.005), "{} vs {want}", vol(&app, 0));
}

/// Shell with three open faces (top, +X, −Y): each adds an opening; all build.
#[test]
fn shell_with_three_open_faces() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("shell", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    let v1 = vol(&app, 0);
    click_face(&mut app, 0, DVec3::X, true);
    let v2 = vol(&app, 0);
    click_face(&mut app, 0, -DVec3::Y, true);
    let v3 = vol(&app, 0);
    assert_eq!(app.ev.error(uid), None);
    assert!(v3 < v2 && v2 < v1, "{v1} {v2} {v3}");
    // walls 2 mm: box minus inner cavity open on three sides: 40x30x20 minus (38x28x18) roughly
    let cavity = (40.0 - 2.0) * (30.0 - 2.0) * (20.0 - 2.0);
    assert!(close(v3, 24000.0 - cavity, 300.0), "{v3} vs {}", 24000.0 - cavity);
    // a Shift-click on an open face closes it again
    click_face(&mut app, 0, DVec3::X, true);
    assert!(vol(&app, 0) > v3, "closing a face again adds its wall back");
    let FeatureKind::Shell { more, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(more.len(), 1);
    app.finish_command(true);
}

/// Shell thickness: zero, negative, too big, huge — an error each time, never a panic or a vanished body.
#[test]
fn shell_with_bad_thickness_reports_an_error() {
    let mut silent = Vec::new();
    for t in [0.0, -2.0, 16.0, 25.0, 1e9] {
        let mut app = box_app(40.0, 30.0, 20.0);
        app.start_command("shell", 0.0);
        click_face(&mut app, 0, DVec3::Z, false);
        let uid = cmd_uid(&app);
        edit(&mut app, uid, |k| if let FeatureKind::Shell { thickness, .. } = k { *thickness = Expr::num(t) });
        if app.ev.error(uid).is_none() { silent.push(format!("{t} -> volume {:.0}", total_vol(&app))); }
        assert_eq!(nbodies(&app), 1);
    }
    assert!(silent.is_empty(), "shell thicknesses accepted without an error: {silent:?}");
}

/// Chamfer by clicking a face (takes its edges) and by one edge.
#[test]
fn chamfer_by_face_and_by_edge() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("chamfer", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    let FeatureKind::Chamfer { edges, dist, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(edges.len(), 4, "a face click takes its four edges");
    let d = dist.eval(&app.doc.params).unwrap();
    assert_eq!(d, 1.0);
    assert_eq!(app.ev.error(uid), None);
    // perimeter 140 mm, each chamfer removes d²/2 per mm, corners differ a little
    let want = 24000.0 - 140.0 * d * d / 2.0;
    assert!(close(vol(&app, 0), want, 2.0), "{} vs {want}", vol(&app, 0));
    app.finish_command(true);

    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("chamfer", 0.0);
    let uid = cmd_uid(&app);
    click_edge(&mut app, 0, |e| !e.circle && e.a[2] == 20.0 && e.b[2] == 20.0 && e.a[1] == 0.0 && e.b[1] == 0.0, false);
    let FeatureKind::Chamfer { edges, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(edges.len(), 1);
    edit(&mut app, uid, |k| if let FeatureKind::Chamfer { dist, .. } = k { *dist = Expr::num(4.0) });
    assert_eq!(app.ev.error(uid), None);
    assert!(close(vol(&app, 0), 24000.0 - 40.0 * 8.0, 1e-2), "{}", vol(&app, 0));
    // the same edge clicked again lets it go
    click_edge(&mut app, 0, |e| !e.circle && e.a[2] == 20.0 && e.b[2] == 20.0 && e.a[1] == 0.0 && e.b[1] == 0.0, false);
    let FeatureKind::Chamfer { edges, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert!(edges.is_empty());
}

/// Fillet by one edge, with a typed radius: the volume matches a quarter-round cut.
#[test]
fn fillet_of_one_edge_removes_the_right_amount() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("fillet", 0.0);
    let uid = cmd_uid(&app);
    click_edge(&mut app, 0, |e| !e.circle && e.a[2] == 20.0 && e.b[2] == 20.0 && e.a[1] == 0.0 && e.b[1] == 0.0, false);
    edit(&mut app, uid, |k| if let FeatureKind::Fillet { radius, .. } = k { *radius = Expr::num(5.0) });
    assert_eq!(app.ev.error(uid), None);
    let want = 24000.0 - 40.0 * 25.0 * (1.0 - std::f64::consts::PI / 4.0);
    assert!(close(vol(&app, 0), want, 3.0), "{} vs {want}", vol(&app, 0));
}

/// Shift + face click with the fillet tool takes every edge of the body.
#[test]
fn shift_click_on_a_face_takes_all_edges_for_a_fillet() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("fillet", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, true);
    let FeatureKind::Fillet { edges, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(edges.len(), 12);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert!(vol(&app, 0) < 24000.0);
}

/// Clicking two adjacent faces with the fillet tool: the shared edge is taken once, seven edges in all.
#[test]
fn two_adjacent_faces_give_seven_fillet_edges() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("fillet", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    click_face(&mut app, 0, DVec3::X, false);
    let FeatureKind::Fillet { edges, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(edges.len(), 7);
    assert_eq!(app.ev.error(uid), None);
}

// ------------------------------------------------------------------ extreme values typed into fields

/// Runs `tool` on a fresh 40x30x20 box with a top-face click, sets one value and reports what happened.
fn try_value(tool: &str, value: &str, set: &dyn Fn(&mut FeatureKind, Expr)) -> (Option<String>, usize, Option<(DVec3, DVec3)>) {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command(tool, 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| set(k, Expr(value.to_string())));
    let err = app.ev.error(uid).map(str::to_string);
    let n = nbodies(&app);
    let mut lo = DVec3::splat(f64::MAX);
    let mut hi = DVec3::splat(f64::MIN);
    for b in &app.ev.model().bodies {
        if let Some((a, c)) = b.mesh().bounds() { lo = lo.min(a); hi = hi.max(c); }
    }
    app.finish_command(true);
    app.do_undo();
    assert_eq!(nbodies(&app), 1, "{tool} {value}: undo restores the box");
    (err, n, (lo.x <= hi.x).then_some((lo, hi)))
}

/// Very large, very small, zero, negative and non-numeric values in the main field of each tool: never an internal error
/// (a caught panic), never a non-finite model, and zero/negative sizes are refused with a message.
#[test]
fn extreme_values_in_tool_fields_are_refused_or_build_cleanly() {
    type Setter = Box<dyn Fn(&mut FeatureKind, Expr)>;
    let cases: Vec<(&str, Setter, Vec<&str>, Vec<&str>)> = vec![
        ("press_pull", Box::new(|k, e| if let FeatureKind::PressPull { distance, .. } = k { *distance = e }), vec!["0", "1e-9", "1e-4", "1e6", "1e9", "-1e9", "w"], vec!["0", "w"]),
        ("extrude", Box::new(|k, e| if let FeatureKind::Extrude { extent: fusion_core::doc::Extent::OneSide(d), .. } = k { *d = e }), vec!["0", "1e-9", "1e-4", "1e6", "1e9", "w"], vec!["0", "w"]),
        ("fillet", Box::new(|k, e| if let FeatureKind::Fillet { radius, .. } = k { *radius = e }), vec!["0", "-1", "1e-6", "9.99", "10", "15", "1e9", "w"], vec!["0", "-1", "1e9", "w"]),
        ("chamfer", Box::new(|k, e| if let FeatureKind::Chamfer { dist, .. } = k { *dist = e }), vec!["0", "-1", "1e-6", "10", "19.9", "1e9", "w"], vec!["0", "-1", "1e9", "w"]),
        ("hole", Box::new(|k, e| if let FeatureKind::Hole { dia, .. } = k { *dia = e }), vec!["0", "-5", "1e-3", "29.9", "100", "1e9", "w"], vec!["0", "-5", "w"]),
        ("shell", Box::new(|k, e| if let FeatureKind::Shell { thickness, .. } = k { *thickness = e }), vec!["0", "-1", "1e-3", "9.9", "w"], vec!["0", "-1", "w"]),
        ("scale", Box::new(|k, e| if let FeatureKind::Scale { factor, .. } = k { factor[0] = e }), vec!["0", "-1", "1e-6", "1e6", "w"], vec!["0", "-1", "w"]),
        ("move", Box::new(|k, e| if let FeatureKind::Move { translate, .. } = k { translate[0] = e }), vec!["1e9", "-1e9", "1e-9", "w"], vec!["w"]),
        ("move", Box::new(|k, e| if let FeatureKind::Move { rotate, .. } = k { rotate[2] = e }), vec!["1e9", "360", "-720", "w"], vec!["w"]),
        ("mirror", Box::new(|k, e| if let FeatureKind::Mirror { offset, .. } = k { *offset = e }), vec!["1e9", "-1e9", "20", "w"], vec!["w"]),
        ("pattern", Box::new(|k, e| if let FeatureKind::Pattern { kind: fusion_core::doc::PatternKind::Rect { nx, .. }, .. } = k { *nx = e }), vec!["0", "1", "1001", "2.4", "-3", "w"], vec!["0", "1", "1001", "-3", "w"]),
        ("draft", Box::new(|k, e| if let FeatureKind::Draft { angle, .. } = k { *angle = e }), vec!["0", "-10", "45", "89.9", "90", "1e9", "w"], vec!["90", "w"]),
        ("split", Box::new(|k, e| if let FeatureKind::Split { offset, .. } = k { *offset = e }), vec!["0", "20", "-5", "1e9", "w"], vec!["0", "20", "-5", "1e9", "w"]),
    ];
    let mut problems = Vec::new();
    for (tool, set, values, must_fail) in &cases {
        for v in values {
            let (err, n, b) = try_value(tool, v, set.as_ref());
            if let Some(e) = &err {
                if e.contains("внутренняя ошибка") { problems.push(format!("{tool} = {v}: caught panic ({e})")); }
            } else {
                if must_fail.contains(v) { problems.push(format!("{tool} = {v}: accepted without an error ({n} bodies)")); }
                if let Some((lo, hi)) = b {
                    if !(lo.is_finite() && hi.is_finite()) { problems.push(format!("{tool} = {v}: non-finite bounds")); }
                }
                // a body cut away completely without a message is covered by the ignored tests about vanishing bodies
                let known_vanish = matches!((*tool, *v), ("press_pull", "-1e9") | ("hole", "100") | ("hole", "1e9"));
                if n == 0 && !known_vanish { problems.push(format!("{tool} = {v}: the body vanished without an error")); }
            }
        }
    }
    assert!(problems.is_empty(), "problems:\n{}", problems.join("\n"));
}

/// A hole wider than the box (dia 100 on a 40x30 face) cuts the whole body away: that must be reported, not delete the body.
#[test]
fn a_hole_wider_than_the_body_reports_instead_of_deleting_it() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| if let FeatureKind::Hole { dia, .. } = k { *dia = Expr::num(100.0) });
    assert!(nbodies(&app) == 1 || app.ev.error(uid).is_some(), "the body vanished without a message");
}

// ------------------------------------------------------------------ switching tools, timeline, suppress

/// Switching to another tool in the middle of a command: the half-done step is dropped (start_command cancels it) and
/// the new tool starts on the untouched document; nothing is left half-way and the history has no stray steps.
#[test]
fn switching_tools_mid_command_drops_the_unfinished_step() {
    let mut app = box_app(40.0, 30.0, 20.0);
    let plain = app.doc.clone();
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let pp = cmd_uid(&app);
    set_distance(&mut app, pp, 7.0);
    for tool in ["fillet", "hole", "shell", "move", "chamfer", "combine", "extrude", "pattern"] {
        app.start_command(tool, 0.0);
        let uid = cmd_uid(&app);
        assert_eq!(app.doc.features.len(), plain.features.len() + 1, "{tool}: only the new step is there");
        assert!(!app.doc.features.iter().any(|f| matches!(f.kind, FeatureKind::PressPull { .. })), "{tool}: the press-pull is gone");
        assert_eq!(app.doc.index_of(uid), Some(1), "{tool}");
        click_face(&mut app, 0, DVec3::Z, false);
    }
    app.cancel_mode();
    assert_eq!(app.doc, plain);
    assert!(app.undo.is_empty());
}

/// Opening an existing step for editing while a new command is half-done.
#[test]
fn editing_a_step_while_a_command_is_open_closes_the_command_cleanly() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let pp = cmd_uid(&app);
    app.finish_command(true);
    let after_pp = app.doc.clone();
    app.start_command("fillet", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    app.edit_feature(pp, 0.0);
    assert_eq!(cmd_uid(&app), pp);
    assert_eq!(app.doc, after_pp, "the unfinished fillet is dropped");
    set_distance(&mut app, pp, 3.0);
    app.finish_command(true);
    assert!(close(bounds(&app, 0).1.z, 23.0, 1e-6));
}

/// With the timeline rolled back to the box, a new press-pull goes in before the later side hole; rolling forward rebuilds both.
#[test]
fn a_step_added_with_the_timeline_rolled_back_goes_in_before_later_steps() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::X, false);
    let hole = cmd_uid(&app);
    edit(&mut app, hole, |k| if let FeatureKind::Hole { plane, at, .. } = k {
        let local = plane.to_local(DVec3::new(40.0, 15.0, 10.0));
        at[0] = Expr::num(local[0]);
        at[1] = Expr::num(local[1]);
    });
    app.finish_command(true);
    app.doc.rollback = 1;
    app.after_doc_change();
    assert!(close(vol(&app, 0), 24000.0, 1e-3));
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let pp = cmd_uid(&app);
    app.finish_command(true);
    assert_eq!(app.doc.index_of(pp), Some(1));
    assert_eq!(app.doc.rollback, 2);
    app.doc.rollback = app.doc.features.len();
    app.after_doc_change();
    assert_eq!(app.ev.error(hole), None);
    assert_eq!(app.ev.error(pp), None);
    let want = 40.0 * 30.0 * 25.0 - std::f64::consts::PI * 16.0 * 40.0;
    assert!(close(vol(&app, 0), want, want * 0.005), "{} vs {want}", vol(&app, 0));
}

/// Suppressing a step and undoing that.
#[test]
fn suppressing_a_hole_and_undoing_it() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let hole = cmd_uid(&app);
    app.finish_command(true);
    let drilled = vol(&app, 0);
    app.checkpoint();
    app.doc.feature_mut(hole).unwrap().suppressed = true;
    app.after_doc_change();
    assert!(close(vol(&app, 0), 24000.0, 1e-3));
    app.do_undo();
    assert!(close(vol(&app, 0), drilled, 1e-6));
}

/// A through hole on the top face, then the box made taller: the hole must still go all the way through.
#[test]
fn a_through_hole_stays_through_when_the_box_gets_taller() {
    use fusion_core::doc::Shape;
    let mut app = box_app(40.0, 30.0, 20.0);
    let box_uid = app.doc.features[0].uid;
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let hole = cmd_uid(&app);
    app.finish_command(true);
    app.edit_feature(box_uid, 0.0);
    edit(&mut app, box_uid, |k| if let FeatureKind::Primitive { shape: Shape::Box { h, .. }, .. } = k { *h = Expr::num(30.0) });
    app.finish_command(true);
    assert_eq!(app.ev.error(hole), None);
    let want = 40.0 * 30.0 * 30.0 - std::f64::consts::PI * 16.0 * 30.0;
    assert!(close(vol(&app, 0), want, want * 0.005), "{} vs {want}: the hole no longer goes through", vol(&app, 0));
}

/// With two bodies a fillet starts at the 1 mm placeholder; if the user types exactly 1 before picking, that value must stay.
#[test]
fn a_typed_fillet_radius_of_one_is_not_replaced_by_the_default() {
    let mut app = box_app(40.0, 40.0, 40.0);
    add_box(&mut app, [100.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    app.start_command("fillet", 0.0);
    let uid = cmd_uid(&app);
    // the user types 1 in the radius field (the panel stores the trimmed text)
    edit(&mut app, uid, |k| if let FeatureKind::Fillet { radius, .. } = k { *radius = Expr("1".into()) });
    click_edge(&mut app, 0, |e| !e.circle && e.a[2] > 39.0 && e.b[2] > 39.0, false);
    let FeatureKind::Fillet { radius, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(radius.eval(&app.doc.params).unwrap(), 1.0, "the typed radius was replaced");
}

/// Editing an existing fillet: a further face click adds its edges, OK keeps both sets; undo restores the old set.
#[test]
fn editing_a_fillet_adds_edges_of_another_face() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("fillet", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    app.finish_command(true);
    let v4 = vol(&app, 0);
    app.edit_feature(uid, 0.0);
    assert_eq!(cmd_pick(&app), PickTarget::Edges);
    click_face(&mut app, 0, -DVec3::Z, false);
    let FeatureKind::Fillet { edges, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_eq!(edges.len(), 8, "the bottom face is picked on the body before the fillet");
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert!(vol(&app, 0) < v4);
    app.do_undo();
    assert!(close(vol(&app, 0), v4, 1e-6));
}

/// Hole tool on the second body: the click picks that body's face and drills only it.
#[test]
fn a_hole_clicked_on_the_second_body_drills_only_that_body() {
    let mut app = box_app(40.0, 30.0, 20.0);
    add_box(&mut app, [100.0, 0.0, 0.0], [40.0, 30.0, 20.0]);
    app.start_command("hole", 0.0);
    click_face(&mut app, 1, DVec3::Z, false);
    let uid = cmd_uid(&app);
    assert_eq!(app.ev.error(uid), None);
    assert!(close(vol(&app, 0), 24000.0, 1e-3), "first body untouched");
    assert!(vol(&app, 1) < 24000.0, "second body drilled");
}

/// Press-pull / face-extrude the top of the second of two side-by-side boxes of the same height: only that box grows.
#[test]
fn press_pull_on_one_of_two_coplanar_tops_moves_only_that_face() {
    let mut wrong = Vec::new();
    for tool in ["press_pull", "extrude"] {
        let mut app = box_app(40.0, 30.0, 20.0);
        add_box(&mut app, [100.0, 0.0, 0.0], [40.0, 30.0, 20.0]);
        app.start_command(tool, 0.0);
        click_face(&mut app, 1, DVec3::Z, false);
        let uid = cmd_uid(&app);
        set_distance(&mut app, uid, 5.0);
        assert_eq!(app.ev.error(uid), None, "{tool}");
        app.finish_command(true);
        assert_eq!(nbodies(&app), 2, "{tool}");
        let (a, b) = (bounds(&app, 0).1.z, bounds(&app, 1).1.z);
        if !(close(a, 20.0, 1e-6) && close(b, 25.0, 1e-6)) { wrong.push(format!("{tool}: clicked box top {b}, other box top {a}")); }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// Two boxes stacked: press-pulling the top box's bottom face down into the lower box joins... it must not merge the two bodies
/// unless they really overlap. Pulling the top face of the lower box up into the upper one merges them.
#[test]
fn press_pull_into_a_neighbour_joins_with_it() {
    let mut app = box_app(40.0, 30.0, 20.0);
    add_box(&mut app, [0.0, 0.0, 30.0], [40.0, 30.0, 10.0]);
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    set_distance(&mut app, uid, 15.0);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert_eq!(nbodies(&app), 1, "pulled into the upper box, the two become one");
    assert!(close(vol(&app, 0), 40.0 * 30.0 * 40.0, 1.0), "{}", vol(&app, 0));
}

// ------------------------------------------------------------------ place, planes, thread, remove

/// Place: click the bottom of the small box, then the top of the big one: the small box sits on the big one.
#[test]
fn placing_one_box_on_top_of_another_by_two_clicks() {
    let mut app = box_app(40.0, 30.0, 20.0);
    add_box(&mut app, [100.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    app.start_command("place", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 1, -DVec3::Z, false);
    assert_eq!(cmd_pick(&app), PickTarget::AlignTarget);
    click_face(&mut app, 0, DVec3::Z, false);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    let (lo, hi) = bounds(&app, 1);
    assert!(close(lo.z, 20.0, 1e-6) && close(hi.z, 30.0, 1e-6), "{lo:?} {hi:?}");
    assert!(close(vol(&app, 1), 1000.0, 1e-3));
}

/// Place a box's side face on the XY plane (lays it down): one click on the face, one on the origin plane.
#[test]
fn placing_a_side_face_on_the_xy_plane_lays_the_box_down() {
    let mut app = app();
    add_box(&mut app, [0.0, 0.0, 5.0], [10.0, 20.0, 40.0]);
    app.start_command("place", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::X, false);
    app.handle_click(Some(Hit::OriginPlane(0, 10.0)), false, 0.0);
    assert_eq!(app.ev.error(uid), None);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(hi.z - lo.z, 10.0, 1e-6), "the 10 mm side is now the height: {lo:?} {hi:?}");
    assert!(close(lo.z, 0.0, 1e-6) || close(hi.z, 0.0, 1e-6), "it lies on XY: {lo:?} {hi:?}");
}

/// Offset plane from a clicked face: 10 mm above the top face.
#[test]
fn an_offset_plane_from_a_clicked_face() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("plane", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    let planes = app.doc.offset_planes();
    assert_eq!(planes.len(), 1);
    assert!(close(planes[0].2.origin[2], 30.0, 1e-9), "{:?}", planes[0].2);
    // and a sketch can be started on it by clicking it
    app.start_sketch();
    app.handle_click(Some(Hit::UserPlane(0, 10.0)), false, 0.0);
    assert!(matches!(app.mode, Mode::Sketch(_)));
    app.cancel_mode();
}

/// Thread on a cylinder: click its top rim.
#[test]
fn thread_on_a_cylinder_by_clicking_its_rim() {
    let mut app = app();
    app.doc.insert(FeatureKind::Primitive { shape: fusion_core::doc::Shape::Cylinder { r: Expr::num(5.0), h: Expr::num(20.0) }, at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)], op: fusion_core::Op::NewBody });
    app.after_doc_change();
    let v0 = vol(&app, 0);
    app.start_command("thread_on", 0.0);
    let uid = cmd_uid(&app);
    click_edge(&mut app, 0, |e| e.circle && e.b[2] > 19.0, false);
    let FeatureKind::ThreadOn { body, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert_ne!(body, 0, "the rim was taken");
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert!(vol(&app, 0) > v0, "a ridge is added on a boss: {} vs {v0}", vol(&app, 0));
}

/// Remove with selected bodies (Delete key path): one undoable step.
#[test]
fn removing_selected_bodies_is_one_undo_step() {
    let mut app = box_app(10.0, 10.0, 10.0);
    add_box(&mut app, [50.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
    let before = app.doc.clone();
    click_face(&mut app, 0, DVec3::Z, true);
    click_face(&mut app, 1, DVec3::Z, true);
    app.run_tool("remove", 0.0);
    assert_eq!(nbodies(&app), 0);
    assert!(app.selection.is_empty(), "removed bodies are not selected any more");
    app.do_undo();
    assert_eq!(app.doc, before);
    assert_eq!(nbodies(&app), 2);
}

/// Fillet on the edges of a pattern copy (a body made by a later step).
#[test]
fn fillet_on_a_pattern_copy() {
    let mut app = box_app(20.0, 20.0, 20.0);
    click_face(&mut app, 0, DVec3::Z, false);
    app.start_command("pattern", 0.0);
    app.finish_command(true);
    app.selection.clear();
    app.start_command("fillet", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 2, DVec3::Z, false);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert!(vol(&app, 2) < 8000.0 && close(vol(&app, 0), 8000.0, 1e-3) && close(vol(&app, 1), 8000.0, 1e-3));
}

/// Draft the side walls of a clicked box by 5°: the top shrinks, the bottom stays.
#[test]
fn draft_a_clicked_box() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("draft", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    let v = vol(&app, 0);
    assert!(v < 24000.0 && v > 20000.0, "{v}");
    let (lo, hi) = bounds(&app, 0);
    assert!(close(hi.z, 20.0, 1e-6) && close(lo.z, 0.0, 1e-6));
}

// ------------------------------------------------------------------ deeper face flows

/// The top of a drilled box is a face with a hole in it: pulling it up keeps the hole open.
#[test]
fn pulling_up_a_face_with_a_hole_keeps_the_hole() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let hole = cmd_uid(&app);
    edit(&mut app, hole, |k| if let FeatureKind::Hole { plane, at, .. } = k {
        let l = plane.to_local(DVec3::new(20.0, 15.0, 20.0));
        at[0] = Expr::num(l[0]);
        at[1] = Expr::num(l[1]);
    });
    app.finish_command(true);
    let a_hole = std::f64::consts::PI * 16.0;
    assert!(close(vol(&app, 0), 24000.0 - a_hole * 20.0, 5.0));
    app.start_command("press_pull", 0.0);
    // click the top away from the hole
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    set_distance(&mut app, uid, 5.0);
    assert_eq!(app.ev.error(uid), None);
    let want = 40.0 * 30.0 * 25.0 - a_hole * 25.0;
    assert!(close(vol(&app, 0), want, 6.0), "{} vs {want}: the hole must continue through the pulled part", vol(&app, 0));
}

/// A blind hole's floor faces up: pulling it up by the hole depth fills the hole.
#[test]
fn pulling_up_the_floor_of_a_blind_hole_makes_it_shallower() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("hole", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let hole = cmd_uid(&app);
    edit(&mut app, hole, |k| if let FeatureKind::Hole { plane, at, depth, dia, .. } = k {
        let l = plane.to_local(DVec3::new(20.0, 15.0, 20.0));
        at[0] = Expr::num(l[0]);
        at[1] = Expr::num(l[1]);
        *depth = Some(Expr::num(8.0));
        *dia = Expr::num(10.0);
    });
    assert_eq!(app.ev.error(hole), None);
    app.finish_command(true);
    let a = std::f64::consts::PI * 25.0;
    assert!(close(vol(&app, 0), 24000.0 - a * 8.0, 5.0), "{}", vol(&app, 0));
    // the floor at z = 12, facing up
    app.start_command("press_pull", 0.0);
    let model = app.pick_model();
    let mesh = model.bodies[0].mesh();
    let tri = mesh.triangles.iter().position(|t| {
        let [p, q, r] = mesh.tri(t);
        (q - p).cross(r - p).normalize_or_zero().dot(DVec3::Z) > 0.999 && (p.z - 12.0).abs() < 1e-6
    }).expect("the floor of the hole");
    let [p, q, r] = mesh.tri(&mesh.triangles[tri]);
    let at = (p + q + r) / 3.0;
    app.last_ray = (at + DVec3::Z * 50.0, -DVec3::Z);
    app.handle_click(Some(Hit::Body { slot: 0, tri, t: 50.0 }), false, 0.0);
    let uid = cmd_uid(&app);
    set_distance(&mut app, uid, 4.0);
    assert_eq!(app.ev.error(uid), None);
    assert!(close(vol(&app, 0), 24000.0 - a * 4.0, 5.0), "{} vs {}", vol(&app, 0), 24000.0 - a * 4.0);
}

/// Press-pull the round top of a cylinder up and down.
#[test]
fn press_pull_the_top_of_a_cylinder() {
    for dist in [5.0, -5.0] {
        let mut app = app();
        app.doc.insert(FeatureKind::Primitive { shape: fusion_core::doc::Shape::Cylinder { r: Expr::num(10.0), h: Expr::num(20.0) }, at: [Expr::num(0.0), Expr::num(0.0), Expr::num(0.0)], op: fusion_core::Op::NewBody });
        app.after_doc_change();
        let v0 = vol(&app, 0);
        app.start_command("press_pull", 0.0);
        click_face(&mut app, 0, DVec3::Z, false);
        let uid = cmd_uid(&app);
        set_distance(&mut app, uid, dist);
        assert_eq!(app.ev.error(uid), None, "{dist}");
        let (lo, hi) = bounds(&app, 0);
        assert!(close(hi.z, 20.0 + dist, 1e-3) && close(lo.z, 0.0, 1e-6), "{dist}: {lo:?} {hi:?}");
        assert!(close(vol(&app, 0), v0 * (20.0 + dist) / 20.0, v0 * 0.002), "{dist}: {} vs {}", vol(&app, 0), v0 * (20.0 + dist) / 20.0);
    }
}

/// Shell a box whose vertical edges were rounded.
#[test]
fn shell_after_a_fillet() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("fillet", 0.0);
    let f = cmd_uid(&app);
    for (x, y) in [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)] {
        click_edge(&mut app, 0, |e| !e.circle && e.a[0] == x && e.a[1] == y && e.b[0] == x && e.b[1] == y, false);
    }
    edit(&mut app, f, |k| if let FeatureKind::Fillet { radius, .. } = k { *radius = Expr::num(5.0) });
    assert_eq!(app.ev.error(f), None);
    app.finish_command(true);
    let v0 = vol(&app, 0);
    app.start_command("shell", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    assert_eq!(app.ev.error(uid), None);
    let v = vol(&app, 0);
    assert!(v < v0 * 0.5 && v > v0 * 0.15, "{v} of {v0}");
}

/// Chamfer the top edges after the vertical edges were rounded.
#[test]
fn chamfer_the_top_after_rounding_the_verticals() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("fillet", 0.0);
    let f = cmd_uid(&app);
    for (x, y) in [(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)] {
        click_edge(&mut app, 0, |e| !e.circle && e.a[0] == x && e.a[1] == y && e.b[0] == x && e.b[1] == y, false);
    }
    edit(&mut app, f, |k| if let FeatureKind::Fillet { radius, .. } = k { *radius = Expr::num(5.0) });
    app.finish_command(true);
    let v0 = vol(&app, 0);
    app.start_command("chamfer", 0.0);
    let uid = cmd_uid(&app);
    click_face(&mut app, 0, DVec3::Z, false);
    let FeatureKind::Chamfer { edges, .. } = app.doc.feature(uid).unwrap().kind.clone() else { panic!() };
    assert!(edges.len() >= 8, "four lines and four arcs around the rounded top: {}", edges.len());
    assert_eq!(app.ev.error(uid), None);
    let v = vol(&app, 0);
    assert!(v < v0 && v > v0 - 200.0, "{v} vs {v0}");
}

/// Move with a 90° turn about Z: a 40x30 box becomes 30x40 around the same centre.
#[test]
fn moving_with_a_quarter_turn() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("move", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| if let FeatureKind::Move { rotate, .. } = k { rotate[2] = Expr::num(90.0) });
    assert_eq!(app.ev.error(uid), None);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(hi.x - lo.x, 30.0, 1e-6) && close(hi.y - lo.y, 40.0, 1e-6), "{lo:?} {hi:?}");
    assert!(close((lo.x + hi.x) / 2.0, 20.0, 1e-6) && close((lo.y + hi.y) / 2.0, 15.0, 1e-6));
}

/// Non-uniform scale.
#[test]
fn non_uniform_scale() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("scale", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| if let FeatureKind::Scale { factor, uniform, .. } = k { *uniform = false; *factor = [Expr::num(1.0), Expr::num(2.0), Expr::num(0.5)]; });
    assert_eq!(app.ev.error(uid), None);
    let (lo, hi) = bounds(&app, 0);
    assert!(close(hi.y - lo.y, 60.0, 1e-6) && close(hi.z - lo.z, 10.0, 1e-6) && close(hi.x - lo.x, 40.0, 1e-6));
}

/// A step that uses a parameter: changing the parameter later moves the model; deleting the parameter gives an error, not a panic.
#[test]
fn a_press_pull_driven_by_a_parameter() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.doc.params.set("lift", "6");
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    edit(&mut app, uid, |k| if let FeatureKind::PressPull { distance, .. } = k { *distance = Expr("lift*2".into()) });
    assert_eq!(app.ev.error(uid), None);
    app.finish_command(true);
    assert!(close(bounds(&app, 0).1.z, 32.0, 1e-6));
    app.checkpoint();
    app.doc.params.set("lift", "-3");
    app.after_doc_change();
    assert!(close(bounds(&app, 0).1.z, 14.0, 1e-6));
    app.checkpoint();
    app.doc.params.items.remove("lift");
    app.after_doc_change();
    assert!(app.ev.error(uid).is_some());
    assert_eq!(nbodies(&app), 1, "the box itself stays");
    app.do_undo();
    assert_eq!(app.ev.error(uid), None);
}

// ------------------------------------------------------------------ OK without a pick, no-op edits

/// OK (or Enter) pressed before anything was picked: the history must not get a step that fails to build.
#[test]
fn ok_before_any_pick_does_not_commit_a_broken_step() {
    let mut broken = Vec::new();
    for tool in ["fillet", "chamfer", "move", "scale", "mirror", "pattern", "split", "draft", "combine", "place", "remove", "thread_on"] {
        let mut app = box_app(40.0, 30.0, 20.0);
        add_box(&mut app, [100.0, 0.0, 0.0], [10.0, 10.0, 10.0]);
        app.start_command(tool, 0.0);
        let uid = cmd_uid(&app);
        app.finish_command(true);
        if app.doc.feature(uid).is_some() {
            if let Some(e) = app.ev.error(uid) { broken.push(format!("{tool}: {e}")); }
        }
    }
    assert!(broken.is_empty(), "steps committed with an error: {broken:?}");
}

/// Opening a step for editing and pressing OK without changing anything must not add an empty undo step.
#[test]
fn ok_on_an_unchanged_edit_adds_no_empty_undo_step() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.start_command("press_pull", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    let uid = cmd_uid(&app);
    app.finish_command(true);
    let n = app.undo.len();
    app.edit_feature(uid, 0.0);
    app.finish_command(true);
    assert_eq!(app.undo.len(), n, "nothing changed, so there is nothing to undo");
}

/// Interference between two overlapping boxes is reported in a toast; the document is untouched.
#[test]
fn interference_check_reports_overlapping_bodies() {
    let mut app = box_app(20.0, 20.0, 20.0);
    add_box(&mut app, [10.0, 0.0, 0.0], [20.0, 20.0, 20.0]);
    let before = app.doc.clone();
    app.run_tool("interference", 0.0);
    let t = app.toast.clone().expect("a toast");
    assert!(t.2, "overlap is shown as a warning: {}", t.0);
    assert!(t.0.contains("4000"), "{}", t.0);
    assert_eq!(app.doc, before);
}

/// Measure: two clicks on the box give two points; a third starts again.
#[test]
fn measuring_two_points_on_a_box() {
    let mut app = box_app(40.0, 30.0, 20.0);
    app.viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 800.0));
    app.run_tool("measure", 0.0);
    click_face(&mut app, 0, DVec3::Z, false);
    click_face(&mut app, 0, -DVec3::Z, false);
    let Mode::Measure(pts) = &app.mode else { panic!() };
    assert_eq!(pts.len(), 2);
    assert!(close((pts[0].z - pts[1].z).abs(), 20.0, 1e-6), "{pts:?}");
    click_face(&mut app, 0, DVec3::X, false);
    let Mode::Measure(pts) = &app.mode else { panic!() };
    assert_eq!(pts.len(), 1);
}
