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

fn load(path: &str) -> Result<fusion_core::Document, String> {
    fusion_core::Document::from_json(&std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?)
}

fn export(doc: &fusion_core::Document, out: &str) -> Result<usize, String> {
    let mut ev = fusion_core::Evaluator::default();
    ev.evaluate(doc);
    let model = ev.model();
    let named: Vec<(String, &fusion_core::Mesh)> = model.bodies.iter().map(|b| (doc.body_name(b).to_string(), b.mesh())).collect();
    if named.is_empty() {
        return Err("the model has no bodies".into());
    }
    let ext = Path::new(out).extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let data: Vec<u8> = match ext.as_str() {
        "stl" => {
            let mut v = Vec::new();
            fusion_core::Mesh::write_stl_binary(&named.iter().map(|(_, m)| *m).collect::<Vec<_>>(), &mut v);
            v
        }
        "obj" => fusion_core::Mesh::write_obj(&named.iter().map(|(n, m)| (n.as_str(), *m)).collect::<Vec<_>>()).into_bytes(),
        "3mf" => fusion_core::Mesh::write_3mf(&named.iter().map(|(n, m)| (n.as_str(), *m)).collect::<Vec<_>>()),
        "step" | "stp" => fusion_core::Mesh::write_step(&named.iter().map(|(n, m)| (n.as_str(), *m)).collect::<Vec<_>>(), &Path::new(out).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()).into_bytes(),
        "dxf" => fusion_core::Mesh::write_dxf(&named.iter().map(|(n, m)| (n.as_str(), *m)).collect::<Vec<_>>()).into_bytes(),
        _ => return Err("export format must be .stl, .obj, .3mf, .step or .dxf".into()),
    };
    crate::files::write_atomic(Path::new(out), &data).map_err(|e| e.to_string())?;
    Ok(named.len())
}

