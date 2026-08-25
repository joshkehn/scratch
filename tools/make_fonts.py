#!/usr/bin/env python3
"""Generate Connect IQ bitmap fonts (BMFont .fnt + page .png) for Zones.

Connect IQ cannot use TTF/OTF directly: fonts have to be baked into BMFont
bitmaps.  This script does that for every supported screen size, sizing each
font so that the longest string it ever has to render still fits inside the
round display (see LAYOUT below, which mirrors ZonesView.mc).

Usage
-----
    # build with the bundled fallback typeface
    python3 tools/make_fonts.py

    # build with a licensed copy of Innovator Grotesk
    python3 tools/make_fonts.py \
        --font ~/fonts/InnovatorGrotesk-Regular.ttf \
        --time-font ~/fonts/InnovatorGrotesk-Bold.ttf

Innovator Grotesk is a commercial typeface from Yep! Type Foundry
(https://yeptype.com/fonts/innovator-grotesk) and is therefore not bundled
here.  Point --font at your own copy; the fonts checked in are built from the
bundled Source Code Pro so that the project compiles as it stands.
"""

import argparse
import math
import os
import sys

try:
    from PIL import Image, ImageDraw, ImageFont
except ImportError:  # pragma: no cover
    sys.exit("Pillow is required: pip install Pillow")

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Screen sizes we build for.  All round, width == height.
BUCKETS = [260, 280, 390, 416, 454]

# Geometry shared with ZonesView.mc.  Keep the two in sync: the sizes chosen
# here are only correct if the view places text at these same fractions.
LAYOUT = {
    "content_r": 0.775,  # x radius -- outer limit for anything but the bezel
    "row_above": 0.270,  # x height -- centre of the sub-display above the time
    "time": 0.448,  # x height -- centre of the main time
    "row_below": 0.712,  # x height -- centre of the sub-displays below
    "col_gap": 0.038,  # x width  -- gap between the two lower sub-displays
    "margin": 0.018,  # x width  -- breathing room against the content circle
}

DIGITS = "0123456789"
LETTERS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"

# Two fonts, because every byte of font resource is memory the watch face
# holds for its whole life.  Narrow character sets keep them small: a watch
# face only ever draws digits, a colon and a zone's prefix letter.
ROLES = {
    # Main time.  Digits and a colon, nothing else.
    "time": {"chars": DIGITS + ":", "probe": "88:88", "cap_frac": 0.21},
    # Everything secondary: the timezone sub-displays ("N 08:45"), the
    # seconds, and the single-letter bezel markers.  Sizing is driven by the
    # two sub-displays that have to sit side by side below the time.
    "zone": {"chars": " " + DIGITS + ":" + LETTERS, "probe": "W 88:88", "cap_frac": 0.075},
}

PAD = 8  # scratch padding around a rendered glyph
PAGE_W = 1024  # wrap the glyph sheet at this width
GAP = 1  # spacing between glyphs on the sheet


def load_face(path, px, variation):
    face = ImageFont.truetype(path, px)
    if variation:
        face.set_variation_by_name(variation)
    return face


def render_glyphs(face, chars):
    """Rasterise each character and record its BMFont metrics."""
    ascent, descent = face.getmetrics()
    baseline = PAD + ascent
    height = ascent + descent + 2 * PAD
    glyphs = {}

    for ch in chars:
        width = int(face.getlength(ch)) + 4 * PAD + face.size
        sheet = Image.new("L", (max(width, 8), max(height, 8)), 0)
        ImageDraw.Draw(sheet).text((PAD, baseline), ch, font=face, fill=255, anchor="ls")
        box = sheet.getbbox()
        advance = face.getlength(ch)

        if box is None:
            # Whitespace. Emit a 2x2 black patch rather than a zero-sized
            # glyph; empty glyph rectangles upset the resource compiler.
            glyphs[ch] = {
                "img": Image.new("L", (2, 2), 0),
                "xoff": 0.0, "top": 1, "below": 1, "adv": advance,
            }
        else:
            glyphs[ch] = {
                "img": sheet.crop(box),
                "xoff": float(box[0] - PAD),
                "top": baseline - box[1],
                "below": box[3] - baseline,
                "adv": advance,
            }
    return glyphs


