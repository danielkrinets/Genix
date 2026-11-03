//! Sketch text: the letters of a string as closed outlines, ready to be extruded or cut like any other profile.
//! The font is the DejaVu Sans that ships with the app (Latin, Cyrillic, Greek, Hebrew and more).
use crate::sketch::{Region, P2};
use ab_glyph::{Font, FontRef, OutlineCurve, ScaleFont};
use serde::{Deserialize, Serialize};

static FONT_BYTES: &[u8] = include_bytes!("../../app/assets/fonts/DejaVuSans.ttf");

/// A line of text in a sketch: `pos` is the start of the baseline, `height` the font size in mm, `angle` in degrees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchText {
    pub text: String,
    pub pos: P2,
    pub height: f64,
    #[serde(default)]
    pub angle: f64,
}

/// Упрощённый двунаправленный порядок: иврит справа налево, числа и латиница слева направо.
/// egui рисует символы слева направо, поэтому порядок серий меняем заранее.
pub fn visual_rtl(s: &str) -> String {
    let is_he = |c: char| ('\u{0590}'..='\u{05FF}').contains(&c);
    if !s.chars().any(is_he) {
        return s.to_string();
    }
    let s = s.replace("{}", "\u{E000}");
    let chars: Vec<char> = s.chars().collect();
    // 0 = влево-вправо, 1 = справа-налево
    let strong: Vec<Option<u8>> = chars.iter().map(|&c| if is_he(c) { Some(1) } else if c.is_alphanumeric() || c == '\u{E000}' { Some(0) } else { None }).collect();
    let mut dir = vec![1u8; chars.len()];
    for i in 0..chars.len() {
        dir[i] = match strong[i] {
            Some(d) => d,
            None => {
                let prev = (0..i).rev().find_map(|j| strong[j]);
                let next = (i + 1..chars.len()).find_map(|j| strong[j]);
                if prev == Some(0) && next == Some(0) { 0 } else { 1 }
            }
        };
    }
    let mut runs: Vec<(u8, String)> = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if let Some(last) = runs.last_mut().filter(|r| r.0 == dir[i]) {
            last.1.push(c);
        } else {
            runs.push((dir[i], c.to_string()));
        }
    }
    let mirror = |c: char| match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '«' => '»',
        '»' => '«',
        c => c,
    };
    let mut out = String::new();
    for (d, run) in runs.into_iter().rev() {
        if d == 1 {
            out.extend(run.chars().rev().map(mirror));
        } else {
            out.push_str(&run);
        }
    }
    out.replace('\u{E000}', "{}")
}

fn flatten(curve: &OutlineCurve, out: &mut Vec<[f64; 2]>) {
    let pt = |p: ab_glyph::Point| [p.x as f64, p.y as f64];
    match curve {
        OutlineCurve::Line(_, b) => out.push(pt(*b)),
        OutlineCurve::Quad(a, c, b) => {
            let (a, c, b) = (pt(*a), pt(*c), pt(*b));
            for i in 1..=8 {
                let t = i as f64 / 8.0;
                let u = 1.0 - t;
                out.push([u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0], u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1]]);
            }
        }
        OutlineCurve::Cubic(a, c1, c2, b) => {
            let (a, c1, c2, b) = (pt(*a), pt(*c1), pt(*c2), pt(*b));
            for i in 1..=10 {
                let t = i as f64 / 10.0;
                let u = 1.0 - t;
                let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
                out.push([w[0] * a[0] + w[1] * c1[0] + w[2] * c2[0] + w[3] * b[0], w[0] * a[1] + w[1] * c1[1] + w[2] * c2[1] + w[3] * b[1]]);
            }
        }
    }
}

fn area(l: &[P2]) -> f64 {
    0.5 * (0..l.len()).map(|i| l[i][0] * l[(i + 1) % l.len()][1] - l[(i + 1) % l.len()][0] * l[i][1]).sum::<f64>()
}

fn inside(poly: &[P2], p: P2) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] {
            c = !c;
        }
        j = i;
    }
    c
}

