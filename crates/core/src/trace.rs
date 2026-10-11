//! Превращение растрового изображения (фото, рисунок) в контуры для чертежа.

use crate::sketch::{Entity, Sketch, P2};
use std::collections::HashMap;

/// Параметры трассировки.
#[derive(Debug, Clone, Copy)]
pub struct TraceOpts {
    /// Порог яркости 0..255: пиксели темнее порога — «фигура».
    pub threshold: u8,
    /// Наоборот: фигура — светлые пиксели.
    pub invert: bool,
    /// Допуск упрощения контура, в пикселях.
    pub tolerance: f64,
    /// Контуры меньше этой площади (в пикселях) считаются шумом.
    pub min_area: f64,
}

impl Default for TraceOpts {
    fn default() -> Self {
        TraceOpts { threshold: 128, invert: false, tolerance: 1.5, min_area: 30.0 }
    }
}

/// Замкнутые контуры в пикселях (y вниз). Дырки — отдельные контуры с обратным обходом.
pub fn trace(gray: &[u8], w: usize, h: usize, o: &TraceOpts) -> Vec<Vec<P2>> {
    let fg = |x: i64, y: i64| -> bool {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return false;
        }
        (gray[y as usize * w + x as usize] <= o.threshold) != o.invert
    };
    // ориентированные рёбра между углами пикселей; фигура справа от ребра
    let mut next: HashMap<(i64, i64), Vec<(i64, i64)>> = HashMap::new();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if !fg(x, y) {
                continue;
            }
            if !fg(x, y - 1) {
                next.entry((x, y)).or_default().push((x + 1, y));
            }
            if !fg(x + 1, y) {
                next.entry((x + 1, y)).or_default().push((x + 1, y + 1));
            }
            if !fg(x, y + 1) {
                next.entry((x + 1, y + 1)).or_default().push((x, y + 1));
            }
            if !fg(x - 1, y) {
                next.entry((x, y + 1)).or_default().push((x, y));
            }
        }
    }
    let mut keys: Vec<(i64, i64)> = next.keys().copied().collect();
    keys.sort();
    let mut loops = Vec::new();
    for k in keys {
        while let Some(&first) = next.get(&k).and_then(|v| v.last()) {
            let mut pts = vec![k];
            let mut cur = k;
            let mut to = first;
            loop {
                let v = next.get_mut(&cur).unwrap();
                v.pop();
                if v.is_empty() {
                    next.remove(&cur);
                }
                cur = to;
                if cur == k {
                    break;
                }
                pts.push(cur);
                match next.get(&cur).and_then(|v| v.last()) {
                    Some(&n) => to = n,
                    None => break,
                }
            }
            if pts.len() >= 4 {
                let f: Vec<P2> = pts.iter().map(|&(x, y)| [x as f64, y as f64]).collect();
                if area(&f).abs() >= o.min_area {
                    let s = simplify_closed(&f, o.tolerance);
                    if s.len() >= 3 {
                        loops.push(s);
                    }
                }
            }
        }
    }
    loops
}

fn area(p: &[P2]) -> f64 {
    let n = p.len();
    (0..n).map(|i| p[i][0] * p[(i + 1) % n][1] - p[(i + 1) % n][0] * p[i][1]).sum::<f64>() / 2.0
}

fn dist_to_seg(p: P2, a: P2, b: P2) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 < 1e-18 { 0.0 } else { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) };
    ((p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)).sqrt()
}

fn rdp(pts: &[P2], tol: f64, out: &mut Vec<P2>) {
    // pts — открытая цепочка; первая точка уже в out, добавляем остальные
    let (a, b) = (pts[0], *pts.last().unwrap());
    let (mut best, mut bi) = (0.0, 0);
    for (i, p) in pts.iter().enumerate().take(pts.len() - 1).skip(1) {
        let d = dist_to_seg(*p, a, b);
        if d > best {
            best = d;
            bi = i;
        }
    }
    if best > tol {
        rdp(&pts[..=bi], tol, out);
        rdp(&pts[bi..], tol, out);
    } else {
        out.push(b);
    }
}

/// Упрощение замкнутого контура (Дуглас–Пекер по двум половинам).
pub fn simplify_closed(p: &[P2], tol: f64) -> Vec<P2> {
    if p.len() < 4 || tol <= 0.0 {
        return p.to_vec();
    }
    let far = (1..p.len())
        .max_by(|&i, &j| {
            let d = |k: usize| (p[k][0] - p[0][0]).powi(2) + (p[k][1] - p[0][1]).powi(2);
            d(i).partial_cmp(&d(j)).unwrap()
        })
        .unwrap();
    let mut first = p[..=far].to_vec();
    let mut second = p[far..].to_vec();
    second.push(p[0]);
    first.dedup();
    let mut out = vec![p[0]];
    rdp(&first, tol, &mut out);
    let mut o2 = vec![];
    rdp(&second, tol, &mut o2);
    out.extend(o2);
    out.pop(); // последняя точка повторяет первую
    out
}

impl Sketch {
    /// Добавить замкнутую ломаную как цепочку линий с общими вершинами.
    pub fn add_loop(&mut self, pts: &[P2]) {
        if pts.len() < 3 {
            return;
        }
        let ids: Vec<usize> = pts.iter().map(|&p| self.add_point(p)).collect();
        for i in 0..ids.len() {
            self.add(Entity::Line { a: ids[i], b: ids[(i + 1) % ids.len()], construction: false });
        }
    }
}

/// Контуры из пикселей → координаты чертежа (мм): центр картинки в нуле, большая сторона = `size`.
pub fn to_sketch_units(loops: &[Vec<P2>], w: usize, h: usize, size: f64) -> Vec<Vec<P2>> {
    let k = size / (w.max(h) as f64);
    let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
    loops.iter().map(|l| l.iter().map(|p| [(p[0] - cx) * k, (cy - p[1]) * k]).collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_with_hole() {
        let (w, h) = (40, 40);
        let mut g = vec![255u8; w * h];
        for y in 5..35 {
            for x in 5..35 {
                g[y * w + x] = 0;
            }
        }
        for y in 15..25 {
            for x in 15..25 {
                g[y * w + x] = 255;
            }
        }
        let loops = trace(&g, w, h, &TraceOpts::default());
        assert_eq!(loops.len(), 2);
        assert!(loops.iter().all(|l| l.len() == 4), "{:?}", loops);
        let mut s = Sketch::default();
        for l in to_sketch_units(&loops, w, h, 40.0) {
            s.add_loop(&l);
        }
        let r = s.regions();
        assert!(r.iter().any(|x| (x.area() - 800.0).abs() < 1e-6), "{:?}", r.iter().map(|x| x.area()).collect::<Vec<_>>());
    }

    #[test]
    fn disc_simplifies_to_polygon() {
        let (w, h) = (100, 100);
        let mut g = vec![255u8; w * h];
        for y in 0..h {
            for x in 0..w {
                if ((x as f64 - 50.0).powi(2) + (y as f64 - 50.0).powi(2)).sqrt() < 30.0 {
                    g[y * w + x] = 10;
                }
            }
        }
        let loops = trace(&g, w, h, &TraceOpts::default());
        assert_eq!(loops.len(), 1);
        assert!(loops[0].len() >= 8 && loops[0].len() < 80, "{}", loops[0].len());
    }
}
