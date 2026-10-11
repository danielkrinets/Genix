//! Мини-рендер модели на процессоре: прозрачный фон, изометрия. Для превью проектов на главной и «Экспорт PNG без фона».

use crate::camera::Camera;
use fusion_core::Mesh;
use glam::DVec3;

pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

/// Нарисовать тела `meshes` с направления (yaw, pitch) в картинку w×h с прозрачным фоном.
pub fn render(meshes: &[&Mesh], yaw: f64, pitch: f64, w: usize, h: usize) -> Option<Rgba> {
    const SS: usize = 2; // сглаживание: рисуем вдвое крупнее и усредняем
    let (bw, bh) = (w * SS, h * SS);
    let mut cam = Camera::default();
    cam.yaw = yaw;
    cam.pitch = pitch;
    let (right, up, _) = cam.basis();
    let eye = cam.eye_dir();
    let light = (-right * 0.35 + up * 0.6 + eye * 0.75).normalize();
    let proj = |p: DVec3| (p.dot(right), p.dot(up), p.dot(eye));
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for m in meshes {
        for v in &m.vertices {
            let (x, y, _) = proj(DVec3::from(*v));
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x), hi.1.max(y));
        }
    }
    if lo.0 >= hi.0 || lo.1 >= hi.1 {
        return None;
    }
    let scale = ((bw as f64 * 0.88) / (hi.0 - lo.0)).min((bh as f64 * 0.88) / (hi.1 - lo.1));
    let (cx, cy) = ((lo.0 + hi.0) / 2.0, (lo.1 + hi.1) / 2.0);
    let to_screen = |p: DVec3| {
        let (x, y, z) = proj(p);
        (bw as f64 / 2.0 + (x - cx) * scale, bh as f64 / 2.0 - (y - cy) * scale, z)
    };
    let mut zbuf = vec![f64::NEG_INFINITY; bw * bh];
    let mut col = vec![[0f32; 3]; bw * bh];
    let base = [172.0f32, 182.0, 198.0];
    for m in meshes {
        for t in &m.triangles {
            let p = m.tri(t).map(|q| to_screen(q));
            let mut n = m.tri_normal(t);
            if n.dot(eye) < 0.0 {
                n = -n;
            }
            let k = 0.38 + 0.62 * n.dot(light).max(0.0) as f32;
            let c = [base[0] * k, base[1] * k, base[2] * k];
            let (minx, maxx) = (p.iter().map(|q| q.0).fold(f64::MAX, f64::min).floor().max(0.0) as usize, (p.iter().map(|q| q.0).fold(f64::MIN, f64::max).ceil() as usize).min(bw - 1));
            let (miny, maxy) = (p.iter().map(|q| q.1).fold(f64::MAX, f64::min).floor().max(0.0) as usize, (p.iter().map(|q| q.1).fold(f64::MIN, f64::max).ceil() as usize).min(bh - 1));
            let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
            if area.abs() < 1e-12 {
                continue;
            }
            for y in miny..=maxy {
                for x in minx..=maxx {
                    let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                    let w0 = ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
                    let w1 = ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                        continue;
                    }
                    let z = w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2;
                    let i = y * bw + x;
                    if z > zbuf[i] {
                        zbuf[i] = z;
                        col[i] = c;
                    }
                }
            }
        }
    }
    // усреднение SS×SS с учётом прозрачности
    let mut data = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (mut r, mut g, mut b, mut a) = (0f32, 0f32, 0f32, 0f32);
            for dy in 0..SS {
                for dx in 0..SS {
                    let i = (y * SS + dy) * bw + x * SS + dx;
                    if zbuf[i] > f64::NEG_INFINITY {
                        r += col[i][0];
                        g += col[i][1];
                        b += col[i][2];
                        a += 1.0;
                    }
                }
            }
            if a > 0.0 {
                let o = (y * w + x) * 4;
                data[o] = (r / a) as u8;
                data[o + 1] = (g / a) as u8;
                data[o + 2] = (b / a) as u8;
                data[o + 3] = (a / (SS * SS) as f32 * 255.0) as u8;
            }
        }
    }
    Some(Rgba { w, h, data })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fusion_core::geom::box_mesh;

    #[test]
    fn box_renders_with_transparent_background() {
        let m = box_mesh(DVec3::ZERO, DVec3::new(40.0, 20.0, 10.0));
        let img = render(&[&m], Camera::ISO.0, Camera::ISO.1, 120, 90).unwrap();
        assert_eq!(img.data.len(), 120 * 90 * 4);
        assert_eq!(img.data[3], 0, "угол картинки должен быть прозрачным");
        let centre = (45 * 120 + 60) * 4;
        assert!(img.data[centre + 3] > 200, "в центре — деталь");
    }
}
