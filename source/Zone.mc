import Toybox.Application;
import Toybox.Lang;
import Toybox.System;

// One clock face reading: either the watch's own timezone or a configured
// alternate.  Everything a frame needs is cached on the instance and only
// refreshed when the minute rolls over.
class Zone {

    // Daylight window used when a zone has no coordinates.
    const FALLBACK_SUNRISE = 6 * 3600;
    const FALLBACK_SUNSET = 18 * 3600;

    // From settings.
    var prefix as String = "";
    var offset as Number = 0;           // seconds east of UTC
    var isLocal as Boolean = false;
    var lat as Float = 0.0;
    var lon as Float = 0.0;
    var hasPosition as Boolean = false;

    // Filled in by Solar.update() once a day.
    var sunrise as Number = 0;
    var sunset as Number = 0;
    var solar as Number = Solar.NORMAL;
    var solarDay as Number = -1;

    // Refreshed once a minute.
    var text as String = "";
    var angle as Float = 0.0;           // degrees clockwise from 12 o'clock
    var isDay as Boolean = true;

    function initialize() {
    }

    // Recompute this zone's reading. `utc` is epoch seconds; `clock` is the
    // watch's own time, which already accounts for the local DST rules.
    function refresh(utc as Number, clock as System.ClockTime, is24Hour as Boolean) as Void {
        var seconds;
        if (isLocal) {
            seconds = clock.hour * 3600 + clock.min * 60;
        } else {
            seconds = (utc + offset) % 86400;
            if (seconds < 0) {
                seconds += 86400;
            }
        }

        var hour = seconds / 3600;
        var minute = (seconds % 3600) / 60;

        text = format(hour, minute, is24Hour);
        angle = (((hour % 12) * 60 + minute) * 0.5).toFloat();
        isDay = daylight(utc, seconds);
    }

    private function format(hour as Number, minute as Number, is24Hour as Boolean) as String {
        if (is24Hour) {
            return hour.format("%02d") + ":" + minute.format("%02d");
        }
        var display = hour % 12;
        if (display == 0) {
            display = 12;
        }
        return display.format("%d") + ":" + minute.format("%02d");
    }

    private function daylight(utc as Number, seconds as Number) as Boolean {
        if (!hasPosition) {
            return seconds >= FALLBACK_SUNRISE && seconds < FALLBACK_SUNSET;
        }
        if (solar == Solar.POLAR_DAY) {
            return true;
        }
        if (solar == Solar.POLAR_NIGHT) {
            return false;
        }
        return utc >= sunrise && utc < sunset;
    }
}

// Reads the zone list out of application properties.
module Config {

    const MAX_ALTERNATES = 3;

    // Characters the bitmap fonts actually contain.
    const ALLOWED_PREFIX = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

    // The watch's own timezone first, then each configured alternate.
    function loadZones() as Array<Zone> {
        var zones = [] as Array<Zone>;

        var home = new Zone();
        home.isLocal = true;
        home.prefix = prefixOf("homePrefix", "H");
        setPosition(home, "homePosition");
        zones.add(home);

        for (var i = 1; i <= MAX_ALTERNATES; i += 1) {
            var prefix = prefixOf("zone" + i + "Prefix", "");
            if (prefix.equals("")) {
                continue;
            }
            var zone = new Zone();
            zone.prefix = prefix;
            zone.offset = (readNumber("zone" + i + "Offset") * 3600).toNumber();
            setPosition(zone, "zone" + i + "Position");
            zones.add(zone);
        }
        return zones;
    }

    // A zone is labelled by a single character, so that it fits on the bezel.
    // Anything the bitmap fonts cannot draw falls back rather than showing a
    // blank marker.
    private function prefixOf(key as String, fallback as String) as String {
        var raw = readString(key);
        if (raw.length() > 0) {
            var first = raw.substring(0, 1);
            if (first != null) {
                var upper = first.toUpper();
                if (ALLOWED_PREFIX.find(upper) != null) {
                    return upper;
                }
            }
        }
        return fallback;
    }

    // Coordinates are entered as "latitude,longitude". Anything else -- an
    // empty field included -- leaves the zone without a position, and it falls
    // back to a fixed daylight window.
    private function setPosition(zone as Zone, key as String) as Void {
        var raw = readString(key);
        var comma = raw.find(",");
        if (comma == null) {
            return;
        }
        var latText = raw.substring(0, comma);
        var lonText = raw.substring(comma + 1, raw.length());
        if (latText == null || lonText == null) {
            return;
        }
        var lat = latText.toFloat();
        var lon = lonText.toFloat();
        if (lat == null || lon == null) {
            return;
        }
        if (lat < -90.0 || lat > 90.0 || lon < -180.0 || lon > 180.0) {
            return;
        }
        // The sunrise equation divides by cos(latitude); keep off the poles.
        if (lat > 89.5) {
            lat = 89.5;
        } else if (lat < -89.5) {
            lat = -89.5;
        }
        zone.lat = lat;
        zone.lon = lon;
        zone.hasPosition = true;
    }

    private function readString(key as String) as String {
        var value = Application.Properties.getValue(key) as String?;
        return (value == null) ? "" : value;
    }

    private function readNumber(key as String) as Float {
        var value = Application.Properties.getValue(key) as Numeric?;
        return (value == null) ? 0.0 : value.toFloat();
    }
}
