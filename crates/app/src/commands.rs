//! Диалоги операций (выдавливание, вращение, примитивы, перемещение, массивы...).
//! Операция живёт в документе с первого кадра — так виден предпросмотр. «Отмена» возвращает снимок.

use crate::widgets::{expr_field, field_label, section};
use eframe::egui::{self, Color32, RichText};
use fusion_core::csg::BoolOp;
use crate::i18n::{tr, tr_dyn};
use crate::tf;
use fusion_core::doc::{Axis3, Extent, HoleKind, PatternKind, RevolveAxis, Shape};
use fusion_core::sketch::Entity;
use fusion_core::doc::PipeShape;
use fusion_core::{Document, Evaluator, Expr, FeatureKind, Op};

/// Что выбирают щелчки в 3D-окне во время команды.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickTarget {
    None,
    Profiles,
    Bodies,
    CombineTarget,
    CombineTools,
    /// Плоская грань тела (для отверстия).
    Face,
    /// Прямые рёбра тела (для фаски и скругления).
    Edges,
    /// Линия чертежа для пути трубы.
    Path,
    /// Куда выровнять: грань другого тела или плоскость.
    AlignTarget,
}

pub struct Command {
    pub uid: u64,
    /// Документ до начала команды (для отмены и для истории правок).
    pub before: Document,
    pub pick: PickTarget,
    /// Операцию выбирает программа (вырезать при выдавливании внутрь, объединить — наружу),
    /// пока пользователь не выбрал её сам.
    pub auto_op: bool,
}

pub enum CmdResult {
    None,
    Changed,
    Ok,
    Cancel,
}

impl Command {
    pub fn pick_for(kind: &FeatureKind) -> PickTarget {
        match kind {
            FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. } | FeatureKind::Sweep { .. } | FeatureKind::Loft { .. } => PickTarget::Profiles,
            FeatureKind::Move { .. } | FeatureKind::Mirror { .. } | FeatureKind::Pattern { .. } | FeatureKind::Remove { .. } | FeatureKind::Scale { .. } => PickTarget::Bodies,
            FeatureKind::Hole { .. } | FeatureKind::OffsetPlane { .. } | FeatureKind::Shell { .. } | FeatureKind::PressPull { .. } | FeatureKind::PlaceOnPlane { .. } => PickTarget::Face,
            FeatureKind::Chamfer { .. } | FeatureKind::Fillet { .. } => PickTarget::Edges,
            FeatureKind::Pipe { .. } => PickTarget::Path,
            FeatureKind::Combine { .. } => PickTarget::CombineTarget,
            _ => PickTarget::None,
        }
    }

    pub fn hint(&self) -> &'static str {
        match self.pick {
            PickTarget::Profiles => tr("Щёлкните профили чертежа (замкнутые области), которые нужно использовать"),
            PickTarget::Bodies => tr("Щёлкните тела, к которым применить операцию"),
            PickTarget::CombineTarget => tr("Щёлкните целевое тело"),
            PickTarget::CombineTools => tr("Щёлкните тела-инструменты"),
            PickTarget::Face => tr("Щёлкните плоскую грань тела (для плоскости — ещё и плоскость начала координат)"),
            PickTarget::Edges => tr("Щёлкните прямые рёбра или дуги; повторный щелчок снимает выбор"),
            PickTarget::Path => tr("Щёлкните линию чертежа — путь трубы"),
            PickTarget::AlignTarget => tr("Куда: щёлкните грань другого тела или плоскость"),
            PickTarget::None => tr("Задайте размеры и положение"),
        }
    }
}

fn num(v: f64) -> Expr {
    Expr::num(v)
}

fn op_combo(ui: &mut egui::Ui, id: &str, op: &mut Op) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        field_label(ui, tr("Операция"));
        egui::ComboBox::from_id_salt(id).selected_text(tr(op.label())).show_ui(ui, |ui| {
            for o in Op::ALL {
                changed |= ui.selectable_value(op, o, tr(o.label())).changed();
            }
        });
    });
    changed
}

fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let mut c = false;
    ui.horizontal(|ui| {
        field_label(ui, label);
        c = add(ui);
    });
    c
}

/// Отрицание выражения для кнопки «обратное направление».
fn negate(e: &Expr) -> Expr {
    let t = e.0.trim();
    if let Ok(v) = t.parse::<f64>() {
        return Expr::num(-v);
    }
    if let Some(inner) = t.strip_prefix("-(").and_then(|s| s.strip_suffix(')')) {
        return Expr(inner.to_string());
    }
    Expr(format!("-({t})"))
}