fn print_json(v: &serde_json::Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

/// Handles the headless commands; `Some(exit code)` when the command line was one of them.
/// `--render-frames model out_dir`, `--eval model`, `--add model feature_json`, `--export model out.stl|obj|3mf`,
/// `--new out.genix`, `--features`. Results are JSON on stdout, errors go to stderr with a non-zero exit code.
pub fn run(args: &[String]) -> Option<i32> {
    let cmd = args.get(1).map(String::as_str)?;
    let arg = |i: usize| args.get(i).map(String::as_str);
    let fail = |msg: String| { eprintln!("error: {msg}"); Some(1) };
    match cmd {
        "--render-frames" => {
            let (Some(model), Some(out)) = (arg(2), arg(3)) else {
                eprintln!("usage: genix --render-frames model.genix out_dir");
                return Some(2);
            };
            Some(match render_frames(Path::new(model), Path::new(out), (WIDTH, HEIGHT), TURN_FRAMES) {
                Ok(n) => { println!("{n} frames written to {out}"); 0 }
                Err(e) => { eprintln!("error: {e}"); 1 }
            })
        }
        "--eval" => {
            let Some(model) = arg(2) else { eprintln!("usage: genix --eval model.genix"); return Some(2) };
            match load(model) {
                Ok(doc) => { let r = fusion_core::agent::report(&doc); print_json(&r); Some(if r["ok"] == true { 0 } else { 3 }) }
                Err(e) => fail(e),
            }
        }
        "--add" => {
            let (Some(model), Some(feature)) = (arg(2), arg(3)) else { eprintln!("usage: genix --add model.genix '<feature json>'  (use - to read the JSON from stdin)"); return Some(2) };
            let feature = if feature == "-" { std::io::read_to_string(std::io::stdin()).unwrap_or_default() } else { feature.to_string() };
            let mut doc = match load(model) { Ok(d) => d, Err(e) => return fail(e) };
            match fusion_core::agent::add_feature(&mut doc, &feature) {
                Ok(r) => match crate::files::write_atomic(Path::new(model), doc.to_json().as_bytes()) {
                    Ok(()) => { print_json(&r); Some(0) }
                    Err(e) => fail(e.to_string()),
                },
                Err(e) => fail(e),
            }
        }
        "--export" => {
            let (Some(model), Some(out)) = (arg(2), arg(3)) else { eprintln!("usage: genix --export model.genix out.stl|out.obj|out.3mf"); return Some(2) };
            match load(model).and_then(|d| export(&d, out)) {
                Ok(n) => { println!("{n} bodies written to {out}"); Some(0) }
                Err(e) => fail(e),
            }
        }
        "--new" => {
            let Some(out) = arg(2) else { eprintln!("usage: genix --new out.genix"); return Some(2) };
            match crate::files::write_atomic(Path::new(out), fusion_core::Document::default().to_json().as_bytes()) {
                Ok(()) => { println!("{out} created"); Some(0) }
                Err(e) => fail(e.to_string()),
            }
        }
        "--features" => { print_json(&fusion_core::agent::catalog()); Some(0) }
        // vendor tools for licence keys: the private key lives only with the vendor, never in the repository
        "--license-keygen" => {
            let Some(out) = arg(2) else { eprintln!("usage: genix --license-keygen private_key_file"); return Some(2) };
            if Path::new(out).exists() {
                return fail(format!("{out} already exists: refusing to overwrite a private key"));
            }
            match crate::license::generate_keypair() {
                Ok((private, public)) => match crate::files::write_atomic(Path::new(out), &private) {
                    Ok(()) => {
                        let hex: String = public.iter().map(|b| format!("{b:02x}")).collect();
                        println!("private key written to {out} (keep it secret and backed up)\npublic key (put it in PUBLIC_KEY_HEX): {hex}");
                        Some(0)
                    }
                    Err(e) => fail(e.to_string()),
                },
                Err(e) => fail(e),
            }
        }
        "--license-sign" => {
            // the length is days, or minutes with an «m» suffix (`90m`)
            let seconds = arg(4).and_then(|d| match d.strip_suffix('m') {
                Some(m) => m.parse::<u64>().ok().map(|m| m * 60),
                None => d.parse::<u64>().ok().map(|d| d * 86_400),
            });
            let (Some(key), Some(email), Some(seconds)) = (arg(2), arg(3), seconds) else {
                eprintln!("usage: genix --license-sign private_key_file email days|minutes+m");
                return Some(2);
            };
            let pkcs8 = match std::fs::read(key) { Ok(b) => b, Err(e) => return fail(format!("{key}: {e}")) };
            let claims = crate::license::Claims { email: email.to_string(), cid: String::new(), until: crate::license::now_unix() + seconds };
            match crate::license::sign(&pkcs8, &claims) {
                Ok(token) => { println!("{token}"); Some(0) }
                Err(e) => fail(e),
            }
        }
        "--license-check" => {
            let Some(token) = arg(2) else { eprintln!("usage: genix --license-check token"); return Some(2) };
            match crate::license::verify(token, &crate::license::public_key()) {
                Ok(c) => { println!("valid: {} until unix {}", c.email, c.until); Some(0) }
                Err(e) => fail(e),
            }
        }
        _ => None,
    }
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

    #[test]
    fn agent_commands_build_check_and_export_a_model() {
        let dir = std::env::temp_dir().join("genix-agent-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let model = dir.join("m.genix").to_string_lossy().to_string();
        let args = |v: &[&str]| std::iter::once("genix").chain(v.iter().copied()).map(String::from).collect::<Vec<_>>();
        assert_eq!(run(&args(&["--new", &model])), Some(0));
        let example = fusion_core::agent::catalog()["examples"]["Primitive/Box"].to_string();
        assert_eq!(run(&args(&["--add", &model, &example])), Some(0));
        assert_eq!(run(&args(&["--add", &model, "{broken"])), Some(1));
        assert_eq!(run(&args(&["--eval", &model])), Some(0));
        let stl = dir.join("m.stl").to_string_lossy().to_string();
        assert_eq!(run(&args(&["--export", &model, &stl])), Some(0));
        assert!(std::fs::metadata(&stl).unwrap().len() > 84);
        for ext in ["step", "dxf", "obj", "3mf"] {
            let f = dir.join(format!("m.{ext}")).to_string_lossy().to_string();
            assert_eq!(run(&args(&["--export", &model, &f])), Some(0), "{ext}");
            assert!(std::fs::metadata(&f).unwrap().len() > 100, "{ext}");
        }
        assert_eq!(run(&args(&["--export", &model, &dir.join("m.xyz").to_string_lossy()])), Some(1));
        assert_eq!(run(&args(&["--features"])), Some(0));
        assert_eq!(run(&args(&["somefile.genix"])), None, "a plain path is not a headless command");
    }
}
