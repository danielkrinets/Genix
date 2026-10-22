//! Registration mascot: a slime drawn from sprites (made in ChatGPT, cut by `tools/mascot_atlas.py`) that watches the cursor,
//! blinks, breathes, hops when clicked and turns away while a password is typed.
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use std::sync::Arc;

/// Pupil offset inside an egg-shaped eye with room `(rx, ry)` for the pupil's centre: follows `target`, never leaves the white.
fn pupil_offset(eye: Pos2, target: Pos2, room: Vec2) -> Vec2 {
    let d = target - eye;
    let len = d.length();
    if len < 1.0 { return Vec2::ZERO; }
    // full deflection when the cursor is about 120 px away, proportional below that
    let m = (len / 120.0).min(1.0);
    Vec2::new(d.x / len * m * room.x, d.y / len * m * room.y)
}

struct Frame {
    tex: egui::TextureHandle,
    size: Vec2,
    /// the slime itself inside the frame's canvas (empty margins left out)
    body: Rect,
}

struct Sprites {
    body: Vec<Frame>,
    eyes: egui::TextureHandle,
    eyes_size: Vec2,
    pupil: egui::TextureHandle,
    pupil_size: Vec2,
    /// flat line, smile, big open smile
    mouths: Vec<(egui::TextureHandle, Vec2)>,
}

fn decode(ctx: &egui::Context, name: &str, bytes: &[u8]) -> (egui::TextureHandle, Vec2, Rect) {
    let img = image::load_from_memory(bytes).map(|i| i.to_rgba8()).unwrap_or_else(|_| image::RgbaImage::new(2, 2));
    let (w, h) = img.dimensions();
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if p.0[3] > 24 {
            x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x + 1); y1 = y1.max(y + 1);
        }
    }
    if x1 <= x0 { (x0, y0, x1, y1) = (0, 0, w, h); }
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    (ctx.load_texture(name, color, egui::TextureOptions::LINEAR), Vec2::new(w as f32, h as f32), Rect::from_min_max(Pos2::new(x0 as f32, y0 as f32), Pos2::new(x1 as f32, y1 as f32)))
}

fn sprites(ctx: &egui::Context) -> Arc<Sprites> {
    let id = egui::Id::new("mascot-sprites");
    if let Some(s) = ctx.data(|d| d.get_temp::<Arc<Sprites>>(id)) {
        return s;
    }
    macro_rules! body {
        ($($n:literal),*) => { vec![$({ let (tex, size, body) = decode(ctx, concat!("slime-body-", $n), include_bytes!(concat!("../assets/mascot/body_", $n, ".png"))); Frame { tex, size, body } }),*] };
    }
    let (eyes, eyes_size, _) = decode(ctx, "slime-eyes", include_bytes!("../assets/mascot/eyes.png"));
    let (pupil, pupil_size, _) = decode(ctx, "slime-pupil", include_bytes!("../assets/mascot/pupil.png"));
    let mouths = [&include_bytes!("../assets/mascot/mouth_0.png")[..], &include_bytes!("../assets/mascot/mouth_1.png")[..], &include_bytes!("../assets/mascot/mouth_2.png")[..]]
        .iter().enumerate().map(|(i, b)| { let (t, sz, _) = decode(ctx, &format!("slime-mouth-{i}"), b); (t, sz) }).collect();
    let s = Arc::new(Sprites { body: body!("00", "01", "02", "03", "04", "05", "06", "07", "08", "09"), eyes, eyes_size, pupil, pupil_size, mouths });
    ctx.data_mut(|d| d.insert_temp(id, s.clone()));
    s
}

/// Which frame of the hop is showing `t` seconds after the click (the hop is the ten frames of the sheet at 12 per second).
fn hop_frame(t: f32) -> Option<usize> {
    let i = (t * 12.0) as usize;
    (t >= 0.0 && i < 10).then_some(i)
}

