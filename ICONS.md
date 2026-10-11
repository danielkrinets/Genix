# Список значков для Genix

Формат: SVG 24×24, линия 1.6, круглые концы, один цвет (чёрный — программа сама перекрасит под тему). Имя файла = id + `.svg`, папка `crates/app/assets/icons/`.
Сейчас подключены только значки группы A (кроме `shell` и пр. — они уже лежат). Остальные я подключу в программу, когда вы их нарисуете: до тех пор используются текстовые символы.

## A. Панель «Деталь» (24 шт, временные уже есть — можно заменить)
sketch, extrude, revolve, sweep, loft, hole, box, cylinder, sphere, torus, coil, pipe, mirror, pattern, move, scale, fillet, chamfer, shell, combine, remove, plane, measure, params

## B. Режим чертежа — инструменты
sk_select, sk_line, sk_rect, sk_rect_center, sk_circle, sk_arc, sk_polygon, sk_slot, sk_trim, sk_offset, sk_corner, sk_dim, sk_angle, sk_finish, sk_photo, sk_files, sk_dxf_import, sk_dxf_export, sk_construction

## C. Ограничения чертежа
c_horizontal, c_vertical, c_parallel, c_perpendicular, c_tangent, c_equal, c_concentric, c_coincident, c_midpoint, c_fix, c_point_on

## D. Нижняя панель вида и куб
nav_orbit, nav_pan, nav_zoom, nav_fit, nav_zoom_in, nav_zoom_out, nav_style, nav_grid, nav_settings, nav_home, nav_panel_left, nav_panel_right, nav_ortho

## E. Дерево слева (папки и элементы)
tree_folder, tree_folder_open, tree_doc_settings, tree_views, tree_origin, tree_bodies, tree_sketches, tree_params, tree_body, tree_sketch, tree_plane, tree_axis, tree_eye, tree_eye_off, tree_arrow_right, tree_arrow_down, tree_lock

## F. История (таймлайн)
tl_first, tl_prev, tl_play, tl_pause, tl_next, tl_last, tl_marker

## G. Меню и общие действия
file_new, file_open, file_save, file_saveas, file_export_stl, file_export_obj, undo, redo, example, help, settings, close, add, delete, rename, hide, show, edit, suppress, rollback, search

## H. Статусы и подсказки
st_ok, st_warning, st_error, st_info, st_constrained, st_free

## I. Панель свойств
prop_material, prop_mass, prop_volume, prop_area, prop_bbox

## J. Значок приложения (отдельно)
- `app.svg` и PNG 1024×1024 (скруглённый квадрат, фон сплошной) — из него делаются .icns (macOS), .ico (Windows), PNG (Linux).

## K. На будущее (когда появятся эти функции)
Чертёж: sk_ellipse, sk_spline, sk_conic, sk_point, sk_text, sk_mirror, sk_pattern_circ, sk_pattern_rect, sk_project, sk_extend, sk_break, sk_scale, sk_move, sk_rotate, sk_midpoint_line, sk_rect_3pt, sk_circle_2pt, sk_circle_3pt, sk_arc_3pt, sk_arc_tangent
Тело: thread, rib, web, emboss, press_pull, boundary_fill, draft, split_body, split_face, align, replace_face, offset_face, delete_face, pattern_path
Опоры: plane_angle, plane_mid, plane_tangent, plane_3pts, axis_cylinder, axis_2pts, axis_perp, point
Проверка: section, interference, center_of_mass, zebra, curvature, draft_analysis
Поверхности: surf_extrude, surf_patch, surf_ruled, surf_offset, surf_trim, surf_stitch, surf_thicken
Сетка: mesh_repair, mesh_reduce, mesh_remesh, mesh_to_solid
Сборка: joint_rigid, joint_revolute, joint_slider, joint_cylindrical, joint_pin_slot, joint_planar, joint_ball, component
Вставка: insert_step, insert_stl, insert_svg, insert_image
Рабочие среды (вкладки): ws_design, ws_surface, ws_mesh, ws_assemble, ws_drawing, ws_manufacture
