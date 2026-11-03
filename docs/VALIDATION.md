# Проверка личного пространства Genix

Дата: 18 октября 2026. Локальная сборка для macOS Apple Silicon (arm64).

- `cargo test --workspace --offline`: 41 тест приложения и 64 теста ядра прошли; один существующий тяжёлый тест отключён по умолчанию.
- Release-сборка, упаковка ZIP и проверка локальной подписи `codesign --verify --deep --strict` успешны. ZIP проверен на целостность. Это ad-hoc подпись, не Developer ID и не нотаризация Apple.
- В запущенном приложении проверены регистрация с тестовыми данными, библиотека с реальными превью, профиль и сохранение имени, демо-покупка с подтверждением, выход, настройки, получение реального ответа GitHub, открытие модели в редакторе, сохранение и возврат в библиотеку.
- Автоматический сценарий проверяет создание кронштейна, запись `.genix`, повторное открытие, изменение параметра, перестроение и экспорт корректного двоичного STL.
- Тесты сохранения проверяют замену файла, очистку временного файла после ошибки, защиту неактивных вкладок при выходе, отмену выхода, ошибку записи и автосохранение неактивной вкладки.

## Версии 0.00.22–0.00.24 (3 ноября 2026)

Новое: «Разрезать» (плоскость по оси X/Y/Z делит тело на два), «Уклон» (боковые стенки наклоняются от нейтральной плоскости), «Резьба» (винтовая резьба по осям X/Y/Z, Join/NewBody), «Импорт STL» (меню «Файл», перетаскивание в окно, «Открыть с помощью» в Finder; данные хранятся внутри проекта).

Автоматические проверки (app 93, core 95 — все проходят): сумма объёмов при разрезе и ошибка, если плоскость не пересекает тело; уклон ящика (точные координаты верха и формула объёма усечённой пирамиды), цилиндра в обе стороны, тела со скруглением, отказ при угле ≥ 45°; резьба на трёх осях (объём между сердечником и полным цилиндром, длина), присоединение к телу, ошибка при слишком глубокой резьбе; STL бинарный и текстовый, перевёрнутые нормали, мусорный файл, обрезанный файл; JSON-круг проекта с импортом; незамкнутая сетка даёт понятную ошибку; кронштейн проходит экспорт STL → импорт с тем же объёмом (1e-4); отмена импорта; плохой файл не меняет проект.

Не проверено вручную в окне: диалог «Импорт STL…», перетаскивание файла, «Открыть с помощью» (обновлённый Info.plist проверен plutil, но не LaunchServices). STL с дырами или самопересечениями не импортируется (нужно замкнутое тело). Единицы STL считаются миллиметрами. Уклон — по вершинам, поэтому на сильно скошенных гранях он приближённый.

## Версия 0.00.21 (2 ноября 2026)

- `cargo test --workspace --offline --release`: 91 тест приложения и 86 тестов ядра прошли; 4 тяжёлых теста отключены по умолчанию (замер времени, пробные рендеры).
- Новые проверки: выбор рёбер по одному и скругление 12 рёбер пластины, запуск каждого инструмента панели, экспорт OBJ/3MF, быстрый отказ скругления со слишком большим числом рёбер, пересчёт в фоновом потоке.
- Вручную в окне (macOS arm64): «Создать папку» без формы, клик по ребру и по грани, применение скругления, стрелки радиуса дают целые значения. Остальные команды проверены только автоматическими тестами.

## Независимая финальная проверка

**Disposition: Pass.** Замечание по контрасту второстепенного текста устранено. Оставшихся существенных замечаний в рамках проверки нет.

| Требование | Итог |
|---|---|
| Мок регистрации → библиотека | Resolved |
| Настоящие превью моделей | Resolved |
| Навигация: настройки, профиль, обновления, сервер | Resolved |
| Редактирование локального профиля | Resolved |
| Мок-подписка с явными ограничениями | Resolved |
| Установленная версия и официальные сборки | Resolved |
| Существующий CAD-редактор сохранён | Resolved |
| Замечание по контрасту второстепенного текста | Resolved |
| Исчерпывающая проверка всех состояний и платформ | Partial |

Тёмная тема, мобильные устройства, узкие окна и все возможные состояния взаимодействия визуально не проверялись. Windows/Linux/iOS в этом запуске не собирались.