/// Нарисовать содержимое диалога. Меняет шаг в документе напрямую.
pub fn dialog_ui(ui: &mut egui::Ui, doc: &mut Document, ev: &Evaluator, cmd: &mut Command, body_name: &dyn Fn(u64) -> String, sketches: &[(u64, String)]) -> CmdResult {
    let params = doc.params.clone();
    let my_index = doc.index_of(cmd.uid);
    let has_bodies = doc.index_of(cmd.uid).is_some_and(|i| !ev.model_before(i).bodies.is_empty());
    let mut changed = false;
    let Some(f) = doc.feature_mut(cmd.uid) else { return CmdResult::Cancel };
    let f_is_fillet = matches!(f.kind, FeatureKind::Fillet { .. });
    let p = &params;
    let uid = cmd.uid;
    match &mut f.kind {
        FeatureKind::Extrude { sketch: _, profiles, extent, op, face } => {
            section(ui, tr("ПРОФИЛИ"));
            if face.is_some() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr("Выбрана грань тела")));
                    if ui.small_button(tr("Сбросить выбор")).clicked() { *face = None; changed = true; }
                });
            } else {
                ui.label(if profiles.is_empty() { RichText::new(tr("Не выбрано — щёлкните область чертежа или грань тела")).color(Color32::from_rgb(200, 120, 0)) } else { RichText::new(tf!("Выбрано: {}", profiles.len())) });
                if !profiles.is_empty() && ui.small_button(tr("Сбросить выбор")).clicked() {
                    profiles.clear();
                    changed = true;
                }
            }
            section(ui, tr("РАЗМЕРЫ"));
            let mut kind = match extent {
                Extent::OneSide(_) => 0,
                Extent::Symmetric(_) => 1,
                Extent::TwoSides(..) => 2,
            };
            let names = [tr("В одну сторону"), tr("Симметрично"), tr("В две стороны")];
            changed |= row(ui, tr("Направление"), |ui| {
                let mut c = false;
                egui::ComboBox::from_id_salt("ext_kind").selected_text(names[kind]).show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        c |= ui.selectable_value(&mut kind, i, *n).changed();
                    }
                });
                c
            });
            let cur = match extent {
                Extent::OneSide(d) | Extent::Symmetric(d) | Extent::TwoSides(d, _) => d.clone(),
            };
            let new_kind = match (kind, &*extent) {
                (0, Extent::OneSide(_)) | (1, Extent::Symmetric(_)) | (2, Extent::TwoSides(..)) => None,
                (0, _) => Some(Extent::OneSide(cur.clone())),
                (1, _) => Some(Extent::Symmetric(cur.clone())),
                _ => Some(Extent::TwoSides(cur.clone(), cur.clone())),
            };
            if let Some(k) = new_kind {
                *extent = k;
                changed = true;
            }
            match extent {
                Extent::OneSide(d) => {
                    changed |= row(ui, tr("Расстояние"), |ui| expr_field(ui, ("ext", uid, 0), d, p, tr("мм")));
                    if ui.button(tr("⇅ Обратное направление")).clicked() {
                        *d = negate(d);
                        changed = true;
                    }
                }
                Extent::Symmetric(d) => changed |= row(ui, tr("Толщина"), |ui| expr_field(ui, ("ext", uid, 1), d, p, tr("мм"))),
                Extent::TwoSides(a, b) => {
                    changed |= row(ui, tr("Вперёд"), |ui| expr_field(ui, ("ext", uid, 2), a, p, tr("мм")));
                    changed |= row(ui, tr("Назад"), |ui| expr_field(ui, ("ext", uid, 3), b, p, tr("мм")));
                }
            }
            if op_combo(ui, "ext_op", op) {
                changed = true;
                cmd.auto_op = false;
            } else if changed && cmd.auto_op && has_bodies {
                if let Extent::OneSide(d) = extent {
                    let v = d.eval(p).unwrap_or(0.0);
                    *op = if v < 0.0 { Op::Cut } else { Op::Join };
                }
            }
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, op } => {
            section(ui, tr("ПРОФИЛИ"));
            ui.label(if profiles.is_empty() { RichText::new(tr("Не выбрано — щёлкните область чертежа")).color(Color32::from_rgb(200, 120, 0)) } else { RichText::new(tf!("Выбрано: {}", profiles.len())) });
            section(ui, tr("ОСЬ"));
            let lines: Vec<(usize, bool)> = ev
                .sketch(*sketch)
                .map(|s| s.sketch.entities.iter().enumerate().filter_map(|(i, e)| if let Entity::Line { construction, .. } = e { Some((i, *construction)) } else { None }).collect())
                .unwrap_or_default();
            let axis_name = |a: &RevolveAxis| match a {
                RevolveAxis::SketchX => tr("Ось X чертежа").to_string(),
                RevolveAxis::SketchY => tr("Ось Y чертежа").to_string(),
                RevolveAxis::Line(i) => {
                    let n = lines.iter().position(|(k, _)| k == i).map(|k| k + 1).unwrap_or(0);
                    tf!("Линия {}", n)
                }
            };
            changed |= row(ui, tr("Ось вращения"), |ui| {
                let mut c = false;
                egui::ComboBox::from_id_salt("rev_axis").selected_text(axis_name(axis)).show_ui(ui, |ui| {
                    c |= ui.selectable_value(axis, RevolveAxis::SketchX, tr("Ось X чертежа")).changed();
                    c |= ui.selectable_value(axis, RevolveAxis::SketchY, tr("Ось Y чертежа")).changed();
                    for (n, (i, cons)) in lines.iter().enumerate() {
                        let label = format!("{}{}", tf!("Линия {}", n + 1), if *cons { tr(" (вспомогательная)") } else { "" });
                        c |= ui.selectable_value(axis, RevolveAxis::Line(*i), label).changed();
                    }
                });
                c
            });
            changed |= row(ui, tr("Угол"), |ui| expr_field(ui, ("rev", uid), angle, p, "°"));
            if op_combo(ui, "rev_op", op) {
                changed = true;
                cmd.auto_op = false;
            }
        }
        FeatureKind::Primitive { shape, at, op } => {
            section(ui, tr("РАЗМЕРЫ"));
            match shape {
                Shape::Box { w, d, h } => {
                    changed |= row(ui, tr("Ширина (X)"), |ui| expr_field(ui, ("box", uid, 0), w, p, tr("мм")));
                    changed |= row(ui, tr("Глубина (Y)"), |ui| expr_field(ui, ("box", uid, 1), d, p, tr("мм")));
                    changed |= row(ui, tr("Высота (Z)"), |ui| expr_field(ui, ("box", uid, 2), h, p, tr("мм")));
                }
                Shape::Cylinder { r, h } => {
                    changed |= row(ui, tr("Радиус"), |ui| expr_field(ui, ("cyl", uid, 0), r, p, tr("мм")));
                    changed |= row(ui, tr("Высота (Z)"), |ui| expr_field(ui, ("cyl", uid, 1), h, p, tr("мм")));
                }
                Shape::Sphere { r } => changed |= row(ui, tr("Радиус"), |ui| expr_field(ui, ("sph", uid), r, p, tr("мм"))),
                Shape::Torus { major, minor } => {
                    changed |= row(ui, tr("Большой радиус"), |ui| expr_field(ui, ("tor", uid, 0), major, p, "мм"));
                    changed |= row(ui, tr("Малый радиус"), |ui| expr_field(ui, ("tor", uid, 1), minor, p, "мм"));
                }
            }
            section(ui, if matches!(shape, Shape::Box { .. }) { tr("ПОЛОЖЕНИЕ УГЛА") } else { tr("ПОЛОЖЕНИЕ ЦЕНТРА") });
            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                changed |= row(ui, a, |ui| expr_field(ui, ("at", uid, i), &mut at[i], p, tr("мм")));
            }
            if op_combo(ui, "prim_op", op) {
                changed = true;
                cmd.auto_op = false;
            }
        }
        FeatureKind::Move { bodies, translate, rotate, copy } => {
            bodies_ui(ui, bodies, body_name, &mut changed);
            section(ui, tr("СДВИГ"));
            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                changed |= row(ui, a, |ui| expr_field(ui, ("mv", uid, i), &mut translate[i], p, tr("мм")));
            }
            section(ui, tr("ПОВОРОТ ВОКРУГ ЦЕНТРА"));
            for (i, a) in [tr("Вокруг X"), tr("Вокруг Y"), tr("Вокруг Z")].iter().enumerate() {
                changed |= row(ui, a, |ui| expr_field(ui, ("rot", uid, i), &mut rotate[i], p, "°"));
            }
            changed |= crate::widgets::checkbox(ui, copy, tr("Создать копию")).changed();
        }
        FeatureKind::Mirror { bodies, normal, join } => {
            bodies_ui(ui, bodies, body_name, &mut changed);
            section(ui, tr("ПЛОСКОСТЬ"));
            let pname = |a: Axis3| match a {
                Axis3::X => tr("YZ (зеркало по X)"),
                Axis3::Y => tr("XZ (зеркало по Y)"),
                Axis3::Z => tr("XY (зеркало по Z)"),
            };
            changed |= row(ui, tr("Плоскость"), |ui| {
                let mut c = false;
                egui::ComboBox::from_id_salt("mir").selected_text(pname(*normal)).show_ui(ui, |ui| {
                    for a in [Axis3::X, Axis3::Y, Axis3::Z] {
                        c |= ui.selectable_value(normal, a, pname(a)).changed();
                    }
                });
                c
            });
            changed |= crate::widgets::checkbox(ui, join, tr("Объединить с исходным телом")).changed();
        }
        FeatureKind::Pattern { bodies, kind } => {
            bodies_ui(ui, bodies, body_name, &mut changed);
            section(ui, tr("ТИП"));
            let mut circ = matches!(kind, PatternKind::Circular { .. });
            ui.horizontal(|ui| {
                if ui.radio_value(&mut circ, false, tr("Прямоугольный")).changed() {
                    *kind = PatternKind::Rect { nx: num(3.0), dx: num(20.0), ny: num(1.0), dy: num(20.0) };
                    changed = true;
                }
                if ui.radio_value(&mut circ, true, tr("Круговой")).changed() {
                    *kind = PatternKind::Circular { count: num(6.0), axis: Axis3::Z, angle: num(360.0) };
                    changed = true;
                }
            });
            match kind {
                PatternKind::Rect { nx, dx, ny, dy } => {
                    changed |= row(ui, tr("Кол-во по X"), |ui| expr_field(ui, ("pat", uid, 0), nx, p, tr("шт")));
                    changed |= row(ui, tr("Шаг по X"), |ui| expr_field(ui, ("pat", uid, 1), dx, p, tr("мм")));
                    changed |= row(ui, tr("Кол-во по Y"), |ui| expr_field(ui, ("pat", uid, 2), ny, p, tr("шт")));
                    changed |= row(ui, tr("Шаг по Y"), |ui| expr_field(ui, ("pat", uid, 3), dy, p, tr("мм")));
                }
                PatternKind::Circular { count, axis, angle } => {
                    changed |= row(ui, tr("Количество"), |ui| expr_field(ui, ("pat", uid, 4), count, p, tr("шт")));
                    changed |= row(ui, tr("Ось"), |ui| {
                        let mut c = false;
                        for a in [Axis3::X, Axis3::Y, Axis3::Z] {
                            c |= ui.selectable_value(axis, a, a.label()).changed();
                        }
                        c
                    });
                    changed |= row(ui, tr("Общий угол"), |ui| expr_field(ui, ("pat", uid, 5), angle, p, "°"));
                }
            }
        }
        FeatureKind::Combine { target, tools, op, keep_tools } => {
            section(ui, tr("ТЕЛА"));
            ui.horizontal(|ui| {
                field_label(ui, tr("Цель"));
                if ui.selectable_label(cmd.pick == PickTarget::CombineTarget, if *target == 0 { tr("выбрать…").to_string() } else { body_name(*target) }).clicked() {
                    cmd.pick = PickTarget::CombineTarget;
                }
            });
            ui.horizontal(|ui| {
                field_label(ui, tr("Инструменты"));
                let t = if tools.is_empty() { tr("выбрать…").to_string() } else { tools.iter().map(|i| body_name(*i)).collect::<Vec<_>>().join(", ") };
                if ui.selectable_label(cmd.pick == PickTarget::CombineTools, t).clicked() {
                    cmd.pick = PickTarget::CombineTools;
                }
            });
            changed |= row(ui, tr("Операция"), |ui| {
                let mut c = false;
                egui::ComboBox::from_id_salt("comb_op").selected_text(tr(op.label())).show_ui(ui, |ui| {
                    for o in BoolOp::ALL {
                        c |= ui.selectable_value(op, o, tr(o.label())).changed();
                    }
                });
                c
            });
            changed |= crate::widgets::checkbox(ui, keep_tools, tr("Сохранить инструменты")).changed();
        }
        FeatureKind::Remove { bodies } => bodies_ui(ui, bodies, body_name, &mut changed),
        FeatureKind::PlaceOnPlane { target, body, to, .. } => {
            section(ui, tr("ОТКУДА"));
            let choosing_target = cmd.pick == PickTarget::AlignTarget;
            if choosing_target {
                ui.horizontal(|ui| {
                    ui.label(tr("Грань выбрана"));
                    if ui.button(tr("Выбрать другую")).clicked() { cmd.pick = PickTarget::Face; }
                });
            } else {
                face_pick_row(ui, &mut cmd.pick, *body != 0, tr("Щёлкните плоскую грань тела, которую нужно поставить"));
            }
            section(ui, tr("КУДА"));
            let floors = [(tr("XY (пол)"), fusion_core::Plane::XY), (tr("XZ (перед)"), fusion_core::Plane::XZ), (tr("YZ (право)"), fusion_core::Plane::YZ)];
            // which of the three origin planes the target is on (sign of its normal does not matter here)
            let n = target.normal();
            let origin_plane = (0..3).find(|&i| to.is_none() && n.dot(floors[i].1.normal()).abs() > 0.99 && target.o().length() < 1e-9);
            let current = origin_plane.unwrap_or(usize::MAX);
            let mut pick = current;
            changed |= row(ui, tr("Плоскость"), |ui| {
                let mut c = false;
                let shown = if current < 3 { floors[current].0 } else { tr("Грань тела") };
                egui::ComboBox::from_id_salt("place_plane").selected_text(shown).show_ui(ui, |ui| {
                    for (i, (name, _)) in floors.iter().enumerate() { c |= ui.selectable_value(&mut pick, i, *name).changed(); }
                });
                c
            });
            if pick != current && pick < 3 {
                *target = floors[pick].1;
                *to = None;
            }
            if choosing_target {
                ui.label(RichText::new(tr("Куда: щёлкните грань другого тела или плоскость")).color(Color32::from_rgb(200, 120, 0)));
            } else if *body != 0 && ui.button(tr("Указать грань в окне")).clicked() {
                cmd.pick = PickTarget::AlignTarget;
            }
            if ui.button(tr("⇅ Перевернуть на другую сторону")).clicked() {
                *target = target.flipped();
                changed = true;
            }
            ui.label(RichText::new(tr("Как «Выровнять» во Fusion: грань ложится на грань или плоскость, точка щелчка совпадает с точкой назначения")).weak().small());
        }
        FeatureKind::PressPull { distance, .. } => {
            section(ui, tr("ГРАНЬ"));
            face_pick_row(ui, &mut cmd.pick, true, tr("Щёлкните плоскую грань тела"));
            section(ui, tr("РАЗМЕРЫ"));
            changed |= row(ui, tr("Расстояние"), |ui| expr_field(ui, ("press_pull", uid, 0), distance, p, tr("мм")));
            ui.label(RichText::new(tr("Больше нуля — вытянуть грань наружу, меньше нуля — вдавить внутрь")).weak().small());
            if ui.button(tr("⇅ Обратное направление")).clicked() {
                *distance = negate(distance);
                changed = true;
            }
        }
        FeatureKind::Hole { plane, at, dia, depth, kind } => {
            section(ui, tr("ПОЛОЖЕНИЕ"));
            ui.label(RichText::new(tr("Щёлкните грань: центр и оси привязываются автоматически")).weak());
            for (i, a) in ["X", "Y"].iter().enumerate() {
                changed |= row(ui, a, |ui| expr_field(ui, ("hole_at", uid, i), &mut at[i], p, "мм"));
            }
            // быстрая привязка по габаритам детали на этой плоскости: углы, середины сторон, центр
            if let Some(idx) = my_index {
                let mut lo = [f64::MAX; 2];
                let mut hi = [f64::MIN; 2];
                for b in &ev.model_before(idx).bodies {
                    for vertex in &b.mesh().vertices {
                        let vertex=glam::DVec3::from(*vertex);
                        if (vertex-glam::DVec3::from(plane.origin)).dot(plane.normal()).abs()>1e-4 {continue;}
                        let q=plane.to_local(vertex);
                        for a in 0..2 {lo[a]=lo[a].min(q[a]);hi[a]=hi[a].max(q[a]);}
                    }
                }
                if lo[0] < hi[0] {
                    let inset = dia.eval(p).unwrap_or(8.0) / 2.0 + 2.0;
                    if ui.button(tr("В центр грани")).clicked() {
                        at[0]=num((lo[0]+hi[0])*0.5);at[1]=num((lo[1]+hi[1])*0.5);changed=true;
                    }
                    ui.label(RichText::new(tr("Привязка к детали")).small().weak());
                    egui::Grid::new(("hole_snap", uid)).spacing([3.0, 3.0]).show(ui, |ui| {
                        for ry in (0..3).rev() {
                            for rx in 0..3 {
                                let pick = |a: usize, r: usize| match r {
                                    0 => lo[a] + inset,
                                    1 => (lo[a] + hi[a]) / 2.0,
                                    _ => hi[a] - inset,
                                };
                                let label = ["↙", "↓", "↘", "←", "●", "→", "↖", "↑", "↗"][(2 - ry) * 3 + rx];
                                if ui.add_sized([30.0, 26.0], egui::Button::new(label)).on_hover_text(tr("Поставить отверстие сюда (отступ от края — радиус + 2 мм)")).clicked() {
                                    at[0] = num((pick(0, rx) * 100.0).round() / 100.0);
                                    at[1] = num((pick(1, ry) * 100.0).round() / 100.0);
                                    changed = true;
                                }
                            }
                            ui.end_row();
                        }
                    });
                }
            }
            section(ui, tr("РАЗМЕРЫ"));
            changed |= row(ui, tr("Диаметр"), |ui| expr_field(ui, ("hole_d", uid), dia, p, "мм"));
            let mut through = depth.is_none();
            if crate::widgets::checkbox(ui, &mut through, tr("Насквозь")).changed() {
                *depth = if through { None } else { Some(num(10.0)) };
                changed = true;
            }
            if let Some(d) = depth {
                changed |= row(ui, tr("Глубина"), |ui| expr_field(ui, ("hole_depth", uid), d, p, "мм"));
            }
            section(ui, tr("ТИП"));
            let mut which = match kind {
                HoleKind::Simple => 0,
                HoleKind::Counterbore { .. } => 1,
                HoleKind::Countersink { .. } => 2,
            };
            let names = [tr("Простое"), tr("С цековкой"), tr("С зенковкой")];
            ui.horizontal(|ui| {
                for (i, n) in names.iter().enumerate() {
                    if ui.selectable_label(which == i, *n).clicked() && which != i {
                        which = i;
                        *kind = match i {
                            0 => HoleKind::Simple,
                            1 => HoleKind::Counterbore { dia: num(16.0), depth: num(3.0) },
                            _ => HoleKind::Countersink { dia: num(16.0), angle: num(90.0) },
                        };
                        changed = true;
                    }
                }
            });
            match kind {
                HoleKind::Simple => {}
                HoleKind::Counterbore { dia: d2, depth: h2 } => {
                    changed |= row(ui, tr("Диаметр цековки"), |ui| expr_field(ui, ("cb_d", uid), d2, p, "мм"));
                    changed |= row(ui, tr("Глубина цековки"), |ui| expr_field(ui, ("cb_h", uid), h2, p, "мм"));
                }
                HoleKind::Countersink { dia: d2, angle } => {
                    changed |= row(ui, tr("Диаметр зенковки"), |ui| expr_field(ui, ("cs_d", uid), d2, p, "мм"));
                    changed |= row(ui, tr("Угол конуса"), |ui| expr_field(ui, ("cs_a", uid), angle, p, "°"));
                }
            }
        }
        FeatureKind::Coil { at, axis, dia, pitch, turns, section: sec, op } => {
            section(ui, tr("ПОЛОЖЕНИЕ"));
            for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                changed |= row(ui, a, |ui| expr_field(ui, ("coil_at", uid, i), &mut at[i], p, "мм"));
            }
            changed |= row(ui, tr("Ось"), |ui| {
                let mut c = false;
                for a in [Axis3::X, Axis3::Y, Axis3::Z] {
                    c |= ui.selectable_value(axis, a, a.label()).changed();
                }
                c
            });
            section(ui, tr("РАЗМЕРЫ"));
            changed |= row(ui, tr("Диаметр спирали"), |ui| expr_field(ui, ("coil", uid, 0), dia, p, "мм"));
            changed |= row(ui, tr("Шаг"), |ui| expr_field(ui, ("coil", uid, 1), pitch, p, "мм"));
            changed |= row(ui, tr("Число витков"), |ui| expr_field(ui, ("coil", uid, 2), turns, p, ""));
            changed |= row(ui, tr("Диаметр сечения"), |ui| expr_field(ui, ("coil", uid, 3), sec, p, "мм"));
            if op_combo(ui, "coil_op", op) {
                changed = true;
                cmd.auto_op = false;
            }
        }
        FeatureKind::Pipe { path, outer, wall, op, shape, corner, bend } => {
            section(ui, tr("ПУТЬ"));
            if *path == 0 {
                ui.label(RichText::new(tr("Не выбрано — щёлкните линию чертежа")).color(Color32::from_rgb(200, 120, 0)));
            }
            changed |= sketch_combo(ui, "pipe_path", path, sketches, None);
            section(ui, tr("СЕЧЕНИЕ"));
            changed |= row(ui, tr("Форма"), |ui| {
                let mut c = false;
                let names = [(PipeShape::Round, tr("Круглая")), (PipeShape::Square, tr("Квадратная"))];
                egui::ComboBox::from_id_salt("pipe_shape").selected_text(names.iter().find(|(k, _)| k == shape).map(|(_, n)| *n).unwrap_or("")).show_ui(ui, |ui| {
                    for (k, n) in names { c |= ui.selectable_value(shape, k, n).changed(); }
                });
                c
            });
            changed |= row(ui, tr("Внешний размер"), |ui| expr_field(ui, ("pipe", uid, 0), outer, p, "мм"));
            changed |= row(ui, tr("Толщина стенки"), |ui| expr_field(ui, ("pipe", uid, 1), wall, p, "мм"));
            ui.label(RichText::new(tr("Стенка 0 — сплошной стержень")).weak().small());
            section(ui, tr("УГЛЫ"));
            if *shape == PipeShape::Square {
                changed |= row(ui, tr("Угол сечения"), |ui| expr_field(ui, ("pipe", uid, 2), corner, p, "мм"));
            }
            changed |= row(ui, tr("Изгиб пути"), |ui| expr_field(ui, ("pipe", uid, 3), bend, p, "мм"));
            ui.label(RichText::new(tr("Радиус скругления изгибов пути; 0 — острые углы")).weak().small());
            if op_combo(ui, "pipe_op", op) {
                changed = true;
                cmd.auto_op = false;
            }
        }
        FeatureKind::Sweep { sketch, profiles, path, op } => {
            section(ui, tr("ПРОФИЛЬ"));
            ui.label(if profiles.is_empty() { RichText::new(tr("Не выбрано — щёлкните область чертежа")).color(Color32::from_rgb(200, 120, 0)) } else { RichText::new(tf!("Выбрано: {}", profiles.len())) });
            section(ui, tr("ПУТЬ"));
            changed |= sketch_combo(ui, "sweep_path", path, sketches, Some(*sketch));
            if op_combo(ui, "sweep_op", op) {
                changed = true;
                cmd.auto_op = false;
            }
        }
        FeatureKind::Loft { profiles, op } => {
            section(ui, tr("ПРОФИЛИ ПО ПОРЯДКУ"));
            if profiles.is_empty() {
                ui.label(RichText::new(tr("Щёлкните профиль в первом чертеже, затем во втором…")).color(Color32::from_rgb(200, 120, 0)));
            }
            let mut remove = None;
            for (i, (s, _)) in profiles.iter().enumerate() {
                ui.horizontal(|ui| {
                    let name = sketches.iter().find(|(u, _)| u == s).map(|(_, n)| n.clone()).unwrap_or_default();
                    ui.label(format!("{}. {name}", i + 1));
                    if ui.small_button("×").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                profiles.remove(i);
                changed = true;
            }
            if op_combo(ui, "loft_op", op) {
                changed = true;
                cmd.auto_op = false;
            }
        }
        FeatureKind::Scale { bodies, factor, uniform } => {
            bodies_ui(ui, bodies, body_name, &mut changed);
            section(ui, tr("МАСШТАБ"));
            changed |= crate::widgets::checkbox(ui, uniform, tr("Одинаково по всем осям")).changed();
            if *uniform {
                changed |= row(ui, tr("Коэффициент"), |ui| expr_field(ui, ("scale", uid, 0), &mut factor[0], p, "×"));
            } else {
                for (i, a) in ["X", "Y", "Z"].iter().enumerate() {
                    changed |= row(ui, a, |ui| expr_field(ui, ("scale", uid, i), &mut factor[i], p, "×"));
                }
            }
        }
        FeatureKind::Chamfer { body, edges, dist: size } | FeatureKind::Fillet { body, edges, radius: size } => {
            let fillet = matches!(f_is_fillet, true);
            let _ = fillet;
            section(ui, tr("ТЕЛО"));
            ui.label(body_name(*body));
            section(ui, tr("РЁБРА"));
            ui.label(if edges.is_empty() { RichText::new(tr("Не выбрано — щёлкните ребро")).color(Color32::from_rgb(200, 120, 0)) } else { RichText::new(tf!("Выбрано: {}", edges.len())) });
            if !edges.is_empty() && ui.small_button(tr("Сбросить выбор")).clicked() {
                edges.clear();
                changed = true;
            }
            if let Some(idx) = my_index {
                if ui.button(tr("Выбрать все рёбра")).on_hover_text(tr("Все рёбра тела, включая внутренние (вогнутые) и круглые")).clicked() {
                    if let Some(b) = ev.model_before(idx).body(*body) {
                        *edges = crate::app::compute_straight_edges(b.mesh());
                        changed = true;
                    }
                }
            }
            section(ui, tr("РАЗМЕР"));
            changed |= row(ui, if f_is_fillet { tr("Радиус") } else { tr("Расстояние") }, |ui| expr_field(ui, ("edge_size", uid), size, p, "мм"));
            ui.label(RichText::new(tr("Работают прямые и круглые рёбра между гранями, выпуклые и вогнутые")).weak().small());
        }
        FeatureKind::OffsetPlane { offset, .. } => {
            section(ui, tr("ОСНОВА"));
            ui.label(RichText::new(tr("Щёлкните грань тела или плоскость начала координат")).weak());
            section(ui, tr("СМЕЩЕНИЕ"));
            changed |= row(ui, tr("Расстояние"), |ui| expr_field(ui, ("plane_off", uid), offset, p, "мм"));
        }
        FeatureKind::Shell { thickness, open, .. } => {
            section(ui, tr("ОТКРЫТАЯ ГРАНЬ"));
            changed |= crate::widgets::checkbox(ui, open, tr("Оставить грань открытой")).changed();
            if *open {
                ui.label(RichText::new(tr("Щёлкните плоскую грань, которая останется без стенки")).weak());
            }
            section(ui, tr("ТОЛЩИНА СТЕНКИ"));
            changed |= row(ui, tr("Толщина"), |ui| expr_field(ui, ("shell_t", uid), thickness, p, "мм"));
            ui.label(RichText::new(tr("Может занять несколько секунд на сложных телах")).weak().small());
        }
        FeatureKind::Sketch { .. } => {}
    }

    if let Some(err) = ev.error(uid).filter(|_| !awaiting_pick(&f.kind)) {
        ui.add_space(4.0);
        ui.label(RichText::new(format!("⚠ {}", tr_dyn(err))).color(Color32::from_rgb(200, 40, 40)));
    }
    ui.add_space(6.0);
    ui.separator();
    let mut result = if changed { CmdResult::Changed } else { CmdResult::None };
    ui.add_space(6.0);
    // same pair as in the sketch: filled primary pill and a text «Отмена», both centred on one line
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if crate::ui_kit::b01(ui, "OK", 88.0).on_hover_text("Enter").clicked() {
            result = CmdResult::Ok;
        }
        if crate::ui_kit::b03(ui, tr("Отмена")).on_hover_text("Esc").clicked() {
            result = CmdResult::Cancel;
        }
    });
    ui.add_space(4.0);
    result
}

