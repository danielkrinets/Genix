//! Построение тел: выдавливание и вращение областей чертежа, примитивы.

use crate::mesh::Mesh;
use crate::sketch::{arc_segments, Plane, Region, P2};
use glam::{DQuat, DVec3};

/// Выдавить область вдоль нормали плоскости от `z0` до `z1` (в мм от плоскости).
pub fn extrude_region(plane: &Plane, region: &Region, z0: f64, z1: f64) -> Option<Mesh> {
    if (z1 - z0).abs() < 1e-9 {
        return None;
    }
    let (lo, hi) = (z0.min(z1), z0.max(z1));
    let (verts2, caps) = region.triangulate();
    if caps.is_empty() {
        return None;
    }
    let n = verts2.len() as u32;
    let normal = plane.normal();
    let mut m = Mesh::default();
    for &z in &[lo, hi] {
        for p in &verts2 {
            m.vertices.push((plane.to_world(*p) + normal * z).to_array());
        }
    }
    for t in &caps {
        let [a, b, c] = t.map(|i| i as u32);
        m.triangles.push([a, c, b]); // низ смотрит против нормали
        m.triangles.push([a + n, b + n, c + n]); // верх — по нормали
    }
    // боковые стенки по каждому контуру (внешний против часовой, отверстия по часовой)
    let mut start = 0u32;
    let loops = std::iter::once(region.outer.len()).chain(region.holes.iter().map(|h| h.len()));
    for len in loops {
        let len = len as u32;
        for i in 0..len {
            let (a, b) = (start + i, start + (i + 1) % len);
            m.triangles.push([a, b, b + n]);
            m.triangles.push([a, b + n, a + n]);
        }
        start += len;
    }
    m.weld(1e-9);
    Some(m)
}

/// Повернуть область вокруг оси, лежащей в плоскости чертежа (точка и направление в координатах чертежа).
pub fn revolve_region(plane: &Plane, region: &Region, axis_p: P2, axis_d: P2, angle_deg: f64) -> Result<Mesh, String> {
    let full = angle_deg.abs() >= 360.0 - 1e-9;
    let angle = angle_deg.clamp(-360.0, 360.0).to_radians();
    if angle.abs() < 1e-9 {
        return Err("нулевой угол".into());
    }
    let ap = plane.to_world(axis_p);
    let ad = (plane.to_world([axis_p[0] + axis_d[0], axis_p[1] + axis_d[1]]) - ap).normalize_or_zero();
    if ad == DVec3::ZERO {
        return Err("ось не задана".into());
    }
    // профиль не должен пересекать ось: все точки по одну сторону
    let side = |p: &P2| {
        let d = [p[0] - axis_p[0], p[1] - axis_p[1]];
        axis_d[0] * d[1] - axis_d[1] * d[0]
    };
    let all = region.outer.iter().chain(region.holes.iter().flatten());
    let (mut pos, mut neg) = (false, false);
    for p in all {
        let s = side(p);
        let tol = 1e-7 * (1.0 + p[0].abs() + p[1].abs());
        pos |= s > tol;
        neg |= s < -tol;
    }
    if pos && neg {
        return Err("профиль пересекает ось вращения".into());
    }

    let steps = arc_segments(angle);
    let rings = if full { steps } else { steps + 1 };
    let (verts2, caps) = region.triangulate();
    let n = verts2.len() as u32;
    let rot = |k: usize, p: DVec3| {
        let q = DQuat::from_axis_angle(ad, angle * k as f64 / steps as f64);
        let r = ap + q * (p - ap);
        // точки на оси остаются точно на месте — это важно для сшивки
        if (p - ap).cross(ad).length() < 1e-9 { p } else { r }
    };
    let base: Vec<DVec3> = verts2.iter().map(|p| plane.to_world(*p)).collect();
    let mut m = Mesh::default();
    for k in 0..rings {
        for p in &base {
            m.vertices.push(rot(k, *p).to_array());
        }
    }
    let idx = |k: usize, i: u32| (k % rings) as u32 * n + i;
    let mut start = 0u32;
    let loops = std::iter::once(region.outer.len()).chain(region.holes.iter().map(|h| h.len()));
    for len in loops {
        let len = len as u32;
        for k in 0..steps {
            for i in 0..len {
                let (a, b) = (start + i, start + (i + 1) % len);
                m.triangles.push([idx(k, a), idx(k + 1, b), idx(k, b)]);
                m.triangles.push([idx(k, a), idx(k + 1, a), idx(k + 1, b)]);
            }
        }
        start += len;
    }
    if !full {
        for t in &caps {
            let [a, b, c] = t.map(|i| i as u32);
            m.triangles.push([idx(0, a), idx(0, b), idx(0, c)]);
            m.triangles.push([idx(steps, a), idx(steps, c), idx(steps, b)]);
        }
    }
    m.weld(1e-9);
    if m.triangles.is_empty() {
        return Err("пустой результат".into());
    }
    // ориентацию проще всего проверить по знаку объёма
    if m.volume() < 0.0 {
        m.flip();
    }
    Ok(m)
}