def apply_spacing(glyphs, tracking, colon_max):
    """Give the digits a common advance, cap the colon, and apply tracking.

    Tabular digits keep the clock from twitching horizontally as the minute
    rolls over, which also means the layout never has to be recomputed.
    """
    present = [d for d in DIGITS if d in glyphs]
    if present:
        tab = max(glyphs[d]["adv"] for d in present)
        for d in present:
            glyphs[d]["xoff"] += (tab - glyphs[d]["adv"]) / 2.0
            glyphs[d]["adv"] = tab

        # A colon is two dots, but a monospaced one is handed a whole digit's
        # width to hold them. Across "88:88" that is the largest single piece
        # of wasted space, and it comes straight off the size of the clock.
        # Cap rather than set, so a proportional face -- which already draws a
        # narrow colon -- is left alone.
        if ":" in glyphs and colon_max:
            limit = tab * colon_max
            colon = glyphs[":"]
            if colon["adv"] > limit:
                colon["xoff"] += (limit - colon["adv"]) / 2.0
                colon["adv"] = limit

    for g in glyphs.values():
        g["adv"] = max(1.0, g["adv"] + tracking)


def cap_height(glyphs):
    return glyphs["0"]["img"].size[1] if "0" in glyphs else 0


def text_width(glyphs, text):
    return sum(int(round(glyphs[c]["adv"])) for c in text)


def px_for_cap(path, variation, chars, target_cap, tracking_frac, colon_max):
    """Find the pixel size whose digit cap height is `target_cap`."""
    lo, hi = 4, max(16, target_cap * 4)
    best = None
    while lo <= hi:
        mid = (lo + hi) // 2
        glyphs = render_glyphs(load_face(path, mid, variation), chars)
        cap = cap_height(glyphs)
        if cap == target_cap:
            best = (mid, glyphs)
            break
        if cap < target_cap:
            best = (mid, glyphs)
            lo = mid + 1
        else:
            hi = mid - 1
    if best is None:
        return None
    px, glyphs = best
    apply_spacing(glyphs, -tracking_frac * target_cap, colon_max)
    return px, glyphs


def build_role(path, variation, role, chars, probe, budget, max_cap, tracking_frac,
               colon_max):
    """Pick the largest cap height whose probe string still fits."""
    for cap in range(max_cap, 5, -1):
        found = px_for_cap(path, variation, chars, cap, tracking_frac, colon_max)
        if found is None:
            continue
        px, glyphs = found
        width = text_width(glyphs, probe)
        if width <= budget(cap):
            return px, cap, glyphs, width
    raise SystemExit("could not fit role %r -- is the typeface extremely wide?" % role)


def budgets(size):
    """Available width for each role's probe string, in pixels.

    Everything is clipped by the content circle; the two lower sub-displays
    additionally have to share one chord between them.
    """
    r = size / 2.0
    rc = LAYOUT["content_r"] * r - LAYOUT["margin"] * size
    gap = LAYOUT["col_gap"] * size

    def chord(dy):
        return 2.0 * math.sqrt(max(rc * rc - dy * dy, 1.0))

    def corner_dy(centre_frac, cap):
        # Worst case is a glyph corner: furthest row of the tallest glyph.
        return abs(centre_frac * size - r) + cap / 2.0

    return {
        "time": lambda cap: chord(corner_dy(LAYOUT["time"], cap)),
        # Two sub-displays plus the gap between them share one chord.
        "zone": lambda cap: (chord(corner_dy(LAYOUT["row_below"], cap)) - gap) / 2.0,
    }


def pack(glyphs, chars):
    """Lay the glyphs out on a single page, wrapping at PAGE_W."""
    placed, x, y, row_h, page_w = {}, 0, 0, 0, 0
    for ch in chars:
        img = glyphs[ch]["img"]
        w, h = img.size
        if x and x + w > PAGE_W:
            y += row_h + GAP
            x, row_h = 0, 0
        placed[ch] = (x, y)
        x += w + GAP
        row_h = max(row_h, h)
        page_w = max(page_w, x - GAP)
    page = Image.new("L", (max(page_w, 1), max(y + row_h, 1)), 0)
    for ch in chars:
        page.paste(glyphs[ch]["img"], placed[ch])
    # Garmin reads the page as a luminance mask -- white glyphs, black ground,
    # no alpha channel. Matches the format shipping watch faces use.
    return Image.merge("RGB", (page, page, page)), placed


