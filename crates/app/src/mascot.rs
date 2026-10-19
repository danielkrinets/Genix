//! Registration mascot: a blob that watches the cursor and turns away while a password is typed.
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};

/// Pupil offset inside an eye of radius `r`: follows `target` and never leaves the white.
fn pupil_offset(eye: Pos2, target: Pos2, r: f32) -> Vec2 {
    let d = target - eye;
    let len = d.length();
    if len < 1.0 { return Vec2::ZERO; }
    d / len * (len * 0.06).min(r * 0.45)
}

/// Draws the mascot centred on `center`; `shy` turns its face away (from the card on its right) and covers the eyes.
pub fn paint(ui: &egui::Ui, center: Pos2, size: f32, shy: bool) {
    let ctx = ui.ctx();
    let reduced = crate::theme::reduced_motion();
    let t = ctx.input(|i| i.time) as f32;
    let shy = ctx.animate_bool_with_time(egui::Id::new("gate-mascot-shy"), shy, if reduced { 0.0 } else { 0.28 });
    let pointer = ctx.input(|i| i.pointer.hover_pos()).unwrap_or(center + Vec2::new(size, 0.0));
    let bob = if reduced { 0.0 } else { (t * 2.0).sin() * size * 0.012 };
    let c = center + Vec2::new(0.0, bob);
    let p = ui.painter();
    let body = crate::theme::accent();
    let shade = body.lerp_to_gamma(Color32::BLACK, 0.25);
    let ink = Color32::from_rgb(22, 26, 36);
    let v = |x: f32, y: f32| Vec2::new(x * size, y * size);

    for side in [-1.0_f32, 1.0] {
        p.add(egui::Shape::ellipse_filled(c + v(0.2 * side, 0.55), v(0.14, 0.07), shade));
        p.add(egui::Shape::convex_polygon(vec![c + v(0.3 * side, -0.45), c + v(0.42 * side, -0.78), c + v(0.1 * side, -0.52)], shade, Stroke::NONE));
    }
    p.add(egui::Shape::ellipse_filled(c, v(0.5, 0.55), body));

    let turn = -0.10 * size * shy;
    let eye_r = 0.11 * size;
    let blink = if !reduced && t % 4.5 < 0.12 { 1.0 } else { 0.0 };
    let mut eyes = [Pos2::ZERO; 2];
    for (k, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
        let eye = c + Vec2::new(turn + 0.17 * size * (1.0 - 0.25 * shy) * side, -0.08 * size);
        eyes[k] = eye;
        let radius = Vec2::new(eye_r * (1.0 - 0.3 * shy), eye_r);
        p.add(egui::Shape::ellipse_filled(eye, radius, Color32::WHITE));
        let follow = pupil_offset(eye, pointer, eye_r);
        let look = follow + (Vec2::new(-eye_r * 0.5, 0.0) - follow) * shy;
        p.add(egui::Shape::circle_filled(eye + look, eye_r * 0.5, ink));
        if blink > 0.0 {
            let lid = egui::Rect::from_center_size(eye, radius * 2.0);
            p.with_clip_rect(lid).rect_filled(lid, 0.0, body);
        }
    }

    let mouth_x = turn + 0.0;
    let width = 0.1 * size * (1.0 - 0.4 * shy);
    let smile: Vec<Pos2> = (0..=12).map(|i| {
        let u = i as f32 / 6.0 - 1.0;
        c + Vec2::new(mouth_x + u * width, 0.15 * size + 0.05 * size * (1.0 - u * u) * (1.0 - shy))
    }).collect();
    p.add(egui::Shape::line(smile, Stroke::new(0.025 * size, ink)));

    for (k, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
        let rest = c + v(0.54 * side, 0.15);
        p.add(egui::Shape::circle_filled(rest.lerp(eyes[k], shy), 0.12 * size, shade));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pupils_follow_the_cursor_but_stay_inside_the_eye() {
        let eye = Pos2::new(100.0, 100.0);
        let r = 10.0;
        assert_eq!(pupil_offset(eye, eye, r), Vec2::ZERO);
        let far = pupil_offset(eye, Pos2::new(5000.0, 100.0), r);
        assert!(far.x > 0.0 && far.length() <= r * 0.45 + 1e-3);
        assert!(pupil_offset(eye, Pos2::new(100.0, 0.0), r).y < 0.0);
    }

    #[test]
    fn turning_away_changes_the_drawing() {
        let render = |shy: bool| {
            let ctx = egui::Context::default();
            let mut input = egui::RawInput::default();
            input.time = Some(1.0);
            ctx.run(input, |ctx| { egui::CentralPanel::default().show(ctx, |ui| paint(ui, Pos2::new(200.0, 200.0), 170.0, shy)); }).shapes
        };
        assert!(render(false).len() > 8);
        assert_ne!(render(false), render(true));
    }
}