pub fn box_mesh(lo: DVec3, hi: DVec3) -> Mesh {
    let (x0, y0, z0, x1, y1, z1) = (lo.x, lo.y, lo.z, hi.x, hi.y, hi.z);
    Mesh {
        vertices: vec![
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ],
        triangles: vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ],
    }
}

/// Цилиндр по оси Z: центр основания, радиус, высота.
pub fn cylinder_mesh(base: DVec3, r: f64, h: f64) -> Mesh {
    let n = arc_segments(std::f64::consts::TAU);
    let outer: Vec<P2> = (0..n)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            [base.x + r * t.cos(), base.y + r * t.sin()]
        })
        .collect();
    let mut plane = Plane::XY;
    plane.origin = [0.0, 0.0, base.z];
    extrude_region(&plane, &Region { outer, holes: vec![], ids: vec![] }, 0.0, h).unwrap_or_default()
}

pub fn sphere_mesh(c: DVec3, r: f64) -> Mesh {
    let n_lon = arc_segments(std::f64::consts::TAU);
    let n_lat = n_lon / 2;
    let mut m = Mesh::default();
    m.vertices.push((c - DVec3::Z * r).to_array());
    for i in 1..n_lat {
        let phi = -std::f64::consts::FRAC_PI_2 + std::f64::consts::PI * i as f64 / n_lat as f64;
        for j in 0..n_lon {
            let th = std::f64::consts::TAU * j as f64 / n_lon as f64;
            m.vertices.push((c + DVec3::new(phi.cos() * th.cos(), phi.cos() * th.sin(), phi.sin()) * r).to_array());
        }
    }
    m.vertices.push((c + DVec3::Z * r).to_array());
    let top = (m.vertices.len() - 1) as u32;
    let ring = |i: usize, j: usize| (1 + i * n_lon + j % n_lon) as u32;
    for j in 0..n_lon {
        m.triangles.push([0, ring(0, j + 1), ring(0, j)]);
        m.triangles.push([top, ring(n_lat - 2, j), ring(n_lat - 2, j + 1)]);
    }
    for i in 0..n_lat - 2 {
        for j in 0..n_lon {
            m.triangles.push([ring(i, j), ring(i, j + 1), ring(i + 1, j + 1)]);
            m.triangles.push([ring(i, j), ring(i + 1, j + 1), ring(i + 1, j)]);
        }
    }
    m
}

/// Две перпендикулярные оси к направлению `t` с правой тройкой x × y = t.
pub fn perp_frame(t: DVec3) -> (DVec3, DVec3) {
    let helper = if t.z.abs() < 0.9 { DVec3::Z } else { DVec3::X };
    let x = (helper - t * helper.dot(t)).normalize();
    (x, t.cross(x))
}

/// Усечённый конус/цилиндр вдоль оси `dir` из точки `origin`: радиус r0 на высоте h0 и r1 на высоте h1.
pub fn frustum_mesh(origin: DVec3, dir: DVec3, r0: f64, h0: f64, r1: f64, h1: f64) -> Mesh {
    let (r0, h0, r1, h1) = if h0 <= h1 { (r0, h0, r1, h1) } else { (r1, h1, r0, h0) };
    let dir = dir.normalize();
    let (x, y) = perp_frame(dir);
    let n = arc_segments(std::f64::consts::TAU);
    let mut m = Mesh::default();
    for (r, h) in [(r0, h0), (r1, h1)] {
        for i in 0..n {
            let t = std::f64::consts::TAU * i as f64 / n as f64;
            m.vertices.push((origin + dir * h + (x * t.cos() + y * t.sin()) * r).to_array());
        }
    }
    m.vertices.push((origin + dir * h0).to_array());
    m.vertices.push((origin + dir * h1).to_array());
    let (c0, c1, n) = (2 * n as u32, 2 * n as u32 + 1, n as u32);
    for i in 0..n {
        let j = (i + 1) % n;
        m.triangles.push([i, j, j + n]);
        m.triangles.push([i, j + n, i + n]);
        m.triangles.push([c0, j, i]);
        m.triangles.push([c1, i + n, j + n]);
    }
    m
}

