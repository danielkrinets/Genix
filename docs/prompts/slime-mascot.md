# Промт для ChatGPT: слайм-маскот по этапам

Как пользоваться: сначала отправь **общий бриф** (он ничего не рисует, только запоминает правила). Потом пиши «Этап 1», «Этап 2» и так далее: ChatGPT делает только этот этап. Если что-то не так, пиши «Этап 2, переделай: …», остальные этапы не трогаются. Картинки ChatGPT отдаёт по одной, поэтому после каждого листа пиши «следующий лист этого этапа».

## Общий бриф (отправить первым)

```
You are my sprite artist. We are building a friendly slime mascot for a desktop CAD app, as separate transparent layers that I will assemble in code. I will say "Stage 1", "Stage 2" and so on. Do ONLY the stage I name, nothing else, and wait.

Fixed rules for every stage and every image:
- Style: clean flat vector look, soft gradients allowed, one thin darker outline of the same hue, no textures, no text, no watermark, no background.
- Background: fully transparent PNG. Never a checkerboard, never white, never a gradient.
- Palette: body base #3A7BF2 (blue), body shade #2455B8, body highlight #9CC2FF, outline #1A3F8F, eye white #FFFFFF, pupil and ink #161A24. Use only these plus pure white for small glints.
- Canvas: every single frame is 1024 x 1024 px. The slime's ground contact point is always the exact bottom-centre of the canvas, at x = 512, y = 960. Never shift it between frames, so frames can be swapped in place.
- Size: the idle slime body is about 640 px wide and 400 px tall inside the canvas. Leave at least 40 px of empty margin on every side.
- Camera: straight front view, no perspective, no 3/4 view, light from the top left.
- NO shadow on the ground, NO face, NO eyes unless the stage says so. Layers are drawn separately and must not contain parts of other layers.
- When a stage asks for several frames, give them as ONE sprite sheet on a transparent background with an even grid, left to right then top to bottom, with 24 px gaps, AND tell me the grid size (columns x rows) and the frame order. Also say how many frames there are.
- Keep the shape, proportions, outline thickness and colours identical across all frames and all stages. Only the pose changes.

Reply only: "Ready. Say Stage 1."
```

## Этап 1: тело (без лица)

```
Stage 1: BODY ONLY. No eyes, no mouth, no cheeks, no shadow.

Make these body poses, each as separate frames (sprite sheets, grid as per the rules):
A) idle breathing loop, 8 frames: from relaxed to slightly taller and narrower (inhale) and back to slightly wider and flatter (exhale). Smooth, subtle, 3 to 4 percent change.
B) squash and stretch jump, 10 frames: rest, crouch (wide and flat), push off (tall and thin), mid-air stretch, peak (round), falling stretch, landing squash (very wide and flat), rebound (slightly tall), settle, rest.
C) shy turn, 6 frames: the body leans to the left and the whole dome narrows by about 25 percent, as if turning its head away; frame 1 is neutral, frame 6 is fully turned.
D) happy bounce, 6 frames: small hop with a slight overshoot.
E) sleeping, 4 frames: low flat blob, slow breathing.
F) melt and re-form, 8 frames: the body flattens into a puddle and rises back.

Keep the same ground point on every frame. Give each pose as its own sprite sheet. Start with A only; I will say "next" for B, C and so on.
```

## Этап 2: глаза (отдельный слой)

```
Stage 2: EYES ONLY, on a transparent background, separate from the body. Draw the two eyes as they should sit on the idle body: eye centres at x = 432 and x = 592, y = 700 on the 1024 canvas, each eye about 120 px wide. Do not draw the body, mouth or anything else.

Make these as separate sprite sheets:
A) blink loop, 8 frames: open, half-lid, closed, half-lid, open (smooth, symmetrical).
B) pupil directions, 9 frames: centre, left, right, up, down, up-left, up-right, down-left, down-right. The pupil must never leave the white; the white and outline stay fixed, only the pupil moves.
C) emotions, 8 frames: neutral, happy (eyes curved up), surprised (wide, small pupils), sleepy (half-closed), sad (inner edge down), focused (slightly narrowed), love (pupils as small hearts in #FF5C8A), dizzy (spiral pupils).
D) shy look: both eyes looking left, slightly narrowed horizontally, 3 frames.

Also give the pupils as their own separate layer (pupils only, same positions, 9 directions as in B) and the eye whites as their own layer (whites and outline only), so code can move the pupils freely.
```

## Этап 3: рот и щёки

```
Stage 3: MOUTH AND CHEEKS ONLY, transparent background, separate layers. Mouth centred at x = 512, y = 790; cheeks at x = 340 and x = 684, y = 770.

A) mouth, 8 frames: neutral small line, smile, big open smile, small "o", sad, talking (3 frames, open sizes small, medium, wide).
B) cheeks, 3 frames: none, soft blush #FF8FAE at 35 percent, strong blush at 60 percent.
Nothing else in the image.
```

## Этап 4: блики, брызги, тень

```
Stage 4: EFFECT LAYERS, all separate, transparent background.
A) highlights: the glossy highlight on the top left of the idle body (one sheet, 3 sizes), and a small secondary glint.
B) drops and splash particles: 12 different small slime droplets and splash shapes in the body palette, each isolated, about 40 to 120 px.
C) puddle: a flat slime puddle seen from the front, 4 frames growing, for the melt animation.
D) ground shadow: a soft elliptical shadow in black at 25 percent opacity, 3 sizes (small for mid-air, normal, wide for squash), centred at x = 512, y = 960.
```

## Этап 5: проверка и сборка

```
Stage 5: QA SHEET. Build one preview image that stacks the idle frame 1 body, the neutral eyes, the neutral mouth and the shadow exactly on top of each other on a light grey background, so I can check alignment. Then list, as a table, every file or sheet you made with: stage, name, grid (columns x rows), frame count and the suggested file name in the form slime_<layer>_<pose>_<columns>x<rows>.png. Point out any frame whose ground point, size or outline thickness differs from the idle frame.
```

## Если ChatGPT путается

- Напомни: «Keep everything from the brief: transparent background, 1024 x 1024 frames, ground point at x = 512, y = 960.»
- Если фон не прозрачный: «Remove the background completely and export as transparent PNG.»
- Если лица стали разными: «Use eye frame 1 of the neutral emotion as the reference and keep it identical.»
- Присылай мне готовые листы с их сеткой (например, 4x2): я подключу их в приложение.
