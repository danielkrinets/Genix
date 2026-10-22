#!/usr/bin/env python3
"""Cuts the ChatGPT sprite sheets in docs/mascot into the small aligned PNGs the app embeds (crates/app/assets/mascot).

Frames are found from the alpha channel (gaps between frames are empty columns/rows), pasted on one canvas per set with the
ground point at the bottom centre, and scaled so the largest frame is `TARGET` px tall. Usage: python3 tools/mascot_atlas.py
"""
import os, sys
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "docs", "mascot")
OUT = os.path.join(ROOT, "crates", "app", "assets", "mascot")
TARGET = 360


def spans(mask_line, min_gap=6):
    """Runs of non-empty cells in a 1-D projection, merging runs closer than `min_gap`."""
    runs, start = [], None
    for i, v in enumerate(mask_line):
        if v and start is None:
            start = i
        if not v and start is not None:
            runs.append([start, i]); start = None
    if start is not None:
        runs.append([start, len(mask_line)])
    merged = []
    for r in runs:
        if merged and r[0] - merged[-1][1] < min_gap:
            merged[-1][1] = r[1]
        else:
            merged.append(r)
    return merged


def projection(alpha, axis, box):
    x0, y0, x1, y1 = box
    crop = alpha.crop(box)
    if axis == "x":
        line = crop.resize((x1 - x0, 1), Image.BOX)
        return [p > 3 for p in line.tobytes()]
    line = crop.resize((1, y1 - y0), Image.BOX)
    return [p > 3 for p in line.tobytes()]


def frames(img, rows, min_gap=14):
    """Frames of a sheet, row by row, left to right, as bounding boxes."""
    alpha = img.getchannel("A").point(lambda v: 255 if v > 24 else 0)
    boxes = []
    ry = spans(projection(alpha, "y", (0, 0, img.width, img.height)), min_gap=min_gap * 3)
    if len(ry) != rows:
        print(f"  note: found {len(ry)} rows, expected {rows}", file=sys.stderr)
    for y0, y1 in ry:
        for x0, x1 in spans(projection(alpha, "x", (0, y0, img.width, y1)), min_gap=min_gap):
            sub = alpha.crop((x0, y0, x1, y1)).getbbox()
            if sub:
                boxes.append((x0 + sub[0], y0 + sub[1], x0 + sub[2], y0 + sub[3]))
    return boxes


def aligned(img, boxes, scale):
    """Frames on a common canvas, ground point (bottom centre of each box) at the canvas bottom centre."""
    w = max(b[2] - b[0] for b in boxes)
    h = max(b[3] - b[1] for b in boxes)
    cw, ch = int(w * scale) + 8, int(h * scale) + 8
    out = []
    for b in boxes:
        f = img.crop(b)
        f = f.resize((max(1, int(f.width * scale)), max(1, int(f.height * scale))), Image.LANCZOS)
        canvas = Image.new("RGBA", (cw, ch), (0, 0, 0, 0))
        canvas.paste(f, ((cw - f.width) // 2, ch - 4 - f.height), f)
        out.append(canvas)
    return out


def main():
    os.makedirs(OUT, exist_ok=True)
    jump = Image.open(os.path.join(SRC, "stage1", "body_jump_5x2.png")).convert("RGBA")
    boxes = frames(jump, rows=2)
    print("jump frames:", len(boxes))
    tallest = max(b[3] - b[1] for b in boxes)
    scale = TARGET / tallest
    for i, f in enumerate(aligned(jump, boxes, scale)):
        f.save(os.path.join(OUT, f"body_{i:02d}.png"), optimize=True)
    # the idle pose is frame 0 (and 9): the resting dome
    pupils = Image.open(os.path.join(SRC, "stage2", "pupils_9.png")).convert("RGBA")
    pb = frames(pupils, rows=3)
    print("pupils:", len(pb))
    ps = sorted(pb, key=lambda b: (b[1] // 200, b[0]))
    centre = ps[4]  # the middle cell of the 3 x 3 grid
    pupil = pupils.crop(centre)
    pupil = pupil.resize((int(pupil.width * 0.5), int(pupil.height * 0.5)), Image.LANCZOS)
    pupil.save(os.path.join(OUT, "pupil.png"), optimize=True)
    whites = Image.open(os.path.join(SRC, "stage2", "eye_whites.png")).convert("RGBA")
    wb = whites.getchannel("A").point(lambda v: 255 if v > 24 else 0).getbbox()
    eyes = whites.crop(wb)
    eyes = eyes.resize((int(eyes.width * 0.5), int(eyes.height * 0.5)), Image.LANCZOS)
    eyes.save(os.path.join(OUT, "eyes.png"), optimize=True)
    mouths = Image.open(os.path.join(SRC, "stage3", "mouths_8.png")).convert("RGBA")
    mb = frames(mouths, rows=2)
    print("mouths:", len(mb))
    for i, b in enumerate(mb):
        m = mouths.crop(b)
        m = m.resize((max(1, int(m.width * 0.5)), max(1, int(m.height * 0.5))), Image.LANCZOS)
        m.save(os.path.join(OUT, f"mouth_{i}.png"), optimize=True)
    print("done:", sorted(os.listdir(OUT)))


if __name__ == "__main__":
    main()