/// Тор: большой радиус R (до центра сечения), малый r.
pub fn torus_mesh(center: DVec3, big: f64, small: f64) -> Mesh {
    let (nu, nv) = (arc_segments(std::f64::consts::TAU), 36usize);
    let mut m = Mesh::default();
    for i in 0..nu {
        let th = std::f64::consts::TAU * i as f64 / nu as f64;
        for j in 0..nv {
            let ph = std::f64::consts::TAU * j as f64 / nv as f64;
            let rr = big + small * ph.cos();
            m.vertices.push((center + DVec3::new(rr * th.cos(), rr * th.sin(), small * ph.sin())).to_array());
        }
    }
    let id = |i: usize, j: usize| ((i % nu) * nv + j % nv) as u32;
    for i in 0..nu {
        for j in 0..nv {
            m.triangles.push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
            m.triangles.push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
        }
    }
    m
}

/// Окружность в плоскости, перпендикулярной направлению `t`, вокруг точки `c`.
pub fn circle_loop(c: DVec3, t: DVec3, radius: f64, ccw: bool) -> Vec<DVec3> {
    let (x, y) = perp_frame(t.normalize());
    let n = arc_segments(std::f64::consts::TAU);
    let mut pts: Vec<DVec3> = (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            c + (x * a.cos() + y * a.sin()) * radius
        })
        .collect();
    if !ccw {
        pts.reverse();
    }
    pts
}

/// Спираль вокруг оси `axis` через `center`: диаметр, шаг, число витков.
pub fn helix_path(center: DVec3, axis: DVec3, dia: f64, pitch: f64, turns: f64) -> Vec<DVec3> {
    let (x, y) = perp_frame(axis.normalize());
    let steps = ((turns.abs() * 48.0).ceil() as usize).max(8);
    (0..=steps)
        .map(|i| {
            let t = turns * std::f64::consts::TAU * i as f64 / steps as f64;
            center + (x * t.cos() + y * t.sin()) * (dia / 2.0) + axis.normalize() * (pitch * t / std::f64::consts::TAU)
        })
        .collect()
}

/// Протянуть профиль (замкнутые контуры в мировых координатах: внешний, затем отверстия) вдоль ломаной.
/// Профиль проецируется на плоскость, перпендикулярную началу пути; на изломах — косые стыки.
pub fn sweep_loops(path: &[DVec3], outer: &[DVec3], holes: &[Vec<DVec3>]) -> Result<Mesh, String> {
    use crate::sketch::signed_area;
    let mut p: Vec<DVec3> = Vec::new();
    for q in path {
        if p.last().is_none_or(|l| l.distance(*q) > 1e-9) {
            p.push(*q);
        }
    }
    if p.len() < 2 {
        return Err("путь слишком короткий".into());
    }
    let m = p.len();
    let d: Vec<DVec3> = (0..m - 1).map(|i| (p[i + 1] - p[i]).normalize()).collect();
    let (x0, y0) = perp_frame(d[0]);
    let to2d = |l: &[DVec3]| -> Vec<[f64; 2]> { l.iter().map(|q| [(*q - p[0]).dot(x0), (*q - p[0]).dot(y0)]).collect() };
    let mut o2 = to2d(outer);
    if signed_area(&o2) < 0.0 {
        o2.reverse();
    }
    let h2: Vec<Vec<[f64; 2]>> = holes
        .iter()
        .map(|h| {
            let mut h = to2d(h);
            if signed_area(&h) > 0.0 {
                h.reverse();
            }
            h
        })
        .collect();
    let region = Region { outer: o2, holes: h2, ids: vec![] };
    let (verts, caps) = region.triangulate();
    if caps.is_empty() {
        return Err("профиль вырожден".into());
    }
    let n = verts.len() as u32;
    // оси по сегментам (параллельный перенос)
    let mut fx = vec![x0];
    let mut fy = vec![y0];
    for j in 0..m - 2 {
        let q = glam::DQuat::from_rotation_arc(d[j], d[j + 1]);
        fx.push(q * fx[j]);
        fy.push(q * fy[j]);
    }
    let mut mesh = Mesh::default();
    for k in 0..m {
        let seg = k.saturating_sub(1); // кольцо в точке k лежит в системе входящего сегмента
        for v in &verts {
            let mut pt = p[k] + fx[seg] * v[0] + fy[seg] * v[1];
            if k > 0 && k < m - 1 {
                let tb = (d[k - 1] + d[k]).normalize_or_zero();
                let c = d[k - 1].dot(tb);
                if c < 0.05 {
                    return Err("слишком резкий поворот пути".into());
                }
                pt += d[k - 1] * (-((pt - p[k]).dot(tb)) / c);
            }
            mesh.vertices.push(pt.to_array());
        }
    }
    let mut start = 0u32;
    let loops = std::iter::once(region.outer.len()).chain(region.holes.iter().map(|h| h.len()));
    for len in loops {
        let len = len as u32;
        for k in 0..(m as u32 - 1) {
            let o = k * n;
            for i in 0..len {
                let (a, b) = (start + i + o, start + (i + 1) % len + o);
                mesh.triangles.push([a, b, b + n]);
                mesh.triangles.push([a, b + n, a + n]);
            }
        }
        start += len;
    }
    let last = (m as u32 - 1) * n;
    for t in &caps {
        let [a, b, c] = t.map(|i| i as u32);
        mesh.triangles.push([a, c, b]);
        mesh.triangles.push([a + last, b + last, c + last]);
    }
    if mesh.volume() < 0.0 {
        mesh.flip();
    }
    Ok(mesh)
}

