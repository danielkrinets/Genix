//! Подготовка данных для GPU: сетки тел (с кэшем по телу), линии, сетка, заливки.

use crate::gpu::{FaceVertex, LineInst, TriVertex, NO_SLOT};
use crate::theme::linear;
use eframe::egui::Color32;
use fusion_core::doc::Body;
use fusion_core::sketch::{Plane, Region, P2};
use fusion_core::Mesh;
use glam::DVec3;
use std::collections::HashMap;
use std::sync::Arc;

/// Готовые к отрисовке данные одного тела (не зависят от его места в списке).
pub struct BodyRender {
    pub faces: Vec<FaceVertex>,
    pub edges: Vec<LineInst>,
}

impl BodyRender {
    fn build(mesh: &Mesh) -> BodyRender {
        let normals = mesh.corner_normals(30.0);
        let mut faces = Vec::with_capacity(mesh.triangles.len() * 3);
        for (t, ns) in mesh.triangles.iter().zip(&normals) {
            for k in 0..3 {
                let p = mesh.vertices[t[k] as usize];
                faces.push(FaceVertex { pos: [p[0] as f32, p[1] as f32, p[2] as f32], nrm: ns[k].as_vec3().to_array(), slot: 0 });
            }
        }
        let edges = mesh
            .sharp_edges(30.0)
            .into_iter()
            .map(|(a, b)| {
                let (pa, pb) = (mesh.vertices[a as usize], mesh.vertices[b as usize]);
                LineInst {
                    p0: [pa[0] as f32, pa[1] as f32, pa[2] as f32],
                    p1: [pb[0] as f32, pb[1] as f32, pb[2] as f32],
                    color: [0.0; 4],
                    width: 1.3,
                    slot: 0,
                }
            })
            .collect();
        BodyRender { faces, edges }
    }
}

/// Кэш по телу: пересчитываются только изменившиеся тела.
#[derive(Default)]
pub struct RenderCache {
    map: HashMap<usize, (Arc<fusion_core::csg::Solid>, Arc<BodyRender>)>,
}

impl RenderCache {
    pub fn get(&mut self, body: &Body) -> Arc<BodyRender> {
        let key = Arc::as_ptr(&body.solid) as usize;
        if let Some((_, r)) = self.map.get(&key) {
            return r.clone();
        }
        let r = Arc::new(BodyRender::build(body.mesh()));
        self.map.insert(key, (body.solid.clone(), r.clone()));
        r
    }

    /// Собрать буферы для всех тел (слот = индекс тела) и выбросить устаревшие записи.
    pub fn assemble(&mut self, bodies: &[Body], edge_width: f32) -> (Vec<FaceVertex>, Vec<LineInst>) {
        let mut faces = Vec::new();
        let mut edges = Vec::new();
        let mut alive = std::collections::HashSet::new();
        for (slot, b) in bodies.iter().enumerate() {
            alive.insert(Arc::as_ptr(&b.solid) as usize);
            let r = self.get(b);
            faces.extend(r.faces.iter().map(|v| FaceVertex { slot: slot as u32, ..*v }));
            edges.extend(r.edges.iter().map(|e| LineInst { slot: slot as u32, width: e.width * edge_width / 1.3, ..*e }));
        }
        // держим и тела из предыдущих состояний (при откате/отмене они снова пригодятся), но не бесконечно
        if self.map.len() > alive.len() + 256 {
            self.map.retain(|k, _| alive.contains(k));
        }
        (faces, edges)
    }
}

pub fn v3(p: DVec3) -> [f32; 3] {
    p.as_vec3().to_array()
}

pub fn line(p0: DVec3, p1: DVec3, color: Color32, width: f32) -> LineInst {
    LineInst { p0: v3(p0), p1: v3(p1), color: linear(color), width, slot: NO_SLOT }
}

pub fn polyline(out: &mut Vec<LineInst>, pts: &[DVec3], color: Color32, width: f32, closed: bool) {
    for w in pts.windows(2) {
        out.push(line(w[0], w[1], color, width));
    }
    if closed && pts.len() > 2 {
        out.push(line(*pts.last().unwrap(), pts[0], color, width));
    }
}

/// Штриховая линия (для вспомогательной геометрии).
pub fn dashed(out: &mut Vec<LineInst>, a: DVec3, b: DVec3, dash: f64, color: Color32, width: f32) {
    let len = a.distance(b);
    if len < 1e-12 || dash <= 0.0 {
        return;
    }
    let n = ((len / dash) as usize).clamp(1, 400);
    for i in (0..n).step_by(2) {
        let t0 = i as f64 / n as f64;
        let t1 = ((i + 1) as f64 / n as f64).min(1.0);
        out.push(line(a.lerp(b, t0), a.lerp(b, t1), color, width));
    }
}

