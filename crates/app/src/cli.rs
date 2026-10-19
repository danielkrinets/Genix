//! Headless helper: `genix --render-frames model.genix out_dir` writes PNG frames of how the model is built
//! (one per feature) and of a 360° turntable. Used to make demo videos without driving the window.
use crate::camera::Camera;
use crate::thumb::{self, Rgba};
use std::path::Path;

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const TURN_FRAMES: usize = 96;

fn save(path: &Path, img: Rgba) -> Result<(), String> {
    let buf = image::RgbaImage::from_raw(img.w as u32, img.h as u32, img.data).ok_or("bad frame size")?;
    buf.save(path).map_err(|e| e.to_string())
}

fn frame(doc: &fusion_core::Document, yaw: f64, size: (usize, usize)) -> Option<Rgba> {
    let mut ev = fusion_core::Evaluator::default();
    ev.evaluate(doc);
    let model = ev.model();
    let meshes: Vec<&fusion_core::Mesh> = model.bodies.iter().filter(|b| !doc.hidden_bodies.contains(&b.id)).map(|b| b.mesh()).collect();
    thumb::render(&meshes, yaw, Camera::ISO.1, size.0, size.1)
}

/// Writes `build_NN.png` for NN = 1..=features and `turn_NNN.png`; returns how many files were written.
pub fn render_frames(model: &Path, out: &Path, size: (usize, usize), turn_frames: usize) -> Result<usize, String> {
    let doc = fusion_core::Document::from_json(&std::fs::read_to_string(model).map_err(|e| e.to_string())?)?;
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let mut written = 0;
    for k in 1..=doc.features.len() {
        let mut part = doc.clone();
        part.rollback = k;
        if let Some(img) = frame(&part, Camera::ISO.0 + 0.35, size) {
            save(&out.join(format!("build_{k:02}.png")), img)?;
            written += 1;
        }
    }
    for i in 0..turn_frames {
        let yaw = Camera::ISO.0 + 0.35 + std::f64::consts::TAU * i as f64 / turn_frames as f64;
        if let Some(img) = frame(&doc, yaw, size) {
            save(&out.join(format!("turn_{i:03}.png")), img)?;
            written += 1;
        }
    }
    Ok(written)
}

/// Handles `--render-frames`; `Some(exit code)` when the command line was a headless command.
pub fn run(args: &[String]) -> Option<i32> {
    if args.get(1).map(String::as_str) != Some("--render-frames") { return None; }
    let (Some(model), Some(out)) = (args.get(2), args.get(3)) else {
        eprintln!("usage: genix --render-frames model.genix out_dir");
        return Some(2);
    };
    Some(match render_frames(Path::new(model), Path::new(out), (WIDTH, HEIGHT), TURN_FRAMES) {
        Ok(n) => { println!("{n} frames written to {out}"); 0 }
        Err(e) => { eprintln!("error: {e}"); 1 }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_written_for_every_feature_and_the_turntable() {
        let dir = std::env::temp_dir().join("genix-cli-test");
        let _ = std::fs::remove_dir_all(&dir);
        let mut doc = fusion_core::Document::default();
        doc.insert(fusion_core::doc::FeatureKind::Primitive {
            shape: fusion_core::doc::Shape::Box { w: fusion_core::Expr::num(10.0), d: fusion_core::Expr::num(10.0), h: fusion_core::Expr::num(10.0) },
            at: [fusion_core::Expr::num(0.0), fusion_core::Expr::num(0.0), fusion_core::Expr::num(0.0)],
            op: fusion_core::Op::NewBody,
        });
        let model = dir.with_extension("genix");
        std::fs::write(&model, doc.to_json()).unwrap();
        assert_eq!(render_frames(&model, &dir, (64, 48), 3).unwrap(), 4);
        assert!(dir.join("build_01.png").exists() && dir.join("turn_002.png").exists());
    }
}