/// Лофт между контурами (в мировых координатах): плавный переход от первого ко всем следующим.
pub fn loft_loops(loops: &[Vec<DVec3>]) -> Result<Mesh, String> {
    use crate::sketch::signed_area;
    if loops.len() < 2 {
        return Err("для лофта нужно минимум два профиля".into());
    }
    const N: usize = 96;
    let centroid = |l: &[DVec3]| l.iter().fold(DVec3::ZERO, |a, p| a + *p) / l.len() as f64;
    let normal = |l: &[DVec3]| {
        let mut n = DVec3::ZERO;
        for i in 0..l.len() {
            let (a, b) = (l[i], l[(i + 1) % l.len()]);
            n += DVec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        n.normalize_or_zero()
    };
    let dir = (centroid(loops.last().unwrap()) - centroid(&loops[0])).normalize_or_zero();
    if dir == DVec3::ZERO {
        return Err("профили лежат в одном месте".into());
    }
    let resample = |l: &[DVec3]| -> Vec<DVec3> {
        let mut pts = l.to_vec();
        pts.push(l[0]);
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            cum.push(cum.last().unwrap() + w[0].distance(w[1]));
        }
        let total = *cum.last().unwrap();
        (0..N)
            .map(|i| {
                let target = total * i as f64 / N as f64;
                let k = cum.partition_point(|c| *c <= target).clamp(1, pts.len() - 1);
                let t = (target - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-12);
                pts[k - 1].lerp(pts[k], t)
            })
            .collect()
    };
    let mut rings: Vec<Vec<DVec3>> = Vec::new();
    for l in loops {
        let mut l = l.clone();
        if normal(&l).dot(dir) < 0.0 {
            l.reverse();
        }
        let mut r = resample(&l);
        if let Some(prev) = rings.last() {
            let best = (0..N)
                .min_by(|&a, &b| {
                    let cost = |s: usize| (0..N).map(|j| prev[j].distance_squared(r[(j + s) % N])).sum::<f64>();
                    cost(a).total_cmp(&cost(b))
                })
                .unwrap();
            r.rotate_left(best);
        }
        rings.push(r);
    }
    let mut m = Mesh::default();
    for r in &rings {
        m.vertices.extend(r.iter().map(|p| p.to_array()));
    }
    let nn = N as u32;
    for k in 0..rings.len() as u32 - 1 {
        for i in 0..nn {
            let (a, b) = (k * nn + i, k * nn + (i + 1) % nn);
            m.triangles.push([a, b, b + nn]);
            m.triangles.push([a, b + nn, a + nn]);
        }
    }
    for (ring, base, top) in [(&rings[0], 0u32, false), (rings.last().unwrap(), (rings.len() as u32 - 1) * nn, true)] {
        let nrm = normal(ring);
        let (x, y) = perp_frame(if nrm == DVec3::ZERO { dir } else { nrm });
        let c = centroid(ring);
        let mut pts: Vec<[f64; 2]> = ring.iter().map(|p| [(*p - c).dot(x), (*p - c).dot(y)]).collect();
        let flip = signed_area(&pts) < 0.0;
        if flip {
            pts.reverse();
        }
        let region = Region { outer: pts, holes: vec![], ids: vec![] };
        let (_, tris) = region.triangulate();
        for t in tris {
            let map = |i: usize| if flip { base + (N - 1 - i) as u32 } else { base + i as u32 };
            let [a, b, cc] = t.map(map);
            m.triangles.push(if top { [a, b, cc] } else { [a, cc, b] });
        }
    }
    if m.volume() < 0.0 {
        m.flip();
    }
    Ok(m)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeOp {
    Chamfer(f64),
    Fillet(f64),
}

/// Грани, сходящиеся на ребре: (нормаль наружу, направление внутрь грани) для каждой из двух.
fn edge_faces(mesh: &Mesh, a: DVec3, b: DVec3) -> Result<[(DVec3, DVec3); 2], String> {
    let len = a.distance(b);
    if len < 1e-9 {
        return Err("ребро вырождено".into());
    }
    let e = (b - a) / len;
    let tol = 1e-5 * (1.0 + len);
    let m = (a + b) * 0.5;
    let on_line = |p: DVec3| (p - a).cross(e).length() < tol;
    let mut found: Vec<(DVec3, DVec3)> = Vec::new();
    for t in &mesh.triangles {
        let p = mesh.tri(t);
        for k in 0..3 {
            let (q0, q1, v) = (p[k], p[(k + 1) % 3], p[(k + 2) % 3]);
            if !on_line(q0) || !on_line(q1) {
                continue;
            }
            let (s0, s1) = ((q0 - a).dot(e), (q1 - a).dot(e));
            let sm = (m - a).dot(e);
            if sm < s0.min(s1) - tol || sm > s0.max(s1) + tol {
                continue;
            }
            let n = mesh.tri_normal(t);
            let inward = (v - m) - e * (v - m).dot(e);
            if inward.length() < 1e-12 {
                continue;
            }
            if !found.iter().any(|(fnrm, _)| fnrm.dot(n) > 1.0 - 1e-7) {
                found.push((n, inward.normalize()));
            }
        }
    }
    if found.len() != 2 {
        return Err("выберите ребро между двумя гранями".into());
    }
    let (f1, f2) = (found[0], found[1]);
    if f1.1.dot(f2.0) > 1e-6 || f2.1.dot(f1.0) > 1e-6 {
        return Err("вогнутые рёбра пока не поддерживаются".into());
    }
    Ok([f1, f2])
}

/// Сечение инструмента вокруг точки ребра `eo` (в плоскости, перпендикулярной ребру).
fn section_points(eo: DVec3, f: &[(DVec3, DVec3); 2], op: EdgeOp) -> Result<Vec<DVec3>, String> {
    let ((n1, u1), (n2, u2)) = (f[0], f[1]);
    let theta = u1.dot(u2).clamp(-1.0, 1.0).acos();
    if theta < 0.05 || theta > std::f64::consts::PI - 0.05 {
        return Err("грани слишком близки к параллельным".into());
    }
    let size = match op {
        EdgeOp::Chamfer(d) => d,
        EdgeOp::Fillet(r) => r / (theta / 2.0).tan(),
    };
    if size <= 0.0 {
        return Err("размер должен быть больше нуля".into());
    }
    let slack = size.max(0.5) * 0.5;
    let (t1, t2) = (eo + u1 * size, eo + u2 * size);
    let mut poly: Vec<DVec3> = Vec::new();
    match op {
        EdgeOp::Chamfer(_) => poly.extend([t1, t2]),
        EdgeOp::Fillet(r) => {
            let c = eo + (u1 + u2).normalize() * (r / (theta / 2.0).sin());
            let (v1, v2) = ((t1 - c) / r, (t2 - c) / r);
            let phi = v1.dot(v2).clamp(-1.0, 1.0).acos();
            for i in 0..=16 {
                let s = i as f64 / 16.0;
                let v = if phi.abs() < 1e-9 { v1 } else { (v1 * ((1.0 - s) * phi).sin() + v2 * (s * phi).sin()) / phi.sin() };
                poly.push(c + v * r);
            }
        }
    }
    poly.extend([t2 + n2 * slack, eo + (n1 + n2) * slack, t1 + n1 * slack]);
    Ok(poly)
}

/// Инструмент (тело для вычитания), который снимает фаску или скругляет прямое выпуклое ребро.
pub fn edge_tool(mesh: &Mesh, a: DVec3, b: DVec3, op: EdgeOp) -> Result<Mesh, String> {
    let len = a.distance(b);
    let faces = edge_faces(mesh, a, b)?;
    let e = (b - a) / len;
    let poly = section_points(a, &faces, op)?;
    let ext = 0.02 * len + 0.05;
    let (px, py) = perp_frame(e);
    let mut pts: Vec<[f64; 2]> = poly.iter().map(|q| [(*q - a).dot(px), (*q - a).dot(py)]).collect();
    if crate::sketch::signed_area(&pts) < 0.0 {
        pts.reverse();
    }
    let plane = Plane { origin: (a - e * ext).to_array(), x: px.to_array(), y: py.to_array() };
    extrude_region(&plane, &Region { outer: pts, holes: vec![], ids: vec![] }, 0.0, len + 2.0 * ext).ok_or_else(|| "не удалось построить инструмент".to_string())
}

/// То же для круглого ребра (окружность вокруг оси `axis` с центром `center`, `p` — точка на ней).
pub fn circle_tool(mesh: &Mesh, center: DVec3, axis: DVec3, p: DVec3, op: EdgeOp) -> Result<Mesh, String> {
    let axis = axis.normalize();
    let r = (p - center - axis * (p - center).dot(axis)).length();
    if r < 1e-6 {
        return Err("ребро вырождено".into());
    }
    let tol = 1e-4 * (1.0 + r);
    let on_circle = |q: DVec3| ((q - center) - axis * (q - center).dot(axis)).length() - r < tol && ((q - center) - axis * (q - center).dot(axis)).length() - r > -tol && ((q - p).dot(axis)).abs() < tol;
    // любой короткий кусок окружности из сетки
    let seg = mesh
        .sharp_edges(30.0)
        .into_iter()
        .map(|(i, j)| (DVec3::from(mesh.vertices[i as usize]), DVec3::from(mesh.vertices[j as usize])))
        .find(|(q0, q1)| on_circle(*q0) && on_circle(*q1))
        .ok_or("круглое ребро не найдено в теле")?;
    let faces = edge_faces(mesh, seg.0, seg.1)?;
    let m = (seg.0 + seg.1) * 0.5;
    let radial = ((m - center) - axis * (m - center).dot(axis)).normalize();
    let poly = section_points(m, &faces, op)?;
    let mut pts: Vec<[f64; 2]> = poly.iter().map(|q| [(*q - center).dot(radial), (*q - center).dot(axis)]).collect();
    if pts.iter().any(|q| q[0] <= 1e-6) {
        return Err("размер слишком велик для этого ребра".into());
    }
    if crate::sketch::signed_area(&pts) < 0.0 {
        pts.reverse();
    }
    let plane = Plane { origin: center.to_array(), x: radial.to_array(), y: axis.to_array() };
    revolve_region(&plane, &Region { outer: pts, holes: vec![], ids: vec![] }, [0.0, 0.0], [0.0, 1.0], 360.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sketch::Sketch;

    fn rect_region(w: f64, h: f64) -> Region {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [w, h]);
        s.regions().remove(0)
    }

    #[test]
    fn extrude_rect_and_ring() {
        let m = extrude_region(&Plane::XY, &rect_region(20.0, 10.0), 0.0, 5.0).unwrap();
        assert!((m.volume() - 1000.0).abs() < 1e-9);
        let m2 = extrude_region(&Plane::XY, &rect_region(20.0, 10.0), 0.0, -5.0).unwrap();
        assert!((m2.volume() - 1000.0).abs() < 1e-9);

        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [20.0, 20.0]);
        s.add_circle([10.0, 10.0], 5.0);
        let ring = s.regions().into_iter().find(|r| !r.holes.is_empty()).unwrap();
        let m = extrude_region(&Plane::XY, &ring, 0.0, 2.0).unwrap();
        assert!((m.volume() - ring.area() * 2.0).abs() < 1e-6);
        assert!(crate::csg::Solid::from_mesh(&m).is_ok(), "сетка должна быть замкнутой");
    }

    #[test]
    fn extrude_on_side_plane_goes_along_normal() {
        let m = extrude_region(&Plane::YZ, &rect_region(10.0, 10.0), 0.0, 3.0).unwrap();
        let (lo, hi) = m.bounds().unwrap();
        assert!((lo.x - 0.0).abs() < 1e-12 && (hi.x - 3.0).abs() < 1e-12);
        assert!(m.volume() > 0.0);
    }

    #[test]
    fn revolve_rect_makes_tube() {
        // прямоугольник от r=5 до r=10, высота 4, вокруг оси Y чертежа
        let mut s = Sketch::default();
        s.add_rect([5.0, 0.0], [10.0, 4.0]);
        let r = s.regions().remove(0);
        let m = revolve_region(&Plane::XZ, &r, [0.0, 0.0], [0.0, 1.0], 360.0).unwrap();
        let exact = std::f64::consts::PI * (100.0 - 25.0) * 4.0;
        assert!((m.volume() - exact).abs() / exact < 0.01);
        assert!(crate::csg::Solid::from_mesh(&m).is_ok());
        let half = revolve_region(&Plane::XZ, &r, [0.0, 0.0], [0.0, 1.0], 180.0).unwrap();
        assert!((half.volume() - exact / 2.0).abs() / exact < 0.01);
        assert!(crate::csg::Solid::from_mesh(&half).is_ok());
    }

    #[test]
    fn revolve_touching_axis_closes() {
        // прямоугольник, касающийся оси — даёт сплошной цилиндр
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [5.0, 10.0]);
        let r = s.regions().remove(0);
        let m = revolve_region(&Plane::XZ, &r, [0.0, 0.0], [0.0, 1.0], 360.0).unwrap();
        let exact = std::f64::consts::PI * 25.0 * 10.0;
        assert!((m.volume() - exact).abs() / exact < 0.01);
        assert!(crate::csg::Solid::from_mesh(&m).is_ok());
    }

    #[test]
    fn revolve_crossing_axis_fails() {
        let mut s = Sketch::default();
        s.add_rect([-5.0, 0.0], [5.0, 10.0]);
        let r = s.regions().remove(0);
        assert!(revolve_region(&Plane::XZ, &r, [0.0, 0.0], [0.0, 1.0], 360.0).is_err());
    }

    #[test]
    fn primitives_are_closed() {
        let s = sphere_mesh(DVec3::ZERO, 10.0);
        let exact = 4.0 / 3.0 * std::f64::consts::PI * 1000.0;
        assert!((s.volume() - exact).abs() / exact < 0.02);
        assert!(crate::csg::Solid::from_mesh(&s).is_ok());
        let c = cylinder_mesh(DVec3::new(1.0, 2.0, 3.0), 4.0, 5.0);
        assert!(crate::csg::Solid::from_mesh(&c).is_ok());
        assert!(c.volume() > 0.0);
    }

    use crate::csg::Solid;

    #[test]
    fn hole_primitives_are_valid_solids() {
        let c = frustum_mesh(DVec3::ZERO, DVec3::Z, 5.0, -3.0, 5.0, 4.0);
        assert!(Solid::from_mesh(&c).is_ok());
        assert!((c.volume() - std::f64::consts::PI * 25.0 * 7.0).abs() / (std::f64::consts::PI * 175.0) < 0.01);
        let cone = frustum_mesh(DVec3::new(1.0, 2.0, 3.0), DVec3::new(1.0, 1.0, 0.0), 2.0, 0.0, 6.0, 5.0);
        assert!(Solid::from_mesh(&cone).is_ok());
        let t = torus_mesh(DVec3::ZERO, 10.0, 2.0);
        let exact = 2.0 * std::f64::consts::PI * std::f64::consts::PI * 10.0 * 4.0;
        assert!((t.volume() - exact).abs() / exact < 0.02);
        assert!(Solid::from_mesh(&t).is_ok());
    }

    #[test]
    fn sweep_straight_and_bent_pipe() {
        let path = [DVec3::ZERO, DVec3::new(0.0, 0.0, 20.0)];
        let outer = circle_loop(path[0], DVec3::Z, 3.0, true);
        let m = sweep_loops(&path, &outer, &[]).unwrap();
        assert!((m.volume() - std::f64::consts::PI * 9.0 * 20.0).abs() / (std::f64::consts::PI * 180.0) < 0.01);
        assert!(Solid::from_mesh(&m).is_ok());
        // труба с изгибом под 90° (косой стык) и стенкой
        let path = [DVec3::ZERO, DVec3::new(0.0, 0.0, 20.0), DVec3::new(20.0, 0.0, 20.0)];
        let outer = circle_loop(path[0], DVec3::Z, 3.0, true);
        let inner = circle_loop(path[0], DVec3::Z, 2.0, false);
        let m = sweep_loops(&path, &outer, &[inner]).unwrap();
        let exact = std::f64::consts::PI * (9.0 - 4.0) * 40.0;
        assert!((m.volume() - exact).abs() / exact < 0.03, "{} vs {exact}", m.volume());
        assert!(Solid::from_mesh(&m).is_ok());
    }

    #[test]
    fn coil_is_valid() {
        let path = helix_path(DVec3::ZERO, DVec3::Z, 20.0, 6.0, 4.0);
        let tangent = (path[1] - path[0]).normalize();
        let prof = circle_loop(path[0], tangent, 1.5, true);
        let m = sweep_loops(&path, &prof, &[]).unwrap();
        let len: f64 = path.windows(2).map(|w| w[0].distance(w[1])).sum();
        let exact = std::f64::consts::PI * 2.25 * len;
        assert!((m.volume() - exact).abs() / exact < 0.05, "{} vs {exact}", m.volume());
        assert!(Solid::from_mesh(&m).is_ok());
    }

    #[test]
    fn loft_square_to_circle() {
        let sq: Vec<DVec3> = [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]].iter().map(|p| DVec3::new(p[0], p[1], 0.0)).collect();
        let circ = circle_loop(DVec3::new(0.0, 0.0, 10.0), DVec3::Z, 4.0, true);
        let m = loft_loops(&[sq, circ]).unwrap();
        assert!(m.volume() > 400.0 && m.volume() < 1000.0, "{}", m.volume());
        assert!(Solid::from_mesh(&m).is_ok());
    }

    #[test]
    fn chamfer_and_fillet_box_edge() {
        let b = box_mesh(DVec3::ZERO, DVec3::new(20.0, 20.0, 10.0));
        let solid = Solid::from_mesh(&b).unwrap();
        let (a, c) = (DVec3::new(0.0, 0.0, 10.0), DVec3::new(20.0, 0.0, 10.0));
        let tool = Solid::from_mesh(&edge_tool(&b, a, c, EdgeOp::Chamfer(2.0)).unwrap()).unwrap();
        let r = solid.boolean(&tool, crate::csg::BoolOp::Subtract);
        assert!((r.volume() - (4000.0 - 0.5 * 4.0 * 20.0)).abs() < 1e-6, "{}", r.volume());
        let tool = Solid::from_mesh(&edge_tool(&b, a, c, EdgeOp::Fillet(3.0)).unwrap()).unwrap();
        let r = solid.boolean(&tool, crate::csg::BoolOp::Subtract);
        let removed = (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 20.0;
        assert!((r.volume() - (4000.0 - removed)).abs() < 3.0, "{} vs {}", r.volume(), 4000.0 - removed);
        // вертикальное ребро тоже
        let tool = edge_tool(&b, DVec3::new(20.0, 20.0, 0.0), DVec3::new(20.0, 20.0, 10.0), EdgeOp::Chamfer(1.0)).unwrap();
        assert!(Solid::from_mesh(&tool).is_ok());
        // ребро не из меша
        assert!(edge_tool(&b, DVec3::new(5.0, 5.0, 5.0), DVec3::new(6.0, 5.0, 5.0), EdgeOp::Chamfer(1.0)).is_err());
    }

    #[test]
    fn round_rim_of_cylinder_and_hole() {
        let cyl = cylinder_mesh(DVec3::ZERO, 10.0, 20.0);
        let solid = Solid::from_mesh(&cyl).unwrap();
        let top_rim = DVec3::new(10.0, 0.0, 20.0);
        let tool = circle_tool(&cyl, DVec3::new(0.0, 0.0, 20.0), DVec3::Z, top_rim, EdgeOp::Chamfer(2.0)).unwrap();
        let r = solid.boolean(&Solid::from_mesh(&tool).unwrap(), crate::csg::BoolOp::Subtract);
        let removed = std::f64::consts::PI * 2.0 * (10.0 - 2.0 / 3.0) * 2.0;
        assert!((solid.volume() - r.volume() - removed).abs() / removed < 0.03, "{} vs {removed}", solid.volume() - r.volume());
        // скругление
        let tool = circle_tool(&cyl, DVec3::new(0.0, 0.0, 20.0), DVec3::Z, top_rim, EdgeOp::Fillet(3.0)).unwrap();
        let r = solid.boolean(&Solid::from_mesh(&tool).unwrap(), crate::csg::BoolOp::Subtract);
        assert!(solid.volume() - r.volume() > 20.0);
        // кромка отверстия в пластине
        let plate = box_mesh(DVec3::new(-20.0, -20.0, 0.0), DVec3::new(20.0, 20.0, 10.0));
        let hole = cylinder_mesh(DVec3::new(0.0, 0.0, -1.0), 5.0, 12.0);
        let holed = Solid::from_mesh(&plate).unwrap().boolean(&Solid::from_mesh(&hole).unwrap(), crate::csg::BoolOp::Subtract);
        let tool = circle_tool(&holed.mesh, DVec3::new(0.0, 0.0, 10.0), DVec3::Z, DVec3::new(5.0, 0.0, 10.0), EdgeOp::Chamfer(1.0)).unwrap();
        let r = holed.boolean(&Solid::from_mesh(&tool).unwrap(), crate::csg::BoolOp::Subtract);
        let removed = std::f64::consts::PI * 2.0 * (5.0 + 1.0 / 3.0) * 0.5;
        assert!((holed.volume() - r.volume() - removed).abs() / removed < 0.05, "{} vs {removed}", holed.volume() - r.volume());
    }
}
