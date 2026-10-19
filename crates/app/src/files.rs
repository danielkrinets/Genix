//! Выбор файлов. На компьютерах — системные диалоги; на iPad/iPhone диалогов нет:
//! документы лежат в папке приложения «Документы» (видна в приложении «Файлы»).

use std::path::{Path, PathBuf};

/// Папка документов приложения (на iOS — `~/Documents` контейнера).
pub fn documents_dir() -> Option<PathBuf> {
    let d = PathBuf::from(std::env::var_os("HOME")?).join("Documents");
    std::fs::create_dir_all(&d).ok()?;
    Some(d)
}

/// Все файлы с данными расширениями в папке документов (для iOS — «все проекты»).
pub fn documents_with(exts: &[&str]) -> Vec<String> {
    let Some(d) = documents_dir() else { return vec![] };
    let mut v: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(d)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()).is_some_and(|x| exts.contains(&x)))
        .map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), e.path().to_string_lossy().into_owned()))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|x| x.1).collect()
}

#[cfg(not(target_os = "ios"))]
pub fn open(label: &str, exts: &[&str]) -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter(label, exts).pick_file()
}

#[cfg(not(target_os = "ios"))]
pub fn save(label: &str, exts: &[&str], name: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter(label, exts).set_file_name(name).save_file()
}

/// iOS: открывать можно только из списка проектов на главной.
#[cfg(target_os = "ios")]
pub fn open(_label: &str, _exts: &[&str]) -> Option<PathBuf> {
    None
}

/// iOS: сохраняем в «Документы» под предложенным именем.
#[cfg(target_os = "ios")]
pub fn save(_label: &str, _exts: &[&str], name: &str) -> Option<PathBuf> {
    Some(documents_dir()?.join(name))
}

/// Записать рядом с назначением и заменить файл только после успешной записи.
/// NamedTempFile обеспечивает замену существующего файла также на Windows.
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    if let Ok(metadata) = std::fs::metadata(path) {
        temp.as_file().set_permissions(metadata.permissions())?;
    }
    temp.write_all(data)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

const KEEP_VERSIONS: usize = 10;

/// Folder with the saved versions of one project; the path hash keeps same-named files in different folders apart.
fn versions_dir(base: &Path, project: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    project.hash(&mut h);
    let stem = project.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    base.join(format!("{stem}-{:08x}", h.finish() as u32))
}

/// Copies the current file into its history before it is overwritten and keeps the newest `KEEP_VERSIONS`.
pub fn snapshot_version(base: &Path, project: &Path, stamp: u64) -> std::io::Result<()> {
    if !project.is_file() { return Ok(()); }
    let dir = versions_dir(base, project);
    std::fs::create_dir_all(&dir)?;
    std::fs::copy(project, dir.join(format!("{stamp}.genix")))?;
    let mut all = list_versions(base, project);
    all.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, old) in all.into_iter().skip(KEEP_VERSIONS) { let _ = std::fs::remove_file(old); }
    Ok(())
}

/// Saved versions of a project as (unix seconds, file), newest first.
pub fn list_versions(base: &Path, project: &Path) -> Vec<(u64, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(versions_dir(base, project)) else { return Vec::new() };
    let mut v: Vec<(u64, PathBuf)> = entries.flatten().filter_map(|e| {
        let stamp = e.path().file_stem()?.to_str()?.parse().ok()?;
        Some((stamp, e.path()))
    }).collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

/// Where versions live: next to the settings file.
pub fn versions_base() -> Option<PathBuf> {
    crate::settings::Settings::path().and_then(|p| p.parent().map(|d| d.join("versions")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_keep_the_newest_ten_in_order_and_skip_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("деталь.genix");
        let base = dir.path().join("versions");
        snapshot_version(&base, &project, 1).unwrap();
        assert!(list_versions(&base, &project).is_empty(), "nothing to snapshot before the first save");
        for stamp in 1..=12u64 {
            std::fs::write(&project, format!("v{stamp}")).unwrap();
            snapshot_version(&base, &project, stamp).unwrap();
        }
        let versions = list_versions(&base, &project);
        assert_eq!(versions.iter().map(|v| v.0).collect::<Vec<_>>(), (3..=12).rev().collect::<Vec<_>>());
        assert_eq!(std::fs::read_to_string(&versions[0].1).unwrap(), "v12");
    }

    #[test]
    fn atomic_save_creates_and_replaces_project() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("деталь.genix");
        write_atomic(&path, b"first project").unwrap();
        write_atomic(&path, b"updated").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"updated");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_replace_preserves_destination_and_cleans_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing");
        std::fs::create_dir(&path).unwrap();
        let original = path.join("project.genix");
        std::fs::write(&original, b"original").unwrap();
        assert!(write_atomic(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&original).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(write_atomic(&dir.path().join("missing/project.genix"), b"data").is_err());
    }
}
