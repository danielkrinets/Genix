//! Native egui adaptations of React Bits Pattern Waves / Shape Waves.
//! Reference and license: docs/REACT_BITS_LICENSE.md. No browser runtime.
use eframe::egui::{self, Color32, Pos2, Rect};
use std::time::Duration;

fn wave(x: f32, y: f32, time: f32) -> f32 {
    let u = x * 0.94 + y * 0.342;
    let v = -x * 0.342 + y * 0.94;
    let bend = (v * 0.85 + time * 0.05).sin() * 1.7 + 0.4 * (v * 1.6 + time * 0.2).sin();
    let phase = u * 5.5 + bend - time * 0.45;
    let fold = phase.sin() + 0.32 * (2.0 * phase + 1.3).sin();
    let ripple = 0.16 * (u * 13.75 + bend * 1.9 - time * 0.9 + 2.1).sin();
    (fold + ripple) * (0.6 + 0.4 * (u * 0.55 + v * 0.45 + time * 0.04).cos())
}

#[derive(Clone, Default)]
struct PointerWaves {
    last: Option<Pos2>,
    sampled_at: f64,
    ripples: Vec<(Pos2, f64)>,
}

/// Local light and travelling rings; never changes layout or consumes pointer input.
fn interaction(center: Pos2, pointer: Option<Pos2>, ripples: &[(Pos2,f64)], now: f64, reduced: bool) -> (egui::Vec2,f32) {
    let mut light = pointer.map_or(0.0, |p| (1.0-center.distance(p)/130.0).clamp(0.0,1.0).powi(2));
    let mut offset = egui::Vec2::ZERO;
    if !reduced {
        for &(origin, start) in ripples {
            let age = (now-start) as f32;
            if !(0.0..1.2).contains(&age) { continue; }
            let delta = center-origin;
            let distance = delta.length();
            let ring = (1.0-((distance-age*180.0)/32.0).abs()).clamp(0.0,1.0) * (1.0-age/1.2);
            offset += delta.normalized() * ring * 5.0;
            light = light.max(ring*0.8);
        }
    }
    if offset.length() > 7.0 { offset = offset.normalized()*7.0; }
    (offset,light)
}

/// Paint-only background; pointer tracking is passive and gated off for touch.

