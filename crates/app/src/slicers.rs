//! "Open in slicer": the model goes to a temporary file and the installed slicer app opens it.
use std::path::{Path, PathBuf};

pub struct Slicer {
    pub name: &'static str,
    /// Names of the `.app` bundle the vendor has used (first one found wins).
    pub apps: &'static [&'static str],
    /// File format the slicer reads best: "3mf" keeps millimetres and body names, "stl" is for resin slicers.
    pub format: &'static str,
}

pub const SLICERS: &[Slicer] = &[
    Slicer { name: "Bambu Studio", apps: &["BambuStudio", "Bambu Studio"], format: "3mf" },
    Slicer { name: "Creality Print", apps: &["Creality Print", "CrealityPrint"], format: "3mf" },
    Slicer { name: "Halot Box (Creality)", apps: &["HALOT BOX", "Halot Box", "HALOT-BOX"], format: "stl" },
    Slicer { name: "Anycubic Slicer Next", apps: &["AnycubicSlicerNext", "Anycubic Slicer Next", "Photon Workshop"], format: "stl" },
    Slicer { name: "OrcaSlicer", apps: &["OrcaSlicer"], format: "3mf" },
    Slicer { name: "PrusaSlicer", apps: &["PrusaSlicer"], format: "3mf" },
    Slicer { name: "UltiMaker Cura", apps: &["UltiMaker Cura", "Ultimaker Cura"], format: "stl" },
];

fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") { dirs.push(PathBuf::from(home).join("Applications")); }
    dirs
}

/// The bundle name of this slicer if it is installed, else `None`.
pub fn installed(slicer: &Slicer) -> Option<&'static str> {
    let dirs = app_dirs();
    slicer.apps.iter().copied().find(|app| dirs.iter().any(|d| d.join(format!("{app}.app")).is_dir()))
}

/// Opens `file` in the macOS app `app`.
pub fn open_in(app: &str, file: &Path) -> Result<(), String> {
    let status = std::process::Command::new("open").arg("-a").arg(app).arg(file).status().map_err(|e| e.to_string())?;
    if status.success() { Ok(()) } else { Err(format!("open: {status}")) }
}

/// Temporary file the slicer will read; one fixed folder, overwritten on each export.
pub fn temp_file(name: &str, format: &str) -> std::io::Result<PathBuf> {
    let dir = std::env::temp_dir().join("genix-slicer");
    std::fs::create_dir_all(&dir)?;
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    Ok(dir.join(format!("{clean}.{format}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sane_and_missing_apps_are_not_reported() {
        assert!(SLICERS.iter().all(|s| !s.apps.is_empty() && matches!(s.format, "3mf" | "stl")));
        let ghost = Slicer { name: "Ghost", apps: &["No Such Slicer 123"], format: "stl" };
        assert_eq!(installed(&ghost), None);
    }

    #[test]
    fn temp_names_are_filesystem_safe() {
        let p = temp_file("Кронштейн / v2", "stl").unwrap();
        assert!(p.to_string_lossy().ends_with("Кронштейн___v2.stl"));
    }
}
