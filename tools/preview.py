#!/usr/bin/env python3
"""Render a preview of the Zones watch face from the generated bitmap fonts.

There is no Connect IQ simulator in every environment, and the layout in
ZonesView.mc is driven entirely by font metrics and the fractions below.  This
script parses the same .fnt/.png resources the watch loads and draws the same
geometry, so a layout change can be eyeballed without a device.

    python3 tools/preview.py --size 416 --out preview.png
"""

import argparse
import math
import os
import sys

from PIL import Image

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Names and values match the constants in ZonesView.mc; tools/check.py
# verifies that they still do.
ROW_ABOVE = 0.270
TIME_ROW = 0.448
SEC_ROW = 0.598
ROW_BELOW = 0.712
COL_GAP = 0.038
PREFIX_GAP = 0.030
TIP_R = 0.975
BASE_R = 0.910
CHAR_R = 0.830
HALF_W = 0.028

BG = (0, 0, 0)
FG_TIME = (255, 255, 255)
FG_ZONE = (170, 170, 170)
FG_SEC = (128, 128, 128)
DAY = (0, 170, 0)
NIGHT = (255, 0, 0)


class BitmapFont:
    """Just enough BMFont to mirror how Connect IQ draws these resources."""

    def __init__(self, fnt_path):
        base_dir = os.path.dirname(fnt_path)
        self.chars = {}
        with open(fnt_path) as fh:
            for line in fh:
                parts = line.split()
                if not parts:
                    continue
                kv = {}
                for p in parts[1:]:
                    if "=" in p:
                        k, v = p.split("=", 1)
                        kv[k] = v.strip('"')
                if parts[0] == "common":
                    self.line_height = int(kv["lineHeight"])
                    self.base = int(kv["base"])
                elif parts[0] == "page":
                    self.page = Image.open(os.path.join(base_dir, kv["file"])).convert("L")
                elif parts[0] == "char":
                    self.chars[chr(int(kv["id"]))] = {k: int(v) for k, v in kv.items()}

    def width(self, text):
        return sum(self.chars[c]["xadvance"] for c in text)

    def draw(self, canvas, x, y, text, color, center=False):
        """Draw with `y` at the top of the line box, as Connect IQ does."""
        if center:
            x -= self.width(text) // 2
        for ch in text:
            g = self.chars[ch]
            if g["width"] and g["height"]:
                mask = self.page.crop(
                    (g["x"], g["y"], g["x"] + g["width"], g["y"] + g["height"])
                )
                tile = Image.new("RGB", mask.size, color)
                canvas.paste(tile, (x + g["xoffset"], y + g["yoffset"]), mask)
            x += g["xadvance"]


def marker(draw_canvas, cx, cy, r, deg, char, font, color, ring):
    """A pointer arrow at the rim plus the zone's prefix letter inside it."""
    from PIL import ImageDraw

    rad = math.radians(deg)
    s, c = math.sin(rad), math.cos(rad)
    tip = (cx + TIP_R * r * s, cy - TIP_R * r * c)
    base = (cx + BASE_R * r * s, cy - BASE_R * r * c)
    half = HALF_W * r
    ImageDraw.Draw(draw_canvas).polygon(
        [tip, (base[0] + half * c, base[1] + half * s),
         (base[0] - half * c, base[1] - half * s)], fill=color)

    cr = CHAR_R * r - ring * (font.base + 2)
    font.draw(draw_canvas, int(cx + cr * s), int(cy - cr * c - font.base / 2),
              char, color, center=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--size", type=int, default=416)
    ap.add_argument("--out", default="preview.png")
    ap.add_argument("--seconds", action="store_true", default=True)
    ap.add_argument("--aod", action="store_true", help="preview always-on mode")
    args = ap.parse_args()

    fonts = os.path.join(REPO, "resources-%d" % args.size, "fonts")
    if not os.path.isdir(fonts):
        sys.exit("no fonts for size %d -- run tools/make_fonts.py first" % args.size)
    f_time = BitmapFont(os.path.join(fonts, "time.fnt"))
    f_zone = BitmapFont(os.path.join(fonts, "zone.fnt"))

    w = h = args.size
    cx = cy = w / 2.0
    r = w / 2.0
    img = Image.new("RGB", (w, h), BG)

    # local time, then the three configured alternates
    local = ("H", 14, 32, True)
    zones = [("L", 19, 32, True), ("T", 3, 32, False), ("S", 11, 32, True)]

    def fmt(hh, mm):
        return "%02d:%02d" % (hh, mm)

    def sub_display(cxx, cyy, ch, hh, mm, is_day):
        gap = int(PREFIX_GAP * w)
        wc = f_zone.width(ch)
        wt = f_zone.width(fmt(hh, mm))
        x = int(cxx - (wc + gap + wt) / 2)
        y = int(cyy - f_zone.base / 2)
        f_zone.draw(img, x, y, ch, DAY if is_day else NIGHT)
        f_zone.draw(img, x + wc + gap, y, fmt(hh, mm), FG_ZONE)

    # --- bezel: every zone, positioned on a 12 hour dial -------------------
    all_zones = [local] + zones
    placed = []
    for ch, hh, mm, is_day in all_zones:
        deg = ((hh % 12) * 60 + mm) * 0.5
        ring = 0
        while any(abs(((deg - d + 180) % 360) - 180) < 14 and ring == rg for d, rg in placed):
            ring += 1
        placed.append((deg, ring))
        marker(img, cx, cy, r, deg, ch, f_zone, DAY if is_day else NIGHT, ring)

    # --- sub-display above -------------------------------------------------
    if len(zones) >= 1:
        sub_display(cx, ROW_ABOVE * h, *zones[0])

    # --- main time ---------------------------------------------------------
    f_time.draw(img, int(cx), int(TIME_ROW * h - f_time.base / 2),
                fmt(local[1], local[2]), FG_TIME, center=True)

    # --- seconds (active display only) -------------------------------------
    if args.seconds and not args.aod:
        f_zone.draw(img, int(cx), int(SEC_ROW * h - f_zone.base / 2),
                    "07", FG_SEC, center=True)

    # --- sub-displays below ------------------------------------------------
    below = zones[1:]
    if len(below) == 1:
        sub_display(cx, ROW_BELOW * h, *below[0])
    elif len(below) == 2:
        gap = int(PREFIX_GAP * w)
        widest = max(f_zone.width(z[0]) + gap + f_zone.width(fmt(z[1], z[2])) for z in below)
        dx = (widest + COL_GAP * w) / 2
        sub_display(cx - dx, ROW_BELOW * h, *below[0])
        sub_display(cx + dx, ROW_BELOW * h, *below[1])

    img.save(args.out)
    print("wrote %s (%dx%d)" % (args.out, w, h))


if __name__ == "__main__":
    main()
