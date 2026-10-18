"""Готовит фирменный знак Genix во всех размерах с системным отступом macOS."""
import os, sys
from PIL import Image

def draw(size):
    source = Image.open(os.path.join(os.path.dirname(__file__), "logo", "genix-1024.png")).convert("RGBA")
    # Dock визуально сравнивает непрозрачную площадь, а не размер PNG. 72% оставляет
    # фирменный квадрат на одной оптической высоте с системными macOS-иконками.
    mark = max(1, round(size * 0.72))
    source = source.resize((mark, mark), Image.Resampling.LANCZOS)
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pad = (size - mark) // 2
    img.alpha_composite(source, (pad, pad))
    return img

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
for base in (16, 32, 128, 256, 512):
    draw(base).save(f"{out}/icon_{base}x{base}.png")
    draw(base * 2).save(f"{out}/icon_{base}x{base}@2x.png")
draw(512).save(os.path.join(os.path.dirname(out.rstrip('/')), "icon.png"))
