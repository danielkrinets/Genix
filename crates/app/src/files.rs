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
