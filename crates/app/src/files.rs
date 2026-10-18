//! Выбор файлов. На компьютерах — системные диалоги; на iPad/iPhone диалогов нет:
//! документы лежат в папке приложения «Документы» (видна в приложении «Файлы»).

use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::*;

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