## Ограничения продукта

Регистрация, вход и серверная подписка — локальные моки. Пароли не сохраняются и не передаются. Реальных платежей, серверного хранения и синхронизации нет. Обновления проверяются через публичный GitHub API; установка вручную со страницы официальной сборки. Локальные изменения пока не отправлены на GitHub.

## Обновление оформления: flat rounded

Добавлены пять акцентных цветов, скруглённые плоские поверхности, док редактора, переключатели, акцентные слайдеры и индикаторы загрузки. Анимации обратной связи занимают 160 мс и учитывают уменьшение движения. Проверены контраст всех вариантов акцента и управление переключателем мышью и клавиатурой. Всего 105 тестов проходят.

После разблокировки Mac проверены библиотека с реальными превью, открытие модели в редакторе, док, смена акцента с синего на индиго, светлая и тёмная темы, настройки и акцентный слайдер. Найдена и исправлена потеря оформления при запуске из-за выбора системной темы egui; добавлен регрессионный тест. Исправлены контраст выбранных вкладок и скругление активных кнопок дока. Новые скриншоты имеют префикс rounded-.

Независимая проверка исходного кода редизайна: Pass. Исправлен отдельный видимый фокус клавиатуры у акцентных цветов; существенных замечаний по коду не осталось. Основные экраны проверены визуально на macOS; все состояния и платформы не проверялись.

Повторная независимая проверка: Pass для новых скриншотов тёмной библиотеки и редактора и исходного кода исправлений. Существенных замечаний нет. Последнее скругление кнопки сетки подтверждено по коду и сборке; отдельный скриншот после этой правки не снят.

## Расширенный поиск и обратная связь

108 тестов проходят: 44 приложения и 64 ядра; один прежний stress-тест игнорируется. Добавлены проверки сочетаний поисковых слов, поиска по пути, доступности/читаемости, числа операций и даты изменения. Переключатель и новая галочка проверены мышью и Space. Отдельный тест подтверждает непрерывный разворот анимации.

В Genix QA на macOS проверены раскрытие фильтров, выбор галочек, минимум 5 операций для модели с 4 операциями (0 результатов), сброс (модель возвращается), очистка текстового поиска и список сортировок. Снимок: screenshots/advanced-search.jpg. Временные промежуточные кадры анимации не записывались; общая длительность 160 мс, reduced-motion и клавиатура завершают переход сразу.

## Фоны по референсам и spacing

Нативно адаптированы Pattern Waves (буквы Genix) и Shape Waves (круги, вырезанная надпись) из React Bits; лицензия сохранена в docs/REACT_BITS_LICENSE.md, включена в приложение и ZIP. В Genix QA визуально проверены регистрация без иконки логотипа, увеличенные поля, переход без аккаунта, буквенный фон библиотеки и широкая отдельная строка поиска. Снимок библиотеки: screenshots/waves-library.jpg.

Фоны статичны при уменьшении движения, repaint ограничен 33 мс и запрашивается только при фокусе окна. Это ограничение частоты запросов, не измеренный FPS. Добавлены тесты отрисовки обоих фонов и длительности галочки (50% на 160 мс, завершение на 320 мс).

Итог этого этапа: 110 тестов (46 приложения, 64 ядра), один прежний stress-тест игнорируется. Скриншот регистрации: screenshots/waves-registration.jpg. На последней итерации яркость буквенного фона дополнительно ограничена для читаемости второстепенных подписей; снимок библиотеки сделан до этого небольшого уменьшения яркости.

## Реакция фонов на курсор

Добавлены пассивная подсветка под мышью, локальное увеличение кругов и затухающие кольца от движения/нажатия. В reduced-motion остаётся свет без смещения и роста. Тест PointerMoved сравнивает отрисовку обоих фонов при фиксированном времени: результат изменяется от мыши, а не от фонового времени. Всего 112 тестов проходят (48 приложения, 64 ядра), один прежний stress-тест игнорируется.

Приложение с исправлением запущено; повторные попытки управлять курсором в Genix QA прерывались защитой от параллельного пользовательского ввода, поэтому управляемая визуальная проверка мышью не завершена. Release и ZIP успешно собраны и проверены.

## CAD, Finder и исправления интерфейса

