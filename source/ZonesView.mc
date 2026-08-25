import Toybox.Graphics;
import Toybox.Lang;
import Toybox.Math;
import Toybox.System;
import Toybox.Time;
import Toybox.WatchUi;

// A watch face built around telling the time in several places at once:
//
//   * the local time, large, in the middle;
//   * up to three alternate timezones in sub-displays above and below it;
//   * the seconds under the time, but only while the display is awake;
//   * every zone marked around the bezel at its own position on a 12 hour
//     dial, green while the sun is up there and red while it is down.
//
// The redraw budget is the thing that costs battery, so everything that can
// only change once a minute -- strings, text positions, marker geometry,
// sunrise times -- is computed in recompute() and cached.  A frame in the
// awake, once-a-second path is nothing but setColor/drawText calls.
class ZonesView extends WatchUi.WatchFace {

    // Layout, as fractions of the screen.  tools/make_fonts.py sizes the
    // bitmap fonts against these same numbers, and tools/preview.py renders
    // them; change one and re-run both.
    const ROW_ABOVE = 0.270;
    const TIME_ROW = 0.448;
    const SEC_ROW = 0.598;
    const ROW_BELOW = 0.712;
    const COL_GAP = 0.038;
    const PREFIX_GAP = 0.030;

    // Bezel marker geometry, as fractions of the screen radius.
    const TIP_R = 0.975;
    const BASE_R = 0.910;
    const CHAR_R = 0.830;
    const HALF_W = 0.028;

    // Two markers closer than this are stacked inwards instead of overlapping.
    const MIN_SEPARATION = 14.0;

    const DEG_TO_RAD = 0.0174532925;

    const COLOR_TIME = 0xFFFFFF;
    const COLOR_ZONE = 0xAAAAAA;
    const COLOR_SECONDS = 0x808080;
    const COLOR_DAY = 0x00AA00;
    const COLOR_NIGHT = 0xFF0000;

    // Always-on displays get a dimmer palette: fewer lit subpixels, less power.
    const DIM_TIME = 0xAAAAAA;
    const DIM_ZONE = 0x555555;
    const DIM_DAY = 0x005500;
    const DIM_NIGHT = 0xAA0000;

    private var _zones as Array<Zone> = [];
    // Untyped: these are null until onLayout() loads them, and every use is
    // downstream of that.
    private var _timeFont;
    private var _zoneFont;

    private var _cx as Number = 0;
    private var _cy as Number = 0;
    private var _radius as Float = 0.0;
    private var _zoneHeight as Number = 0;
    private var _prefixGap as Number = 0;
    private var _columnGap as Number = 0;
    private var _yAbove as Number = 0;
    private var _yTime as Number = 0;
    private var _ySeconds as Number = 0;
    private var _yBelow as Number = 0;

    private var _lowPower as Boolean = false;
    private var _burnInProtection as Boolean = false;
    private var _is24Hour as Boolean = true;

    // -1 forces the next frame to recompute the cache.
    private var _minuteStamp as Number = -1;

    // Cached once a minute, one entry per zone.
    private var _timeText as String = "";
    private var _timeX as Number = 0;
    private var _timeY as Number = 0;
    private var _arrow as Array = [];
    private var _markX as Array = [];
    private var _markY as Array = [];
    private var _markRing as Array = [];
    private var _subPrefixX as Array = [];
    private var _subTimeX as Array = [];
    private var _subY as Array = [];

    function initialize() {
        WatchFace.initialize();
        _zones = Config.loadZones();
        allocate();
    }

    function onLayout(dc as Dc) as Void {
        var width = dc.getWidth();
        var height = dc.getHeight();
        _cx = width / 2;
        _cy = height / 2;
        _radius = ((width < height) ? width : height) / 2.0;

        _timeFont = WatchUi.loadResource(Rez.Fonts.TimeFont) as FontResource;
        _zoneFont = WatchUi.loadResource(Rez.Fonts.ZoneFont) as FontResource;

        // The fonts are generated with a line box that hugs the digits, so
        // half the font height is the distance from a row's centre to its top.
        var timeHeight = dc.getFontHeight(_timeFont);
        _zoneHeight = dc.getFontHeight(_zoneFont);

        _yAbove = (ROW_ABOVE * height - _zoneHeight / 2).toNumber();
        _yTime = (TIME_ROW * height - timeHeight / 2).toNumber();
        _ySeconds = (SEC_ROW * height - _zoneHeight / 2).toNumber();
        _yBelow = (ROW_BELOW * height - _zoneHeight / 2).toNumber();
        _prefixGap = (PREFIX_GAP * width).toNumber();
        _columnGap = (COL_GAP * width).toNumber();

        readDeviceSettings();
        _minuteStamp = -1;
    }

