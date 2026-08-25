import Toybox.Application;
import Toybox.Lang;
import Toybox.WatchUi;

class ZonesApp extends Application.AppBase {

    private var _view as ZonesView?;

    function initialize() {
        AppBase.initialize();
    }

    // Deliberately unannotated: the SDK's declared return type for this has
    // changed shape across releases.
    function getInitialView() {
        _view = new ZonesView();
        return [_view];
    }

    function onSettingsChanged() as Void {
        var view = _view;
        if (view != null) {
            view.onSettingsChanged();
        }
        WatchUi.requestUpdate();
    }
}