fn bodies_ui(ui: &mut egui::Ui, bodies: &mut Vec<u64>, body_name: &dyn Fn(u64) -> String, changed: &mut bool) {
    section(ui, tr("ТЕЛА"));
    if bodies.is_empty() {
        ui.label(RichText::new(tr("Не выбрано — щёлкните тело в окне")).color(Color32::from_rgb(200, 120, 0)));
    } else {
        let mut remove = None;
        for (i, b) in bodies.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(body_name(*b));
                if ui.small_button("×").on_hover_text(tr("Убрать из выбора")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            bodies.remove(i);
            *changed = true;
        }
    }
}

/// Выбор чертежа из списка (для пути).
fn sketch_combo(ui: &mut egui::Ui, id: &str, current: &mut u64, sketches: &[(u64, String)], exclude: Option<u64>) -> bool {
    let mut changed = false;
    let name = sketches.iter().find(|(u, _)| u == current).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("выберите чертёж").to_string());
    ui.horizontal(|ui| {
        field_label(ui, tr("Чертёж пути"));
        egui::ComboBox::from_id_salt(id).selected_text(name).show_ui(ui, |ui| {
            for (u, n) in sketches.iter().filter(|(u, _)| Some(*u) != exclude) {
                changed |= ui.selectable_value(current, *u, n.as_str()).changed();
            }
        });
    });
    changed
}

