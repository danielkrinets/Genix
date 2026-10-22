//! Проверка опубликованной сборки. Не устанавливает обновления автоматически.
use crate::i18n::tr;
use eframe::egui::{self, RichText};
use serde::Deserialize;
use std::sync::mpsc::{self, Receiver};

pub const RELEASE_URL: &str = "https://github.com/danielkrinets/Genix/releases/tag/genix-latest";
const API_URL: &str = "https://api.github.com/repos/danielkrinets/Genix/releases/tags/genix-latest";

#[derive(Debug, Deserialize)]
pub struct Release {
    name: String,
    tag_name: String,
    updated_at: String,
    #[serde(default)]
    prerelease: bool,
}

#[derive(Default)]
pub struct Updates {
    pending: Option<Receiver<Result<Release, String>>>,
    result: Option<Result<Release, String>>,
}

impl Updates {
    fn check(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(rx);
        self.result = None;
        let ctx = ctx.clone();
        let request = ehttp::Request::get(API_URL)
            .with_header("User-Agent", "Genix")
            .with_header("Accept", "application/vnd.github+json")
            .with_timeout(Some(std::time::Duration::from_secs(15)));
        ehttp::fetch(request, move |response| {
            let result = response.and_then(|response| {
                if response.status != 200 {
                    return Err(format!("GitHub: HTTP {}", response.status));
                }
                serde_json::from_slice::<Release>(&response.bytes).map_err(|e| e.to_string())
            });
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => { self.result = Some(result); self.pending = None; }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.result = Some(Err(tr("Проверка прервана. Попробуйте ещё раз.").into()));
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {},
            }
        }
        ui.heading(tr("Обновления"));
        ui.label(tr("Официальные сборки Genix"));
        ui.add_space(20.0);
        ui.label(format!("Genix {} · {}", crate::version::version(), std::env::consts::OS));
        ui.label(RichText::new(tr("Установленная версия")).color(crate::home::muted(ui)));
        ui.add_space(16.0);
        if ui.add_enabled(self.pending.is_none(), egui::Button::new(tr("Проверить обновления"))).clicked() {
            self.check(ui.ctx());
        }
        if self.pending.is_some() {
            crate::widgets::loading(ui, tr("Проверяем GitHub…"));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(result) = &self.result {
            ui.add_space(16.0);
            match result {
                Ok(release) => {
                    ui.label(RichText::new(&release.name).strong());
                    ui.label(format!("{} · {}", release.tag_name, release.updated_at));
                    if release.prerelease { ui.label(tr("Предварительная сборка")); }
                    ui.label(tr("Сравните опубликованную сборку с установленной перед загрузкой."));
                }
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, tr("Не удалось проверить обновления"));
                    ui.label(tr("Проверьте подключение к интернету и повторите попытку."));
                    ui.collapsing(tr("Подробности"), |ui| { ui.label(error); });
                }
            }
        }
        ui.add_space(16.0);
        ui.hyperlink_to(tr("Открыть страницу загрузки"), RELEASE_URL);
        ui.label(RichText::new(tr("Установка выполняется вручную. Сначала сохраните проекты.")).color(crate::home::muted(ui)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_release_without_claiming_newer_version() {
        let release: Release = serde_json::from_str(r#"{"name":"Genix","tag_name":"genix-latest","updated_at":"2026-10-15T16:28:14Z","prerelease":true}"#).unwrap();
        assert_eq!(release.tag_name, "genix-latest");
        assert!(release.prerelease);
        assert!(serde_json::from_str::<Release>(r#"{"message":"Not Found"}"#).is_err());
    }
}
