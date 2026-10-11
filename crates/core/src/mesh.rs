//! Треугольная сетка тела: измерения, обработка, экспорт, данные для отрисовки.

use glam::{DMat4, DVec3};
use std::collections::HashMap;

pub type V3 = [f64; 3];

/// Замкнутая треугольная сетка. Обход вершин против часовой стрелки, если смотреть снаружи.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<V3>,
    pub triangles: Vec<[u32; 3]>,
}

fn v(a: V3) -> DVec3 {
    DVec3::from(a)
}

impl Mesh {
    pub fn is_empty(&self) -> bool {
        self.triangles.is_empty()
    }

    pub fn tri(&self, t: &[u32; 3]) -> [DVec3; 3] {
        [v(self.vertices[t[0] as usize]), v(self.vertices[t[1] as usize]), v(self.vertices[t[2] as usize])]
    }

    pub fn tri_normal(&self, t: &[u32; 3]) -> DVec3 {
        let [a, b, c] = self.tri(t);
        (b - a).cross(c - a).normalize_or_zero()
    }

    /// Объём (сумма знаковых тетраэдров). Положителен для правильно ориентированной замкнутой сетки.
    pub fn volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = self.tri(t);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }

    pub fn area(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = self.tri(t);
                (b - a).cross(c - a).length() / 2.0
            })
            .sum()
    }

    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let mut it = self.vertices.iter().map(|&p| v(p));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    pub fn transformed(&self, m: &DMat4) -> Mesh {
        let mut out = Mesh {
            vertices: self.vertices.iter().map(|&p| m.transform_point3(v(p)).to_array()).collect(),
            triangles: self.triangles.clone(),
        };
        // зеркальное преобразование выворачивает треугольники — возвращаем обход
        if m.determinant() < 0.0 {
            for t in &mut out.triangles {
                t.swap(1, 2);
            }
        }
        out
    }

    pub fn flip(&mut self) {
        for t in &mut self.triangles {
            t.swap(1, 2);
        }
    }

    /// Добавить другую сетку (без слияния вершин).
    pub fn append(&mut self, o: &Mesh) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&o.vertices);
        self.triangles.extend(o.triangles.iter().map(|t| [t[0] + base, t[1] + base, t[2] + base]));
    }

    /// Слить совпадающие (в пределах допуска) вершины и выбросить вырожденные треугольники.
    pub fn weld(&mut self, tol: f64) {
        let q = |x: f64| (x / tol).round() as i64;
        let mut map: HashMap<[i64; 3], u32> = HashMap::with_capacity(self.vertices.len());
        let mut remap = Vec::with_capacity(self.vertices.len());
        let mut verts = Vec::with_capacity(self.vertices.len());
        for p in &self.vertices {
            let key = [q(p[0]), q(p[1]), q(p[2])];
            let id = *map.entry(key).or_insert_with(|| {
                // нормализуем -0.0, чтобы побитовое сравнение в булевых операциях совпадало
                verts.push(p.map(|c| if c == 0.0 { 0.0 } else { c }));
                (verts.len() - 1) as u32
            });
            remap.push(id);
        }
        self.vertices = verts;
        self.triangles = self
            .triangles
            .iter()
            .map(|t| t.map(|i| remap[i as usize]))
            .filter(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2])
            .collect();
    }

    /// Рёбра для обводки: граничные и те, где соседние грани расходятся больше `min_angle_deg`.
    pub fn sharp_edges(&self, min_angle_deg: f64) -> Vec<(u32, u32)> {
        let cos_limit = min_angle_deg.to_radians().cos();
        let mut edges: HashMap<(u32, u32), (DVec3, Option<DVec3>, u8)> = HashMap::new();
        for t in &self.triangles {
            let n = self.tri_normal(t);
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let e = edges.entry((a.min(b), a.max(b))).or_insert((n, None, 0));
                if e.2 == 1 {
                    e.1 = Some(n);
                }
                e.2 = e.2.saturating_add(1);
            }
        }
        let mut out: Vec<(u32, u32)> = edges
            .into_iter()
            .filter(|(_, (n0, n1, c))| *c != 2 || n1.is_none_or(|n1| n0.dot(n1) < cos_limit))
            .map(|(e, _)| e)
            .collect();
        out.sort_unstable();
        out
    }

    /// Нормали для гладкой заливки: на каждый угол треугольника — среднее нормалей соседних граней,
    /// которые отклоняются меньше `crease_deg` (острые рёбра остаются острыми).
    pub fn corner_normals(&self, crease_deg: f64) -> Vec<[DVec3; 3]> {
        let cos_limit = crease_deg.to_radians().cos();
        let face_n: Vec<DVec3> = self.triangles.iter().map(|t| self.tri_normal(t)).collect();
        // вес — площадь × угол при вершине
        let mut vert_faces: Vec<Vec<(u32, f64)>> = vec![Vec::new(); self.vertices.len()];
        for (fi, t) in self.triangles.iter().enumerate() {
            let p = self.tri(t);
            let area = (p[1] - p[0]).cross(p[2] - p[0]).length();
            for k in 0..3 {
                let (a, b) = ((p[(k + 1) % 3] - p[k]).normalize_or_zero(), (p[(k + 2) % 3] - p[k]).normalize_or_zero());
                let ang = a.dot(b).clamp(-1.0, 1.0).acos();
                vert_faces[t[k] as usize].push((fi as u32, ang * area.max(1e-30).sqrt()));
            }
        }
        self.triangles
            .iter()
            .enumerate()
            .map(|(fi, t)| {
                let n0 = face_n[fi];
                t.map(|vi| {
                    let mut s = DVec3::ZERO;
                    for &(f, w) in &vert_faces[vi as usize] {
                        let n = face_n[f as usize];
                        if n.dot(n0) >= cos_limit {
                            s += n * w;
                        }
                    }
                    let s = s.normalize_or_zero();
                    if s == DVec3::ZERO { n0 } else { s }
                })
            })
            .collect()
    }

    /// Группы треугольников, образующих одну плоскую грань (для выбора граней и чертежа на грани).
    pub fn planar_faces(&self) -> Vec<u32> {
        let n = self.triangles.len();
        let normals: Vec<DVec3> = self.triangles.iter().map(|t| self.tri_normal(t)).collect();
        let mut parent: Vec<u32> = (0..n as u32).collect();
        fn find(p: &mut [u32], i: u32) -> u32 {
            let mut r = i;
            while p[r as usize] != r {
                r = p[r as usize];
            }
            let mut j = i;
            while p[j as usize] != r {
                let nx = p[j as usize];
                p[j as usize] = r;
                j = nx;
            }
            r
        }
        let mut by_edge: HashMap<(u32, u32), u32> = HashMap::new();
        for (fi, t) in self.triangles.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = (a.min(b), a.max(b));
                if let Some(&other) = by_edge.get(&key) {
                    let (na, nb) = (normals[fi], normals[other as usize]);
                    let p0 = v(self.vertices[t[0] as usize]);
                    let q0 = v(self.vertices[self.triangles[other as usize][0] as usize]);
                    // одна плоскость: нормали совпадают и точки лежат в одной плоскости
                    if na.dot(nb) > 1.0 - 1e-9 && (q0 - p0).dot(na).abs() < 1e-6 {
                        let (ra, rb) = (find(&mut parent, fi as u32), find(&mut parent, other));
                        parent[ra as usize] = rb;
                    }
                } else {
                    by_edge.insert(key, fi as u32);
                }
            }
        }
        (0..n as u32).map(|i| find(&mut parent, i)).collect()
    }

    /// Пересечение луча с сеткой: (расстояние, индекс треугольника) ближайшего попадания.
    pub fn raycast(&self, origin: DVec3, dir: DVec3) -> Option<(f64, usize)> {
        let mut best: Option<(f64, usize)> = None;
        for (i, t) in self.triangles.iter().enumerate() {
            let [a, b, c] = self.tri(t);
            // Möller–Trumbore
            let e1 = b - a;
            let e2 = c - a;
            let p = dir.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-14 {
                continue;
            }
            let inv = 1.0 / det;
            let s = origin - a;
            let u = s.dot(p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let w = dir.dot(q) * inv;
            if w < 0.0 || u + w > 1.0 {
                continue;
            }
            let tt = e2.dot(q) * inv;
            if tt > 1e-9 && best.is_none_or(|(bt, _)| tt < bt) {
                best = Some((tt, i));
            }
        }
        best
    }

    /// Двоичный STL (стандарт для 3D-печати).
    pub fn write_stl_binary(meshes: &[&Mesh], out: &mut Vec<u8>) {
        let count: usize = meshes.iter().map(|m| m.triangles.len()).sum();
        out.extend_from_slice(&[0u8; 80]);
        out.extend_from_slice(&(count as u32).to_le_bytes());
        for m in meshes {
            for t in &m.triangles {
                let n = m.tri_normal(t);
                for c in n.to_array() {
                    out.extend_from_slice(&(c as f32).to_le_bytes());
                }
                for &i in t {
                    for c in m.vertices[i as usize] {
                        out.extend_from_slice(&(c as f32).to_le_bytes());
                    }
                }
                out.extend_from_slice(&[0, 0]);
            }
        }
    }

    /// Текстовый OBJ: каждое тело — отдельный объект.
    pub fn write_obj(meshes: &[(&str, &Mesh)]) -> String {
        use std::fmt::Write;
        let mut s = String::from("# Genix\n");
        let mut base = 1;
        for (name, m) in meshes {
            let _ = writeln!(s, "o {}", name.replace(char::is_whitespace, "_"));
            for p in &m.vertices {
                let _ = writeln!(s, "v {} {} {}", p[0], p[1], p[2]);
            }
            for t in &m.triangles {
                let _ = writeln!(s, "f {} {} {}", t[0] as usize + base, t[1] as usize + base, t[2] as usize + base);
            }
            base += m.vertices.len();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube() -> Mesh {
        crate::geom::box_mesh(DVec3::ZERO, DVec3::new(2.0, 3.0, 4.0))
    }

    #[test]
    fn measures() {
        let c = cube();
        assert!((c.volume() - 24.0).abs() < 1e-12);
        assert!((c.area() - 2.0 * (6.0 + 8.0 + 12.0)).abs() < 1e-12);
        assert_eq!(c.sharp_edges(30.0).len(), 12);
        let faces = c.planar_faces();
        let mut ids = faces.clone();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn mirror_keeps_volume_positive() {
        let m = cube().transformed(&DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
        assert!((m.volume() - 24.0).abs() < 1e-12);
    }

    #[test]
    fn raycast_hits_front_face() {
        let c = cube();
        let (t, _) = c.raycast(DVec3::new(1.0, 1.0, 10.0), DVec3::NEG_Z).unwrap();
        assert!((t - 6.0).abs() < 1e-12);
        assert!(c.raycast(DVec3::new(10.0, 10.0, 10.0), DVec3::NEG_Z).is_none());
    }

    #[test]
    fn stl_binary_size() {
        let c = cube();
        let mut out = Vec::new();
        Mesh::write_stl_binary(&[&c], &mut out);
        assert_eq!(out.len(), 84 + 50 * 12);
    }

    #[test]
    fn smooth_normals_keep_box_flat() {
        let c = cube();
        for (fi, ns) in c.corner_normals(30.0).iter().enumerate() {
            let n0 = c.tri_normal(&c.triangles[fi]);
            for n in ns {
                assert!((*n - n0).length() < 1e-9);
            }
        }
    }
}