/// All closed contours of the string in sketch coordinates (baseline start at the origin, before rotation and shift).
fn contours(text: &str, height: f64) -> Vec<Vec<P2>> {
    let Ok(font) = FontRef::try_from_slice(FONT_BYTES) else { return Vec::new() };
    // Hebrew runs are laid out right to left; points (niqqud, cantillation) are dropped: the font has no mark positioning
    let text: String = visual_rtl(text).chars().filter(|c| !('\u{0591}'..='\u{05C7}').contains(c)).collect();
    let text = text.as_str();
    let units = font.units_per_em().unwrap_or(2048.0) as f64;
    let scale = height / units;
    let scaled = font.as_scaled(ab_glyph::PxScale::from(units as f32));
    let mut out = Vec::new();
    let mut x = 0.0f64;
    let mut prev = None;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(p) = prev {
            x += scaled.kern(p, id) as f64;
        }
        if let Some(outline) = font.outline(id) {
            let mut cur: Vec<[f64; 2]> = Vec::new();
            let mut start = [0.0f64; 2];
            for curve in &outline.curves {
                let first = match curve {
                    OutlineCurve::Line(a, _) | OutlineCurve::Quad(a, _, _) | OutlineCurve::Cubic(a, _, _, _) => [a.x as f64, a.y as f64],
                };
                let continues = cur.last().is_some_and(|l| (l[0] - first[0]).abs() < 1e-3 && (l[1] - first[1]).abs() < 1e-3);
                if !continues {
                    if cur.len() >= 3 {
                        out.push(cur.iter().map(|p| [(p[0] + x) * scale, p[1] * scale]).collect::<Vec<_>>());
                    }
                    cur = vec![first];
                    start = first;
                }
                flatten(curve, &mut cur);
            }
            if cur.len() >= 3 {
                if let (Some(l), true) = (cur.last(), true) {
                    if (l[0] - start[0]).abs() < 1e-3 && (l[1] - start[1]).abs() < 1e-3 {
                        cur.pop();
                    }
                }
                out.push(cur.iter().map(|p| [(p[0] + x) * scale, p[1] * scale]).collect::<Vec<_>>());
            }
        }
        x += scaled.h_advance(id) as f64;
        prev = Some(id);
    }
    out.retain(|c| c.len() >= 3 && area(c).abs() > 1e-9);
    out
}

