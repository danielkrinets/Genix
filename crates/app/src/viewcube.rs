//! Куб видов в углу окна: щелчок по грани поворачивает камеру к стандартному виду.

use crate::camera::Camera;
use crate::i18n::tr;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke};
use glam::DVec3;

pub struct Face {
    pub normal: DVec3,
    pub right: DVec3,
    pub label: &'static str,
}

pub const FACES: [Face; 6] = [
    Face { normal: DVec3::Z, right: DVec3::X, label: "ВЕРХ" },
    Face { normal: DVec3::NEG_Z, right: DVec3::X, label: "НИЗ" },
    Face { normal: DVec3::NEG_Y, right: DVec3::X, label: "ПЕРЕД" },
    Face { normal: DVec3::Y, right: DVec3::NEG_X, label: "ЗАД" },
    Face { normal: DVec3::X, right: DVec3::Y, label: "ПРАВО" },
    Face { normal: DVec3::NEG_X, right: DVec3::NEG_Y, label: "ЛЕВО" },
];

pub enum CubeAction {
    None,
    View(DVec3, DVec3),
    Home,
}

/// Нарисовать куб в правом верхнем углу `view`. Возвращает выбранный вид.
pub fn show(ui: &mut egui::Ui, view: Rect, cam: &Camera, dark: bool) -> CubeAction {
    let size = 104.0;
    let area = Rect::from_min_size(Pos2::new(view.right() - size - 14.0, view.top() + 12.0), egui::vec2(size, size));
    let resp = ui.interact(area, ui.id().with("viewcube"), Sense::click());
    let painter = ui.painter_at(area.expand(2.0));
    let center = area.center();
    let (right, up, fwd) = cam.basis();
    let scale = size as f64 * 0.26;
    let proj = |v: DVec3| Pos2::new(center.x + (v.dot(right) * scale) as f32, center.y - (v.dot(up) * scale) as f32);
    let hover = resp.hover_pos();
    let mut hovered = None;
    // видимые грани, от дальних к ближним
    let mut faces: Vec<(usize, f64)> = FACES.iter().enumerate().map(|(i, f)| (i, f.normal.dot(-fwd))).filter(|(_, d)| *d > 0.02).collect();
    faces.sort_by(|a, b| a.1.total_cmp(&b.1));
    let polys: Vec<(usize, Vec<Pos2>, f64)> = faces
        .iter()
        .map(|&(i, d)| {
            let f = &FACES[i];
            let u = f.right;
            let v = f.normal.cross(u);
            let c = f.normal;
            let pts = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(a, b)| proj(c + u * a + v * b)).to_vec();
            (i, pts, d)
        })
        .collect();
    if let Some(h) = hover {
        for (i, pts, _) in polys.iter().rev() {
            if inside(h, pts) {
                hovered = Some(*i);
                break;
            }
        }
    }
    let (fill, text, edge) = if dark {
        (Color32::from_rgb(70, 76, 86), Color32::from_rgb(225, 228, 235), Color32::from_rgb(140, 150, 165))
    } else {
        (Color32::from_rgb(238, 240, 244), Color32::from_rgb(60, 66, 76), Color32::from_rgb(150, 158, 170))
    };
    for (i, pts, d) in &polys {
        let f = if Some(*i) == hovered { crate::theme::ACCENT.gamma_multiply(0.85) } else { fill };
        // чуть темнее грани, отвёрнутые от света
        let k = (0.82 + 0.18 * d) as f32;
        let f = Color32::from_rgb((f.r() as f32 * k) as u8, (f.g() as f32 * k) as u8, (f.b() as f32 * k) as u8);
        painter.add(Shape::convex_polygon(pts.clone(), f, Stroke::new(1.0_f32, edge)));
        let c = pts.iter().fold(egui::Vec2::ZERO, |a, p| a + p.to_vec2()) / 4.0;
        if *d > 0.25 {
            let alpha = ((*d - 0.25) / 0.5).clamp(0.0, 1.0) as f32;
            let tc = if Some(*i) == hovered { Color32::WHITE } else { text };
            painter.text(c.to_pos2(), Align2::CENTER_CENTER, tr(FACES[*i].label), FontId::proportional(9.0), tc.gamma_multiply(alpha));
        }
    }
    // оси в углу куба
    let o = DVec3::new(-1.25, -1.25, -1.25);
    for (dir, col) in [(DVec3::X, Color32::from_rgb(220, 60, 60)), (DVec3::Y, Color32::from_rgb(60, 170, 60)), (DVec3::Z, Color32::from_rgb(60, 110, 230))] {
        painter.line_segment([proj(o), proj(o + dir * 0.9)], Stroke::new(2.0_f32, col));
    }
    // кнопка «Домой»
    let home = Rect::from_min_size(Pos2::new(area.left() - 4.0, area.top() - 4.0), egui::vec2(20.0, 20.0));
    let hr = ui.interact(home, ui.id().with("home"), Sense::click()).on_hover_text(tr("Домой: изометрия и вписать модель"));
    let hp = ui.painter_at(home.expand(2.0));
    hp.text(home.center(), Align2::CENTER_CENTER, "🏠", FontId::proportional(14.0), if hr.hovered() { crate::theme::ACCENT } else { text });
    if hr.clicked() {
        return CubeAction::Home;
    }
    if resp.clicked() {
        if let Some(i) = hovered {
            return CubeAction::View(FACES[i].normal, FACES[i].right);
        }
    }
    CubeAction::None
}

fn inside(p: Pos2, poly: &[Pos2]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        let c = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if c.abs() < 1e-6 {
            continue;
        }
        if sign == 0.0 {
            sign = c.signum();
        } else if c.signum() != sign {
            return false;
        }
    }
    true
}
