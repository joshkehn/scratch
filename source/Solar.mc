import Toybox.Lang;
import Toybox.Math;

// Sunrise and sunset from the low precision solar equations
// (https://en.wikipedia.org/wiki/Sunrise_equation).  Good to about a minute,
// which is far more than a coloured letter needs, and it runs once per zone
// per day rather than once per frame.
//
// Times are carried as "days since J2000 noon" instead of Julian dates so
// every intermediate stays small: Monkey C floats are single precision, and a
// Julian date would burn the whole mantissa on the integer part.
module Solar {

    const DEG_TO_RAD = 0.0174532925;
    const RAD_TO_DEG = 57.2957795;

    // 2000-01-01T12:00:00Z as epoch seconds.
    const J2000_EPOCH = 946728000;

    // sin(23.4397 deg) -- the obliquity of the ecliptic.
    const SIN_OBLIQUITY = 0.39779;

    // sin(-0.833 deg) -- the sun's apparent radius plus atmospheric
    // refraction, i.e. the altitude that counts as "risen".
    const SIN_HORIZON = -0.01454;

    // Values for Zone.solar.
    const NORMAL = 0;
    const POLAR_DAY = 1;
    const POLAR_NIGHT = 2;

    // Whole days since J2000 noon containing `epoch`.
    function dayNumber(epoch as Number) as Number {
        return Math.round((epoch - J2000_EPOCH) / 86400.0 + 0.0008).toNumber();
    }

    // Fill in `zone.sunrise` and `zone.sunset` (epoch seconds) for `day`.
    function update(zone as Zone, day as Number) as Void {
        // Solar day for this longitude, in days since J2000 noon.
        var jStar = day - zone.lon / 360.0;

        var meanDegrees = norm360(357.5291 + 0.98560028 * jStar);
        var mean = meanDegrees * DEG_TO_RAD;
        var centre = 1.9148 * Math.sin(mean)
                   + 0.0200 * Math.sin(2 * mean)
                   + 0.0003 * Math.sin(3 * mean);
        // 282.9372 = 180 + the argument of perihelion.
        var lambda = norm360(meanDegrees + centre + 282.9372) * DEG_TO_RAD;

        // Solar noon, as a fraction of a day either side of `day`.
        var transit = -zone.lon / 360.0
                    + 0.0053 * Math.sin(mean)
                    - 0.0069 * Math.sin(2 * lambda);

        var sinDecl = Math.sin(lambda) * SIN_OBLIQUITY;
        var cosDecl = Math.sqrt(1.0 - sinDecl * sinDecl);
        var lat = zone.lat * DEG_TO_RAD;
        var hourAngle = (SIN_HORIZON - Math.sin(lat) * sinDecl) / (Math.cos(lat) * cosDecl);

        if (hourAngle >= 1.0) {
            zone.solar = POLAR_NIGHT;
            return;
        }
        if (hourAngle <= -1.0) {
            zone.solar = POLAR_DAY;
            return;
        }

        var half = Math.acos(hourAngle) * RAD_TO_DEG / 360.0;
        var noon = J2000_EPOCH + day * 86400;
        zone.sunrise = noon + ((transit - half) * 86400.0).toNumber();
        zone.sunset = noon + ((transit + half) * 86400.0).toNumber();
        zone.solar = NORMAL;
    }

    function norm360(degrees as Numeric) as Float {
        var d = degrees - Math.floor(degrees / 360.0) * 360.0;
        return d.toFloat();
    }
}
