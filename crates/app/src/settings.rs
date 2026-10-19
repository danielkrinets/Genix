//! Настройки программы: хранятся в файле рядом с другими настройками приложений пользователя.

use crate::i18n::Lang;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeChoice {
    /// Как в системе.
    #[default]
    Auto,
    Light,
    Dark,
}

/// Схема управления мышью и клавиатурой.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ControlScheme {
    /// Левая кнопка — вращение, правая — сдвиг.
    #[default]
    Standard,
    /// Как в Blender: средняя кнопка — вращение, Shift — сдвиг, Ctrl — масштаб, цифровая клавиатура — виды, G/S/N/T.
    Blender,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VisualStyle {
    /// Тела с рёбрами.
    #[default]
    Shaded,
    /// Тела без обводки рёбер.
    ShadedNoEdges,
    /// Только рёбра.
    Wireframe,
}

impl VisualStyle {
    pub const ALL: [VisualStyle; 3] = [VisualStyle::Shaded, VisualStyle::ShadedNoEdges, VisualStyle::Wireframe];
    pub fn label(self) -> &'static str {
        match self {
            VisualStyle::Shaded => "Тела с рёбрами",
            VisualStyle::ShadedNoEdges => "Тела без рёбер",
            VisualStyle::Wireframe => "Каркас",
        }
    }
    /// (рисовать грани, рисовать рёбра)
    pub fn parts(self) -> (bool, bool) {
        match self {
            VisualStyle::Shaded => (true, true),
            VisualStyle::ShadedNoEdges => (true, false),
            VisualStyle::Wireframe => (false, true),
        }
    }
}

/// Человек, которому открыт проект (демо: хранится только на этом устройстве).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Person {
    pub email: String,
    /// `true` — может править, `false` — только смотреть.
    pub editor: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Share {
    pub path: String,
    pub people: Vec<Person>,
}

/// Демонстрационный билет подписки на сервер; `activated` — корешок оторван.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ticket {
    pub number: String,
    pub activated: bool,
}

/// Папка проектов в библиотеке. `auto_sync` — сохранять её модели на сервер подписки (пока демонстрация).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectFolder {
    pub name: String,
    pub projects: Vec<String>,
    pub auto_sync: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Lang,
    pub theme: ThemeChoice,
    pub accent: crate::theme::Accent,
    pub reduce_motion: bool,
    /// Масштаб интерфейса (1.0 = обычный).
    pub ui_scale: f32,
    pub show_grid: bool,
    pub visual_style: VisualStyle,
    pub orbit_speed: f32,
    pub zoom_speed: f32,
    pub invert_zoom: bool,
    /// Привязка к сетке в новых чертежах.
    pub snap_grid: bool,
    /// Материал по умолчанию для расчёта массы (индекс в списке).
    pub material: usize,
    /// Автосохранение открытого файла каждые N минут (0 — выключено).
    pub autosave_minutes: u32,
    /// Толщина линий рёбер деталей.
    pub edge_width: f32,
    /// Угол обзора камеры, градусы (меньше — ближе к чертёжной проекции).
    pub fov_deg: f32,
    /// Числа вдоль осей сетки.
    pub grid_numbers: bool,
    pub controls: ControlScheme,
    /// Ключ лицензии (`DEMO` — демо-режим).
    pub license: Option<String>,
    /// Почта вошедшего аккаунта (демо: проверка локальная).
    pub account: Option<String>,
    pub account_skipped: bool,
    pub display_name: String,
    /// Локальная демонстрационная подписка, привязанная к почте.
    pub server_demo_account: Option<String>,
    /// Недавние проекты (полные пути), новые первыми.
    pub recent: Vec<String>,
    pub folders: Vec<ProjectFolder>,
    pub ticket: Option<Ticket>,
    pub shares: Vec<Share>,
    /// Команда рабочего пространства (демо, только на этом устройстве).
    pub team: Vec<Person>,
    /// Версия, для которой уже показывали экран первой загрузки (пусто — программа только что установлена).
    pub splash_seen: String,
    pub show_left_panel: bool,
    pub show_right_panel: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lang: Lang::detect(),
            theme: ThemeChoice::Auto,
            accent: crate::theme::Accent::default(),
            reduce_motion: false,
            ui_scale: 1.0,
            show_grid: true,
            visual_style: VisualStyle::Shaded,
            orbit_speed: 1.0,
            zoom_speed: 1.0,
            invert_zoom: false,
            snap_grid: true,
            material: 0,
            autosave_minutes: 0,
            edge_width: 1.3,
            fov_deg: 35.0,
            grid_numbers: true,
            controls: ControlScheme::Standard,
            license: None,
            account: None,
            account_skipped: false,
            display_name: String::new(),
            server_demo_account: None,
            recent: Vec::new(),
            folders: Vec::new(),
            ticket: None,
            shares: Vec::new(),
            team: Vec::new(),
            splash_seen: String::new(),
            show_left_panel: true,
            show_right_panel: true,
        }
    }
}