Итог: 117 тестов проходят (53 приложения, 64 ядра), один прежний stress-тест игнорируется. Новые тесты восстанавливают четыре открытые полукруглые кромки капсулы и строят ограниченные инструменты скругления с проверкой объёма. Диаметр полукруга не превращает дугу в полную окружность. Центровка проверяется на реальной плоской грани, повторное открытие проекта не создаёт дубликат вкладки. Точный пользовательский файл «косточка» не предоставлен: регрессия воспроизведена на программно построенных аналогах.

Проверены запуск из macOS LaunchServices с .genix при закрытом приложении (открылась модель «Кронштейн») и открытие второго finder-warm.genix в запущенном приложении (новая вкладка). Холодный обработчик регистрируется до запуска AppKit и повторно на will-finish-launching; делегат Winit не подменяется. Старые экземпляры Genix QA были остановлены перед финальной проверкой. Данные пользователя не изменялись.

В окне проверены светлая тема и контраст буквенного фона, увеличенные подписи, панель отверстия с кнопкой «В центр грани» и сообщением о привязках. Снимок панели: screenshots/hole-placement.jpg. Лупа получила отдельный отступ; радиус подсветки меню — 6px. Перетаскивание файлов реализовано через RawInput.dropped_files; отдельная ручная drag-and-drop проверка пока не проводилась.

Добавлен CLAUDE.md с архитектурой, инструкциями сборки, ограничениями моков, совместимостью формата дуг и правилами нативного QA. Исторические утверждения выше об отсутствии отправки на GitHub относятся к предыдущим этапам; итог публикации фиксируется в сообщении о завершении.

Финальная визуальная проверка: лупа отделена от текста, подписи читаемы, рисунок виден в светлой теме. Снимок: screenshots/library-light-fixed.jpg. Финальная локальная Apple Silicon ZIP-сборка проверена codesign и unzip.

## Полноценный workspace и CAD-взаимодействия

118 активных тестов проходят: 54 приложения и 64 ядра; один тяжёлый stress-тест ядра остаётся игнорируемым. Добавлены регрессии для сохранения материалов/тел-резаков и для пересечения инструментов скругления соседних круглых кромок. Release-сборка и `dist/Genix-macOS.zip` созданы офлайн.

В живом `Genix.app` проверены: белая подпись активной кнопки «Фильтры» на синем фоне, раскрытие расширенных фильтров, отдельный экран настроек с выходом из аккаунта, выбор материала и переключатель прозрачного тела-отверстия. Нативная строка macOS содержит Genix / Файл / Правка / Вид; в «Файл» доступны Новый проект, Открыть, Сохранить и Сохранить как. Контрольные снимки лежат в `docs/screenshots/review-2026-10-18/`.


## Ветка feature/backgrounds, 20 октября 2026 (QA-копия на отдельном профиле)

Тесты: 71 приложения и 67 ядра проходят; один тяжёлый тест ядра игнорируется. Сборка release без предупреждений.

**Проверено глазами на macOS (скриншоты QA-копии):** поле поиска с «Фильтры ▾» внутри; боковая панель (выбранный пункт сплошной, без логотипа, без раскрытия по наведению); вкладки без обводки; регистрация со слизью (заливка, поворот головы при вводе пароля, без рук); большая кнопка «+» с меню «Новая модель / Новая папка» и окно выбора имени; палитра команд ⌘K; создание папки; режим чертежа (панель закреплена справа); освещение материалов (изменение заметно слабо на плоских гранях, лучше на отверстиях).

**Написано и покрыто тестами, но не осмотрено глазами:** группы вкладок; режим перемещения (Shift+клик, удержание, перетаскивание в папку); вращение карточки через 3 с; окно «Поделиться»; Team; билет-подписка; заставка первой загрузки; плашка «Открываем проект…»; открытие в слайсерах; серые пункты меню; версии файла; контекстные меню; поля значений над моделью; ползунок масштаба.

**Не проверялось:** Finder и нативное меню macOS, iPad/iPhone, подпись Apple.

## 0.00.27 — modeling batch 1 (checked 2026-11-03)

