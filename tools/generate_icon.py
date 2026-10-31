"""Generate Slate's app icon: assets/slate.ico and assets/slate-256.png.

Usage (from the repo root, needs Pillow):

    python tools/generate_icon.py [--preview DIR]

The motif is a dark rounded slate tablet holding a Markdown page: a cyan
heading bar, two light body lines and a warm text caret. Each small size has
its own integer pixel layout so the bars land on whole pixels and stay crisp
at 16/24/32 px; only the tablet corners are anti-aliased (8x supersampling
with a box filter keeps pixel-aligned edges exact).
"""

import argparse
import os
import sys

from PIL import Image, ImageDraw

SIZES = [16, 24, 32, 48, 64, 128, 256]
SS = 8  # supersampling factor

# Nord-like palette shared with the UI.
RIM = (22, 28, 37)          # title bar fill
TOP = (52, 62, 80)          # tablet gradient, top
BOTTOM = (36, 44, 58)       # editor surface
HEADING = (136, 192, 208)   # accent
TEXT = (216, 222, 233)      # text
TEXT_DIM = (160, 170, 188)  # second body line, slightly quieter
CARET = (235, 203, 139)     # warm accent

# Hand-tuned layouts for the small sizes, in output pixels:
#   tablet = (x0, y0, x1, y1, radius, rim)   (x1/y1 exclusive)
#   bars   = [(x0, y0, x1, y1, color)]
#   caret  = (x0, y0, x1, y1)
SMALL = {
    16: dict(
        tablet=(0, 0, 16, 16, 3, 1),
        bars=[(3, 3, 13, 5, HEADING), (3, 7, 12, 8, TEXT), (3, 10, 9, 11, TEXT_DIM)],
        caret=(10, 9, 12, 13),
    ),
    24: dict(
        tablet=(1, 1, 23, 23, 4, 1),
        bars=[(5, 5, 18, 8, HEADING), (5, 10, 17, 12, TEXT), (5, 14, 12, 16, TEXT_DIM)],
        caret=(14, 13, 16, 19),
    ),
    32: dict(
        tablet=(1, 1, 31, 31, 6, 1),
        bars=[(7, 7, 24, 11, HEADING), (7, 14, 22, 16, TEXT), (7, 19, 16, 21, TEXT_DIM)],
        caret=(18, 17, 20, 25),
    ),
}

# Master layout on a 256 grid; larger sizes are scaled from it.
MASTER = dict(
    tablet=(12, 12, 244, 244, 52, 6),
    bars=[(56, 58, 200, 90, HEADING), (56, 116, 184, 134, TEXT), (56, 156, 140, 174, TEXT_DIM)],
    caret=(158, 142, 172, 190),
)


def layout_for(size):
    """Returns (layout, round_caps) for an output size."""
    if size in SMALL:
        return SMALL[size], False
    k = size / 256

    def r(v):
        return int(round(v * k))

    x0, y0, x1, y1, rad, rim = MASTER["tablet"]
    return dict(
        tablet=(r(x0), r(y0), r(x1), r(y1), max(2, r(rad)), max(1, r(rim))),
        bars=[(r(a), r(b), r(c), r(d), col) for a, b, c, d, col in MASTER["bars"]],
        caret=tuple(r(v) for v in MASTER["caret"]),
    ), size >= 64  # round caps only where they span enough pixels


def render(size):
    layout, round_caps = layout_for(size)
    big = size * SS

    def s(v):
        return v * SS

    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))

    # Rim (outer shape) then a vertical gradient face inset by the rim width.
    x0, y0, x1, y1, rad, rim = layout["tablet"]
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (s(x0), s(y0), s(x1) - 1, s(y1) - 1), radius=s(rad), fill=255
    )
    img.paste(Image.new("RGBA", (big, big), RIM + (255,)), (0, 0), mask)

    face = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    fd = ImageDraw.Draw(face)
    fy0, fy1 = s(y0 + rim), s(y1 - rim)
    for y in range(fy0, fy1):
        t = (y - fy0) / max(1, fy1 - fy0 - 1)
        col = tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM))
        fd.line([(0, y), (big, y)], fill=col + (255,))
    fmask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(fmask).rounded_rectangle(
        (s(x0 + rim), s(y0 + rim), s(x1 - rim) - 1, s(y1 - rim) - 1),
        radius=max(0, s(rad - rim)),
        fill=255,
    )
    img.paste(face, (0, 0), fmask)

    d = ImageDraw.Draw(img)

    def bar(box, col, cap_radius):
        box = (s(box[0]), s(box[1]), s(box[2]) - 1, s(box[3]) - 1)
        if round_caps:
            d.rounded_rectangle(box, radius=cap_radius, fill=col + (255,))
        else:
            d.rectangle(box, fill=col + (255,))

    for bx0, by0, bx1, by1, col in layout["bars"]:
        bar((bx0, by0, bx1, by1), col, s(by1 - by0) // 2)
    cx0, cy0, cx1, cy1 = layout["caret"]
    bar((cx0, cy0, cx1, cy1), CARET, s(cx1 - cx0) // 2)

    return img.resize((size, size), Image.Resampling.BOX)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--preview", help="also write per-size PNGs (and 8x zooms) here")
    args = parser.parse_args()

    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    assets = os.path.join(root, "assets")
    os.makedirs(assets, exist_ok=True)

    images = {size: render(size) for size in SIZES}
    images[256].save(os.path.join(assets, "slate-256.png"), optimize=True)
    # Pass the hand-tuned small renders explicitly; otherwise Pillow would
    # downscale every size from the 256 px image.
    images[256].save(
        os.path.join(assets, "slate.ico"),
        format="ICO",
        sizes=[(size, size) for size in SIZES],
        append_images=[images[size] for size in SIZES if size != 256],
    )

    if args.preview:
        os.makedirs(args.preview, exist_ok=True)
        for size, img in images.items():
            img.save(os.path.join(args.preview, f"slate-{size}.png"))
            if size <= 48:
                zoom = img.resize((size * 8, size * 8), Image.Resampling.NEAREST)
                zoom.save(os.path.join(args.preview, f"slate-{size}-x8.png"))
    print(f"wrote {os.path.join(assets, 'slate.ico')} and slate-256.png", file=sys.stderr)


if __name__ == "__main__":
    main()
