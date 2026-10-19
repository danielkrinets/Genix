//! Subscription ticket in the spirit of the React Bits TearTicket: drag the stub off the perforation to activate.
//! Mock only: no server, payment or licence is involved.
use eframe::egui::{self, epaint::TextShape, Color32, Pos2, Rect, Shape, Stroke, Vec2};

const W: f32 = 460.0;
const H: f32 = 250.0;
const STUB: f32 = 150.0;
const TEAR: f32 = 0.52;

#[derive(Clone, Default)]
struct Tear {
    theta: f32,
    grab: Option<Pos2>,
    fall: Option<f64>,
}

/// Ticket number derived from the account e-mail, stable between launches.
pub fn number_for(email: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    email.hash(&mut h);
    format!("GX-{:06}", h.finish() % 1_000_000)
}

fn rounded(r: Rect, [nw, ne, se, sw]: [f32; 4]) -> Vec<Pos2> {
    let mut pts = Vec::new();
    for (corner, radius, from) in [(r.left_top(), nw, 180.0_f32), (r.right_top(), ne, 270.0), (r.right_bottom(), se, 0.0), (r.left_bottom(), sw, 90.0)] {
        let centre = corner + Vec2::new(if from == 180.0 || from == 90.0 { radius } else { -radius }, if from == 180.0 || from == 270.0 { radius } else { -radius });
        for i in 0..=6 {
            let a = (from + 90.0 * i as f32 / 6.0).to_radians();
            pts.push(centre + Vec2::new(a.cos(), a.sin()) * radius);
        }
    }
    pts
}

/// Draws the ticket and handles the tear. Returns true once, when the stub has come off and the ticket is activated.
pub fn show(ui: &mut egui::Ui, number: &str, activated: bool) -> bool {
    let k = (ui.available_width() / W).min(1.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(W, H) * k, egui::Sense::hover());
    let id = egui::Id::new("subscription-ticket");
    let now = ui.input(|i| i.time);
    let reduced = crate::theme::reduced_motion();
    let mut st: Tear = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let mut finished = false;
    let at = |x: f32, y: f32| rect.min + Vec2::new(x, y) * k;
    let hinge = Vec2::new(W - STUB, H);

    if !activated {
        if st.fall.is_none() {
            let resp = ui.interact(Rect::from_min_max(at(W - STUB, 0.0), at(W, H)), id.with("stub"), egui::Sense::drag());
            if resp.drag_started() { st.grab = ui.input(|i| i.pointer.interact_pos()); }
            if let (true, Some(g), Some(p)) = (resp.dragged(), st.grab, ui.input(|i| i.pointer.interact_pos())) {
                st.theta = (((p.x - g.x) + (p.y - g.y)).max(0.0) / k / STUB * 0.72).min(0.9);
            } else {
                st.grab = None;
                st.theta = (st.theta - ui.input(|i| i.stable_dt) * 3.0).max(0.0);
                if st.theta > 0.0 { ui.ctx().request_repaint(); }
            }
            if st.theta >= TEAR { st.fall = Some(now); }
        }
        if let Some(start) = st.fall {
            if reduced || now - start > 0.65 { finished = true; } else { ui.ctx().request_repaint(); }
        }
    }

    let (bg, accent, ink) = (crate::theme::surface(ui), crate::theme::accent(), ui.visuals().text_color());
    let paper = bg.lerp_to_gamma(accent, 0.14);
    let stub_paper = bg.lerp_to_gamma(accent, 0.24);
    let edge = Stroke::new(1.0_f32, accent.gamma_multiply(0.35));
    let p = ui.painter();
    let body = rounded(Rect::from_min_max(at(0.0, 0.0), at(W - STUB, H)), [16.0 * k, 0.0, 0.0, 16.0 * k]);
    let galley = |text: &str, size: f32, color: Color32, width: f32| p.layout(text.to_string(), egui::FontId::proportional(size), color, width * k);

    let stub_visible = !activated && !finished;
    let (angle, shift, alpha) = match st.fall {
        Some(start) => {
            let t = (now - start) as f32;
            (st.theta + 1.2 * t, Vec2::new(60.0 * t, 700.0 * t * t) * k, 1.0 - ((t - 0.15) / 0.45).clamp(0.0, 1.0))
        }
        None => (st.theta, Vec2::ZERO, 1.0),
    };
    let to_screen = |local: Vec2| -> Pos2 {
        let r = local - hinge;
        let (s, c) = angle.sin_cos();
        at(0.0, 0.0) + (hinge + Vec2::new(r.x * c - r.y * s, r.x * s + r.y * c)) * k + shift
    };
    let draw_stub = |p: &egui::Painter| {
        if !stub_visible || alpha <= 0.0 { return; }
        let local = rounded(Rect::from_min_max(Pos2::new(W - STUB, 0.0), Pos2::new(W, H)), [0.0, 16.0, 16.0, 0.0]);
        let pts: Vec<Pos2> = local.iter().map(|q| to_screen(q.to_vec2())).collect();
        p.add(Shape::convex_polygon(pts, stub_paper.gamma_multiply(alpha), Stroke::new(1.0_f32, edge.color.gamma_multiply(alpha))));
        for (text, size, y, color) in [(crate::i18n::tr("АКТИВАЦИЯ"), 12.0, 28.0, accent), (crate::i18n::tr("Потяните и оторвите, чтобы активировать"), 13.0, 60.0, ink)] {
            let g = galley(text, size, color.gamma_multiply(alpha), STUB - 32.0);
            p.add(Shape::Text(TextShape::new(to_screen(Vec2::new(W - STUB + 16.0, y)), g, color.gamma_multiply(alpha)).with_angle(angle)));
        }
    };

    if stub_visible && st.theta == 0.0 && st.fall.is_none() { draw_stub(p); }
    p.add(Shape::convex_polygon(body, paper, edge));
    for i in 0..12 {
        let y = 14.0 + (H - 28.0) * i as f32 / 11.0;
        p.circle_filled(at(W - STUB, y), 3.0 * k, bg);
    }
    if stub_visible && (st.theta > 0.0 || st.fall.is_some()) { draw_stub(p); }

    p.galley(at(26.0, 28.0), galley(crate::i18n::tr("Genix Server"), 24.0, ink, W - STUB - 52.0), ink);
    p.galley(at(26.0, 66.0), galley(crate::i18n::tr("Подписка на сервер · демонстрация"), 14.0, ink.gamma_multiply(0.7), W - STUB - 52.0), ink);
    p.galley(at(26.0, H - 74.0), galley(&number.to_string(), 18.0, accent, W - STUB - 52.0), accent);
    let state = if activated || finished { crate::i18n::tr("Активирована (демо)") } else { crate::i18n::tr("Ожидает активации (демо)") };
    p.galley(at(26.0, H - 44.0), galley(&state, 13.0, ink.gamma_multiply(0.7), W - STUB - 52.0), ink);

    if finished { st = Tear::default(); }
    ui.data_mut(|d| d.insert_temp(id, st));
    finished
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_is_stable_per_account_and_renders_both_states() {
        assert_eq!(number_for("a@b.c"), number_for("a@b.c"));
        assert_ne!(number_for("a@b.c"), number_for("x@y.z"));
        for activated in [false, true] {
            let ctx = egui::Context::default();
            let out = ctx.run(egui::RawInput::default(), |ctx| { egui::CentralPanel::default().show(ctx, |ui| assert!(!show(ui, "GX-000001", activated))); });
            assert!(out.shapes.len() > 5);
        }
    }
}