/// Почта в нижнем регистре: проверена и ещё не встречается в `existing`.
fn checked_email(email: &str, existing: &[Person]) -> Result<String, &'static str> {
    let email = email.trim().to_lowercase();
    if !crate::home::valid_email(&email) { return Err("Некорректная почта"); }
    if existing.iter().any(|p| p.email == email) { return Err("Этот человек уже в списке"); }
    Ok(email)
}

impl Settings {
    pub fn people_of(&self, path: &str) -> &[Person] {
        self.shares.iter().find(|s| s.path == path).map_or(&[], |s| s.people.as_slice())
    }

    /// Добавить человека в проект (демо: приглашение никуда не отправляется).
    pub fn add_person(&mut self, path: &str, email: &str, editor: bool) -> Result<(), &'static str> {
        let email = checked_email(email, self.people_of(path))?;
        match self.shares.iter_mut().find(|s| s.path == path) {
            Some(s) => s.people.push(Person { email, editor }),
            None => self.shares.push(Share { path: path.to_string(), people: vec![Person { email, editor }] }),
        }
        Ok(())
    }

    /// Добавить человека в команду.
    pub fn add_team_member(&mut self, email: &str, editor: bool) -> Result<(), &'static str> {
        let email = checked_email(email, &self.team)?;
        self.team.push(Person { email, editor });
        Ok(())
    }

    pub fn remove_person(&mut self, path: &str, email: &str) {
        for s in &mut self.shares { if s.path == path { s.people.retain(|p| p.email != email); } }
        self.shares.retain(|s| !s.people.is_empty());
    }

    /// Подписка на сервер оформлена (пока локальная демонстрация).
    pub fn server_active(&self) -> bool {
        self.account.is_some() && self.server_demo_account == self.account
    }

    pub fn folder_of(&self, path: &str) -> Option<usize> {
        self.folders.iter().position(|f| f.projects.iter().any(|p| p == path))
    }

    /// Положить проект в папку `to` (`None` — вынуть из папки); проект бывает только в одной папке.
    pub fn move_to_folder(&mut self, path: &str, to: Option<usize>) {
        for f in &mut self.folders { f.projects.retain(|p| p != path); }
        if let Some(f) = to.and_then(|i| self.folders.get_mut(i)) { f.projects.push(path.to_string()); }
    }

    pub fn path() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os("GENIX_SETTINGS_PATH") { return Some(PathBuf::from(path)); }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let dir = if cfg!(any(target_os = "macos", target_os = "ios")) {
            home?.join("Library/Application Support/Genix")
        } else if cfg!(target_os = "windows") {
            PathBuf::from(std::env::var_os("APPDATA")?).join("Genix")
        } else {
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| home.map(|h| h.join(".config")))?.join("genix")
        };
        Some(dir.join("settings.json"))
    }

    pub fn load() -> Settings {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = crate::files::write_atomic(&p, s.as_bytes());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_partial_files() {
        let mut s = Settings::default();
        s.lang = Lang::En;
        s.ui_scale = 1.25;
        s.accent = crate::theme::Accent::Indigo;
        s.reduce_motion = true;
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
        // файл от старой версии без новых полей читается, недостающее — по умолчанию
        let old: Settings = serde_json::from_str(r#"{"lang":"En"}"#).unwrap();
        assert_eq!(old.lang, Lang::En);
        assert_eq!(old.accent, crate::theme::Accent::Blue);
        assert!(!old.reduce_motion);
        assert!(old.show_grid);
        // мусор не роняет программу
        assert!(serde_json::from_str::<Settings>("{oops").is_err());
    }

    #[test]
    fn a_project_lives_in_one_folder_and_old_files_have_none() {
        let mut s = Settings::default();
        s.folders = vec![ProjectFolder { name: "A".into(), ..Default::default() }, ProjectFolder { name: "B".into(), ..Default::default() }];
        s.move_to_folder("/x.genix", Some(0));
        s.move_to_folder("/x.genix", Some(1));
        assert_eq!((s.folder_of("/x.genix"), s.folders[0].projects.len()), (Some(1), 0));
        s.move_to_folder("/x.genix", None);
        assert_eq!(s.folder_of("/x.genix"), None);
        let old: Settings = serde_json::from_str(r#"{"lang":"En"}"#).unwrap();
        assert!(old.folders.is_empty());
    }

    #[test]
    fn people_are_added_once_validated_and_removed() {
        let mut s = Settings::default();
        assert_eq!(s.add_person("/a.genix", "nope", true), Err("Некорректная почта"));
        assert_eq!(s.add_person("/a.genix", " Ann@Example.com ", true), Ok(()));
        assert_eq!(s.add_person("/a.genix", "ann@example.com", false), Err("Этот человек уже в списке"));
        assert_eq!(s.add_team_member("bob@example.com", true), Ok(()));
        assert_eq!(s.add_team_member("bob@example.com", true), Err("Этот человек уже в списке"));
        assert_eq!(s.people_of("/a.genix").len(), 1);
        s.remove_person("/a.genix", "ann@example.com");
        assert!(s.people_of("/a.genix").is_empty() && s.shares.is_empty());
    }

    #[test]
    fn styles() {
        assert_eq!(VisualStyle::Wireframe.parts(), (false, true));
        assert_eq!(VisualStyle::Shaded.parts(), (true, true));
    }
}
