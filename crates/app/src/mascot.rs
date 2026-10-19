//! Registration mascot: a slime that watches the cursor and turns away while a password is typed.
use eframe::egui::{self, Color32, Pos2, Stroke, Vec2};

/// Pupil offset inside an eye of radius `r`: follows `target` and never leaves the white.
fn pupil_offset(eye: Pos2, target: Pos2, r: f32) -> Vec2 {
    let d = target - eye;
    let len = d.length();
    if len < 1.0 { return Vec2::ZERO; }
    d / len * (len * 0.06).min(r * 0.45)
}

/// Draws the slime sitting on `ground` (bottom centre). Everything scales with `width`.
/// `shy` leans it away from the form on its right, looks away and closes the eyes.
pub fn paint(ui: &egui::Ui, ground: Pos2, width: f32, shy: bool) {
    let ctx = ui.ctx();
    let reduced = crate::theme::reduced_motion();
    let t = ctx.input(|i| i.time) as f32;
    let shy = ctx.animate_bool_with_time(egui::Id::new("gate-mascot-shy"), shy, if reduced { 0.0 } else { 0.28 });
    let pointer = ctx.input(|i| i.pointer.hover_pos()).unwrap_or(ground + Vec2::new(width, -width * 0.3));
    let p = ui.painter();
    let height = width * 0.62;
    let wobble = |a: f32| if reduced { 0.0 } else { (t * 1.6 + a * 3.0).sin() * 0.015 + (t * 2.3 - a * 2.0).cos() * 0.01 };
    let breathe = if reduced { 1.0 } else { 1.0 + (t * 1.8).sin() * 0.025 };
    let lean = -0.10 * width * shy;
    let body = crate::theme::accent();
    let shade = body.lerp_to_gamma(Color32::BLACK, 0.3);
    let ink = Color32::from_rgb(22, 26, 36);

    p.add(egui::Shape::ellipse_filled(ground, Vec2::new(width * 0.62, height * 0.07), shade.gamma_multiply(0.45)));
    let dome: Vec<Pos2> = (0..=40).map(|k| {
        let a = std::f32::consts::PI * k as f32 / 40.0;
        let s = a.sin();
        Pos2::new(ground.x + 0.5 * width * a.cos() * (1.0 + wobble(a)) + lean * s, ground.y - height * breathe * s.powf(0.75) * (1.0 + wobble(a + 1.0)))
    }).collect();
    p.add(egui::Shape::convex_polygon(dome, body.gamma_multiply(0.94), Stroke::new(width * 0.012, shade)));
    p.add(egui::Shape::ellipse_filled(Pos2::new(ground.x - 0.18 * width + lean * 0.6, ground.y - 0.62 * height), Vec2::new(0.1 * width, 0.06 * height), Color32::from_white_alpha(90)));

    let eye_y = ground.y - 0.45 * height;
    let eye_r = 0.085 * width;
    let blink = !reduced && t % 4.5 < 0.12;
    for side in [-1.0_f32, 1.0] {
        let eye = Pos2::new(ground.x + lean * 0.9 + 0.16 * width * (1.0 - 0.2 * shy) * side, eye_y);
        let radius = Vec2::new(eye_r * (1.0 - 0.3 * shy), eye_r);
        p.add(egui::Shape::ellipse_filled(eye, radius, Color32::WHITE));
        let follow = pupil_offset(eye, pointer, eye_r);
        let look = follow + (Vec2::new(-eye_r * 0.5, 0.0) - follow) * shy;
        p.add(egui::Shape::circle_filled(eye + look, eye_r * 0.5, ink));
        let closed = if blink { 1.0 } else { shy };
        if closed > 0.0 {
            let lid = egui::Rect::from_min_size(eye - radius, Vec2::new(radius.x * 2.0, radius.y * 2.0 * closed));
            p.with_clip_rect(egui::Rect::from_center_size(eye, radius * 2.0)).rect_filled(lid, 0.0, body);
        }
    }

    let mouth: Vec<Pos2> = (0..=12).map(|i| {
        let u = i as f32 / 6.0 - 1.0;
        Pos2::new(ground.x + lean * 0.9 + u * 0.07 * width, eye_y + 0.14 * height + 0.04 * height * (1.0 - u * u) * (1.0 - shy))
    }).collect();
    p.add(egui::Shape::line(mouth, Stroke::new(0.02 * width, ink)));
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
    fn turning_away_changes_the_drawing_and_size_scales_it() {
        let render = |shy: bool, width: f32| {
            let ctx = egui::Context::default();
            let mut input = egui::RawInput::default();
            input.time = Some(1.0);
            ctx.run(input, |ctx| { egui::CentralPanel::default().show(ctx, |ui| paint(ui, Pos2::new(300.0, 500.0), width, shy)); }).shapes
        };
        assert!(render(false, 300.0).len() > 8);
        assert_ne!(render(false, 300.0), render(true, 300.0));
        assert_ne!(render(false, 300.0), render(false, 450.0));
    }
}
