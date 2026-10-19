//! Импорт фото или рисунка в чертёж: порог яркости → контуры → линии.

use crate::i18n::tr;
use crate::tf;
use crate::theme::accent;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke};
use fusion_core::sketch::P2;
use fusion_core::trace::{to_sketch_units, trace, TraceOpts};

pub struct TraceDialog {
    pub name: String,
    gray: Vec<u8>,
    w: usize,
    h: usize,
    opts: TraceOpts,
    /// Размер большей стороны картинки в чертеже, мм.
    size: f64,
    loops: Vec<Vec<P2>>,
    dirty: bool,
}

pub enum TraceResult {
    None,
    Cancel,
    /// Контуры в координатах чертежа (мм).
    Insert(Vec<Vec<P2>>),
}

const MAX_SIDE: u32 = 700;

impl TraceDialog {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let img = image::open(path).map_err(|e| format!("{e}"))?;
        let img = if img.width().max(img.height()) > MAX_SIDE { img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle) } else { img };
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width() as usize, rgba.height() as usize);
        // прозрачные места считаем белым фоном
        let gray = rgba
            .pixels()
            .map(|p| {
                let l = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
                let a = p[3] as f32 / 255.0;
                (l * a + 255.0 * (1.0 - a)) as u8
            })
            .collect();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut d = TraceDialog { name, gray, w, h, opts: TraceOpts::default(), size: 100.0, loops: vec![], dirty: true };
        d.opts.threshold = d.auto_threshold();
        d.retrace();
        Ok(d)
    }

    /// Порог по Оцу: хорошо делит тёмное и светлое.
    fn auto_threshold(&self) -> u8 {
        let mut hist = [0f64; 256];
        for &g in &self.gray {
            hist[g as usize] += 1.0;
        }
        let total: f64 = hist.iter().sum();
        let sum: f64 = hist.iter().enumerate().map(|(i, c)| i as f64 * c).sum();
        let (mut wb, mut sb, mut best, mut th) = (0.0, 0.0, -1.0, 128u8);
        for t in 0..256 {
            wb += hist[t];
            if wb == 0.0 {
                continue;
            }
            let wf = total - wb;
            if wf == 0.0 {
                break;
            }
            sb += t as f64 * hist[t];
            let (mb, mf) = (sb / wb, (sum - sb) / wf);
            let var = wb * wf * (mb - mf) * (mb - mf);
            if var > best {
                best = var;
                th = t as u8;
            }
        }
        th.max(1)
    }

    fn retrace(&mut self) {
        self.loops = trace(&self.gray, self.w, self.h, &self.opts);
        self.dirty = false;
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> TraceResult {
        let mut result = TraceResult::None;
        let mut open = true;
        egui::Window::new(tr("Фото → чертёж")).open(&mut open).collapsible(false).resizable(false).default_width(360.0).show(ctx, |ui| {
            ui.label(RichText::new(&self.name).weak());
            let mut ch = false;
            ui.horizontal(|ui| {
                ui.label(tr("Порог яркости"));
                ch |= ui.add(egui::Slider::new(&mut self.opts.threshold, 1..=254)).changed();
            });
            ch |= crate::widgets::checkbox(ui, &mut self.opts.invert, tr("Фигура — светлое на тёмном")).changed();
            ui.horizontal(|ui| {
                ui.label(tr("Гладкость"));
                ch |= ui.add(egui::Slider::new(&mut self.opts.tolerance, 0.3..=6.0).fixed_decimals(1)).changed();
            });
            ui.horizontal(|ui| {
                ui.label(tr("Убрать мелкий мусор"));
                ch |= ui.add(egui::Slider::new(&mut self.opts.min_area, 0.0..=800.0).logarithmic(true)).changed();
            });
            ui.horizontal(|ui| {
                ui.label(tr("Размер по большей стороне"));
                ui.add(egui::DragValue::new(&mut self.size).range(1.0..=100000.0).suffix(tr(" мм")));
            });
            if ch {
                self.dirty = true;
            }
            if self.dirty {
                self.retrace();
            }
            // предпросмотр контуров
            let side = 300.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side * (self.h as f32 / self.w as f32).min(1.0).max(0.3)), egui::Sense::hover());
            let k = (rect.width() / self.w as f32).min(rect.height() / self.h as f32);
            let origin = rect.center() - egui::vec2(self.w as f32, self.h as f32) * k / 2.0;
            let p = ui.painter_at(rect);
            p.rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
            for l in &self.loops {
                let pts: Vec<Pos2> = l.iter().map(|q| origin + egui::vec2(q[0] as f32, q[1] as f32) * k).collect();
                p.add(egui::Shape::closed_line(pts, Stroke::new(1.2_f32, accent())));
            }
            p.rect_stroke(Rect::from_min_size(origin, egui::vec2(self.w as f32, self.h as f32) * k), 0.0, Stroke::new(1.0_f32, Color32::from_gray(110)), egui::StrokeKind::Inside);
            let pts: usize = self.loops.iter().map(|l| l.len()).sum();
            ui.label(RichText::new(tf!("Контуров: {}, отрезков: {}", self.loops.len(), pts)).weak().small());
            if pts > 1500 {
                ui.label(RichText::new(tr("Очень много отрезков — увеличьте гладкость")).color(Color32::from_rgb(200, 120, 0)).small());
            }
            ui.horizontal(|ui| {
                let ok = egui::Button::new(RichText::new(tr("  Вставить в чертёж  ")).strong().color(crate::theme::on_accent())).fill(crate::theme::accent());
                if ui.add_enabled(!self.loops.is_empty(), ok).clicked() {
                    result = TraceResult::Insert(to_sketch_units(&self.loops, self.w, self.h, self.size));
                }
                if ui.button(tr("Отмена")).clicked() {
                    result = TraceResult::Cancel;
                }
            });
        });
        if !open {
            result = TraceResult::Cancel;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_png_and_traces_shape() {
        let mut img = image::RgbaImage::from_pixel(120, 80, image::Rgba([255, 255, 255, 255]));
        for y in 20..60 {
            for x in 30..90 {
                img.put_pixel(x, y, image::Rgba([20, 20, 20, 255]));
            }
        }
        let path = std::env::temp_dir().join("genix_trace_test.png");
        img.save(&path).unwrap();
        let d = TraceDialog::load(&path).unwrap();
        assert_eq!(d.loops.len(), 1);
        let units = to_sketch_units(&d.loops, d.w, d.h, 120.0);
        let xs: Vec<f64> = units[0].iter().map(|p| p[0]).collect();
        let width = xs.iter().cloned().fold(f64::MIN, f64::max) - xs.iter().cloned().fold(f64::MAX, f64::min);
        assert!((width - 60.0).abs() < 1.0, "{width}");
    }
}
