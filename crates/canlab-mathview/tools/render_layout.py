"""Render the math layout dump to a PNG, for eyeballing without a GPU window.

    python render_layout.py layout.json layout.png

The positions come from the layout engine; only the glyphs are drawn by Pillow,
so this shows structure, spacing, ghost delimiters, empty slots and every caret
position rather than exact typography.
"""

import json
import sys

from PIL import Image, ImageDraw, ImageFont

BG = (24, 24, 28)
INK = (232, 232, 234)
GHOST = (120, 120, 130)
CARET = (255, 159, 67)
LABEL = (150, 150, 160)
STROKE = (110, 110, 120)

FONT_CANDIDATES = [
    r"C:\Windows\Fonts\segoeui.ttf",
    r"C:\Windows\Fonts\DejaVuSans.ttf",
    r"C:\Windows\Fonts\arial.ttf",
]


def font(size, italic=False):
    for path in FONT_CANDIDATES:
        try:
            return ImageFont.truetype(path, int(round(size)))
        except OSError:
            continue
    return ImageFont.load_default()


def main(src, dst):
    scenes = json.load(open(src, encoding="utf-8"))
    width = 980
    height = int(max(s["top"] + s["height"] for s in scenes) + 40)
    image = Image.new("RGB", (width, height), BG)
    draw = ImageDraw.Draw(image)

    for scene in scenes:
        top = scene["top"]
        draw.text((30, top - 26), scene["label"], font=font(13), fill=LABEL)

        for item in scene["items"]:
            kind = item["k"]
            if kind == "t":
                colour = GHOST if item.get("g") else INK
                draw.text(
                    (30 + item["x"], top + item["y"]),
                    item["v"],
                    font=font(item["s"]),
                    fill=colour,
                    anchor="ls",
                )
            elif kind == "r":
                x0 = 30 + item["x"]
                y0 = top + item["y"]
                draw.rectangle(
                    [x0, y0, x0 + max(item["w"], 1), y0 + max(item["h"], 1.0)],
                    fill=INK,
                )
            elif kind == "p":
                x0 = 30 + item["x"]
                y0 = top + item["y"]
                draw.rectangle(
                    [x0, y0, x0 + item["w"], y0 + item["h"]], outline=STROKE
                )

        for x, y, h in scene["carets"]:
            x0 = 30 + x
            draw.line([x0, top + y, x0, top + y + h], fill=CARET, width=1)

    image.save(dst)
    print(f"wrote {dst} ({width}x{height})")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
