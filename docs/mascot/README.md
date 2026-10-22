# Slime mascot art from ChatGPT

Sheets made with `docs/prompts/slime-mascot.md`. Stage 1 (body, no face) is done:

| File | Pose | Grid asked for |
|---|---|---|
| `stage1/body_idle_4x2.png` | breathing | 4 x 2, 8 frames |
| `stage1/body_jump_5x2.png` | squash and stretch jump | 5 x 2, 10 frames |
| `stage1/body_shy_6x1.png` | shy turn | 6 x 1, 6 frames |
| `stage1/body_happy_3x2.png` | happy bounce | 3 x 2, 6 frames |
| `stage1/body_sleep_4x1.png` | sleeping | 4 x 1, 4 frames |
| `stage1/body_melt_4x2.png` | melt and re-form | 4 x 2, 8 frames |

Not as specified: the sheets are not 1024 x 1024 per frame with a 24 px gap and a fixed ground point; each sheet has its own size. Before use, cut the frames by their blobs and re-align them on the ground point.

## In the app

`tools/mascot_atlas.py` cuts the sheets into `crates/app/assets/mascot/` (10 body frames from the jump sheet, eye whites, one pupil, mouths). `crates/app/src/mascot.rs` draws them on the registration screen: breathing, blinking, pupils that follow the cursor, a hop on click (the ten jump frames), a turn away with a flat mouth while the password is typed. Screenshots: `in-genix-happy.jpg`, `in-genix-shy.jpg`.

Not used yet: the other body sheets (idle, shy turn, happy, sleep, melt), the 2A blink sheet, the emotions and effects (stages 2C to 5 were not made).