New: extrude **taper** (wall draft angle, also symmetric/downwards), extent **through all** (forward / both ways), **mirror about an offset plane**, **Combine → Split** (a tool body cuts the target in two), **plane at angle** (construction plane, usable for sketches), **interference check** (Inspect → Пересечения, toast with overlapping pairs and volumes).
Old files open unchanged: the new fields have serde defaults (`taper`, `offset`).
Tests: core 103 + app 95 pass (`cargo test --workspace --offline`); 8 new core tests in `modeling_batch_tests` / `interference_tests` check frustum volumes, through-all hole volume, mirror position, split volumes, angle-plane normal, default-field loading.
Live (QA copy `com.danielkrinets.genix.qa`, isolated settings): `crates/core/examples/modeling_demo.rs` writes a model with all of them; it opens without errors, tapered posts and the angled plane are visible, the Extrude panel shows the new "Уклон стенок" row.
Not checked by eye: the new "Насквозь" options in the direction drop-down (the background window did not redraw the open combo box), the interference toast, the angle-plane tool picked with the mouse.

## 0.00.28 — modeling batch 2 (checked 2026-11-03)

New: **thin extrude** (Extrude → «Тонкая стенка»: wall along the outline and around holes, also with taper), **3D grid pattern** (Count/Step along Z).
Tests: core 106 + app 95 pass. Live (QA copy): the demo model shows the thin-walled box and the 2×1×2 cube grid next to the tapered posts.
Not checked by eye: typing the new fields in the panel; thin extrude picked from a body face.

## 0.00.29 — bolt holes, tooltip pictures, command footer (checked 2026-11-03)

New: **hole for a bolt** — `HoleKind::Tapped` (tap drill + optional modeled thread groove, up to 40 mm), ISO metric table `crates/core/src/fasteners.rs` (M2–M24: clearance close/normal/loose, tap drill, DIN 974 counterbore, ISO 10642 countersink) and a «Под болт» block in the Hole panel. The 3x3 placement grid moved from the side panel to a card at the top centre of the viewport (arrows point towards the edge/corner, centre dot).
Tooltips now show before/after pictures drawn by the renderer (`cargo test -p fusion-app render_tutorial_pictures -- --ignored`, then `python3 tools/make_tips.py`): 21 tools, cross-fade loop, side by side with reduced motion.
Fix: OK/Cancel of every command now sit in a footer outside the scrolling fields (the side panel had no scroll, tall dialogs pushed OK off the screen).
Tests: core 109 + app 97. Live (QA copy): Hole panel shows the bolt block, tapped preset sets Ø5 / thread Ø6 / pitch 1; the corner button gave X 6, Y 34 on a 40x40 face; OK/Cancel stay at the bottom of the panel.
Not checked by eye: a modeled thread in a hole on screen, tooltips with pictures on hover, scrolling of a very tall panel.

## 0.00.30 — place card, value dragging (checked 2026-11-03)

«Наложить» got the same 3x3 card as the hole (middle / edge / corner of the target face). Fixed along the way: floating cards no longer let the model underneath react to the pointer (the origin plane below the card used to override the chosen target); the chosen anchor is kept as the state to return to.
Dragging a value by its unit or caption now scales with the value (5 mm moves 0.1 per 6 px, 400 mm moves 10; counts stay whole; at most 20 ticks per frame), so a value no longer flies away.
Tests: core 110 + app 98. Live (QA copy): the corner button put the 16 mm block into the corner of the 40 mm plate (top view); not checked: other anchors, rotated source faces (the inset assumes aligned axes).

## 0.00.31 — agent interface (checked 2026-11-03)

`genix --new / --features / --add / --eval / --export` (docs/AGENTS.md); logic in `crates/core/src/agent.rs`. A failed `--add` leaves the file untouched.
Tests: core 112 + app 99. Checked with the release binary: new → features → add (box from the catalog, via stdin) → export STL.

## 0.00.32 — extrude up to a body, start offset (checked 2026-11-03)

