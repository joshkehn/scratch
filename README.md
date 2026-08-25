# Zones

A Garmin Connect IQ watch face for telling the time in several places at once.

![preview](docs/preview.png)

* The local time, large, in the middle.
* Up to three alternate timezones in sub-displays above and below it, each
  labelled with a single character.
* The seconds under the time, shown only while the display is awake — never in
  always-on mode.
* Every zone, the watch's own included, marked around the bezel at its own
  reading on a 12 hour dial: a pointer arrow at the rim and the zone's letter
  just inside it, **green while the sun is up there** and **red while it is
  down**.

Two zones showing near enough the same time land on the same spot on the dial,
which is the truth — the arrows stay put and the letters step inwards so both
stay readable.

## Building and testing

51 round devices are supported, from 260×260 MIP watches up to 454×454 AMOLED.
Each build carries only the bitmap fonts baked for its own screen size.

### Setup, once

1. Install the [Connect IQ SDK
   Manager](https://developer.garmin.com/connect-iq/sdk/), and through it an SDK
   and the device descriptions for whatever you own.
2. Install the **Monkey C** extension for VS Code (Garmin publishes it), which
   drives the compiler, simulator and debugger.
3. Make a developer key. `Monkey C: Generate a Developer Key` from the command
   palette, or:

   ```sh
   openssl genrsa -out developer_key.pem 4096
   openssl pkcs8 -topk8 -inform PEM -outform DER \
       -in developer_key.pem -out developer_key.der -nocrypt
   ```

Put `bin/` and `developer_key.*` out of the repo's way — `.gitignore` already
covers `bin/`.

### Simulator

```sh
monkeyc -f monkey.jungle -o bin/zones.prg -y developer_key.der -d venu2 -w
connectiq                        # start the simulator
monkeydo bin/zones.prg venu2     # load the face into it
```

`-w` turns on warnings; leave it on. In VS Code, F5 does the same thing and asks
which device.

**Settings** live in the simulator under *File ▸ Edit Persistent Storage ▸ Edit
Application.Properties data*. Something like this exercises the whole face —
four zones spread around the dial, two of them in the dark:

| key | value |
| --- | --- |
| `homePrefix` / `homePosition` | `N` / `40.71,-74.01` |
| `zone1Prefix` / `Offset` / `Position` | `L` / `1` / `51.51,-0.13` |
| `zone2Prefix` / `Offset` / `Position` | `T` / `9` / `35.68,139.65` |
| `zone3Prefix` / `Offset` / `Position` | `S` / `-8` / `37.77,-122.42` |

Worth looking at specifically:

* **One device per screen size**, because each loads different bitmap fonts:
  `fenix7` (260), `fenix7x` (280), `vivoactive5` (390), `venu2` (416),
  `venu3` (454).
* **Low power mode.** The seconds under the time must disappear, and on an
  AMOLED device the palette should dim and the whole face shift a couple of
  pixels each minute.
* **Colliding markers.** Give two zones offsets 12 hours apart — they read the
  same time on a 12 hour dial, so their arrows land on the same spot and the
  letters should step inwards rather than overlap.
* **Peak memory**, in the simulator's memory view. Watch faces have a tight
  budget and this one carries bitmap fonts.

### On the watch

Build for the exact device, then sideload over USB:

```sh
monkeyc -f monkey.jungle -o bin/zones.prg -y developer_key.der -d fenix7 -r
```

`Monkey C: Build for Device` does the same from VS Code. Copy `zones.prg` into
`GARMIN/APPS/` on the watch's mass storage, **eject properly** so the write
flushes, then unplug. The face appears in the watch's own watch-face list.

One wrinkle: **settings for a sideloaded face often cannot be edited from the
phone.** Garmin Connect only reliably exposes settings for apps installed
through the store. Two ways round it while developing:

* Edit the defaults in `resources/settings/properties.xml` and rebuild. They
  apply on a fresh install, so the face comes up already configured — much the
  faster loop.
* Or upload it to the Connect IQ store as a private/beta app (`monkeyc -e -o
  bin/zones.iq …` builds the store bundle) and install from there, which gets
  you real settings.

## Settings

Configured from Garmin Connect on the phone.

| Setting | Meaning |
| --- | --- |
| Watch timezone letter | Labels the watch's own marker on the bezel. |
| Zone *n* letter | One character. **Empty hides the zone.** |
| Zone *n* UTC offset | Hours east of UTC: `-5` for New York, `5.5` for Delhi. |
| Zone *n* position | `latitude,longitude`, e.g. `51.5,-0.13`. |

The position is only used to work out whether the sun is up, and can be left
empty — the zone then treats 06:00–18:00 as daylight. Everything else works
without it.

**Offsets do not follow daylight saving.** Connect IQ has no timezone database,
so an alternate zone is a fixed offset and needs changing twice a year: Berlin
is `1` in winter and `2` in summer. The watch's own time is unaffected — it
comes from the device, which does handle DST.

## Battery

A watch face is redrawn once a second while you are looking at it and once a
minute the rest of the time, so the cost is in what each redraw does.

* **No `onPartialUpdate`.** Seconds are only wanted while the display is awake,
  so there is nothing to draw between minutes and the always-on path is left
  alone entirely.
* **Everything that can only change once a minute is cached** — the time
  strings, every text position, the bezel geometry. A frame in the once-a-second
  path is nothing but `setColor` and `drawText`.
* **Sunrise and sunset are computed once per zone per day**, not per frame, and
  from a closed-form solar equation rather than a network or GPS lookup.
* **No permissions and no sensors.** The face reads the clock and its own
  settings, nothing else.
* **Nothing allocates while drawing.** Per-zone scratch space is allocated when
  the zone list changes.
* **Two fonts, narrow character sets.** Digits, a colon and A–Z, sized so only
  one set per screen size ships.
* On always-on displays the palette dims and the whole face shifts a couple of
  pixels each minute, to spare the panel.

## The typeface

The face ships with [Source Code
Pro](https://github.com/adobe-fonts/source-code-pro) (SIL Open Font License, see
`tools/fallback-font/`) so that it builds and runs as it stands.

It is designed for [Innovator
Grotesk](https://yeptype.com/fonts/innovator-grotesk) (Yep! Type Foundry), which
is commercial and so is not in this repo. Swapping it in is one command —
see below.

### Generating the glyphs

Connect IQ cannot load TTF or OTF at runtime. Fonts have to be baked into
[BMFont](https://www.angelcode.com/products/bmfont/) bitmaps: a `.fnt` metrics
file plus a `.png` glyph sheet, declared in a `fonts.xml`. `tools/make_fonts.py`
does the whole job — it rasterises the glyphs, packs the sheet, writes the
metrics and emits the resource XML, for all five screen sizes.

**1. Find the weights you want.** Innovator Grotesk is a variable font, so name
the instance rather than picking a separate file:

```sh
python3 tools/make_fonts.py --font ~/fonts/InnovatorGrotesk.ttf --list-variations
```

**2. Generate.** Two roles: `time` is the big clock, `zone` is everything
secondary. A heavier weight for the time reads better at a glance; keep the
secondary one lighter so the small text at 19px does not fill in.

```sh
python3 tools/make_fonts.py \
    --font          ~/fonts/InnovatorGrotesk.ttf --variation      Regular \
    --time-font     ~/fonts/InnovatorGrotesk.ttf --time-variation Bold
```

With static instances instead, just point at the two files and drop
`--variation`. `--time-font` defaults to `--font` if you only want one weight.

**3. Read the fit report.** Glyph sizes are not fixed — the script picks the
largest that still fits inside the round display, so a narrower typeface comes
out *bigger* rather than overflowing. It prints what it chose:

```
416 time  cap=70px line=70px  '88:88'=277px (fits 285px)  sheet=539x70
416 zone  cap=19px line=23px  'W 88:88'=105px (fits 110px)  sheet=549x23
```

`cap` is the digit height in pixels, and `fits` is the width available at that
row on a round screen. If `cap` comes out smaller than you like, the typeface is
wide for its height; `--tracking 0.04` takes more air out between glyphs, and
`--colon-max` caps how much width the colon may take (0.55 of a digit by
default, which mostly matters for monospaced faces like the bundled one).

**4. Check it.**

```sh
python3 tools/check.py
python3 tools/preview.py --size 416 --out preview.png
```

The preview parses the same `.fnt` files the watch loads, so it shows the real
glyphs at the real sizes.

Output lands in `resources-260/fonts/` … `resources-454/fonts/` and is meant to
be committed — the build needs it. That means the generated bitmaps carry your
licensed glyphs, so check what your licence says about embedding before pushing
them anywhere public.

Only the characters the face can draw are baked in: digits, a colon, a space and
A–Z for the zone letters. That keeps the resources to about 24 KB per device.

## Checks

There is no Connect IQ simulator in every environment, and the compiler cannot
see any of this, so:

```sh
python3 tools/check.py       # resources, fonts, device list, layout, solar
python3 tools/preview.py --size 416 --out preview.png
```

`tools/preview.py` parses the same `.fnt` resources the watch loads and draws
the same geometry, so a layout change can be eyeballed without a device.
`tools/check.py` verifies that the layout constants in `ZonesView.mc`,
`preview.py` and `make_fonts.py` still agree, that every glyph in a `.fnt` is
inside its page and listed in its filter, that every product in the manifest has
fonts for its screen size, and that `Solar.mc` reproduces published sunrise and
sunset times.

## Layout

`source/ZonesView.mc` places everything as a fraction of the screen, so the same
numbers work on every size:

```
                 pointer arrows + letters around the rim
                          L 19:32          0.270
                          14:32            0.448
                            07             0.598   (awake only)
                    T 03:32   S 11:32      0.712
```

`tools/make_fonts.py` sizes the glyphs against those same fractions, which is
why the two have to stay in step and why `tools/check.py` checks that they do.