    function onUpdate(dc as Dc) as Void {
        var clock = System.getClockTime();
        var stamp = clock.hour * 60 + clock.min;
        if (stamp != _minuteStamp) {
            _minuteStamp = stamp;
            recompute(dc, clock);
        }

        if (dc has :setAntiAlias) {
            dc.setAntiAlias(true);
        }
        dc.setColor(Graphics.COLOR_BLACK, Graphics.COLOR_BLACK);
        dc.clear();

        drawBezel(dc);
        drawSubDisplays(dc);

        dc.setColor(dimmed() ? DIM_TIME : COLOR_TIME, Graphics.COLOR_TRANSPARENT);
        dc.drawText(_timeX, _timeY, _timeFont, _timeText, Graphics.TEXT_JUSTIFY_CENTER);

        // Seconds belong to the awake display only. Leaving them out of the
        // always-on path is also why this face needs no onPartialUpdate().
        if (!_lowPower) {
            dc.setColor(COLOR_SECONDS, Graphics.COLOR_TRANSPARENT);
            dc.drawText(_cx, _ySeconds, _zoneFont, clock.sec.format("%02d"),
                        Graphics.TEXT_JUSTIFY_CENTER);
        }
    }

    function onEnterSleep() as Void {
        _lowPower = true;
        _minuteStamp = -1;
        WatchUi.requestUpdate();
    }

    function onExitSleep() as Void {
        _lowPower = false;
        _minuteStamp = -1;
        WatchUi.requestUpdate();
    }

    function onSettingsChanged() as Void {
        _zones = Config.loadZones();
        allocate();
        readDeviceSettings();
        _minuteStamp = -1;
    }

    // --- cache -----------------------------------------------------------

    // Called at most once a minute, and on any state change that alters the
    // layout. Does all of the arithmetic so that onUpdate() does none.
    private function recompute(dc as Dc, clock as System.ClockTime) as Void {
        var utc = Time.now().value();
        var day = Solar.dayNumber(utc);

        // Always-on displays shift the whole face a couple of pixels each
        // minute so that nothing is burnt into the panel.
        var offsetX = 0;
        var offsetY = 0;
        if (_burnInProtection && _lowPower) {
            var phase = _minuteStamp % 4;
            offsetX = (phase == 1) ? 2 : ((phase == 3) ? -2 : 0);
            offsetY = (phase == 2) ? 2 : ((phase == 0) ? -2 : 0);
        }

        for (var i = 0; i < _zones.size(); i += 1) {
            var zone = _zones[i];
            if (zone.hasPosition && zone.solarDay != day) {
                Solar.update(zone, day);
                zone.solarDay = day;
            }
            zone.refresh(utc, clock, _is24Hour);
        }

        _timeText = _zones[0].text;
        _timeX = _cx + offsetX;
        _timeY = _yTime + offsetY;

        layoutBezel(offsetX, offsetY);
        layoutSubDisplays(dc, offsetX, offsetY);
    }

    // One marker per zone, at its own reading on a 12 hour dial.
    private function layoutBezel(offsetX as Number, offsetY as Number) as Void {
        for (var i = 0; i < _zones.size(); i += 1) {
            var zone = _zones[i];

            // Zones showing near enough the same time land on the same spot;
            // step their letters inwards so each one stays readable.
            var ring = 0;
            var clash = true;
            while (clash) {
                clash = false;
                for (var j = 0; j < i; j += 1) {
                    if (_markRing[j] == ring
                            && separation(zone.angle, _zones[j].angle) < MIN_SEPARATION) {
                        ring += 1;
                        clash = true;
                        break;
                    }
                }
            }
            _markRing[i] = ring;

            var radians = zone.angle * DEG_TO_RAD;
            var sin = Math.sin(radians);
            var cos = Math.cos(radians);

            // An arrow pointing out at the rim: apex outwards, base inwards.
            var baseX = _cx + BASE_R * _radius * sin + offsetX;
            var baseY = _cy - BASE_R * _radius * cos + offsetY;
            var half = HALF_W * _radius;
            var arrow = _arrow[i];
            arrow[0][0] = (_cx + TIP_R * _radius * sin + offsetX).toNumber();
            arrow[0][1] = (_cy - TIP_R * _radius * cos + offsetY).toNumber();
            arrow[1][0] = (baseX + half * cos).toNumber();
            arrow[1][1] = (baseY + half * sin).toNumber();
            arrow[2][0] = (baseX - half * cos).toNumber();
            arrow[2][1] = (baseY - half * sin).toNumber();

            var charR = CHAR_R * _radius - ring * (_zoneHeight + 2);
            _markX[i] = (_cx + charR * sin + offsetX).toNumber();
            _markY[i] = (_cy - charR * cos - _zoneHeight / 2 + offsetY).toNumber();
        }
    }