/// Regions of a text: every letter body with its counters (the hole in "o", "а", "8") as holes.
pub fn regions(t: &SketchText) -> Vec<Region> {
    if t.height <= 0.0 || t.text.trim().is_empty() {
        return Vec::new();
    }
    let (s, c) = t.angle.to_radians().sin_cos();
    let placed: Vec<Vec<P2>> = contours(&t.text, t.height).into_iter().map(|l| l.into_iter().map(|p| [t.pos[0] + p[0] * c - p[1] * s, t.pos[1] + p[0] * s + p[1] * c]).collect()).collect();
    let depth: Vec<usize> = (0..placed.len()).map(|i| (0..placed.len()).filter(|&j| j != i && inside(&placed[j], placed[i][0])).count()).collect();
    let mut regions: Vec<(usize, Region)> = Vec::new();
    for i in 0..placed.len() {
        if depth[i] % 2 == 0 {
            let mut outer = placed[i].clone();
            if area(&outer) < 0.0 {
                outer.reverse();
            }
            regions.push((i, Region { outer, holes: Vec::new(), ids: Vec::new() }));
        }
    }
    for i in 0..placed.len() {
        if depth[i] % 2 == 1 {
            // the hole belongs to the smallest outer contour that contains it
            let owner = regions.iter().enumerate().filter(|(_, (j, _))| inside(&placed[*j], placed[i][0])).min_by(|a, b| area(&placed[a.1 .0]).abs().total_cmp(&area(&placed[b.1 .0]).abs())).map(|(k, _)| k);
            if let Some(k) = owner {
                let mut hole = placed[i].clone();
                if area(&hole) > 0.0 {
                    hole.reverse();
                }
                regions[k].1.holes.push(hole);
            }
        }
    }
    regions.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hebrew_is_laid_out_right_to_left_and_numbers_stay_readable() {
        assert_eq!(visual_rtl("שלום"), "םולש");
        assert_eq!(visual_rtl("abc"), "abc");
        // the first Hebrew letter ends up on the right: the outline of "שלום" is the mirror order of the logical string
        let he = regions(&SketchText { text: "של".into(), pos: [0.0, 0.0], height: 10.0, angle: 0.0 });
        let logical_first = regions(&SketchText { text: "ש".into(), pos: [0.0, 0.0], height: 10.0, angle: 0.0 });
        let right_edge = |rs: &[Region]| rs.iter().flat_map(|r| r.outer.iter()).map(|p| p[0]).fold(f64::MIN, f64::max);
        let shin_width = right_edge(&logical_first);
        assert!(right_edge(&he) > shin_width, "a second letter lies to the left or right of the first, never on top of it");
        let leftmost_shin = regions(&SketchText { text: "ש".into(), pos: [0.0, 0.0], height: 10.0, angle: 0.0 }).iter().flat_map(|r| r.outer.iter()).map(|p| p[0]).fold(f64::MAX, f64::min);
        let leftmost_he = he.iter().flat_map(|r| r.outer.iter()).map(|p| p[0]).fold(f64::MAX, f64::min);
        assert!(leftmost_he >= leftmost_shin - 1e-9);
        // a Hebrew word has regions (the font covers the block) and mixed text keeps the digits in order
        assert!(!regions(&SketchText { text: "שלום 123".into(), pos: [0.0, 0.0], height: 10.0, angle: 0.0 }).is_empty());
        assert_eq!(visual_rtl("גופים 12"), "12 םיפוג");
        // niqqud do not add stray regions
        let plain = regions(&SketchText { text: "שלום".into(), pos: [0.0, 0.0], height: 10.0, angle: 0.0 }).len();
        let pointed = regions(&SketchText { text: "שָׁלוֹם".into(), pos: [0.0, 0.0], height: 10.0, angle: 0.0 }).len();
        assert_eq!(plain, pointed);
    }

    fn t(text: &str, height: f64) -> SketchText {
        SketchText { text: text.into(), pos: [0.0, 0.0], height, angle: 0.0 }
    }

    #[test]
    fn letters_become_regions_and_counters_become_holes() {
        // "I" is one solid bar, "O" is a ring: one region with one hole
        assert_eq!(regions(&t("I", 10.0)).len(), 1);
        let o = regions(&t("O", 10.0));
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].holes.len(), 1);
        // "8" has two counters, "ii" has two letters with a dot each
        assert_eq!(regions(&t("8", 10.0))[0].holes.len(), 2);
        assert_eq!(regions(&t("ii", 10.0)).len(), 4);
        // Cyrillic works with the bundled font
        assert_eq!(regions(&t("Ф", 10.0)).len(), 1);
        assert!(regions(&t("   ", 10.0)).is_empty());
        assert!(regions(&t("A", 0.0)).is_empty());
    }

    #[test]
    fn size_position_and_rotation_follow_the_settings() {
        let small = regions(&t("H", 10.0));
        let big = regions(&t("H", 20.0));
        let ratio = big[0].area() / small[0].area();
        assert!((ratio - 4.0).abs() < 0.01, "area scales with the square of the height: {ratio}");
        let moved = SketchText { pos: [100.0, 50.0], ..t("H", 10.0) };
        let b = regions(&moved)[0].outer.iter().fold([f64::MAX, f64::MAX], |m, p| [m[0].min(p[0]), m[1].min(p[1])]);
        assert!(b[0] > 99.0 && b[1] > 49.0, "{b:?}");
        let turned = SketchText { angle: 90.0, ..t("H", 10.0) };
        let (w, h) = {
            let o = &regions(&turned)[0].outer;
            let (xs, ys): (Vec<f64>, Vec<f64>) = (o.iter().map(|p| p[0]).collect(), o.iter().map(|p| p[1]).collect());
            (xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max) - ys.iter().cloned().fold(f64::MAX, f64::min))
        };
        assert!(w > h, "an H turned by 90 degrees lies on its side: {w} x {h}");
    }
}
