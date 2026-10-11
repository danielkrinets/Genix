"""Рисует иконку приложения (изометрический «каркас» детали) во всех размерах для macOS."""
import os, sys
from PIL import Image, ImageDraw

def draw(size):
    S = 1024
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # скруглённый квадрат-основа в стиле macOS
    pad = 90
    d.rounded_rectangle([pad, pad, S - pad, S - pad], radius=190, fill=(24, 32, 46, 255))
    # изометрический куб
    cx, cy, a = S / 2, S / 2 + 40, 250
    h = a * 0.58
    top = [(cx, cy - 2 * h), (cx + a, cy - h), (cx, cy), (cx - a, cy - h)]
    left = [(cx - a, cy - h), (cx, cy), (cx, cy + 2 * h - 40), (cx - a, cy + h - 40)]
    right = [(cx + a, cy - h), (cx, cy), (cx, cy + 2 * h - 40), (cx + a, cy + h - 40)]
    d.polygon(top, fill=(235, 238, 243))
    d.polygon(left, fill=(160, 168, 180))
    d.polygon(right, fill=(110, 118, 132))
    # отверстие сверху
    ex, ey, rw, rh = cx, cy - h, 92, 53
    d.ellipse([ex - rw, ey - rh, ex + rw, ey + rh], fill=(70, 78, 92))
    # фирменный синий акцент — рёбра
    for poly in (top, left, right):
        d.line(poly + [poly[0]], fill=(6, 150, 215), width=14, joint="curve")
    return img.resize((size, size), Image.LANCZOS)

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
for base in (16, 32, 128, 256, 512):
    draw(base).save(f"{out}/icon_{base}x{base}.png")
    draw(base * 2).save(f"{out}/icon_{base}x{base}@2x.png")
draw(512).save(os.path.join(os.path.dirname(out.rstrip('/')), "icon.png"))