Extrude: extent «До тела» (to the nearest side of another body's bounding box) and «Смещение начала». Old files open unchanged (`start` defaults to 0).
Tests: core 113 + app 99. Not checked by eye in the window.

## 0.00.33 — sketch text, ellipse, spline (checked 2026-11-03)

Sketch tools «Эллипс» (centre, major semi-axis, a point on it), «Сплайн» (fit points, Enter finishes, a click on the first point closes), «Текст» (letters become profiles with their counters; DejaVu Sans, so Latin, Cyrillic, Greek, Hebrew). Core: `Entity::Ellipse`, `Entity::Spline` (consecutive fit points, centripetal Catmull-Rom), `Sketch::texts` + `crates/core/src/text.rs` (needs the `ab_glyph` crate that egui already uses). Ellipses/splines leave through DXF as polylines; trimming and offsetting ignore them. Old files open unchanged (`texts` defaults to empty).
Tests: core 120 + app 99: ellipse areas (also tilted), closed/open spline regions, text regions (O has 1 hole, 8 has 2, ii gives 4, Cyrillic), size/position/rotation, extrusion of a letter ring and of an oval post by volume.
Live (QA copy): the three tools show in the sketch toolbar with icons; the text tool previews «Текст» in Cyrillic, «Добавить» adds it (profiles 1 → 6, the list of texts with edit/delete appears); an ellipse was created.
Not checked by eye: the ellipse's third click (the automation sent it without a mouse move, so the ellipse came out flat), spline drawing, extruding a letter in the window (checked by tests only), constraints on ellipses/splines (none are supported: they are free shapes).

## 0.00.34 — STEP and DXF export (checked 2026-11-03)

File → «Экспорт STEP (САПР)…» and «Экспорт DXF (3D)…» (selected bodies or all visible ones), `genix --export model out.step|out.dxf`.
STEP = ISO 10303-21 AP214, each body a faceted B-rep (one planar face per triangle, millimetres, ASCII with \X2\ escapes for non-ASCII names). DXF = one 3DFACE per triangle, ASCII layer per body. Both carry the exact triangles: CAD programs see facets, not curved faces or features.
Tests: core 122 + app 99. Structural checks on the demo model (8 bodies, 1088 triangles): STEP ids are 1..n without gaps, no dangling references, balanced parentheses, one shell per body; DXF is valid group-code pairs with 1088 3DFACE.
**NOT checked in a real CAD program** (none is installed here and there is no STEP validator): open `demo.step` in Fusion/FreeCAD and report. The STEP skeleton follows the usual AP214 faceted-brep layout; if a program refuses it, the file header/product block is the first suspect.

## 0.00.35 — text fixes, flush placing, Hebrew (checked 2026-11-03)

- «Наложить»: the card now aligns the real footprint of the placed face flush against the middle / edge / corner of the target face, replaying the turn the body makes (`fusion_core::doc::place_anchor`). Test: seven anchors with a source face that ends up turned, flush within 1e-6 and standing on the plate.
- Text: a finished sketch now draws the letters in the 3D view (before, only the curves were drawn, so the text seemed to vanish); a text can be exploded into ordinary closed polylines (⛓ in the list, `Sketch::explode_text`); the text field takes the keyboard at once with the word selected, Enter adds, Esc cancels, sketch hotkeys stay off while a text is open (typing «Привет» used to switch tools), a second click only moves the line.
- Hebrew: sketch text is laid out right to left (shared `visual_rtl` now lives in core and the UI uses it), numbers stay readable, niqqud are dropped (the font has no mark positioning). Hebrew UI: 92 strings added for the new features; the Hebrew table still covers only ~240 of ~1000 strings, the rest shows English. Arabic needs shaping and is not supported. Typing Hebrew into an input field still shows it in logical order (egui text fields have no bidi).
- Hole panel redesigned (one type bar, screw size + fit, one summary line, «Свои размеры»); the sketch constraint hint is a caption under the buttons instead of a tooltip that covered them.
Tests: core 125 + app 99. Live QA of these was interrupted: the QA window was being clicked by hand at the same time, so the last screenshots show someone else's actions.

## 0.00.36–0.00.37 — sketch mirror, patterns, symmetric and collinear constraints (checked 2026-11-03)

Sketch: «Зеркало» tool (select curves, then click a line: mirror copies, arcs flip), «Массив из выбранного» panel (rectangular grid or circular ring, centre = selected point or the origin). Constraints «На линии» (two lines on one straight line) and «Симметрия» (two points, then the axis line). Old files open unchanged.
Tests (core 128 + app 99): mirrored rectangle regions and arc mid point; 3x2 grid = 5 copies / 6 regions, six circles evenly round a circle, a 4-copy 90° arc ends on the y axis, splines stay consecutive; collinear lines solve onto the x axis; symmetric point lands exactly on the mirror image (9,2) about x = 5 and the sketch becomes fully defined.
Not checked by eye in the window.

## 0.00.38 — thread on an existing cylinder (checked 2026-11-03)

Create → «Резьба по цилиндру»: click a round edge (the rim of a rod or the mouth of a hole); the thread runs from it along the cylinder for «Длина резьбы», external (ridge fused to the boss) or internal (groove cut into the wall), decided automatically by probing just inside/outside the wall. `FeatureKind::ThreadOn`.
Test (core 129): a boss r=5 gets a ridge reaching about r+depth with the body staying one solid and the volume between the plain and fat cylinder; a through hole r=4 gets a groove (volume drops but less than a full bore); a rim in the air and a straight edge give a message, not a crash.
Also fixed: my sketch icons sk_ellipse/sk_spline/sk_text/sk_mirror had overwritten the designer's originals (restored from the first commit).
Not checked by eye in the window: picking the rim with the mouse.

## 0.00.39 — shell with several open faces (checked 2026-11-03)

Shell: Shift-click adds further open faces (click again removes), «Только первая» resets; `Shell.more` (old files: none). Test (core 130): box 30x30x20, wall 2: top open gives 5832 mm3, top + side open gives 4896 mm3 (within 1 %), old JSON without `more` loads. Not checked by eye in the window.

## 0.00.40 — chamfer with two distances and with a distance and an angle (checked 2026-11-03)

Chamfer panel: «Равные / Два расстояния / Угол», «Расстояние 2» or the angle, «Поменять грани местами». `Chamfer.second` + `flip` (old files: equal chamfer); `EdgeOp::Chamfer2`, `ChamferAngle`, `ChamferAngleFlipped` work for straight and round edges.
Test (core 131): on a 40 mm cube edge: 4 x 4 removes 320 mm3, 4 x 10 removes 800, flipping keeps the volume, 4 at 45 deg = 4 x 4, 4 at 60 deg gives the leg 4*tan60 (volume matches), 95 deg is refused with a message, old JSON without the new fields loads. Not checked by eye in the window; corners where several asymmetric chamfers meet use the same joins as equal chamfers and were not tested.

## 0.00.41 — measure angles and areas (checked 2026-11-03)

Inspect → «Измерить» now has four kinds: «Расстояние» (as before), «Угол» (three points: start, vertex, end), «Площадь» (click a flat face: sum of its triangles) and «Между гранями» (two flat faces: 90° at a box corner, 180° for faces in one plane).
Test (core 132): box 40x30x20: top face 1200, side 600, whole surface 2*(1200+800+600) exactly; angles 90/45/180 from three points; faces 90/180/135 from their normals. Not checked by eye in the window.

## 0.00.42 — several holes in one operation (checked 2026-11-03)

Hole: Shift-click on the same face adds another centre, «+ Ещё отверстие» adds one by numbers, a list «ЕЩЁ ОТВЕРСТИЯ» with ✕; all share diameter, depth and kind (counterbore, countersink, tapped). `Hole.more` (old files: none), at most 200 extra centres.
Test (core 133): three holes r=3 in a 60x30x10 plate remove 3 x pi*9*10 mm3 within 2 %, counterbored pair builds, 300 centres are refused, old JSON loads. Not checked by eye in the window.

## Status of the user's list (2026-11-03, versions 0.00.36–0.00.42)
Done: sketch mirror and patterns, symmetric and collinear constraints, thread on an existing cylinder, shell with several open faces, chamfer with two distances / distance + angle, measuring angles and areas, several holes at once. Every item passed its automated tests before the next was started; none of them was exercised by eye in the window (the QA window was shared with the user's own clicks).

## 0.00.43 — tooltip pictures for every toolbar tool (checked 2026-11-03)

Added before/after pictures drawn by the renderer for pipe, sweep, loft, place, interference, thread on a cylinder, offset plane, plane at angle, sketch and measure (the planes and the measured edge are shown as thin sheets/bars). All 31 toolbar tools now have a picture; the test decodes the new ones. Regenerate with `cargo test -p fusion-app render_tutorial_pictures -- --ignored` + `python3 tools/make_tips.py`. Not checked by eye on hover.
