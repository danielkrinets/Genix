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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: Lang,
    pub theme: ThemeChoice,
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
    /// Недавние проекты (полные пути), новые первыми.
    pub recent: Vec<String>,
    pub show_left_panel: bool,
    pub show_right_panel: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lang: Lang::detect(),
            theme: ThemeChoice::Auto,
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
            recent: Vec::new(),
            show_left_panel: true,
            show_right_panel: true,
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
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
                let _ = std::fs::write(p, s);
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
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(s, back);
        // файл от старой версии без новых полей читается, недостающее — по умолчанию
        let old: Settings = serde_json::from_str(r#"{"lang":"En"}"#).unwrap();
        assert_eq!(old.lang, Lang::En);
        assert!(old.show_grid);
        // мусор не роняет программу
        assert!(serde_json::from_str::<Settings>("{oops").is_err());
    }

    #[test]
    fn styles() {
        assert_eq!(VisualStyle::Wireframe.parts(), (false, true));
        assert_eq!(VisualStyle::Shaded.parts(), (true, true));
    }
}