/// «Красивый» шаг сетки (1-2-5) около `approx`.
pub fn nice_step(approx: f64) -> f64 {
    let e = 10f64.powf(approx.max(1e-6).log10().floor());
    let m = approx / e;
    let k = if m < 1.5 {
        1.0
    } else if m < 3.5 {
        2.0
    } else if m < 7.5 {
        5.0
    } else {
        10.0
    };
    k * e
}

/// Десятичный шаг: наименьшая степень десяти, при которой клетка не уже `min_px` пикселей.
pub fn decade_step(wpp: f64, min_px: f64) -> f64 {
    10f64.powf((wpp.max(1e-9) * min_px).log10().ceil())
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Сетка, плавно «растущая» вместе с камерой: уровни по степеням десяти, каждый проявляется,
/// когда его клетки становятся достаточно широкими, и остаётся, пока не уйдёт за пределы экрана.
pub fn grid_adaptive(out: &mut Vec<LineInst>, plane: &Plane, center: P2, wpp: f64, radius: f64, minor: Color32, major: Color32) {
    let radius = radius * 0.7; // сетка конечная и растворяется к краям, а не уходит в бесконечность
    let e0 = (wpp.max(1e-9) * 9.0).log10().ceil() as i32;
    for e in e0..e0 + 4 {
        let step = 10f64.powi(e);
        let px = step / wpp;
        let fade = smooth(9.0, 32.0, px) as f32;
        if fade < 0.02 || px > 4000.0 {
            continue;
        }
        let half = ((radius / step) as i64).clamp(4, 220);
        let coarser = px < 400.0; // следующий уровень ещё виден — его линии рисует он
        grid_level(out, plane, center, step, half, minor.gamma_multiply(fade), major.gamma_multiply(fade), coarser);
    }
}

fn grid_level(out: &mut Vec<LineInst>, plane: &Plane, center: P2, step: f64, half_cells: i64, minor: Color32, major: Color32, skip_decades: bool) {
    let cx = (center[0] / step).round() as i64;
    let cy = (center[1] / step).round() as i64;
    let (x0, x1) = ((cx - half_cells) as f64 * step, (cx + half_cells) as f64 * step);
    let (y0, y1) = ((cy - half_cells) as f64 * step, (cy + half_cells) as f64 * step);
    let radius = half_cells as f64 * step;
    let seg = |out: &mut Vec<LineInst>, a: P2, b: P2, col: Color32| {
        const K: usize = 6;
        for k in 0..K {
            let (t0, t1) = (k as f64 / K as f64, (k + 1) as f64 / K as f64);
            let mid = [a[0] + (b[0] - a[0]) * (t0 + t1) / 2.0, a[1] + (b[1] - a[1]) * (t0 + t1) / 2.0];
            let d = ((mid[0] - center[0]).powi(2) + (mid[1] - center[1]).powi(2)).sqrt() / radius;
            let alpha = (1.0 - d).clamp(0.0, 1.0).powf(1.7) as f32;
            if alpha < 0.03 {
                continue;
            }
            let p0 = [a[0] + (b[0] - a[0]) * t0, a[1] + (b[1] - a[1]) * t0];
            let p1 = [a[0] + (b[0] - a[0]) * t1, a[1] + (b[1] - a[1]) * t1];
            out.push(line(plane.to_world(p0), plane.to_world(p1), col.gamma_multiply(alpha), 1.0));
        }
    };
    for i in -half_cells..=half_cells {
        let (gx, gy) = (cx + i, cy + i);
        let (x, y) = (gx as f64 * step, gy as f64 * step);
        if gx != 0 && !(skip_decades && gx % 10 == 0) {
            seg(out, [x, y0], [x, y1], if gx % 5 == 0 { major } else { minor });
        }
        if gy != 0 && !(skip_decades && gy % 10 == 0) {
            seg(out, [x0, y], [x1, y], if gy % 5 == 0 { major } else { minor });
        }
    }
}

/// Сетка на плоскости вокруг точки (в координатах плоскости).
pub fn grid(out: &mut Vec<LineInst>, plane: &Plane, center: P2, step: f64, half_cells: i64, minor: Color32, major: Color32) {
    let major_every = 5;
    let cx = (center[0] / step).round() as i64;
    let cy = (center[1] / step).round() as i64;
    let (x0, x1) = ((cx - half_cells) as f64 * step, (cx + half_cells) as f64 * step);
    let (y0, y1) = ((cy - half_cells) as f64 * step, (cy + half_cells) as f64 * step);
    let radius = half_cells as f64 * step;
    // линия режется на куски; чем дальше от центра обзора, тем бледнее — сетка растворяется вдали
    let seg = |out: &mut Vec<LineInst>, a: P2, b: P2, col: Color32| {
        const K: usize = 10;
        for k in 0..K {
            let (t0, t1) = (k as f64 / K as f64, (k + 1) as f64 / K as f64);
            let mid = [a[0] + (b[0] - a[0]) * (t0 + t1) / 2.0, a[1] + (b[1] - a[1]) * (t0 + t1) / 2.0];
            let d = ((mid[0] - center[0]).powi(2) + (mid[1] - center[1]).powi(2)).sqrt() / radius;
            let alpha = (1.0 - d).clamp(0.0, 1.0).powf(1.4) as f32;
            if alpha < 0.03 {
                continue;
            }
            let p0 = [a[0] + (b[0] - a[0]) * t0, a[1] + (b[1] - a[1]) * t0];
            let p1 = [a[0] + (b[0] - a[0]) * t1, a[1] + (b[1] - a[1]) * t1];
            out.push(line(plane.to_world(p0), plane.to_world(p1), col.gamma_multiply(alpha), 1.0));
        }
    };
    for i in -half_cells..=half_cells {
        let (gx, gy) = (cx + i, cy + i);
        let (x, y) = (gx as f64 * step, gy as f64 * step);
        // линии через начало координат рисуются осями отдельно
        if gx != 0 {
            seg(out, [x, y0], [x, y1], if gx % major_every == 0 { major } else { minor });
        }
        if gy != 0 {
            seg(out, [x0, y], [x1, y], if gy % major_every == 0 { major } else { minor });
        }
    }
}

/// Числа вдоль осей X и Y плоскости через каждые 5 шагов сетки.
pub fn axis_labels(painter: &eframe::egui::Painter, cam: &crate::camera::Camera, rect: eframe::egui::Rect, plane: &Plane, major: f64, color: Color32) {
    let font = eframe::egui::FontId::proportional(10.5);
    // числа не рисуем, если они наползают друг на друга (вид вдоль оси, сильное отдаление): минимум 46 px между подписями
    let mut drawn: [Vec<eframe::egui::Pos2>; 2] = [Vec::new(), Vec::new()];
    let mut shown = 0;
    let mut ks: Vec<i32> = (-40..=40).filter(|k| *k != 0).collect();
    ks.sort_by_key(|k| k.abs());
    for k in ks {
        let v = k as f64 * major;
        for (axis, (p, off)) in [([v, 0.0], eframe::egui::vec2(0.0, 9.0)), ([0.0, v], eframe::egui::vec2(-11.0, 0.0))].into_iter().enumerate() {
            let Some(s) = cam.project(rect, plane.to_world(p)) else { continue };
            let q = s + off;
            if !rect.shrink(30.0).contains(q) || shown > 60 {
                continue;
            }
            if drawn[axis].iter().any(|l| l.distance(q) < 46.0) {
                continue;
            }
            drawn[axis].push(q);
            painter.text(q, eframe::egui::Align2::CENTER_CENTER, fusion_core::expr::format_num(v), font.clone(), color);
            shown += 1;
        }
    }
}

/// Заливка области чертежа полупрозрачным цветом.
pub fn fill_region(out: &mut Vec<TriVertex>, plane: &Plane, region: &Region, color: Color32) {
    let (verts, tris) = region.triangulate();
    let c = linear(color);
    for t in tris {
        for i in t {
            out.push(TriVertex { pos: v3(plane.to_world(verts[i])), color: c });
        }
    }
}

/// Полупрозрачный прямоугольник на плоскости.
pub fn fill_quad(out: &mut Vec<TriVertex>, plane: &Plane, a: P2, b: P2, color: Color32) {
    let c = linear(color);
    let p = [[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]].map(|q| v3(plane.to_world(q)));
    for i in [0, 1, 2, 0, 2, 3] {
        out.push(TriVertex { pos: p[i], color: c });
    }
}

pub fn fill_tris(out: &mut Vec<TriVertex>, mesh: &Mesh, tris: impl Iterator<Item = usize>, color: Color32) {
    let c = linear(color);
    for t in tris {
        for &i in &mesh.triangles[t] {
            let p = mesh.vertices[i as usize];
            out.push(TriVertex { pos: [p[0] as f32, p[1] as f32, p[2] as f32], color: c });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_steps() {
        assert_eq!(nice_step(1.2), 1.0);
        assert_eq!(nice_step(2.6), 2.0);
        assert_eq!(nice_step(40.0), 50.0);
        assert!((nice_step(0.07) - 0.05).abs() < 1e-12);
    }
}