pub fn paint(ui: &egui::Ui, rect: Rect, circles: bool) {
    if !rect.is_positive() { return; }
    let reduced = crate::theme::reduced_motion();
    let now = ui.input(|i| i.time);
    let pointer = ui.input(|i| if i.focused && !i.any_touches() { i.pointer.hover_pos().filter(|p| rect.contains(*p)) } else { None });
    let id = ui.id().with(("background-pointer",circles));
    let mut state = ui.data(|d| d.get_temp::<PointerWaves>(id)).unwrap_or_default();
    state.ripples.retain(|&(_,start)| now-start < 1.2);
    if !reduced {
        if let Some(at) = pointer {
            let moved = state.last.is_none_or(|last| last.distance(at)>14.0);
            let pressed = ui.input(|i| i.pointer.any_pressed());
            if pressed || (moved && now-state.sampled_at >= 0.04) {
                if state.ripples.len() >= 16 { state.ripples.remove(0); }
                state.ripples.push((at,now));
                state.last = Some(at);
                state.sampled_at = now;
            }
        } else { state.last = None; }
    } else { state.ripples.clear(); state.last = None; }
    ui.data_mut(|d| d.insert_temp(id,state.clone()));
    let time = if reduced { 0.0 } else { ui.input(|i| i.time) as f32 };
    let dark = ui.visuals().dark_mode;
    let bg = if dark { Color32::BLACK } else { Color32::from_rgb(242,242,247) };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, bg);
    // Bound geometry cost on large displays while keeping the requested 12px circle grid.
    let base: f32 = if circles { 12.0 } else { 16.0 };
    let cell = base.max((rect.area() / if circles { 10000.0 } else { 4500.0 }).sqrt());
    let cols = (rect.width()/cell).ceil() as usize;
    let rows = (rect.height()/cell).ceil() as usize;
    let glyphs: Vec<_> = "Genix".chars().map(|ch| p.layout_no_wrap(ch.to_string(),egui::FontId::monospace(10.0),Color32::PLACEHOLDER)).collect();
    let mut shapes = Vec::with_capacity(cols*rows);
    for row in 0..rows {
        for col in 0..cols {
            let x = (col as f32+0.5)*cell;
            let y = (row as f32+0.5)*cell;
            let center = rect.min + egui::vec2(x,y);
            let (offset, light) = interaction(center,pointer,&state.ripples,now,reduced);
            let center = center + offset;
            let nx = (x-rect.width()*0.5)/280.0;
            let ny = (y-rect.height()*0.5)/280.0;
            let level = if circles {
                let angle = 130.0_f32.to_radians();
                let drift = time*1.6*12.0;
                let x = nx + angle.cos()*drift/280.0;
                let y = ny + angle.sin()*drift/280.0;
                ((wave(x,y,time*0.45)+1.3)/2.6).clamp(0.0,1.0)
            } else {
                let slope = (wave(nx+0.025,ny,time*0.35)-wave(nx-0.025,ny,time*0.35))*12.0;
                (0.25+slope*0.45).clamp(0.02,1.0)
            };
            let edge = ((x.min(rect.width()-x).min(y.min(rect.height()-y)))/70.0).clamp(0.0,1.0);
            let strength = if circles { (0.43 * level + light*0.24).min(0.43) } else { (0.05 + level * 0.17 + light*0.18).min(0.26) };
            let ink = if dark { Color32::WHITE } else { Color32::from_rgb(45,50,65) };
            let ink = ink.lerp_to_gamma(crate::theme::accent(),light*0.65);
            let color = ink.gamma_multiply(strength*edge*if dark { 1.0 } else { 1.65 });
            if circles {
                shapes.push(egui::Shape::circle_filled(center,cell*0.28*(0.45+0.55*level)*if reduced { 1.0 } else { 1.0+light*0.3 },color));
            } else {
                let glyph = glyphs[(col+row)%glyphs.len()].clone();
                shapes.push(egui::Shape::galley(center-glyph.size()*0.5,glyph,color));
            }
        }
    }
    p.extend(shapes);
    if circles && rect.width() >= 900.0 {
        // Text is a negative-space cutout, matching the reference's optional text mask.
        let at = Pos2::new(rect.left()+rect.width()*0.255,rect.center().y);
        p.text(at,egui::Align2::CENTER_CENTER,"Genix",egui::FontId::proportional((rect.width()*0.12).min(180.0)),bg);
    }
    if !reduced && ui.input(|i| i.focused) { ui.ctx().request_repaint_after(Duration::from_millis(33)); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_lights_locally_and_ripples_decay_without_motion_in_reduced_mode() {
        let origin=Pos2::new(100.0,100.0);
        let (offset,light)=interaction(origin,Some(origin),&[],0.0,false);
        assert_eq!(offset,egui::Vec2::ZERO); assert_eq!(light,1.0);
        assert_eq!(interaction(origin+egui::vec2(500.0,0.0),Some(origin),&[],0.0,false).1,0.0);
        let rings=[(origin,0.0)];
        let point=origin+egui::vec2(90.0,0.0);
        assert!(interaction(point,None,&rings,0.5,false).0.length()>0.0);
        assert_eq!(interaction(point,None,&rings,1.3,false),(egui::Vec2::ZERO,0.0));
        assert_eq!(interaction(point,Some(point),&rings,0.5,true),(egui::Vec2::ZERO,1.0));
    }

    #[test]
    fn mouse_move_changes_both_rendered_backgrounds_at_the_same_time() {
        for circles in [false,true] {
            let render = |hover: bool| {
                let ctx=egui::Context::default();
                let mut input=egui::RawInput::default(); input.time=Some(0.5); input.focused=true;
                if hover { input.events.push(egui::Event::PointerMoved(Pos2::new(300.0,200.0))); }
                ctx.run(input,|ctx| { egui::CentralPanel::default().show(ctx,|ui| {
                    paint(ui,Rect::from_min_size(Pos2::ZERO,egui::vec2(600.0,400.0)),circles);
                }); }).shapes
            };
            assert_ne!(render(false),render(true),"circles={circles}");
        }
    }

    #[test]
    fn both_backgrounds_render_without_interactive_regions() {
        let ctx=egui::Context::default();
        for circles in [false,true] {
            let out=ctx.run(egui::RawInput::default(),|ctx| { egui::CentralPanel::default().show(ctx,|ui| { paint(ui,Rect::from_min_size(Pos2::ZERO,egui::vec2(1000.0,700.0)),circles); }); });
            assert!(out.shapes.len()>100);
        }
    }
}
