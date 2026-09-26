"""The fixed set the text reader is measured on: 24 pictures of known text.

Four typefaces, three sizes, Spanish and English, and three conditions: clean, JPEG at quality
35 (what a phone or a web page recompresses a scan to), and a slight blur (a scan slightly out of
focus). Written once and committed, with the text each picture holds in `truth.json`: fonts differ
from machine to machine, so regenerating it elsewhere would measure a different set.

    python bench/experiments/ocr-20260926/make_set.py
"""

import json
import pathlib

from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = pathlib.Path(__file__).parent / "set"
FONTS = {
    "sans": "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "serif": "/usr/share/fonts/noto/NotoSerif-Regular.ttf",
    "plex": "/usr/share/fonts/TTF/IBMPlexSans-Regular.ttf",
    "mono": "/usr/share/fonts/TTF/IBMPlexMono-Regular.ttf",
}
TEXTS = {
    "es": [
        "El envío llegó al almacén el 3 de marzo.",
        "Importe pendiente: 1.280,50 euros (IVA incluido).",
        "La señora Núñez firmó la recepción a las 17:45.",
    ],
    "en": [
        "The shipment reached the warehouse on 3 March.",
        "Amount due: 1,280.50 euros, tax included.",
        "Signed for by Ms. O'Neill at 5:45 pm; ref #A-2041.",
    ],
}
SIZES = [18, 26, 36]
CONDITIONS = ["clean", "jpeg35", "blur"]


def render(lines, font, size):
    f = ImageFont.truetype(font, size)
    width = max(int(f.getlength(l)) for l in lines) + 80
    height = int(size * 1.9) * len(lines) + 60
    img = Image.new("L", (width, height), 255)
    d = ImageDraw.Draw(img)
    y = 30
    for l in lines:
        d.text((40, y), l, fill=0, font=f)
        y += int(size * 1.9)
    return img


def main():
    HERE.mkdir(exist_ok=True)
    truth = {}
    n = 0
    for fi, (fname, font) in enumerate(FONTS.items()):
        for lang, lines in TEXTS.items():
            for si, size in enumerate(SIZES):
                # Each typeface and language gets every size once, and the conditions rotate, so
                # 24 pictures cover every combination of font, language and size.
                cond = CONDITIONS[(fi + si) % 3]
                img = render(lines, font, size)
                name = f"{n:02d}-{fname}-{lang}-{size}-{cond}"
                if cond == "blur":
                    img = img.filter(ImageFilter.GaussianBlur(0.8))
                if cond == "jpeg35":
                    path = HERE / f"{name}.jpg"
                    img.save(path, quality=35)
                else:
                    path = HERE / f"{name}.png"
                    img.save(path)
                truth[path.name] = "\n".join(lines)
                n += 1
    (HERE / "truth.json").write_text(json.dumps(truth, ensure_ascii=False, indent=1))
    print(f"{n} pictures in {HERE}")


if __name__ == "__main__":
    main()