/// Грань уже выбрана — «Выбрать другую грань»; во время выбора — подсказка.
fn face_pick_row(ui: &mut egui::Ui, pick: &mut PickTarget, has_face: bool, hint: &str) {
    if *pick == PickTarget::Face || !has_face {
        *pick = PickTarget::Face;
        ui.label(RichText::new(hint).color(Color32::from_rgb(200, 120, 0)));
    } else {
        ui.horizontal(|ui| {
            ui.label(tr("Грань выбрана"));
            if ui.button(tr("Выбрать другую")).clicked() { *pick = PickTarget::Face; }
        });
    }
}

/// Пользователь ещё не выбрал то, без чего операция не строится: ошибку не показываем, ждём щелчка.
pub fn awaiting_pick(kind: &FeatureKind) -> bool {
    match kind {
        FeatureKind::Extrude { profiles, face, .. } => profiles.is_empty() && face.is_none(),
        FeatureKind::Revolve { profiles, .. } => profiles.is_empty(),
        FeatureKind::Sweep { profiles, path, .. } => profiles.is_empty() || *path == 0,
        FeatureKind::Pipe { path, .. } => *path == 0,
        FeatureKind::Loft { profiles, .. } => profiles.len() < 2,
        FeatureKind::Chamfer { edges, .. } | FeatureKind::Fillet { edges, .. } => edges.is_empty(),
        FeatureKind::PlaceOnPlane { body, .. } => *body == 0,
        FeatureKind::Move { bodies, .. } | FeatureKind::Mirror { bodies, .. } | FeatureKind::Pattern { bodies, .. } | FeatureKind::Scale { bodies, .. } | FeatureKind::Remove { bodies } => bodies.is_empty(),
        FeatureKind::Combine { target, tools, .. } => *target == 0 || tools.is_empty(),
        _ => false,
    }
}