def write_fnt(dest, name, px, glyphs, placed, page_size, page_file, chars):
    top = max(g["top"] for g in glyphs.values())
    below = max(0, max(g["below"] for g in glyphs.values()))
    # Our character set has no descenders, so binding the line box to the
    # letters themselves makes dc.getFontHeight() a usable centring metric.
    line_height = top + below

    lines = [
        'info face="%s" size=%d bold=0 italic=0 charset="" unicode=1 '
        "stretchH=100 smooth=1 aa=1 padding=0,0,0,0 spacing=%d,%d" % (name, px, GAP, GAP),
        "common lineHeight=%d base=%d scaleW=%d scaleH=%d pages=1 packed=0"
        % (line_height, top, page_size[0], page_size[1]),
        'page id=0 file="%s"' % page_file,
        "chars count=%d" % len(chars),
    ]
    for ch in chars:
        g = glyphs[ch]
        x, y = placed[ch]
        w, h = g["img"].size
        lines.append(
            "char id=%d x=%d y=%d width=%d height=%d xoffset=%d yoffset=%d "
            "xadvance=%d page=0 chnl=15"
            % (ord(ch), x, y, w, h, int(round(g["xoff"])), top - g["top"],
               int(round(g["adv"])))
        )
    with open(dest, "w") as fh:
        fh.write("\n".join(lines) + "\n")
    return line_height


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    fallback = os.path.join(REPO, "tools", "fallback-font")
    ap.add_argument("--font", default=os.path.join(fallback, "SourceCodePro-Regular.ttf"),
                    help="typeface for the sub-displays, seconds and bezel")
    ap.add_argument("--time-font", help="typeface for the main time (defaults to --font)")
    ap.add_argument("--variation", help="named instance to use for a variable font")
    ap.add_argument("--time-variation", help="named instance for the main time")
    ap.add_argument("--tracking", type=float, default=0.025,
                    help="letter-spacing to remove, as a fraction of cap height")
    ap.add_argument("--colon-max", type=float, default=0.55,
                    help="widest the colon may be, as a fraction of a digit's "
                         "advance; mainly matters for monospaced faces")
    ap.add_argument("--list-variations", action="store_true",
                    help="print the named instances of --font and exit")
    args = ap.parse_args()

    if args.list_variations:
        face = ImageFont.truetype(args.font, 32)
        try:
            names = face.get_variation_names()
        except OSError:
            sys.exit("%s is not a variable font" % args.font)
        for name in names:
            print(name.decode() if isinstance(name, bytes) else name)
        return

    time_font = args.time_font or args.font
    time_variation = args.time_variation or args.variation
    for path in {args.font, time_font}:
        if not os.path.exists(path):
            sys.exit("no such font file: %s" % path)

    print("body font: %s" % args.font)
    print("time font: %s\n" % time_font)

    for size in BUCKETS:
        out_dir = os.path.join(REPO, "resources-%d" % size, "fonts")
        os.makedirs(out_dir, exist_ok=True)
        budget = budgets(size)
        entries = []

        for role, spec in ROLES.items():
            path = time_font if role == "time" else args.font
            variation = time_variation if role == "time" else args.variation
            chars = spec["chars"]

            px, cap, glyphs, width = build_role(
                path, variation, role, chars, spec["probe"], budget[role],
                int(spec["cap_frac"] * size), args.tracking, args.colon_max,
            )
            page, placed = pack(glyphs, chars)
            page_file = "%s.png" % role
            page.save(os.path.join(out_dir, page_file), optimize=True)
            line_height = write_fnt(
                os.path.join(out_dir, "%s.fnt" % role), "Zones-%s" % role, px,
                glyphs, placed, page.size, page_file, chars,
            )
            entries.append((role, chars))
            print("%3d %-5s cap=%2dpx line=%2dpx  '%s'=%dpx (fits %dpx)  sheet=%dx%d"
                  % (size, role, cap, line_height, spec["probe"], width,
                     int(budget[role](cap)), page.size[0], page.size[1]))

        with open(os.path.join(out_dir, "fonts.xml"), "w") as fh:
            fh.write("<fonts>\n")
            for role, chars in entries:
                fh.write('    <font id="%sFont" filename="%s.fnt" antialias="true" '
                         'filter="%s" />\n'
                         % (role.capitalize(), role,
                            chars.replace("&", "&amp;").replace('"', "&quot;")
                                 .replace("<", "&lt;").replace(">", "&gt;")))
            fh.write("</fonts>\n")
        print()


if __name__ == "__main__":
    main()
