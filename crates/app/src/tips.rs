//! Tooltip pictures for the toolbar: before/after frames drawn by the renderer itself (see `tools/make_tips.py`).
use eframe::egui::{self, Color32, Rect, Sense};

include!("tips_index.rs");

const CELL: f32 = 3.2;
const FADE: f32 = 0.3;

fn texture(ctx: &egui::Context, id: &str, tag: &str, bytes: &'static [u8]) -> Option<egui::TextureHandle> {
    let key = egui::Id::new(("tip-texture", id, tag));
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
        return Some(t);
    }
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
    let handle = ctx.load_texture(format!("tip-{id}-{tag}"), color, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(key, handle.clone()));
    Some(handle)
}

/// Mix factor of the "after" picture for time `t` (seconds): the picture rests, fades, rests, fades back.
fn after_mix(t: f32) -> f32 {
    let t = t.rem_euclid(2.0 * CELL);
    let ramp = |x: f32| (x / FADE).clamp(0.0, 1.0);
    if t < CELL { ramp(t - (CELL - FADE)) } else { 1.0 - ramp(t - (2.0 * CELL - FADE)) }
}

/// Draws the picture(s) of tool `id`; false when this tool has none (the caller then draws its diagram).
pub fn show(ui: &mut egui::Ui, id: &str) -> bool {
    let Some((before, after)) = bytes(id) else { return false };
    let Some(after_tex) = texture(ui.ctx(), id, "after", after) else { return false };
    let before_tex = before.and_then(|b| texture(ui.ctx(), id, "before", b));
    let width = 250.0;
    let size = egui::vec2(width, width * after_tex.size()[1] as f32 / after_tex.size()[0] as f32);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let p = ui.painter_at(rect);
    let animate = before_tex.is_some() && !crate::theme::reduced_motion();
    match (&before_tex, animate) {
        (Some(b), true) => {
            let mix = after_mix(ui.input(|i| i.time) as f32);
            p.image(b.id(), rect, uv, Color32::WHITE);
            p.image(after_tex.id(), rect, uv, Color32::from_white_alpha((mix * 255.0) as u8));
            ui.ctx().request_repaint();
        }
        (Some(b), false) => {
            // reduced motion: both pictures side by side, no movement
            let half = egui::vec2(width / 2.0 - 2.0, size.y / 2.0 - 1.0);
            let top = rect.min + egui::vec2(0.0, (size.y - half.y) / 2.0);
            p.image(b.id(), Rect::from_min_size(top, half), uv, Color32::WHITE);
            p.image(after_tex.id(), Rect::from_min_size(top + egui::vec2(half.x + 4.0, 0.0), half), uv, Color32::WHITE);
        }
        (None, _) => {
            p.image(after_tex.id(), rect, uv, Color32::WHITE);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_rests_fades_and_loops() {
        assert_eq!(after_mix(0.0), 0.0);
        assert_eq!(after_mix(CELL - FADE - 0.01), 0.0);
        assert!((after_mix(CELL - FADE / 2.0) - 0.5).abs() < 1e-3);
        assert_eq!(after_mix(CELL + 0.1), 1.0);
        assert!((after_mix(2.0 * CELL - FADE / 2.0) - 0.5).abs() < 1e-3);
        assert_eq!(after_mix(2.0 * CELL), 0.0);
    }

    #[test]
    fn every_listed_picture_decodes() {
        for id in ["extrude", "fillet", "draft", "thread", "split", "box"] {
            let (before, after) = bytes(id).unwrap_or_else(|| panic!("нет картинки для {id}"));
            assert!(image::load_from_memory(after).is_ok(), "{id} after");
            if let Some(b) = before {
                assert!(image::load_from_memory(b).is_ok(), "{id} before");
            }
        }
    }
}
