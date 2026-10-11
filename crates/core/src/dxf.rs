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
        }
        let _ = i;
    }
    s.push_str("0\nENDSEC\n0\nEOF\n");
    s
}

/// Прочитать DXF и добавить кривые в чертёж. Возвращает число добавленных кривых.
pub fn import(sk: &mut Sketch, text: &str) -> Result<usize, String> {
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
    let num = |d: &[(i32, String)], code: i32| d.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.parse::<f64>().ok());
    let mut added = 0;
    for (name, d) in &ents {
        let cons = d.iter().any(|(c, v)| *c == 8 && v.to_uppercase().contains("CONSTR"));
        match name.as_str() {
            "LINE" => {
                if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (num(d, 10), num(d, 20), num(d, 11), num(d, 21)) {
                    let l = sk.add_line([x1, y1], [x2, y2]);
                    sk.entities[l].set_construction(cons);
                    added += 1;
                }
            }
            "CIRCLE" => {
                if let (Some(x), Some(y), Some(r)) = (num(d, 10), num(d, 20), num(d, 40)) {
                    let e = sk.add_circle([x, y], r);
                    sk.entities[e].set_construction(cons);
                    added += 1;
                }
            }
            "ARC" => {
                if let (Some(x), Some(y), Some(r), Some(a0), Some(a1)) = (num(d, 10), num(d, 20), num(d, 40), num(d, 50), num(d, 51)) {
                    let pt = |a: f64| [x + r * a.to_radians().cos(), y + r * a.to_radians().sin()];
                    let (c, a, b) = (sk.add_point([x, y]), sk.add_point(pt(a0)), sk.add_point(pt(a1)));
                    sk.add(Entity::Arc { c, a, b, construction: cons });
                    added += 1;
                }
            }
            "LWPOLYLINE" => {
                let mut xs = vec![];
                let mut cur: Option<f64> = None;
                for (c, v) in d {
                    match *c {
                        10 => cur = v.parse().ok(),
                        20 => {
                            if let (Some(x), Ok(y)) = (cur.take(), v.parse::<f64>()) {
                                xs.push([x, y]);
                            }
                        }
                        _ => {}
                    }
                }
                let closed = num(d, 70).map(|f| (f as i32) & 1 == 1).unwrap_or(false);
                if xs.len() >= 2 {
                    let ids: Vec<usize> = xs.iter().map(|&p| sk.add_point(p)).collect();
                    let n = if closed { ids.len() } else { ids.len() - 1 };
                    for i in 0..n {
                        sk.add(Entity::Line { a: ids[i], b: ids[(i + 1) % ids.len()], construction: cons });
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

    #[test]
    fn polyline_and_garbage() {
        let mut s = Sketch::default();
        let t = "0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n3\n70\n1\n10\n0\n20\n0\n10\n10\n20\n0\n10\n5\n20\n8\n0\nENDSEC\n0\nEOF\n";
        assert_eq!(import(&mut s, t).unwrap(), 3);
        assert!(import(&mut s, "hello").is_err());
    }
}