    // The first alternate sits above the time; the next two share the row
    // below it.
    private function layoutSubDisplays(dc as Dc, offsetX as Number, offsetY as Number) as Void {
        var count = _zones.size();
        if (count > 1) {
            placeSub(dc, 1, _cx + offsetX, _yAbove + offsetY);
        }
        if (count == 3) {
            placeSub(dc, 2, _cx + offsetX, _yBelow + offsetY);
        } else if (count > 3) {
            var widest = subWidth(dc, 2);
            var other = subWidth(dc, 3);
            if (other > widest) {
                widest = other;
            }
            var dx = (widest + _columnGap) / 2;
            placeSub(dc, 2, _cx - dx + offsetX, _yBelow + offsetY);
            placeSub(dc, 3, _cx + dx + offsetX, _yBelow + offsetY);
        }
    }

    private function subWidth(dc as Dc, index as Number) as Number {
        var zone = _zones[index];
        return dc.getTextWidthInPixels(zone.prefix, _zoneFont) + _prefixGap
             + dc.getTextWidthInPixels(zone.text, _zoneFont);
    }

    private function placeSub(dc as Dc, index as Number, centreX as Number, y as Number) as Void {
        var zone = _zones[index];
        var prefixWidth = dc.getTextWidthInPixels(zone.prefix, _zoneFont);
        var total = prefixWidth + _prefixGap + dc.getTextWidthInPixels(zone.text, _zoneFont);
        _subPrefixX[index] = centreX - total / 2;
        _subTimeX[index] = _subPrefixX[index] + prefixWidth + _prefixGap;
        _subY[index] = y;
    }

    // --- drawing ---------------------------------------------------------

    private function drawBezel(dc as Dc) as Void {
        for (var i = 0; i < _zones.size(); i += 1) {
            var zone = _zones[i];
            dc.setColor(dayColor(zone.isDay), Graphics.COLOR_TRANSPARENT);
            dc.fillPolygon(_arrow[i]);
            dc.drawText(_markX[i], _markY[i], _zoneFont, zone.prefix,
                        Graphics.TEXT_JUSTIFY_CENTER);
        }
    }

    private function drawSubDisplays(dc as Dc) as Void {
        var zoneColor = dimmed() ? DIM_ZONE : COLOR_ZONE;
        for (var i = 1; i < _zones.size(); i += 1) {
            var zone = _zones[i];
            dc.setColor(dayColor(zone.isDay), Graphics.COLOR_TRANSPARENT);
            dc.drawText(_subPrefixX[i], _subY[i], _zoneFont, zone.prefix,
                        Graphics.TEXT_JUSTIFY_LEFT);
            dc.setColor(zoneColor, Graphics.COLOR_TRANSPARENT);
            dc.drawText(_subTimeX[i], _subY[i], _zoneFont, zone.text,
                        Graphics.TEXT_JUSTIFY_LEFT);
        }
    }

    // --- helpers ---------------------------------------------------------

    private function dimmed() as Boolean {
        return _lowPower && _burnInProtection;
    }

    private function dayColor(isDay as Boolean) as Number {
        if (dimmed()) {
            return isDay ? DIM_DAY : DIM_NIGHT;
        }
        return isDay ? COLOR_DAY : COLOR_NIGHT;
    }

    private function separation(a as Float, b as Float) as Float {
        var d = a - b;
        if (d < 0) {
            d = -d;
        }
        return (d > 180.0) ? 360.0 - d : d;
    }

    private function readDeviceSettings() as Void {
        var settings = System.getDeviceSettings();
        _is24Hour = settings.is24Hour;
        _burnInProtection = (settings has :requiresBurnInProtection)
                         && settings.requiresBurnInProtection;
    }

    // Per-zone scratch space, allocated when the zone list changes so that
    // nothing allocates while drawing.
    private function allocate() as Void {
        var count = _zones.size();
        _arrow = new [count];
        for (var i = 0; i < count; i += 1) {
            _arrow[i] = [[0, 0], [0, 0], [0, 0]];
        }
        _markX = new [count];
        _markY = new [count];
        _markRing = new [count];
        _subPrefixX = new [count];
        _subTimeX = new [count];
        _subY = new [count];
    }
}
