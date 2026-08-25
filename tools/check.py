#!/usr/bin/env python3
"""Check the parts of this project the Monkey C compiler cannot.

Font metrics, layout fractions and the device list are spread across Monkey C,
XML, a jungle file and two Python tools, and nothing but agreement between them
makes the face lay out correctly.  This checks that agreement, then runs the
solar checks.

    python3 tools/check.py
"""

import os
import re
import sys
import xml.etree.ElementTree as ET

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "tools"))

import check_solar  # noqa: E402
import make_fonts  # noqa: E402

failures = []


def fail(message):
    failures.append(message)
    print("FAIL %s" % message)


def ok(message):
    print("ok   %s" % message)


def monkeyc_constants(path):
    source = open(path).read()
    return {m.group(1): float(m.group(2))
            for m in re.finditer(r"^\s*const\s+(\w+)\s*=\s*(-?\d+\.\d+)\s*;",
                                 source, re.M)}


def python_constants(path):
    source = open(path).read()
    return {m.group(1): float(m.group(2))
            for m in re.finditer(r"^(\w+)\s*=\s*(-?\d+\.\d+)\s*$", source, re.M)}


def check_xml():
    for root, _, names in os.walk(REPO):
        if ".git" in root:
            continue
        for name in sorted(names):
            if not name.endswith(".xml"):
                continue
            path = os.path.join(root, name)
            try:
                ET.parse(path)
            except ET.ParseError as exc:
                fail("%s is not well formed: %s" % (os.path.relpath(path, REPO), exc))
                return
    ok("every XML resource parses")


def check_layout_constants():
    """ZonesView.mc, preview.py and make_fonts.py must agree on the geometry."""
    view = monkeyc_constants(os.path.join(REPO, "source", "ZonesView.mc"))
    preview = python_constants(os.path.join(REPO, "tools", "preview.py"))
    shared = ["ROW_ABOVE", "TIME_ROW", "SEC_ROW", "ROW_BELOW", "COL_GAP",
              "PREFIX_GAP", "TIP_R", "BASE_R", "CHAR_R", "HALF_W"]

    before = len(failures)
    for name in shared:
        if name not in view:
            fail("ZonesView.mc has no constant %s" % name)
        elif name not in preview:
            fail("preview.py has no constant %s" % name)
        elif abs(view[name] - preview[name]) > 1e-9:
            fail("%s is %s in ZonesView.mc but %s in preview.py"
                 % (name, view[name], preview[name]))
    if len(failures) == before:
        ok("preview.py matches the layout in ZonesView.mc")

    # make_fonts.py sizes the glyphs against the rows they have to sit on.
    before = len(failures)
    for font_key, view_name in (("time", "TIME_ROW"), ("row_below", "ROW_BELOW"),
                                ("row_above", "ROW_ABOVE"), ("col_gap", "COL_GAP")):
        if abs(make_fonts.LAYOUT[font_key] - view.get(view_name, -1)) > 1e-9:
            fail("make_fonts.py LAYOUT[%r] is %s but ZonesView.%s is %s"
                 % (font_key, make_fonts.LAYOUT[font_key], view_name,
                    view.get(view_name)))
    if len(failures) == before:
        ok("make_fonts.py sizes glyphs against the rows ZonesView.mc uses")


def check_fonts():
    """Each .fnt must match its page, and fonts.xml must list every glyph."""
    from PIL import Image

    for size in make_fonts.BUCKETS:
        directory = os.path.join(REPO, "resources-%d" % size, "fonts")
        if not os.path.isdir(directory):
            fail("no fonts generated for %dx%d" % (size, size))
            continue

        declared = {}
        for font in ET.parse(os.path.join(directory, "fonts.xml")).getroot():
            declared[font.get("filename")] = (font.get("id"), font.get("filter"))

        for role in make_fonts.ROLES:
            name = "%s.fnt" % role
            if name not in declared:
                fail("%dx%d fonts.xml does not declare %s" % (size, size, name))
                continue
            _, filter_chars = declared[name]

            page = None
            glyphs = []
            for line in open(os.path.join(directory, name)):
                fields = dict(re.findall(r'(\w+)=("[^"]*"|\S+)', line))
                if line.startswith("page "):
                    page = fields["file"].strip('"')
                elif line.startswith("char "):
                    glyphs.append({k: int(v) for k, v in fields.items()
                                   if k in ("id", "x", "y", "width", "height")})

            image = Image.open(os.path.join(directory, page))
            if image.mode != "RGB":
                fail("%dx%d %s is %s; Connect IQ reads the page as a luminance "
                     "mask, so it must be RGB" % (size, size, page, image.mode))
            for glyph in glyphs:
                if (glyph["x"] + glyph["width"] > image.size[0]
                        or glyph["y"] + glyph["height"] > image.size[1]):
                    fail("%dx%d %s: glyph %d falls outside the page"
                         % (size, size, name, glyph["id"]))
                    break
            missing = [chr(g["id"]) for g in glyphs if chr(g["id"]) not in filter_chars]
            if missing:
                fail("%dx%d %s: filter omits %r" % (size, size, name, missing))

    if not failures:
        ok("bitmap fonts are consistent with their pages and filters")


def check_devices():
    """Every product in the manifest needs font resources from the jungle."""
    manifest = ET.parse(os.path.join(REPO, "manifest.xml")).getroot()
    namespace = {"iq": "http://www.garmin.com/xml/connectiq"}
    products = {p.get("id") for p in manifest.iter("{%s}product" % namespace["iq"])}

    jungle = open(os.path.join(REPO, "monkey.jungle")).read()
    routed = dict(re.findall(r"^(\w+)\.resourcePath\s*=.*?resources-(\d+)$",
                             jungle, re.M))

    unrouted = products - set(routed)
    if unrouted:
        fail("no font resources for: %s" % ", ".join(sorted(unrouted)))
    stray = set(routed) - products
    if stray:
        fail("jungle routes devices missing from the manifest: %s"
             % ", ".join(sorted(stray)))
    for device, size in sorted(routed.items()):
        if int(size) not in make_fonts.BUCKETS:
            fail("%s points at resources-%s, which is not a generated size"
                 % (device, size))
    if not unrouted and not stray:
        ok("all %d products have fonts for their screen size" % len(products))


def main():
    check_xml()
    check_layout_constants()
    check_fonts()
    check_devices()

    print()
    check_solar.main()

    if failures:
        sys.exit("\n%d project check(s) failed" % len(failures))


if __name__ == "__main__":
    main()
