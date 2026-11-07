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

    /// Reads an STL file (binary or ASCII) into a welded mesh with outward winding.
    pub fn read_stl(data: &[u8]) -> Result<Mesh, String> {
        let binary = data.len() >= 84 && {
            let n = u32::from_le_bytes([data[80], data[81], data[82], data[83]]) as usize;
            data.len() == 84 + 50 * n
        };
        let mut corners: Vec<V3> = Vec::new();
        if binary {
            for rec in data[84..].chunks_exact(50) {
                for k in 0..3 {
                    let o = 12 + 12 * k;
                    let f = |j: usize| f32::from_le_bytes([rec[o + 4 * j], rec[o + 4 * j + 1], rec[o + 4 * j + 2], rec[o + 4 * j + 3]]) as f64;
                    corners.push([f(0), f(1), f(2)]);
                }
            }
        } else {
            let text = std::str::from_utf8(data).map_err(|_| "это не файл STL".to_string())?;
            if !text.trim_start().starts_with("solid") {
                return Err("это не файл STL".into());
            }
            for line in text.lines() {
                let mut w = line.split_whitespace();
                if w.next() == Some("vertex") {
                    let c: Vec<f64> = w.filter_map(|x| x.parse().ok()).collect();
                    if c.len() != 3 {
                        return Err("повреждённая строка вершины в STL".into());
                    }
                    corners.push([c[0], c[1], c[2]]);
                }
            }
        }
        if corners.is_empty() || corners.len() % 3 != 0 || corners.iter().flatten().any(|c| !c.is_finite()) {
            return Err("в файле STL нет треугольников".into());
        }
        let mut mesh = Mesh { triangles: (0..corners.len() as u32 / 3).map(|i| [3 * i, 3 * i + 1, 3 * i + 2]).collect(), vertices: corners };
        mesh.weld(1e-6);
        if mesh.triangles.is_empty() {
            return Err("в файле STL нет треугольников".into());
        }
        if mesh.volume() < 0.0 {
            mesh.flip();
        }
        Ok(mesh)
    }

    /// The separate pieces of the mesh: triangles joined through shared vertices stay together.
    pub fn components(&self) -> Vec<Mesh> {
        let n = self.vertices.len();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut Vec<usize>, mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        for t in &self.triangles {
            let a = find(&mut parent, t[0] as usize);
            for &v in &t[1..] {
                let b = find(&mut parent, v as usize);
                if a != b {
                    parent[b] = a;
                }
            }
        }
        let mut groups: std::collections::BTreeMap<usize, Mesh> = std::collections::BTreeMap::new();
        let mut remap: Vec<Option<u32>> = vec![None; n];
        for t in &self.triangles {
            let root = find(&mut parent, t[0] as usize);
            let g = groups.entry(root).or_insert_with(|| Mesh { vertices: Vec::new(), triangles: Vec::new() });
            let tri = t.map(|v| {
                *remap[v as usize].get_or_insert_with(|| {
                    g.vertices.push(self.vertices[v as usize]);
                    (g.vertices.len() - 1) as u32
                })
            });
            g.triangles.push(tri);
        }
        groups.into_values().collect()
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

    /// 3MF — формат для 3D-печати (Cura, PrusaSlicer, Bambu Studio): тела с названиями, единицы — миллиметры.
    pub fn write_3mf(meshes: &[(&str, &Mesh)]) -> Vec<u8> {
        use std::fmt::Write;
        fn esc(s: &str) -> String {
            s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
        }
        let mut model = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n<metadata name=\"Application\">Genix</metadata>\n<resources>\n");
        for (i, (name, m)) in meshes.iter().enumerate() {
            let _ = writeln!(model, "<object id=\"{}\" name=\"{}\" type=\"model\"><mesh><vertices>", i + 1, esc(name));
            for v in &m.vertices {
                let _ = writeln!(model, "<vertex x=\"{}\" y=\"{}\" z=\"{}\"/>", v[0], v[1], v[2]);
            }
            model.push_str("</vertices><triangles>\n");
            for t in &m.triangles {
                let _ = writeln!(model, "<triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>", t[0], t[1], t[2]);
            }
            model.push_str("</triangles></mesh></object>\n");
        }
        model.push_str("</resources>\n<build>\n");
        for i in 0..meshes.len() {
            let _ = writeln!(model, "<item objectid=\"{}\"/>", i + 1);
        }
        model.push_str("</build>\n</model>\n");
        let types = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"model\" ContentType=\"application/vnd.ms-package.3dmanufacturing-3dmodel+xml\"/></Types>";
        let rels = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Target=\"/3D/3dmodel.model\" Id=\"rel0\" Type=\"http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel\"/></Relationships>";
        zip_stored(&[("[Content_Types].xml", types.as_bytes()), ("_rels/.rels", rels.as_bytes()), ("3D/3dmodel.model", model.as_bytes())])
    }

    /// Area of the listed triangles (a face of the body is a group of them), in mm².
    pub fn triangles_area(&self, tris: &[usize]) -> f64 {
        tris.iter()
            .map(|&t| {
                let [a, b, c] = self.triangles[t].map(|i| glam::DVec3::from(self.vertices[i as usize]));
                (b - a).cross(c - a).length() * 0.5
            })
            .sum()
    }

    /// Angle in degrees at `vertex` between the rays to `a` and `c` (0..=180).
    pub fn angle_at(a: glam::DVec3, vertex: glam::DVec3, c: glam::DVec3) -> f64 {
        let (u, v) = ((a - vertex).normalize_or_zero(), (c - vertex).normalize_or_zero());
        u.dot(v).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// Angle between two faces in degrees, from their outward normals: 90 for the faces of a box that meet at an edge,
    /// 180 for two coplanar faces, small for a sharp wedge.
    pub fn angle_between_faces(n1: glam::DVec3, n2: glam::DVec3) -> f64 {
        180.0 - n1.normalize_or_zero().dot(n2.normalize_or_zero()).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// STEP (ISO 10303-21, AP214): every body as a faceted B-rep, one planar face per triangle, millimetres.
    /// It carries the exact triangles of the model (no curved surfaces), which CAD programs import as a solid made of facets.
    pub fn write_step(meshes: &[(&str, &Mesh)], file_name: &str) -> String {
        use std::fmt::Write;
        fn q(s: &str) -> String {
            s.chars().map(|c| if c.is_ascii() && c != '\'' && c != '\\' && !c.is_control() { c.to_string() } else { format!("\\X2\\{:04X}\\X0\\", c as u32) }).collect()
        }
        let real = |v: f64| {
            let v = if v.abs() < 5e-7 { 0.0 } else { v };
            format!("{v:.6}")
        };
        let mut s = String::new();
        let _ = write!(s, "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION(('Genix model'),'2;1');\nFILE_NAME('{}','2026-01-01T00:00:00',('Genix'),('Genix'),'Genix','Genix','');\nFILE_SCHEMA(('AUTOMOTIVE_DESIGN {{ 1 0 10303 214 1 1 1 1 }}'));\nENDSEC;\nDATA;\n", q(file_name));
        let id = std::cell::Cell::new(0u64);
        let next = |s: &mut String, body: String| -> u64 {
            id.set(id.get() + 1);
            let _ = writeln!(s, "#{}={body};", id.get());
            id.get()
        };
        let app_ctx = next(&mut s, "APPLICATION_CONTEXT('core data for automotive mechanical design processes')".into());
        next(&mut s, format!("APPLICATION_PROTOCOL_DEFINITION('international standard','automotive_design',2000,#{app_ctx})"));
        let len = next(&mut s, "(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.))".into());
        let ang = next(&mut s, "(NAMED_UNIT(*) PLANE_ANGLE_UNIT() SI_UNIT($,.RADIAN.))".into());
        let ste = next(&mut s, "(NAMED_UNIT(*) SI_UNIT($,.STERADIAN.) SOLID_ANGLE_UNIT())".into());
        let unc = next(&mut s, format!("UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#{len},'distance_accuracy_value','')"));
        let ctx = next(&mut s, format!("(GEOMETRIC_REPRESENTATION_CONTEXT(3) GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{unc})) GLOBAL_UNIT_ASSIGNED_CONTEXT((#{len},#{ang},#{ste})) REPRESENTATION_CONTEXT('Context','3D'))"));
        let pctx = next(&mut s, format!("PRODUCT_CONTEXT('',#{app_ctx},'mechanical')"));
        let dctx = next(&mut s, format!("PRODUCT_DEFINITION_CONTEXT('part definition',#{app_ctx},'design')"));
        let product = next(&mut s, format!("PRODUCT('{0}','{0}','',(#{pctx}))", q(file_name)));
        let formation = next(&mut s, format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
        let definition = next(&mut s, format!("PRODUCT_DEFINITION('design','',#{formation},#{dctx})"));
        let def_shape = next(&mut s, format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
        let origin = next(&mut s, "CARTESIAN_POINT('',(0.,0.,0.))".into());
        let dir_z = next(&mut s, "DIRECTION('',(0.,0.,1.))".into());
        let dir_x = next(&mut s, "DIRECTION('',(1.,0.,0.))".into());
        let axis = next(&mut s, format!("AXIS2_PLACEMENT_3D('',#{origin},#{dir_z},#{dir_x})"));
        let mut breps = Vec::new();
        for (name, m) in meshes {
            let first_point = id.get() + 1;
            for v in &m.vertices {
                next(&mut s, format!("CARTESIAN_POINT('',({},{},{}))", real(v[0]), real(v[1]), real(v[2])));
            }
            let mut faces = Vec::with_capacity(m.triangles.len());
            for t in &m.triangles {
                let pts: Vec<String> = t.iter().map(|&i| format!("#{}", first_point + i as u64)).collect();
                let lp = next(&mut s, format!("POLY_LOOP('',({}))", pts.join(",")));
                let bound = next(&mut s, format!("FACE_OUTER_BOUND('',#{lp},.T.)"));
                faces.push(next(&mut s, format!("FACE('',(#{bound}))")));
            }
            let shell = next(&mut s, format!("CLOSED_SHELL('',({}))", faces.iter().map(|f| format!("#{f}")).collect::<Vec<_>>().join(",")));
            breps.push(next(&mut s, format!("FACETED_BREP('{}',#{shell})", q(name))));
        }
        let items: Vec<String> = std::iter::once(axis).chain(breps).map(|i| format!("#{i}")).collect();
        let rep = next(&mut s, format!("FACETED_BREP_SHAPE_REPRESENTATION('',({}),#{ctx})", items.join(",")));
        next(&mut s, format!("SHAPE_DEFINITION_REPRESENTATION(#{def_shape},#{rep})"));
        s.push_str("ENDSEC;\nEND-ISO-10303-21;\n");
        s
    }

    /// DXF with every triangle as a 3DFACE on a layer named after its body (AutoCAD, Fusion, FreeCAD, LibreCAD read it).
    pub fn write_dxf(meshes: &[(&str, &Mesh)]) -> String {
        use std::fmt::Write;
        let mut s = String::from("0\nSECTION\n2\nHEADER\n9\n$ACADVER\n1\nAC1009\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
        for (k, (name, m)) in meshes.iter().enumerate() {
            // layer names stay plain ASCII (older DXF readers choke on anything else); a name without ASCII letters becomes Body_N
            let ascii: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
            let layer = if ascii.chars().any(|c| c.is_ascii_alphanumeric()) { ascii } else { format!("Body_{}", k + 1) };
            for t in &m.triangles {
                let v = |i: usize| m.vertices[t[i] as usize];
                let _ = write!(s, "0\n3DFACE\n8\n{layer}\n");
                // the fourth corner repeats the third: a triangle
                for (k, p) in [v(0), v(1), v(2), v(2)].iter().enumerate() {
                    let _ = write!(s, "{}\n{}\n{}\n{}\n{}\n{}\n", 10 + k, p[0], 20 + k, p[1], 30 + k, p[2]);
                }
            }
        }
        s.push_str("0\nENDSEC\n0\nEOF\n");
        s
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

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Минимальный zip без сжатия (достаточно для 3MF).
fn zip_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = out.len() as u32;
        let crc = crc32(data);
        let (n, len) = (name.as_bytes(), data.len() as u32);
        for (buf, local) in [(&mut out, true), (&mut central, false)] {
            buf.extend_from_slice(&(if local { 0x0403_4b50u32 } else { 0x0201_4b50u32 }).to_le_bytes());
            if !local {
                buf.extend_from_slice(&20u16.to_le_bytes()); // создано версией
            }
            buf.extend_from_slice(&20u16.to_le_bytes()); // нужна версия
            buf.extend_from_slice(&0x0800u16.to_le_bytes()); // имена в UTF-8
            buf.extend_from_slice(&0u16.to_le_bytes()); // без сжатия
            buf.extend_from_slice(&0u16.to_le_bytes()); // время
            buf.extend_from_slice(&0x21u16.to_le_bytes()); // дата 1980-01-01
            buf.extend_from_slice(&crc.to_le_bytes());
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(&len.to_le_bytes());
            buf.extend_from_slice(&(n.len() as u16).to_le_bytes());
            buf.extend_from_slice(&0u16.to_le_bytes()); // extra
            if !local {
                buf.extend_from_slice(&[0u8; 6]); // комментарий, диск, внутренние атрибуты
                buf.extend_from_slice(&0u32.to_le_bytes()); // внешние атрибуты
                buf.extend_from_slice(&offset.to_le_bytes());
            }
            buf.extend_from_slice(n);
            if local {
                buf.extend_from_slice(data);
            }
        }
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
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
    fn obj_and_3mf_exports_hold_every_triangle() {
        let c = cube();
        let obj = Mesh::write_obj(&[("cube", &c)]);
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 12, "one face line per triangle");
        assert_eq!(obj.lines().filter(|l| l.starts_with("v ")).count(), c.vertices.len());
        let zip = Mesh::write_3mf(&[("cube", &c)]);
        assert_eq!(&zip[..2], b"PK", "a 3MF file is a zip archive");
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

    #[test]
    fn threemf_is_a_valid_zip_with_the_model() {
        let m = cube();
        let bytes = Mesh::write_3mf(&[("Тело <1>", &m)]);
        if let Ok(out) = std::env::var("GENIX_3MF_OUT") {
            std::fs::write(out, &bytes).unwrap();
        }
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        // конец архива: подпись, 3 файла, каталог лежит ровно перед концом
        let e = bytes.len() - 22;
        assert_eq!(&bytes[e..e + 4], b"PK\x05\x06");
        assert_eq!(u16::from_le_bytes([bytes[e + 10], bytes[e + 11]]), 3);
        let size = u32::from_le_bytes(bytes[e + 12..e + 16].try_into().unwrap()) as usize;
        let off = u32::from_le_bytes(bytes[e + 16..e + 20].try_into().unwrap()) as usize;
        assert_eq!(off + size, e);
        assert_eq!(&bytes[off..off + 4], b"PK\x01\x02");
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Тело &lt;1&gt;") && text.contains("unit=\"millimeter\""));
        assert_eq!(text.matches("<triangle ").count(), m.triangles.len());
    }
}

#[cfg(test)]
mod measure_tests {
    use super::*;
    use glam::DVec3;

    #[test]
    fn areas_and_angles_of_a_box_come_out_exact() {
        let m = crate::geom::box_mesh(DVec3::ZERO, DVec3::new(40.0, 30.0, 20.0));
        // the top face: the triangles whose normal is +z
        let top: Vec<usize> = (0..m.triangles.len()).filter(|&t| m.tri_normal(&m.triangles[t]).z > 0.99).collect();
        assert_eq!(top.len(), 2);
        assert!((m.triangles_area(&top) - 1200.0).abs() < 1e-9);
        let side: Vec<usize> = (0..m.triangles.len()).filter(|&t| m.tri_normal(&m.triangles[t]).x > 0.99).collect();
        assert!((m.triangles_area(&side) - 600.0).abs() < 1e-9);
        let all: Vec<usize> = (0..m.triangles.len()).collect();
        assert!((m.triangles_area(&all) - 2.0 * (1200.0 + 800.0 + 600.0)).abs() < 1e-9, "the whole surface");
        // three points
        assert!((Mesh::angle_at(DVec3::X, DVec3::ZERO, DVec3::Y) - 90.0).abs() < 1e-9);
        assert!((Mesh::angle_at(DVec3::X, DVec3::ZERO, DVec3::new(1.0, 1.0, 0.0)) - 45.0).abs() < 1e-9);
        assert!((Mesh::angle_at(DVec3::X, DVec3::ZERO, DVec3::NEG_X) - 180.0).abs() < 1e-9);
        // faces: box walls meet at 90, a coplanar pair at 180, an obtuse wedge in between
        assert!((Mesh::angle_between_faces(DVec3::Z, DVec3::X) - 90.0).abs() < 1e-9);
        assert!((Mesh::angle_between_faces(DVec3::Z, DVec3::Z) - 180.0).abs() < 1e-9);
        let tilted = DVec3::new(1.0, 0.0, 1.0).normalize();
        assert!((Mesh::angle_between_faces(DVec3::Z, tilted) - 135.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod export_format_tests {
    use super::*;

    fn cube() -> Mesh {
        crate::geom::box_mesh(glam::DVec3::ZERO, glam::DVec3::new(10.0, 20.0, 30.0))
    }

    #[test]
    fn step_file_is_structurally_sound() {
        let (a, b) = (cube(), cube());
        let text = Mesh::write_step(&[("Тело 1", &a), ("Body 2", &b)], "part.step");
        assert!(text.starts_with("ISO-10303-21;\nHEADER;") && text.trim_end().ends_with("END-ISO-10303-21;"));
        assert!(text.contains("FILE_SCHEMA((\'AUTOMOTIVE_DESIGN") || text.contains("FILE_SCHEMA(('AUTOMOTIVE_DESIGN"));
        let data = text.split("DATA;\n").nth(1).unwrap().split("ENDSEC;").next().unwrap();
        let mut ids = Vec::new();
        for line in data.lines() {
            let (id, rest) = line.split_once('=').expect("every entity line is #n=...;");
            assert!(rest.ends_with(';'), "{line}");
            ids.push(id.trim_start_matches('#').parse::<u64>().unwrap());
            assert_eq!(rest.matches('(').count(), rest.matches(')').count(), "balanced parentheses: {line}");
        }
        assert_eq!(ids, (1..=ids.len() as u64).collect::<Vec<_>>(), "ids are 1..n without gaps");
        // every reference points at an existing entity that was defined earlier or later in the file
        for line in data.lines() {
            let rest = line.split_once('=').unwrap().1;
            let mut chars = rest.char_indices().peekable();
            while let Some((i, c)) = chars.next() {
                if c == '#' {
                    let n: String = rest[i + 1..].chars().take_while(|c| c.is_ascii_digit()).collect();
                    let n: u64 = n.parse().expect("a reference has digits");
                    assert!(n >= 1 && n <= ids.len() as u64, "dangling #{n} in {line}");
                }
            }
        }
        // 2 bodies x 12 triangles: 24 faces, 24 loops, 2 closed shells, 2 breps, 16 points
        assert_eq!(data.matches("FACE_OUTER_BOUND").count(), 24);
        assert_eq!(data.matches("POLY_LOOP").count(), 24);
        assert_eq!(data.matches("CLOSED_SHELL").count(), 2);
        assert_eq!(data.matches("FACETED_BREP(").count(), 2);
        assert!(data.contains("CARTESIAN_POINT('',(10.000000,20.000000,30.000000))"));
        // non-ASCII names are escaped, the file stays plain ASCII
        assert!(text.is_ascii(), "STEP must be ASCII");
        assert!(text.contains("\\X2\\"));
    }

    #[test]
    fn dxf_holds_one_3dface_per_triangle_with_body_layers() {
        let a = cube();
        let text = Mesh::write_dxf(&[("Тело 1", &a)]);
        assert_eq!(text.matches("3DFACE").count(), 12);
        assert!(text.contains("\n8\n") && text.contains("0\nEOF"));
        let codes: Vec<&str> = text.lines().step_by(2).collect();
        assert_eq!(codes.iter().filter(|c| c.trim() == "13").count(), 12, "every face has its fourth corner");
        assert!(text.contains("\n10\n0\n20\n0\n30\n0\n") || text.contains("\n10\n10\n") || text.contains("\n11\n"));
    }
}