/// Сведения о документе для заготовки новой операции.
pub struct Ctx {
    pub has_bodies: bool,
    pub selected: Vec<u64>,
    pub last_sketch: Option<(u64, Vec<fusion_core::sketch::ProfileRef>)>,
    /// Верхняя грань первого тела и центр на ней (для заготовки отверстия).
    pub top_face: Option<(fusion_core::Plane, [f64; 2])>,
}

/// Шаг по умолчанию для новой команды.
pub fn default_feature(kind: &str, c: &Ctx) -> FeatureKind {
    let op = if c.has_bodies { Op::Join } else { Op::NewBody };
    let zero = || [num(0.0), num(0.0), num(0.0)];
    let selected = &c.selected;
    match kind {
        "extrude" => {
            let (sketch, profiles) = c.last_sketch.clone().unwrap_or((0, vec![]));
            FeatureKind::Extrude { sketch, profiles, extent: Extent::OneSide(num(10.0)), op, face: None }
        }
        "revolve" => {
            let (sketch, profiles) = c.last_sketch.clone().unwrap_or((0, vec![]));
            FeatureKind::Revolve { sketch, profiles, axis: RevolveAxis::SketchY, angle: num(360.0), op }
        }
        "sweep" => {
            let (sketch, profiles) = c.last_sketch.clone().unwrap_or((0, vec![]));
            FeatureKind::Sweep { sketch, profiles, path: 0, op }
        }
        "shell" => FeatureKind::Shell { plane: c.top_face.map(|f| f.0).unwrap_or(fusion_core::Plane::XY), thickness: num(2.0), open: true },
        "plane" => FeatureKind::OffsetPlane { base: fusion_core::Plane::XY, offset: num(10.0) },
        "loft" => FeatureKind::Loft { profiles: vec![], op },
        "pipe" => FeatureKind::Pipe { path: 0, outer: num(10.0), wall: num(1.5), op: Op::NewBody, shape: fusion_core::doc::PipeShape::Round, corner: num(0.0), bend: num(0.0) },
        "box" => FeatureKind::Primitive { shape: Shape::Box { w: num(20.0), d: num(20.0), h: num(20.0) }, at: zero(), op },
        "cylinder" => FeatureKind::Primitive { shape: Shape::Cylinder { r: num(10.0), h: num(20.0) }, at: zero(), op },
        "sphere" => FeatureKind::Primitive { shape: Shape::Sphere { r: num(10.0) }, at: zero(), op },
        "torus" => FeatureKind::Primitive { shape: Shape::Torus { major: num(15.0), minor: num(4.0) }, at: zero(), op },
        "coil" => FeatureKind::Coil { at: zero(), axis: Axis3::Z, dia: num(20.0), pitch: num(6.0), turns: num(5.0), section: num(2.0), op: Op::NewBody },
        "hole" => {
            let (plane, at) = c.top_face.unwrap_or((fusion_core::Plane::XY, [0.0, 0.0]));
            FeatureKind::Hole { plane, at: [num(at[0].round()), num(at[1].round())], dia: num(8.0), depth: None, kind: HoleKind::Simple }
        }
        "place" => {
            let (face, at) = c.top_face.unwrap_or((fusion_core::Plane::XY, [0.0, 0.0]));
            FeatureKind::PlaceOnPlane { body: selected.first().copied().unwrap_or(0), face, seed: face.to_world(at).to_array(), target: fusion_core::Plane::XY, to: None }
        }
        "press_pull" => {
            let (plane, at) = c.top_face.unwrap_or((fusion_core::Plane::XY, [0.0, 0.0]));
            FeatureKind::PressPull { plane, seed: plane.to_world(at).to_array(), distance: num(5.0) }
        }
        "move" => FeatureKind::Move { bodies: selected.to_vec(), translate: zero(), rotate: zero(), copy: false },
        "scale" => FeatureKind::Scale { bodies: selected.to_vec(), factor: [num(2.0), num(1.0), num(1.0)], uniform: true },
        "mirror" => FeatureKind::Mirror { bodies: selected.to_vec(), normal: Axis3::X, join: false },
        "pattern" => FeatureKind::Pattern { bodies: selected.to_vec(), kind: PatternKind::Rect { nx: num(3.0), dx: num(30.0), ny: num(1.0), dy: num(30.0) } },
        "chamfer" => FeatureKind::Chamfer { body: selected.first().copied().unwrap_or(0), edges: vec![], dist: num(1.0) },
        "fillet" => FeatureKind::Fillet { body: selected.first().copied().unwrap_or(0), edges: vec![], radius: num(2.0) },
        "combine" => FeatureKind::Combine {
            target: selected.first().copied().unwrap_or(0),
            tools: selected.iter().skip(1).copied().collect(),
            op: BoolOp::Union,
            keep_tools: false,
        },
        _ => FeatureKind::Remove { bodies: selected.to_vec() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negate_roundtrip() {
        assert_eq!(negate(&Expr::num(10.0)).0, "-10");
        assert_eq!(negate(&Expr::from("w")).0, "-(w)");
        assert_eq!(negate(&negate(&Expr::from("w"))).0, "w");
    }
}
