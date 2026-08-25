#!/usr/bin/env python3
"""Check source/Solar.mc against known sunrise and sunset times.

Solar.mc cannot be exercised without a device or the Connect IQ simulator, and
a silent error there shows up as nothing worse than a letter in the wrong
colour -- exactly the sort of bug that survives a casual look.  This is a
line-for-line port of the Monkey C, run against published times.

    python3 tools/check_solar.py
"""

import math
import re
import os
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

J2000_EPOCH = 946728000
SIN_OBLIQUITY = 0.39779
SIN_HORIZON = -0.01454

NORMAL, POLAR_DAY, POLAR_NIGHT = 0, 1, 2


def norm360(degrees):
    return degrees - math.floor(degrees / 360.0) * 360.0


def day_number(epoch):
    return round((epoch - J2000_EPOCH) / 86400.0 + 0.0008)


def solar(day, lat, lon):
    """Port of Solar.update(). Returns (state, sunrise, sunset) in epoch secs."""
    j_star = day - lon / 360.0

    mean_degrees = norm360(357.5291 + 0.98560028 * j_star)
    mean = math.radians(mean_degrees)
    centre = (1.9148 * math.sin(mean)
              + 0.0200 * math.sin(2 * mean)
              + 0.0003 * math.sin(3 * mean))
    lam = math.radians(norm360(mean_degrees + centre + 282.9372))

    transit = (-lon / 360.0
               + 0.0053 * math.sin(mean)
               - 0.0069 * math.sin(2 * lam))

    sin_decl = math.sin(lam) * SIN_OBLIQUITY
    cos_decl = math.sqrt(1.0 - sin_decl * sin_decl)
    lat_rad = math.radians(lat)
    hour_angle = ((SIN_HORIZON - math.sin(lat_rad) * sin_decl)
                  / (math.cos(lat_rad) * cos_decl))

    if hour_angle >= 1.0:
        return POLAR_NIGHT, 0, 0
    if hour_angle <= -1.0:
        return POLAR_DAY, 0, 0

    half = math.degrees(math.acos(hour_angle)) / 360.0
    noon = J2000_EPOCH + day * 86400
    return (NORMAL,
            noon + int((transit - half) * 86400.0),
            noon + int((transit + half) * 86400.0))


def utc_hhmm(epoch):
    epoch = int(epoch) % 86400
    return "%02d:%02d" % (epoch // 3600, (epoch % 3600) // 60)


def epoch_for(y, mo, d):
    """Epoch seconds at 12:00 UTC on the given date (no time zone maths)."""
    days = (y - 1970) * 365 + (y - 1969) // 4
    cumulative = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334]
    days += cumulative[mo - 1] + (d - 1)
    if mo > 2 and y % 4 == 0:
        days += 1
    return days * 86400 + 43200


# Published sunrise/sunset in UTC, for places far apart in both latitude and
# longitude. Solstice dates, where the sun's declination is at its extreme.
CASES = [
    # name,           lat,      lon,      date,           sunrise, sunset
    ("London Jun",    51.5074,  -0.1278,  (2026, 6, 21),  "03:43", "20:21"),
    ("London Dec",    51.5074,  -0.1278,  (2026, 12, 21), "08:03", "15:53"),
    ("New York Jun",  40.7128, -74.0060,  (2026, 6, 21),  "09:25", "00:31"),
    ("Tokyo Dec",     35.6762, 139.6503,  (2026, 12, 21), "21:47", "07:31"),
    ("Sydney Jun",   -33.8688, 151.2093,  (2026, 6, 21),  "20:59", "06:53"),
]

# The cases above all sit near a solstice, where the equation of time happens
# to be small. It is the term that moves sunrise and sunset together, so check
# it directly against its published curve: solar noon on the prime meridian,
# less the 12:00 mean noon, is the equation of time.
EQUATION_OF_TIME = [
    ((2026, 2, 11), -14.2),   # yearly minimum
    ((2026, 3, 21), -7.4),
    ((2026, 5, 14), 3.7),     # spring maximum
    ((2026, 7, 26), -6.5),    # summer minimum
    ((2026, 9, 21), 6.9),
    ((2026, 11, 3), 16.4),    # yearly maximum
]

