//! Твёрдые тела и булевы операции (объединение, вычитание, пересечение) на библиотеке Manifold.

use crate::mesh::Mesh;
use glam::DMat4;
use manifold_csg::Manifold;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BoolOp {
    Union,
    Subtract,
    Intersect,
}

impl BoolOp {
    pub const ALL: [BoolOp; 3] = [BoolOp::Union, BoolOp::Subtract, BoolOp::Intersect];
    pub fn label(self) -> &'static str {
        match self {
            BoolOp::Union => "Объединить",
            BoolOp::Subtract => "Вырезать",
            BoolOp::Intersect => "Пересечь",
        }
    }
}

/// Замкнутое тело: точное представление для операций + сетка для показа и экспорта.
#[derive(Debug, Clone)]
pub struct Solid {
    manifold: Manifold,
    pub mesh: Mesh,
}

impl Solid {
    /// Тело из замкнутой сетки (вершины должны быть общими у соседних треугольников).
    pub fn from_mesh(m: &Mesh) -> Result<Solid, String> {
        let pos: Vec<f64> = m.vertices.iter().flatten().copied().collect();
        let idx: Vec<u64> = m.triangles.iter().flatten().map(|&i| i as u64).collect();
        let manifold = Manifold::from_mesh_f64(&pos, 3, &idx).map_err(|e| format!("тело не замкнуто: {e}"))?;
        Ok(Solid::from_manifold(manifold))
    }

    fn from_manifold(manifold: Manifold) -> Solid {
        let (props, n, tris) = manifold.to_mesh_f64();
        let mesh = Mesh {
            vertices: props.chunks(n).map(|p| [p[0], p[1], p[2]]).collect(),
            triangles: tris.chunks(3).map(|t| [t[0] as u32, t[1] as u32, t[2] as u32]).collect(),
        };
        Solid { manifold, mesh }
    }

    pub fn is_empty(&self) -> bool {
        self.manifold.is_empty()
    }

    pub fn volume(&self) -> f64 {
        self.manifold.volume()
    }

    pub fn boolean(&self, other: &Solid, op: BoolOp) -> Solid {
        let r = match op {
            BoolOp::Union => self.manifold.union(&other.manifold),
            BoolOp::Subtract => self.manifold.difference(&other.manifold),
            BoolOp::Intersect => self.manifold.intersection(&other.manifold),
        };
        Solid::from_manifold(r)
    }

    /// Объединить много тел за один проход (быстрее последовательных объединений).
    pub fn union_all(parts: &[Solid]) -> Solid {
        let ms: Vec<Manifold> = parts.iter().map(|s| s.manifold.clone()).collect();
        Solid::from_manifold(Manifold::batch_union(&ms))
    }

    /// Аффинное преобразование (зеркальное тоже — ориентация граней сохраняется).
    pub fn transformed(&self, m: &DMat4) -> Solid {
        let c = m.to_cols_array();
        let a = [c[0], c[1], c[2], c[4], c[5], c[6], c[8], c[9], c[10], c[12], c[13], c[14]];
        Solid::from_manifold(self.manifold.transform(&a))
    }

    /// Сжатие тела: все точки, отстоящие от границы меньше чем на радиус шара `ball`, убираются.
    pub fn eroded(&self, ball: &Solid) -> Solid {
        Solid::from_manifold(self.manifold.minkowski_difference(&ball.manifold))
    }

    /// Пересекаются ли габариты двух тел (быстрая проверка перед булевой операцией).
    pub fn bounds_overlap(&self, other: &Solid) -> bool {
        match (self.mesh.bounds(), other.mesh.bounds()) {
            (Some((alo, ahi)), Some((blo, bhi))) => {
                let eps = 1e-9;
                alo.x <= bhi.x + eps
                    && blo.x <= ahi.x + eps
                    && alo.y <= bhi.y + eps
                    && blo.y <= ahi.y + eps
                    && alo.z <= bhi.z + eps
                    && blo.z <= ahi.z + eps
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{box_mesh, cylinder_mesh};
    use glam::DVec3;

    fn s(m: Mesh) -> Solid {
        Solid::from_mesh(&m).unwrap()
    }

    #[test]
    fn hole_through_box() {
        let b = s(box_mesh(DVec3::ZERO, DVec3::new(40.0, 25.0, 15.0)));
        let cm = cylinder_mesh(DVec3::new(20.0, 12.5, -1.0), 5.0, 17.0);
        let cyl_area = cm.volume() / 17.0;
        let r = b.boolean(&s(cm), BoolOp::Subtract);
        assert!((r.volume() - (15000.0 - cyl_area * 15.0)).abs() < 1e-6);
        assert!((r.mesh.volume() - r.volume()).abs() < 1e-6);
    }

    #[test]
    fn plug_fills_hole_exactly() {
        // совпадающие стенки отверстия и пробки — классический вырожденный случай
        let plate = s(box_mesh(DVec3::ZERO, DVec3::new(40.0, 25.0, 10.0)));
        let plug = s(cylinder_mesh(DVec3::new(20.0, 12.5, 0.0), 5.0, 10.0));
        let filled = plate.boolean(&plug, BoolOp::Subtract).boolean(&plug, BoolOp::Union);
        assert!((filled.volume() - 10000.0).abs() < 1e-6);
        let cut = filled.boolean(&s(box_mesh(DVec3::splat(-1.0), DVec3::new(4.0, 4.0, 20.0))), BoolOp::Subtract);
        assert!((cut.volume() - 9840.0).abs() < 1e-6);
    }

    #[test]
    fn join_touching_faces_and_intersect() {
        let a = s(box_mesh(DVec3::ZERO, DVec3::new(10.0, 10.0, 10.0)));
        let b = s(box_mesh(DVec3::new(2.0, 2.0, 10.0), DVec3::new(8.0, 8.0, 15.0)));
        assert!((a.boolean(&b, BoolOp::Union).volume() - 1180.0).abs() < 1e-9);
        let c = s(box_mesh(DVec3::splat(5.0), DVec3::splat(15.0)));
        assert!((a.boolean(&c, BoolOp::Intersect).volume() - 125.0).abs() < 1e-9);
        let far = s(box_mesh(DVec3::splat(50.0), DVec3::splat(60.0)));
        assert!(!a.bounds_overlap(&far));
        assert!(a.boolean(&far, BoolOp::Intersect).is_empty());
    }

    #[test]
    fn mirror_transform_keeps_solid_valid() {
        let a = s(box_mesh(DVec3::new(1.0, 0.0, 0.0), DVec3::new(3.0, 2.0, 2.0)));
        let m = a.transformed(&DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0)));
        assert!((m.volume() - 8.0).abs() < 1e-9);
        assert!((m.mesh.bounds().unwrap().0.x + 3.0).abs() < 1e-9);
    }

    #[test]
    fn open_mesh_is_rejected() {
        let mut m = box_mesh(DVec3::ZERO, DVec3::ONE);
        m.triangles.pop();
        assert!(Solid::from_mesh(&m).is_err());
    }
}