/// Draws the slime sitting on `ground` (bottom centre). Everything scales with `width`.
/// `shy` turns its head away from the form on its right: the body narrows and leans, the eyes look aside.
pub fn paint(ui: &egui::Ui, ground: Pos2, width: f32, shy: bool) {
    let ctx = ui.ctx();
    let sp = sprites(ctx);
    let reduced = crate::theme::reduced_motion();
    let t = ctx.input(|i| i.time) as f32;
    let shy = ctx.animate_bool_with_time(egui::Id::new("gate-mascot-shy"), shy, if reduced { 0.0 } else { 0.28 });
    let pointer = ctx.input(|i| i.pointer.hover_pos()).unwrap_or(ground + Vec2::new(width, -width * 0.3));
    let p = ui.painter();

    // the resting dome is the 351 px wide frame 0; `k` turns sprite pixels into screen pixels
    let rest = sp.body[0].body;
    let k = width / rest.width();
    let hop_id = egui::Id::new("gate-mascot-hop");
    let rest_rect = Rect::from_min_max(Pos2::new(ground.x - width * 0.5, ground.y - rest.height() * k), Pos2::new(ground.x + width * 0.5, ground.y));
    if !reduced && ctx.input(|i| i.pointer.primary_pressed()) && ctx.input(|i| i.pointer.interact_pos()).is_some_and(|q| rest_rect.expand(8.0).contains(q)) {
        ctx.data_mut(|d| d.insert_temp(hop_id, t));
    }
    let hop = ctx.data(|d| d.get_temp::<f32>(hop_id)).and_then(|t0| hop_frame(t - t0));
    if hop.is_some() { ctx.request_repaint(); }
    let frame = &sp.body[hop.unwrap_or(0)];

    // breathing (idle only) and the turn away
    let breathe = if reduced || hop.is_some() { 0.0 } else { (t * 1.8).sin() };
    let sy = 1.0 + 0.03 * breathe;
    let sx = (1.0 - 0.015 * breathe) * (1.0 - 0.16 * shy);
    let lean = -0.06 * width * shy;
    // frames are cropped to a common canvas whose bottom centre is the ground point (4 px above the canvas edge)
    let canvas = frame.size * k;
    let origin = Pos2::new(ground.x - canvas.x * 0.5 * sx + lean, ground.y + 4.0 * k * sy - canvas.y * sy);
    let body_rect = Rect::from_min_size(origin, Vec2::new(canvas.x * sx, canvas.y * sy));
    p.add(egui::Shape::ellipse_filled(ground + Vec2::new(0.0, 2.0), Vec2::new(width * 0.5 * (1.0 + 0.05 * hop.map_or(0.0, |h| (h as f32 / 9.0 * std::f32::consts::PI).sin())), width * 0.045), Color32::from_black_alpha(46)));
    p.image(frame.tex.id(), body_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

    // the face sits at the same place of every frame: relative to the visible slime
    let vis = Rect::from_min_max(
        body_rect.min + Vec2::new(frame.body.min.x / frame.size.x * body_rect.width(), frame.body.min.y / frame.size.y * body_rect.height()),
        body_rect.min + Vec2::new(frame.body.max.x / frame.size.x * body_rect.width(), frame.body.max.y / frame.size.y * body_rect.height()),
    );
    let face_scale = (vis.width() / width).clamp(0.6, 1.25);
    let centre = Pos2::new(vis.center().x - 0.1 * width * shy, vis.top() + 0.5 * vis.height());
    let phase = (t % 4.5) / 0.22;
    let blink = if reduced || phase >= 1.0 { 0.0 } else { (std::f32::consts::PI * phase).sin() };
    let eye_scale = 0.62 * k * face_scale;
    let eyes_size = sp.eyes_size * eye_scale;
    let ex = 1.0 - 0.12 * shy;
    let eyes_rect = Rect::from_center_size(centre, Vec2::new(eyes_size.x * ex, eyes_size.y * (1.0 - 0.9 * blink)));
    p.image(sp.eyes.id(), eyes_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    // the two eyes of the sprite sit at 21.5 % and 78.5 % of its width; the pupil (a bit over half the white) has this much room
    let pupil_size = sp.pupil_size * eye_scale * 0.78;
    let room = Vec2::new(eyes_rect.width() * 0.07, eyes_rect.height() * 0.2);
    for fx in [0.215, 0.785] {
        let eye = Pos2::new(eyes_rect.left() + eyes_rect.width() * fx, eyes_rect.center().y);
        let follow = pupil_offset(eye, pointer, room);
        let look = follow + (Vec2::new(-room.x, room.y * 0.2) - follow) * shy;
        p.image(sp.pupil.id(), Rect::from_center_size(eye + look, Vec2::new(pupil_size.x * ex, pupil_size.y * (1.0 - 0.9 * blink))), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    // the mouth: a flat line while it is turned away, a big open smile mid-hop, a smile otherwise
    let (mouth, msize) = &sp.mouths[if shy > 0.5 { 0 } else if hop.is_some() { 2 } else { 1 }];
    let msize = *msize * eye_scale * 0.95;
    let mouth_rect = Rect::from_center_size(Pos2::new(centre.x, eyes_rect.bottom() + 0.07 * width + msize.y * 0.5), Vec2::new(msize.x * ex, msize.y));
    p.image(mouth.id(), mouth_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pupils_follow_the_cursor_but_stay_inside_the_eye() {
        let eye = Pos2::new(100.0, 100.0);
        let room = Vec2::new(10.0, 20.0);
        assert_eq!(pupil_offset(eye, eye, room), Vec2::ZERO);
        let far = pupil_offset(eye, Pos2::new(5000.0, 100.0), room);
        assert!((far.x - 10.0).abs() < 1e-3 && far.y.abs() < 1e-3);
        let up = pupil_offset(eye, Pos2::new(100.0, -5000.0), room);
        assert!(up.y < -19.0 && up.x.abs() < 1e-3);
        // never outside the ellipse of room
        for (dx, dy) in [(3000.0, 3000.0), (-80.0, 60.0), (10.0, -500.0)] {
            let o = pupil_offset(eye, Pos2::new(100.0 + dx, 100.0 + dy), room);
            assert!((o.x / room.x).powi(2) + (o.y / room.y).powi(2) <= 1.0 + 1e-4);
        }
    }

    #[test]
    fn the_hop_plays_ten_frames_and_then_stops() {
        assert_eq!(hop_frame(0.0), Some(0));
        assert_eq!(hop_frame(0.5), Some(6));
        assert_eq!(hop_frame(0.83), Some(9));
        assert_eq!(hop_frame(1.0), None);
        assert_eq!(hop_frame(-0.1), None);
    }

    #[test]
    fn every_drawn_point_is_finite_and_the_slime_is_drawn_from_textures() {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.time = Some(2.0);
        let out = ctx.run(input, |ctx| { egui::CentralPanel::default().show(ctx, |ui| paint(ui, Pos2::new(300.0, 500.0), 300.0, false)); });
        let meshes: Vec<&egui::Mesh> = out.shapes.iter().filter_map(|s| if let egui::Shape::Mesh(m) = &s.shape { Some(&**m) } else { None }).collect();
        assert!(meshes.len() >= 4, "body, eyes and two pupils are textured quads: {}", meshes.len());
        assert!(meshes.iter().flat_map(|m| &m.vertices).all(|v| v.pos.x.is_finite() && v.pos.y.is_finite()));
    }

    #[test]
    fn turning_away_changes_the_drawing_and_size_scales_it() {
        let render = |shy: bool, width: f32| {
            let ctx = egui::Context::default();
            let mut input = egui::RawInput::default();
            input.time = Some(1.0);
            ctx.run(input, |ctx| { egui::CentralPanel::default().show(ctx, |ui| paint(ui, Pos2::new(300.0, 500.0), width, shy)); }).shapes
        };
        assert!(render(false, 300.0).len() > 4);
        assert_ne!(render(false, 300.0), render(true, 300.0));
        assert_ne!(render(false, 300.0), render(false, 450.0));
    }
}