# Day length at the equator is nearly constant, and a little over twelve hours
# rather than exactly twelve, because sunrise is reckoned from the top of the
# sun's disc through refraction rather than from its centre.
EQUATOR_DAY_MINUTES = 727.0

POLAR = [
    ("Tromso Jun (midnight sun)", 69.6496, 18.9560, (2026, 6, 21), POLAR_DAY),
    ("Tromso Dec (polar night)",  69.6496, 18.9560, (2026, 12, 21), POLAR_NIGHT),
]


def minutes_apart(got_epoch, expected_hhmm):
    got = int(got_epoch) % 86400
    hh, mm = expected_hhmm.split(":")
    want = int(hh) * 3600 + int(mm) * 60
    diff = abs(got - want)
    return min(diff, 86400 - diff) / 60.0


def check_source_matches():
    """Guard against this port drifting away from the Monkey C it mirrors."""
    src = open(os.path.join(REPO, "source", "Solar.mc")).read()
    for literal in ("357.5291", "0.98560028", "1.9148", "282.9372",
                    "0.0053", "0.0069", "0.39779", "-0.01454", "946728000"):
        if literal not in src:
            sys.exit("Solar.mc no longer contains %s -- update this port" % literal)


def main():
    check_source_matches()
    tolerance = 2.0
    worst = 0.0
    failures = 0

    for name, lat, lon, date, want_rise, want_set in CASES:
        day = day_number(epoch_for(*date))
        state, rise, dusk = solar(day, lat, lon)
        if state != NORMAL:
            print("FAIL %-14s expected a normal day, got state %d" % (name, state))
            failures += 1
            continue
        drift = max(minutes_apart(rise, want_rise), minutes_apart(dusk, want_set))
        worst = max(worst, drift)
        ok = drift <= tolerance
        failures += 0 if ok else 1
        print("%s %-14s rise %s (want %s)  set %s (want %s)  off by %.1f min"
              % ("ok  " if ok else "FAIL", name, utc_hhmm(rise), want_rise,
                 utc_hhmm(dusk), want_set, drift))

    print()
    for date, want in EQUATION_OF_TIME:
        state, rise, dusk = solar(day_number(epoch_for(*date)), 0.0, 0.0)
        noon = ((rise + dusk) / 2.0) % 86400
        got = (43200 - noon) / 60.0
        drift = abs(got - want)
        worst = max(worst, drift)
        ok = drift <= 1.0
        failures += 0 if ok else 1
        print("%s eq of time %04d-%02d-%02d  %+5.1f min (want %+5.1f)"
              % ("ok  " if ok else "FAIL", date[0], date[1], date[2], got, want))

    print()
    for date in [(2026, 3, 21), (2026, 6, 21), (2026, 9, 21), (2026, 12, 21)]:
        state, rise, dusk = solar(day_number(epoch_for(*date)), 0.0, 0.0)
        length = (dusk - rise) / 60.0
        drift = abs(length - EQUATOR_DAY_MINUTES)
        ok = drift <= 3.0
        failures += 0 if ok else 1
        print("%s equator day %04d-%02d-%02d  %.1f min (want %.0f)"
              % ("ok  " if ok else "FAIL", date[0], date[1], date[2],
                 length, EQUATOR_DAY_MINUTES))

    print()
    for name, lat, lon, date, want_state in POLAR:
        state, _, _ = solar(day_number(epoch_for(*date)), lat, lon)
        ok = state == want_state
        failures += 0 if ok else 1
        print("%s %-26s state %d (want %d)"
              % ("ok  " if ok else "FAIL", name, state, want_state))

    print("\nworst drift %.1f min" % worst)
    if failures:
        sys.exit("%d check(s) failed" % failures)
    print("all checks passed")


if __name__ == "__main__":
    main()
