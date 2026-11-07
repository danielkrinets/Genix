//! Обмен чертежами с другими программами через DXF (текстовый, ASCII): линии, окружности, дуги, ломаные.

use crate::sketch::{Entity, Sketch};
use glam::DVec2;
use std::fmt::Write;

/// Записать чертёж в DXF (миллиметры).
pub fn export(sk: &Sketch) -> String {
    let mut s = String::new();
    let _ = write!(s, "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n");
    for (i, e) in sk.entities.iter().enumerate() {
        let layer = if e.construction() { "CONSTRUCTION" } else { "0" };
        match *e {
            Entity::Line { a, b, .. } => {
                let (p, q) = (sk.p(a), sk.p(b));
                let _ = write!(s, "0\nLINE\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n11\n{}\n21\n{}\n31\n0\n", p.x, p.y, q.x, q.y);
            }
            Entity::Circle { c, r, .. } => {
                let c = sk.p(c);
                let _ = write!(s, "0\nCIRCLE\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n40\n{}\n", c.x, c.y, r);
            }
            Entity::Arc { c, a, b, .. } => {
                let (cc, pa, pb) = (sk.p(c), sk.p(a), sk.p(b));
                let ang = |p: DVec2| (p - cc).y.atan2((p - cc).x).to_degrees().rem_euclid(360.0);
                let _ = write!(s, "0\nARC\n8\n{layer}\n10\n{}\n20\n{}\n30\n0\n40\n{}\n50\n{}\n51\n{}\n", cc.x, cc.y, (pa - cc).length(), ang(pa), ang(pb));
            }
            // ellipses and splines leave as polylines (LWPOLYLINE) of their sampled outline
            Entity::Ellipse { .. } | Entity::Spline { .. } => {
                let mut pts = sk.tessellate(i);
                let closed = matches!(*e, Entity::Ellipse { .. } | Entity::Spline { closed: true, .. });
                // the closed flag already joins the last vertex to the first
                if closed && pts.len() > 2 && (DVec2::from(pts[0]) - DVec2::from(pts[pts.len() - 1])).length() < 1e-9 {
                    pts.pop();
                }
                let _ = write!(s, "0\nLWPOLYLINE\n8\n{layer}\n90\n{}\n70\n{}\n", pts.len(), if closed { 1 } else { 0 });
                for p in &pts {
                    let _ = write!(s, "10\n{}\n20\n{}\n", p[0], p[1]);
                }
            }
        }
        let _ = i;
    }
    // letters of sketch text leave as closed outlines, so they survive in other programs and come back as regions
    for t in &sk.texts {
        for r in crate::text::regions(t) {
            for ring in std::iter::once(&r.outer).chain(r.holes.iter()) {
                let _ = write!(s, "0\nLWPOLYLINE\n8\n0\n90\n{}\n70\n1\n", ring.len());
                for p in ring {
                    let _ = write!(s, "10\n{}\n20\n{}\n", p[0], p[1]);
                }
            }
        }
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

/// Прочитать DXF и добавить кривые в чертёж. Возвращает число добавленных кривых.
pub fn import(sk: &mut Sketch, text: &str) -> Result<usize, String> {
    // Windows programs often start the file with a UTF-8 byte order mark
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines().map(|l| l.trim());
    let mut pairs: Vec<(i32, String)> = Vec::new();
    while let (Some(c), Some(v)) = (lines.next(), lines.next()) {
        let code: i32 = c.parse().map_err(|_| "это не текстовый DXF-файл".to_string())?;
        pairs.push((code, v.to_string()));
    }
    // разбить на сущности внутри секции ENTITIES
    let mut in_entities = false;
    let mut ents: Vec<(String, Vec<(i32, String)>)> = Vec::new();
    for (i, (c, v)) in pairs.iter().enumerate() {
        if *c == 0 {
            match v.as_str() {
                "SECTION" => {
                    in_entities = pairs.get(i + 1).map(|(c2, v2)| *c2 == 2 && v2 == "ENTITIES").unwrap_or(false);
                }
                "ENDSEC" => in_entities = false,
                "EOF" => break,
                name if in_entities => ents.push((name.to_string(), vec![])),
                _ => {}
            }
        } else if in_entities {
            if let Some(last) = ents.last_mut() {
                last.1.push((*c, v.clone()));
            }
        }
    }
    // non-finite values (nan, inf, 1e999) would be saved as null and make the project unreadable
    let num = |d: &[(i32, String)], code: i32| d.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.parse::<f64>().ok()).filter(|v| v.is_finite());
    // curves imported together share coincident end points, so their corners can be filleted and dragged as one
    let mut shared: Vec<usize> = Vec::new();
    let mut point = |sk: &mut Sketch, p: [f64; 2]| -> usize {
        if let Some(&i) = shared.iter().find(|&&i| (sk.p(i) - DVec2::from(p)).length() < 1e-9) {
            return i;
        }
        let i = sk.add_point(p);
        shared.push(i);
        i
    };
    let mut added = 0;
    for (name, d) in &ents {
        let cons = d.iter().any(|(c, v)| *c == 8 && v.to_uppercase().contains("CONSTR"));
        match name.as_str() {
            "LINE" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (num(d, 10), num(d, 20), num(d, 11), num(d, 21)) {
                    if (x1 - x2).abs() > 1e-12 || (y1 - y2).abs() > 1e-12 {
                        let (a, b) = (point(sk, [x1, y1]), point(sk, [x2, y2]));
                        sk.add(Entity::Line { a, b, construction: cons });
                        added += 1;
                    }
                }
            }
            "CIRCLE" => {
                if let (Some(x), Some(y), Some(r)) = (num(d, 10), num(d, 20), num(d, 40).filter(|r| *r > 0.0)) {
                    let e = sk.add_circle([x, y], r);
                    sk.entities[e].set_construction(cons);
                    added += 1;
                }
            }
            "ARC" => {
                if let (Some(x), Some(y), Some(r), Some(a0), Some(a1)) = (num(d, 10), num(d, 20), num(d, 40).filter(|r| *r > 0.0), num(d, 50), num(d, 51)) {
                    let sweep = (a1 - a0).rem_euclid(360.0);
                    if sweep < 1e-9 || 360.0 - sweep < 1e-9 {
                        // an arc from 0 to 360 degrees is a whole circle
                        let e = sk.add_circle([x, y], r);
                        sk.entities[e].set_construction(cons);
                    } else {
                        let pt = |a: f64| [x + r * a.to_radians().cos(), y + r * a.to_radians().sin()];
                        let c = sk.add_point([x, y]);
                        let (a, b) = (point(sk, pt(a0)), point(sk, pt(a1)));
                        sk.add(Entity::Arc { c, a, b, construction: cons });
                    }
                    added += 1;
                }
            }
            "LWPOLYLINE" => {
                let mut xs = vec![];
                let mut cur: Option<f64> = None;
                let mut bulges = vec![];
                for (c, v) in d {
                    match *c {
                        10 => cur = v.parse().ok().filter(|x: &f64| x.is_finite()),
                        20 => {
                            if let (Some(x), Some(y)) = (cur.take(), v.parse::<f64>().ok().filter(|y| y.is_finite())) {
                                // a repeated vertex would make a zero-length line
                                if xs.last() != Some(&[x, y]) {
                                    xs.push([x, y]);
                                    bulges.push(0.0_f64);
                                }
                            }
                        }
                        42 => {
                            if let Some(bulge) = bulges.last_mut() {
                                *bulge = v.parse().unwrap_or(0.0);
                            }
                        }
                        _ => {}
                    }
                }
                let closed = num(d, 70).map(|f| (f as i32) & 1 == 1).unwrap_or(false);
                // a closed outline that also repeats its first vertex at the end
                if closed && xs.len() > 2 && xs.first() == xs.last() {
                    xs.pop();
                    bulges.pop();
                }
                if xs.len() >= 2 {
                    let ids: Vec<usize> = xs.iter().map(|&p| point(sk, p)).collect();
                    let n = if closed { ids.len() } else { ids.len() - 1 };
                    for i in 0..n {
                        let (a, b) = (ids[i], ids[(i + 1) % ids.len()]);
                        let bulge = bulges[i];
                        if bulge == 0.0 || !bulge.is_finite() || sk.p(a) == sk.p(b) {
                            sk.add(Entity::Line { a, b, construction: cons });
                        } else {
                            // DXF bulge = tan(sweep / 4), signed by traversal direction.
                            let chord = sk.p(b) - sk.p(a);
                            let center = (sk.p(a) + sk.p(b)) * 0.5
                                + DVec2::new(-chord.y, chord.x) * ((1.0 / bulge - bulge) * 0.25);
                            let c = sk.add_point(center.to_array());
                            // Sketch arcs are always counterclockwise.
                            let (a, b) = if bulge < 0.0 { (b, a) } else { (a, b) };
                            sk.add(Entity::Arc { c, a, b, construction: cons });
                        }
                        added += 1;
                    }
                }
            }
            _ => {}
        }
    }
    if added == 0 {
        return Err("в файле нет линий, окружностей или дуг".into());
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut s = Sketch::default();
        s.add_rect([0.0, 0.0], [20.0, 10.0]);
        s.add_circle([5.0, 5.0], 2.0);
        let c = s.add_point([40.0, 0.0]);
        let (a, b) = (s.add_point([50.0, 0.0]), s.add_point([40.0, 10.0]));
        s.add(Entity::Arc { c, a, b, construction: false });
        let t = export(&s);
        let mut s2 = Sketch::default();
        assert_eq!(import(&mut s2, &t).unwrap(), 6);
        assert_eq!(s2.entities.len(), 6);
        let arc = s2.entities.iter().find(|e| matches!(e, Entity::Arc { .. })).unwrap();
        if let Entity::Arc { c, a, b, .. } = *arc {
            assert!((s2.p(a) - s2.p(c)).length() - 10.0 < 1e-9);
            assert!((s2.p(b) - glam::DVec2::new(40.0, 10.0)).length() < 1e-6);
        }
    }

    fn polyline(vertices: &str, closed: bool) -> String {
        format!("0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\nCONSTRUCTION\n70\n{}\n{vertices}0\nENDSEC\n0\nEOF\n", u8::from(closed))
    }

    #[test]
    fn polyline_bulge_direction_and_sweep() {
        for bulge in [1.0_f64, -1.0, 0.5, -0.5, 2.0, -2.0] {
            let mut s = Sketch::default();
            let text = polyline(&format!("10\n0\n20\n0\n42\n{bulge}\n10\n10\n20\n0\n"), false);
            assert_eq!(import(&mut s, &text).unwrap(), 1);
            let Entity::Arc { c, a, b, construction } = s.entities[0] else { panic!("expected arc") };
            assert!(construction);
            let radius = 2.5 * (bulge.abs() + 1.0 / bulge.abs());
            assert!((s.p(a).distance(s.p(c)) - radius).abs() < 1e-9);
            assert!((s.p(b).distance(s.p(c)) - radius).abs() < 1e-9);
            let points = s.tessellate(0);
            let middle = DVec2::from_array(points[points.len() / 2]);
            assert!((middle.y + 5.0 * bulge).abs() < 0.1);
            assert_eq!(s.p(a).x, if bulge > 0.0 { 0.0 } else { 10.0 });
        }
    }

    #[test]
    fn closing_bulge_and_shared_endpoints() {
        let vertices = "10\n0\n20\n0\n10\n10\n20\n0\n42\n1\n";
        let mut open = Sketch::default();
        assert_eq!(import(&mut open, &polyline(vertices, false)).unwrap(), 1);
        assert!(matches!(open.entities[0], Entity::Line { .. }));
        let mut closed = Sketch::default();
        assert_eq!(import(&mut closed, &polyline(vertices, true)).unwrap(), 2);
        let Entity::Line { a: la, b: lb, .. } = closed.entities[0] else { panic!("expected line") };
        let Entity::Arc { a, b, .. } = closed.entities[1] else { panic!("expected closing arc") };
        assert_eq!((a, b), (lb, la));
        // Exporting as individual DXF arcs preserves the curved geometry too.
        let mut restored = Sketch::default();
        assert_eq!(import(&mut restored, &export(&closed)).unwrap(), 2);
        assert!(matches!(restored.entities[1], Entity::Arc { .. }));
    }

    #[test]
    fn polyline_and_garbage() {
        let mut s = Sketch::default();
        let t = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n3\n70\n1\n10\n0\n20\n0\n10\n10\n20\n0\n10\n5\n20\n8\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(import(&mut s, t).unwrap(), 3);
        assert!(import(&mut s, "hello").is_err());
    }
}
